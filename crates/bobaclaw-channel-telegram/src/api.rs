use bobaclaw_core::TelegramConfig;
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
        let resp = self
            .client
            .post(self.url(method))
            .json(body)
            .send()
            .await?
            .error_for_status()?;
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
    ) -> anyhow::Result<Message> {
        #[derive(Serialize)]
        struct Body {
            chat_id: i64,
            text: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            reply_to_message_id: Option<i64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            message_thread_id: Option<i64>,
        }
        let body = Body {
            chat_id,
            text: truncate_telegram(text),
            reply_to_message_id: reply_to,
            message_thread_id: thread_id,
        };
        self.call("sendMessage", &body).await
    }

    pub async fn edit_message_text(
        &self,
        chat_id: i64,
        message_id: i64,
        text: &str,
    ) -> anyhow::Result<()> {
        #[derive(Serialize)]
        struct Body {
            chat_id: i64,
            message_id: i64,
            text: String,
        }
        let body = Body {
            chat_id,
            message_id,
            text: truncate_telegram(text),
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

fn build_http_client(proxy: Option<&str>) -> anyhow::Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder();
    if let Some(url) = proxy {
        let proxy = reqwest::Proxy::all(url)
            .map_err(|e| anyhow::anyhow!("invalid channels.telegram.proxy_url: {e}"))?;
        builder = builder.proxy(proxy);
    }
    Ok(builder.build()?)
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
