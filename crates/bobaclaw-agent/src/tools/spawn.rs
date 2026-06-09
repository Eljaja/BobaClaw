use bobaclaw_core::{BobaConfig, BobaPaths, NormalizedRequest};
use bobaclaw_mcp::McpHub;
use bobaclaw_provider::{FunctionSpec, ToolCall, ToolSpec};
use serde::Deserialize;
use serde_json::json;
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use crate::subagent::SubagentManager;
use crate::turn_context::TurnContext;

pub const SPAWN: &str = "spawn";

pub fn spawn_tool_spec() -> ToolSpec {
    ToolSpec {
        kind: "function".into(),
        function: FunctionSpec {
            name: SPAWN.into(),
            description: "Spawn a subagent in the background (fire-and-forget). \
                Use when the task is long-running and the parent can continue without waiting. \
                Result is appended to the session when complete. \
                Same task/context/preset/backend params as subagent."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "task": { "type": "string" },
                    "label": { "type": "string" },
                    "context": { "type": "string" },
                    "preset": { "type": "string" },
                    "backend": { "type": "string" },
                    "wake": { "type": "boolean", "description": "Wake parent on completion (default from config)" }
                },
                "required": ["task"]
            }),
        },
    }
}

pub fn is_spawn_tool(name: &str) -> bool {
    name == SPAWN
}

#[derive(Debug, Deserialize)]
struct SpawnArgs {
    task: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    context: Option<String>,
    #[serde(default)]
    preset: Option<String>,
    #[serde(default)]
    backend: Option<String>,
    #[serde(default)]
    wake: Option<bool>,
}

pub struct SpawnToolResult {
    pub body: String,
    pub exit_code: i32,
}

#[allow(clippy::too_many_arguments)]
pub async fn handle_spawn_tool(
    _paths: &BobaPaths,
    config: &BobaConfig,
    pool: &SqlitePool,
    mcp: Option<&Arc<McpHub>>,
    session_id: &str,
    req: &NormalizedRequest,
    turn_ctx: &TurnContext,
    call: &ToolCall,
    _progress: Option<&dyn crate::progress::AgentProgress>,
    cancel: &CancellationToken,
    manager: &SubagentManager,
) -> anyhow::Result<SpawnToolResult> {
    if call.function.name != SPAWN {
        anyhow::bail!("unknown tool: {}", call.function.name);
    }
    let args: SpawnArgs = match serde_json::from_str(&call.function.arguments) {
        Ok(a) => a,
        Err(e) => {
            return Ok(SpawnToolResult {
                body: format!("invalid spawn arguments: {e}"),
                exit_code: 1,
            });
        }
    };
    let task = args.task.trim();
    if task.is_empty() {
        return Ok(SpawnToolResult {
            body: "spawn: task is required".into(),
            exit_code: 1,
        });
    }
    if !config.subagents.enabled {
        return Ok(SpawnToolResult {
            body: "spawn: subagents are disabled in config".into(),
            exit_code: 1,
        });
    }

    let hub = match mcp {
        Some(h) => h.clone(),
        None => Arc::new(McpHub::connect(&config.mcp_servers).await),
    };
    let wake_parent = args
        .wake
        .unwrap_or(config.subagents.spawn.wake_parent_on_complete);
    let msg = manager
        .spawn_async(
            Arc::new(pool.clone()),
            hub,
            session_id.to_string(),
            req.clone(),
            turn_ctx.clone(),
            task.to_string(),
            args.label,
            args.context,
            args.preset,
            args.backend,
            wake_parent,
            cancel.clone(),
        )
        .await?;

    Ok(SpawnToolResult {
        body: msg,
        exit_code: 0,
    })
}
