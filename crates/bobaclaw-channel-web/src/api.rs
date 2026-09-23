//! HTTP handlers for `/ui` and `/api/web/*`.

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::sse::{KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::Json;
use bobaclaw_agent::sanitize_user_reply;
use bobaclaw_core::{IngressKind, NormalizedRequest};
use bobaclaw_state::{HistoryMessage, SessionStore, SessionSummary};
use serde::{Deserialize, Serialize};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_stream::StreamExt;

use crate::events::{ChannelProgress, WebEvent};
use crate::state::WebState;

const INDEX_HTML: &str = include_str!("../assets/index.html");
const MAX_MESSAGE_BYTES: usize = 32 * 1024;
const SESSION_LIST_LIMIT: i64 = 200;

/// Content-Security-Policy for `/ui`: only the nonce'd inline script/style may run, and
/// the page may only talk to its own origin.
pub(crate) fn csp_for_nonce(nonce: &str) -> String {
    format!(
        "default-src 'none'; script-src 'nonce-{nonce}'; style-src 'nonce-{nonce}'; \
         connect-src 'self'; img-src 'self' data:; base-uri 'none'; form-action 'none'; \
         frame-ancestors 'none'"
    )
}

pub(crate) fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

pub(crate) async fn ui_index(State(state): State<WebState>) -> Response {
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let body = INDEX_HTML
        .replace("__BOBACLAW_TITLE__", &html_escape(&state.title))
        .replace("__CSP_NONCE__", &nonce);
    let mut resp = (StatusCode::OK, body).into_response();
    let h = resp.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    if let Ok(csp) = HeaderValue::from_str(&csp_for_nonce(&nonce)) {
        h.insert(header::CONTENT_SECURITY_POLICY, csp);
    }
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp
}

pub(crate) struct ApiError(StatusCode, String);

impl ApiError {
    fn not_found() -> Self {
        Self(StatusCode::NOT_FOUND, "session not found".into())
    }

    fn internal(e: anyhow::Error) -> Self {
        tracing::warn!("web api: {e:#}");
        Self(StatusCode::INTERNAL_SERVER_ERROR, "internal error".into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({ "error": self.1 }))).into_response()
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct ApiConfig {
    title: String,
    agent_group: String,
}

pub(crate) async fn api_config(State(state): State<WebState>) -> Json<ApiConfig> {
    Json(ApiConfig {
        title: state.title.clone(),
        agent_group: state.agent_group.clone(),
    })
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct ApiSession {
    pub id: String,
    pub title: String,
    pub started_at: f64,
    pub updated_at: f64,
    pub message_count: i64,
}

impl From<SessionSummary> for ApiSession {
    fn from(s: SessionSummary) -> Self {
        Self {
            id: s.id,
            title: s.title,
            started_at: s.started_at,
            updated_at: s.updated_at,
            message_count: s.message_count,
        }
    }
}

pub(crate) async fn list_sessions(
    State(state): State<WebState>,
) -> Result<Json<Vec<ApiSession>>, ApiError> {
    let rows = SessionStore::new(&state.pool)
        .list_sessions_for_ingress(&state.agent_group, IngressKind::Web, SESSION_LIST_LIMIT)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(rows.into_iter().map(ApiSession::from).collect()))
}

pub(crate) async fn create_session(
    State(state): State<WebState>,
) -> Result<(StatusCode, Json<ApiSession>), ApiError> {
    let store = SessionStore::new(&state.pool);
    let id = store
        .create_session(&state.agent_group, IngressKind::Web)
        .await
        .map_err(ApiError::internal)?;
    let info = store
        .get_session(&id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    Ok((
        StatusCode::CREATED,
        Json(ApiSession {
            id,
            title: "New chat".into(),
            started_at: info.started_at,
            updated_at: info.started_at,
            message_count: 0,
        }),
    ))
}

/// Only web-ingress sessions of this agent group are reachable through the web API.
async fn ensure_web_session(state: &WebState, session_id: &str) -> Result<(), ApiError> {
    let info = SessionStore::new(&state.pool)
        .get_session(session_id)
        .await
        .map_err(ApiError::internal)?;
    match info {
        Some(i) if i.source == IngressKind::Web.as_str() && i.agent_group == state.agent_group => {
            Ok(())
        }
        _ => Err(ApiError::not_found()),
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct ApiMessage {
    pub id: i64,
    /// `user`, `assistant`, or `summary` (a compaction marker).
    pub role: String,
    pub text: String,
    pub timestamp: f64,
}

/// Map stored rows to display messages: strip the tool-results appendix from assistant
/// rows, turn `compaction` rows into `summary` markers, drop everything else.
pub(crate) fn history_for_display(rows: Vec<HistoryMessage>) -> Vec<ApiMessage> {
    rows.into_iter()
        .filter_map(|m| {
            let (role, text) = match m.role.as_str() {
                "user" => ("user", m.content),
                "assistant" => ("assistant", sanitize_user_reply(&m.content)),
                "compaction" => ("summary", m.content),
                _ => return None,
            };
            if text.trim().is_empty() && role != "summary" {
                return None;
            }
            Some(ApiMessage {
                id: m.id,
                role: role.into(),
                text,
                timestamp: m.timestamp,
            })
        })
        .collect()
}

pub(crate) async fn list_messages(
    State(state): State<WebState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<ApiMessage>>, ApiError> {
    ensure_web_session(&state, &id).await?;
    let rows = SessionStore::new(&state.pool)
        .list_history(&id)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(history_for_display(rows)))
}

#[derive(Debug, Deserialize)]
pub(crate) struct SendMessageBody {
    text: String,
}

pub(crate) async fn send_message(
    State(state): State<WebState>,
    Path(id): Path<String>,
    Json(body): Json<SendMessageBody>,
) -> Result<Response, ApiError> {
    let text = body.text.trim().to_string();
    if text.is_empty() {
        return Err(ApiError(StatusCode::BAD_REQUEST, "empty message".into()));
    }
    if text.len() > MAX_MESSAGE_BYTES {
        return Err(ApiError(
            StatusCode::PAYLOAD_TOO_LARGE,
            format!("message exceeds {MAX_MESSAGE_BYTES} bytes"),
        ));
    }
    ensure_web_session(&state, &id).await?;

    let req = NormalizedRequest {
        request_id: uuid::Uuid::new_v4(),
        ingress: IngressKind::Web,
        agent_group: state.agent_group.clone(),
        session_id: Some(id.clone()),
        channel_peer: None,
        user_text: text,
        attachments: Vec::new(),
        model_override: None,
    };

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<WebEvent>();
    let _ = tx.send(WebEvent::Start {
        session_id: id.clone(),
    });
    let runner = state.runner.clone();
    // The turn runs in its own task: a browser disconnect drops the stream receiver but
    // never cancels the turn (it is persisted like any other). Use /interrupt to stop.
    tokio::spawn(async move {
        let progress = ChannelProgress::new(tx.clone());
        let final_event = match runner.run_turn(req, &progress).await {
            Ok(resp) => WebEvent::Done {
                reply: sanitize_user_reply(&resp.text),
                session_id: resp.session_id,
                run_id: resp.run_id,
                interrupted: resp.interrupted,
            },
            Err(e) => {
                tracing::warn!("web turn failed for session {id}: {e:#}");
                WebEvent::Error {
                    message: format!("{e:#}"),
                }
            }
        };
        let _ = tx.send(final_event);
    });

    let stream = UnboundedReceiverStream::new(rx).map(|ev| Ok::<_, Infallible>(ev.to_sse()));
    let mut resp = Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response();
    let h = resp.headers_mut();
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.insert(
        header::HeaderName::from_static("x-accel-buffering"),
        HeaderValue::from_static("no"),
    );
    Ok(resp)
}

#[derive(Debug, Serialize)]
pub(crate) struct InterruptResponse {
    interrupted: bool,
}

pub(crate) async fn interrupt(
    State(state): State<WebState>,
    Path(id): Path<String>,
) -> Result<Json<InterruptResponse>, ApiError> {
    ensure_web_session(&state, &id).await?;
    let interrupted = state.runner.interrupt_session(&id).await;
    Ok(Json(InterruptResponse { interrupted }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: i64, role: &str, content: &str) -> HistoryMessage {
        HistoryMessage {
            id,
            role: role.into(),
            content: content.into(),
            timestamp: 1.0,
        }
    }

    #[test]
    fn history_hides_tool_results_and_marks_compaction() {
        let out = history_for_display(vec![
            row(1, "user", "hi"),
            row(
                2,
                "assistant",
                "Hello!\n\n<!-- tool-results -->\n[exec exit=0]\nuid=0",
            ),
            row(3, "compaction", "summary of earlier"),
            row(4, "system", "internal"),
            row(5, "assistant", "<!-- tool-results -->\n[exec exit=0]"),
        ]);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].role, "user");
        assert_eq!(out[1].role, "assistant");
        assert_eq!(out[1].text, "Hello!");
        assert_eq!(out[2].role, "summary");
    }

    #[test]
    fn escapes_html() {
        assert_eq!(
            html_escape(r#"<a href="x">&'"#),
            "&lt;a href=&quot;x&quot;&gt;&amp;&#39;"
        );
    }
}
