use std::sync::Arc;

use bobaclaw_core::{BobaConfig, BobaPaths, NormalizedRequest, TurnInterrupted};
use bobaclaw_mcp::McpHub;
use bobaclaw_provider::{ConversationMessage, ToolCall, ToolChatClient, ToolSpec};
use bobaclaw_skills::SkillRegistry;
use bobaclaw_state::SessionStore;
use sqlx::SqlitePool;
use tokio_util::sync::CancellationToken;

use crate::cancel::{check_cancel, interrupted_reply};
use crate::compaction::{effective_history, history_to_conversation, maybe_compact_session};
use crate::progress::{emit, AgentEvent, AgentProgress};
use crate::prompt::build_system_prompt;
use crate::review::build_review_snapshot;
use crate::tools::{
    exec_tool_spec, handle_exec_tool, handle_mcp_tool, handle_schedule_tool, handle_skill_tool,
    is_mcp_tool, is_skill_tool, schedule_tool_specs, skill_tool_specs, SKILL_MANAGE,
};

const MAX_TOOL_ITERATIONS: usize = 16;
const MAX_ACTION_RETRIES: usize = 2;
/// Extra completions when the model stops without user-visible text (common on long tool loops).
const MAX_EMPTY_RESPONSE_RETRIES: usize = 3;
const PERSIST_TOOL_BODY_MAX: usize = 4_000;
const TOOL_RESULTS_MARKER: &str = "\n\n<!-- tool-results -->\n";

const ACTION_REQUIRED_NUDGE: &str = "The user's request requires real tool output. \
You replied without calling exec, schedule, or a configured MCP tool. \
Call the appropriate tool now, then answer using only that output.";

const SUMMARY_RESPONSE_NUDGE: &str = "Reply to the user in plain language. \
Summarize what you accomplished using tool output already in this conversation. \
Include concrete results, errors, and next steps. Call a tool only if something is still missing.";

