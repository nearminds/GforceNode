//! Token exchange and node secret management.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::NodeConfig;

#[derive(Debug, Serialize)]
struct RegisterRequest {
    registration_token: String,
    system_info: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct RegisterResponse {
    node_token: String,
}

/// Exchange a one-time registration token for a permanent node secret.
/// The permanent secret is stored in config.toml.
pub async fn register_node(
    server: &str,
    registration_token: &str,
    system_info: serde_json::Value,
    use_tls: bool,
) -> Result<NodeConfig> {
    let scheme = if use_tls { "https" } else { "http" };
    let url = format!("{scheme}://{server}/api/v1/nodes/register");

    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .json(&RegisterRequest {
            registration_token: registration_token.into(),
            system_info,
        })
        .send()
        .await
        .context("Failed to connect to GForce server")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Registration failed (HTTP {}): {}", status, body);
    }

    let register_resp: RegisterResponse = resp.json().await.context("Invalid response")?;

    let config = NodeConfig {
        server: server.into(),
        node_token: register_resp.node_token,
        workspace_root: crate::config::default_workspace_root()
            .to_string_lossy()
            .to_string(),
        use_tls,
        allowed_commands: Default::default(),
        unrestricted_mode: false,
        heartbeat_interval_seconds: 30,
    };

    config.save().context("Failed to save config")?;
    tracing::info!("Node registered successfully. Config saved to ~/.gforce-node/config.toml");

    Ok(config)
}
