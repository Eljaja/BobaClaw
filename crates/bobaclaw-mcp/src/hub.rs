use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use bobaclaw_core::{McpServerConfig, McpServers};
use bobaclaw_provider::{FunctionSpec, ToolCall, ToolSpec};
use rmcp::model::{CallToolRequestParams, Tool as McpTool};
use rmcp::service::{RunningService, ServiceExt};
use rmcp::transport::{ConfigureCommandExt, StreamableHttpClientTransport, TokioChildProcess};
use rmcp::RoleClient;
use serde_json::{json, Value};
use tokio::process::Command;
use tokio::sync::Mutex;

use crate::naming::prefixed_tool_name;
use crate::result_format::format_call_tool_result;

const MAX_TOOLS_PER_SERVER: usize = 48;

#[derive(Debug, Clone)]
pub struct McpToolBinding {
    pub prefixed_name: String,
    pub server_name: String,
    pub original_name: String,
    pub spec: ToolSpec,
}

#[derive(Debug, Clone)]
pub struct McpServerStatus {
    pub name: String,
    pub connected: bool,
    pub tool_count: usize,
    pub error: Option<String>,
}

struct McpServerHandle {
    client: Mutex<RunningService<RoleClient, ()>>,
    timeout: Duration,
}

/// Connected MCP servers and their tool catalog.
pub struct McpHub {
    servers: HashMap<String, Arc<McpServerHandle>>,
    bindings: HashMap<String, McpToolBinding>,
}

impl McpHub {
    pub async fn connect(servers: &McpServers) -> Self {
        let mut hub = Self {
            servers: HashMap::new(),
            bindings: HashMap::new(),
        };

        for (name, cfg) in servers {
            if !cfg.enabled {
                tracing::debug!("mcp server '{name}' disabled, skipping");
                continue;
            }
            match hub.connect_one(name, cfg).await {
                Ok(bindings) => {
                    for b in bindings {
                        hub.bindings.insert(b.prefixed_name.clone(), b);
                    }
                }
                Err(e) => {
                    tracing::warn!("mcp server '{name}' failed to connect: {e:#}");
                }
            }
        }

        tracing::info!(
            "mcp: {} server(s), {} tool(s)",
            hub.servers.len(),
            hub.bindings.len()
        );
        hub
    }

    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    pub fn tool_specs(&self) -> Vec<ToolSpec> {
        let mut specs: Vec<_> = self.bindings.values().map(|b| b.spec.clone()).collect();
        specs.sort_by(|a, b| a.function.name.cmp(&b.function.name));
        specs
    }

