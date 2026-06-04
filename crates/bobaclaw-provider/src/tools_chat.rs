use std::time::Duration;

use bobaclaw_core::ProviderConfig;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::openai_compat::content_value_to_string;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    #[serde(rename = "type")]
    pub kind: String,
    pub function: FunctionSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub function: FunctionCallPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCallPayload {
    pub name: String,
    pub arguments: String,
}

/// OpenAI-compatible conversation message (chat + tool loop).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl ConversationMessage {
    pub fn system(text: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: Some(Value::String(text.into())),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: Some(Value::String(text.into())),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    pub fn assistant_text(text: impl Into<String>) -> Self {
        Self {
            role: "assistant".into(),
            content: Some(Value::String(text.into())),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    pub fn tool_result(tool_call_id: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            role: "tool".into(),
            content: Some(Value::String(body.into())),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
            name: None,
        }
    }

    pub fn text_content(&self) -> String {
        self.content
            .as_ref()
            .map(content_value_to_string)
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone)]
pub struct ChatTurnResult {
    pub message: ConversationMessage,
    pub finish_reason: Option<String>,
}

pub struct ToolChatClient {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
}

impl ToolChatClient {
    pub fn from_provider(cfg: &ProviderConfig, api_key: String) -> anyhow::Result<Self> {
        let timeout = Duration::from_secs(cfg.request_timeout_secs.max(5));
        let client = reqwest::Client::builder().timeout(timeout).build()?;
        Ok(Self {
            client,
            base_url: cfg.base_url.trim_end_matches('/').to_string(),
            api_key,
            model: cfg.model.clone(),
        })
    }

    pub async fn chat_turn(
        &self,
        messages: &[ConversationMessage],
        tools: &[ToolSpec],
        model_override: Option<&str>,
    ) -> anyhow::Result<ChatTurnResult> {
        let model = model_override.unwrap_or(&self.model);
        let url = format!("{}/chat/completions", self.base_url);

        #[derive(Serialize)]
        struct Request<'a> {
            model: &'a str,
            messages: &'a [ConversationMessage],
            tools: &'a [ToolSpec],
            tool_choice: &'static str,
        }

        #[derive(Deserialize)]
        struct Choice {
            message: ApiTurnMessage,
            finish_reason: Option<String>,
        }

        #[derive(Deserialize)]
        struct Response {
            choices: Vec<Choice>,
        }

        #[derive(Deserialize)]
        struct ApiTurnMessage {
            role: Option<String>,
            #[serde(default)]
            content: Option<Value>,
            #[serde(default)]
            tool_calls: Option<Vec<ToolCall>>,
            #[serde(default)]
            reasoning: Option<String>,
        }

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&Request {
                model,
                messages,
                tools,
                tool_choice: "auto",
            })
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("provider error {status}: {body}");
        }

        let parsed: Response = resp.json().await?;
        let choice = parsed
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("empty choices from provider"))?;

        let mut content = choice.message.content;
        if content.as_ref().map(content_value_to_string).unwrap_or_default().trim().is_empty() {
            if let Some(reasoning) = choice.message.reasoning {
                content = Some(Value::String(reasoning));
            }
        }

        let message = ConversationMessage {
            role: choice
                .message
                .role
                .unwrap_or_else(|| "assistant".to_string()),
            content,
            tool_calls: choice.message.tool_calls,
            tool_call_id: None,
            name: None,
        };

        Ok(ChatTurnResult {
            message,
            finish_reason: choice.finish_reason,
        })
    }

    /// Text-only completion (summarization / compaction).
    pub async fn complete_text(
        &self,
        messages: &[ConversationMessage],
        model_override: Option<&str>,
    ) -> anyhow::Result<String> {
        let turn = self.chat_turn(messages, &[], model_override).await?;
        let text = turn.message.text_content();
        if text.trim().is_empty() {
            anyhow::bail!("empty completion from provider");
        }
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_result_message_shape() {
        let m = ConversationMessage::tool_result("call_1", "output");
        assert_eq!(m.role, "tool");
        assert_eq!(m.tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(m.text_content(), "output");
    }

    #[test]
    fn system_and_user_text() {
        assert_eq!(
            ConversationMessage::system("sys").text_content(),
            "sys"
        );
        assert_eq!(ConversationMessage::user("hi").text_content(), "hi");
    }
}
