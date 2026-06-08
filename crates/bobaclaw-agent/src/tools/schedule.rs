use bobaclaw_core::{ChannelPeer, NormalizedRequest};
use bobaclaw_provider::{FunctionSpec, ToolCall, ToolSpec};
use bobaclaw_state::{CronStore, ScheduledTaskStore};
use chrono::{Duration, Utc};
use cron::Schedule;
use serde::Deserialize;
use serde_json::json;
use sqlx::SqlitePool;
use std::str::FromStr;
use uuid::Uuid;

const TOOL_SCHEDULE: &str = "schedule";
const TOOL_SCHEDULE_RECURRING: &str = "schedule_recurring";
const TOOL_SCHEDULE_LIST: &str = "schedule_list";
const TOOL_SCHEDULE_CANCEL: &str = "schedule_cancel";
const MAX_DELAY_SECS: u64 = 7 * 24 * 3600;

pub fn schedule_tool_specs() -> Vec<ToolSpec> {
    vec![
        one_shot_spec(),
        recurring_spec(),
        list_spec(),
        cancel_spec(),
    ]
}

fn one_shot_spec() -> ToolSpec {
    ToolSpec {
        kind: "function".into(),
        function: FunctionSpec {
            name: TOOL_SCHEDULE.into(),
            description: "Schedule a one-shot task for the future. At run_at the agent runs `prompt` and \
                delivers the result to the user (Telegram if this chat is Telegram, otherwise CLI outbox). \
                For simple reminders set `deliver_message` to the exact text — no LLM call at fire time. \
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
                        "description": "Optional fixed text to deliver to the user. If set, delivered directly without an LLM turn."
                    }
                },
                "required": ["delay_seconds", "prompt"]
            }),
        },
    }
}

fn recurring_spec() -> ToolSpec {
    ToolSpec {
        kind: "function".into(),
        function: FunctionSpec {
            name: TOOL_SCHEDULE_RECURRING.into(),
            description:
                "Create a recurring cron job. Standard 5-field cron: \"min hour dom month dow\" \
                (e.g. \"0 9 * * 1\" = Mondays 09:00 UTC, \"*/5 * * * *\" = every 5 minutes). \
                At each fire the agent runs `prompt` and delivers to this chat. \
                Use `schedule_list` / `schedule_cancel` to manage jobs."
                    .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "cron": {
                        "type": "string",
                        "description": "5-field cron expression (min hour dom month dow)."
                    },
                    "prompt": {
                        "type": "string",
                        "description": "What the agent should do on each run."
                    },
                    "deliver_message": {
                        "type": "string",
                        "description": "Optional fixed text to deliver each run (skips LLM when set)."
                    },
                    "job_id": {
                        "type": "string",
                        "description": "Optional stable id (alphanumeric + dash). Auto-generated if omitted."
                    }
                },
                "required": ["cron", "prompt"]
            }),
        },
    }
}

