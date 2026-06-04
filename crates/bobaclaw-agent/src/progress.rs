use std::fmt;

/// Live status for CLI / TUI (thinking, tool calls).
#[derive(Debug, Clone)]
pub enum AgentEvent {
    /// Calling the LLM (tool-loop iteration).
    LlmThinking { iteration: u32 },
    /// About to run a tool.
    ToolStart { name: String, label: String },
    /// Tool finished.
    ToolEnd {
        name: String,
        exit_code: i32,
        preview: String,
    },
    /// Context compaction (LLM summary), Hermes/OpenClaw style.
    Compacting { tokens: u32 },
    /// Partial assistant text (channel streaming, e.g. Telegram editMessage).
    AssistantChunk { text: String },
}

pub trait AgentProgress: Send + Sync {
    fn on_event(&self, event: AgentEvent);
}

impl<F> AgentProgress for F
where
    F: Fn(AgentEvent) + Send + Sync,
{
    fn on_event(&self, event: AgentEvent) {
        self(event);
    }
}

pub(crate) fn emit(progress: Option<&dyn AgentProgress>, event: AgentEvent) {
    if let Some(p) = progress {
        p.on_event(event);
    }
}

impl fmt::Display for AgentEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AgentEvent::LlmThinking { iteration } => {
                write!(f, "думаю (шаг {iteration})")
            }
            AgentEvent::ToolStart { name, label } => write!(f, "{name}: {label}"),
            AgentEvent::ToolEnd {
                name,
                exit_code,
                preview,
            } => write!(f, "{name} exit={exit_code} {preview}"),
            AgentEvent::Compacting { tokens } => write!(f, "compacting context (~{tokens} tok)"),
            AgentEvent::AssistantChunk { text } => write!(f, "{}", truncate_preview(text, 80)),
        }
    }
}

fn truncate_preview(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}
