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

/// One-line status for CLI / logs (no raw tool output).
pub fn format_status_line(event: &AgentEvent) -> String {
    match event {
        AgentEvent::LlmThinking { iteration } => format!("Thinking… (step {iteration})"),
        AgentEvent::ToolStart { name, label } => {
            format!("Running {name}: {}", sanitize_status_text(label, 72))
        }
        AgentEvent::ToolEnd { name, exit_code, .. } => {
            if *exit_code == 0 {
                format!("Finished {name} (ok)")
            } else {
                format!("Finished {name} (exit {exit_code})")
            }
        }
        AgentEvent::Compacting { tokens } if *tokens > 0 => {
            format!("Compacting context (~{tokens} tokens)…")
        }
        AgentEvent::Compacting { .. } => "Compacting context…".into(),
        AgentEvent::AssistantChunk { .. } => "Writing reply…".into(),
    }
}

/// Strip HTML-ish noise from tool output before optional previews.
pub fn sanitize_status_text(s: &str, max: usize) -> String {
    let flat: String = strip_html_tags(s)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    truncate_preview(&flat, max)
}

fn strip_html_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

impl fmt::Display for AgentEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", format_status_line(self))
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