#[derive(Debug, Clone)]
struct ToolPersistEntry {
    name: String,
    exit_code: i32,
    body: String,
}

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
    let client = ToolChatClient::from_provider(&config.provider, api_key)?;
    let tools: Vec<ToolSpec> = {
        let mut t = vec![exec_tool_spec()];
        t.extend(schedule_tool_specs());
        t.extend(skill_tool_specs());
        if let Some(hub) = mcp {
            t.extend(hub.tool_specs());
        }
        t
    };

    let requires_action = user_request_requires_tools(&req.user_text);
    let mut last_run_id = None;
    let mut executed = false;
    let mut final_text = String::new();
    let mut action_retries = 0usize;
    let mut tool_persist: Vec<ToolPersistEntry> = Vec::new();
    let mut hit_iteration_limit = false;
    let mut tool_call_count = 0usize;
    let mut skill_manage_used = false;

    for iteration in 1..=MAX_TOOL_ITERATIONS {
        if let Err(TurnInterrupted) = check_cancel(cancel) {
            return finish_interrupted(
                final_text,
                tool_persist,
                session_id,
                last_run_id,
                executed,
                tool_call_count,
                skill_manage_used,
                &messages,
                progress,
            );
        }
        emit(
            progress,
            AgentEvent::LlmThinking {
                iteration: iteration as u32,
            },
        );
        let turn = match client
            .chat_turn(
                &messages,
                &tools,
                req.model_override.as_deref(),
                Some(cancel),
            )
            .await
        {
            Ok(t) => t,
            Err(e) if is_turn_interrupted(&e) => {
                return finish_interrupted(
                    final_text,
                    tool_persist,
                    session_id,
                    last_run_id,
                    executed,
                    tool_call_count,
                    skill_manage_used,
                    &messages,
                    progress,
                )
            }
            Err(e) => return Err(e),
        };

        let assistant = turn.message.clone();
        let assistant_text = assistant.text_content();
        let tool_calls = assistant.tool_calls.clone();
        let will_action_retry = tool_calls.as_ref().is_none_or(|c| c.is_empty())
            && requires_action
            && !executed
            && action_retries < MAX_ACTION_RETRIES;

        if !assistant_text.trim().is_empty() && !will_action_retry {
            final_text = assistant_text.clone();
            emit(
                progress,
                AgentEvent::AssistantChunk {
                    text: assistant_text.clone(),
                },
            );
        }
        messages.push(assistant);

        match tool_calls {
            Some(calls) if !calls.is_empty() => {
                for call in calls {
                    if let Err(TurnInterrupted) = check_cancel(cancel) {
                        return finish_interrupted(
                            final_text,
                            tool_persist,
                            session_id,
                            last_run_id,
                            executed,
                            tool_call_count,
                            skill_manage_used,
                            &messages,
                            progress,
                        );
                    }
                    tool_call_count += 1;
                    if call.function.name == SKILL_MANAGE {
                        skill_manage_used = true;
                    }
                    let (body, entry) = match run_tool_call(
                        paths,
                        config,
                        pool,
                        mcp,
                        session_id,
                        req,
                        &call,
                        progress,
                        cancel,
                        &mut last_run_id,
                        &mut executed,
                    )
                    .await
                    {
                        Ok(v) => v,
                        Err(e) if e.is::<TurnInterrupted>() => {
                            return finish_interrupted(
                                final_text,
                                tool_persist,
                                session_id,
                                last_run_id,
                                executed,
                                tool_call_count,
                                skill_manage_used,
                                &messages,
                                progress,
                            )
                        }
                        Err(e) => return Err(e),
                    };
                    tool_persist.push(entry);
                    messages.push(ConversationMessage::tool_result(call.id.clone(), body));
                }
            }
            _ => {
                if will_action_retry {
                    action_retries += 1;
                    messages.push(ConversationMessage::user(ACTION_REQUIRED_NUDGE));
                    continue;
                }
                final_text = messages
                    .last()
                    .map(|m| m.text_content())
                    .unwrap_or_default();
                break;
            }
        }
    }

    if final_text.is_empty() {
        hit_iteration_limit = true;
    }

    let mut empty_attempt = 0u32;
    while final_text.trim().is_empty() && empty_attempt < MAX_EMPTY_RESPONSE_RETRIES as u32 {
        if let Err(TurnInterrupted) = check_cancel(cancel) {
            return finish_interrupted(
                final_text,
                tool_persist,
                session_id,
                last_run_id,
                executed,
                tool_call_count,
                skill_manage_used,
                &messages,
                progress,
            );
        }
        empty_attempt += 1;
        emit(
            progress,
            AgentEvent::EmptyResponseRetry {
                attempt: empty_attempt,
            },
        );
        let nudge = if requires_action && !executed {
            ACTION_REQUIRED_NUDGE
        } else {
            SUMMARY_RESPONSE_NUDGE
        };
        let retry_tools =
            if (requires_action && !executed) || messages.iter().any(|m| m.role == "tool") {
                tools.as_slice()
            } else {
                &[]
            };
        messages.push(ConversationMessage::user(nudge));
        emit(
            progress,
            AgentEvent::LlmThinking {
                iteration: MAX_TOOL_ITERATIONS as u32 + empty_attempt,
            },
        );
        let turn = match client
            .chat_turn(
                &messages,
                retry_tools,
                req.model_override.as_deref(),
                Some(cancel),
            )
            .await
        {
            Ok(t) => t,
            Err(e) if is_turn_interrupted(&e) => {
                return finish_interrupted(
                    final_text,
                    tool_persist,
                    session_id,
                    last_run_id,
                    executed,
                    tool_call_count,
                    skill_manage_used,
                    &messages,
                    progress,
                )
            }
            Err(e) => return Err(e),
        };
        let assistant = turn.message.clone();
        let assistant_text = assistant.text_content();
        if !assistant_text.trim().is_empty() {
            emit(
                progress,
                AgentEvent::AssistantChunk {
                    text: assistant_text.clone(),
                },
            );
            final_text = assistant_text;
        }
        messages.push(assistant);
    }

    if let Err(TurnInterrupted) = check_cancel(cancel) {
        return finish_interrupted(
            final_text,
            tool_persist,
            session_id,
            last_run_id,
            executed,
            tool_call_count,
            skill_manage_used,
            &messages,
            progress,
        );
    }

    if final_text.trim().is_empty() {
        final_text = if hit_iteration_limit {
            "Reached the per-turn tool step limit (16) before producing a final reply. \
Ask me to continue or narrow the task."
                .into()
        } else {
            "(model finished without a text response)".into()
        };
    } else if hit_iteration_limit && !final_text.contains("tool step limit") {
        final_text.push_str(
            "\n\n(Reached the 16-step tool limit this turn; partial progress may be in tool output above.)",
        );
    }

    let persisted_assistant = build_persisted_assistant(&final_text, &tool_persist);
    let review_snapshot = build_review_snapshot(&messages);

    Ok(TurnOutcome {
        text: final_text,
        persisted_assistant,
        session_id: session_id.to_string(),
        last_run_id,
        executed,
        tool_call_count,
        skill_manage_used,
        interrupted: false,
        review_snapshot,
    })
}

