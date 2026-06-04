use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::channels::ChannelPeer;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IngressKind {
    Cli,
    Rest,
    OpenAiCompat,
    Cron,
    Webhook,
    Chat,
    Telegram,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedRequest {
    pub request_id: Uuid,
    pub ingress: IngressKind,
    pub agent_group: String,
    pub session_id: Option<String>,
    /// Per-chat routing key (Telegram DM, group, forum thread).
    pub channel_peer: Option<ChannelPeer>,
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
            channel_peer: None,
            user_text: message.to_string(),
            model_override: None,
        }
    }

    pub fn telegram(
        message: &str,
        agent_group: &str,
        peer: ChannelPeer,
    ) -> Self {
        Self {
            request_id: Uuid::new_v4(),
            ingress: IngressKind::Telegram,
            agent_group: agent_group.to_string(),
            session_id: None,
            channel_peer: Some(peer),
            user_text: message.to_string(),
            model_override: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_request_fields() {
        let r = NormalizedRequest::cli("hi", "home");
        assert_eq!(r.ingress, IngressKind::Cli);
        assert_eq!(r.agent_group, "home");
        assert_eq!(r.user_text, "hi");
    }
}
