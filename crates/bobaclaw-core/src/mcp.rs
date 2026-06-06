use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// MCP server table from `config.yaml` (`mcp_servers` key).
pub type McpServers = HashMap<String, McpServerConfig>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Streamable HTTP MCP endpoint (e.g. `http://127.0.0.1:3000/mcp`). When set, `command` is ignored.
    #[serde(default)]
    pub url: String,
    /// Stdio subprocess executable (e.g. `npx`, `uv`, path to binary). Omit when using `url`.
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
    pub fn uses_http(&self) -> bool {
        !self.url.trim().is_empty()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_http_mcp_server_without_command() {
        let raw = r#"
url: http://127.0.0.1:3000/mcp
enabled: true
timeout_secs: 180
"#;
        let cfg: McpServerConfig = serde_yaml::from_str(raw).unwrap();
        assert!(cfg.uses_http());
        assert_eq!(cfg.url, "http://127.0.0.1:3000/mcp");
        assert!(cfg.command.is_empty());
    }
}
