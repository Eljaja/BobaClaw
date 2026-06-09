use std::path::Path;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::channels::ChannelPeer;

const MAX_TEXT_INJECT_BYTES: usize = 100 * 1024;

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
    /// Synthetic wake after background spawn completion.
    SpawnWake,
}

impl IngressKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::Rest => "rest",
            Self::OpenAiCompat => "openai_compat",
            Self::Cron => "cron",
            Self::Webhook => "webhook",
            Self::Chat => "chat",
            Self::Telegram => "telegram",
            Self::SpawnWake => "spawn_wake",
        }
    }
}

/// Workspace-relative attachment from a channel (Telegram, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceAttachment {
    /// Path under the agent workspace, e.g. `inbox/telegram/42/99/result.json`.
    pub workspace_rel: String,
    pub kind: AttachmentKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentKind {
    Image,
    Document,
    Voice,
    Audio,
    Video,
}

impl AttachmentKind {
    pub fn from_channel_label(label: &str) -> Self {
        match label {
            "image" => Self::Image,
            "voice" => Self::Voice,
            "audio" => Self::Audio,
            "video" => Self::Video,
            _ => Self::Document,
        }
    }

    fn tag_prefix(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Voice => "audio",
            Self::Audio => "audio",
            Self::Video => "video",
            Self::Document => "file",
        }
    }
}

impl WorkspaceAttachment {
    /// PicoClaw-style path tag: `[file:inbox/telegram/…/doc.json]`.
    pub fn path_tag(&self) -> String {
        format!("[{}:{}]", self.kind.tag_prefix(), self.workspace_rel)
    }

    fn host_path(&self, workspace: &Path) -> std::path::PathBuf {
        workspace.join(&self.workspace_rel)
    }

    fn maybe_inject_text(&self, workspace: &Path) -> Option<String> {
        let path = self.host_path(workspace);
        let ext = path.extension().and_then(|e| e.to_str())?;
        if !matches!(ext, "md" | "txt" | "json" | "yaml" | "yml" | "toml" | "csv") {
            return None;
        }
        let bytes = std::fs::read(&path).ok()?;
        if bytes.len() > MAX_TEXT_INJECT_BYTES {
            return None;
        }
        let text = String::from_utf8(bytes).ok()?;
        let name = self
            .original_name
            .as_deref()
            .or(path.file_name().and_then(|n| n.to_str()))
            .unwrap_or("file");
        Some(format!("[Content of {name}]:\n{text}"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedRequest {
    pub request_id: Uuid,
    pub ingress: IngressKind,
    pub agent_group: String,
    pub session_id: Option<String>,
    /// Per-chat routing key (Telegram DM, group, forum thread).
    pub channel_peer: Option<ChannelPeer>,
    /// User-visible text (caption / message), without attachment path tags.
    pub user_text: String,
    /// Files already stored under the agent workspace (`workspace_rel` paths).
    #[serde(default)]
    pub attachments: Vec<WorkspaceAttachment>,
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
            attachments: Vec::new(),
            model_override: None,
        }
    }

    pub fn telegram(
        message: &str,
        agent_group: &str,
        peer: ChannelPeer,
        attachments: Vec<WorkspaceAttachment>,
    ) -> Self {
        Self {
            request_id: Uuid::new_v4(),
            ingress: IngressKind::Telegram,
            agent_group: agent_group.to_string(),
            session_id: None,
            channel_peer: Some(peer),
            user_text: message.to_string(),
            attachments,
            model_override: None,
        }
    }

    /// Text stored in session history and sent to the LLM (path tags + optional small-file injection).
    pub fn format_user_content(&self, workspace: &Path) -> String {
        format_user_content(&self.user_text, &self.attachments, workspace)
    }

    /// Concurrency scope: same key → serialized turns; different keys → may run in parallel.
    pub fn dispatch_scope(&self) -> String {
        if let Some(ref sid) = self.session_id {
            return format!("session:{sid}");
        }
        if let Some(ref peer) = self.channel_peer {
            return format!("peer:{}", peer.route_key());
        }
        match self.ingress {
            IngressKind::Cli => format!("cli:{}", self.agent_group),
            IngressKind::Cron => format!("cron:{}", self.agent_group),
            IngressKind::Rest | IngressKind::OpenAiCompat => format!("api:{}", self.agent_group),
            IngressKind::Chat => format!("chat:{}", self.agent_group),
            IngressKind::Webhook => format!("webhook:{}", self.agent_group),
            IngressKind::Telegram => format!("telegram:{}", self.agent_group),
            IngressKind::SpawnWake => format!("spawn_wake:{}", self.agent_group),
        }
    }

    /// Deliver channel key for spawn job persistence (mirrors scheduled_tasks).
    pub fn spawn_deliver_channel(&self) -> &'static str {
        match self.ingress {
            IngressKind::Telegram => "telegram",
            IngressKind::Cli => "cli",
            IngressKind::Rest | IngressKind::OpenAiCompat => "api",
            IngressKind::Cron => "cron",
            IngressKind::Webhook => "webhook",
            IngressKind::Chat => "chat",
            IngressKind::SpawnWake => "spawn_wake",
        }
    }
}

/// Merge caption/text with attachment path tags (picoClaw / nullclaw convention).
pub fn format_user_content(
    user_text: &str,
    attachments: &[WorkspaceAttachment],
    workspace: &Path,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    let trimmed = user_text.trim();
    if !trimmed.is_empty() {
        parts.push(trimmed.to_string());
    }
    for att in attachments {
        parts.push(att.path_tag());
        if let Some(injected) = att.maybe_inject_text(workspace) {
            parts.push(injected);
        }
    }
    parts.join("\n")
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
        assert!(r.attachments.is_empty());
    }

    #[test]
    fn format_attachment_tags() {
        let att = WorkspaceAttachment {
            workspace_rel: "inbox/telegram/1/2/result.json".into(),
            kind: AttachmentKind::Document,
            original_name: Some("result.json".into()),
        };
        let out = format_user_content("разбери", std::slice::from_ref(&att), Path::new("/ws"));
        assert!(out.contains("разбери"));
        assert!(out.contains("[file:inbox/telegram/1/2/result.json]"));
    }

    #[test]
    fn dispatch_scope_telegram_peer() {
        let peer = crate::channels::ChannelPeer::telegram(42, None);
        let r = NormalizedRequest::telegram("hi", "home", peer, vec![]);
        assert_eq!(r.dispatch_scope(), "peer:telegram:42");
    }

    #[test]
    fn dispatch_scope_cli() {
        let r = NormalizedRequest::cli("hi", "home");
        assert_eq!(r.dispatch_scope(), "cli:home");
    }

    #[test]
    fn format_file_only() {
        let att = WorkspaceAttachment {
            workspace_rel: "inbox/telegram/1/2/a.txt".into(),
            kind: AttachmentKind::Document,
            original_name: None,
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("inbox/telegram/1/2/a.txt");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"hello").unwrap();
        let out = format_user_content("", std::slice::from_ref(&att), dir.path());
        assert_eq!(
            out,
            "[file:inbox/telegram/1/2/a.txt]\n[Content of a.txt]:\nhello"
        );
    }
}
