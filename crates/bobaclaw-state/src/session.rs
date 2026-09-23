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

    /// Like [`Self::resolve_session`] but never creates a session: returns the active
    /// session this request would be routed to, if any.
    pub async fn find_session(&self, req: &NormalizedRequest) -> anyhow::Result<Option<String>> {
        if let Some(ref sid) = req.session_id {
            return Ok(Some(sid.clone()));
        }
        if let Some(ref peer) = req.channel_peer {
            let Some(sid) = RouteStore::new(self.pool).get_session_id(peer).await? else {
                return Ok(None);
            };
            return Ok(sqlx::query_scalar(
                "SELECT id FROM sessions WHERE id = ?1 AND ended_at IS NULL",
            )
            .bind(&sid)
            .fetch_optional(self.pool)
            .await?);
        }
        Ok(sqlx::query_scalar::<_, String>(
            "SELECT id FROM sessions WHERE source = ?1 AND agent_group = ?2 AND ended_at IS NULL ORDER BY started_at DESC LIMIT 1",
        )
        .bind(ingress_source(req.ingress))
        .bind(&req.agent_group)
        .fetch_optional(self.pool)
        .await?)
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
        self.create_session(agent_group, ingress).await
    }

    /// Always create a fresh (non-routed) session for an ingress, e.g. web UI "New chat".
    pub async fn create_session(
        &self,
        agent_group: &str,
        ingress: IngressKind,
    ) -> anyhow::Result<String> {
        let id = format!("sess_{}", Uuid::new_v4());
        let now = Utc::now().timestamp_millis() as f64 / 1000.0;
        sqlx::query(
            "INSERT INTO sessions (id, source, agent_group, started_at) VALUES (?1, ?2, ?3, ?4)",
        )
        .bind(&id)
        .bind(ingress_source(ingress))
        .bind(agent_group)
        .bind(now)
        .execute(self.pool)
        .await?;
        Ok(id)
    }

    /// Session metadata (source, group, end state), or `None` if the id is unknown.
    pub async fn get_session(&self, session_id: &str) -> anyhow::Result<Option<SessionInfo>> {
        let row = sqlx::query_as::<_, (String, String, String, f64, Option<f64>)>(
            "SELECT id, source, agent_group, started_at, ended_at FROM sessions WHERE id = ?1",
        )
        .bind(session_id)
        .fetch_optional(self.pool)
        .await?;
        Ok(row.map(
            |(id, source, agent_group, started_at, ended_at)| SessionInfo {
                id,
                source,
                agent_group,
                started_at,
                ended_at,
            },
        ))
    }

    /// Sessions created by one ingress for one agent group, most recently active first.
    ///
    /// `title` is the stored session title, else a snippet of the first user message.
    pub async fn list_sessions_for_ingress(
        &self,
        agent_group: &str,
        ingress: IngressKind,
        limit: i64,
    ) -> anyhow::Result<Vec<SessionSummary>> {
        let rows = sqlx::query_as::<
            _,
            (
                String,
                Option<String>,
                f64,
                Option<f64>,
                i64,
                Option<f64>,
                Option<String>,
            ),
        >(
            "SELECT s.id, s.title, s.started_at, s.ended_at, s.message_count,
                    (SELECT MAX(m.timestamp) FROM messages m WHERE m.session_id = s.id) AS last_ts,
                    (SELECT m.content FROM messages m
                      WHERE m.session_id = s.id AND m.role = 'user'
                      ORDER BY m.id ASC LIMIT 1) AS first_user
             FROM sessions s
             WHERE s.source = ?1 AND s.agent_group = ?2
             ORDER BY COALESCE(last_ts, s.started_at) DESC, s.started_at DESC
             LIMIT ?3",
        )
        .bind(ingress_source(ingress))
        .bind(agent_group)
        .bind(limit.clamp(1, 500))
        .fetch_all(self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(id, title, started_at, ended_at, message_count, last_ts, first_user)| {
                    let title = title
                        .filter(|t| !t.trim().is_empty())
                        .or_else(|| first_user.map(|u| title_snippet(&u, SESSION_TITLE_CHARS)))
                        .filter(|t| !t.is_empty())
                        .unwrap_or_else(|| "New chat".to_string());
                    SessionSummary {
                        id,
                        title,
                        started_at,
                        updated_at: last_ts.unwrap_or(started_at),
                        ended_at,
                        message_count,
                    }
                },
            )
            .collect())
    }

    /// Messages with timestamps for display (all roles, ordered by id).
    pub async fn list_history(&self, session_id: &str) -> anyhow::Result<Vec<HistoryMessage>> {
        let rows = sqlx::query_as::<_, (i64, String, String, f64)>(
            "SELECT id, role, COALESCE(content, ''), timestamp FROM messages WHERE session_id = ?1 ORDER BY id ASC",
        )
        .bind(session_id)
        .fetch_all(self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, role, content, timestamp)| HistoryMessage {
                id,
                role,
                content,
                timestamp,
            })
            .collect())
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
        self.insert_message(session_id, role, content, None).await
    }

    /// Append a `compaction` summary row covering every message with `id <= covers_through_id`.
    pub async fn append_compaction(
        &self,
        session_id: &str,
        content: &str,
        covers_through_id: i64,
    ) -> anyhow::Result<()> {
        self.insert_message(session_id, "compaction", content, Some(covers_through_id))
            .await
    }

    async fn insert_message(
        &self,
        session_id: &str,
        role: &str,
        content: &str,
        covers_through_id: Option<i64>,
    ) -> anyhow::Result<()> {
        let now = Utc::now().timestamp_millis() as f64 / 1000.0;
        sqlx::query(
            "INSERT INTO messages (session_id, role, content, timestamp, covers_through_id) VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .bind(session_id)
        .bind(role)
        .bind(content)
        .bind(now)
        .bind(covers_through_id)
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

    /// Full message rows (with ids and compaction boundaries), ordered by id.
    pub async fn list_stored_messages(
        &self,
        session_id: &str,
    ) -> anyhow::Result<Vec<StoredMessage>> {
        let rows = sqlx::query_as::<_, (i64, String, String, Option<i64>)>(
            "SELECT id, role, COALESCE(content, ''), covers_through_id FROM messages WHERE session_id = ?1 ORDER BY id ASC",
        )
        .bind(session_id)
        .fetch_all(self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, role, content, covers_through_id)| StoredMessage {
                id,
                role,
                content,
                covers_through_id,
            })
            .collect())
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

/// A persisted message row. `covers_through_id` is set only on `compaction` rows written
/// after the boundary migration: the id of the last message the summary covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredMessage {
    pub id: i64,
    pub role: String,
    pub content: String,
    pub covers_through_id: Option<i64>,
}

