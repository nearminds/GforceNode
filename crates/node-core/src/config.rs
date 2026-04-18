//! Node configuration — reads from ~/.gforce-node/config.toml

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Default config directory: ~/.gforce-node/
pub fn config_dir() -> PathBuf {
    dirs_home().join(".gforce-node")
}

/// Default workspace root for sandboxed operations.
pub fn default_workspace_root() -> PathBuf {
    dirs_home().join("gforce-workspaces")
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    /// Gforce server URL (e.g., "gforce.nearminds.org")
    pub server: String,

    /// Permanent node auth token. Presented as `Authorization: Bearer`
    /// on every heartbeat. Only the SHA-256 hash is stored server-side.
    pub node_token: String,

    /// Server-assigned node UUID (returned by /nodes/enroll).
    #[serde(default)]
    pub node_id: Option<String>,

    /// Server-assigned infrastructure UUID (returned by /nodes/enroll).
    #[serde(default)]
    pub infrastructure_id: Option<String>,

    /// Workspace root for sandboxed file operations
    #[serde(default = "default_workspace_root_str")]
    pub workspace_root: String,

    /// Whether to use TLS (wss://) — always true in production
    #[serde(default = "default_true")]
    pub use_tls: bool,

    /// Allowed command prefixes (empty = restricted mode)
    #[serde(default = "default_allowed_commands")]
    pub allowed_commands: Vec<String>,

    /// Enable unrestricted mode (allows any shell command)
    #[serde(default)]
    pub unrestricted_mode: bool,

    /// Heartbeat interval in seconds
    #[serde(default = "default_heartbeat_interval")]
    pub heartbeat_interval_seconds: u64,
}

fn default_workspace_root_str() -> String {
    default_workspace_root().to_string_lossy().to_string()
}

fn default_true() -> bool {
    true
}

fn default_heartbeat_interval() -> u64 {
    30
}

fn default_allowed_commands() -> Vec<String> {
    vec![
        "git".into(),
        "npm".into(),
        "npx".into(),
        "node".into(),
        "pip".into(),
        "python".into(),
        "python3".into(),
        "pytest".into(),
        "cargo".into(),
        "rustc".into(),
        "go".into(),
        "make".into(),
        "docker".into(),
        "ls".into(),
        "cat".into(),
        "find".into(),
        "grep".into(),
        "wc".into(),
        "head".into(),
        "tail".into(),
    ]
}

impl NodeConfig {
    /// Load config from the default path.
    pub fn load() -> Result<Self> {
        let path = config_dir().join("config.toml");
        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config from {}", path.display()))?;
        toml::from_str(&contents).context("Failed to parse config.toml")
    }

    /// Save config to the default path with restricted permissions.
    pub fn save(&self) -> Result<()> {
        let dir = config_dir();
        std::fs::create_dir_all(&dir)?;

        let path = dir.join("config.toml");
        let contents = toml::to_string_pretty(self)?;
        std::fs::write(&path, &contents)?;

        // Set file permissions to 0600 (owner read/write only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        }

        Ok(())
    }

    /// Build the WebSocket URL from config.
    pub fn ws_url(&self) -> String {
        let scheme = if self.use_tls { "wss" } else { "ws" };
        format!("{scheme}://{}/api/v1/nodes/ws", self.server)
    }
}
