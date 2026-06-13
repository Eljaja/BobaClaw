use bobaclaw_core::{ChannelPeer, IngressKind, NormalizedRequest};
use chrono::Utc;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::routes::{create_session_for_route, RouteStore};

pub struct SessionStore<'a> {
    pool: &'a SqlitePool,
}

impl<'a> SessionStore<'a> {
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn resolve_session(&self, req: &NormalizedRequest) -> anyhow::Result<String> {
        if let Some(ref sid) = req.session_id {
            return Ok(sid.clone());
        }
        if let Some(ref peer) = req.channel_peer {
            return self
                .get_or_create_routed(peer, &req.agent_group, req.ingress)
                .await;
        }
        self.get_or_create_for_ingress(&req.agent_group, req.ingress)
            .await
    }

    pub async fn get_or_create_routed(
        &self,
        peer: &ChannelPeer,
        agent_group: &str,
        ingress: IngressKind,
    ) -> anyhow::Result<String> {
        let routes = RouteStore::new(self.pool);
        if let Some(sid) = routes.get_session_id(peer).await? {
            let active: Option<String> =
                sqlx::query_scalar("SELECT id FROM sessions WHERE id = ?1 AND ended_at IS NULL")
                    .bind(&sid)
                    .fetch_optional(self.pool)
                    .await?;
            if active.is_some() {
                return Ok(sid);
            }
        }

        let source = ingress_source(ingress);
        let user_id = peer.peer.as_str();
        let session_id =
            create_session_for_route(self.pool, &source, agent_group, Some(user_id)).await?;
        routes.upsert(peer, agent_group, &session_id).await?;
        Ok(session_id)
    }

    pub async fn get_or_create_for_ingress(
        &self,
        agent_group: &str,
        ingress: IngressKind,
    ) -> anyhow::Result<String> {
        let source = ingress_source(ingress);
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

    pub async fn get_or_create_cli(&self, agent_group: &str) -> anyhow::Result<String> {
        self.get_or_create_for_ingress(agent_group, IngressKind::Cli)
            .await
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

        sqlx::query("UPDATE sessions SET message_count = message_count + 1 WHERE id = ?1")
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

    /// End the active session for a channel peer and create a fresh routed session.
    pub async fn reset_routed_session(
        &self,
        peer: &ChannelPeer,
        agent_group: &str,
        ingress: IngressKind,
    ) -> anyhow::Result<(u64, String)> {
        let routes = RouteStore::new(self.pool);
        let now = Utc::now().timestamp_millis() as f64 / 1000.0;
        let mut ended = 0u64;

        if let Some(sid) = routes.get_session_id(peer).await? {
            let result = sqlx::query(
                "UPDATE sessions SET ended_at = ?1, end_reason = 'channel_new' WHERE id = ?2 AND ended_at IS NULL",
            )
            .bind(now)
            .bind(&sid)
            .execute(self.pool)
            .await?;
            ended = result.rows_affected();
        }

        let new_id = self
            .get_or_create_routed(peer, agent_group, ingress)
            .await?;
        Ok((ended, new_id))
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

    pub async fn list_messages(&self, session_id: &str) -> anyhow::Result<Vec<(String, String)>> {
        let rows = sqlx::query_as::<_, (String, String)>(
            "SELECT role, COALESCE(content, '') FROM messages WHERE session_id = ?1 ORDER BY id ASC",
        )
        .bind(session_id)
        .fetch_all(self.pool)
        .await?;
        Ok(rows)
    }

    /// Count user-role messages in a session (for memory review turn gate).
    pub async fn count_user_messages(&self, session_id: &str) -> anyhow::Result<usize> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM messages WHERE session_id = ?1 AND role = 'user'",
        )
        .bind(session_id)
        .fetch_one(self.pool)
        .await?;
        Ok(count as usize)
    }

    /// Search message history for an agent group via FTS5.
    pub async fn search_messages(
        &self,
        agent_group: &str,
        query: &str,
        limit: i64,
    ) -> anyhow::Result<Vec<MessageSearchHit>> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }
        if trimmed.len() > 200 {
            anyhow::bail!("query too long (max 200 chars)");
        }

