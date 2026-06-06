use bobaclaw_core::{BobaConfig, BobaPaths};
use bobaclaw_provider::{ConversationMessage, ToolChatClient};
use bobaclaw_skill_forge::SkillForge;
use bobaclaw_state::StateDb;
use tracing::info;

use crate::tools::{
    handle_skill_tool, skill_tool_specs, SKILL_MANAGE,
};

/// Hermes default: background skill review after this many tool calls in one turn.
pub const SKILL_REVIEW_TOOL_THRESHOLD: usize = 10;

const MAX_REVIEW_ITERATIONS: usize = 4;
const SNAPSHOT_MAX_CHARS: usize = 12_000;
const MESSAGE_SNIPPET_MAX: usize = 2_000;

const REVIEW_SYSTEM: &str = "You are a background skill review agent for BobaClaw. \
The main agent finished a tool-heavy turn without saving procedural knowledge. \
Read the conversation snapshot. If the workflow is repeatable and non-trivial, \
create exactly one skill via skill_manage(action=create) with valid SKILL.md YAML frontmatter \
(name, description) and actionable steps. \
You may use skill_view and skills_list to avoid duplicates. \
If nothing is worth saving, reply with exactly: NO_SKILL_NEEDED. \
Do not invent tool output; only use skill tools.";

pub struct TurnSkillMetrics {
    pub tool_call_count: usize,
    pub skill_manage_used: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillSaveSource {
    BackgroundReview,
    ForgeAutoPromote,
}

#[derive(Debug, Clone)]
pub struct SkillSaveOutcome {
    pub skill_name: String,
    pub source: SkillSaveSource,
}

/// Post-turn skill save: background LLM review (Hermes-style), then forge auto-promote fallback.
pub async fn maybe_post_turn_skill_save(
    paths: &BobaPaths,
    config: &BobaConfig,
    state: &StateDb,
    agent_group: &str,
    metrics: &TurnSkillMetrics,
    snapshot: &str,
    last_run_id: Option<&str>,
) -> Option<SkillSaveOutcome> {
    if metrics.skill_manage_used {
        return None;
    }
    if metrics.tool_call_count < SKILL_REVIEW_TOOL_THRESHOLD {
        return None;
    }
    if snapshot.trim().is_empty() {
        return None;
    }

    if let Some(name) = run_background_skill_review(paths, config, agent_group, snapshot).await {
        info!(skill = name.as_str(), "background skill review created skill");
        return Some(SkillSaveOutcome {
            skill_name: name,
            source: SkillSaveSource::BackgroundReview,
        });
    }

    let run_id = last_run_id?;
    let forge = SkillForge::new(paths.clone(), agent_group.to_string());
    match forge.draft_and_promote_from_run(state, run_id).await {
        Ok(name) => {
            info!(run_id = run_id, skill = name.as_str(), "forge auto-promoted skill from run");
            Some(SkillSaveOutcome {
                skill_name: name,
                source: SkillSaveSource::ForgeAutoPromote,
            })
        }
        Err(e) => {
            tracing::debug!(run_id = run_id, error = %e, "post-turn skill save skipped");
            None
        }
    }
}

async fn run_background_skill_review(
    paths: &BobaPaths,
    config: &BobaConfig,
    agent_group: &str,
    snapshot: &str,
) -> Option<String> {
    let api_key = config.resolve_api_key().ok()?;
    let client = ToolChatClient::from_provider(&config.provider, api_key).ok()?;
    let tools = skill_tool_specs();

    let mut messages = vec![
        ConversationMessage::system(REVIEW_SYSTEM),
        ConversationMessage::user(format!(
            "Conversation snapshot from the completed turn:\n\n{snapshot}"
        )),
    ];

    for _ in 0..MAX_REVIEW_ITERATIONS {
        let turn = client.chat_turn(&messages, &tools, None).await.ok()?;
        let assistant = turn.message.clone();
        let tool_calls = assistant.tool_calls.clone();
        let assistant_text = assistant.text_content();

        match tool_calls {
            Some(calls) if !calls.is_empty() => {
                messages.push(assistant);
                for call in &calls {
                    if call.function.name == SKILL_MANAGE {
                        if let Ok(body) = handle_skill_tool(paths, agent_group, call) {
                            if let Some(name) = parse_created_skill_name(&body) {
                                return Some(name);
                            }
                        }
                    }
                    let body = handle_skill_tool(paths, agent_group, call)
                        .unwrap_or_else(|e| format!("error: {e}"));
                    messages.push(ConversationMessage::tool_result(call.id.clone(), body));
                }
            }
            _ => {
                if assistant_text.contains("NO_SKILL_NEEDED") {
                    return None;
                }
                break;
            }
        }
    }
    None
}

pub fn build_review_snapshot(messages: &[ConversationMessage]) -> String {
    let mut lines = Vec::new();
    for msg in messages.iter().filter(|m| m.role != "system") {
        let text = truncate_chars(&msg.text_content(), MESSAGE_SNIPPET_MAX);
        if text.trim().is_empty() {
            continue;
        }
        lines.push(format!("[{}] {}", msg.role, text));
    }
    truncate_chars(&lines.join("\n\n"), SNAPSHOT_MAX_CHARS)
}

pub fn parse_created_skill_name(body: &str) -> Option<String> {
    const PREFIX: &str = "Created skill '";
    let start = body.find(PREFIX)? + PREFIX.len();
    let rest = &body[start..];
    let end = rest.find('\'')?;
    Some(rest[..end].to_string())
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(20)).collect();
    out.push_str("\n… (truncated)");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_matches_hermes() {
        assert_eq!(SKILL_REVIEW_TOOL_THRESHOLD, 10);
    }

    #[test]
    fn parse_create_response() {
        assert_eq!(
            parse_created_skill_name("Created skill 'deploy-app' at /tmp/x"),
            Some("deploy-app".into())
        );
        assert_eq!(parse_created_skill_name("Patched foo"), None);
    }

    #[test]
    fn snapshot_skips_system_and_truncates() {
        let messages = vec![
            ConversationMessage::system("secret system"),
            ConversationMessage::user("run deploy"),
            ConversationMessage::assistant_text("done"),
        ];
        let snap = build_review_snapshot(&messages);
        assert!(!snap.contains("secret system"));
        assert!(snap.contains("[user]"));
        assert!(snap.contains("run deploy"));
    }
}