fn list_spec() -> ToolSpec {
    ToolSpec {
        kind: "function".into(),
        function: FunctionSpec {
            name: TOOL_SCHEDULE_LIST.into(),
            description: "List pending one-shot scheduled tasks and active recurring cron jobs."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
    }
}

fn cancel_spec() -> ToolSpec {
    ToolSpec {
        kind: "function".into(),
        function: FunctionSpec {
            name: TOOL_SCHEDULE_CANCEL.into(),
            description:
                "Cancel a pending one-shot task (sched_…) or disable a recurring job (cron_…)."
                    .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Task or job id from schedule / schedule_recurring / schedule_list."
                    }
                },
                "required": ["id"]
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

#[derive(Debug, Deserialize)]
struct RecurringArgs {
    cron: String,
    prompt: String,
    #[serde(default)]
    deliver_message: Option<String>,
    #[serde(default)]
    job_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CancelArgs {
    id: String,
}

pub async fn handle_schedule_tool(
    pool: &SqlitePool,
    agent_group: &str,
    session_id: &str,
    req: &NormalizedRequest,
    call: &ToolCall,
) -> anyhow::Result<String> {
    match call.function.name.as_str() {
        TOOL_SCHEDULE => handle_one_shot(pool, agent_group, session_id, req, call).await,
        TOOL_SCHEDULE_RECURRING => handle_recurring(pool, agent_group, session_id, req, call).await,
        TOOL_SCHEDULE_LIST => handle_list(pool).await,
        TOOL_SCHEDULE_CANCEL => handle_cancel(pool, call).await,
        other => anyhow::bail!("unknown schedule tool: {other}"),
    }
}

async fn handle_one_shot(
    pool: &SqlitePool,
    agent_group: &str,
    session_id: &str,
    req: &NormalizedRequest,
    call: &ToolCall,
) -> anyhow::Result<String> {
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

async fn handle_recurring(
    pool: &SqlitePool,
    agent_group: &str,
    session_id: &str,
    req: &NormalizedRequest,
    call: &ToolCall,
) -> anyhow::Result<String> {
    let args: RecurringArgs = serde_json::from_str(&call.function.arguments)
        .map_err(|e| anyhow::anyhow!("invalid schedule_recurring arguments: {e}"))?;

    let cron_expr = args.cron.trim();
    if cron_expr.is_empty() {
        anyhow::bail!("cron expression is empty");
    }
    Schedule::from_str(cron_expr).map_err(|e| anyhow::anyhow!("invalid cron expression: {e}"))?;

    let prompt = args.prompt.trim();
    if prompt.is_empty() {
        anyhow::bail!("prompt is empty");
    }

    let id = match args
        .job_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(custom) => {
            if !custom
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                anyhow::bail!("job_id must be alphanumeric, dash, or underscore");
            }
            format!("cron_{custom}")
        }
        None => format!("cron_{}", Uuid::new_v4()),
    };

    let (deliver_channel, deliver_peer) = deliver_target(req);
    let store = CronStore::new(pool);
    let job = store
        .insert_agent_job(
            &id,
            cron_expr,
            agent_group,
            prompt,
            deliver_channel.as_deref(),
            deliver_peer.as_deref(),
            args.deliver_message.as_deref(),
            Some(session_id),
        )
        .await?;

    Ok(format!(
        "recurring job {} cron=\"{}\" group={}; deliver via {} peer={}; prompt={}",
        job.id,
        job.cron_expr,
        job.agent_group,
        deliver_channel.unwrap_or_else(|| "cli".into()),
        deliver_peer.unwrap_or_else(|| "-".into()),
        truncate(&job.prompt, 120),
    ))
}

async fn handle_list(pool: &SqlitePool) -> anyhow::Result<String> {
    let sched = ScheduledTaskStore::new(pool).list_pending().await?;
    let cron = CronStore::new(pool).list_enabled().await?;

    if sched.is_empty() && cron.is_empty() {
        return Ok("no pending one-shot tasks or active recurring jobs".into());
    }

    let mut lines = Vec::new();
    if !sched.is_empty() {
        lines.push("one-shot:".into());
        for t in sched {
            lines.push(format!(
                "  {} run_at={} group={} deliver={}/{} prompt={}",
                t.id,
                t.run_at,
                t.agent_group,
                t.deliver_channel.as_deref().unwrap_or("cli"),
                t.deliver_peer.as_deref().unwrap_or("-"),
                truncate(&t.prompt, 80),
            ));
        }
    }
    if !cron.is_empty() {
        lines.push("recurring:".into());
        for j in cron {
            lines.push(format!(
                "  {} cron=\"{}\" group={} deliver={}/{} prompt={}",
                j.id,
                j.cron_expr,
                j.agent_group,
                j.deliver_channel.as_deref().unwrap_or("cli"),
                j.deliver_peer.as_deref().unwrap_or("-"),
                truncate(&j.prompt, 80),
            ));
        }
    }
    Ok(lines.join("\n"))
}

async fn handle_cancel(pool: &SqlitePool, call: &ToolCall) -> anyhow::Result<String> {
    let args: CancelArgs = serde_json::from_str(&call.function.arguments)
        .map_err(|e| anyhow::anyhow!("invalid schedule_cancel arguments: {e}"))?;
    let id = args.id.trim();
    if id.is_empty() {
        anyhow::bail!("id is empty");
    }

    if id.starts_with("sched_") {
        if ScheduledTaskStore::new(pool).cancel(id).await? {
            return Ok(format!("cancelled one-shot task {id}"));
        }
        anyhow::bail!("one-shot task not found or not pending: {id}");
    }

    if id.starts_with("cron_") {
        if CronStore::new(pool).disable(id).await? {
            return Ok(format!("disabled recurring job {id}"));
        }
        anyhow::bail!("recurring job not found or already disabled: {id}");
    }

    anyhow::bail!("id must start with sched_ or cron_; got {id}");
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

#[cfg(test)]
mod tests {
    use super::*;
    use bobaclaw_state::StateDb;
    use tempfile::TempDir;

    async fn test_pool() -> (TempDir, SqlitePool) {
        let dir = TempDir::new().unwrap();
        let paths = bobaclaw_core::BobaPaths::from_home(dir.path().to_path_buf());
        let state = StateDb::open(&paths.state_db).await.unwrap();
        (dir, state.pool().clone())
    }

    #[tokio::test]
    async fn recurring_validates_cron() {
        let (_dir, pool) = test_pool().await;
        let call = ToolCall {
            id: "1".into(),
            kind: "function".into(),
            function: bobaclaw_provider::FunctionCallPayload {
                name: TOOL_SCHEDULE_RECURRING.into(),
                arguments: json!({
                    "cron": "not a cron",
                    "prompt": "ping"
                })
                .to_string(),
            },
        };
        let req = NormalizedRequest::cli("hi", "home");
        let err = handle_schedule_tool(&pool, "home", "sess1", &req, &call)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("invalid cron"));
    }

    #[tokio::test]
    async fn one_shot_inserts_task() {
        let (_dir, pool) = test_pool().await;
        let call = ToolCall {
            id: "1".into(),
            kind: "function".into(),
            function: bobaclaw_provider::FunctionCallPayload {
                name: TOOL_SCHEDULE.into(),
                arguments: json!({
                    "delay_seconds": 30,
                    "prompt": "say beer",
                    "deliver_message": "пиво"
                })
                .to_string(),
            },
        };
        let req = NormalizedRequest::telegram(
            "hi",
            "home",
            bobaclaw_core::ChannelPeer {
                channel: "telegram".into(),
                peer: "123".into(),
                thread_id: None,
            },
            vec![],
        );
        let out = handle_schedule_tool(&pool, "home", "sess1", &req, &call)
            .await
            .unwrap();
        assert!(out.contains("sched_"));
        let pending = ScheduledTaskStore::new(&pool).list_pending().await.unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].deliver_text.as_deref(), Some("пиво"));
    }
}
