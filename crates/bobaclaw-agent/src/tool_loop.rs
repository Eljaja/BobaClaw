use async_recursion::async_recursion;
use std::sync::Arc;

use bobaclaw_core::{BobaConfig, BobaPaths, NormalizedRequest, TurnInterrupted};
use bobaclaw_mcp::McpHub;
use bobaclaw_provider::{ConversationMessage, ToolCall, ToolChatClient, ToolSpec};
use sqlx::SqlitePool;
use tokio_util::sync::CancellationToken;

use crate::cancel::check_cancel;
use crate::progress::{emit, AgentEvent, AgentProgress};
use crate::subagent::SubagentManager;
use crate::tools::{
    handle_exec_tool, handle_mcp_tool, handle_memory_tool, handle_schedule_tool, handle_skill_tool,
    handle_spawn_status_tool, handle_spawn_tool, handle_subagent_tool, is_mcp_tool, is_memory_tool,
    is_skill_tool, is_spawn_status_tool, is_spawn_tool, is_subagent_tool, MEMORY_MANAGE,
    SKILL_MANAGE,
};
use crate::turn_context::{TurnContext, TurnMode};

const MAX_ACTION_RETRIES: usize = 2;
const MAX_EMPTY_RESPONSE_RETRIES: usize = 3;
pub(crate) const PERSIST_TOOL_BODY_MAX: usize = 4_000;

const ACTION_REQUIRED_NUDGE: &str = "The user's request requires real tool output. \
You replied without calling exec, schedule, or a configured MCP tool. \
Call the appropriate tool now, then answer using only that output.";

const SUMMARY_RESPONSE_NUDGE: &str = "Reply to the user in plain language. \
Summarize what you accomplished using tool output already in this conversation. \
Include concrete results, errors, and next steps. Call a tool only if something is still missing.";

#[derive(Debug, Clone)]
pub(crate) struct ToolPersistEntry {
    pub name: String,
    pub exit_code: i32,
    pub body: String,
}

pub struct ToolLoopOutcome {
    pub final_text: String,
    pub executed: bool,
    pub tool_call_count: usize,
    pub skill_manage_used: bool,
    pub memory_manage_used: bool,
    pub last_run_id: Option<String>,
    pub tool_persist: Vec<ToolPersistEntry>,
    pub hit_iteration_limit: bool,
    pub interrupted: bool,
}

