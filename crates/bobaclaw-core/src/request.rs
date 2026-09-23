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
    /// Local browser chat UI (`channels.web`).
    Web,
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
            Self::Web => "web",
            Self::SpawnWake => "spawn_wake",
        }
    }

    /// Whether a new request of this kind cancels the in-flight turn on its session.
    ///
    /// Interactive user messages (CLI, chat UI, Telegram, Web UI) preempt: newest message wins.
    /// Background / programmatic ingress (spawn wake, cron, webhook, REST, OpenAI-compat)
    /// never cancels another turn; it queues behind the running one instead.
    pub fn preempts_in_flight(self) -> bool {
        matches!(self, Self::Cli | Self::Chat | Self::Telegram | Self::Web)
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

    /// Dispatcher concurrency scope for a resolved session: all turns that write to the
    /// same session history share this key and are serialized.
    pub fn session_scope(session_id: &str) -> String {
        format!("session:{session_id}")
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
            IngressKind::Web => "web",
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
    fn web_ingress_labels() {
        assert_eq!(IngressKind::Web.as_str(), "web");
        let mut r = NormalizedRequest::cli("hi", "home");
        r.ingress = IngressKind::Web;
        assert_eq!(r.spawn_deliver_channel(), "web");
    }

    #[test]
    fn session_scope_key() {
        assert_eq!(NormalizedRequest::session_scope("sess_1"), "session:sess_1");
    }

    #[test]
    fn preemption_policy_by_ingress() {
        assert!(IngressKind::Telegram.preempts_in_flight());
        assert!(IngressKind::Cli.preempts_in_flight());
        assert!(IngressKind::Chat.preempts_in_flight());
        assert!(IngressKind::Web.preempts_in_flight());
        assert!(!IngressKind::SpawnWake.preempts_in_flight());
        assert!(!IngressKind::Cron.preempts_in_flight());
        assert!(!IngressKind::Webhook.preempts_in_flight());
        assert!(!IngressKind::Rest.preempts_in_flight());
        assert!(!IngressKind::OpenAiCompat.preempts_in_flight());
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