    pub fn statuses(&self, configured: &McpServers) -> Vec<McpServerStatus> {
        let mut out = Vec::new();
        for (name, cfg) in configured {
            if !cfg.enabled {
                out.push(McpServerStatus {
                    name: name.clone(),
                    connected: false,
                    tool_count: 0,
                    error: Some("disabled".into()),
                });
                continue;
            }
            if let Some(handle) = self.servers.get(name) {
                let count = self
                    .bindings
                    .values()
                    .filter(|b| b.server_name == *name)
                    .count();
                out.push(McpServerStatus {
                    name: name.clone(),
                    connected: true,
                    tool_count: count,
                    error: None,
                });
                let _ = handle;
            } else {
                out.push(McpServerStatus {
                    name: name.clone(),
                    connected: false,
                    tool_count: 0,
                    error: Some("failed to connect at startup (see logs)".into()),
                });
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    pub fn lookup(&self, prefixed_name: &str) -> Option<&McpToolBinding> {
        self.bindings.get(prefixed_name)
    }

    pub async fn call_tool(&self, call: &ToolCall) -> anyhow::Result<String> {
        let binding = self
            .bindings
            .get(&call.function.name)
            .ok_or_else(|| anyhow::anyhow!("unknown MCP tool: {}", call.function.name))?;

        let handle = self
            .servers
            .get(&binding.server_name)
            .ok_or_else(|| anyhow::anyhow!("MCP server '{}' not connected", binding.server_name))?;

        let args: Value = if call.function.arguments.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(&call.function.arguments)
                .map_err(|e| anyhow::anyhow!("invalid MCP tool arguments: {e}"))?
        };

        let args_obj = args
            .as_object()
            .cloned()
            .unwrap_or_default();
        let params = CallToolRequestParams::new(binding.original_name.clone()).with_arguments(args_obj);

        let timeout = handle.timeout;
        let result = {
            let client = handle.client.lock().await;
            tokio::time::timeout(timeout, client.call_tool(params))
                .await
                .map_err(|_| {
                    anyhow::anyhow!(
                        "MCP tool '{}' timed out after {}s",
                        call.function.name,
                        timeout.as_secs()
                    )
                })??
        };

        Ok(format_call_tool_result(&result))
    }

    async fn connect_one(
        &mut self,
        name: &str,
        cfg: &McpServerConfig,
    ) -> anyhow::Result<Vec<McpToolBinding>> {
        let connect_timeout = Duration::from_secs(cfg.connect_timeout_secs.max(5));
        let client = if cfg.uses_http() {
            let url = cfg.url.as_deref().unwrap_or("").trim();
            let transport = StreamableHttpClientTransport::from_uri(url);
            tokio::time::timeout(connect_timeout, ().serve(transport))
                .await
                .map_err(|_| {
                    anyhow::anyhow!(
                        "mcp server '{name}': HTTP connect timed out after {}s (url: {url})",
                        connect_timeout.as_secs()
                    )
                })??
        } else {
            let command = cfg.command.trim();
            if command.is_empty() {
                anyhow::bail!("mcp server '{name}': command is empty (or set url for HTTP MCP)");
            }

            let mut cmd = Command::new(command);
            cmd.args(&cfg.args);
            for (k, v) in cfg.resolve_env() {
                cmd.env(k, v);
            }
            cmd.stdin(std::process::Stdio::null());

            let transport = TokioChildProcess::new(cmd.configure(|c| {
                c.kill_on_drop(true);
            }))?;

            tokio::time::timeout(connect_timeout, ().serve(transport))
                .await
                .map_err(|_| {
                    anyhow::anyhow!(
                        "mcp server '{name}': connect timed out after {}s",
                        connect_timeout.as_secs()
                    )
                })??
        };

        let tools = client.list_all_tools().await?;
        let bindings = register_tools(name, cfg, &tools)?;

        self.servers.insert(
            name.to_string(),
            Arc::new(McpServerHandle {
                client: Mutex::new(client),
                timeout: Duration::from_secs(cfg.timeout_secs.max(5)),
            }),
        );

        Ok(bindings)
    }
}

fn register_tools(
    server_name: &str,
    cfg: &McpServerConfig,
    tools: &[McpTool],
) -> anyhow::Result<Vec<McpToolBinding>> {
    let allow: Option<std::collections::HashSet<_>> = if cfg.tools_allowlist.is_empty() {
        None
    } else {
        Some(cfg.tools_allowlist.iter().cloned().collect())
    };
    let deny: std::collections::HashSet<_> = cfg.tools_denylist.iter().cloned().collect();

    let mut bindings = Vec::new();
    for tool in tools.iter().take(MAX_TOOLS_PER_SERVER) {
        let original = tool.name.as_ref();
        if deny.contains(original) {
            continue;
        }
        if let Some(allow) = &allow {
            if !allow.contains(original) {
                continue;
            }
        }

        let prefixed = prefixed_tool_name(server_name, original);
        let description = tool
            .description
            .as_deref()
            .unwrap_or("MCP tool")
            .to_string();
        let desc = format!("[MCP:{server_name}] {description}");

        let parameters = Value::Object((*tool.input_schema).clone());

        bindings.push(McpToolBinding {
            prefixed_name: prefixed.clone(),
            server_name: server_name.to_string(),
            original_name: original.to_string(),
            spec: ToolSpec {
                kind: "function".into(),
                function: FunctionSpec {
                    name: prefixed,
                    description: desc,
                    parameters,
                },
            },
        });
    }

    if tools.len() > MAX_TOOLS_PER_SERVER {
        tracing::warn!(
            "mcp server '{server_name}': capped at {MAX_TOOLS_PER_SERVER} tools (server advertises {})",
            tools.len()
        );
    }

    Ok(bindings)
}
