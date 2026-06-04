//! User-visible activity text while the agent runs (Telegram message edits).

use bobaclaw_agent::AgentEvent;

const HEADER: &str = "BobaClaw\n────────";

/// Full plain-text body for an in-progress Telegram message.
pub fn stream_message(activity: &str) -> String {
    let line = activity.trim();
    if line.is_empty() {
        format!("{HEADER}\nWorking…")
    } else {
        format!("{HEADER}\n{line}")
    }
}

pub fn initial_activity() -> &'static str {
    "Working…"
}

pub fn format_activity(event: &AgentEvent) -> String {
    match event {
        AgentEvent::LlmThinking { iteration } => format!("Thinking… (step {iteration})"),
        AgentEvent::ToolStart { name, label } => {
            let cmd = sanitize_one_line(label, 72);
            format!("Running {name}\n  $ {cmd}")
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

fn sanitize_one_line(s: &str, max: usize) -> String {
    let flat: String = strip_html_tags(s)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    truncate_chars(&flat, max)
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

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_end_no_html_dump() {
        let e = AgentEvent::ToolEnd {
            name: "exec".into(),
            exit_code: 0,
            preview: "<li class=\"x\">".into(),
        };
        assert_eq!(format_activity(&e), "Finished exec (ok)");
    }

    #[test]
    fn stream_has_header() {
        let m = stream_message("Thinking… (step 1)");
        assert!(m.starts_with("BobaClaw"));
        assert!(m.contains("Thinking"));
    }
}
