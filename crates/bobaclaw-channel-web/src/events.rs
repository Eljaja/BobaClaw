//! Server-Sent Event payloads for a web UI turn.

use axum::response::sse::Event;
use bobaclaw_agent::{sanitize_user_reply, AgentEvent, AgentProgress};
use serde::Serialize;
use tokio::sync::mpsc::UnboundedSender;

const LABEL_MAX_CHARS: usize = 500;
const PREVIEW_MAX_CHARS: usize = 4000;

/// One SSE frame. The SSE `event:` name equals `type`; `data:` is this JSON object.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WebEvent {
    /// Turn accepted for this session.
    Start {
        session_id: String,
    },
    /// LLM call in progress (tool-loop iteration).
    Thinking {
        iteration: u32,
    },
    ToolStart {
        name: String,
        label: String,
    },
    ToolEnd {
        name: String,
        exit_code: i32,
        preview: String,
    },
    Compacting {
        tokens: u32,
    },
    /// Intermediate assistant text (may be superseded by `done.reply`).
    AssistantChunk {
        text: String,
    },
    Retry {
        attempt: u32,
        max_attempts: u32,
    },
    Interrupted,
    SubagentStart {
        id: String,
        label: String,
    },
    SubagentEnd {
        id: String,
        exit_code: i32,
        preview: String,
    },
    /// Final, sanitized reply. Always the last frame of a successful turn.
    Done {
        reply: String,
        session_id: String,
        run_id: Option<String>,
        interrupted: bool,
    },
    /// Turn failed. Last frame of a failed turn.
    Error {
        message: String,
    },
}

impl WebEvent {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Start { .. } => "start",
            Self::Thinking { .. } => "thinking",
            Self::ToolStart { .. } => "tool_start",
            Self::ToolEnd { .. } => "tool_end",
            Self::Compacting { .. } => "compacting",
            Self::AssistantChunk { .. } => "assistant_chunk",
            Self::Retry { .. } => "retry",
            Self::Interrupted => "interrupted",
            Self::SubagentStart { .. } => "subagent_start",
            Self::SubagentEnd { .. } => "subagent_end",
            Self::Done { .. } => "done",
            Self::Error { .. } => "error",
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| r#"{"type":"error"}"#.into())
    }

    pub fn to_sse(&self) -> Event {
        Event::default().event(self.name()).data(self.to_json())
    }

    pub fn from_agent(event: AgentEvent) -> Self {
        match event {
            AgentEvent::LlmThinking { iteration } => Self::Thinking { iteration },
            AgentEvent::ToolStart { name, label } => Self::ToolStart {
                name,
                label: truncate_chars(&label, LABEL_MAX_CHARS),
            },
            AgentEvent::ToolEnd {
                name,
                exit_code,
                preview,
            } => Self::ToolEnd {
                name,
                exit_code,
                preview: truncate_chars(&preview, PREVIEW_MAX_CHARS),
            },
            AgentEvent::Compacting { tokens } => Self::Compacting { tokens },
            AgentEvent::AssistantChunk { text } => Self::AssistantChunk {
                text: sanitize_user_reply(&text),
            },
            AgentEvent::EmptyResponseRetry {
                attempt,
                max_attempts,
            } => Self::Retry {
                attempt,
                max_attempts,
            },
            AgentEvent::Interrupted => Self::Interrupted,
            AgentEvent::SubagentStart { id, label } => Self::SubagentStart {
                id,
                label: truncate_chars(&label, LABEL_MAX_CHARS),
            },
            AgentEvent::SubagentEnd {
                id,
                exit_code,
                preview,
            } => Self::SubagentEnd {
                id,
                exit_code,
                preview: truncate_chars(&preview, PREVIEW_MAX_CHARS),
            },
        }
    }
}

/// Progress callback that forwards agent events into the SSE channel.
///
/// Sends never block; if the browser disconnected the receiver is gone and events are
/// dropped while the turn keeps running (and is persisted as usual).
pub struct ChannelProgress {
    tx: UnboundedSender<WebEvent>,
}

impl ChannelProgress {
    pub fn new(tx: UnboundedSender<WebEvent>) -> Self {
        Self { tx }
    }
}

impl AgentProgress for ChannelProgress {
    fn on_event(&self, event: AgentEvent) {
        let _ = self.tx.send(WebEvent::from_agent(event));
    }
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_with_type_tag() {
        let e = WebEvent::ToolEnd {
            name: "exec".into(),
            exit_code: 1,
            preview: "boom".into(),
        };
        assert_eq!(e.name(), "tool_end");
        let v: serde_json::Value = serde_json::from_str(&e.to_json()).unwrap();
        assert_eq!(v["type"], "tool_end");
        assert_eq!(v["exit_code"], 1);
        assert_eq!(v["preview"], "boom");

        let v: serde_json::Value = serde_json::from_str(&WebEvent::Interrupted.to_json()).unwrap();
        assert_eq!(v, serde_json::json!({"type": "interrupted"}));

        let done = WebEvent::Done {
            reply: "hi".into(),
            session_id: "sess_1".into(),
            run_id: None,
            interrupted: false,
        };
        let v: serde_json::Value = serde_json::from_str(&done.to_json()).unwrap();
        assert_eq!(v["type"], "done");
        assert_eq!(v["reply"], "hi");
        assert!(v["run_id"].is_null());
    }

    #[test]
    fn event_names_match_type_tag() {
        let all = [
            WebEvent::Start {
                session_id: "s".into(),
            },
            WebEvent::Thinking { iteration: 1 },
            WebEvent::ToolStart {
                name: "n".into(),
                label: "l".into(),
            },
            WebEvent::Compacting { tokens: 3 },
            WebEvent::AssistantChunk { text: "t".into() },
            WebEvent::Retry {
                attempt: 1,
                max_attempts: 2,
            },
            WebEvent::SubagentStart {
                id: "i".into(),
                label: "l".into(),
            },
            WebEvent::SubagentEnd {
                id: "i".into(),
                exit_code: 0,
                preview: String::new(),
            },
            WebEvent::Error {
                message: "m".into(),
            },
        ];
        for e in all {
            let v: serde_json::Value = serde_json::from_str(&e.to_json()).unwrap();
            assert_eq!(v["type"], e.name());
        }
    }

    #[test]
    fn maps_agent_events_and_sanitizes() {
        let e = WebEvent::from_agent(AgentEvent::AssistantChunk {
            text: "Answer\n\n<!-- tool-results -->\n[exec exit=0]\nsecret".into(),
        });
        assert_eq!(
            e,
            WebEvent::AssistantChunk {
                text: "Answer".into()
            }
        );
        let e = WebEvent::from_agent(AgentEvent::ToolEnd {
            name: "exec".into(),
            exit_code: 0,
            preview: "x".repeat(PREVIEW_MAX_CHARS + 10),
        });
        let WebEvent::ToolEnd { preview, .. } = e else {
            panic!("expected tool_end");
        };
        assert_eq!(preview.chars().count(), PREVIEW_MAX_CHARS + 1);
        assert_eq!(
            WebEvent::from_agent(AgentEvent::LlmThinking { iteration: 2 }),
            WebEvent::Thinking { iteration: 2 }
        );
    }

    #[test]
    fn channel_progress_survives_dropped_receiver() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let p = ChannelProgress::new(tx);
        drop(rx);
        p.on_event(AgentEvent::Interrupted);
    }
}
