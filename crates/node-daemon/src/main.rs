//! gforce-node daemon — connects to GForce and executes commands.
//!
//! This is the long-running background process installed as a system service.
//! It maintains a WebSocket connection to the GForce server and handles
//! incoming commands by dispatching them to the node-executor crate.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clap::Parser;
use node_core::commands::{OutputMessage, ResultMessage};
use node_core::config::NodeConfig;
use node_core::connection::{run_connection, ConnectionEvent, OutboundMessage};
use node_core::heartbeat::run_heartbeat;
use node_executor::files;
use node_executor::shell;
use node_executor::system;
use tokio::sync::mpsc;

#[derive(Parser)]
#[command(name = "gforce-node-daemon", about = "GForce Node Agent Daemon")]
struct Args {
    /// Path to config file (default: ~/.gforce-node/config.toml)
    #[arg(long)]
    config: Option<PathBuf>,

    /// Log level (trace, debug, info, warn, error)
    #[arg(long, default_value = "info")]
    log_level: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| args.log_level.parse().unwrap_or_default()),
        )
        .json()
        .init();

    // Load config
    let config = if let Some(path) = &args.config {
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        toml::from_str::<NodeConfig>(&contents)?
    } else {
        NodeConfig::load()?
    };

    tracing::info!(server = %config.server, "Starting gforce-node daemon");

    // Collect system info for initial auth
    let sys_info = system::collect_system_info();

    // Set up channels
    let (event_tx, mut event_rx) = mpsc::channel::<ConnectionEvent>(64);
    let (outbound_tx, outbound_rx) = mpsc::channel::<OutboundMessage>(256);

    // Active command counter
    let active_commands = Arc::new(AtomicUsize::new(0));
    let start_time = Instant::now();

    // Ensure workspace root exists
    let workspace_root = PathBuf::from(&config.workspace_root);
    std::fs::create_dir_all(&workspace_root)?;

    // Spawn connection loop
    let config_clone = config.clone();
    let sys_info_clone = sys_info.clone();
    tokio::spawn(async move {
        run_connection(&config_clone, sys_info_clone, event_tx, outbound_rx).await;
    });

    // Spawn heartbeat loop
    let hb_tx = outbound_tx.clone();
    let hb_active = active_commands.clone();
    tokio::spawn(async move {
        run_heartbeat(
            Duration::from_secs(config.heartbeat_interval_seconds),
            start_time,
            hb_active,
            hb_tx,
        )
        .await;
    });

    // Main event loop: handle commands from the server
    tracing::info!("Daemon running. Waiting for commands...");

    while let Some(event) = event_rx.recv().await {
        match event {
            ConnectionEvent::Connected => {
                tracing::info!("Connected to GForce server");
            }
            ConnectionEvent::Disconnected(reason) => {
                tracing::warn!(reason = %reason, "Disconnected from server");
            }
            ConnectionEvent::Command(cmd) => {
                let cmd_tx = outbound_tx.clone();
                let ac = active_commands.clone();
                let ws_root = workspace_root.clone();
                let allowed = config.allowed_commands.clone();
                let unrestricted = config.unrestricted_mode;

                tokio::spawn(async move {
                    ac.fetch_add(1, Ordering::Relaxed);
                    let result = handle_command(
                        &cmd.command_id,
                        &cmd.action,
                        &cmd.payload,
                        cmd.timeout_seconds,
                        &ws_root,
                        &allowed,
                        unrestricted,
                        &cmd_tx,
                    )
                    .await;
                    ac.fetch_sub(1, Ordering::Relaxed);

                    // Send result
                    let result_msg = match result {
                        Ok(msg) => msg,
                        Err(e) => ResultMessage::error(&cmd.command_id, &e.to_string()),
                    };

                    let json = serde_json::to_value(&result_msg).unwrap();
                    let _ = cmd_tx.send(OutboundMessage::Json(json)).await;
                });
            }
        }
    }

    Ok(())
}

