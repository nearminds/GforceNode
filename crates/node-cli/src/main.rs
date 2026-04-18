//! gforce-node CLI — user-facing commands for enrollment and management.
//!
//! Usage:
//!   gforce-node register --token <TOKEN> --server gforce.nearminds.org
//!   gforce-node install
//!   gforce-node status
//!   gforce-node logs
//!   gforce-node uninstall
//!
//! All three supported OSes (Linux, macOS, Windows) share the same
//! commands — the service module picks the right backend (systemd /
//! launchd / Windows SCM).

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use node_core::config::{config_dir, NodeConfig};
use node_core::service;

#[derive(Parser)]
#[command(
    name = "gforce-node",
    about = "Gforce Node Agent — connect your machine to Gforce",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Enrol this machine with a Gforce team using a one-time token.
    Register {
        /// One-time enrollment token from the Gforce UI.
        ///
        /// Can also be supplied via the TOKEN env var so the installer
        /// can run as a single `curl … | TOKEN=… sh` command.
        #[arg(long, env = "TOKEN")]
        token: String,

        /// Gforce server hostname (e.g. gforce.nearminds.org).
        #[arg(long, env = "GFORCE_SERVER", default_value = "gforce.nearminds.org")]
        server: String,

        /// Disable TLS (local dev only).
        #[arg(long, default_value = "false")]
        no_tls: bool,
    },

    /// Show agent status and configuration.
    Status,

    /// Show recent daemon logs.
    Logs {
        /// Number of lines to show.
        #[arg(short, long, default_value = "50")]
        lines: usize,
    },

    /// Start the daemon in the foreground (for testing).
    Run,

    /// Install as a system service (systemd / launchd / Windows Service).
    Install {
        /// Override the daemon binary path.
        #[arg(long)]
        daemon: Option<std::path::PathBuf>,
    },

    /// Uninstall the system service and remove config.
    Uninstall {
        /// Also remove the workspace directory.
        #[arg(long)]
        remove_workspaces: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Register {
            token,
            server,
            no_tls,
        } => cmd_register(&server, &token, !no_tls).await?,

        Commands::Status => cmd_status(),

        Commands::Logs { lines } => cmd_logs(lines)?,

        Commands::Run => cmd_run()?,

        Commands::Install { daemon } => cmd_install(daemon)?,

        Commands::Uninstall { remove_workspaces } => cmd_uninstall(remove_workspaces)?,
    }

    Ok(())
}

// ── Commands ──────────────────────────────────────────────────────────────

async fn cmd_register(server: &str, token: &str, use_tls: bool) -> Result<()> {
    println!("Enrolling with Gforce server: {server}");

    let sys_info = node_executor::system::collect_system_info();
    let config = node_core::auth::register_node(server, token, sys_info, use_tls).await?;

    println!("Enrolment successful!");
    println!("  Server:    {}", config.server);
    println!("  Node id:   {}", config.node_id.as_deref().unwrap_or("—"));
    println!(
        "  Config:    {}",
        config_dir().join("config.toml").display()
    );
    println!();
    println!("Next step:");
    println!("  gforce-node install   # install as a background service");
    Ok(())
}

fn cmd_status() {
    match NodeConfig::load() {
        Ok(config) => {
            println!("Gforce Node Agent");
            println!("  Server:       {}", config.server);
            println!(
                "  Node id:      {}",
                config.node_id.as_deref().unwrap_or("—")
            );
            println!("  Workspace:    {}", config.workspace_root);
            println!(
                "  TLS:          {}",
                if config.use_tls { "yes" } else { "no" }
            );
            println!("  Heartbeat:    {}s", config.heartbeat_interval_seconds);
            match service::status() {
                Ok(s) => println!("  Service:      {s}"),
                Err(e) => println!("  Service:      (unknown — {e})"),
            }
        }
        Err(e) => {
            println!("Not enrolled. Run 'gforce-node register' first.");
            println!("  Error: {e}");
        }
    }
}

fn cmd_logs(lines: usize) -> Result<()> {
    let log_path = config_dir().join("daemon.log");
    if log_path.exists() {
        let content = std::fs::read_to_string(&log_path)?;
        let all: Vec<&str> = content.lines().collect();
        let start = all.len().saturating_sub(lines);
        for line in &all[start..] {
            println!("{line}");
        }
    } else {
        println!("No logs found at {}", log_path.display());
        println!(
            "Use your platform's service logs instead: \
            `journalctl -u gforce-node -f` on Linux, \
            `log show --predicate 'process == \"gforce-node-daemon\"'` on macOS, \
            or the Event Viewer on Windows."
        );
    }
    Ok(())
}

fn cmd_run() -> Result<()> {
    println!("Starting daemon in foreground mode...");
    println!("Press Ctrl+C to stop.");

    let status = std::process::Command::new("gforce-node-daemon")
        .status()
        .context("Failed to start daemon. Is gforce-node-daemon in PATH?")?;
    std::process::exit(status.code().unwrap_or(1));
}

fn cmd_install(daemon: Option<std::path::PathBuf>) -> Result<()> {
    let path = daemon.unwrap_or_else(service::default_daemon_path);
    if !path.exists() {
        eprintln!(
            "warning: daemon binary {} does not exist on disk; \
             service will be registered but will not start until the \
             binary is installed there.",
            path.display()
        );
    }
    service::install(&path)?;
    println!("Service installed and started.");
    println!("  Daemon: {}", path.display());
    Ok(())
}

fn cmd_uninstall(remove_workspaces: bool) -> Result<()> {
    service::uninstall()?;

    if remove_workspaces {
        if let Ok(config) = NodeConfig::load() {
            let ws = std::path::PathBuf::from(&config.workspace_root);
            if ws.exists() {
                println!("Removing workspace: {}", ws.display());
                std::fs::remove_dir_all(&ws)?;
            }
        }
    }

    let cfg_dir = config_dir();
    if cfg_dir.exists() {
        println!("Removing config: {}", cfg_dir.display());
        std::fs::remove_dir_all(&cfg_dir)?;
    }

    println!("Uninstalled successfully.");
    Ok(())
}
