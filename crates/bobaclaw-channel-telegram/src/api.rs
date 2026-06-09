use std::path::{Path, PathBuf};

use bobaclaw_core::{TelegramConfig, TelegramFormat};

use crate::format::{format_for_telegram, FormattedMessage, TelegramFormatMode};
use crate::split::{split_for_telegram, utf16_len, TELEGRAM_MAX_UTF16, TELEGRAM_SPLIT_UTF16};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

const API_BASE: &str = "https://api.telegram.org";

#[derive(Clone)]
pub struct TelegramApi {
    client: reqwest::Client,
    token: String,
}

impl TelegramApi {
    pub fn from_config(cfg: &TelegramConfig) -> anyhow::Result<Self> {
        let token = cfg.resolve_bot_token()?;
        Self::new(token, cfg.resolve_proxy())
    }

    pub fn new(token: impl Into<String>, proxy: Option<String>) -> anyhow::Result<Self> {
        let client = build_http_client(proxy.as_deref())?;
        Ok(Self {
            client,
            token: token.into(),
        })
    }

    fn url(&self, method: &str) -> String {
        format!("{API_BASE}/bot{}/{method}", self.token)
    }

    async fn call<T: DeserializeOwned>(
        &self,
        method: &str,
        body: &impl Serialize,
    ) -> anyhow::Result<T> {
        let url = self.url(method);
        let resp = match self.client.post(&url).json(body).send().await {
            Ok(r) => r,
            Err(e) => {
                let mut err = anyhow::anyhow!("telegram {method} request failed: {e}");
                let hint = hint_proxy_connect_error(&e);
                if !hint.is_empty() {
                    err = err.context(hint);
                }
                return Err(err);
            }
        };
        let resp = resp
            .error_for_status()
            .map_err(|e| anyhow::anyhow!("telegram {method} HTTP error: {e}"))?;
        let envelope: ApiEnvelope<T> = resp.json().await?;
        if !envelope.ok {
            anyhow::bail!(
                "telegram {} failed: {}",
                method,
                envelope.description.unwrap_or_default()
            );
        }
        envelope
            .result
            .ok_or_else(|| anyhow::anyhow!("telegram {method}: empty result"))
    }

    pub async fn get_me(&self) -> anyhow::Result<User> {
        #[derive(Serialize)]
        struct Empty {}
        self.call("getMe", &Empty {}).await
    }

    pub async fn set_my_commands(&self) -> anyhow::Result<()> {
        #[derive(Serialize)]
        struct Cmd {
            command: &'static str,
            description: &'static str,
        }
        #[derive(Serialize)]
        struct Body {
            commands: Vec<Cmd>,
        }
        let body = Body {
            commands: vec![
                Cmd {
                    command: "new",
                    description: "Новая сессия (сброс истории)",
                },
                Cmd {
                    command: "help",
                    description: "Справка по командам",
                },
                Cmd {
                    command: "subagents",
                    description: "Фоновые субагенты (spawn)",
                },
            ],
        };
        let _: bool = self.call("setMyCommands", &body).await?;
        Ok(())
    }

    /// Drop webhook so long-polling (`getUpdates`) can receive messages.
    pub async fn delete_webhook(&self) -> anyhow::Result<()> {
        #[derive(Serialize)]
        struct Body {
            drop_pending_updates: bool,
        }
        let _: bool = self
            .call(
                "deleteWebhook",
                &Body {
                    drop_pending_updates: false,
                },
            )
            .await?;
        Ok(())
    }

