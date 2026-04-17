//! Sandboxed shell command execution with streaming output.

use std::path::Path;
use std::process::Stdio;
use std::time::Instant;

use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

/// Output line from a running command.
#[derive(Debug)]
pub struct OutputLine {
    pub stream: &'static str, // "stdout" or "stderr"
    pub line: String,
}

/// Result of a completed command.
#[derive(Debug)]
pub struct CommandResult {
    pub exit_code: i32,
    pub duration_ms: u64,
}

/// Check if a command is allowed based on the allowlist.
pub fn is_command_allowed(command: &str, allowed_prefixes: &[String], unrestricted: bool) -> bool {
    if unrestricted {
        return true;
    }
    let first_word = command.split_whitespace().next().unwrap_or("");
    // Also check the basename (e.g., /usr/bin/git -> git)
    let basename = first_word.rsplit('/').next().unwrap_or(first_word);
    allowed_prefixes
        .iter()
        .any(|prefix| basename == prefix || first_word.starts_with(prefix))
}

/// Execute a shell command with streaming output.
///
/// Lines are sent to `output_tx` as they are produced. The function
/// returns the exit code and duration when the command completes.
pub async fn run_command(
    command: &str,
    cwd: Option<&Path>,
    output_tx: mpsc::Sender<OutputLine>,
    timeout_seconds: u64,
) -> Result<CommandResult> {
    let start = Instant::now();

    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(cwd.unwrap_or_else(|| Path::new(".")))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("Failed to spawn command")?;

    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");

    let out_tx = output_tx.clone();
    let stdout_handle = tokio::spawn(async move {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = out_tx
                .send(OutputLine {
                    stream: "stdout",
                    line,
                })
                .await;
        }
    });

    let err_tx = output_tx;
    let stderr_handle = tokio::spawn(async move {
        let reader = BufReader::new(stderr);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = err_tx
                .send(OutputLine {
                    stream: "stderr",
                    line,
                })
                .await;
        }
    });

    // Wait with timeout
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_seconds),
        child.wait(),
    )
    .await;

    // Wait for output readers to finish
    let _ = stdout_handle.await;
    let _ = stderr_handle.await;

    let duration_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(Ok(status)) => Ok(CommandResult {
            exit_code: status.code().unwrap_or(-1),
            duration_ms,
        }),
        Ok(Err(e)) => Err(e.into()),
        Err(_) => {
            // Timeout — child is killed by kill_on_drop
            anyhow::bail!("Command timed out after {}s", timeout_seconds);
        }
    }
}
