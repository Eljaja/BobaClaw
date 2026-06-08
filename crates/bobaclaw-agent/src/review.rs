use bobaclaw_core::{BobaConfig, BobaPaths};
use bobaclaw_provider::{ConversationMessage, ToolChatClient};
use tracing::info;

use crate::tools::{
    handle_memory_tool, handle_skill_tool, memory_tool_spec, skill_tool_specs, MEMORY_MANAGE,
    SKILL_MANAGE,
};

/// Hermes default: background memory review every N user turns.
pub const MEMORY_REVIEW_TURN_THRESHOLD: usize = 10;

/// Hermes default: background skill review after this many tool calls in one turn.
pub const SKILL_REVIEW_TOOL_THRESHOLD: usize = 10;

const MAX_REVIEW_ITERATIONS: usize = 4;
const SNAPSHOT_MAX_CHARS: usize = 12_000;
const MESSAGE_SNIPPET_MAX: usize = 2_000;
const EXISTING_MEMORY_MAX_CHARS: usize = 4_000;

const MEMORY_REVIEW_SYSTEM: &str = "You are a background memory review agent for BobaClaw. \
The main agent completed a turn. Read the conversation snapshot and existing memory (if any). \
Extract durable facts, preferences, or user-specific context worth persisting across sessions. \
If worth saving, call memory_manage(action=append) exactly once with path MEMORY.md or memory/<file>.md|.txt. \
Do not save repeatable multi-step tool workflows here — those belong in skills, not memory. \
If nothing is worth saving, reply with exactly: NO_MEMORY_NEEDED. \
Do not invent tool output; only use memory_manage.";

const SKILL_REVIEW_SYSTEM: &str = "You are a background skill review agent for BobaClaw. \
The main agent finished a tool-heavy turn without saving procedural knowledge. \
Read the conversation snapshot. Save a skill only when the workflow is repeatable, multi-step, and tool-specific. \
User facts, preferences, and one-off answers are NOT skills — reply NO_SKILL_NEEDED for those. \
If a skill is warranted, create exactly one via skill_manage(action=create) with valid SKILL.md YAML frontmatter \
(name, description) and actionable steps. \
You may use skill_view and skills_list to avoid duplicates. \
If nothing is worth saving, reply with exactly: NO_SKILL_NEEDED. \
Do not invent tool output; only use skill tools.";

pub struct TurnReviewMetrics {
    pub tool_call_count: usize,
    pub skill_manage_used: bool,
    pub memory_manage_used: bool,
    pub user_message_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PostTurnSave {
    Memory { path: String },
    Skill { name: String },
}

#[derive(Debug, Clone, Default)]
pub struct PostTurnReviewOutcome {
    pub memory: Option<PostTurnSave>,
    pub skill: Option<PostTurnSave>,
}

pub fn should_run_memory_review(user_message_count: usize, memory_manage_used: bool) -> bool {
    !memory_manage_used
        && user_message_count > 0
        && user_message_count % MEMORY_REVIEW_TURN_THRESHOLD == 0
}

pub fn should_run_skill_review(tool_call_count: usize, skill_manage_used: bool) -> bool {
    !skill_manage_used && tool_call_count >= SKILL_REVIEW_TOOL_THRESHOLD
}

/// Post-turn review: memory track first, then skill track (Hermes PR #2235 pattern).
pub async fn maybe_post_turn_review(
    paths: &BobaPaths,
    config: &BobaConfig,
    agent_group: &str,
    metrics: &TurnReviewMetrics,
    snapshot: &str,
) -> PostTurnReviewOutcome {
    let mut outcome = PostTurnReviewOutcome::default();

    if snapshot.trim().is_empty() {
        return outcome;
    }

    if should_run_memory_review(metrics.user_message_count, metrics.memory_manage_used) {
        if let Some(path) = run_background_memory_review(paths, config, agent_group, snapshot).await
        {
            info!(path = path.as_str(), "background memory review appended");
            outcome.memory = Some(PostTurnSave::Memory { path });
        }
    }

    if should_run_skill_review(metrics.tool_call_count, metrics.skill_manage_used) {
        if let Some(name) = run_background_skill_review(paths, config, agent_group, snapshot).await
        {
            info!(
                skill = name.as_str(),
                "background skill review created skill"
            );
            outcome.skill = Some(PostTurnSave::Skill { name });
        }
    }

    outcome
}

async fn run_background_memory_review(
    paths: &BobaPaths,
    config: &BobaConfig,
    agent_group: &str,
    snapshot: &str,
) -> Option<String> {
    let api_key = config.resolve_api_key().ok()?;
    let client = ToolChatClient::from_provider(&config.provider, api_key).ok()?;
    let tools = vec![memory_tool_spec()];

    let existing = load_existing_memory_snippet(paths, agent_group);
    let system = if existing.is_empty() {
        MEMORY_REVIEW_SYSTEM.to_string()
    } else {
        format!("{MEMORY_REVIEW_SYSTEM}\n\nExisting workspace memory (reference only):\n{existing}")
    };

    let mut messages = vec![
        ConversationMessage::system(system),
        ConversationMessage::user(format!(
            "Conversation snapshot from the completed turn:\n\n{snapshot}"
        )),
    ];

    for _ in 0..MAX_REVIEW_ITERATIONS {
        let turn = client.chat_turn(&messages, &tools, None, None).await.ok()?;
        let assistant = turn.message.clone();
        let tool_calls = assistant.tool_calls.clone();
        let assistant_text = assistant.text_content();

        match tool_calls {
            Some(calls) if !calls.is_empty() => {
                messages.push(assistant);
                for call in &calls {
                    if call.function.name == MEMORY_MANAGE {
                        if let Ok(body) = handle_memory_tool(paths, agent_group, call) {
                            if let Some(path) = parse_appended_memory_path(&body) {
                                return Some(path);
                            }
                        }
                    }
                    let body = handle_memory_tool(paths, agent_group, call)
                        .unwrap_or_else(|e| format!("error: {e}"));
                    messages.push(ConversationMessage::tool_result(call.id.clone(), body));
                }
            }
            _ => {
                if assistant_text.contains("NO_MEMORY_NEEDED") {
                    return None;
                }
                break;
            }
        }
    }
    None
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
        ConversationMessage::system(SKILL_REVIEW_SYSTEM),
        ConversationMessage::user(format!(
            "Conversation snapshot from the completed turn:\n\n{snapshot}"
        )),
    ];

