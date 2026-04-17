//! Git operations (clone, checkout, pull).

use std::path::Path;

use anyhow::Result;
use tokio::sync::mpsc;

use crate::shell::{run_command, CommandResult, OutputLine};

/// Clone a git repository.
pub async fn clone(
    url: &str,
    target_path: &Path,
    branch: Option<&str>,
    output_tx: mpsc::Sender<OutputLine>,
) -> Result<CommandResult> {
    let mut cmd = format!("git clone {url} {}", target_path.display());
    if let Some(b) = branch {
        cmd = format!("git clone -b {b} {url} {}", target_path.display());
    }
    run_command(&cmd, None, output_tx, 600).await
}

/// Pull latest changes in a repository.
pub async fn pull(
    repo_path: &Path,
    output_tx: mpsc::Sender<OutputLine>,
) -> Result<CommandResult> {
    run_command("git pull", Some(repo_path), output_tx, 120).await
}

/// Checkout a branch.
pub async fn checkout(
    repo_path: &Path,
    branch: &str,
    output_tx: mpsc::Sender<OutputLine>,
) -> Result<CommandResult> {
    run_command(
        &format!("git checkout {branch}"),
        Some(repo_path),
        output_tx,
        30,
    )
    .await
}
