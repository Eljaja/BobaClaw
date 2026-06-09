use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use bobaclaw_state::SpawnJobRecord;

#[async_trait]
pub trait ChannelDelivery: Send + Sync {
    async fn notify_spawn_complete(
        &self,
        job: &SpawnJobRecord,
        summary: &str,
    ) -> anyhow::Result<()>;
    async fn present_wake_reply(&self, job: &SpawnJobRecord, reply: &str) -> anyhow::Result<()>;
}

#[derive(Default)]
pub struct DeliveryRegistry {
    channels: HashMap<String, Arc<dyn ChannelDelivery>>,
}

impl DeliveryRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, channel: impl Into<String>, delivery: Arc<dyn ChannelDelivery>) {
        self.channels.insert(channel.into(), delivery);
    }

    pub fn get(&self, channel: &str) -> Option<Arc<dyn ChannelDelivery>> {
        self.channels.get(channel).cloned()
    }
}

/// Register default CLI/API outbox and optional Telegram delivery.
pub fn build_delivery_registry(
    paths_home: std::path::PathBuf,
    telegram: Option<Arc<dyn ChannelDelivery>>,
) -> Arc<DeliveryRegistry> {
    let mut reg = DeliveryRegistry::new();
    let outbox = Arc::new(OutboxChannelDelivery::new(paths_home));
    reg.register("cli", outbox.clone());
    reg.register("api", outbox);
    if let Some(tg) = telegram {
        reg.register("telegram", tg);
    }
    Arc::new(reg)
}

/// CLI / api: write to outbox only (no external API).
pub struct OutboxChannelDelivery {
    paths_home: std::path::PathBuf,
}

impl OutboxChannelDelivery {
    pub fn new(paths_home: std::path::PathBuf) -> Self {
        Self { paths_home }
    }

    fn write_outbox(&self, text: &str) -> anyhow::Result<()> {
        let outbox = self.paths_home.join("outbox");
        std::fs::create_dir_all(&outbox)?;
        let path = outbox.join(format!("spawn_{}.txt", chrono::Utc::now().timestamp()));
        std::fs::write(&path, text)?;
        tracing::info!("spawn delivery written to {}", path.display());
        Ok(())
    }
}

#[async_trait]
impl ChannelDelivery for OutboxChannelDelivery {
    async fn notify_spawn_complete(
        &self,
        job: &SpawnJobRecord,
        summary: &str,
    ) -> anyhow::Result<()> {
        let label = job.label.as_deref().unwrap_or(&job.id);
        let text = format!(
            "[Spawn `{label}` completed]\n{summary}\n(task_id={})",
            job.id
        );
        self.write_outbox(&text)
    }

    async fn present_wake_reply(&self, job: &SpawnJobRecord, reply: &str) -> anyhow::Result<()> {
        let label = job.label.as_deref().unwrap_or(&job.id);
        let text = format!("[Spawn wake `{label}`]\n\n{reply}");
        self.write_outbox(&text)
    }
}
