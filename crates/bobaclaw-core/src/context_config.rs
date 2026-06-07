use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextConfig {
    /// Auto-summarize when estimated tokens exceed window − reserve.
    #[serde(default = "default_compression_enabled")]
    pub compression_enabled: bool,
    #[serde(default = "default_context_window_tokens")]
    pub context_window_tokens: u32,
    #[serde(default = "default_reserve_tokens")]
    pub reserve_tokens: u32,
    /// Recent messages kept verbatim at the tail (Hermes-style).
    #[serde(default = "default_keep_recent_messages")]
    pub keep_recent_messages: usize,
}

fn default_compression_enabled() -> bool {
    true
}

fn default_context_window_tokens() -> u32 {
    128_000
}

fn default_reserve_tokens() -> u32 {
    16_000
}

fn default_keep_recent_messages() -> usize {
    8
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            compression_enabled: default_compression_enabled(),
            context_window_tokens: default_context_window_tokens(),
            reserve_tokens: default_reserve_tokens(),
            keep_recent_messages: default_keep_recent_messages(),
        }
    }
}

impl ContextConfig {
    pub fn compact_threshold_tokens(&self) -> u32 {
        self.context_window_tokens
            .saturating_sub(self.reserve_tokens)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_threshold() {
        let c = ContextConfig::default();
        assert_eq!(c.compact_threshold_tokens(), 112_000);
        assert!(c.compression_enabled);
    }
}
