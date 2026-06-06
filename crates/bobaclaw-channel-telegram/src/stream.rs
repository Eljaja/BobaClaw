use std::sync::Mutex;
use std::time::{Duration, Instant};

use bobaclaw_agent::{ActivityLog, AgentEvent, AgentProgress};
use bobaclaw_core::TelegramFormat;

use crate::api::TelegramApi;
use crate::format::{format_for_telegram, TelegramFormatMode};
use crate::status::{render_activity_log, stream_message};

/// Streams agent status via Telegram `editMessageText`; final answer is formatted separately.
pub struct TelegramStream {
    api: TelegramApi,
    chat_id: i64,
    message_id: i64,
    format: TelegramFormat,
    interval: Duration,
    last_edit: Mutex<Instant>,
    activity: ActivityLog,
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
            activity: ActivityLog::new(),
        }
    }

    fn maybe_edit(&self, force: bool) {
        let mut last = self.last_edit.lock().unwrap();
        if !force && last.elapsed() < self.interval {
            return;
        }
        let body = stream_message(&render_activity_log(&self.activity));
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

    fn append_event(&self, event: &AgentEvent) {
        self.activity.push_event(event);
        self.maybe_edit(false);
    }

    /// Replace the placeholder with the final answer; retry plain and truncated edits before failing.
    pub async fn finalize_with_fallback(&self, final_text: &str) -> anyhow::Result<()> {
        const TELEGRAM_SAFE: usize = 4000;

        if self
            .api
            .edit_message_text(self.chat_id, self.message_id, final_text, self.format)
            .await
            .is_ok()
        {
            return Ok(());
        }

        if final_text.chars().count() > TELEGRAM_SAFE {
            let truncated = truncate_utf8_prefix(final_text, TELEGRAM_SAFE.saturating_sub(64));
            let with_note = format!("{truncated}\n\n… (truncated for Telegram)");
            if self
                .api
                .edit_message_text(
                    self.chat_id,
                    self.message_id,
                    &with_note,
                    TelegramFormat::Plain,
                )
                .await
                .is_ok()
            {
                return Ok(());
            }
        }

        self.api
            .edit_message_text(
                self.chat_id,
                self.message_id,
                final_text,
                TelegramFormat::Plain,
            )
            .await
    }
}

fn truncate_utf8_prefix(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

impl AgentProgress for TelegramStream {
    fn on_event(&self, event: AgentEvent) {
        self.append_event(&event);
    }
}