        let fts_query = build_fts_query(trimmed);
        let limit = limit.clamp(1, 50);

        let rows = sqlx::query_as::<_, (String, String, f64, String)>(
            "SELECT m.session_id, m.role, m.timestamp,
                    snippet(messages_fts, 0, '[', ']', '…', 32) AS snippet
             FROM messages_fts
             INNER JOIN messages m ON m.id = messages_fts.rowid
             INNER JOIN sessions s ON s.id = m.session_id
             WHERE messages_fts MATCH ?1 AND s.agent_group = ?2
             ORDER BY rank
             LIMIT ?3",
        )
        .bind(&fts_query)
        .bind(agent_group)
        .bind(limit)
        .fetch_all(self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|(session_id, role, timestamp, snippet)| MessageSearchHit {
                session_id,
                role,
                timestamp,
                snippet,
            })
            .collect())
    }
}

#[derive(Debug, Clone)]
pub struct MessageSearchHit {
    pub session_id: String,
    pub role: String,
    pub timestamp: f64,
    pub snippet: String,
}

fn build_fts_query(raw: &str) -> String {
    raw.split_whitespace()
        .filter(|t| !t.is_empty())
        .map(|term| {
            let escaped = term.replace('"', "\"\"");
            format!("\"{escaped}\"")
        })
        .collect::<Vec<_>>()
        .join(" OR ")
}

fn ingress_source(kind: IngressKind) -> String {
    match kind {
        IngressKind::Cli => "cli",
        IngressKind::Rest => "rest",
        IngressKind::OpenAiCompat => "openai_compat",
        IngressKind::Cron => "cron",
        IngressKind::Webhook => "webhook",
        IngressKind::Chat => "chat",
        IngressKind::Telegram => "telegram",
        IngressKind::SpawnWake => "spawn_wake",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StateDb;

    #[tokio::test]
    async fn session_messages_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let db = StateDb::open(&dir.path().join("state.db")).await.unwrap();
        let store = SessionStore::new(db.pool());
        let sid = store.get_or_create_cli("test").await.unwrap();
        store.append_message(&sid, "user", "hello").await.unwrap();
        store
            .append_message(&sid, "assistant", "world")
            .await
            .unwrap();
        store
            .append_message(&sid, "compaction", "summary")
            .await
            .unwrap();

        let all = store.list_messages(&sid).await.unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].1, "hello");

        let recent = store.recent_messages(&sid, 2).await.unwrap();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].0, "assistant");

        assert_eq!(store.count_user_messages(&sid).await.unwrap(), 1);

        let hits = store.search_messages("test", "hello", 5).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].snippet.contains("hello"));
    }

    #[tokio::test]
    async fn end_active_cli_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let db = StateDb::open(&dir.path().join("state.db")).await.unwrap();
        let store = SessionStore::new(db.pool());
        let _ = store.get_or_create_cli("g").await.unwrap();
        let n = store.end_active_cli_sessions("g").await.unwrap();
        assert_eq!(n, 1);
        let sid2 = store.get_or_create_cli("g").await.unwrap();
        assert!(!sid2.is_empty());
    }

    #[tokio::test]
    async fn reset_routed_session_creates_fresh_session() {
        use bobaclaw_core::ChannelPeer;

        let dir = tempfile::tempdir().unwrap();
        let db = StateDb::open(&dir.path().join("state.db")).await.unwrap();
        let store = SessionStore::new(db.pool());
        let peer = ChannelPeer::telegram(42, None);
        let sid1 = store
            .get_or_create_routed(&peer, "home", IngressKind::Telegram)
            .await
            .unwrap();
        store.append_message(&sid1, "user", "hello").await.unwrap();

        let (ended, sid2) = store
            .reset_routed_session(&peer, "home", IngressKind::Telegram)
            .await
            .unwrap();
        assert_eq!(ended, 1);
        assert_ne!(sid1, sid2);

        let msgs = store.list_messages(&sid2).await.unwrap();
        assert!(msgs.is_empty());
    }
}
