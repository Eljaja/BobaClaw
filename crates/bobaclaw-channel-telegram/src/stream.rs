use std::sync::Mutex;
use std::time::{Duration, Instant};

use bobaclaw_agent::{AgentEvent, AgentProgress};

use crate::api::{truncate_telegram, TelegramApi};

/// Streams agent status and reply text via Telegram `editMessageText`.
pub struct TelegramStream {
    api: TelegramApi,
    chat_id: i64,
    message_id: i64,
    interval: Duration,
    last_edit: Mutex<Instant>,
    status: Mutex<String>,
    draft: Mutex<String>,
}

impl TelegramStream {
    pub fn new(
        api: TelegramApi,
        chat_id: i64,
        message_id: i64,
        interval_ms: u64,
    ) -> Self {
        Self {
            api,
            chat_id,
            message_id,
            interval: Duration::from_millis(interval_ms.max(300)),
            last_edit: Mutex::new(Instant::now() - Duration::from_secs(60)),
            status: Mutex::new("…".into()),
            draft: Mutex::new(String::new()),
        }
    }

    fn maybe_edit(&self, force: bool) {
        let mut last = self.last_edit.lock().unwrap();
        if !force && last.elapsed() < self.interval {
            return;
        }
        let status = self.status.lock().unwrap().clone();
        let draft = self.draft.lock().unwrap().clone();
        let body = if draft.is_empty() {
            status
        } else {
            format!("{status}\n\n{draft}")
        };
        let api = self.api.clone();
        let chat_id = self.chat_id;
        let message_id = self.message_id;
        let text = truncate_telegram(&body);
        *last = Instant::now();
        tokio::spawn(async move {
            if let Err(e) = api.edit_message_text(chat_id, message_id, &text).await {
                tracing::debug!("telegram stream edit: {e}");
            }
        });
    }

    pub fn set_status(&self, s: impl Into<String>) {
        *self.status.lock().unwrap() = s.into();
        self.maybe_edit(false);
    }

    pub fn push_draft(&self, chunk: &str) {
        self.draft.lock().unwrap().push_str(chunk);
        self.maybe_edit(false);
    }

    pub async fn finalize(&self, final_text: &str) -> anyhow::Result<()> {
        let text = truncate_telegram(final_text);
        self.api
            .edit_message_text(self.chat_id, self.message_id, &text)
            .await?;
        Ok(())
    }
}

impl AgentProgress for TelegramStream {
    fn on_event(&self, event: AgentEvent) {
        match &event {
            AgentEvent::AssistantChunk { text } => {
                self.push_draft(text);
            }
            _ => {
                self.set_status(event.to_string());
            }
        }
    }
}
