use bobaclaw_core::{ChannelPeer, NormalizedRequest};
use bobaclaw_provider::{FunctionSpec, ToolCall, ToolSpec};
use bobaclaw_state::ScheduledTaskStore;
use chrono::{Duration, Utc};
use serde::Deserialize;
use serde_json::json;
use sqlx::SqlitePool;

const TOOL_NAME: &str = "schedule";
const MAX_DELAY_SECS: u64 = 7 * 24 * 3600;

pub fn schedule_tool_spec() -> ToolSpec {
    ToolSpec {
        kind: "function".into(),
        function: FunctionSpec {
            name: TOOL_NAME.into(),
            description: "Schedule a one-shot task for the future. At run_at the agent runs `prompt` and \
                delivers the result to the user (Telegram if this chat is Telegram, otherwise CLI outbox). \
                Use for reminders, delayed messages, and \"do X in N minutes\". Do not claim you cannot schedule."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "delay_seconds": {
                        "type": "integer",
                        "description": "Seconds from now until the task runs (1..604800)."
                    },
                    "prompt": {
                        "type": "string",
                        "description": "What the agent should do when the task fires (may include the exact user message to send)."
                    },
                    "deliver_message": {
                        "type": "string",
                        "description": "Optional fixed text to deliver to the user. If omitted, the agent reply at fire time is delivered."
                    }
                },
                "required": ["delay_seconds", "prompt"]
            }),
        },
    }
}

#[derive(Debug, Deserialize)]
struct ScheduleArgs {
    delay_seconds: u64,
    prompt: String,
    #[serde(default)]
    deliver_message: Option<String>,
}

pub async fn handle_schedule_tool(
    pool: &SqlitePool,
    agent_group: &str,
    session_id: &str,
    req: &NormalizedRequest,
    call: &ToolCall,
) -> anyhow::Result<String> {
    if call.function.name != TOOL_NAME {
        anyhow::bail!("unknown tool: {}", call.function.name);
    }

    let args: ScheduleArgs = serde_json::from_str(&call.function.arguments)
        .map_err(|e| anyhow::anyhow!("invalid schedule arguments: {e}"))?;

    if args.delay_seconds == 0 || args.delay_seconds > MAX_DELAY_SECS {
        anyhow::bail!("delay_seconds must be 1..{MAX_DELAY_SECS}");
    }
    let prompt = args.prompt.trim();
    if prompt.is_empty() {
        anyhow::bail!("prompt is empty");
    }

    let run_at = (Utc::now() + Duration::seconds(args.delay_seconds as i64)).timestamp_millis()
        as f64
        / 1000.0;

    let (deliver_channel, deliver_peer) = deliver_target(req);

    let store = ScheduledTaskStore::new(pool);
    let task = store
        .insert(
            agent_group,
            prompt,
            args.deliver_message.as_deref(),
            run_at,
            deliver_channel.as_deref(),
            deliver_peer.as_deref(),
            Some(session_id),
        )
        .await?;

    let when = chrono::DateTime::from_timestamp(run_at as i64, 0)
        .map(|t| t.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| run_at.to_string());

    Ok(format!(
        "scheduled task {} at {when} (in {}s); deliver via {} peer={}; prompt={}",
        task.id,
        args.delay_seconds,
        deliver_channel.unwrap_or_else(|| "cli".into()),
        deliver_peer.unwrap_or_else(|| "-".into()),
        truncate(&task.prompt, 120),
    ))
}

fn deliver_target(req: &NormalizedRequest) -> (Option<String>, Option<String>) {
    if let Some(ChannelPeer { channel, peer, .. }) = &req.channel_peer {
        return (Some(channel.clone()), Some(peer.clone()));
    }
    match req.ingress {
        bobaclaw_core::IngressKind::Telegram => (Some("telegram".into()), None),
        _ => (Some("cli".into()), None),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}
