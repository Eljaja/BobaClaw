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
    /// Optional HTTP(S) proxy for streamable HTTP MCP (e.g. `http://127.0.0.1:8080`).
    /// Ignored for stdio MCP. Values may use `$VAR` or `${VAR}`.
    #[serde(default)]
    pub proxy_url: String,
    /// Host env var holding a Bearer token for HTTP MCP (`Authorization: Bearer …`).
    /// Ignored when empty or unset. Not required for Parallel Search MCP free tier.
    #[serde(default)]
    pub auth_env: String,
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

impl Default for McpServerConfig {
    fn default() -> Self {
        Self {
            url: None,
            proxy_url: String::new(),
            auth_env: String::new(),
            command: String::new(),
            args: Vec::new(),
            env: HashMap::new(),
            enabled: default_true(),
            timeout_secs: default_mcp_tool_timeout_secs(),
            connect_timeout_secs: default_mcp_connect_timeout_secs(),
            tools_allowlist: Vec::new(),
            tools_denylist: Vec::new(),
        }
    }
}

impl McpServerConfig {
    /// True when this entry uses streamable HTTP instead of a stdio subprocess.
    pub fn uses_http(&self) -> bool {
        self.url.as_deref().is_some_and(|u| !u.trim().is_empty())
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

    pub fn resolve_proxy_url(&self) -> Option<String> {
        let resolved = resolve_env_value(self.proxy_url.trim());
        if resolved.is_empty() {
            None
        } else {
            Some(resolved)
        }
    }

    /// Bearer token for HTTP MCP, from [`Self::auth_env`] when set in the host environment.
    pub fn resolve_auth_token(&self) -> Option<String> {
        let key = self.auth_env.trim();
        if key.is_empty() {
            return None;
        }
        let token = std::env::var(key).ok()?;
        let token = token.trim();
        if token.is_empty() {
            None
        } else {
            Some(token.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_proxy_url_expands_env() {
        std::env::set_var("MCP_PROXY", "http://127.0.0.1:11882");
        let cfg = McpServerConfig {
            proxy_url: "${MCP_PROXY}".into(),
            ..Default::default()
        };
        assert_eq!(
            cfg.resolve_proxy_url().as_deref(),
            Some("http://127.0.0.1:11882")
        );
    }

    #[test]
    fn resolve_auth_token_from_env() {
        std::env::set_var("PARALLEL_API_KEY", "test-token");
        let cfg = McpServerConfig {
            auth_env: "PARALLEL_API_KEY".into(),
            ..Default::default()
        };
        assert_eq!(cfg.resolve_auth_token().as_deref(), Some("test-token"));
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
