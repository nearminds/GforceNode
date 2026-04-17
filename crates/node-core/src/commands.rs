//! Command type definitions for the node protocol.

use serde::{Deserialize, Serialize};

/// A command sent from the server to this node agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerCommand {
    #[serde(rename = "type")]
    pub msg_type: String, // "command"
    pub command_id: String,
    pub action: String,
    pub payload: serde_json::Value,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
}

fn default_timeout() -> u64 {
    300
}

/// Streaming output sent from the agent to the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputMessage {
    #[serde(rename = "type")]
    pub msg_type: String, // "output"
    pub command_id: String,
    pub stream: String, // "stdout" | "stderr"
    pub data: String,
}

impl OutputMessage {
    pub fn stdout(command_id: &str, data: String) -> Self {
        Self {
            msg_type: "output".into(),
            command_id: command_id.into(),
            stream: "stdout".into(),
            data,
        }
    }

    pub fn stderr(command_id: &str, data: String) -> Self {
        Self {
            msg_type: "output".into(),
            command_id: command_id.into(),
            stream: "stderr".into(),
            data,
        }
    }
}

/// Final result sent from the agent to the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResultMessage {
    #[serde(rename = "type")]
    pub msg_type: String, // "result"
    pub command_id: String,
    pub exit_code: Option<i32>,
    pub success: bool,
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl ResultMessage {
    pub fn ok(command_id: &str, duration_ms: u64) -> Self {
        Self {
            msg_type: "result".into(),
            command_id: command_id.into(),
            exit_code: Some(0),
            success: true,
            duration_ms: Some(duration_ms),
            data: None,
        }
    }

    pub fn ok_with_data(command_id: &str, duration_ms: u64, data: serde_json::Value) -> Self {
        Self {
            msg_type: "result".into(),
            command_id: command_id.into(),
            exit_code: Some(0),
            success: true,
            duration_ms: Some(duration_ms),
            data: Some(data),
        }
    }

    pub fn fail(command_id: &str, exit_code: i32, duration_ms: u64) -> Self {
        Self {
            msg_type: "result".into(),
            command_id: command_id.into(),
            exit_code: Some(exit_code),
            success: false,
            duration_ms: Some(duration_ms),
            data: None,
        }
    }

    pub fn error(command_id: &str, message: &str) -> Self {
        Self {
            msg_type: "result".into(),
            command_id: command_id.into(),
            exit_code: None,
            success: false,
            duration_ms: None,
            data: Some(serde_json::json!({ "error": message })),
        }
    }
}

/// Authentication message sent as the first message after connecting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthMessage {
    #[serde(rename = "type")]
    pub msg_type: String, // "auth"
    pub node_token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_info: Option<serde_json::Value>,
}

/// Heartbeat message sent periodically.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatMessage {
    #[serde(rename = "type")]
    pub msg_type: String, // "heartbeat"
    pub uptime_seconds: u64,
    pub cpu_percent: f32,
    pub memory_percent: f32,
    pub disk_free_gb: f32,
    pub active_commands: usize,
}