    pub async fn get_updates(&self, offset: i64, timeout_secs: u32) -> anyhow::Result<Vec<Update>> {
        #[derive(Serialize)]
        struct Body<'a> {
            offset: i64,
            timeout: u32,
            #[serde(rename = "allowed_updates")]
            allowed: &'a [&'a str],
        }
        let body = Body {
            offset,
            timeout: timeout_secs,
            allowed: &["message", "edited_message"],
        };
        self.call("getUpdates", &body).await
    }

    pub async fn send_message(
        &self,
        chat_id: i64,
        text: &str,
        reply_to: Option<i64>,
        thread_id: Option<i64>,
        format: TelegramFormat,
    ) -> anyhow::Result<Message> {
        let parts = split_for_telegram(text, TELEGRAM_SPLIT_UTF16);
        let mut last = None;
        for (i, part) in parts.iter().enumerate() {
            let reply = if i == 0 { reply_to } else { None };
            let thread = if i == 0 { thread_id } else { None };
            last = Some(
                self.send_message_part(chat_id, part, reply, thread, format)
                    .await?,
            );
        }
        last.ok_or_else(|| anyhow::anyhow!("telegram sendMessage: empty text"))
    }

    async fn send_message_part(
        &self,
        chat_id: i64,
        text: &str,
        reply_to: Option<i64>,
        thread_id: Option<i64>,
        format: TelegramFormat,
    ) -> anyhow::Result<Message> {
        let msg = format_for_telegram(text, format_mode(format));
        if utf16_len(&msg.text) <= TELEGRAM_MAX_UTF16 {
            return self
                .send_single_formatted(chat_id, text, reply_to, thread_id, format)
                .await;
        }

        let subparts = split_for_telegram(text, TELEGRAM_SPLIT_UTF16 / 2);
        if subparts.len() <= 1 {
            return self
                .send_single_formatted(chat_id, text, reply_to, thread_id, TelegramFormat::Plain)
                .await;
        }

        let mut last = None;
        for (i, part) in subparts.iter().enumerate() {
            let reply = if i == 0 { reply_to } else { None };
            let thread = if i == 0 { thread_id } else { None };
            last = Some(
                self.send_single_formatted(chat_id, part, reply, thread, format)
                    .await?,
            );
        }
        last.ok_or_else(|| anyhow::anyhow!("telegram sendMessage: empty text"))
    }

    async fn send_single_formatted(
        &self,
        chat_id: i64,
        text: &str,
        reply_to: Option<i64>,
        thread_id: Option<i64>,
        format: TelegramFormat,
    ) -> anyhow::Result<Message> {
        let msg = format_for_telegram(text, format_mode(format));
        match self
            .send_formatted(chat_id, &msg, reply_to, thread_id)
            .await
        {
            Ok(m) => Ok(m),
            Err(e) if format == TelegramFormat::Html && is_telegram_parse_error(&e) => {
                tracing::debug!("telegram HTML parse failed, retrying plain: {e}");
                let plain = format_for_telegram(text, TelegramFormatMode::Plain);
                self.send_formatted(chat_id, &plain, reply_to, thread_id)
                    .await
            }
            Err(e) => Err(e),
        }
    }

    /// Replace a placeholder with the first chunk; send the rest as follow-up messages.
    pub async fn edit_or_send_long(
        &self,
        chat_id: i64,
        message_id: i64,
        text: &str,
        format: TelegramFormat,
    ) -> anyhow::Result<()> {
        let parts = split_for_telegram(text, TELEGRAM_SPLIT_UTF16);
        self.edit_message_part(chat_id, message_id, &parts[0], format)
            .await?;
        for part in parts.iter().skip(1) {
            self.send_message_part(chat_id, part, None, None, format)
                .await?;
        }
        Ok(())
    }

    async fn edit_message_part(
        &self,
        chat_id: i64,
        message_id: i64,
        text: &str,
        format: TelegramFormat,
    ) -> anyhow::Result<()> {
        let msg = format_for_telegram(text, format_mode(format));
        if utf16_len(&msg.text) > TELEGRAM_MAX_UTF16 {
            let subparts = split_for_telegram(text, TELEGRAM_SPLIT_UTF16 / 2);
            if subparts.len() <= 1 {
                let plain = format_for_telegram(text, TelegramFormatMode::Plain);
                return self.edit_formatted(chat_id, message_id, &plain).await;
            }
            self.edit_single_formatted(chat_id, message_id, &subparts[0], format)
                .await?;
            for part in subparts.iter().skip(1) {
                self.send_single_formatted(chat_id, part, None, None, format)
                    .await?;
            }
            return Ok(());
        }
        self.edit_single_formatted(chat_id, message_id, text, format)
            .await
    }

    async fn edit_single_formatted(
        &self,
        chat_id: i64,
        message_id: i64,
        text: &str,
        format: TelegramFormat,
    ) -> anyhow::Result<()> {
        let msg = format_for_telegram(text, format_mode(format));
        match self.edit_formatted(chat_id, message_id, &msg).await {
            Ok(()) => Ok(()),
            Err(e) if format == TelegramFormat::Html && is_telegram_parse_error(&e) => {
                tracing::debug!("telegram HTML edit parse failed, retrying plain: {e}");
                let plain = format_for_telegram(text, TelegramFormatMode::Plain);
                self.edit_formatted(chat_id, message_id, &plain).await
            }
            Err(e) => Err(e),
        }
    }

    pub async fn edit_message_text(
        &self,
        chat_id: i64,
        message_id: i64,
        text: &str,
        format: TelegramFormat,
    ) -> anyhow::Result<()> {
        self.edit_message_part(chat_id, message_id, text, format)
            .await
    }

    pub async fn send_formatted(
        &self,
        chat_id: i64,
        msg: &FormattedMessage,
        reply_to: Option<i64>,
        thread_id: Option<i64>,
    ) -> anyhow::Result<Message> {
        #[derive(Serialize)]
        struct Body<'a> {
            chat_id: i64,
            text: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            parse_mode: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            reply_to_message_id: Option<i64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            message_thread_id: Option<i64>,
        }
        let body = Body {
            chat_id,
            text: ensure_telegram_limit(&msg.text),
            parse_mode: msg.parse_mode,
            reply_to_message_id: reply_to,
            message_thread_id: thread_id,
        };
        self.call("sendMessage", &body).await
    }

    pub async fn edit_formatted(
        &self,
        chat_id: i64,
        message_id: i64,
        msg: &FormattedMessage,
    ) -> anyhow::Result<()> {
        #[derive(Serialize)]
        struct Body<'a> {
            chat_id: i64,
            message_id: i64,
            text: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            parse_mode: Option<&'a str>,
        }
        let body = Body {
            chat_id,
            message_id,
            text: ensure_telegram_limit(&msg.text),
            parse_mode: msg.parse_mode,
        };
        let _: serde_json::Value = self.call("editMessageText", &body).await?;
        Ok(())
    }

    pub async fn send_chat_action(&self, chat_id: i64, action: &str) -> anyhow::Result<()> {
        #[derive(Serialize)]
        struct Body<'a> {
            chat_id: i64,
            action: &'a str,
        }
        let _: serde_json::Value = self
            .call("sendChatAction", &Body { chat_id, action })
            .await?;
        Ok(())
    }

    pub async fn get_file(&self, file_id: &str) -> anyhow::Result<TelegramFile> {
        #[derive(Serialize)]
        struct Body<'a> {
            file_id: &'a str,
        }
        self.call("getFile", &Body { file_id }).await
    }

    pub fn file_download_url(&self, file_path: &str) -> String {
        format!("{API_BASE}/file/bot{}/{file_path}", self.token)
    }

    /// Download a Telegram file by `file_id` into `dest`. Returns `dest` on success.
    pub async fn download_to_path(
        &self,
        file_id: &str,
        dest: &Path,
        default_ext: &str,
    ) -> Option<PathBuf> {
        let info = match self.get_file(file_id).await {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!("telegram getFile({file_id}): {e}");
                return None;
            }
        };
        let file_path = info.file_path.filter(|p| !p.is_empty())?;
        let url = self.file_download_url(&file_path);

        let bytes = match self.client.get(&url).send().await {
            Ok(r) => match r.error_for_status() {
                Ok(r) => match r.bytes().await {
                    Ok(b) => b,
                    Err(e) => {
                        tracing::warn!("telegram download body: {e}");
                        return None;
                    }
                },
                Err(e) => {
                    tracing::warn!("telegram download HTTP: {e}");
                    return None;
                }
            },
            Err(e) => {
                tracing::warn!("telegram download request: {e}");
                return None;
            }
        };

        let mut dest_path = dest.to_path_buf();
        if dest_path.extension().is_none() {
            if !default_ext.is_empty() {
                dest_path.set_extension(default_ext.trim_start_matches('.'));
            } else if let Some(ext) = Path::new(&file_path).extension() {
                dest_path.set_extension(ext);
            }
        }
        self.write_download(&dest_path, &bytes).await
    }

    async fn write_download(&self, dest: &Path, bytes: &[u8]) -> Option<PathBuf> {
        if let Some(parent) = dest.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                tracing::warn!("telegram media mkdir {}: {e}", parent.display());
                return None;
            }
        }
        match std::fs::write(dest, bytes) {
            Ok(()) => Some(dest.to_path_buf()),
            Err(e) => {
                tracing::warn!("telegram media write {}: {e}", dest.display());
                None
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct ApiEnvelope<T> {
    ok: bool,
    result: Option<T>,
    description: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct User {
    pub id: i64,
    pub username: Option<String>,
    pub is_bot: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Chat {
    pub id: i64,
    #[serde(rename = "type")]
    pub chat_type: String,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MessageEntity {
    #[serde(rename = "type")]
    pub entity_type: String,
    pub offset: u32,
    pub length: u32,
    pub user: Option<User>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PhotoSize {
    pub file_id: String,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Document {
    pub file_id: String,
    pub file_name: Option<String>,
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Voice {
    pub file_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Audio {
    pub file_id: String,
    pub file_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Video {
    pub file_id: String,
    pub file_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TelegramFile {
    pub file_id: String,
    pub file_path: Option<String>,
    pub file_size: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Message {
    pub message_id: i64,
    pub chat: Chat,
    pub from: Option<User>,
    pub text: Option<String>,
    pub caption: Option<String>,
    pub document: Option<Document>,
    pub photo: Option<Vec<PhotoSize>>,
    pub voice: Option<Voice>,
    pub audio: Option<Audio>,
    pub video: Option<Video>,
    pub entities: Option<Vec<MessageEntity>>,
    pub reply_to_message: Option<Box<Message>>,
    pub message_thread_id: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Update {
    pub update_id: i64,
    pub message: Option<Message>,
    pub edited_message: Option<Message>,
}

fn hint_proxy_connect_error(err: &reqwest::Error) -> String {
    if err.is_connect() || err.to_string().contains("tunnel") {
        "Check channels.telegram.proxy_url (empty = direct). \
         If the proxy cannot CONNECT to api.telegram.org, clear proxy_url or fix the proxy."
            .into()
    } else {
        String::new()
    }
}

fn build_http_client(proxy: Option<&str>) -> anyhow::Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder();
    if let Some(url) = proxy {
        let proxy = reqwest::Proxy::all(url)
            .map_err(|e| anyhow::anyhow!("invalid channels.telegram.proxy_url: {e}"))?;
        builder = builder.proxy(proxy);
    }
    Ok(builder.build()?)
}

fn is_telegram_parse_error(err: &anyhow::Error) -> bool {
    let s = err.to_string().to_ascii_lowercase();
    s.contains("parse") || s.contains("entity") || s.contains("can't find end")
}

fn format_mode(f: TelegramFormat) -> TelegramFormatMode {
    match f {
        TelegramFormat::Plain => TelegramFormatMode::Plain,
        TelegramFormat::Html => TelegramFormatMode::Html,
    }
}

/// Last-resort guard: split instead of silently dropping the tail.
fn ensure_telegram_limit(text: &str) -> String {
    if utf16_len(text) <= TELEGRAM_MAX_UTF16 {
        return text.to_string();
    }
    tracing::warn!(
        "telegram formatted chunk still exceeds limit ({} UTF-16); hard-splitting",
        utf16_len(text)
    );
    split_for_telegram(text, TELEGRAM_MAX_UTF16)
        .into_iter()
        .next()
        .unwrap_or_default()
}
