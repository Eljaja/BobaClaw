use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnJobRecord {
    pub id: String,
    pub subagent_id: Option<String>,
    pub session_id: String,
    pub agent_group: String,
    pub ingress: String,
    pub deliver_channel: Option<String>,
    pub deliver_peer: Option<String>,
    pub deliver_thread_id: Option<String>,
    pub label: Option<String>,
    pub task_preview: Option<String>,
    pub backend: Option<String>,
    pub status: String,
    pub exit_code: Option<i32>,
    pub result_preview: Option<String>,
    pub result_body: Option<String>,
    pub parent_request_id: Option<String>,
    pub wake_parent: bool,
    pub notified_at: Option<f64>,
    pub created_at: f64,
    pub started_at: Option<f64>,
    pub finished_at: Option<f64>,
}

pub struct SpawnJobStore<'a> {
    pool: &'a SqlitePool,
}

impl<'a> SpawnJobStore<'a> {
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn insert_running(
        &self,
        session_id: &str,
        agent_group: &str,
        ingress: &str,
        deliver_channel: Option<&str>,
        deliver_peer: Option<&str>,
        deliver_thread_id: Option<&str>,
        label: Option<&str>,
        task_preview: &str,
        backend: Option<&str>,
        parent_request_id: Option<&str>,
        wake_parent: bool,
    ) -> anyhow::Result<SpawnJobRecord> {
        let id = format!("spawn_{}", Uuid::new_v4());
        let now = now_secs();
        sqlx::query(
            r#"
            INSERT INTO spawn_jobs (
                id, session_id, agent_group, ingress, deliver_channel, deliver_peer,
                deliver_thread_id, label, task_preview, backend, status, parent_request_id,
                wake_parent, created_at, started_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'running', ?11, ?12, ?13, ?13)
            "#,
        )
        .bind(&id)
        .bind(session_id)
        .bind(agent_group)
        .bind(ingress)
        .bind(deliver_channel)
        .bind(deliver_peer)
        .bind(deliver_thread_id)
        .bind(label)
        .bind(task_preview)
        .bind(backend)
        .bind(parent_request_id)
        .bind(i32::from(wake_parent))
        .bind(now)
        .execute(self.pool)
        .await?;
        Ok(SpawnJobRecord {
            id,
            subagent_id: None,
            session_id: session_id.to_string(),
            agent_group: agent_group.to_string(),
            ingress: ingress.to_string(),
            deliver_channel: deliver_channel.map(str::to_string),
            deliver_peer: deliver_peer.map(str::to_string),
            deliver_thread_id: deliver_thread_id.map(str::to_string),
            label: label.map(str::to_string),
            task_preview: Some(task_preview.to_string()),
            backend: backend.map(str::to_string),
            status: "running".into(),
            exit_code: None,
            result_preview: None,
            result_body: None,
            parent_request_id: parent_request_id.map(str::to_string),
            wake_parent,
            notified_at: None,
            created_at: now,
            started_at: Some(now),
            finished_at: None,
        })
    }

