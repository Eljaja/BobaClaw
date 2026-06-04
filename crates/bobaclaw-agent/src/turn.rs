use bobaclaw_core::{BobaConfig, BobaPaths, NormalizedRequest};
use bobaclaw_provider::{ConversationMessage, ToolChatClient, ToolSpec};
use bobaclaw_skills::SkillRegistry;
use bobaclaw_state::SessionStore;
use sqlx::SqlitePool;

use crate::compaction::{
    effective_history, history_to_conversation, maybe_compact_session,
};
use crate::progress::{emit, AgentProgress};
use crate::prompt::build_system_prompt;
use crate::tools::{
    exec_tool_spec, handle_exec_tool, handle_schedule_tool, schedule_tool_spec,
};

const MAX_TOOL_ITERATIONS: usize = 16;

pub struct TurnOutcome {
    pub text: String,
    pub session_id: String,
    pub last_run_id: Option<String>,
    pub executed: bool,
}

pub async fn run_agent_turn(
    paths: &BobaPaths,
    config: &BobaConfig,
    pool: &SqlitePool,
    skills: &SkillRegistry,
    session_id: &str,
    req: &NormalizedRequest,
    progress: Option<&dyn AgentProgress>,
) -> anyhow::Result<TurnOutcome> {
    maybe_compact_session(pool, config, session_id, progress).await?;

    let all = SessionStore::new(pool)
        .list_messages(session_id)
        .await
        .map_err(|e| anyhow::anyhow!("не удалось прочитать историю сессии: {e}"))?;
    let history = effective_history(&all);

    let mut messages = vec![ConversationMessage::system(build_system_prompt(
        paths,
        &req.agent_group,
        skills,
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
    let tools: Vec<ToolSpec> = vec![exec_tool_spec(), schedule_tool_spec()];

    let mut last_run_id = None;
    let mut executed = false;
    let mut final_text = String::new();

    for iteration in 1..=MAX_TOOL_ITERATIONS {
        emit(
            progress,
            crate::progress::AgentEvent::LlmThinking {
                iteration: iteration as u32,
            },
        );
        let turn = client
            .chat_turn(&messages, &tools, req.model_override.as_deref())
            .await?;

        let assistant = turn.message.clone();
        let tool_calls = assistant.tool_calls.clone();
        messages.push(assistant);

        match tool_calls {
            Some(calls) if !calls.is_empty() => {
                for call in calls {
                    let body = if call.function.name == "schedule" {
                        handle_schedule_tool(
                            pool,
                            &req.agent_group,
                            session_id,
                            req,
                            &call,
                        )
                        .await?
                    } else {
                        let result = handle_exec_tool(
                            paths,
                            config,
                            &req.agent_group,
                            pool,
                            session_id,
                            &req.request_id.to_string(),
                            &call,
                            progress,
                        )
                        .await?;
                        executed = true;
                        last_run_id = Some(result.run_id);
                        result.body
                    };
                    if call.function.name == "schedule" {
                        executed = true;
                    }
                    messages.push(ConversationMessage::tool_result(call.id.clone(), body));
                }
            }
            _ => {
                final_text = messages
                    .last()
                    .map(|m| m.text_content())
                    .unwrap_or_default();
                break;
            }
        }
    }

    if final_text.trim().is_empty() {
        final_text = "(model finished without a text response)".into();
    }

    Ok(TurnOutcome {
        text: final_text,
        session_id: session_id.to_string(),
        last_run_id,
        executed,
    })
}
