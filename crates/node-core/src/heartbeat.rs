//! Periodic heartbeat reporting via HTTPS.
//!
//! Posts to `POST /api/v1/nodes/heartbeat` every `interval` seconds,
//! authenticated by `Authorization: Bearer <node_token>`. The server
//! uses this to track liveness and flip nodes to ``offline`` after
//! three missed intervals.
//!
//! Network / transient server errors are logged and ignored — the
//! next tick retries. A 401 surfaces an error so the daemon can exit
//! instead of looping with a revoked token.

use std::time::Duration;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::NodeConfig;

#[derive(Debug, Serialize)]
struct HeartbeatBody<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    agent_version: Option<&'a str>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct HeartbeatData {
    node_id: String,
    status: String,
    last_heartbeat_at: String,
}

#[derive(Debug, Deserialize)]
struct ApiEnvelope<T> {
    #[allow(dead_code)]
    data: Option<T>,
    #[allow(dead_code)]
    error: Option<Value>,
}

/// Outcome of one heartbeat attempt.
pub enum HeartbeatOutcome {
    Ok,
    NetworkError(anyhow::Error),
    Unauthorized,
    ServerError(u16),
}

fn build_url(config: &NodeConfig) -> String {
    let scheme = if config.use_tls { "https" } else { "http" };
    format!("{scheme}://{}/api/v1/nodes/heartbeat", config.server)
}

/// Send a single heartbeat.
pub async fn send_once(
    client: &reqwest::Client,
    config: &NodeConfig,
) -> HeartbeatOutcome {
    let url = build_url(config);
    let body = HeartbeatBody {
        agent_version: Some(env!("CARGO_PKG_VERSION")),
    };

    let result = client
        .post(&url)
        .bearer_auth(&config.node_token)
        .json(&body)
        .send()
        .await;

    match result {
        Err(e) => HeartbeatOutcome::NetworkError(e.into()),
        Ok(resp) => {
            let status = resp.status();
            if status == reqwest::StatusCode::UNAUTHORIZED {
                return HeartbeatOutcome::Unauthorized;
            }
            if !status.is_success() {
                return HeartbeatOutcome::ServerError(status.as_u16());
            }
            match resp.json::<ApiEnvelope<HeartbeatData>>().await {
                Ok(_) => HeartbeatOutcome::Ok,
                Err(e) => HeartbeatOutcome::NetworkError(e.into()),
            }
        }
    }
}

/// Run the heartbeat loop until the node token is rejected.
pub async fn run_heartbeat_loop(config: NodeConfig) -> Result<()> {
    let interval = Duration::from_secs(config.heartbeat_interval_seconds.max(5));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;

    let mut ticker = tokio::time::interval(interval);

    loop {
        ticker.tick().await;
        match send_once(&client, &config).await {
            HeartbeatOutcome::Ok => {
                tracing::debug!("heartbeat ok");
            }
            HeartbeatOutcome::NetworkError(e) => {
                tracing::warn!(error = %e, "heartbeat network error — will retry");
            }
            HeartbeatOutcome::ServerError(code) => {
                tracing::warn!(code, "heartbeat server error — will retry");
            }
            HeartbeatOutcome::Unauthorized => {
                tracing::error!(
                    "heartbeat rejected with 401 — node token revoked; stopping"
                );
                anyhow::bail!("node token rejected by server (401)");
            }
        }
    }
}
