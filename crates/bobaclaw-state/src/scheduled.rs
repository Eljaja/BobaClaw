use chrono::Utc;
use sqlx::SqlitePool;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ScheduledTask {
    pub id: String,
    pub agent_group: String,
    pub prompt: String,
    pub deliver_text: Option<String>,
    pub run_at: f64,
    pub deliver_channel: Option<String>,
    pub deliver_peer: Option<String>,
    pub source_session_id: Option<String>,
    pub status: String,
}

pub struct ScheduledTaskStore<'a> {
    pool: &'a SqlitePool,
}

impl<'a> ScheduledTaskStore<'a> {
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn insert(
        &self,
        agent_group: &str,
        prompt: &str,
        deliver_text: Option<&str>,
        run_at: f64,
        deliver_channel: Option<&str>,
        deliver_peer: Option<&str>,
        source_session_id: Option<&str>,
    ) -> anyhow::Result<ScheduledTask> {
        let id = format!("sched_{}", Uuid::new_v4());
        let now = Utc::now().timestamp_millis() as f64 / 1000.0;
        sqlx::query(
            r#"
            INSERT INTO scheduled_tasks
                (id, agent_group, prompt, deliver_text, run_at, deliver_channel, deliver_peer, source_session_id, status, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'pending', ?9)
            "#,
        )
        .bind(&id)
        .bind(agent_group)
        .bind(prompt)
        .bind(deliver_text)
        .bind(run_at)
        .bind(deliver_channel)
        .bind(deliver_peer)
        .bind(source_session_id)
        .bind(now)
        .execute(self.pool)
        .await?;
        Ok(ScheduledTask {
            id,
            agent_group: agent_group.to_string(),
            prompt: prompt.to_string(),
            deliver_text: deliver_text.map(str::to_string),
            run_at,
            deliver_channel: deliver_channel.map(str::to_string),
            deliver_peer: deliver_peer.map(str::to_string),
            source_session_id: source_session_id.map(str::to_string),
            status: "pending".into(),
        })
    }

    pub async fn list_due(&self, now: f64) -> anyhow::Result<Vec<ScheduledTask>> {
        let rows = sqlx::query_as::<_, TaskRow>(
            "SELECT id, agent_group, prompt, deliver_text, run_at, deliver_channel, deliver_peer, source_session_id, status
             FROM scheduled_tasks WHERE status = 'pending' AND run_at <= ?1 ORDER BY run_at ASC LIMIT 20",
        )
        .bind(now)
        .fetch_all(self.pool)
        .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    pub async fn list_pending(&self) -> anyhow::Result<Vec<ScheduledTask>> {
        let rows = sqlx::query_as::<_, TaskRow>(
            "SELECT id, agent_group, prompt, deliver_text, run_at, deliver_channel, deliver_peer, source_session_id, status
             FROM scheduled_tasks WHERE status = 'pending' ORDER BY run_at ASC",
        )
        .fetch_all(self.pool)
        .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    pub async fn mark_running(&self, id: &str) -> anyhow::Result<bool> {
        let r = sqlx::query(
            "UPDATE scheduled_tasks SET status = 'running' WHERE id = ?1 AND status = 'pending'",
        )
        .bind(id)
        .execute(self.pool)
        .await?;
        Ok(r.rows_affected() > 0)
    }

    pub async fn mark_done(&self, id: &str) -> anyhow::Result<()> {
        let now = Utc::now().timestamp_millis() as f64 / 1000.0;
        sqlx::query(
            "UPDATE scheduled_tasks SET status = 'done', completed_at = ?1 WHERE id = ?2",
        )
        .bind(now)
        .bind(id)
        .execute(self.pool)
        .await?;
        Ok(())
    }

    pub async fn mark_failed(&self, id: &str, err: &str) -> anyhow::Result<()> {
        let now = Utc::now().timestamp_millis() as f64 / 1000.0;
        sqlx::query(
            "UPDATE scheduled_tasks SET status = 'failed', completed_at = ?1, last_error = ?2 WHERE id = ?3",
        )
        .bind(now)
        .bind(err)
        .bind(id)
        .execute(self.pool)
        .await?;
        Ok(())
    }

    pub async fn cancel(&self, id: &str) -> anyhow::Result<bool> {
        let r = sqlx::query(
            "UPDATE scheduled_tasks SET status = 'cancelled', completed_at = ?1 WHERE id = ?2 AND status = 'pending'",
        )
        .bind(Utc::now().timestamp_millis() as f64 / 1000.0)
        .bind(id)
        .execute(self.pool)
        .await?;
        Ok(r.rows_affected() > 0)
    }
}

#[derive(sqlx::FromRow)]
struct TaskRow {
    id: String,
    agent_group: String,
    prompt: String,
    deliver_text: Option<String>,
    run_at: f64,
    deliver_channel: Option<String>,
    deliver_peer: Option<String>,
    source_session_id: Option<String>,
    status: String,
}

impl From<TaskRow> for ScheduledTask {
    fn from(r: TaskRow) -> Self {
        Self {
            id: r.id,
            agent_group: r.agent_group,
            prompt: r.prompt,
            deliver_text: r.deliver_text,
            run_at: r.run_at,
            deliver_channel: r.deliver_channel,
            deliver_peer: r.deliver_peer,
            source_session_id: r.source_session_id,
            status: r.status,
        }
    }
}