fn is_turn_interrupted(err: &anyhow::Error) -> bool {
    err.is::<TurnInterrupted>() || err.downcast_ref::<TurnInterrupted>().is_some()
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
    messages: &[bobaclaw_provider::ConversationMessage],
    progress: Option<&dyn AgentProgress>,
) -> anyhow::Result<TurnOutcome> {
    emit(progress, AgentEvent::Interrupted);
    let text = interrupted_reply(&partial_text);
    let persisted_assistant = build_persisted_assistant(&text, &tool_persist);
    let review_snapshot = build_review_snapshot(messages);
    Ok(TurnOutcome {
        text,
        persisted_assistant,
        session_id: session_id.to_string(),
        last_run_id,
        executed,
        tool_call_count,
        skill_manage_used,
        interrupted: true,
        review_snapshot,
    })
}

#[allow(clippy::too_many_arguments)]
async fn run_tool_call(
    paths: &BobaPaths,
    config: &BobaConfig,
    pool: &SqlitePool,
    mcp: Option<&Arc<McpHub>>,
    session_id: &str,
    req: &NormalizedRequest,
    call: &ToolCall,
    progress: Option<&dyn AgentProgress>,
    cancel: &CancellationToken,
    last_run_id: &mut Option<String>,
    executed: &mut bool,
) -> anyhow::Result<(String, ToolPersistEntry)> {
    if let Err(TurnInterrupted) = check_cancel(cancel) {
        return Err(TurnInterrupted.into());
    }
    let name = call.function.name.clone();
    let (body, exit_code) = if matches!(
        name.as_str(),
        "schedule" | "schedule_recurring" | "schedule_list" | "schedule_cancel"
    ) {
        let body = handle_schedule_tool(pool, &req.agent_group, session_id, req, call).await?;
        *executed = true;
        (body, 0)
    } else if is_mcp_tool(mcp, &name) {
        let hub = mcp.expect("mcp hub required for mcp_* tools");
        let body = handle_mcp_tool(hub, call, progress).await?;
        *executed = true;
        let exit_code = if body.starts_with("MCP error:") { 1 } else { 0 };
        (body, exit_code)
    } else if name == "exec" {
        let result = handle_exec_tool(
            paths,
            config,
            &req.agent_group,
            pool,
            session_id,
            &req.request_id.to_string(),
            call,
            progress,
            cancel,
        )
        .await?;
        *executed = true;
        if !result.run_id.is_empty() {
            *last_run_id = Some(result.run_id);
        }
        (result.body, result.exit_code)
    } else if is_skill_tool(&name) {
        let body = handle_skill_tool(paths, &req.agent_group, call)?;
        *executed = true;
        (body, 0)
    } else {
        anyhow::bail!("unknown tool: {name}");
    };

    let entry = ToolPersistEntry {
        name,
        exit_code,
        body: truncate_for_persist(&body, PERSIST_TOOL_BODY_MAX),
    };
    Ok((body, entry))
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

fn truncate_for_persist(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max_chars.saturating_sub(40)).collect();
    out.push_str("\n… (truncated for session storage)");
    out
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
}
