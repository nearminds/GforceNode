//! One-time enrollment against the Gforce control plane.
//!
//! Calls `POST /api/v1/nodes/enroll` exactly once with the one-time
//! `enrollment_token` minted by the Gforce UI when the on-prem
//! Infrastructure row was created. On success we receive a long-lived
//! `auth_token` which the daemon presents on every heartbeat.
//!
//! The server stores only the SHA-256 hash of `auth_token`. We keep the
//! raw value in `~/.gforce-node/config.toml` with mode 0600 so a leaked
//! file is the only realistic loss path.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::NodeConfig;

/// Matches the server's `EnrollNodeRequest` Pydantic schema.
#[derive(Debug, Serialize)]
struct EnrollRequest<'a> {
    enrollment_token: &'a str,
    os: String,
    arch: String,
    hostname: String,
    cpu_cores: Option<u32>,
    ram_gb: Option<u32>,
    disk_gb: Option<u32>,
    gpu: Option<String>,
    agent_version: &'a str,
}

#[derive(Debug, Deserialize)]
struct EnrollData {
    node_id: String,
    infrastructure_id: String,
    auth_token: String,
    #[allow(dead_code)]
    enrolled_at: String,
}

/// Standard Gforce API envelope: `{data, error, meta}`.
#[derive(Debug, Deserialize)]
struct ApiEnvelope<T> {
    data: Option<T>,
    error: Option<Value>,
}

fn str_field(sys: &Value, key: &str) -> Option<String> {
    sys.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
}

fn u32_from_u64(sys: &Value, key: &str) -> Option<u32> {
    sys.get(key).and_then(|v| v.as_u64()).map(|n| n as u32)
}

fn u32_from_f64(sys: &Value, key: &str) -> Option<u32> {
    sys.get(key).and_then(|v| v.as_f64()).map(|n| n.round() as u32)
}

/// Perform enrollment and persist the resulting config. The server
/// rejects a token that has already been consumed, so a retry after
/// success will fail explicitly rather than silently re-enrol.
pub async fn register_node(
    server: &str,
    enrollment_token: &str,
    system_info: Value,
    use_tls: bool,
) -> Result<NodeConfig> {
    let scheme = if use_tls { "https" } else { "http" };
    let url = format!("{scheme}://{server}/api/v1/nodes/enroll");

    let agent_version = env!("CARGO_PKG_VERSION");

    let body = EnrollRequest {
        enrollment_token,
        os: str_field(&system_info, "os").unwrap_or_else(|| "unknown".into()),
        arch: str_field(&system_info, "arch").unwrap_or_else(|| "unknown".into()),
        hostname: str_field(&system_info, "hostname").unwrap_or_else(|| "unknown".into()),
        cpu_cores: u32_from_u64(&system_info, "cpu_cores"),
        ram_gb: u32_from_f64(&system_info, "memory_total_gb"),
        disk_gb: u32_from_f64(&system_info, "disk_total_gb"),
        gpu: str_field(&system_info, "gpu"),
        agent_version,
    };

    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .context("Failed to connect to Gforce server")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("Enrollment failed (HTTP {}): {}", status, text);
    }

    let envelope: ApiEnvelope<EnrollData> =
        resp.json().await.context("Invalid response from server")?;

    if let Some(err) = envelope.error {
        anyhow::bail!("Enrollment failed: {}", err);
    }
    let data = envelope
        .data
        .context("Server returned no data in enrollment response")?;

    let config = NodeConfig {
        server: server.into(),
        node_token: data.auth_token,
        node_id: Some(data.node_id),
        infrastructure_id: Some(data.infrastructure_id),
        workspace_root: crate::config::default_workspace_root()
            .to_string_lossy()
            .to_string(),
        use_tls,
        allowed_commands: Default::default(),
        unrestricted_mode: false,
        heartbeat_interval_seconds: 30,
    };

    config.save().context("Failed to save config")?;
    tracing::info!(
        "Node enrolled successfully. Config saved to ~/.gforce-node/config.toml"
    );

    Ok(config)
}
