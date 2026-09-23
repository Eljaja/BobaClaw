use bobaclaw_core::BobaConfig;
use bobaclaw_provider::{ConversationMessage, ToolChatClient};
use bobaclaw_state::{SessionStore, StoredMessage};
use sqlx::SqlitePool;

use crate::context::{estimate_tokens, transcript_lines};
use crate::progress::{emit, AgentEvent, AgentProgress};
use crate::prompt::{
    strip_summary_prefix, summarizer_user_message, SUMMARIZER_SYSTEM, SUMMARY_PREFIX,
};

/// Rebuild `messages` after compaction: keep system prompt + in-turn tail, refresh DB history.
///
/// `history_boundary` is the index in `messages` where the in-turn tail starts; it is updated
/// to the rebuilt layout so later rebuilds in the same turn slice correctly.
pub async fn maybe_ensure_context_budget(
    pool: &SqlitePool,
    config: &BobaConfig,
    session_id: &str,
    messages: &mut Vec<ConversationMessage>,
    history_boundary: &mut usize,
    progress: Option<&dyn AgentProgress>,
) -> anyhow::Result<()> {
    if !config.context.compression_enabled {
        return Ok(());
    }
    let tokens = estimate_tokens(messages);
    if tokens < config.context.guard_threshold_tokens() {
        return Ok(());
    }
    if !maybe_compact_session(pool, config, session_id, progress).await? {
        return Ok(());
    }
    let sessions = SessionStore::new(pool);
    let all = sessions.list_stored_messages(session_id).await?;
    let effective = effective_history(&all);
    let fresh_history = history_to_conversation(&effective);
    let system = messages
        .first()
        .cloned()
        .unwrap_or_else(|| ConversationMessage::system(String::new()));
    let in_turn = messages.get(*history_boundary..).unwrap_or(&[]).to_vec();
    messages.clear();
    messages.push(system);
    messages.extend(fresh_history);
    *history_boundary = messages.len();
    messages.extend(in_turn);
    Ok(())
}

fn last_compaction_index(rows: &[StoredMessage]) -> Option<usize> {
    rows.iter().rposition(|r| r.role == "compaction")
}

fn previous_summary_body(rows: &[StoredMessage]) -> Option<String> {
    let i = last_compaction_index(rows)?;
    let body = strip_summary_prefix(&rows[i].content);
    if body.is_empty() {
        None
    } else {
        Some(body)
    }
}

/// Non-compaction rows not yet covered by the latest summary, in id order.
///
/// With a boundary (`covers_through_id`) this is every message after it — including the
/// kept tail that sits before the compaction row. Legacy compaction rows without a boundary
/// fall back to "everything after the compaction row".
fn uncovered_rows(rows: &[StoredMessage]) -> Vec<&StoredMessage> {
    let not_compaction = |r: &&StoredMessage| r.role != "compaction";
    match last_compaction_index(rows) {
        Some(i) => match rows[i].covers_through_id {
            Some(boundary) => rows
                .iter()
                .filter(not_compaction)
                .filter(|r| r.id > boundary)
                .collect(),
            None => rows[i + 1..].iter().filter(not_compaction).collect(),
        },
        None => rows.iter().filter(not_compaction).collect(),
    }
}

fn to_pairs<'a>(rows: impl IntoIterator<Item = &'a StoredMessage>) -> Vec<(String, String)> {
    rows.into_iter()
        .map(|r| (r.role.clone(), r.content.clone()))
        .collect()
}

/// What the next compaction summarizes and where its boundary lands.
#[derive(Debug)]
struct CompactionPlan {
    to_summarize: Vec<(String, String)>,
    covers_through_id: i64,
    previous_summary: Option<String>,
}

/// Summarize uncovered rows except the last `tail_keep` (at least 1, so the current user
/// message always stays verbatim). `None` when fewer than 2 rows would be summarized.
fn plan_compaction(rows: &[StoredMessage], tail_keep: usize) -> Option<CompactionPlan> {
    let pending = uncovered_rows(rows);
    let end = pending.len().saturating_sub(tail_keep.max(1));
    if end < 2 {
        return None;
    }
    let slice = &pending[..end];
    Some(CompactionPlan {
        covers_through_id: slice.last()?.id,
        to_summarize: to_pairs(slice.iter().copied()),
        previous_summary: previous_summary_body(rows),
    })
}

