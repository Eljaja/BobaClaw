use bobaclaw_core::{TelegramConfig, TelegramFormat};

use crate::format::{format_for_telegram, FormattedMessage, TelegramFormatMode};
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

    async fn call<T: DeserializeOwned>(&self, method: &str, body: &impl Serialize) -> anyhow::Result<T> {
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
        let resp = resp.error_for_status().map_err(|e| {
            anyhow::anyhow!("telegram {method} HTTP error: {e}")
        })?;
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

    pub async fn get_updates(
        &self,
        offset: i64,
        timeout_secs: u32,
    ) -> anyhow::Result<Vec<Update>> {
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
        let msg = format_for_telegram(text, format_mode(format));
        match self.send_formatted(chat_id, &msg, reply_to, thread_id).await {
            Ok(m) => Ok(m),
            Err(e) if format == TelegramFormat::Html && is_telegram_parse_error(&e) => {
                tracing::debug!("telegram HTML parse failed, retrying plain: {e}");
                let plain = format_for_telegram(text, TelegramFormatMode::Plain);
                self.send_formatted(chat_id, &plain, reply_to, thread_id).await
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
            text: truncate_telegram(&msg.text),
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
            text: truncate_telegram(&msg.text),
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
        let _: serde_json::Value = self.call("sendChatAction", &Body { chat_id, action }).await?;
        Ok(())
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
pub struct Message {
    pub message_id: i64,
    pub chat: Chat,
    pub from: Option<User>,
    pub text: Option<String>,
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

pub fn truncate_telegram(text: &str) -> String {
    const MAX: usize = 4096;
    if text.len() <= MAX {
        return text.to_string();
    }
    let mut end = MAX;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}
