use chrono::Utc;
use sqlx::SqlitePool;

pub struct PairingStore<'a> {
    pool: &'a SqlitePool,
}

impl<'a> PairingStore<'a> {
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn is_approved(&self, channel: &str, peer: &str) -> anyhow::Result<bool> {
        let n = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM pairing_requests WHERE channel = ?1 AND peer = ?2 AND status = 'approved'",
        )
        .bind(channel)
        .bind(peer)
        .fetch_one(self.pool)
        .await?;
        Ok(n > 0)
    }

    pub async fn pending_code(&self, channel: &str, peer: &str) -> anyhow::Result<Option<String>> {
        let code = sqlx::query_scalar::<_, String>(
            "SELECT code FROM pairing_requests WHERE channel = ?1 AND peer = ?2 AND status = 'pending' ORDER BY created_at DESC LIMIT 1",
        )
        .bind(channel)
        .bind(peer)
        .fetch_optional(self.pool)
        .await?;
        Ok(code)
    }

    pub async fn create_or_get_pending(
        &self,
        channel: &str,
        peer: &str,
        display_name: Option<&str>,
    ) -> anyhow::Result<String> {
        if let Some(code) = self.pending_code(channel, peer).await? {
            return Ok(code);
        }
        let code = generate_pairing_code();
        let now = Utc::now().timestamp_millis() as f64 / 1000.0;
        sqlx::query(
            "INSERT INTO pairing_requests (channel, peer, code, display_name, status, created_at) VALUES (?1, ?2, ?3, ?4, 'pending', ?5)",
        )
        .bind(channel)
        .bind(peer)
        .bind(&code)
        .bind(display_name)
        .bind(now)
        .execute(self.pool)
        .await?;
        Ok(code)
    }

    pub async fn approve_by_code(&self, channel: &str, code: &str) -> anyhow::Result<Option<String>> {
        let now = Utc::now().timestamp_millis() as f64 / 1000.0;
        let peer: Option<String> = sqlx::query_scalar(
            "SELECT peer FROM pairing_requests WHERE channel = ?1 AND code = ?2 AND status = 'pending'",
        )
        .bind(channel)
        .bind(code)
        .fetch_optional(self.pool)
        .await?;

        let Some(peer) = peer else {
            return Ok(None);
        };

        sqlx::query(
            "UPDATE pairing_requests SET status = 'approved', resolved_at = ?1 WHERE channel = ?2 AND code = ?3 AND status = 'pending'",
        )
        .bind(now)
        .bind(channel)
        .bind(code)
        .execute(self.pool)
        .await?;

        sqlx::query(
            "UPDATE pairing_requests SET status = 'superseded', resolved_at = ?1 WHERE channel = ?2 AND peer = ?3 AND status = 'pending' AND code != ?4",
        )
        .bind(now)
        .bind(channel)
        .bind(&peer)
        .bind(code)
        .execute(self.pool)
        .await?;

        Ok(Some(peer))
    }

    pub async fn list_pending(&self, channel: Option<&str>) -> anyhow::Result<Vec<PairingRow>> {
        let rows = if let Some(ch) = channel {
            sqlx::query_as::<_, PairingRow>(
                "SELECT channel, peer, code, COALESCE(display_name,''), created_at FROM pairing_requests WHERE status = 'pending' AND channel = ?1 ORDER BY created_at DESC",
            )
            .bind(ch)
            .fetch_all(self.pool)
            .await?
        } else {
            sqlx::query_as::<_, PairingRow>(
                "SELECT channel, peer, code, COALESCE(display_name,''), created_at FROM pairing_requests WHERE status = 'pending' ORDER BY created_at DESC",
            )
            .fetch_all(self.pool)
            .await?
        };
        Ok(rows)
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PairingRow {
    pub channel: String,
    pub peer: String,
    pub code: String,
    pub display_name: String,
    pub created_at: f64,
}

fn generate_pairing_code() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{:06}", n % 1_000_000)
}