#[async_recursion]
#[allow(clippy::too_many_arguments)]
pub async fn run_tool_loop(
    paths: &BobaPaths,
    config: &BobaConfig,
    pool: &SqlitePool,
    mcp: Option<&Arc<McpHub>>,
    session_id: &str,
    req: &NormalizedRequest,
    turn_ctx: &TurnContext,
    mode: TurnMode,
    client: &ToolChatClient,
    tools: &[ToolSpec],
    messages: &mut Vec<ConversationMessage>,
    model_override: Option<&str>,
    requires_action: bool,
    max_iterations: usize,
    progress: Option<&dyn AgentProgress>,
    cancel: &CancellationToken,
    subagent: Option<&SubagentManager>,
) -> anyhow::Result<ToolLoopOutcome> {
    let mut last_run_id = None;
    let mut executed = false;
    let mut final_text = String::new();
    let mut action_retries = 0usize;
    let mut tool_persist: Vec<ToolPersistEntry> = Vec::new();
    let mut hit_iteration_limit = false;
    let mut tool_call_count = 0usize;
    let mut skill_manage_used = false;
    let mut memory_manage_used = false;

    for iteration in 1..=max_iterations {
        if let Err(TurnInterrupted) = check_cancel(cancel) {
            return Ok(interrupted_outcome(
                final_text,
                executed,
                tool_call_count,
                skill_manage_used,
                memory_manage_used,
                last_run_id,
                tool_persist,
            ));
        }
        emit(
            progress,
            AgentEvent::LlmThinking {
                iteration: iteration as u32,
            },
        );
        let turn = match client
            .chat_turn(messages, tools, model_override, Some(cancel))
            .await
        {
            Ok(t) => t,
            Err(e) if is_turn_interrupted(&e) => {
                return Ok(interrupted_outcome(
                    final_text,
                    executed,
                    tool_call_count,
                    skill_manage_used,
                    memory_manage_used,
                    last_run_id,
                    tool_persist,
                ));
            }
            Err(e) => return Err(e),
        };

        let assistant = turn.message.clone();
        let assistant_text = assistant.text_content();
        let tool_calls = assistant.tool_calls.clone();
        let will_action_retry = mode == TurnMode::Parent
            && tool_calls.as_ref().is_none_or(|c| c.is_empty())
            && requires_action
            && !executed
            && action_retries < MAX_ACTION_RETRIES;

        if !assistant_text.trim().is_empty() && !will_action_retry {
            final_text = assistant_text.clone();
            if mode == TurnMode::Parent {
                emit(
                    progress,
                    AgentEvent::AssistantChunk {
                        text: assistant_text.clone(),
                    },
                );
            }
        }
        messages.push(assistant);

        match tool_calls {
            Some(calls) if !calls.is_empty() => {
                for call in calls {
                    if let Err(TurnInterrupted) = check_cancel(cancel) {
                        return Ok(interrupted_outcome(
                            final_text,
                            executed,
                            tool_call_count,
                            skill_manage_used,
                            memory_manage_used,
                            last_run_id,
                            tool_persist,
                        ));
                    }
                    tool_call_count += 1;
                    if call.function.name == SKILL_MANAGE {
                        skill_manage_used = true;
                    }
                    if call.function.name == MEMORY_MANAGE {
                        memory_manage_used = true;
                    }
                    let (body, entry) = match run_tool_call(
                        paths,
                        config,
                        pool,
                        mcp,
                        session_id,
                        req,
                        turn_ctx,
                        &call,
                        progress,
                        cancel,
                        &mut last_run_id,
                        &mut executed,
                        subagent,
                    )
                    .await
                    {
                        Ok(v) => v,
                        Err(e) if e.is::<TurnInterrupted>() => {
                            return Ok(interrupted_outcome(
                                final_text,
                                executed,
                                tool_call_count,
                                skill_manage_used,
                                memory_manage_used,
                                last_run_id,
                                tool_persist,
                            ));
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

    if mode == TurnMode::Parent {
        let mut empty_attempt = 0u32;
        while final_text.trim().is_empty() && empty_attempt < MAX_EMPTY_RESPONSE_RETRIES as u32 {
            if let Err(TurnInterrupted) = check_cancel(cancel) {
                return Ok(interrupted_outcome(
                    final_text,
                    executed,
                    tool_call_count,
                    skill_manage_used,
                    memory_manage_used,
                    last_run_id,
                    tool_persist,
                ));
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
                    tools
                } else {
                    &[]
                };
            messages.push(ConversationMessage::user(nudge));
            emit(
                progress,
                AgentEvent::LlmThinking {
                    iteration: max_iterations as u32 + empty_attempt,
                },
            );
            let turn = match client
                .chat_turn(messages, retry_tools, model_override, Some(cancel))
                .await
            {
                Ok(t) => t,
                Err(e) if is_turn_interrupted(&e) => {
                    return Ok(interrupted_outcome(
                        final_text,
                        executed,
                        tool_call_count,
                        skill_manage_used,
                        memory_manage_used,
                        last_run_id,
                        tool_persist,
                    ));
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
    }

    if final_text.trim().is_empty() {
        final_text = if hit_iteration_limit {
            format!(
                "Reached the tool step limit ({max_iterations}) before producing a final reply. \
Ask to continue or narrow the task."
            )
        } else {
            "(model finished without a text response)".into()
        };
    } else if hit_iteration_limit
        && mode == TurnMode::Parent
        && !final_text.contains("tool step limit")
    {
        final_text.push_str(&format!(
            "\n\n(Reached the {max_iterations}-step tool limit; partial progress may be in tool output above.)"
        ));
    }

    Ok(ToolLoopOutcome {
        final_text,
        executed,
        tool_call_count,
        skill_manage_used,
        memory_manage_used,
        last_run_id,
        tool_persist,
        hit_iteration_limit,
        interrupted: false,
    })
}

fn interrupted_outcome(
    final_text: String,
    executed: bool,
    tool_call_count: usize,
    skill_manage_used: bool,
    memory_manage_used: bool,
    last_run_id: Option<String>,
    tool_persist: Vec<ToolPersistEntry>,
) -> ToolLoopOutcome {
    ToolLoopOutcome {
        final_text,
        executed,
        tool_call_count,
        skill_manage_used,
        memory_manage_used,
        last_run_id,
        tool_persist,
        hit_iteration_limit: false,
        interrupted: true,
    }
}

fn is_turn_interrupted(err: &anyhow::Error) -> bool {
    err.is::<TurnInterrupted>() || err.downcast_ref::<TurnInterrupted>().is_some()
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_tool_call(
    paths: &BobaPaths,
    config: &BobaConfig,
    pool: &SqlitePool,
    mcp: Option<&Arc<McpHub>>,
    session_id: &str,
    req: &NormalizedRequest,
    turn_ctx: &TurnContext,
    call: &ToolCall,
    progress: Option<&dyn AgentProgress>,
    cancel: &CancellationToken,
    last_run_id: &mut Option<String>,
    executed: &mut bool,
    subagent: Option<&SubagentManager>,
) -> anyhow::Result<(String, ToolPersistEntry)> {
    if let Err(TurnInterrupted) = check_cancel(cancel) {
        return Err(TurnInterrupted.into());
    }
    let name = call.function.name.clone();
    let (body, exit_code) = if is_subagent_tool(&name) {
        let manager = subagent.ok_or_else(|| anyhow::anyhow!("subagent manager not configured"))?;
        let delegation_ctx = turn_ctx.for_delegation(last_run_id.as_deref());
        let body = handle_subagent_tool(
            paths,
            config,
            pool,
            mcp,
            session_id,
            req,
            &delegation_ctx,
            call,
            progress,
            cancel,
            manager,
        )
        .await?;
        *executed = true;
        (body.body, body.exit_code)
    } else if is_spawn_tool(&name) {
        let manager = subagent.ok_or_else(|| anyhow::anyhow!("subagent manager not configured"))?;
        let delegation_ctx = turn_ctx.for_delegation(last_run_id.as_deref());
        let body = handle_spawn_tool(
            paths,
            config,
            pool,
            mcp,
            session_id,
            req,
            &delegation_ctx,
            call,
            progress,
            cancel,
            manager,
        )
        .await?;
        *executed = true;
        (body.body, body.exit_code)
    } else if is_spawn_status_tool(&name) {
        let body = handle_spawn_status_tool(paths, config, pool, session_id, call).await?;
        *executed = true;
        (body.body, body.exit_code)
    } else if matches!(
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
    } else if is_memory_tool(&name) {
        let body = handle_memory_tool(paths, &req.agent_group, call)?;
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

pub(crate) fn truncate_for_persist(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max_chars.saturating_sub(40)).collect();
    out.push_str("\n… (truncated for session storage)");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_for_persist_caps_length() {
        let long = "x".repeat(5000);
        let out = truncate_for_persist(&long, 100);
        assert!(out.chars().count() <= 100);
    }
}
