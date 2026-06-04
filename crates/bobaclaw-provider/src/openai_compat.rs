use async_trait::async_trait;
use bobaclaw_core::ProviderConfig;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

pub struct OpenAiCompatProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
}

impl OpenAiCompatProvider {
    pub fn new(cfg: &ProviderConfig, api_key: String) -> anyhow::Result<Self> {
        let timeout = std::time::Duration::from_secs(cfg.request_timeout_secs.max(5));
        let client = reqwest::Client::builder().timeout(timeout).build()?;
        Ok(Self {
            client,
            base_url: cfg.base_url.trim_end_matches('/').to_string(),
            api_key,
            model: cfg.model.clone(),
        })
    }

    pub async fn chat_completion(
        &self,
        messages: Vec<ChatMessage>,
        model_override: Option<&str>,
    ) -> anyhow::Result<String> {
        let model = model_override.unwrap_or(&self.model);
        let url = format!("{}/chat/completions", self.base_url);

        #[derive(Serialize)]
        struct Request<'a> {
            model: &'a str,
            messages: Vec<ChatMessage>,
        }

        #[derive(Deserialize)]
        struct Choice {
            message: ApiMessage,
        }

        #[derive(Deserialize)]
        struct Response {
            choices: Vec<Choice>,
        }

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&Request { model, messages })
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("provider error {status}: {body}");
        }

        let parsed: Response = resp.json().await?;
        parsed
            .choices
            .into_iter()
            .next()
            .map(|c| extract_assistant_text(&c.message))
            .ok_or_else(|| anyhow::anyhow!("empty choices from provider"))
    }
}

pub(crate) fn content_value_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(parts) => parts
            .iter()
            .filter_map(|p| {
                p.get("text")
                    .and_then(|t| t.as_str())
                    .or_else(|| p.as_str())
            })
            .collect::<Vec<_>>()
            .join("\n"),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn extract_assistant_text(msg: &ApiMessage) -> String {
    let from_content = msg
        .content
        .as_ref()
        .map(content_value_to_string)
        .unwrap_or_default();
    if !from_content.trim().is_empty() {
        return from_content;
    }
    msg.reasoning.clone().unwrap_or_default()
}

#[derive(Deserialize)]
struct ApiMessage {
    role: String,
    #[serde(default)]
    content: Option<serde_json::Value>,
    #[serde(default)]
    reasoning: Option<String>,
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(&self, messages: Vec<ChatMessage>) -> anyhow::Result<String>;
}

#[async_trait]
impl LlmProvider for OpenAiCompatProvider {
    async fn complete(&self, messages: Vec<ChatMessage>) -> anyhow::Result<String> {
        self.chat_completion(messages, None).await
    }
}