    for _ in 0..MAX_REVIEW_ITERATIONS {
        let turn = client.chat_turn(&messages, &tools, None, None).await.ok()?;
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

fn load_existing_memory_snippet(paths: &BobaPaths, agent_group: &str) -> String {
    let workspace = paths.group_workspace(agent_group);
    let mut parts = Vec::new();

    let memory_md = workspace.join("MEMORY.md");
    if memory_md.is_file() {
        if let Ok(text) = std::fs::read_to_string(&memory_md) {
            parts.push(format!(
                "## MEMORY.md\n{}",
                truncate_chars(&text, EXISTING_MEMORY_MAX_CHARS)
            ));
        }
    }

    let memory_dir = workspace.join("memory");
    if memory_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&memory_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if ext != "md" && ext != "txt" {
                    continue;
                }
                if let Ok(text) = std::fs::read_to_string(&path) {
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
                    parts.push(format!(
                        "## memory/{name}\n{}",
                        truncate_chars(&text, EXISTING_MEMORY_MAX_CHARS / 2)
                    ));
                }
            }
        }
    }

    truncate_chars(&parts.join("\n\n"), EXISTING_MEMORY_MAX_CHARS)
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

pub fn parse_appended_memory_path(body: &str) -> Option<String> {
    const PREFIX: &str = "Appended to '";
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
    fn thresholds_match_hermes() {
        assert_eq!(MEMORY_REVIEW_TURN_THRESHOLD, 10);
        assert_eq!(SKILL_REVIEW_TOOL_THRESHOLD, 10);
    }

    #[test]
    fn memory_gate_every_tenth_user_turn() {
        assert!(!should_run_memory_review(5, false));
        assert!(!should_run_memory_review(10, true));
        assert!(should_run_memory_review(10, false));
        assert!(should_run_memory_review(20, false));
    }

    #[test]
    fn skill_gate_at_ten_tools() {
        assert!(!should_run_skill_review(9, false));
        assert!(!should_run_skill_review(10, true));
        assert!(should_run_skill_review(10, false));
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
    fn parse_append_response() {
        assert_eq!(
            parse_appended_memory_path("Appended to 'MEMORY.md'."),
            Some("MEMORY.md".into())
        );
        assert_eq!(
            parse_appended_memory_path("Appended to 'memory/notes.txt'."),
            Some("memory/notes.txt".into())
        );
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

    #[test]
    fn load_existing_memory_snippet_reads_files() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_path_buf();
        let ws = home.join("workspace").join("home");
        std::fs::create_dir_all(ws.join("memory")).unwrap();
        std::fs::write(ws.join("MEMORY.md"), "user likes tea").unwrap();
        std::fs::write(ws.join("memory/words.txt"), "codeword").unwrap();

        let paths = BobaPaths {
            home: home.clone(),
            config: home.join("config.yaml"),
            state_db: home.join("state.db"),
            runs: home.join("runs"),
            workspace: home.join("workspace"),
        };
        let snippet = load_existing_memory_snippet(&paths, "home");
        assert!(snippet.contains("user likes tea"));
        assert!(snippet.contains("codeword"));
    }
}
