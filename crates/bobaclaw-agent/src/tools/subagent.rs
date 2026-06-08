use bobaclaw_core::{BobaConfig, BobaPaths, NormalizedRequest};
use bobaclaw_mcp::McpHub;
use bobaclaw_provider::{FunctionSpec, ToolCall, ToolSpec};
use serde::Deserialize;
use serde_json::json;
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use crate::progress::AgentProgress;
use crate::subagent::SubagentManager;
use crate::turn_context::TurnContext;

pub const SUBAGENT: &str = "subagent";

pub fn subagent_tool_spec() -> ToolSpec {
    ToolSpec {
        kind: "function".into(),
        function: FunctionSpec {
            name: SUBAGENT.into(),
            description: "Delegate a focused subtask to an isolated agent loop with fresh context. \
                Use for multi-step research, many files, or work that benefits from context quarantine — \
                not for one-liner questions or a single exec/MCP call. \
                Write a self-contained task (goal, scope, expected output). \
                Optional context for snippets the child cannot infer from parent history."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "task": {
                        "type": "string",
                        "description": "Self-contained subtask for the subagent."
                    },
                    "label": {
                        "type": "string",
                        "description": "Optional short label for logs."
                    },
                    "context": {
                        "type": "string",
                        "description": "Optional parent context the subagent cannot see otherwise."
                    },
                    "preset": {
                        "type": "string",
                        "description": "Optional preset id from config subagents.presets."
                    },
                    "backend": {
                        "type": "string",
                        "description": "Backend: native (default), claude-code, codex, cursor."
                    }
                },
                "required": ["task"]
            }),
        },
    }
}

pub fn is_subagent_tool(name: &str) -> bool {
    name == SUBAGENT
}

#[derive(Debug, Deserialize)]
struct SubagentArgs {
    task: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    context: Option<String>,
    #[serde(default)]
    preset: Option<String>,
    #[serde(default)]
    backend: Option<String>,
}

pub struct SubagentToolResult {
    pub body: String,
    pub exit_code: i32,
}

#[allow(clippy::too_many_arguments)]
pub async fn handle_subagent_tool(
    _paths: &BobaPaths,
    config: &BobaConfig,
    pool: &SqlitePool,
    mcp: Option<&Arc<McpHub>>,
    session_id: &str,
    req: &NormalizedRequest,
    turn_ctx: &TurnContext,
    call: &ToolCall,
    progress: Option<&dyn AgentProgress>,
    cancel: &CancellationToken,
    manager: &SubagentManager,
) -> anyhow::Result<SubagentToolResult> {
    if call.function.name != SUBAGENT {
        anyhow::bail!("unknown tool: {}", call.function.name);
    }
    let args: SubagentArgs = match serde_json::from_str(&call.function.arguments) {
        Ok(a) => a,
        Err(e) => {
            return Ok(SubagentToolResult {
                body: format!("invalid subagent arguments: {e}"),
                exit_code: 1,
            });
        }
    };
    let task = args.task.trim();
    if task.is_empty() {
        return Ok(SubagentToolResult {
            body: "subagent: task is required and must be non-empty".into(),
            exit_code: 1,
        });
    }
    if let Some(preset) = args.preset.as_deref().filter(|s| !s.trim().is_empty()) {
        if config.subagents.preset(preset).is_none() {
            return Ok(SubagentToolResult {
                body: format!("unknown subagent preset: {preset}"),
                exit_code: 1,
            });
        }
    }

    if config.subagents.persist_child_sessions {
        let child_id = format!("subagent_sess_{}", uuid::Uuid::new_v4());
        let now = chrono::Utc::now().timestamp_millis() as f64 / 1000.0;
        sqlx::query(
            "INSERT INTO sessions (id, source, agent_group, parent_session_id, started_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .bind(&child_id)
        .bind("subagent")
        .bind(&req.agent_group)
        .bind(session_id)
        .bind(now)
        .execute(pool)
        .await
        .ok();
    }

    let result = manager
        .run_sync(
            pool,
            mcp,
            session_id,
            req,
            turn_ctx,
            task,
            args.label.as_deref(),
            args.context.as_deref(),
            args.preset.as_deref(),
            args.backend.as_deref(),
            progress,
            cancel,
        )
        .await?;

    Ok(SubagentToolResult {
        body: format!(
            "subagent_id={}\nexit_code={}\n\n{}",
            result.subagent_id, result.exit_code, result.body
        ),
        exit_code: result.exit_code,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_subagent_tool_matches() {
        assert!(is_subagent_tool("subagent"));
        assert!(!is_subagent_tool("spawn"));
    }
}
