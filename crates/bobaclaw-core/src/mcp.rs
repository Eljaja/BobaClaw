use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// MCP server table from `config.yaml` (`mcp_servers` key).
pub type McpServers = HashMap<String, McpServerConfig>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Streamable HTTP MCP endpoint (e.g. `http://127.0.0.1:3000/mcp`).
    /// When set, BobaClaw connects to a long-lived server instead of spawning `command`.
    #[serde(default)]
    pub url: Option<String>,
    /// Executable for stdio MCP (e.g. `npx`, `uv`, path to binary). Ignored when `url` is set.
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Extra env vars for the subprocess (values may use `$VAR` or `${VAR}`).
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Per-tool-call timeout (seconds).
    #[serde(default = "default_mcp_tool_timeout_secs")]
    pub timeout_secs: u64,
    /// Initial connection / handshake timeout (seconds).
    #[serde(default = "default_mcp_connect_timeout_secs")]
    pub connect_timeout_secs: u64,
    /// If non-empty, only these original MCP tool names are exposed.
    #[serde(default)]
    pub tools_allowlist: Vec<String>,
    /// Original MCP tool names to hide.
    #[serde(default)]
    pub tools_denylist: Vec<String>,
}

fn default_true() -> bool {
    true
}

fn default_mcp_tool_timeout_secs() -> u64 {
    120
}

fn default_mcp_connect_timeout_secs() -> u64 {
    60
}

impl McpServerConfig {
    /// True when this entry uses streamable HTTP instead of a stdio subprocess.
    pub fn uses_http(&self) -> bool {
        self.url
            .as_deref()
            .is_some_and(|u| !u.trim().is_empty())
    }

    pub fn resolve_env(&self) -> HashMap<String, String> {
        let mut out = HashMap::new();
        for (k, v) in &self.env {
            out.insert(k.clone(), resolve_env_value(v));
        }
        if !out.contains_key("PATH") {
            if let Ok(path) = std::env::var("PATH") {
                out.insert("PATH".into(), path);
            }
        }
        out
    }
}

/// Resolve `$VAR` / `${VAR}` from the host environment; leave literal otherwise.
pub fn resolve_env_value(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.starts_with("${") && trimmed.ends_with('}') {
        let key = &trimmed[2..trimmed.len() - 1];
        return std::env::var(key).unwrap_or_default();
    }
    if let Some(key) = trimmed.strip_prefix('$') {
        if !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return std::env::var(key).unwrap_or_default();
        }
    }
    raw.to_string()
}