/// Session metadata row.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionInfo {
    pub id: String,
    pub source: String,
    pub agent_group: String,
    pub started_at: f64,
    pub ended_at: Option<f64>,
}

/// One entry of a session list (web UI sidebar).
#[derive(Debug, Clone, PartialEq)]
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    pub started_at: f64,
    /// Timestamp of the latest message, or `started_at` for an empty session.
    pub updated_at: f64,
    pub ended_at: Option<f64>,
    pub message_count: i64,
}

/// A persisted message with its timestamp (display / history view).
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryMessage {
    pub id: i64,
    pub role: String,
    pub content: String,
    pub timestamp: f64,
}

const SESSION_TITLE_CHARS: usize = 60;

/// Single-line, whitespace-collapsed prefix of `text` (max `max` chars, `…` when cut).
fn title_snippet(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    let mut out: String = flat.chars().take(max).collect();
    out.push('…');
    out
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
        IngressKind::Web => "web",
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

    #[tokio::test]
    async fn compaction_row_stores_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let db = StateDb::open(&dir.path().join("state.db")).await.unwrap();
        let store = SessionStore::new(db.pool());
        let sid = store.get_or_create_cli("test").await.unwrap();
        store.append_message(&sid, "user", "a").await.unwrap();
        store.append_message(&sid, "assistant", "b").await.unwrap();
        let rows = store.list_stored_messages(&sid).await.unwrap();
        let boundary = rows[0].id;
        store
            .append_compaction(&sid, "sum", boundary)
            .await
            .unwrap();

        let rows = store.list_stored_messages(&sid).await.unwrap();
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().take(2).all(|r| r.covers_through_id.is_none()));
        assert_eq!(rows[2].role, "compaction");
        assert_eq!(rows[2].covers_through_id, Some(boundary));
        assert_eq!(store.list_messages(&sid).await.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn create_and_list_sessions_for_ingress() {
        let dir = tempfile::tempdir().unwrap();
        let db = StateDb::open(&dir.path().join("state.db")).await.unwrap();
        let store = SessionStore::new(db.pool());

        let a = store
            .create_session("home", IngressKind::Web)
            .await
            .unwrap();
        let b = store
            .create_session("home", IngressKind::Web)
            .await
            .unwrap();
        assert_ne!(a, b);
        // Other ingress / group must not show up.
        let cli = store.get_or_create_cli("home").await.unwrap();
        store.append_message(&cli, "user", "cli msg").await.unwrap();
        let _other = store
            .create_session("work", IngressKind::Web)
            .await
            .unwrap();
        // Timestamps have millisecond resolution; keep ordering deterministic.
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;

        store
            .append_message(&a, "user", "  Deploy   the\nstaging cluster please ")
            .await
            .unwrap();
        store.append_message(&a, "assistant", "done").await.unwrap();

        let list = store
            .list_sessions_for_ingress("home", IngressKind::Web, 50)
            .await
            .unwrap();
        assert_eq!(list.len(), 2);
        // `a` has the latest message, so it sorts first.
        assert_eq!(list[0].id, a);
        assert_eq!(list[0].title, "Deploy the staging cluster please");
        assert_eq!(list[0].message_count, 2);
        assert!(list[0].updated_at >= list[0].started_at);
        assert_eq!(list[1].id, b);
        assert_eq!(list[1].title, "New chat");
        assert_eq!(list[1].updated_at, list[1].started_at);

        let info = store.get_session(&a).await.unwrap().unwrap();
        assert_eq!(info.source, "web");
        assert_eq!(info.agent_group, "home");
        assert!(info.ended_at.is_none());
        assert!(store.get_session("sess_missing").await.unwrap().is_none());

        // Creating a web session does not affect get_or_create_for_ingress for CLI.
        assert_eq!(store.get_or_create_cli("home").await.unwrap(), cli);
    }

    #[tokio::test]
    async fn list_history_has_timestamps_and_roles() {
        let dir = tempfile::tempdir().unwrap();
        let db = StateDb::open(&dir.path().join("state.db")).await.unwrap();
        let store = SessionStore::new(db.pool());
        let sid = store
            .create_session("home", IngressKind::Web)
            .await
            .unwrap();
        store.append_message(&sid, "user", "q").await.unwrap();
        store.append_message(&sid, "assistant", "a").await.unwrap();
        store.append_compaction(&sid, "sum", 1).await.unwrap();
        let rows = store.list_history(&sid).await.unwrap();
        assert_eq!(
            rows.iter().map(|r| r.role.as_str()).collect::<Vec<_>>(),
            vec!["user", "assistant", "compaction"]
        );
        assert!(rows.iter().all(|r| r.timestamp > 0.0));
        assert!(rows.windows(2).all(|w| w[0].id < w[1].id));
    }

    #[test]
    fn title_snippet_truncates_on_chars() {
        assert_eq!(title_snippet("a  b\n c", 10), "a b c");
        assert_eq!(title_snippet("привет мир", 6), "привет…");
    }

    #[tokio::test]
    async fn find_session_does_not_create() {
        use bobaclaw_core::ChannelPeer;

        let dir = tempfile::tempdir().unwrap();
        let db = StateDb::open(&dir.path().join("state.db")).await.unwrap();
        let store = SessionStore::new(db.pool());
        let peer = ChannelPeer::telegram(7, None);
        let req = NormalizedRequest::telegram("", "home", peer, Vec::new());
        assert_eq!(store.find_session(&req).await.unwrap(), None);
        let sid = store.resolve_session(&req).await.unwrap();
        assert_eq!(store.find_session(&req).await.unwrap(), Some(sid));

        let cli = NormalizedRequest::cli("", "home");
        assert_eq!(store.find_session(&cli).await.unwrap(), None);
        let cli_sid = store.resolve_session(&cli).await.unwrap();
        assert_eq!(store.find_session(&cli).await.unwrap(), Some(cli_sid));
    }
}
