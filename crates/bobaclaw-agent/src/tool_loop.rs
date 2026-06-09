use async_recursion::async_recursion;
use std::sync::Arc;

use bobaclaw_core::{
    BobaConfig, BobaPaths, NormalizedRequest, TurnInterrupted, TOOL_BODY_PERSIST_MAX_CHARS,
};
use bobaclaw_mcp::McpHub;
use bobaclaw_provider::{ConversationMessage, ToolCall, ToolChatClient, ToolSpec};
use sqlx::SqlitePool;
use tokio_util::sync::CancellationToken;

use crate::cancel::check_cancel;
use crate::compaction::maybe_ensure_context_budget;
use crate::progress::{emit, AgentEvent, AgentProgress};
use crate::subagent::SubagentManager;
use crate::tools::{
    dispatch_tool_call, ToolCallContext, ToolCallOutcome, MEMORY_MANAGE, SKILL_MANAGE,
};
use crate::turn_context::{TurnContext, TurnMode};

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

struct ToolLoopState {
    final_text: String,
    executed: bool,
    tool_call_count: usize,
    skill_manage_used: bool,
    memory_manage_used: bool,
    last_run_id: Option<String>,
    tool_persist: Vec<ToolPersistEntry>,
    action_retries: usize,
}

impl ToolLoopState {
    fn new() -> Self {
        Self {
            final_text: String::new(),
            executed: false,
            tool_call_count: 0,
            skill_manage_used: false,
            memory_manage_used: false,
            last_run_id: None,
            tool_persist: Vec::new(),
            action_retries: 0,
        }
    }

    fn interrupted(self) -> ToolLoopOutcome {
        ToolLoopOutcome {
            final_text: self.final_text,
            executed: self.executed,
            tool_call_count: self.tool_call_count,
            skill_manage_used: self.skill_manage_used,
            memory_manage_used: self.memory_manage_used,
            last_run_id: self.last_run_id,
            tool_persist: self.tool_persist,
            hit_iteration_limit: false,
            interrupted: true,
        }
    }

    fn complete(self, hit_iteration_limit: bool) -> ToolLoopOutcome {
        ToolLoopOutcome {
            final_text: self.final_text,
            executed: self.executed,
            tool_call_count: self.tool_call_count,
            skill_manage_used: self.skill_manage_used,
            memory_manage_used: self.memory_manage_used,
            last_run_id: self.last_run_id,
            tool_persist: self.tool_persist,
            hit_iteration_limit,
            interrupted: false,
        }
    }
}