async fn handle_command(
    command_id: &str,
    action: &str,
    payload: &serde_json::Value,
    timeout_seconds: u64,
    workspace_root: &PathBuf,
    allowed_commands: &[String],
    unrestricted: bool,
    outbound_tx: &mpsc::Sender<OutboundMessage>,
) -> Result<ResultMessage> {
    let start = Instant::now();

    match action {
        "shell_exec" => {
            let cmd = payload
                .get("command")
                .and_then(|v| v.as_str())
                .context("Missing 'command' in payload")?;

            if !shell::is_command_allowed(cmd, allowed_commands, unrestricted) {
                return Ok(ResultMessage::error(
                    command_id,
                    &format!("Command not allowed: {cmd}. Enable unrestricted_mode in config to allow all commands."),
                ));
            }

            let cwd = payload
                .get("cwd")
                .and_then(|v| v.as_str())
                .map(PathBuf::from);
            let cwd_path = cwd.as_deref().unwrap_or(workspace_root.as_path());

            // Set up streaming output
            let (output_tx, mut output_rx) = mpsc::channel(256);
            let cmd_id = command_id.to_string();
            let ob_tx = outbound_tx.clone();

            // Forward output lines to the server
            tokio::spawn(async move {
                while let Some(line) = output_rx.recv().await {
                    let msg = if line.stream == "stdout" {
                        OutputMessage::stdout(&cmd_id, line.line)
                    } else {
                        OutputMessage::stderr(&cmd_id, line.line)
                    };
                    let json = serde_json::to_value(&msg).unwrap();
                    let _ = ob_tx.send(OutboundMessage::Json(json)).await;
                }
            });

            let result = shell::run_command(cmd, Some(cwd_path), output_tx, timeout_seconds).await?;
            let duration = start.elapsed().as_millis() as u64;

            if result.exit_code == 0 {
                Ok(ResultMessage::ok(command_id, duration))
            } else {
                Ok(ResultMessage::fail(command_id, result.exit_code, duration))
            }
        }

        "git_clone" => {
            let url = payload
                .get("url")
                .and_then(|v| v.as_str())
                .context("Missing 'url' in payload")?;
            let path = payload
                .get("path")
                .and_then(|v| v.as_str())
                .map(|p| workspace_root.join(p))
                .unwrap_or_else(|| {
                    let repo_name = url.rsplit('/').next().unwrap_or("repo");
                    let repo_name = repo_name.strip_suffix(".git").unwrap_or(repo_name);
                    workspace_root.join(repo_name)
                });
            let branch = payload.get("branch").and_then(|v| v.as_str());

            let (output_tx, mut output_rx) = mpsc::channel(256);
            let cmd_id = command_id.to_string();
            let ob_tx = outbound_tx.clone();
            tokio::spawn(async move {
                while let Some(line) = output_rx.recv().await {
                    let msg = OutputMessage::stdout(&cmd_id, line.line);
                    let json = serde_json::to_value(&msg).unwrap();
                    let _ = ob_tx.send(OutboundMessage::Json(json)).await;
                }
            });

            let result = node_executor::git::clone(url, &path, branch, output_tx).await?;
            let duration = start.elapsed().as_millis() as u64;

            if result.exit_code == 0 {
                Ok(ResultMessage::ok(command_id, duration))
            } else {
                Ok(ResultMessage::fail(command_id, result.exit_code, duration))
            }
        }

        "file_read" => {
            let path = payload
                .get("path")
                .and_then(|v| v.as_str())
                .context("Missing 'path' in payload")?;
            let content = files::read_file(path, workspace_root)?;
            let duration = start.elapsed().as_millis() as u64;
            Ok(ResultMessage::ok_with_data(
                command_id,
                duration,
                serde_json::json!({ "content": content }),
            ))
        }

        "file_write" => {
            let path = payload
                .get("path")
                .and_then(|v| v.as_str())
                .context("Missing 'path' in payload")?;
            let content = payload
                .get("content")
                .and_then(|v| v.as_str())
                .context("Missing 'content' in payload")?;
            files::write_file(path, content, workspace_root)?;
            let duration = start.elapsed().as_millis() as u64;
            Ok(ResultMessage::ok(command_id, duration))
        }

        "file_list" => {
            let path = payload
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or(".");
            let entries = files::list_files(path, workspace_root)?;
            let duration = start.elapsed().as_millis() as u64;
            Ok(ResultMessage::ok_with_data(
                command_id,
                duration,
                serde_json::json!({ "entries": entries }),
            ))
        }

        "system_info" => {
            let info = system::collect_system_info();
            let duration = start.elapsed().as_millis() as u64;
            Ok(ResultMessage::ok_with_data(command_id, duration, info))
        }

        _ => Ok(ResultMessage::error(
            command_id,
            &format!("Unknown action: {action}"),
        )),
    }
}
