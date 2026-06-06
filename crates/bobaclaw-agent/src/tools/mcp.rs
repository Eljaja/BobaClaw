use std::sync::Arc;

use bobaclaw_mcp::McpHub;
use bobaclaw_provider::ToolCall;

use crate::progress::{emit, sanitize_status_text, AgentEvent, AgentProgress};

pub async fn handle_mcp_tool(
    hub: &McpHub,
    call: &ToolCall,
    progress: Option<&dyn AgentProgress>,
) -> anyhow::Result<String> {
    let label = sanitize_status_text(&call.function.name, 48);
    emit(
        progress,
        AgentEvent::ToolStart {
            name: call.function.name.clone(),
            label,
        },
    );

    let result = hub.call_tool(call).await;
    let (body, exit_code) = match &result {
        Ok(text) => (text.clone(), 0),
        Err(e) => (format!("MCP error: {e:#}"), 1),
    };

    let preview = sanitize_status_text(&body, 120);
    emit(
        progress,
        AgentEvent::ToolEnd {
            name: call.function.name.clone(),
            exit_code,
            preview,
        },
    );

    result.map(|_| body)
}

pub fn is_mcp_tool(hub: Option<&Arc<McpHub>>, name: &str) -> bool {
    hub.and_then(|h| h.lookup(name)).is_some()
}
