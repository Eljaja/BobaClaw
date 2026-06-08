use std::fmt;
use std::sync::Mutex;

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
    /// Model ended the tool loop without user-visible text; asking again.
    EmptyResponseRetry { attempt: u32 },
    /// User or operator cancelled the in-flight turn.
    Interrupted,
    /// Subagent delegation started.
    SubagentStart { id: String, label: String },
    /// Subagent delegation finished.
    SubagentEnd {
        id: String,
        exit_code: i32,
        preview: String,
    },
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

const STEP_SEP: &str = "────────";

/// Accumulates multi-step agent activity for interactive display (CLI / Telegram).
#[derive(Debug, Default)]
pub struct ActivityLog {
    blocks: Mutex<Vec<String>>,
}

impl ActivityLog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_event(&self, event: &AgentEvent) {
        let block = format_step_block(event);
        if block.is_empty() {
            return;
        }
        self.blocks.lock().unwrap().push(block);
    }

    pub fn render(&self) -> String {
        let blocks = self.blocks.lock().unwrap();
        if blocks.is_empty() {
            return String::new();
        }
        blocks.join(&format!("\n{STEP_SEP}\n"))
    }

    /// Compact one-liner for CLI spinner (last meaningful line).
    pub fn spinner_line(&self) -> String {
        let blocks = self.blocks.lock().unwrap();
        let Some(last) = blocks.last() else {
            return "Working…".into();
        };
        last.lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("Working…")
            .to_string()
    }

    /// Keep the tail when the rendered log exceeds a byte budget (e.g. Telegram 4096).
    pub fn render_truncated(&self, max_bytes: usize) -> String {
        let full = self.render();
        if full.is_empty() {
            return full;
        }
        if full.len() <= max_bytes {
            return full;
        }
        let marker = "… (earlier steps hidden)";
        let budget = max_bytes.saturating_sub(marker.len() + STEP_SEP.len() + 2);
        let tail = truncate_tail_bytes(&full, budget);
        format!("{marker}\n{STEP_SEP}\n{tail}")
    }
}

/// Multi-line block for one agent step (appended to the activity log).
pub fn format_step_block(event: &AgentEvent) -> String {
    match event {
        AgentEvent::LlmThinking { iteration } => format!("Thinking… (step {iteration})"),
        AgentEvent::ToolStart { name, label } => {
            format!("Running {name}\n  $ {}", sanitize_status_text(label, 120))
        }
        AgentEvent::ToolEnd {
            name,
            exit_code,
            preview,
        } => {
            let status = if *exit_code == 0 {
                format!("Finished {name} (ok)")
            } else {
                format!("Finished {name} (exit {exit_code})")
            };
            let preview = preview.trim();
            if preview.is_empty() {
                status
            } else {
                let indented = preview
                    .lines()
                    .map(|line| format!("  {}", sanitize_status_text(line, 120)))
                    .collect::<Vec<_>>()
                    .join("\n");
                format!("{status}\n{indented}")
            }
        }
        AgentEvent::Compacting { tokens } if *tokens > 0 => {
            format!("Compacting context (~{tokens} tokens)…")
        }
        AgentEvent::Compacting { .. } => "Compacting context…".into(),
        AgentEvent::AssistantChunk { text } => format_assistant_block(text),
        AgentEvent::EmptyResponseRetry { attempt } => {
            format!("No reply text yet — retrying summary ({attempt}/{MAX_EMPTY_RESPONSE_RETRIES})")
        }
        AgentEvent::Interrupted => "⚡ Прервано".into(),
        AgentEvent::SubagentStart { label, .. } => format!("Subagent `{label}` starting…"),
        AgentEvent::SubagentEnd {
            id,
            exit_code,
            preview,
        } => {
            let status = if *exit_code == 0 {
                format!("Subagent `{id}` finished (ok)")
            } else {
                format!("Subagent `{id}` failed (exit {exit_code})")
            };
            let preview = preview.trim();
            if preview.is_empty() {
                status
            } else {
                format!("{status}\n  {}", sanitize_status_text(preview, 120))
            }
        }
    }
}

const MAX_EMPTY_RESPONSE_RETRIES: u32 = 3;

fn format_assistant_block(text: &str) -> String {
    let t = text.trim();
    if t.is_empty() {
        return String::new();
    }
    let lines: Vec<&str> = t.lines().take(12).collect();
    let joined = lines.join("\n");
    truncate_preview(&joined, 600)
}

/// One-line status for CLI / logs.
pub fn format_status_line(event: &AgentEvent) -> String {
    match event {
        AgentEvent::ToolEnd {
            name,
            exit_code,
            preview,
            ..
        } => {
            let status = if *exit_code == 0 {
                format!("Finished {name} (ok)")
            } else {
                format!("Finished {name} (exit {exit_code})")
            };
            let preview = preview.trim();
            if preview.is_empty() {
                status
            } else {
                format!("{status}: {}", sanitize_status_text(preview, 56))
            }
        }
        _ => format_step_block(event)
            .lines()
            .next()
            .unwrap_or("Working…")
            .to_string(),
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

fn truncate_tail_bytes(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut start = s.len().saturating_sub(max_bytes);
    while start < s.len() && !s.is_char_boundary(start) {
        start += 1;
    }
    s[start..].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_log_accumulates_steps() {
        let log = ActivityLog::new();
        log.push_event(&AgentEvent::LlmThinking { iteration: 1 });
        log.push_event(&AgentEvent::ToolStart {
            name: "exec".into(),
            label: "id".into(),
        });
        log.push_event(&AgentEvent::ToolEnd {
            name: "exec".into(),
            exit_code: 0,
            preview: "uid=1000".into(),
        });
        let rendered = log.render();
        assert!(rendered.contains("Thinking… (step 1)"));
        assert!(rendered.contains("Running exec"));
        assert!(rendered.contains("uid=1000"));
        assert!(rendered.contains(STEP_SEP));
    }

    #[test]
    fn tool_end_includes_preview() {
        let e = AgentEvent::ToolEnd {
            name: "exec".into(),
            exit_code: 0,
            preview: "hello".into(),
        };
        assert!(format_step_block(&e).contains("hello"));
        assert!(format_status_line(&e).contains("hello"));
    }

    #[test]
    fn subagent_events_format() {
        let start = AgentEvent::SubagentStart {
            id: "subagent_1".into(),
            label: "research".into(),
        };
        assert!(format_step_block(&start).contains("research"));
        let end = AgentEvent::SubagentEnd {
            id: "subagent_1".into(),
            exit_code: 0,
            preview: "Done: found 3 files".into(),
        };
        assert!(format_step_block(&end).contains("finished"));
        assert!(format_step_block(&end).contains("Done"));
    }
}
