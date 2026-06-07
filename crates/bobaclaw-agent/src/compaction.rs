use bobaclaw_core::BobaConfig;
use bobaclaw_provider::{ConversationMessage, ToolChatClient};
use bobaclaw_state::SessionStore;
use sqlx::SqlitePool;

use crate::context::{estimate_tokens, transcript_lines};
use crate::progress::{emit, AgentEvent, AgentProgress};
use crate::prompt::{
    strip_summary_prefix, summarizer_user_message, SUMMARIZER_SYSTEM, SUMMARY_PREFIX,
};

fn last_compaction_index(rows: &[(String, String)]) -> Option<usize> {
    rows.iter()
        .enumerate()
        .rev()
        .find_map(|(i, (role, _))| (role == "compaction").then_some(i))
}

fn previous_summary_body(rows: &[(String, String)]) -> Option<String> {
    let i = last_compaction_index(rows)?;
    let body = strip_summary_prefix(&rows[i].1);
    if body.is_empty() {
        None
    } else {
        Some(body)
    }
}

fn summarize_slice<'a>(
    rows: &'a [(String, String)],
    tail_keep: usize,
) -> Option<&'a [(String, String)]> {
    let start = last_compaction_index(rows).map(|i| i + 1).unwrap_or(0);
    let end = rows.len().saturating_sub(tail_keep.max(1));
    if end <= start || end - start < 2 {
        return None;
    }
    Some(&rows[start..end])
}

pub async fn maybe_compact_session(
    pool: &SqlitePool,
    config: &BobaConfig,
    session_id: &str,
    progress: Option<&dyn AgentProgress>,
) -> anyhow::Result<bool> {
    if !config.context.compression_enabled {
        return Ok(false);
    }
    let sessions = SessionStore::new(pool);
    let all = sessions.list_messages(session_id).await?;
    if all.len() < 6 {
        return Ok(false);
    }

    let effective = effective_history(&all);
    let tokens = estimate_tokens(&history_to_conversation(&effective));
    if tokens <= config.context.compact_threshold_tokens() {
        return Ok(false);
    }

    let tail_keep = config.context.keep_recent_messages.min(all.len());
    let Some(to_summarize) = summarize_slice(&all, tail_keep) else {
        return Ok(false);
    };

    emit(progress, AgentEvent::Compacting { tokens });
    let prev = previous_summary_body(&all);
    let summary = summarize_turns(config, to_summarize, prev.as_deref()).await?;
    let full = format!("{SUMMARY_PREFIX}{summary}");
    sessions
        .append_message(session_id, "compaction", &full)
        .await?;
    Ok(true)
}

pub async fn force_compact_session(
    pool: &SqlitePool,
    config: &BobaConfig,
    session_id: &str,
    progress: Option<&dyn AgentProgress>,
) -> anyhow::Result<String> {
    let sessions = SessionStore::new(pool);
    let all = sessions.list_messages(session_id).await?;
    if all.len() < 2 {
        anyhow::bail!("too few messages for compaction");
    }
    let tail_keep = config.context.keep_recent_messages.min(all.len());
    let to_summarize = summarize_slice(&all, tail_keep.max(2))
        .ok_or_else(|| anyhow::anyhow!("nothing to compact (history too short)"))?;
    emit(progress, AgentEvent::Compacting { tokens: 0 });
    let prev = previous_summary_body(&all);
    let summary = summarize_turns(config, to_summarize, prev.as_deref()).await?;
    let full = format!("{SUMMARY_PREFIX}{summary}");
    sessions
        .append_message(session_id, "compaction", &full)
        .await?;
    Ok(full)
}

async fn summarize_turns(
    config: &BobaConfig,
    turns: &[(String, String)],
    previous_summary: Option<&str>,
) -> anyhow::Result<String> {
    let transcript = transcript_lines(turns);
    let api_key = config.resolve_api_key()?;
    let client = ToolChatClient::from_provider(&config.provider, api_key)?;
    let messages = vec![
        ConversationMessage::system(SUMMARIZER_SYSTEM),
        ConversationMessage::user(summarizer_user_message(&transcript, previous_summary)),
    ];
    client.complete_text(&messages, None).await
}

pub fn history_to_conversation(rows: &[(String, String)]) -> Vec<ConversationMessage> {
    rows.iter()
        .map(|(role, content)| {
            let api_role = match role.as_str() {
                "compaction" => "user",
                "tool" => "tool",
                other => other,
            };
            ConversationMessage {
                role: api_role.to_string(),
                content: Some(serde_json::Value::String(content.clone())),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            }
        })
        .collect()
}

pub fn effective_history(rows: &[(String, String)]) -> Vec<(String, String)> {
    let mut last_compaction = None;
    for (i, (role, _)) in rows.iter().enumerate() {
        if role == "compaction" {
            last_compaction = Some(i);
        }
    }
    match last_compaction {
        Some(i) => rows[i..].to_vec(),
        None => rows.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(r, c)| (r.to_string(), c.to_string()))
            .collect()
    }

    #[test]
    fn effective_history_from_last_compaction() {
        let all = rows(&[("user", "old"), ("compaction", "sum1"), ("user", "new")]);
        let eff = effective_history(&all);
        assert_eq!(eff.len(), 2);
        assert_eq!(eff[0].0, "compaction");
        assert_eq!(eff[1].1, "new");
    }

    #[test]
    fn history_maps_compaction_to_user() {
        let h = rows(&[("compaction", "x")]);
        let conv = history_to_conversation(&h);
        assert_eq!(conv[0].role, "user");
    }

    #[test]
    fn strip_summary_via_prompt() {
        use crate::prompt::{strip_summary_prefix, SUMMARY_PREFIX};
        let inner = "task";
        assert_eq!(
            strip_summary_prefix(&format!("{SUMMARY_PREFIX}{inner}")),
            inner
        );
    }
}
