use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Max LLM↔tool iterations per user turn (each tool batch counts as one step).
    #[serde(default = "default_max_tool_iterations")]
    pub max_tool_iterations: usize,
    /// Parent-mode nudges when the model replies without tool calls while tools are offered.
    #[serde(default = "default_max_action_retries")]
    pub max_action_retries: usize,
    /// Retries when the model ends the tool loop without user-visible text.
    #[serde(default = "default_max_empty_response_retries")]
    pub max_empty_response_retries: u32,
}

fn default_max_tool_iterations() -> usize {
    60
}

fn default_max_action_retries() -> usize {
    2
}

fn default_max_empty_response_retries() -> u32 {
    3
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_tool_iterations: default_max_tool_iterations(),
            max_action_retries: default_max_action_retries(),
            max_empty_response_retries: default_max_empty_response_retries(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_max_tool_iterations_is_sixty() {
        assert_eq!(AgentConfig::default().max_tool_iterations, 60);
    }

    #[test]
    fn default_retry_limits() {
        let a = AgentConfig::default();
        assert_eq!(a.max_action_retries, 2);
        assert_eq!(a.max_empty_response_retries, 3);
    }
}
