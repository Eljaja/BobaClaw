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
    /// Set for Docker stdio MCP; stopped on drop so `/obscura mcp` does not leak.
    docker_container: Option<String>,
}

impl Drop for McpServerHandle {
    fn drop(&mut self) {
        if let Some(name) = self.docker_container.take() {
            crate::docker_stdio::stop_mcp_container(&name);
        }
    }
}

/// Connected MCP servers and their tool catalog.
pub struct McpHub {
    configs: McpServers,
    servers: Mutex<HashMap<String, Arc<McpServerHandle>>>,
    bindings: HashMap<String, McpToolBinding>,
}

impl McpHub {
    pub async fn connect(servers: &McpServers) -> Self {
        crate::docker_stdio::cleanup_stale_mcp_containers();

        let hub_servers = Mutex::new(HashMap::new());
        let mut bindings = HashMap::new();

        for (name, cfg) in servers {
            if !cfg.enabled {
                tracing::debug!("mcp server '{name}' disabled, skipping");
                continue;
            }
            match connect_server(name, cfg).await {
                Ok((handle, server_bindings)) => {
                    hub_servers.lock().await.insert(name.clone(), handle);
                    for b in server_bindings {
                        bindings.insert(b.prefixed_name.clone(), b);
                    }
                }
                Err(e) => {
                    tracing::warn!("mcp server '{name}' failed to connect: {e:#}");
                }
            }
        }

        let connected = hub_servers.lock().await.len();
        tracing::info!("mcp: {connected} server(s), {} tool(s)", bindings.len());

        Self {
            configs: servers.clone(),
            servers: hub_servers,
            bindings,
        }
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
        let connected_servers = self
            .servers
            .try_lock()
            .map(|s| s.keys().cloned().collect::<std::collections::HashSet<_>>())
            .unwrap_or_default();

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
            if connected_servers.contains(name) {
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

        match self.invoke_tool(binding, call).await {
            Ok(text) => Ok(text),
            Err(e) if is_transport_closed(&e) => {
                tracing::warn!(
                    "mcp server '{}' transport closed; reconnecting and retrying once",
                    binding.server_name
                );
                self.reconnect_server(&binding.server_name).await?;
                self.invoke_tool(binding, call).await
            }
            Err(e) => Err(e),
        }
    }

    async fn reconnect_server(&self, name: &str) -> anyhow::Result<()> {
        let cfg = self
            .configs
            .get(name)
            .filter(|c| c.enabled)
            .ok_or_else(|| anyhow::anyhow!("MCP server '{name}' is not configured or disabled"))?;

        let (handle, _) = connect_server(name, cfg).await?;
        self.servers.lock().await.insert(name.to_string(), handle);
        Ok(())
    }

    async fn invoke_tool(
        &self,
        binding: &McpToolBinding,
        call: &ToolCall,
    ) -> anyhow::Result<String> {
        let handle = self
            .servers
            .lock()
            .await
            .get(&binding.server_name)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("MCP server '{}' not connected", binding.server_name))?;

        let args: Value = if call.function.arguments.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(&call.function.arguments)
                .map_err(|e| anyhow::anyhow!("invalid MCP tool arguments: {e}"))?
        };

        let mut args_obj = args.as_object().cloned().unwrap_or_default();
        normalize_browser_tool_args(&binding.original_name, &mut args_obj);
        let params =
            CallToolRequestParams::new(binding.original_name.clone()).with_arguments(args_obj);

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
}

async fn connect_server(
    name: &str,
    cfg: &McpServerConfig,
) -> anyhow::Result<(Arc<McpServerHandle>, Vec<McpToolBinding>)> {
    let connect_timeout = Duration::from_secs(cfg.connect_timeout_secs.max(5));
    let (client, docker_container) = if cfg.uses_http() {
        let url = cfg.url.as_deref().unwrap_or("").trim();
        let transport = StreamableHttpClientTransport::from_uri(url);
        let client = tokio::time::timeout(connect_timeout, ().serve(transport))
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "mcp server '{name}': HTTP connect timed out after {}s (url: {url})",
                    connect_timeout.as_secs()
                )
            })??;
        (client, None)
    } else {
        let command = cfg.command.trim();
        if command.is_empty() {
            anyhow::bail!("mcp server '{name}': command is empty (or set url for HTTP MCP)");
        }

        let (mut cmd, docker_container) =
            if let Some(prepared) = crate::docker_stdio::prepare_stdio_command(name, cfg)? {
                (prepared.command, Some(prepared.container_name))
            } else {
                let mut cmd = Command::new(command);
                cmd.args(&cfg.args);
                (cmd, None)
            };

        for (k, v) in cfg.resolve_env() {
            cmd.env(k, v);
        }
        cmd.stdin(std::process::Stdio::null());

        let transport = TokioChildProcess::new(cmd.configure(|c| {
            c.kill_on_drop(true);
        }))?;

        let client = match tokio::time::timeout(connect_timeout, ().serve(transport)).await {
            Ok(Ok(client)) => client,
            Ok(Err(e)) => {
                if let Some(ref c) = docker_container {
                    crate::docker_stdio::stop_mcp_container(c);
                }
                return Err(e.into());
            }
            Err(_) => {
                if let Some(ref c) = docker_container {
                    crate::docker_stdio::stop_mcp_container(c);
                }
                anyhow::bail!(
                    "mcp server '{name}': connect timed out after {}s",
                    connect_timeout.as_secs()
                );
            }
        };
        (client, docker_container)
    };

    let tools = client.list_all_tools().await?;
    let bindings = register_tools(name, cfg, &tools)?;

    let handle = Arc::new(McpServerHandle {
        client: Mutex::new(client),
        timeout: Duration::from_secs(cfg.timeout_secs.max(5)),
        docker_container,
    });

    Ok((handle, bindings))
}

fn is_transport_closed(err: &anyhow::Error) -> bool {
    err.to_string().contains("Transport closed")
}

/// `browser_navigate` defaults to `waitUntil: load`, which never fires on heavy pages
/// (ya.ru, news sites with endless ads). DOM-ready is enough for snapshot/scrape.
fn normalize_browser_tool_args(tool: &str, args: &mut serde_json::Map<String, Value>) {
    if tool == "browser_navigate" && !args.contains_key("waitUntil") {
        args.insert("waitUntil".into(), json!("domcontentloaded"));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigate_defaults_to_domcontentloaded() {
        let mut args = serde_json::Map::new();
        args.insert("url".into(), json!("https://ya.ru"));
        normalize_browser_tool_args("browser_navigate", &mut args);
        assert_eq!(
            args.get("waitUntil").and_then(|v| v.as_str()),
            Some("domcontentloaded")
        );
    }

    #[test]
    fn navigate_respects_explicit_wait_until() {
        let mut args = serde_json::Map::new();
        args.insert("url".into(), json!("https://ya.ru"));
        args.insert("waitUntil".into(), json!("load"));
        normalize_browser_tool_args("browser_navigate", &mut args);
        assert_eq!(args.get("waitUntil").and_then(|v| v.as_str()), Some("load"));
    }
}
