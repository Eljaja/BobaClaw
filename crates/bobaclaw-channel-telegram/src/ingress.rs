use bobaclaw_core::policy::ChatKind;
use bobaclaw_core::ChannelPeer;

use crate::api::{Message, User};

#[derive(Debug, Clone)]
pub struct InboundMessage {
    pub peer: ChannelPeer,
    pub chat_kind: ChatKind,
    pub user_id: i64,
    pub user_name: Option<String>,
    pub text: String,
    pub message_id: i64,
    pub reply_to_message_id: Option<i64>,
    pub is_bot_mentioned: bool,
    pub is_reply_to_bot: bool,
}

pub fn parse_message(msg: &Message, bot_id: i64, bot_username: Option<&str>) -> Option<InboundMessage> {
    let text = msg.text.as_deref()?.trim();
    if text.is_empty() {
        return None;
    }
    if text.starts_with('/') {
        let cmd = text.split_whitespace().next().unwrap_or("");
        if matches!(cmd, "/start" | "/help" | "/pair") {
            // handled as commands in runtime
        }
    }

    let user = msg.from.as_ref()?;
    if user.is_bot {
        return None;
    }

    let chat_kind = match msg.chat.chat_type.as_str() {
        "private" => ChatKind::Private,
        "group" => ChatKind::Group,
        "supergroup" => ChatKind::Supergroup,
        "channel" => ChatKind::Channel,
        _ => ChatKind::Private,
    };

    let thread_id = msg.message_thread_id;
    let peer = ChannelPeer::telegram(msg.chat.id, thread_id);

    let is_bot_mentioned = message_mentions_bot(msg, bot_id, bot_username);
    let is_reply_to_bot = msg
        .reply_to_message
        .as_ref()
        .and_then(|r| r.from.as_ref())
        .map(|u| u.id == bot_id)
        .unwrap_or(false);

    Some(InboundMessage {
        peer,
        chat_kind,
        user_id: user.id,
        user_name: user.username.clone().or_else(|| {
            msg.chat.title.clone()
        }),
        text: text.to_string(),
        message_id: msg.message_id,
        reply_to_message_id: msg
            .reply_to_message
            .as_ref()
            .map(|m| m.message_id),
        is_bot_mentioned,
        is_reply_to_bot,
    })
}

fn message_mentions_bot(msg: &Message, bot_id: i64, bot_username: Option<&str>) -> bool {
    let Some(entities) = &msg.entities else {
        return false;
    };
    let text = msg.text.as_deref().unwrap_or("");
    for ent in entities {
        if ent.entity_type != "mention" {
            continue;
        }
        let start = ent.offset as usize;
        let end = start + ent.length as usize;
        if let Some(slice) = text.get(start..end) {
            if let Some(u) = bot_username {
                if slice.eq_ignore_ascii_case(&format!("@{u}")) {
                    return true;
                }
            }
        }
        if ent.user.as_ref().map(|u| u.id) == Some(bot_id) {
            return true;
        }
    }
    false
}

pub fn display_user(user: &User) -> String {
    user.username
        .as_ref()
        .map(|u| format!("@{u}"))
        .unwrap_or_else(|| user.id.to_string())
}