async fn run_compaction(
    sessions: &SessionStore<'_>,
    config: &BobaConfig,
    session_id: &str,
    plan: CompactionPlan,
) -> anyhow::Result<String> {
    let summary =
        summarize_turns(config, &plan.to_summarize, plan.previous_summary.as_deref()).await?;
    let full = format!("{SUMMARY_PREFIX}{summary}");
    sessions
        .append_compaction(session_id, &full, plan.covers_through_id)
        .await?;
    Ok(full)
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
    let all = sessions.list_stored_messages(session_id).await?;
    if all.len() < 6 {
        return Ok(false);
    }

    let effective = effective_history(&all);
    let tokens = estimate_tokens(&history_to_conversation(&effective));
    if tokens <= config.context.compact_threshold_tokens() {
        return Ok(false);
    }

    let tail_keep = config.context.keep_recent_messages.min(all.len());
    let Some(plan) = plan_compaction(&all, tail_keep) else {
        return Ok(false);
    };

    emit(progress, AgentEvent::Compacting { tokens });
    run_compaction(&sessions, config, session_id, plan).await?;
    Ok(true)
}

pub async fn force_compact_session(
    pool: &SqlitePool,
    config: &BobaConfig,
    session_id: &str,
    progress: Option<&dyn AgentProgress>,
) -> anyhow::Result<String> {
    let sessions = SessionStore::new(pool);
    let all = sessions.list_stored_messages(session_id).await?;
    if all.len() < 2 {
        anyhow::bail!("too few messages for compaction");
    }
    let tail_keep = config.context.keep_recent_messages.min(all.len());
    let plan = plan_compaction(&all, tail_keep.max(2))
        .ok_or_else(|| anyhow::anyhow!("nothing to compact (history too short)"))?;
    emit(progress, AgentEvent::Compacting { tokens: 0 });
    run_compaction(&sessions, config, session_id, plan).await
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

/// Model-visible history: latest compaction summary (if any) followed by every message it
/// does not cover, in id order.
pub fn effective_history(rows: &[StoredMessage]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Some(i) = last_compaction_index(rows) {
        out.push((rows[i].role.clone(), rows[i].content.clone()));
    }
    out.extend(to_pairs(uncovered_rows(rows)));
    out
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

    fn stored(pairs: &[(&str, &str)]) -> Vec<StoredMessage> {
        pairs
            .iter()
            .enumerate()
            .map(|(i, (r, c))| StoredMessage {
                id: i as i64 + 1,
                role: r.to_string(),
                content: c.to_string(),
                covers_through_id: None,
            })
            .collect()
    }

    fn contents(h: &[(String, String)]) -> Vec<&str> {
        h.iter().map(|(_, c)| c.as_str()).collect()
    }

    #[test]
    fn effective_history_legacy_compaction_without_boundary() {
        let all = stored(&[("user", "old"), ("compaction", "sum1"), ("user", "new")]);
        let eff = effective_history(&all);
        assert_eq!(eff.len(), 2);
        assert_eq!(eff[0].0, "compaction");
        assert_eq!(eff[1].1, "new");
    }

    #[test]
    fn effective_history_keeps_tail_before_compaction_row() {
        let mut all = stored(&[
            ("user", "u1"),
            ("assistant", "a1"),
            ("user", "u2"),
            ("assistant", "a2"),
            ("user", "u3"),
            ("compaction", "sum"),
            ("assistant", "a3"),
        ]);
        all[5].covers_through_id = Some(2);
        let eff = effective_history(&all);
        assert_eq!(contents(&eff), ["sum", "u2", "a2", "u3", "a3"]);
        assert_eq!(eff[0].0, "compaction");
    }

    #[test]
    fn plan_keeps_tail_and_latest_user_message() {
        let all = stored(&[
            ("user", "u1"),
            ("assistant", "a1"),
            ("user", "u2"),
            ("assistant", "a2"),
            ("user", "u3"),
        ]);
        let plan = plan_compaction(&all, 2).unwrap();
        assert_eq!(contents(&plan.to_summarize), ["u1", "a1", "u2"]);
        assert_eq!(plan.covers_through_id, 3);
        assert!(plan.previous_summary.is_none());
        // Tail is never empty: the current user message stays verbatim even with tail_keep=0.
        let plan = plan_compaction(&all, 0).unwrap();
        assert_eq!(plan.covers_through_id, 4);
        assert!(plan_compaction(&all, 4).is_none());
    }

    #[test]
    fn plan_after_legacy_compaction_starts_after_row() {
        let all = stored(&[
            ("user", "old"),
            ("compaction", "sum1"),
            ("user", "n1"),
            ("assistant", "n2"),
            ("user", "n3"),
        ]);
        let plan = plan_compaction(&all, 1).unwrap();
        assert_eq!(contents(&plan.to_summarize), ["n1", "n2"]);
        assert_eq!(plan.previous_summary.as_deref(), Some("sum1"));
    }

    /// Simulates what `run_compaction` persists, without calling the summarizer LLM.
    async fn apply_plan(store: &SessionStore<'_>, sid: &str, plan: &CompactionPlan, body: &str) {
        store
            .append_compaction(
                sid,
                &format!("{SUMMARY_PREFIX}{body}"),
                plan.covers_through_id,
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn compaction_keeps_recent_messages_across_two_rounds() {
        let dir = tempfile::tempdir().unwrap();
        let db = bobaclaw_state::StateDb::open(&dir.path().join("state.db"))
            .await
            .unwrap();
        let store = SessionStore::new(db.pool());
        let sid = store.get_or_create_cli("test").await.unwrap();
        for (role, c) in [
            ("user", "u1"),
            ("assistant", "a1"),
            ("user", "u2"),
            ("assistant", "a2"),
            ("user", "u3"),
            ("assistant", "a3"),
            ("user", "u4-current"),
        ] {
            store.append_message(&sid, role, c).await.unwrap();
        }

        // Round 1: compaction runs after the current user message was appended (loop_.rs order).
        let all = store.list_stored_messages(&sid).await.unwrap();
        let plan = plan_compaction(&all, 3).unwrap();
        assert_eq!(contents(&plan.to_summarize), ["u1", "a1", "u2", "a2"]);
        apply_plan(&store, &sid, &plan, "S1").await;

        let all = store.list_stored_messages(&sid).await.unwrap();
        let eff = effective_history(&all);
        assert_eq!(eff[0].0, "compaction");
        assert!(eff[0].1.ends_with("S1"));
        assert_eq!(&contents(&eff)[1..], ["u3", "a3", "u4-current"]);
        let conv = history_to_conversation(&eff);
        assert_eq!(conv.last().unwrap().role, "user");
        assert_eq!(conv.last().unwrap().text_content(), "u4-current");

        // More conversation, then round 2: the tail kept by round 1 must be summarized now.
        for (role, c) in [("assistant", "a4"), ("user", "u5"), ("assistant", "a5")] {
            store.append_message(&sid, role, c).await.unwrap();
        }
        store
            .append_message(&sid, "user", "u6-current")
            .await
            .unwrap();
        let all = store.list_stored_messages(&sid).await.unwrap();
        let plan = plan_compaction(&all, 2).unwrap();
        assert_eq!(
            contents(&plan.to_summarize),
            ["u3", "a3", "u4-current", "a4", "u5"]
        );
        assert_eq!(plan.previous_summary.as_deref(), Some("S1"));
        apply_plan(&store, &sid, &plan, "S2").await;

        let all = store.list_stored_messages(&sid).await.unwrap();
        let eff = effective_history(&all);
        assert!(eff[0].1.ends_with("S2"));
        assert_eq!(&contents(&eff)[1..], ["a5", "u6-current"]);
        assert_eq!(eff.iter().filter(|(r, _)| r == "compaction").count(), 1);
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
