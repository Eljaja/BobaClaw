use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Max LLM↔tool iterations per user turn (each tool batch counts as one step).
    #[serde(default = "default_max_tool_iterations")]
    pub max_tool_iterations: usize,
}

fn default_max_tool_iterations() -> usize {
    60
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_tool_iterations: default_max_tool_iterations(),
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
}
