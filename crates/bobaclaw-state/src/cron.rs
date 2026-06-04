use chrono::Utc;
use sqlx::SqlitePool;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct CronJobRow {
    pub id: String,
    pub cron_expr: String,
    pub agent_group: String,
    pub prompt: String,
    pub enabled: i64,
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

    pub async fn list_enabled(&self) -> anyhow::Result<Vec<CronJobRow>> {
        sqlx::query_as::<_, CronJobRow>(
            "SELECT id, cron_expr, agent_group, prompt, enabled FROM cron_jobs WHERE enabled = 1",
        )
        .fetch_all(self.pool)
        .await
        .map_err(Into::into)
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
