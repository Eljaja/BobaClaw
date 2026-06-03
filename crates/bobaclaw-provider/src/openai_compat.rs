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
    pub fn new(cfg: &ProviderConfig, api_key: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: cfg.base_url.trim_end_matches('/').to_string(),
            api_key,
            model: cfg.model.clone(),
        }
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
            message: ChatMessage,
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
            .map(|c| c.message.content)
            .ok_or_else(|| anyhow::anyhow!("empty choices from provider"))
    }
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
