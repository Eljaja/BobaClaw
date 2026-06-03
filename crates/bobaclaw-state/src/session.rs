use bobaclaw_core::IngressKind;
use chrono::Utc;
use sqlx::SqlitePool;
use uuid::Uuid;

pub struct SessionStore<'a> {
    pool: &'a SqlitePool,
}

impl<'a> SessionStore<'a> {
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn get_or_create_cli(
        &self,
        agent_group: &str,
    ) -> anyhow::Result<String> {
        let source = ingress_source(IngressKind::Cli);
        if let Some(id) = sqlx::query_scalar::<_, String>(
            "SELECT id FROM sessions WHERE source = ?1 AND agent_group = ?2 AND ended_at IS NULL ORDER BY started_at DESC LIMIT 1",
        )
        .bind(&source)
        .bind(agent_group)
        .fetch_optional(self.pool)
        .await?
        {
            return Ok(id);
        }

        let id = format!("sess_{}", Uuid::new_v4());
        let now = Utc::now().timestamp_millis() as f64 / 1000.0;
        sqlx::query(
            "INSERT INTO sessions (id, source, agent_group, started_at) VALUES (?1, ?2, ?3, ?4)",
        )
        .bind(&id)
        .bind(&source)
        .bind(agent_group)
        .bind(now)
        .execute(self.pool)
        .await?;
        Ok(id)
    }

    pub async fn append_message(
        &self,
        session_id: &str,
        role: &str,
        content: &str,
    ) -> anyhow::Result<()> {
        let now = Utc::now().timestamp_millis() as f64 / 1000.0;
        sqlx::query(
            "INSERT INTO messages (session_id, role, content, timestamp) VALUES (?1, ?2, ?3, ?4)",
        )
        .bind(session_id)
        .bind(role)
        .bind(content)
        .bind(now)
        .execute(self.pool)
        .await?;

        sqlx::query(
            "UPDATE sessions SET message_count = message_count + 1 WHERE id = ?1",
        )
        .bind(session_id)
        .execute(self.pool)
        .await?;
        Ok(())
    }

    pub async fn end_active_cli_sessions(&self, agent_group: &str) -> anyhow::Result<u64> {
        let source = ingress_source(IngressKind::Cli);
        let now = Utc::now().timestamp_millis() as f64 / 1000.0;
        let result = sqlx::query(
            "UPDATE sessions SET ended_at = ?1, end_reason = 'interactive_new' WHERE source = ?2 AND agent_group = ?3 AND ended_at IS NULL",
        )
        .bind(now)
        .bind(&source)
        .bind(agent_group)
        .execute(self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn recent_messages(
        &self,
        session_id: &str,
        limit: i64,
    ) -> anyhow::Result<Vec<(String, String)>> {
        let rows = sqlx::query_as::<_, (String, String)>(
            "SELECT role, COALESCE(content, '') FROM messages WHERE session_id = ?1 ORDER BY id DESC LIMIT ?2",
        )
        .bind(session_id)
        .bind(limit)
        .fetch_all(self.pool)
        .await?;
        Ok(rows.into_iter().rev().collect())
    }
}

fn ingress_source(kind: IngressKind) -> String {
    match kind {
        IngressKind::Cli => "cli",
        IngressKind::Rest => "rest",
        IngressKind::OpenAiCompat => "openai_compat",
        IngressKind::Cron => "cron",
        IngressKind::Webhook => "webhook",
        IngressKind::Chat => "chat",
    }
    .to_string()
}
