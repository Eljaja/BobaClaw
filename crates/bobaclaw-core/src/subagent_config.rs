use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentConfig {
    #[serde(default = "default_subagents_enabled")]
    pub enabled: bool,
    #[serde(default = "default_max_depth")]
    pub max_depth: u8,
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: usize,
    #[serde(default = "default_max_tool_iterations")]
    pub max_tool_iterations: usize,
    #[serde(default = "default_child_timeout_seconds")]
    pub child_timeout_seconds: u64,
    #[serde(default = "default_result_max_chars")]
    pub result_max_chars: usize,
    #[serde(default)]
    pub persist_child_sessions: bool,
    #[serde(default = "default_backend")]
    pub default_backend: String,
    /// Optional cheaper model for all native child runs (Phase B).
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub presets: HashMap<String, SubagentPreset>,
    #[serde(default)]
    pub backends: SubagentBackendsConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SubagentPreset {
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub system_extra: Option<String>,
    #[serde(default)]
    pub tools_allowlist: Vec<String>,
    #[serde(default)]
    pub skills: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentBackendsConfig {
    #[serde(default)]
    pub claude_code: ClaudeCodeBackendConfig,
    #[serde(default)]
    pub codex: CodexBackendConfig,
    #[serde(default)]
    pub cursor: CursorBackendConfig,
}

impl Default for SubagentBackendsConfig {
    fn default() -> Self {
        Self {
            claude_code: ClaudeCodeBackendConfig::default(),
            codex: CodexBackendConfig::default(),
            cursor: CursorBackendConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeCodeBackendConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_claude_command")]
    pub command: String,
    #[serde(default = "default_claude_api_key_env")]
    pub api_key_env: String,
    #[serde(default = "default_claude_max_turns")]
    pub max_turns: u32,
    #[serde(default = "default_external_timeout_secs")]
    pub timeout_secs: u64,
}

impl Default for ClaudeCodeBackendConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            command: default_claude_command(),
            api_key_env: default_claude_api_key_env(),
            max_turns: default_claude_max_turns(),
            timeout_secs: default_external_timeout_secs(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexBackendConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_codex_command")]
    pub command: String,
    #[serde(default = "default_codex_api_key_env")]
    pub api_key_env: String,
    #[serde(default = "default_codex_sandbox")]
    pub sandbox: String,
    #[serde(default = "default_external_timeout_secs")]
    pub timeout_secs: u64,
}

impl Default for CodexBackendConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            command: default_codex_command(),
            api_key_env: default_codex_api_key_env(),
            sandbox: default_codex_sandbox(),
            timeout_secs: default_external_timeout_secs(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CursorBackendConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_cursor_wrapper_command")]
    pub wrapper_command: String,
    #[serde(default = "default_cursor_api_key_env")]
    pub api_key_env: String,
    #[serde(default = "default_cursor_model")]
    pub model: String,
    #[serde(default = "default_external_timeout_secs")]
    pub timeout_secs: u64,
}

impl Default for CursorBackendConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            wrapper_command: default_cursor_wrapper_command(),
            api_key_env: default_cursor_api_key_env(),
            model: default_cursor_model(),
            timeout_secs: default_external_timeout_secs(),
        }
    }
}

fn default_subagents_enabled() -> bool {
    true
}

fn default_max_depth() -> u8 {
    1
}

fn default_max_concurrent() -> usize {
    2
}

fn default_max_tool_iterations() -> usize {
    20
}

fn default_child_timeout_seconds() -> u64 {
    600
}

fn default_result_max_chars() -> usize {
    12_000
}

fn default_backend() -> String {
    "native".into()
}

fn default_claude_command() -> String {
    "claude".into()
}

fn default_claude_api_key_env() -> String {
    "ANTHROPIC_API_KEY".into()
}

fn default_claude_max_turns() -> u32 {
    30
}

fn default_codex_command() -> String {
    "codex".into()
}

fn default_codex_api_key_env() -> String {
    "CODEX_API_KEY".into()
}

fn default_codex_sandbox() -> String {
    "workspace-write".into()
}

fn default_cursor_wrapper_command() -> String {
    "python3".into()
}

fn default_cursor_api_key_env() -> String {
    "CURSOR_API_KEY".into()
}

fn default_cursor_model() -> String {
    "composer-2.5".into()
}

fn default_external_timeout_secs() -> u64 {
    900
}

impl Default for SubagentConfig {
    fn default() -> Self {
        Self {
            enabled: default_subagents_enabled(),
            max_depth: default_max_depth(),
            max_concurrent: default_max_concurrent(),
            max_tool_iterations: default_max_tool_iterations(),
            child_timeout_seconds: default_child_timeout_seconds(),
            result_max_chars: default_result_max_chars(),
            persist_child_sessions: false,
            default_backend: default_backend(),
            model: None,
            presets: HashMap::new(),
            backends: SubagentBackendsConfig::default(),
        }
    }
}

impl SubagentConfig {
    pub fn preset(&self, id: &str) -> Option<&SubagentPreset> {
        self.presets.get(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_plan() {
        let cfg = SubagentConfig::default();
        assert!(cfg.enabled);
        assert_eq!(cfg.max_depth, 1);
        assert_eq!(cfg.max_concurrent, 2);
        assert_eq!(cfg.max_tool_iterations, 20);
        assert_eq!(cfg.child_timeout_seconds, 600);
        assert!(!cfg.backends.claude_code.enabled);
    }
}
