use std::sync::Mutex;
use std::time::{Duration, Instant};

use bobaclaw_agent::{AgentEvent, AgentProgress};
use bobaclaw_core::TelegramFormat;

use crate::api::TelegramApi;
use crate::format::{format_for_telegram, TelegramFormatMode};
use crate::status::{format_activity, initial_activity, stream_message};

/// Streams agent status via Telegram `editMessageText`; final answer is formatted separately.
pub struct TelegramStream {
    api: TelegramApi,
    chat_id: i64,
    message_id: i64,
    format: TelegramFormat,
    interval: Duration,
    last_edit: Mutex<Instant>,
    activity: Mutex<String>,
}

impl TelegramStream {
    pub fn new(
        api: TelegramApi,
        chat_id: i64,
        message_id: i64,
        interval_ms: u64,
        format: TelegramFormat,
    ) -> Self {
        Self {
            api,
            chat_id,
            message_id,
            format,
            interval: Duration::from_millis(interval_ms.max(300)),
            last_edit: Mutex::new(Instant::now() - Duration::from_secs(60)),
            activity: Mutex::new(initial_activity().into()),
        }
    }

    fn maybe_edit(&self, force: bool) {
        let mut last = self.last_edit.lock().unwrap();
        if !force && last.elapsed() < self.interval {
            return;
        }
        let activity = self.activity.lock().unwrap().clone();
        let body = stream_message(&activity);
        let api = self.api.clone();
        let chat_id = self.chat_id;
        let message_id = self.message_id;
        let msg = format_for_telegram(&body, TelegramFormatMode::Plain);
        *last = Instant::now();
        tokio::spawn(async move {
            if let Err(e) = api.edit_formatted(chat_id, message_id, &msg).await {
                tracing::debug!("telegram stream edit: {e}");
            }
        });
    }

    fn set_activity(&self, line: String) {
        *self.activity.lock().unwrap() = line;
        self.maybe_edit(false);
    }

    pub async fn finalize(&self, final_text: &str) -> anyhow::Result<()> {
        self.api
            .edit_message_text(self.chat_id, self.message_id, final_text, self.format)
            .await?;
        Ok(())
    }
}

impl AgentProgress for TelegramStream {
    fn on_event(&self, event: AgentEvent) {
        // Do not interleave assistant token stream into the status message.
        self.set_activity(format_activity(&event));
    }
}
