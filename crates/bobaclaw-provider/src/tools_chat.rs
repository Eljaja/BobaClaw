use std::time::Duration;

use bobaclaw_core::ProviderConfig;
use reqwest::StatusCode;
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

/// ASI Cloud (and some OpenAI-compatible gateways) reject requests when `user` or `tool`
/// messages omit `content` or send JSON `null`. Assistant messages may omit content.
pub fn normalize_messages_for_provider(messages: &mut [ConversationMessage]) {
    for msg in messages.iter_mut() {
        match msg.role.as_str() {
            "user" | "tool" | "system" => {
                let text = msg.text_content();
                msg.content = Some(serde_json::Value::String(text));
            }
            "assistant" if matches!(msg.content, Some(serde_json::Value::Null)) => {
                msg.content = None;
            }
            _ => {}
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChatTurnResult {
    pub message: ConversationMessage,
    pub finish_reason: Option<String>,
    /// Provider reasoning channel; not shown to the user or mixed into `message.content`.
    pub reasoning: Option<String>,
}

const MAX_CHAT_REQUEST_RETRIES: u32 = 3;
const CHAT_RETRY_BACKOFF_MS: u64 = 800;

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

        let mut last_err: Option<anyhow::Error> = None;
        let mut parsed: Option<Response> = None;
        for attempt in 1..=MAX_CHAT_REQUEST_RETRIES {
            let mut outbound = messages.to_vec();
            normalize_messages_for_provider(&mut outbound);
            let send_result = self
                .client
                .post(&url)
                .bearer_auth(&self.api_key)
                .json(&Request {
                    model,
                    messages: &outbound,
                    tools,
                    tool_choice: "auto",
                })
                .send()
                .await;

            let resp = match send_result {
                Ok(r) => r,
                Err(e) => {
                    last_err = Some(e.into());
                    if attempt < MAX_CHAT_REQUEST_RETRIES {
                        tokio::time::sleep(Duration::from_millis(
                            CHAT_RETRY_BACKOFF_MS * attempt as u64,
                        ))
                        .await;
                        continue;
                    }
                    return Err(last_err.unwrap());
                }
            };

            let status = resp.status();
            if status.is_success() {
                match resp.json::<Response>().await {
                    Ok(p) => {
                        parsed = Some(p);
                        break;
                    }
                    Err(e) => {
                        last_err = Some(e.into());
                        if attempt < MAX_CHAT_REQUEST_RETRIES {
                            tokio::time::sleep(Duration::from_millis(
                                CHAT_RETRY_BACKOFF_MS * attempt as u64,
                            ))
                            .await;
                            continue;
                        }
                        return Err(last_err.unwrap());
                    }
                }
            } else {
                let body = resp.text().await.unwrap_or_default();
                let err = anyhow::anyhow!("provider error {status}: {body}");
                let validation_422 = status == StatusCode::UNPROCESSABLE_ENTITY
                    && body.contains("Validation of the message failed");
                let retryable = validation_422
                    || status == StatusCode::TOO_MANY_REQUESTS
                    || status == StatusCode::REQUEST_TIMEOUT
                    || status.is_server_error();
                last_err = Some(err);
                if retryable && attempt < MAX_CHAT_REQUEST_RETRIES {
                    tokio::time::sleep(Duration::from_millis(
                        CHAT_RETRY_BACKOFF_MS * attempt as u64,
                    ))
                    .await;
                    continue;
                }
                return Err(last_err.unwrap());
            }
        }
        let parsed = parsed.ok_or_else(|| {
            last_err.unwrap_or_else(|| anyhow::anyhow!("chat request failed after retries"))
        })?;
        let choice = parsed
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("empty choices from provider"))?;

        let message = ConversationMessage {
            role: choice
                .message
                .role
                .unwrap_or_else(|| "assistant".to_string()),
            content: choice.message.content,
            tool_calls: choice.message.tool_calls,
            tool_call_id: None,
            name: None,
        };

        Ok(ChatTurnResult {
            message,
            finish_reason: choice.finish_reason,
            reasoning: choice.message.reasoning,
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

    #[test]
    fn normalize_fills_missing_user_and_tool_content() {
        let mut msgs = vec![
            ConversationMessage {
                role: "user".into(),
                content: None,
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
            ConversationMessage {
                role: "tool".into(),
                content: None,
                tool_calls: None,
                tool_call_id: Some("call_1".into()),
                name: None,
            },
            ConversationMessage {
                role: "assistant".into(),
                content: None,
                tool_calls: Some(vec![ToolCall {
                    id: "call_1".into(),
                    kind: "function".into(),
                    function: FunctionCallPayload {
                        name: "exec".into(),
                        arguments: "{}".into(),
                    },
                }]),
                tool_call_id: None,
                name: None,
            },
        ];
        normalize_messages_for_provider(&mut msgs);
        assert_eq!(msgs[0].content.as_ref().and_then(|v| v.as_str()), Some(""));
        assert_eq!(msgs[1].content.as_ref().and_then(|v| v.as_str()), Some(""));
        assert!(msgs[2].content.is_none());
    }
}
