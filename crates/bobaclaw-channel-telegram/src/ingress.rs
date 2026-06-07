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
    #[allow(dead_code)]
    pub reply_to_message_id: Option<i64>,
    pub is_bot_mentioned: bool,
    pub is_reply_to_bot: bool,
}

pub fn message_has_attachments(msg: &Message) -> bool {
    msg.document.is_some()
        || msg.photo.as_ref().is_some_and(|p| !p.is_empty())
        || msg.voice.is_some()
        || msg.audio.is_some()
        || msg.video.is_some()
}

pub fn parse_message(
    msg: &Message,
    bot_id: i64,
    bot_username: Option<&str>,
) -> Option<InboundMessage> {
    let text = msg
        .text
        .as_deref()
        .or(msg.caption.as_deref())
        .map(str::trim)
        .unwrap_or("")
        .to_string();

    if text.is_empty() && !message_has_attachments(msg) {
        return None;
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
        user_name: user.username.clone().or_else(|| msg.chat.title.clone()),
        text,
        message_id: msg.message_id,
        reply_to_message_id: msg.reply_to_message.as_ref().map(|m| m.message_id),
        is_bot_mentioned,
        is_reply_to_bot,
    })
}

fn message_mentions_bot(msg: &Message, bot_id: i64, bot_username: Option<&str>) -> bool {
    let Some(entities) = &msg.entities else {
        return false;
    };
    let text = msg.text.as_deref().or(msg.caption.as_deref()).unwrap_or("");
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

#[allow(dead_code)]
pub fn display_user(user: &User) -> String {
    user.username
        .as_ref()
        .map(|u| format!("@{u}"))
        .unwrap_or_else(|| user.id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{Chat, Document, Message, PhotoSize, User};

    fn test_msg(text: Option<&str>, caption: Option<&str>, doc: bool) -> Message {
        Message {
            message_id: 1,
            chat: Chat {
                id: 42,
                chat_type: "private".into(),
                title: None,
            },
            from: Some(User {
                id: 99,
                username: Some("alice".into()),
                is_bot: false,
            }),
            text: text.map(str::to_string),
            caption: caption.map(str::to_string),
            document: doc.then(|| Document {
                file_id: "fid".into(),
                file_name: Some("notes.txt".into()),
                mime_type: Some("text/plain".into()),
            }),
            photo: None,
            voice: None,
            audio: None,
            video: None,
            entities: None,
            reply_to_message: None,
            message_thread_id: None,
        }
    }

    #[test]
    fn parse_document_without_caption() {
        let msg = test_msg(None, None, true);
        let inbound = parse_message(&msg, 1, None).expect("document only");
        assert!(inbound.text.is_empty());
    }

    #[test]
    fn parse_photo_with_caption() {
        let mut msg = test_msg(None, Some("look"), false);
        msg.photo = Some(vec![PhotoSize {
            file_id: "p1".into(),
            width: 100,
            height: 100,
        }]);
        let inbound = parse_message(&msg, 1, None).unwrap();
        assert_eq!(inbound.text, "look");
    }

    #[test]
    fn skip_empty_message() {
        let msg = test_msg(None, None, false);
        assert!(parse_message(&msg, 1, None).is_none());
    }
}
