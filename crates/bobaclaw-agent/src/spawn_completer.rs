use std::sync::Arc;

use bobaclaw_core::{BobaConfig, ChannelPeer, IngressKind, NormalizedRequest};
use bobaclaw_state::{SessionStore, SpawnJobRecord, SpawnJobStore};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::channel_delivery::DeliveryRegistry;
use crate::dispatcher::AgentDispatcher;
use crate::subagent::SubagentRunResult;

pub struct SpawnCompleter {
    config: BobaConfig,
    pool: SqlitePool,
    dispatcher: Arc<AgentDispatcher>,
    deliveries: Arc<DeliveryRegistry>,
}

impl SpawnCompleter {
    pub fn new(
        config: BobaConfig,
        pool: SqlitePool,
        dispatcher: Arc<AgentDispatcher>,
        deliveries: Arc<DeliveryRegistry>,
    ) -> Self {
        Self {
            config,
            pool,
            dispatcher,
            deliveries,
        }
    }

    pub async fn on_complete(
        &self,
        job_id: &str,
        result: &anyhow::Result<SubagentRunResult>,
        cancelled: bool,
    ) -> anyhow::Result<()> {
        let store = SpawnJobStore::new(&self.pool);
        let Some(mut job) = store.get(job_id).await? else {
            return Ok(());
        };

        let spawn_cfg = &self.config.subagents.spawn;
        let (status, exit_code, preview, body, deliver_result) = if cancelled {
            (
                "cancelled",
                None,
                Some("cancelled with parent turn".to_string()),
                None,
                false,
            )
        } else {
            match result {
                Ok(r) if r.exit_code == 0 => {
                    let preview = truncate(&r.body, 200);
                    let body = truncate(&r.body, spawn_cfg.result_persist_chars);
                    (
                        status_label(r.exit_code),
                        Some(r.exit_code),
                        Some(preview),
                        Some(body),
                        true,
                    )
                }
                Ok(r) => {
                    let preview = truncate(&r.body, 200);
                    let body = truncate(&r.body, spawn_cfg.result_persist_chars);
                    (
                        status_label(r.exit_code),
                        Some(r.exit_code),
                        Some(preview),
                        Some(body),
                        false,
                    )
                }
                Err(e) => (
                    "failed",
                    None,
                    Some(truncate(&e.to_string(), 200)),
                    None,
                    false,
                ),
            }
        };

        store
            .finalize(
                job_id,
                status,
                exit_code,
                preview.as_deref(),
                body.as_deref(),
            )
            .await?;
        job.status = status.to_string();
        job.exit_code = exit_code;
        job.result_preview = preview.clone();
        job.result_body = body.clone();

        if deliver_result {
            if let Ok(r) = result {
                let label = job.label.as_deref().unwrap_or(job_id);
                let delivery = format!("[Subagent `{label}` completed]\n\n{}", r.body);
                SessionStore::new(&self.pool)
                    .append_message(&job.session_id, "assistant", &delivery)
                    .await?;
            }
        }

        if spawn_cfg.notify_on_complete && job.notified_at.is_none() {
            if let Some(channel) = job.deliver_channel.as_deref() {
                if let Some(delivery) = self.deliveries.get(channel) {
                    let summary = preview.as_deref().unwrap_or(status);
                    if delivery.notify_spawn_complete(&job, summary).await.is_ok() {
                        store.mark_notified(job_id).await.ok();
                    }
                }
            }
        }

        let should_wake = job.wake_parent
            && spawn_cfg.wake_parent_on_complete
            && (status == "completed" || (spawn_cfg.wake_on_failure && status == "failed"));

        if !should_wake {
            return Ok(());
        }

        let scope = format!("session:{}", job.session_id);
        if self.dispatcher.is_scope_busy(&scope).await {
            tracing::info!("spawn wake skipped: session {scope} busy");
            return Ok(());
        }

        let since = chrono::Utc::now().timestamp_millis() as f64 / 1000.0 - 3600.0;
        let wake_count = store.count_recent_wakes(&job.session_id, since).await?;
        if wake_count as u32 >= spawn_cfg.wake_max_per_hour_per_session {
            tracing::warn!("spawn wake rate limit for session {}", job.session_id);
            return Ok(());
        }

        let label = job.label.as_deref().unwrap_or(job_id);
        let result_text = preview.as_deref().unwrap_or("(no preview)");
        let wake_text = format!(
            "[Background subagent `{label}` ({job_id}) completed]\n\n{result_text}\n\n\
             Integrate this result for the user. Do not spawn another background agent unless clearly needed."
        );

        let channel_peer = build_channel_peer(&job);
        let req = NormalizedRequest {
            request_id: Uuid::new_v4(),
            ingress: IngressKind::SpawnWake,
            agent_group: job.agent_group.clone(),
            session_id: Some(job.session_id.clone()),
            channel_peer,
            user_text: wake_text,
            attachments: Vec::new(),
            model_override: None,
        };

        match self.dispatcher.handle(req).await {
            Ok(resp) => {
                if let Some(channel) = job.deliver_channel.as_deref() {
                    if let Some(delivery) = self.deliveries.get(channel) {
                        let _ = delivery.present_wake_reply(&job, &resp.text).await;
                    }
                }
            }
            Err(e) => tracing::warn!("spawn wake turn failed for {job_id}: {e}"),
        }

        Ok(())
    }
}

fn build_channel_peer(job: &SpawnJobRecord) -> Option<ChannelPeer> {
    let peer = job.deliver_peer.as_deref()?;
    let channel = job.deliver_channel.as_deref().unwrap_or("telegram");
    if channel != "telegram" {
        return None;
    }
    Some(ChannelPeer {
        channel: "telegram".into(),
        peer: peer.to_string(),
        thread_id: job.deliver_thread_id.clone(),
    })
}

fn status_label(exit_code: i32) -> &'static str {
    if exit_code == 0 {
        "completed"
    } else {
        "failed"
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}
