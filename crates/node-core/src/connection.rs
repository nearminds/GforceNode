//! WebSocket connection manager with auto-reconnect.

use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

use crate::commands::{AuthMessage, ServerCommand};
use crate::config::NodeConfig;

/// Active command counter shared across the connection.
pub type ActiveCommandCount = Arc<AtomicUsize>;

/// Events produced by the connection for the daemon to handle.
#[derive(Debug)]
pub enum ConnectionEvent {
    /// A command received from the server.
    Command(ServerCommand),
    /// Connection was closed (will auto-reconnect).
    Disconnected(String),
    /// Connection established.
    Connected,
}

/// Outbound messages to send to the server.
#[derive(Debug)]
pub enum OutboundMessage {
    /// JSON message to send.
    Json(serde_json::Value),
}

/// Run the WebSocket connection loop with auto-reconnect.
///
/// This function never returns (unless cancelled). It:
/// 1. Connects to the server
/// 2. Sends auth message
/// 3. Forwards incoming commands to `event_tx`
/// 4. Forwards outbound messages from `outbound_rx` to the server
/// 5. On disconnect, waits with exponential backoff and reconnects
pub async fn run_connection(
    config: &NodeConfig,
    system_info: serde_json::Value,
    event_tx: mpsc::Sender<ConnectionEvent>,
    mut outbound_rx: mpsc::Receiver<OutboundMessage>,
) {
    let mut backoff = Duration::from_secs(3);
    let max_backoff = Duration::from_secs(300); // 5 minutes

    loop {
        let ws_url = config.ws_url();
        tracing::info!(url = %ws_url, "Connecting to GForce server...");

        match connect_and_run(
            &ws_url,
            &config.node_token,
            &system_info,
            &event_tx,
            &mut outbound_rx,
        )
        .await
        {
            Ok(()) => {
                // Clean disconnect
                let _ = event_tx
                    .send(ConnectionEvent::Disconnected("clean disconnect".into()))
                    .await;
            }
            Err(e) => {
                tracing::warn!(error = %e, "Connection error");
                let _ = event_tx
                    .send(ConnectionEvent::Disconnected(e.to_string()))
                    .await;
            }
        }

        tracing::info!(
            backoff_secs = backoff.as_secs(),
            "Reconnecting in {} seconds...",
            backoff.as_secs()
        );
        tokio::time::sleep(backoff).await;

        // Exponential backoff with cap
        backoff = (backoff * 2).min(max_backoff);
    }
}

async fn connect_and_run(
    ws_url: &str,
    node_token: &str,
    system_info: &serde_json::Value,
    event_tx: &mpsc::Sender<ConnectionEvent>,
    outbound_rx: &mut mpsc::Receiver<OutboundMessage>,
) -> Result<()> {
    let (ws_stream, _) = connect_async(ws_url)
        .await
        .context("WebSocket connection failed")?;

    let (mut write, mut read) = ws_stream.split();

    // Send auth message
    let auth = AuthMessage {
        msg_type: "auth".into(),
        node_token: node_token.into(),
        system_info: Some(system_info.clone()),
    };
    let auth_json = serde_json::to_string(&auth)?;
    write.send(Message::Text(auth_json)).await?;

    tracing::info!("Connected and authenticated");
    let _ = event_tx.send(ConnectionEvent::Connected).await;

    // Main loop: read from server and write outbound messages
    loop {
        tokio::select! {
            // Incoming from server
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<serde_json::Value>(&text) {
                            Ok(value) => {
                                if value.get("type").and_then(|t| t.as_str()) == Some("command") {
                                    match serde_json::from_value::<ServerCommand>(value) {
                                        Ok(cmd) => {
                                            let _ = event_tx.send(ConnectionEvent::Command(cmd)).await;
                                        }
                                        Err(e) => {
                                            tracing::warn!(error = %e, "Failed to parse command");
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!(error = %e, "Failed to parse message");
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        tracing::info!("Server closed connection");
                        return Ok(());
                    }
                    Some(Ok(Message::Ping(data))) => {
                        write.send(Message::Pong(data)).await?;
                    }
                    Some(Ok(_)) => {} // ignore binary, pong
                    Some(Err(e)) => {
                        return Err(e.into());
                    }
                }
            }

            // Outbound messages
            out = outbound_rx.recv() => {
                match out {
                    Some(OutboundMessage::Json(value)) => {
                        let text = serde_json::to_string(&value)?;
                        write.send(Message::Text(text)).await?;
                    }
                    None => {
                        // Sender dropped
                        return Ok(());
                    }
                }
            }
        }
    }
}
