//! gforce-node daemon — long-running background process.
//!
//! v1 scope: enrol (once, via the CLI) then POST a heartbeat every
//! `heartbeat_interval_seconds` to keep the server's liveness tracker
//! fresh. The old WebSocket / command-dispatch pipeline is intentionally
//! dormant; the server does not ship a command-dispatch surface yet and
//! re-enabling it under a feature flag is a follow-up.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use node_core::config::NodeConfig;
use node_core::heartbeat::run_heartbeat_loop;

#[derive(Parser)]
#[command(name = "gforce-node-daemon", about = "Gforce Node Agent Daemon")]
struct Args {
    /// Path to config file (default: ~/.gforce-node/config.toml).
    #[arg(long)]
    config: Option<PathBuf>,

    /// Log level (trace, debug, info, warn, error).
    #[arg(long, default_value = "info")]
    log_level: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| args.log_level.parse().unwrap_or_default()),
        )
        .json()
        .init();

    let config = if let Some(path) = &args.config {
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        toml::from_str::<NodeConfig>(&contents)?
    } else {
        NodeConfig::load()?
    };

    tracing::info!(
        server = %config.server,
        interval = config.heartbeat_interval_seconds,
        "gforce-node daemon starting"
    );

    // Ensure workspace root exists so the (future) executor can write to it.
    let workspace_root = PathBuf::from(&config.workspace_root);
    if !workspace_root.exists() {
        std::fs::create_dir_all(&workspace_root).with_context(|| {
            format!("creating workspace root {}", workspace_root.display())
        })?;
    }

    // Block on the heartbeat loop until the server permanently rejects us
    // (401) or the process is signalled to stop.
    run_heartbeat_loop(config).await
}
