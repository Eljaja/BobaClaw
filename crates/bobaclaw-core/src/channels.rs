use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChannelsConfig {
    #[serde(default)]
    pub telegram: TelegramConfig,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DmPolicy {
    Open,
    Allowlist,
    #[default]
    Pairing,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupPolicy {
    Open,
    #[default]
    Allowlist,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub bot_token: String,
    #[serde(default = "default_telegram_token_env")]
    pub bot_token_env: String,
    #[serde(default = "default_true")]
    pub polling: bool,
    #[serde(default)]
    pub dm_policy: DmPolicy,
    #[serde(default)]
    pub allow_from: Vec<i64>,
    #[serde(default)]
    pub group_policy: GroupPolicy,
    #[serde(default)]
    pub allowed_groups: Vec<i64>,
    #[serde(default = "default_true")]
    pub group_require_mention: bool,
    /// Min interval between `editMessageText` while the agent is working (ms).
    #[serde(default = "default_stream_edit_ms")]
    pub stream_edit_interval_ms: u64,
    /// Bot API proxy: inline URL or via `proxy_env` (HTTP/HTTPS/SOCKS5).
    #[serde(default)]
    pub proxy_url: String,
    #[serde(default)]
    pub proxy_env: String,
    /// `html` — markdown → Telegram HTML (GramIO-style); `plain` — no formatting.
    #[serde(default = "default_telegram_format")]
    pub format: TelegramFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelegramFormat {
    Plain,
    Html,
}

fn default_telegram_format() -> TelegramFormat {
    TelegramFormat::Html
}

fn default_telegram_token_env() -> String {
    "TELEGRAM_BOT_TOKEN".into()
}

fn default_stream_edit_ms() -> u64 {
    800
}

fn default_true() -> bool {
    true
}

impl Default for TelegramConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bot_token: String::new(),
            bot_token_env: default_telegram_token_env(),
            polling: true,
            dm_policy: DmPolicy::Pairing,
            allow_from: Vec::new(),
            group_policy: GroupPolicy::Allowlist,
            allowed_groups: Vec::new(),
            group_require_mention: true,
            stream_edit_interval_ms: default_stream_edit_ms(),
            proxy_url: String::new(),
            proxy_env: String::new(),
            format: default_telegram_format(),
        }
    }
}

impl TelegramConfig {
    /// Resolved proxy URL for Telegram Bot API, if configured.
    pub fn resolve_proxy(&self) -> Option<String> {
        let inline = self.proxy_url.trim();
        if !inline.is_empty() {
            return Some(inline.to_string());
        }
        if self.proxy_env.trim().is_empty() {
            return None;
        }
        std::env::var(self.proxy_env.trim())
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    pub fn resolve_bot_token(&self) -> anyhow::Result<String> {
        let inline = self.bot_token.trim();
        if !inline.is_empty() {
            return Ok(inline.to_string());
        }
        if self.bot_token_env.is_empty() {
            anyhow::bail!("set channels.telegram.bot_token or bot_token_env");
        }
        std::env::var(&self.bot_token_env).map_err(|_| {
            anyhow::anyhow!(
                "missing Telegram bot token: env {} or channels.telegram.bot_token",
                self.bot_token_env
            )
        })
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RoutingConfig {
    #[serde(default)]
    pub rules: Vec<RoutingRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingRule {
    pub r#match: RouteMatch,
    pub agent_group: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteMatch {
    pub channel: String,
    #[serde(default = "default_peer_wildcard")]
    pub peer: String,
}

fn default_peer_wildcard() -> String {
    "*".into()
}

/// Stable inbound identity: channel + chat id (+ optional forum thread).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelPeer {
    pub channel: String,
    pub peer: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
}

impl ChannelPeer {
    pub fn telegram(chat_id: i64, thread_id: Option<i64>) -> Self {
        Self {
            channel: "telegram".into(),
            peer: chat_id.to_string(),
            thread_id: thread_id.map(|t| t.to_string()),
        }
    }

    pub fn route_key(&self) -> String {
        match &self.thread_id {
            Some(t) => format!("{}:{}:{}", self.channel, self.peer, t),
            None => format!("{}:{}", self.channel, self.peer),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_proxy_prefers_inline_url() {
        let c = TelegramConfig {
            proxy_url: "http://127.0.0.1:7890".into(),
            ..Default::default()
        };
        assert_eq!(c.resolve_proxy().as_deref(), Some("http://127.0.0.1:7890"));
    }
}
