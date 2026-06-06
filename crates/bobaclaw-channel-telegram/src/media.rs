use std::path::{Path, PathBuf};

use bobaclaw_core::{AttachmentKind, BobaPaths, WorkspaceAttachment};
use tracing::warn;

use crate::api::{Message, TelegramApi};

#[derive(Debug, Clone)]
pub struct DownloadedFile {
    /// Path inside the bubblewrap workspace (`/workspace/...`).
    pub workspace_rel: String,
    pub host_path: PathBuf,
    pub kind: &'static str,
    pub original_name: Option<String>,
}

fn telegram_inbox_dir(
    paths: &BobaPaths,
    agent_group: &str,
    chat_id: i64,
    message_id: i64,
) -> PathBuf {
    paths
        .group_workspace(agent_group)
        .join("inbox")
        .join("telegram")
        .join(chat_id.to_string())
        .join(message_id.to_string())
}

/// Download Telegram attachments into the agent workspace so `exec` can read them at `/workspace/inbox/telegram/...`.
pub async fn download_message_media(
    api: &TelegramApi,
    paths: &BobaPaths,
    agent_group: &str,
    msg: &Message,
) -> Vec<DownloadedFile> {
    let chat_id = msg.chat.id;
    let message_id = msg.message_id;
    let dir = telegram_inbox_dir(paths, agent_group, chat_id, message_id);

    if let Err(e) = std::fs::create_dir_all(&dir) {
        warn!("telegram media: cannot create dir {}: {e}", dir.display());
        return Vec::new();
    }

    let mut out = Vec::new();

    if let Some(photos) = &msg.photo {
        if let Some(photo) = photos.last() {
            let name = "photo.jpg";
            if let Some(path) = api
                .download_to_path(&photo.file_id, &dir.join(name), ".jpg")
                .await
            {
                push_downloaded(&mut out, &dir, path, "image", Some(name.into()));
            }
        }
    }

    if let Some(doc) = &msg.document {
        let filename = sanitize_filename(
            doc.file_name
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or("document"),
        );
        let dest = unique_path(&dir, &filename);
        if let Some(path) = api.download_to_path(&doc.file_id, &dest, "").await {
            push_downloaded(&mut out, &dir, path, "document", doc.file_name.clone());
        }
    }

    if let Some(voice) = &msg.voice {
        let dest = dir.join("voice.ogg");
        if let Some(path) = api.download_to_path(&voice.file_id, &dest, ".ogg").await {
            push_downloaded(
                &mut out,
                &dir,
                path,
                "voice",
                Some("voice.ogg".into()),
            );
        }
    }

    if let Some(audio) = &msg.audio {
        let filename = sanitize_filename(
            audio
                .file_name
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or("audio.mp3"),
        );
        let dest = unique_path(&dir, &filename);
        if let Some(path) = api.download_to_path(&audio.file_id, &dest, ".mp3").await {
            push_downloaded(&mut out, &dir, path, "audio", audio.file_name.clone());
        }
    }

    if let Some(video) = &msg.video {
        let filename = sanitize_filename(
            video
                .file_name
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or("video.mp4"),
        );
        let dest = unique_path(&dir, &filename);
        if let Some(path) = api.download_to_path(&video.file_id, &dest, ".mp4").await {
            push_downloaded(&mut out, &dir, path, "video", video.file_name.clone());
        }
    }

    out
}

impl From<DownloadedFile> for WorkspaceAttachment {
    fn from(f: DownloadedFile) -> Self {
        WorkspaceAttachment {
            workspace_rel: f.workspace_rel,
            kind: AttachmentKind::from_channel_label(f.kind),
            original_name: f.original_name,
        }
    }
}

fn push_downloaded(
    out: &mut Vec<DownloadedFile>,
    inbox_dir: &Path,
    host_path: PathBuf,
    kind: &'static str,
    original_name: Option<String>,
) {
    let chat = inbox_dir
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("0");
    let message = inbox_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("0");
    let filename = host_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");
    let workspace_rel = format!("inbox/telegram/{chat}/{message}/{filename}");

    out.push(DownloadedFile {
        workspace_rel,
        host_path,
        kind,
        original_name,
    });
}

fn sanitize_filename(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ' ') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() {
        s = "file".into();
    }
    if s.len() > 200 {
        s.truncate(200);
    }
    s
}

fn unique_path(dir: &Path, filename: &str) -> PathBuf {
    let base = dir.join(filename);
    if !base.exists() {
        return base;
    }
    let stem = Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("file");
    let ext = Path::new(filename).extension().and_then(|e| e.to_str());
    for i in 2..100 {
        let name = match ext {
            Some(e) => format!("{stem}-{i}.{e}"),
            None => format!("{stem}-{i}"),
        };
        let p = dir.join(&name);
        if !p.exists() {
            return p;
        }
    }
    base
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_rel_matches_inbox_layout() {
        let dir = PathBuf::from("/ws/home/inbox/telegram/42/99");
        let host = dir.join("result.json");
        let mut out = Vec::new();
        push_downloaded(
            &mut out,
            &dir,
            host,
            "document",
            Some("result.json".into()),
        );
        assert_eq!(out[0].workspace_rel, "inbox/telegram/42/99/result.json");
    }
}
