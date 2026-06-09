use bobaclaw_core::{BobaConfig, BobaPaths};
use bobaclaw_provider::{FunctionSpec, ToolCall, ToolSpec};
use bobaclaw_state::SpawnJobStore;
use serde::Deserialize;
use serde_json::json;
use sqlx::SqlitePool;

pub const SPAWN_STATUS: &str = "spawn_status";

pub fn spawn_status_tool_spec() -> ToolSpec {
    ToolSpec {
        kind: "function".into(),
        function: FunctionSpec {
            name: SPAWN_STATUS.into(),
            description: "Check status of a background spawn job in the current session. \
                Provide task_id and/or label (latest match)."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "task_id": { "type": "string", "description": "spawn_<uuid> from spawn tool" },
                    "label": { "type": "string", "description": "Optional label; returns latest match" }
                }
            }),
        },
    }
}

pub fn is_spawn_status_tool(name: &str) -> bool {
    name == SPAWN_STATUS
}

#[derive(Debug, Deserialize, Default)]
struct SpawnStatusArgs {
    #[serde(default)]
    task_id: Option<String>,
    #[serde(default)]
    label: Option<String>,
}

pub struct SpawnStatusToolResult {
    pub body: String,
    pub exit_code: i32,
}

pub async fn handle_spawn_status_tool(
    _paths: &BobaPaths,
    config: &BobaConfig,
    pool: &SqlitePool,
    session_id: &str,
    call: &ToolCall,
) -> anyhow::Result<SpawnStatusToolResult> {
    if call.function.name != SPAWN_STATUS {
        anyhow::bail!("unknown tool: {}", call.function.name);
    }
    if !config.subagents.enabled {
        return Ok(SpawnStatusToolResult {
            body: "spawn_status: subagents are disabled in config".into(),
            exit_code: 1,
        });
    }

    let args: SpawnStatusArgs = match serde_json::from_str(&call.function.arguments) {
        Ok(a) => a,
        Err(e) => {
            return Ok(SpawnStatusToolResult {
                body: format!("invalid spawn_status arguments: {e}"),
                exit_code: 1,
            });
        }
    };

    let task_id = args
        .task_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let label = args
        .label
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if task_id.is_none() && label.is_none() {
        return Ok(SpawnStatusToolResult {
            body: "spawn_status: task_id or label is required".into(),
            exit_code: 1,
        });
    }

    let store = SpawnJobStore::new(pool);
    let job = if let Some(id) = task_id {
        let job = store.get(id).await?;
        match job {
            Some(j) if j.session_id == session_id => Some(j),
            Some(_) => {
                return Ok(SpawnStatusToolResult {
                    body: "spawn_status: task not found in this session".into(),
                    exit_code: 1,
                });
            }
            None => None,
        }
    } else if let Some(lbl) = label {
        store.find_by_label_in_session(session_id, lbl).await?
    } else {
        None
    };

    let Some(job) = job else {
        return Ok(SpawnStatusToolResult {
            body: "spawn_status: no matching job".into(),
            exit_code: 1,
        });
    };

    let body = format!(
        "task_id={}\nstatus={}\nsubagent_id={}\nexit_code={}\nfinished_at={}\npreview={}",
        job.id,
        job.status,
        job.subagent_id.as_deref().unwrap_or("-"),
        job.exit_code
            .map(|c| c.to_string())
            .unwrap_or_else(|| "-".into()),
        job.finished_at
            .map(|t| t.to_string())
            .unwrap_or_else(|| "-".into()),
        job.result_preview.as_deref().unwrap_or("-"),
    );

    Ok(SpawnStatusToolResult { body, exit_code: 0 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bobaclaw_state::StateDb;

    #[test]
    fn is_spawn_status_tool_matches() {
        assert!(is_spawn_status_tool("spawn_status"));
        assert!(!is_spawn_status_tool("spawn"));
    }

    #[tokio::test]
    async fn spawn_status_by_task_id_wrong_session_denied() {
        let dir = tempfile::tempdir().unwrap();
        let db = StateDb::open(&dir.path().join("test.db")).await.unwrap();
        let pool = db.pool();
        sqlx::query(
            "INSERT INTO sessions (id, source, agent_group, started_at) VALUES ('sess_a', 'cli', 'home', 1.0)",
        )
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO sessions (id, source, agent_group, started_at) VALUES ('sess_b', 'cli', 'home', 1.0)",
        )
        .execute(pool)
        .await
        .unwrap();
        let store = SpawnJobStore::new(pool);
        let job = store
            .insert_running(
                "sess_a",
                "home",
                "cli",
                Some("cli"),
                None,
                None,
                None,
                "task",
                None,
                None,
                true,
            )
            .await
            .unwrap();

        let config = BobaConfig::default();
        let paths = BobaPaths::from_home(dir.path().to_path_buf());
        let call = ToolCall {
            id: "1".into(),
            kind: "function".into(),
            function: bobaclaw_provider::FunctionCallPayload {
                name: SPAWN_STATUS.into(),
                arguments: serde_json::json!({ "task_id": job.id }).to_string(),
            },
        };
        let out = handle_spawn_status_tool(&paths, &config, pool, "sess_b", &call)
            .await
            .unwrap();
        assert_eq!(out.exit_code, 1);
        assert!(out.body.contains("not found in this session"));
    }
}
