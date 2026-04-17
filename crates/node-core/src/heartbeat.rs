//! Periodic heartbeat reporting.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::mpsc;

use crate::commands::HeartbeatMessage;
use crate::connection::OutboundMessage;

/// Run the heartbeat loop, sending system stats every `interval` seconds.
pub async fn run_heartbeat(
    interval: Duration,
    start_time: Instant,
    active_commands: Arc<AtomicUsize>,
    outbound_tx: mpsc::Sender<OutboundMessage>,
) {
    let mut ticker = tokio::time::interval(interval);
    ticker.tick().await; // Skip the first immediate tick

    loop {
        ticker.tick().await;

        let stats = collect_system_stats();
        let heartbeat = HeartbeatMessage {
            msg_type: "heartbeat".into(),
            uptime_seconds: start_time.elapsed().as_secs(),
            cpu_percent: stats.cpu_percent,
            memory_percent: stats.memory_percent,
            disk_free_gb: stats.disk_free_gb,
            active_commands: active_commands.load(Ordering::Relaxed),
        };

        let json = match serde_json::to_value(&heartbeat) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "Failed to serialize heartbeat");
                continue;
            }
        };

        if outbound_tx.send(OutboundMessage::Json(json)).await.is_err() {
            tracing::debug!("Outbound channel closed, stopping heartbeat");
            break;
        }
    }
}

struct SystemStats {
    cpu_percent: f32,
    memory_percent: f32,
    disk_free_gb: f32,
}

fn collect_system_stats() -> SystemStats {
    // Use sysinfo to collect real metrics.
    // For now, return placeholder values that will be filled in
    // when node-executor's system module is integrated.
    SystemStats {
        cpu_percent: 0.0,
        memory_percent: 0.0,
        disk_free_gb: 0.0,
    }
}