    pub async fn link_subagent(&self, id: &str, subagent_id: &str) -> anyhow::Result<()> {
        sqlx::query("UPDATE spawn_jobs SET subagent_id = ?1 WHERE id = ?2")
            .bind(subagent_id)
            .bind(id)
            .execute(self.pool)
            .await?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn finalize(
        &self,
        id: &str,
        status: &str,
        exit_code: Option<i32>,
        result_preview: Option<&str>,
        result_body: Option<&str>,
    ) -> anyhow::Result<()> {
        let now = now_secs();
        sqlx::query(
            r#"
            UPDATE spawn_jobs SET
                status = ?1, exit_code = ?2, result_preview = ?3, result_body = ?4,
                finished_at = ?5
            WHERE id = ?6
            "#,
        )
        .bind(status)
        .bind(exit_code)
        .bind(result_preview)
        .bind(result_body)
        .bind(now)
        .bind(id)
        .execute(self.pool)
        .await?;
        Ok(())
    }

    pub async fn mark_notified(&self, id: &str) -> anyhow::Result<()> {
        let now = now_secs();
        sqlx::query("UPDATE spawn_jobs SET notified_at = ?1 WHERE id = ?2")
            .bind(now)
            .bind(id)
            .execute(self.pool)
            .await?;
        Ok(())
    }

    pub async fn get(&self, id: &str) -> anyhow::Result<Option<SpawnJobRecord>> {
        let row = sqlx::query_as::<_, JobRow>("SELECT * FROM spawn_jobs WHERE id = ?1")
            .bind(id)
            .fetch_optional(self.pool)
            .await?;
        Ok(row.map(Into::into))
    }

    pub async fn list_by_session(&self, session_id: &str) -> anyhow::Result<Vec<SpawnJobRecord>> {
        let rows = sqlx::query_as::<_, JobRow>(
            "SELECT * FROM spawn_jobs WHERE session_id = ?1 ORDER BY created_at DESC LIMIT 50",
        )
        .bind(session_id)
        .fetch_all(self.pool)
        .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    pub async fn find_by_label_in_session(
        &self,
        session_id: &str,
        label: &str,
    ) -> anyhow::Result<Option<SpawnJobRecord>> {
        let row = sqlx::query_as::<_, JobRow>(
            "SELECT * FROM spawn_jobs WHERE session_id = ?1 AND label = ?2 ORDER BY created_at DESC LIMIT 1",
        )
        .bind(session_id)
        .bind(label)
        .fetch_optional(self.pool)
        .await?;
        Ok(row.map(Into::into))
    }

    pub async fn count_recent_wakes(
        &self,
        session_id: &str,
        since_secs: f64,
    ) -> anyhow::Result<i64> {
        let count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM spawn_jobs WHERE session_id = ?1 AND finished_at >= ?2 AND status = 'completed'",
        )
        .bind(session_id)
        .bind(since_secs)
        .fetch_one(self.pool)
        .await?;
        Ok(count)
    }
}

#[derive(sqlx::FromRow)]
struct JobRow {
    id: String,
    subagent_id: Option<String>,
    session_id: String,
    agent_group: String,
    ingress: String,
    deliver_channel: Option<String>,
    deliver_peer: Option<String>,
    deliver_thread_id: Option<String>,
    label: Option<String>,
    task_preview: Option<String>,
    backend: Option<String>,
    status: String,
    exit_code: Option<i32>,
    result_preview: Option<String>,
    result_body: Option<String>,
    parent_request_id: Option<String>,
    wake_parent: i32,
    notified_at: Option<f64>,
    created_at: f64,
    started_at: Option<f64>,
    finished_at: Option<f64>,
}

impl From<JobRow> for SpawnJobRecord {
    fn from(r: JobRow) -> Self {
        Self {
            id: r.id,
            subagent_id: r.subagent_id,
            session_id: r.session_id,
            agent_group: r.agent_group,
            ingress: r.ingress,
            deliver_channel: r.deliver_channel,
            deliver_peer: r.deliver_peer,
            deliver_thread_id: r.deliver_thread_id,
            label: r.label,
            task_preview: r.task_preview,
            backend: r.backend,
            status: r.status,
            exit_code: r.exit_code,
            result_preview: r.result_preview,
            result_body: r.result_body,
            parent_request_id: r.parent_request_id,
            wake_parent: r.wake_parent != 0,
            notified_at: r.notified_at,
            created_at: r.created_at,
            started_at: r.started_at,
            finished_at: r.finished_at,
        }
    }
}

fn now_secs() -> f64 {
    Utc::now().timestamp_millis() as f64 / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::StateDb;

    async fn seed_session(pool: &SqlitePool, id: &str) {
        sqlx::query(
            "INSERT INTO sessions (id, source, agent_group, started_at) VALUES (?1, 'cli', 'home', 1.0)",
        )
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn insert_finalize_list() {
        let dir = tempfile::tempdir().unwrap();
        let db = StateDb::open(&dir.path().join("test.db")).await.unwrap();
        let pool = db.pool();
        seed_session(pool, "sess_1").await;
        let store = SpawnJobStore::new(pool);
        let job = store
            .insert_running(
                "sess_1",
                "home",
                "cli",
                Some("cli"),
                None,
                None,
                Some("research"),
                "do research",
                Some("native"),
                Some("req_1"),
                true,
            )
            .await
            .unwrap();
        store.link_subagent(&job.id, "subagent_abc").await.unwrap();
        store
            .finalize(
                &job.id,
                "completed",
                Some(0),
                Some("done"),
                Some("full body"),
            )
            .await
            .unwrap();
        let list = store.list_by_session("sess_1").await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].status, "completed");
        assert_eq!(list[0].subagent_id.as_deref(), Some("subagent_abc"));
    }
}
