use bobaclaw_core::ChannelPeer;
use chrono::Utc;
use sqlx::SqlitePool;
use uuid::Uuid;

pub struct RouteStore<'a> {
    pool: &'a SqlitePool,
}

impl<'a> RouteStore<'a> {
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn get_session_id(&self, peer: &ChannelPeer) -> anyhow::Result<Option<String>> {
        let thread = peer.thread_id.as_deref();
        let thread_key = thread.unwrap_or("");
        let id = sqlx::query_scalar::<_, String>(
            "SELECT session_id FROM routes WHERE channel = ?1 AND peer = ?2 AND COALESCE(thread_id, '') = ?3",
        )
        .bind(&peer.channel)
        .bind(&peer.peer)
        .bind(thread_key)
        .fetch_optional(self.pool)
        .await?;
        Ok(id)
    }

    pub async fn upsert(
        &self,
        peer: &ChannelPeer,
        agent_group: &str,
        session_id: &str,
    ) -> anyhow::Result<()> {
        let thread_key = peer.thread_id.as_deref().unwrap_or("");
        sqlx::query(
            r#"
            INSERT INTO routes (channel, peer, thread_id, agent_group, session_id)
            VALUES (?1, ?2, NULLIF(?3, ''), ?4, ?5)
            ON CONFLICT(channel, peer, thread_id) DO UPDATE SET
                agent_group = excluded.agent_group,
                session_id = excluded.session_id
            "#,
        )
        .bind(&peer.channel)
        .bind(&peer.peer)
        .bind(thread_key)
        .bind(agent_group)
        .bind(session_id)
        .execute(self.pool)
        .await?;
        Ok(())
    }
}

pub async fn create_session_for_route(
    pool: &SqlitePool,
    source: &str,
    agent_group: &str,
    user_id: Option<&str>,
) -> anyhow::Result<String> {
    let id = format!("sess_{}", Uuid::new_v4());
    let now = Utc::now().timestamp_millis() as f64 / 1000.0;
    sqlx::query(
        "INSERT INTO sessions (id, source, agent_group, user_id, started_at) VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind(&id)
    .bind(source)
    .bind(agent_group)
    .bind(user_id)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(id)
}
