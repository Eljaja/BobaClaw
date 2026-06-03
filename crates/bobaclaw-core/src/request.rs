use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IngressKind {
    Cli,
    Rest,
    OpenAiCompat,
    Cron,
    Webhook,
    Chat,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedRequest {
    pub request_id: Uuid,
    pub ingress: IngressKind,
    pub agent_group: String,
    pub session_id: Option<String>,
    pub user_text: String,
    pub model_override: Option<String>,
}

impl NormalizedRequest {
    pub fn cli(message: &str, agent_group: &str) -> Self {
        Self {
            request_id: Uuid::new_v4(),
            ingress: IngressKind::Cli,
            agent_group: agent_group.to_string(),
            session_id: None,
            user_text: message.to_string(),
            model_override: None,
        }
    }
}
