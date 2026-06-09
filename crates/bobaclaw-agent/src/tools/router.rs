use std::sync::Arc;

use async_trait::async_trait;
use bobaclaw_core::{BobaConfig, BobaPaths, NormalizedRequest, TurnInterrupted};
use bobaclaw_mcp::McpHub;
use bobaclaw_provider::ToolCall;
use sqlx::SqlitePool;
use tokio_util::sync::CancellationToken;

use crate::cancel::check_cancel;
use crate::progress::AgentProgress;
use crate::subagent::SubagentManager;
use crate::turn_context::TurnContext;

use super::{
    handle_exec_tool, handle_mcp_tool, handle_memory_tool, handle_schedule_tool, handle_skill_tool,
    handle_spawn_status_tool, handle_spawn_tool, handle_subagent_tool, is_mcp_tool, is_memory_tool,
    is_skill_tool, is_spawn_status_tool, is_spawn_tool, is_subagent_tool,
};

pub struct ToolCallResult {
    pub body: String,
    pub exit_code: i32,
}

/// Mutable per-turn state updated by tool handlers.
pub struct ToolCallOutcome<'a> {
    pub last_run_id: &'a mut Option<String>,
    pub executed: &'a mut bool,
}

/// Shared inputs for dispatching a single tool call.
pub struct ToolCallContext<'a> {
    pub paths: &'a BobaPaths,
    pub config: &'a BobaConfig,
    pub pool: &'a SqlitePool,
    pub mcp: Option<&'a Arc<McpHub>>,
    pub session_id: &'a str,
    pub req: &'a NormalizedRequest,
    pub turn_ctx: &'a TurnContext,
    pub progress: Option<&'a dyn AgentProgress>,
    pub cancel: &'a CancellationToken,
    pub subagent: Option<&'a SubagentManager>,
    pub outcome: ToolCallOutcome<'a>,
}

#[async_trait]
trait ToolHandler: Send + Sync {
    fn matches(&self, name: &str, ctx: &ToolCallContext<'_>) -> bool;
    async fn handle(
        &self,
        ctx: &mut ToolCallContext<'_>,
        call: &ToolCall,
    ) -> anyhow::Result<ToolCallResult>;
}

struct SubagentHandler;
struct SpawnHandler;
struct SpawnStatusHandler;
struct ScheduleHandler;
struct McpHandler;
struct ExecHandler;
struct SkillHandler;
struct MemoryHandler;

#[async_trait]
impl ToolHandler for SubagentHandler {
    fn matches(&self, name: &str, _ctx: &ToolCallContext<'_>) -> bool {
        is_subagent_tool(name)
    }

    async fn handle(
        &self,
        ctx: &mut ToolCallContext<'_>,
        call: &ToolCall,
    ) -> anyhow::Result<ToolCallResult> {
        let manager = ctx
            .subagent
            .ok_or_else(|| anyhow::anyhow!("subagent manager not configured"))?;
        let delegation_ctx = ctx
            .turn_ctx
            .for_delegation(ctx.outcome.last_run_id.as_deref());
        let body = handle_subagent_tool(
            ctx.paths,
            ctx.config,
            ctx.pool,
            ctx.mcp,
            ctx.session_id,
            ctx.req,
            &delegation_ctx,
            call,
            ctx.progress,
            ctx.cancel,
            manager,
        )
        .await?;
        *ctx.outcome.executed = true;
        Ok(ToolCallResult {
            body: body.body,
            exit_code: body.exit_code,
        })
    }
}

#[async_trait]
impl ToolHandler for SpawnHandler {
    fn matches(&self, name: &str, _ctx: &ToolCallContext<'_>) -> bool {
        is_spawn_tool(name)
    }

    async fn handle(
        &self,
        ctx: &mut ToolCallContext<'_>,
        call: &ToolCall,
    ) -> anyhow::Result<ToolCallResult> {
        let manager = ctx
            .subagent
            .ok_or_else(|| anyhow::anyhow!("subagent manager not configured"))?;
        let delegation_ctx = ctx
            .turn_ctx
            .for_delegation(ctx.outcome.last_run_id.as_deref());
        let body = handle_spawn_tool(
            ctx.paths,
            ctx.config,
            ctx.pool,
            ctx.mcp,
            ctx.session_id,
            ctx.req,
            &delegation_ctx,
            call,
            ctx.progress,
            ctx.cancel,
            manager,
        )
        .await?;
        *ctx.outcome.executed = true;
        Ok(ToolCallResult {
            body: body.body,
            exit_code: body.exit_code,
        })
    }
}

#[async_trait]
impl ToolHandler for SpawnStatusHandler {
    fn matches(&self, name: &str, _ctx: &ToolCallContext<'_>) -> bool {
        is_spawn_status_tool(name)
    }

