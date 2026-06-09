use std::sync::Arc;

use async_trait::async_trait;
use bobaclaw_agent::ChannelDelivery;
use bobaclaw_core::BobaConfig;
use bobaclaw_state::SpawnJobRecord;

use crate::api::TelegramApi;
use crate::split::{split_for_telegram, TELEGRAM_SPLIT_UTF16};

pub struct TelegramChannelDelivery {
    config: BobaConfig,
    api: Arc<TelegramApi>,
}

impl TelegramChannelDelivery {
    pub fn new(config: BobaConfig, api: Arc<TelegramApi>) -> Self {
        Self { config, api }
    }

    fn chat_id(job: &SpawnJobRecord) -> anyhow::Result<i64> {
        let peer = job
            .deliver_peer
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("telegram delivery missing peer"))?;
        peer.parse()
            .map_err(|e| anyhow::anyhow!("invalid telegram chat id {peer}: {e}"))
    }

    fn thread_id(job: &SpawnJobRecord) -> Option<i64> {
        job.deliver_thread_id
            .as_deref()
            .and_then(|t| t.parse().ok())
    }
}

#[async_trait]
impl ChannelDelivery for TelegramChannelDelivery {
    async fn notify_spawn_complete(
        &self,
        job: &SpawnJobRecord,
        summary: &str,
    ) -> anyhow::Result<()> {
        let chat_id = Self::chat_id(job)?;
        let label = job.label.as_deref().unwrap_or(&job.id);
        let text = format!(
            "✅ Subagent `{label}` finished ({status})\n{summary}\n(id: {})",
            job.id,
            status = job.status
        );
        let fmt = self.config.channels.telegram.format;
        self.api
            .send_message(chat_id, &text, None, Self::thread_id(job), fmt)
            .await?;
        Ok(())
    }

    async fn present_wake_reply(&self, job: &SpawnJobRecord, reply: &str) -> anyhow::Result<()> {
        let chat_id = Self::chat_id(job)?;
        let label = job.label.as_deref().unwrap_or(&job.id);
        let header = format!("[Spawn wake `{label}`]\n\n");
        let full = format!("{header}{reply}");
        let fmt = self.config.channels.telegram.format;
        let thread = Self::thread_id(job);

        for chunk in split_for_telegram(&full, TELEGRAM_SPLIT_UTF16) {
            self.api
                .send_message(chat_id, &chunk, None, thread, fmt)
                .await?;
        }
        Ok(())
    }
}
