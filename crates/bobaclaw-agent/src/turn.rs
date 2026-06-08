use std::sync::Arc;

use bobaclaw_core::{BobaConfig, BobaPaths, NormalizedRequest, TurnInterrupted};
use bobaclaw_mcp::McpHub;
use bobaclaw_provider::ConversationMessage;
use bobaclaw_skills::SkillRegistry;
use bobaclaw_state::SessionStore;
use sqlx::SqlitePool;
use tokio_util::sync::CancellationToken;

use crate::cancel::{check_cancel, interrupted_reply};
use crate::compaction::{effective_history, history_to_conversation, maybe_compact_session};
use crate::progress::{emit, AgentEvent, AgentProgress};
use crate::prompt::build_system_prompt;
use crate::review::build_review_snapshot;
use crate::subagent::SubagentManager;
use crate::tool_loop::{run_tool_loop, ToolPersistEntry};
use crate::tools::build_parent_tool_specs;
use crate::turn_context::{TurnContext, TurnMode};

const TOOL_RESULTS_MARKER: &str = "\n\n<!-- tool-results -->\n";
const TOOL_RESULTS_HTML_COMMENT: &str = "<!-- tool-results -->";

/// Markers where leaked provider/tool XML starts — trim from the earliest match.
const LEAKED_TOOL_XML_MARKERS: &[&str] = &[
    "<invoke ",
    "<invoke>",
    "<tool_call",
    "<minimax:tool_call",
    "</parameter>",
    "</invoke>",
    "</minimax:tool_call>",
];

pub struct TurnOutcome {
    /// User-facing reply (Telegram / CLI).
    pub text: String,
    /// Stored in session DB; may include a `<!-- tool-results -->` appendix for the next turn.
    pub persisted_assistant: String,
    #[allow(dead_code)]
    pub session_id: String,
    pub last_run_id: Option<String>,
    pub executed: bool,
    pub tool_call_count: usize,
    pub skill_manage_used: bool,
    pub memory_manage_used: bool,
    pub interrupted: bool,
    /// Truncated conversation for background skill review (excludes system prompt).
    pub review_snapshot: String,
}

#[allow(clippy::too_many_arguments)]
pub async fn run_agent_turn(
    paths: &BobaPaths,
    config: &BobaConfig,
    pool: &SqlitePool,
    skills: &SkillRegistry,
    mcp: Option<&Arc<McpHub>>,
    session_id: &str,
    req: &NormalizedRequest,
    progress: Option<&dyn AgentProgress>,
    cancel: &CancellationToken,
    subagent: Option<&SubagentManager>,
) -> anyhow::Result<TurnOutcome> {
    if let Err(TurnInterrupted) = check_cancel(cancel) {
        return finish_interrupted(
            String::new(),
            Vec::new(),
            session_id,
            None,
            false,
            0,
            false,
            false,
            &[],
            progress,
        );
    }
    maybe_compact_session(pool, config, session_id, progress).await?;
    if let Err(TurnInterrupted) = check_cancel(cancel) {
        return finish_interrupted(
            String::new(),
            Vec::new(),
            session_id,
            None,
            false,
            0,
            false,
            false,
            &[],
            progress,
        );
    }

    let all = SessionStore::new(pool)
        .list_messages(session_id)
        .await
        .map_err(|e| anyhow::anyhow!("не удалось прочитать историю сессии: {e}"))?;
    let history = effective_history(&all);

    let mut messages = vec![ConversationMessage::system(build_system_prompt(
        paths,
        &req.agent_group,
        skills,
        mcp,
    ))];
    messages.extend(history_to_conversation(&history));

    if let Some(skill) = skills.match_request(&req.user_text) {
        if let Some(sys) = messages.first_mut() {
            sys.content = Some(serde_json::Value::String(format!(
                "{}\n\nMatched skill `{}`:\n{}",
                sys.text_content(),
                skill.name,
                skill.body
            )));
        }
    }

    let api_key = config.resolve_api_key()?;
    let client = bobaclaw_provider::ToolChatClient::from_provider(&config.provider, api_key)?;
    let subagents_enabled = config.subagents.enabled && subagent.is_some();
    let tools = build_parent_tool_specs(mcp, subagents_enabled);

    let requires_action = user_request_requires_tools(&req.user_text);
    let turn_ctx = TurnContext::parent(session_id);
    let max_tool_iterations = config.agent.max_tool_iterations;

    let loop_outcome = run_tool_loop(
        paths,
        config,
        pool,
        mcp,
        session_id,
        req,
        &turn_ctx,
        TurnMode::Parent,
        &client,
        &tools,
        &mut messages,
        req.model_override.as_deref(),
        requires_action,
        max_tool_iterations,
        progress,
        cancel,
        subagent,
    )
    .await?;

    if loop_outcome.interrupted {
        return finish_interrupted(
            loop_outcome.final_text,
            loop_outcome.tool_persist,
            session_id,
            loop_outcome.last_run_id,
            loop_outcome.executed,
            loop_outcome.tool_call_count,
            loop_outcome.skill_manage_used,
            loop_outcome.memory_manage_used,
            &messages,
            progress,
        );
    }

    let persisted_assistant =
        build_persisted_assistant(&loop_outcome.final_text, &loop_outcome.tool_persist);
    let review_snapshot = build_review_snapshot(&messages);
    let user_text = sanitize_user_reply(&loop_outcome.final_text);

    Ok(TurnOutcome {
        text: user_text,
        persisted_assistant,
        session_id: session_id.to_string(),
        last_run_id: loop_outcome.last_run_id,
        executed: loop_outcome.executed,
        tool_call_count: loop_outcome.tool_call_count,
        skill_manage_used: loop_outcome.skill_manage_used,
        memory_manage_used: loop_outcome.memory_manage_used,
        interrupted: false,
        review_snapshot,
    })
}

