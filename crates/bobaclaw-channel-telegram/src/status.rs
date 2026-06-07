//! User-visible activity text while the agent runs (Telegram message edits).

use bobaclaw_agent::ActivityLog;

const HEADER: &str = "BobaClaw\n────────";
/// Telegram message body limit minus header and formatting slack.
pub const STREAM_BODY_BUDGET: usize = 3900;

/// Full plain-text body for an in-progress Telegram message.
pub fn stream_message(activity: &str) -> String {
    let body = activity.trim();
    if body.is_empty() {
        format!("{HEADER}\nWorking…")
    } else {
        format!("{HEADER}\n{body}")
    }
}

pub fn initial_activity() -> &'static str {
    "Working…"
}

pub fn render_activity_log(log: &ActivityLog) -> String {
    log.render_truncated(STREAM_BODY_BUDGET)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bobaclaw_agent::{format_step_block, AgentEvent};

    #[test]
    fn tool_end_no_html_dump() {
        let e = AgentEvent::ToolEnd {
            name: "exec".into(),
            exit_code: 0,
            preview: "<li class=\"x\">".into(),
        };
        let block = format_step_block(&e);
        assert!(block.contains("Finished exec (ok)"));
        assert!(!block.contains("<li"));
    }

    #[test]
    fn stream_has_header() {
        let m = stream_message("Thinking… (step 1)");
        assert!(m.starts_with("BobaClaw"));
        assert!(m.contains("Thinking"));
    }

    #[test]
    fn log_renders_multiple_steps() {
        let log = ActivityLog::new();
        log.push_event(&AgentEvent::LlmThinking { iteration: 1 });
        log.push_event(&AgentEvent::ToolStart {
            name: "exec".into(),
            label: "uname -a".into(),
        });
        let body = render_activity_log(&log);
        assert!(body.contains("step 1"));
        assert!(body.contains("uname -a"));
    }
}
