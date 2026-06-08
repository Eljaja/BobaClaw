use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
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
    /// When true, in-flight progress edits must not touch the placeholder message.
    finalized: Arc<AtomicBool>,
    /// Bumped on finalize and before each progress edit; stale edits are dropped.
    edit_generation: Arc<AtomicU64>,
}

/// Telegram progress shows tool/status lines only — not partial assistant prose.
pub(crate) fn telegram_progress_event(event: &AgentEvent) -> bool {
    !matches!(event, AgentEvent::AssistantChunk { .. })
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
            finalized: Arc::new(AtomicBool::new(false)),
            edit_generation: Arc::new(AtomicU64::new(0)),
        }
    }

    fn maybe_edit(&self, force: bool) {
        if self.finalized.load(Ordering::Acquire) {
            return;
        }
        let mut last = self.last_edit.lock().unwrap();
        if !force && last.elapsed() < self.interval {
            return;
        }
        let body = stream_message(&render_activity_log(&self.activity));
        let api = self.api.clone();
        let chat_id = self.chat_id;
        let message_id = self.message_id;
        let msg = format_for_telegram(&body, TelegramFormatMode::Plain);
        let generation = self.edit_generation.fetch_add(1, Ordering::AcqRel) + 1;
        let gen = Arc::clone(&self.edit_generation);
        let fin = Arc::clone(&self.finalized);
        *last = Instant::now();
        tokio::spawn(async move {
            let generation_ok = gen.load(Ordering::Acquire) == generation;
            let not_finalized = !fin.load(Ordering::Acquire);
            if generation_ok && not_finalized {
                if let Err(e) = api.edit_formatted(chat_id, message_id, &msg).await {
                    tracing::debug!("telegram stream edit: {e}");
                }
            }
        });
    }

    fn append_event(&self, event: &AgentEvent) {
        if !telegram_progress_event(event) {
            return;
        }
        self.activity.push_event(event);
        self.maybe_edit(false);
    }

    /// Replace the placeholder with the first chunk; overflow goes to follow-up messages.
    pub async fn finalize_with_fallback(&self, final_text: &str) -> anyhow::Result<()> {
        self.finalized.store(true, Ordering::Release);
        self.edit_generation.fetch_add(1, Ordering::AcqRel);
        match self
            .api
            .edit_or_send_long(self.chat_id, self.message_id, final_text, self.format)
            .await
        {
            Ok(()) => Ok(()),
            Err(first) => self
                .api
                .edit_or_send_long(
                    self.chat_id,
                    self.message_id,
                    final_text,
                    TelegramFormat::Plain,
                )
                .await
                .map_err(|second| {
                    second.context(format!(
                        "telegram finalize failed after HTML retry: {first}"
                    ))
                }),
        }
    }
}

impl AgentProgress for TelegramStream {
    fn on_event(&self, event: AgentEvent) {
        self.append_event(&event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bobaclaw_agent::AgentEvent;

    #[test]
    fn progress_skips_assistant_chunks() {
        assert!(telegram_progress_event(&AgentEvent::LlmThinking {
            iteration: 1
        }));
        assert!(telegram_progress_event(&AgentEvent::ToolStart {
            name: "exec".into(),
            label: "ls".into(),
        }));
        assert!(!telegram_progress_event(&AgentEvent::AssistantChunk {
            text: "partial answer".into(),
        }));
    }
}
