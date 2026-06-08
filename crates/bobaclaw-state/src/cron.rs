use chrono::Utc;
use sqlx::SqlitePool;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct CronJobRow {
    pub id: String,
    pub cron_expr: String,
    pub agent_group: String,
    pub prompt: String,
    pub enabled: i64,
    pub deliver_channel: Option<String>,
    pub deliver_peer: Option<String>,
    pub deliver_text: Option<String>,
    pub source_session_id: Option<String>,
}

pub struct CronStore<'a> {
    pool: &'a SqlitePool,
}

impl<'a> CronStore<'a> {
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn upsert(
        &self,
        id: &str,
        cron_expr: &str,
        agent_group: &str,
        prompt: &str,
    ) -> anyhow::Result<()> {
        let now = Utc::now().timestamp_millis() as f64 / 1000.0;
        sqlx::query(
            r#"
            INSERT INTO cron_jobs (id, cron_expr, agent_group, prompt, enabled, created_at)
            VALUES (?1, ?2, ?3, ?4, 1, ?5)
            ON CONFLICT(id) DO UPDATE SET
                cron_expr = excluded.cron_expr,
                agent_group = excluded.agent_group,
                prompt = excluded.prompt,
                enabled = 1
            "#,
        )
        .bind(id)
        .bind(cron_expr)
        .bind(agent_group)
        .bind(prompt)
        .bind(now)
        .execute(self.pool)
        .await?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn insert_agent_job(
        &self,
        id: &str,
        cron_expr: &str,
        agent_group: &str,
        prompt: &str,
        deliver_channel: Option<&str>,
        deliver_peer: Option<&str>,
        deliver_text: Option<&str>,
        source_session_id: Option<&str>,
    ) -> anyhow::Result<CronJobRow> {
        let now = Utc::now().timestamp_millis() as f64 / 1000.0;
        sqlx::query(
            r#"
            INSERT INTO cron_jobs
                (id, cron_expr, agent_group, prompt, enabled, created_at,
                 deliver_channel, deliver_peer, deliver_text, source_session_id)
            VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6, ?7, ?8, ?9)
            "#,
        )
        .bind(id)
        .bind(cron_expr)
        .bind(agent_group)
        .bind(prompt)
        .bind(now)
        .bind(deliver_channel)
        .bind(deliver_peer)
        .bind(deliver_text)
        .bind(source_session_id)
        .execute(self.pool)
        .await?;
        Ok(CronJobRow {
            id: id.to_string(),
            cron_expr: cron_expr.to_string(),
            agent_group: agent_group.to_string(),
            prompt: prompt.to_string(),
            enabled: 1,
            deliver_channel: deliver_channel.map(str::to_string),
            deliver_peer: deliver_peer.map(str::to_string),
            deliver_text: deliver_text.map(str::to_string),
            source_session_id: source_session_id.map(str::to_string),
        })
    }

    pub async fn list_enabled(&self) -> anyhow::Result<Vec<CronJobRow>> {
        sqlx::query_as::<_, CronJobRow>(
            "SELECT id, cron_expr, agent_group, prompt, enabled,
                    deliver_channel, deliver_peer, deliver_text, source_session_id
             FROM cron_jobs WHERE enabled = 1",
        )
        .fetch_all(self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn list_all(&self) -> anyhow::Result<Vec<CronJobRow>> {
        sqlx::query_as::<_, CronJobRow>(
            "SELECT id, cron_expr, agent_group, prompt, enabled,
                    deliver_channel, deliver_peer, deliver_text, source_session_id
             FROM cron_jobs ORDER BY created_at DESC",
        )
        .fetch_all(self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn disable(&self, id: &str) -> anyhow::Result<bool> {
        let r = sqlx::query("UPDATE cron_jobs SET enabled = 0 WHERE id = ?1 AND enabled = 1")
            .bind(id)
            .execute(self.pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }

    pub async fn record_run(&self, job_id: &str, status: &str) -> anyhow::Result<String> {
        let run_id = format!("cronrun_{}", uuid::Uuid::new_v4());
        let now = Utc::now().timestamp_millis() as f64 / 1000.0;
        sqlx::query(
            "INSERT INTO cron_runs (id, job_id, status, created_at) VALUES (?1, ?2, ?3, ?4)",
        )
        .bind(&run_id)
        .bind(job_id)
        .bind(status)
        .bind(now)
        .execute(self.pool)
        .await?;
        Ok(run_id)
    }

    pub async fn last_run_at(&self, job_id: &str) -> anyhow::Result<Option<f64>> {
        let t = sqlx::query_scalar::<_, f64>(
            "SELECT created_at FROM cron_runs WHERE job_id = ?1 ORDER BY created_at DESC LIMIT 1",
        )
        .bind(job_id)
        .fetch_optional(self.pool)
        .await?;
        Ok(t)
    }
}
