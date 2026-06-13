use bobaclaw_core::{RunEventKind, RunStatus};
use chrono::Utc;
use serde_json::Value;
use sqlx::SqlitePool;

#[derive(Debug, Clone)]
pub struct RunRecord {
    pub id: String,
    pub status: RunStatus,
    pub executor_profile: String,
    pub capsule_dir: Option<String>,
    pub exit_code: Option<i32>,
    pub summary: Option<String>,
}

pub struct RunLedger<'a> {
    pool: &'a SqlitePool,
}

impl<'a> RunLedger<'a> {
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn create_run(
        &self,
        run_id: &str,
        session_id: Option<&str>,
        request_id: Option<&str>,
        executor_profile: &str,
    ) -> anyhow::Result<()> {
        let now = now_secs();
        sqlx::query(
            "INSERT INTO runs (id, session_id, request_id, status, executor_profile, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .bind(run_id)
        .bind(session_id)
        .bind(request_id)
        .bind(status_str(RunStatus::Created))
        .bind(executor_profile)
        .bind(now)
        .bind(now)
        .execute(self.pool)
        .await?;
        self.append_event(run_id, RunEventKind::Created, None)
            .await?;
        Ok(())
    }

    pub async fn set_capsule_dir(&self, run_id: &str, dir: &str) -> anyhow::Result<()> {
        let now = now_secs();
        sqlx::query("UPDATE runs SET status = ?1, capsule_dir = ?2, updated_at = ?3 WHERE id = ?4")
            .bind(status_str(RunStatus::ScriptSaved))
            .bind(dir)
            .bind(now)
            .bind(run_id)
            .execute(self.pool)
            .await?;
        self.append_event(
            run_id,
            RunEventKind::ScriptSaved,
            Some(serde_json::json!({ "capsule_dir": dir })),
        )
        .await?;
        Ok(())
    }

    pub async fn mark_started(&self, run_id: &str) -> anyhow::Result<()> {
        self.update_status(run_id, RunStatus::Started, RunEventKind::Started, None)
            .await
    }

    pub async fn mark_completed(
        &self,
        run_id: &str,
        exit_code: i32,
        summary: &str,
    ) -> anyhow::Result<()> {
        let now = now_secs();
        let status = if exit_code == 0 {
            RunStatus::Completed
        } else {
            RunStatus::Failed
        };
        sqlx::query(
            "UPDATE runs SET status = ?1, exit_code = ?2, summary = ?3, updated_at = ?4 WHERE id = ?5",
        )
        .bind(status_str(status))
        .bind(exit_code)
        .bind(summary)
        .bind(now)
        .bind(run_id)
        .execute(self.pool)
        .await?;
        let kind = if exit_code == 0 {
            RunEventKind::Completed
        } else {
            RunEventKind::Failed
        };
        self.append_event(
            run_id,
            kind,
            Some(serde_json::json!({ "exit_code": exit_code, "summary": summary })),
        )
        .await?;
        Ok(())
    }

    pub async fn mark_denied(&self, run_id: &str, reason: &str) -> anyhow::Result<()> {
        let now = now_secs();
        sqlx::query("UPDATE runs SET status = ?1, summary = ?2, updated_at = ?3 WHERE id = ?4")
            .bind(status_str(RunStatus::Denied))
            .bind(reason)
            .bind(now)
            .bind(run_id)
            .execute(self.pool)
            .await?;
        self.append_event(
            run_id,
            RunEventKind::Denied,
            Some(serde_json::json!({ "reason": reason })),
        )
        .await?;
        Ok(())
    }

    pub async fn append_event(
        &self,
        run_id: &str,
        kind: RunEventKind,
        payload: Option<Value>,
    ) -> anyhow::Result<()> {
        let payload_str = payload.map(|p| p.to_string());
        sqlx::query(
            "INSERT INTO run_events (run_id, kind, payload, created_at) VALUES (?1, ?2, ?3, ?4)",
        )
        .bind(run_id)
        .bind(event_kind_str(kind))
        .bind(payload_str)
        .bind(now_secs())
        .execute(self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_run(&self, run_id: &str) -> anyhow::Result<Option<RunRecord>> {
        let row =
            sqlx::query_as::<_, (String, String, Option<String>, Option<i32>, Option<String>)>(
                "SELECT id, status, capsule_dir, exit_code, summary FROM runs WHERE id = ?1",
            )
            .bind(run_id)
            .fetch_optional(self.pool)
            .await?;

        Ok(
            row.map(|(id, status, capsule_dir, exit_code, summary)| RunRecord {
                id,
                status: parse_status(&status),
                executor_profile: String::new(),
                capsule_dir,
                exit_code,
                summary,
            }),
        )
    }

    /// Returns `Some(agent_group)` when the run exists and belongs to the given group.
    pub async fn verify_run_agent_group(
        &self,
        run_id: &str,
        agent_group: &str,
    ) -> anyhow::Result<Option<String>> {
        let row: Option<String> = sqlx::query_scalar(
            "SELECT s.agent_group FROM runs r
             INNER JOIN sessions s ON s.id = r.session_id
             WHERE r.id = ?1",
        )
        .bind(run_id)
        .fetch_optional(self.pool)
        .await?;

        Ok(row.filter(|g| g == agent_group))
    }

    async fn update_status(
        &self,
        run_id: &str,
        status: RunStatus,
        event: RunEventKind,
        payload: Option<Value>,
    ) -> anyhow::Result<()> {
        let now = now_secs();
        sqlx::query("UPDATE runs SET status = ?1, updated_at = ?2 WHERE id = ?3")
            .bind(status_str(status))
            .bind(now)
            .bind(run_id)
            .execute(self.pool)
            .await?;
        self.append_event(run_id, event, payload).await
    }
}

fn now_secs() -> f64 {
    Utc::now().timestamp_millis() as f64 / 1000.0
}

fn status_str(s: RunStatus) -> &'static str {
    match s {
        RunStatus::Created => "created",
        RunStatus::ScriptSaved => "script_saved",
        RunStatus::Approved => "approved",
        RunStatus::Started => "started",
        RunStatus::Completed => "completed",
        RunStatus::Failed => "failed",
        RunStatus::Timeout => "timeout",
        RunStatus::Denied => "denied",
        RunStatus::Partial => "partial",
    }
}

fn parse_status(s: &str) -> RunStatus {
    match s {
        "script_saved" => RunStatus::ScriptSaved,
        "approved" => RunStatus::Approved,
        "started" => RunStatus::Started,
        "completed" => RunStatus::Completed,
        "failed" => RunStatus::Failed,
        "timeout" => RunStatus::Timeout,
        "denied" => RunStatus::Denied,
        "partial" => RunStatus::Partial,
        _ => RunStatus::Created,
    }
}

fn event_kind_str(k: RunEventKind) -> &'static str {
    match k {
        RunEventKind::Created => "created",
        RunEventKind::ScriptSaved => "script_saved",
        RunEventKind::Approved => "approved",
        RunEventKind::Started => "started",
        RunEventKind::Stdout => "stdout",
        RunEventKind::Stderr => "stderr",
        RunEventKind::Artifact => "artifact",
        RunEventKind::ResultJson => "result_json",
        RunEventKind::Completed => "completed",
        RunEventKind::Failed => "failed",
        RunEventKind::Timeout => "timeout",
        RunEventKind::Denied => "denied",
    }
}
