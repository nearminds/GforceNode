//! gforce-node CLI — user-facing commands for registration and management.
//!
//! Usage:
//!   gforce-node register --token <TOKEN> --server gforce.nearminds.org
//!   gforce-node status
//!   gforce-node logs
//!   gforce-node uninstall

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use node_core::config::{config_dir, NodeConfig};

#[derive(Parser)]
#[command(
    name = "gforce-node",
    about = "GForce Node Agent — connect your machine to GForce",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Register this machine with a GForce team
    Register {
        /// One-time registration token from GForce UI
        #[arg(long)]
        token: String,

        /// GForce server hostname (e.g., gforce.nearminds.org)
        #[arg(long)]
        server: String,

        /// Disable TLS (for local development only)
        #[arg(long, default_value = "false")]
        no_tls: bool,
    },

    /// Show agent status and configuration
    Status,

    /// Show recent logs
    Logs {
        /// Number of lines to show
        #[arg(short, long, default_value = "50")]
        lines: usize,
    },

    /// Start the daemon in the foreground (for testing)
    Run,

    /// Install as a system service (launchd on macOS, systemd on Linux)
    Install,

    /// Uninstall the system service and remove config
    Uninstall {
        /// Also remove the workspace directory
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
        } => {
            println!("Registering with GForce server: {server}");

            let sys_info = node_executor::system::collect_system_info();
            let config =
                node_core::auth::register_node(&server, &token, sys_info, !no_tls).await?;

            println!("Registration successful!");
            println!("  Server:    {}", config.server);
            println!("  Config:    {}", config_dir().join("config.toml").display());
            println!("  Workspace: {}", config.workspace_root);
            println!();
            println!("Next steps:");
            println!("  gforce-node install   # Install as system service");
            println!("  gforce-node run       # Or run in foreground for testing");
        }

        Commands::Status => {
            match NodeConfig::load() {
                Ok(config) => {
                    println!("GForce Node Agent");
                    println!("  Server:       {}", config.server);
                    println!("  Workspace:    {}", config.workspace_root);
                    println!("  TLS:          {}", if config.use_tls { "yes" } else { "no" });
                    println!("  Unrestricted: {}", if config.unrestricted_mode { "yes" } else { "no" });
                    println!("  Heartbeat:    {}s", config.heartbeat_interval_seconds);
                    println!("  Allowed cmds: {}", config.allowed_commands.join(", "));

                    // Check if daemon is running (macOS/Linux)
                    #[cfg(target_os = "macos")]
                    {
                        let output = std::process::Command::new("launchctl")
                            .args(["list", "org.nearminds.gforce-node"])
                            .output();
                        match output {
                            Ok(o) if o.status.success() => println!("  Service:      running"),
                            _ => println!("  Service:      not running"),
                        }
                    }
                    #[cfg(target_os = "linux")]
                    {
                        let output = std::process::Command::new("systemctl")
                            .args(["--user", "is-active", "gforce-node"])
                            .output();
                        match output {
                            Ok(o) if o.status.success() => println!("  Service:      running"),
                            _ => println!("  Service:      not running"),
                        }
                    }
                }
                Err(e) => {
                    println!("Not registered. Run 'gforce-node register' first.");
                    println!("  Error: {e}");
                }
            }
        }

        Commands::Logs { lines } => {
            let log_path = config_dir().join("daemon.log");
            if log_path.exists() {
                let content = std::fs::read_to_string(&log_path)?;
                let all_lines: Vec<&str> = content.lines().collect();
                let start = all_lines.len().saturating_sub(lines);
                for line in &all_lines[start..] {
                    println!("{line}");
                }
            } else {
                println!("No logs found at {}", log_path.display());
            }
        }

        Commands::Run => {
            // Run the daemon in the foreground (delegates to node-daemon binary)
            println!("Starting daemon in foreground mode...");
            println!("Press Ctrl+C to stop.");

            let status = std::process::Command::new("gforce-node-daemon")
                .status()
                .context("Failed to start daemon. Is gforce-node-daemon in PATH?")?;

            std::process::exit(status.code().unwrap_or(1));
        }

        Commands::Install => {
            install_service()?;
        }

        Commands::Uninstall { remove_workspaces } => {
            uninstall_service()?;

            if remove_workspaces {
                if let Ok(config) = NodeConfig::load() {
                    let ws = std::path::PathBuf::from(&config.workspace_root);
                    if ws.exists() {
                        println!("Removing workspace: {}", ws.display());
                        std::fs::remove_dir_all(&ws)?;
                    }
                }
            }

            // Remove config directory
            let cfg_dir = config_dir();
            if cfg_dir.exists() {
                println!("Removing config: {}", cfg_dir.display());
                std::fs::remove_dir_all(&cfg_dir)?;
            }

            println!("Uninstalled successfully.");
        }
    }

    Ok(())
}

#[cfg(target_os = "macos")]
fn install_service() -> Result<()> {
    let plist_path = dirs_home()
        .join("Library/LaunchAgents/org.nearminds.gforce-node.plist");

    let daemon_path = which::which("gforce-node-daemon")
        .unwrap_or_else(|_| std::path::PathBuf::from("/usr/local/bin/gforce-node-daemon"));

    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>org.nearminds.gforce-node</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{}/daemon.log</string>
    <key>StandardErrorPath</key>
    <string>{}/daemon.log</string>
</dict>
</plist>"#,
        daemon_path.display(),
        config_dir().display(),
        config_dir().display(),
    );

    if let Some(parent) = plist_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&plist_path, plist)?;

    std::process::Command::new("launchctl")
        .args(["load", &plist_path.to_string_lossy()])
        .status()?;

    println!("Service installed and started.");
    println!("  Plist: {}", plist_path.display());
    println!("  Logs:  {}/daemon.log", config_dir().display());
    Ok(())
}

#[cfg(target_os = "linux")]
fn install_service() -> Result<()> {
    let service_dir = dirs_home().join(".config/systemd/user");
    std::fs::create_dir_all(&service_dir)?;

    let service_path = service_dir.join("gforce-node.service");

    let daemon_path = which::which("gforce-node-daemon")
        .unwrap_or_else(|_| std::path::PathBuf::from("/usr/local/bin/gforce-node-daemon"));

    let unit = format!(
        r#"[Unit]
Description=GForce Node Agent
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart={}
Restart=always
RestartSec=10

[Install]
WantedBy=default.target
"#,
        daemon_path.display()
    );

    std::fs::write(&service_path, unit)?;

    std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status()?;
    std::process::Command::new("systemctl")
        .args(["--user", "enable", "--now", "gforce-node"])
        .status()?;

    println!("Service installed and started.");
    println!("  Unit:  {}", service_path.display());
    println!("  Logs:  journalctl --user -u gforce-node -f");
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn install_service() -> Result<()> {
    println!("Automatic service installation is not supported on this platform.");
    println!("Run 'gforce-node run' to start the daemon manually.");
    Ok(())
}

fn uninstall_service() -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let plist = dirs_home().join("Library/LaunchAgents/org.nearminds.gforce-node.plist");
        if plist.exists() {
            let _ = std::process::Command::new("launchctl")
                .args(["unload", &plist.to_string_lossy()])
                .status();
            std::fs::remove_file(&plist)?;
        }
    }

    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("systemctl")
            .args(["--user", "disable", "--now", "gforce-node"])
            .status();
        let service = dirs_home().join(".config/systemd/user/gforce-node.service");
        if service.exists() {
            std::fs::remove_file(&service)?;
        }
    }

    Ok(())
}

fn dirs_home() -> std::path::PathBuf {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
}
