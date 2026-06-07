//! Benchmark Obscura MCP via the same path as BobaClaw agent.
//! Run: cargo run -p bobaclaw-mcp --example bench_obscura

use std::time::{Duration, Instant};

use bobaclaw_core::{BobaConfig, BobaPaths};
use bobaclaw_mcp::McpHub;
use bobaclaw_provider::ToolCall;
use serde_json::json;

fn bench_url() -> String {
    std::env::var("MCP_BENCH_URL").unwrap_or_else(|_| "https://ya.ru".into())
}

fn bench_wait_until() -> Option<String> {
    std::env::var("MCP_BENCH_WAIT_UNTIL")
        .ok()
        .filter(|s| !s.is_empty())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let url = bench_url();
    let paths = BobaPaths::resolve()?;
    let config = BobaConfig::load(&paths.config)?;
    let obscura = config
        .mcp_servers
        .get("obscura")
        .ok_or_else(|| anyhow::anyhow!("mcp_servers.obscura not configured"))?;

    let servers = [("obscura".to_string(), obscura.clone())]
        .into_iter()
        .collect();

    print!("connect … ");
    let t0 = Instant::now();
    let hub = McpHub::connect(&servers).await;
    let connect_ms = t0.elapsed().as_millis();
    println!("{connect_ms} ms ({} tools)", hub.tool_specs().len());

    let tool_name = hub
        .tool_specs()
        .iter()
        .find(|t| t.function.name.contains("navigate"))
        .map(|t| t.function.name.clone())
        .ok_or_else(|| anyhow::anyhow!("no browser_navigate tool"))?;

    let mut nav_args = serde_json::Map::new();
    nav_args.insert("url".into(), json!(url));
    if let Some(wait) = bench_wait_until() {
        nav_args.insert("waitUntil".into(), json!(wait));
    }
    let call: ToolCall = serde_json::from_value(json!({
        "id": "bench-1",
        "type": "function",
        "function": {
            "name": tool_name,
            "arguments": serde_json::to_string(&nav_args)?
        }
    }))?;

    print!("browser_navigate({url}) … ");
    let t1 = Instant::now();
    let nav_cap = std::env::var("MCP_BENCH_NAV_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(180);
    let nav = tokio::time::timeout(Duration::from_secs(nav_cap), hub.call_tool(&call)).await;
    let nav_ms = t1.elapsed().as_millis();
    match nav {
        Ok(Ok(body)) => {
            let preview: String = body.chars().take(120).collect();
            println!("{nav_ms} ms OK — {preview}…");
        }
        Ok(Err(e)) => println!("{nav_ms} ms ERR — {e:#}"),
        Err(_) => println!("{nav_ms} ms TIMEOUT (180s)"),
    }

    let snap_name = hub
        .tool_specs()
        .iter()
        .find(|t| t.function.name.contains("snapshot"))
        .map(|t| t.function.name.clone());

    if let Some(snap_name) = snap_name {
        let call: ToolCall = serde_json::from_value(json!({
            "id": "bench-2",
            "type": "function",
            "function": {
                "name": snap_name,
                "arguments": "{}"
            }
        }))?;
        print!("browser_snapshot … ");
        let t2 = Instant::now();
        let snap = tokio::time::timeout(Duration::from_secs(60), hub.call_tool(&call)).await;
        let snap_ms = t2.elapsed().as_millis();
        match snap {
            Ok(Ok(body)) => {
                let preview: String = body.chars().take(120).collect();
                println!("{snap_ms} ms OK — {preview}…");
            }
            Ok(Err(e)) => println!("{snap_ms} ms ERR — {e:#}"),
            Err(_) => println!("{snap_ms} ms TIMEOUT (60s)"),
        }
    }

    Ok(())
}