    async fn handle(
        &self,
        ctx: &mut ToolCallContext<'_>,
        call: &ToolCall,
    ) -> anyhow::Result<ToolCallResult> {
        let body =
            handle_spawn_status_tool(ctx.paths, ctx.config, ctx.pool, ctx.session_id, call).await?;
        *ctx.outcome.executed = true;
        Ok(ToolCallResult {
            body: body.body,
            exit_code: body.exit_code,
        })
    }
}

#[async_trait]
impl ToolHandler for ScheduleHandler {
    fn matches(&self, name: &str, _ctx: &ToolCallContext<'_>) -> bool {
        matches!(
            name,
            "schedule" | "schedule_recurring" | "schedule_list" | "schedule_cancel"
        )
    }

    async fn handle(
        &self,
        ctx: &mut ToolCallContext<'_>,
        call: &ToolCall,
    ) -> anyhow::Result<ToolCallResult> {
        let body = handle_schedule_tool(
            ctx.pool,
            &ctx.req.agent_group,
            ctx.session_id,
            ctx.req,
            call,
        )
        .await?;
        *ctx.outcome.executed = true;
        Ok(ToolCallResult { body, exit_code: 0 })
    }
}

#[async_trait]
impl ToolHandler for McpHandler {
    fn matches(&self, name: &str, ctx: &ToolCallContext<'_>) -> bool {
        is_mcp_tool(ctx.mcp, name)
    }

    async fn handle(
        &self,
        ctx: &mut ToolCallContext<'_>,
        call: &ToolCall,
    ) -> anyhow::Result<ToolCallResult> {
        let hub = ctx.mcp.expect("mcp hub required for mcp_* tools");
        let body = handle_mcp_tool(hub, call, ctx.progress).await?;
        *ctx.outcome.executed = true;
        let exit_code = if body.starts_with("MCP error:") { 1 } else { 0 };
        Ok(ToolCallResult { body, exit_code })
    }
}

#[async_trait]
impl ToolHandler for ExecHandler {
    fn matches(&self, name: &str, _ctx: &ToolCallContext<'_>) -> bool {
        name == "exec"
    }

    async fn handle(
        &self,
        ctx: &mut ToolCallContext<'_>,
        call: &ToolCall,
    ) -> anyhow::Result<ToolCallResult> {
        let result = handle_exec_tool(
            ctx.paths,
            ctx.config,
            &ctx.req.agent_group,
            ctx.pool,
            ctx.session_id,
            &ctx.req.request_id.to_string(),
            call,
            ctx.progress,
            ctx.cancel,
        )
        .await?;
        *ctx.outcome.executed = true;
        if !result.run_id.is_empty() {
            *ctx.outcome.last_run_id = Some(result.run_id);
        }
        Ok(ToolCallResult {
            body: result.body,
            exit_code: result.exit_code,
        })
    }
}

#[async_trait]
impl ToolHandler for SkillHandler {
    fn matches(&self, name: &str, _ctx: &ToolCallContext<'_>) -> bool {
        is_skill_tool(name)
    }

    async fn handle(
        &self,
        ctx: &mut ToolCallContext<'_>,
        call: &ToolCall,
    ) -> anyhow::Result<ToolCallResult> {
        let body = handle_skill_tool(ctx.paths, &ctx.req.agent_group, call)?;
        *ctx.outcome.executed = true;
        Ok(ToolCallResult { body, exit_code: 0 })
    }
}

#[async_trait]
impl ToolHandler for MemoryHandler {
    fn matches(&self, name: &str, _ctx: &ToolCallContext<'_>) -> bool {
        is_memory_tool(name)
    }

    async fn handle(
        &self,
        ctx: &mut ToolCallContext<'_>,
        call: &ToolCall,
    ) -> anyhow::Result<ToolCallResult> {
        let body = handle_memory_tool(ctx.paths, &ctx.req.agent_group, call)?;
        *ctx.outcome.executed = true;
        Ok(ToolCallResult { body, exit_code: 0 })
    }
}

fn handlers() -> &'static Vec<Box<dyn ToolHandler>> {
    static HANDLERS: std::sync::OnceLock<Vec<Box<dyn ToolHandler>>> = std::sync::OnceLock::new();
    HANDLERS.get_or_init(|| {
        vec![
            Box::new(SubagentHandler),
            Box::new(SpawnHandler),
            Box::new(SpawnStatusHandler),
            Box::new(ScheduleHandler),
            Box::new(McpHandler),
            Box::new(ExecHandler),
            Box::new(SkillHandler),
            Box::new(MemoryHandler),
        ]
    })
}

pub(crate) async fn dispatch_tool_call(
    ctx: &mut ToolCallContext<'_>,
    call: &ToolCall,
) -> anyhow::Result<ToolCallResult> {
    if let Err(TurnInterrupted) = check_cancel(ctx.cancel) {
        return Err(TurnInterrupted.into());
    }
    let name = call.function.name.as_str();
    for handler in handlers() {
        if handler.matches(name, ctx) {
            return handler.handle(ctx, call).await;
        }
    }
    anyhow::bail!("unknown tool: {name}");
}
