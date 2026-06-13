use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsConfig {
    #[serde(default)]
    pub web_fetch: WebFetchConfig,
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            web_fetch: WebFetchConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebFetchConfig {
    /// Host-side HTTP GET for the agent. Off by default (SSRF surface).
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_web_fetch_max_bytes")]
    pub max_bytes: usize,
    #[serde(default = "default_web_fetch_timeout_secs")]
    pub timeout_secs: u64,
    #[serde(default = "default_web_fetch_max_redirects")]
    pub max_redirects: usize,
}

fn default_web_fetch_max_bytes() -> usize {
    512_000
}

fn default_web_fetch_timeout_secs() -> u64 {
    30
}

fn default_web_fetch_max_redirects() -> usize {
    5
}

impl Default for WebFetchConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_bytes: default_web_fetch_max_bytes(),
            timeout_secs: default_web_fetch_timeout_secs(),
            max_redirects: default_web_fetch_max_redirects(),
        }
    }
}