/// Parent turn offered tools but the model may reply without calling them.
pub fn parent_turn_offered_tools(mode: TurnMode, tools: &[ToolSpec]) -> bool {
    mode == TurnMode::Parent && !tools.is_empty()
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
    let max_action_retries = config.agent.max_action_retries;
    let max_empty_response_retries = config.agent.max_empty_response_retries;
    let mut state = ToolLoopState::new();
    let mut hit_iteration_limit = false;
    let history_boundary = messages.len();

    for iteration in 1..=max_iterations {
        if let Err(TurnInterrupted) = check_cancel(cancel) {
            return Ok(state.interrupted());
        }
        maybe_ensure_context_budget(
            pool,
            config,
            session_id,
            messages,
            history_boundary,
            progress,
        )
        .await?;
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
            Err(e) if is_turn_interrupted(&e) => return Ok(state.interrupted()),
            Err(e) => return Err(e),
        };

        let assistant = turn.message.clone();
        let assistant_text = assistant.text_content();
        let tool_calls = assistant.tool_calls.clone();
        let will_action_retry = requires_action
            && tool_calls.as_ref().is_none_or(|c| c.is_empty())
            && !state.executed
            && state.action_retries < max_action_retries;

        if !assistant_text.trim().is_empty() && !will_action_retry {
            state.final_text = assistant_text.clone();
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
                        return Ok(state.interrupted());
                    }
                    state.tool_call_count += 1;
                    if call.function.name == SKILL_MANAGE {
                        state.skill_manage_used = true;
                    }
                    if call.function.name == MEMORY_MANAGE {
                        state.memory_manage_used = true;
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
                        &mut state.last_run_id,
                        &mut state.executed,
                        subagent,
                    )
                    .await
                    {
                        Ok(v) => v,
                        Err(e) if e.is::<TurnInterrupted>() => return Ok(state.interrupted()),
                        Err(e) => return Err(e),
                    };
                    state.tool_persist.push(entry);
                    messages.push(ConversationMessage::tool_result(call.id.clone(), body));
                }
            }
            _ => {
                if will_action_retry {
                    state.action_retries += 1;
                    messages.push(ConversationMessage::user(ACTION_REQUIRED_NUDGE));
                    continue;
                }
                state.final_text = messages
                    .last()
                    .map(|m| m.text_content())
                    .unwrap_or_default();
                break;
            }
        }
    }

    if state.final_text.is_empty() {
        hit_iteration_limit = true;
    }

    if mode == TurnMode::Parent {
        let mut empty_attempt = 0u32;
        while state.final_text.trim().is_empty() && empty_attempt < max_empty_response_retries {
            if let Err(TurnInterrupted) = check_cancel(cancel) {
                return Ok(state.interrupted());
            }
            empty_attempt += 1;
            emit(
                progress,
                AgentEvent::EmptyResponseRetry {
                    attempt: empty_attempt,
                    max_attempts: max_empty_response_retries,
                },
            );
            let nudge = if requires_action && !state.executed {
                ACTION_REQUIRED_NUDGE
            } else {
                SUMMARY_RESPONSE_NUDGE
            };
            let retry_tools = if (requires_action && !state.executed)
                || messages.iter().any(|m| m.role == "tool")
            {
                tools
            } else {
                &[]
            };
            messages.push(ConversationMessage::user(nudge));
            maybe_ensure_context_budget(
                pool,
                config,
                session_id,
                messages,
                history_boundary,
                progress,
            )
            .await?;
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
                Err(e) if is_turn_interrupted(&e) => return Ok(state.interrupted()),
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
                state.final_text = assistant_text;
            }
            messages.push(assistant);
        }
    }

    if state.final_text.trim().is_empty() {
        state.final_text = if hit_iteration_limit {
            format!(
                "Reached the tool step limit ({max_iterations}) before producing a final reply. \
Ask to continue or narrow the task."
            )
        } else {
            "(model finished without a text response)".into()
        };
    } else if hit_iteration_limit
        && mode == TurnMode::Parent
        && !state.final_text.contains("tool step limit")
    {
        state.final_text.push_str(&format!(
            "\n\n(Reached the {max_iterations}-step tool limit; partial progress may be in tool output above.)"
        ));
    }

    Ok(state.complete(hit_iteration_limit))
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
    let name = call.function.name.clone();
    let mut ctx = ToolCallContext {
        paths,
        config,
        pool,
        mcp,
        session_id,
        req,
        turn_ctx,
        progress,
        cancel,
        subagent,
        outcome: ToolCallOutcome {
            last_run_id,
            executed,
        },
    };
    let result = dispatch_tool_call(&mut ctx, call).await?;
    let entry = ToolPersistEntry {
        name,
        exit_code: result.exit_code,
        body: truncate_for_persist(&result.body, TOOL_BODY_PERSIST_MAX_CHARS),
    };
    Ok((result.body, entry))
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

    #[test]
    fn parent_turn_offered_tools_structural() {
        use bobaclaw_provider::FunctionSpec;
        assert!(parent_turn_offered_tools(
            TurnMode::Parent,
            &[ToolSpec {
                kind: "function".into(),
                function: FunctionSpec {
                    name: "exec".into(),
                    description: String::new(),
                    parameters: serde_json::json!({}),
                },
            }]
        ));
        assert!(!parent_turn_offered_tools(TurnMode::Parent, &[]));
    }
}