#[allow(clippy::too_many_arguments)]
fn finish_interrupted(
    partial_text: String,
    tool_persist: Vec<ToolPersistEntry>,
    session_id: &str,
    last_run_id: Option<String>,
    executed: bool,
    tool_call_count: usize,
    skill_manage_used: bool,
    memory_manage_used: bool,
    messages: &[bobaclaw_provider::ConversationMessage],
    progress: Option<&dyn AgentProgress>,
) -> anyhow::Result<TurnOutcome> {
    emit(progress, AgentEvent::Interrupted);
    let text = interrupted_reply(&partial_text);
    let persisted_assistant = build_persisted_assistant(&text, &tool_persist);
    let review_snapshot = build_review_snapshot(messages);
    Ok(TurnOutcome {
        text: sanitize_user_reply(&text),
        persisted_assistant,
        session_id: session_id.to_string(),
        last_run_id,
        executed,
        tool_call_count,
        skill_manage_used,
        memory_manage_used,
        interrupted: true,
        review_snapshot,
    })
}

/// Remove session-internal appendix and provider XML tool markup from user-visible replies.
pub fn sanitize_user_reply(text: &str) -> String {
    let mut s = text.to_string();
    if let Some(idx) = s.find(TOOL_RESULTS_HTML_COMMENT) {
        s.truncate(idx);
    }
    s = strip_leaked_tool_xml(&s);
    trim_trailing_tool_persist_lines(&s)
}

fn strip_leaked_tool_xml(s: &str) -> String {
    let mut cut = s.len();
    for marker in LEAKED_TOOL_XML_MARKERS {
        if let Some(i) = s.find(marker) {
            cut = cut.min(i);
        }
    }
    s[..cut].trim_end().to_string()
}

/// Drop echoed `[exec exit=0]` lines the model sometimes appends after tool loops.
fn trim_trailing_tool_persist_lines(s: &str) -> String {
    let mut lines: Vec<&str> = s.lines().collect();
    while let Some(last) = lines.last() {
        let t = last.trim();
        if t.is_empty() {
            lines.pop();
            continue;
        }
        if t.starts_with('[') && t.contains(" exit=") && t.ends_with(']') {
            lines.pop();
            continue;
        }
        break;
    }
    lines.join("\n").trim_end().to_string()
}

fn build_persisted_assistant(final_text: &str, tools: &[ToolPersistEntry]) -> String {
    if tools.is_empty() {
        return final_text.to_string();
    }
    let mut s = final_text.to_string();
    s.push_str(TOOL_RESULTS_MARKER);
    for t in tools {
        s.push_str(&format!("[{} exit={}]\n", t.name, t.exit_code));
        s.push_str(&t.body);
        if !t.body.ends_with('\n') {
            s.push('\n');
        }
    }
    s
}

/// Heuristic: user expects commands/tools, not a plan-only reply.
pub fn user_request_requires_tools(text: &str) -> bool {
    let lower = text.to_lowercase();
    const VERBS: &[&str] = &[
        "скачай",
        "download",
        "fetch",
        "curl",
        "wget",
        "run ",
        "execute",
        "exec ",
        "install",
        "build",
        "compile",
        "проверь",
        "check ",
        "test ",
        "fix ",
        "deploy",
        "ssh ",
        "запусти",
        "установ",
        "собери",
        "выполни",
        "scrape",
        "парси",
        "parse ",
        "create ",
        "создай",
        "удали",
        "delete ",
        "write ",
        "запиши",
    ];
    VERBS.iter().any(|v| lower.contains(v))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_tools_detects_action_verbs() {
        assert!(user_request_requires_tools("Скачай сайт example.com"));
        assert!(user_request_requires_tools("please run npm test"));
        assert!(!user_request_requires_tools("what is Rust?"));
    }

    #[test]
    fn persisted_assistant_includes_tool_block() {
        let tools = vec![ToolPersistEntry {
            name: "exec".into(),
            exit_code: 0,
            body: "ok\n".into(),
        }];
        let p = build_persisted_assistant("Done.", &tools);
        assert!(p.contains("<!-- tool-results -->"));
        assert!(p.contains("[exec exit=0]"));
    }

    #[test]
    fn sanitize_strips_tool_results_appendix_and_xml() {
        let raw = "Honcho analysis here.\n\n\
<!-- tool-results -->\n\
[exec exit=0]\n\
command: curl -sL example.com\n\
</parameter>\n\
</invoke>\n\
<invoke name=\"exec\">\n\
</minimax:tool_call>";
        let clean = sanitize_user_reply(raw);
        assert_eq!(clean, "Honcho analysis here.");
    }

    #[test]
    fn sanitize_strips_trailing_exec_exit_line() {
        let raw = "Summary of findings.\n[exec exit=0]";
        assert_eq!(sanitize_user_reply(raw), "Summary of findings.");
    }

    #[test]
    fn sanitize_preserves_normal_markdown() {
        let raw = "## Title\n\nParagraph with `code` and [link](https://example.com).";
        assert_eq!(sanitize_user_reply(raw), raw);
    }
}
