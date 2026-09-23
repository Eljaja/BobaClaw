//! Local browser chat UI channel (`channels.web`).
//!
//! * `GET /ui` — self-contained HTML page (no auth; it holds no data).
//! * `/api/web/*` — JSON + SSE API, bearer token when `channels.web.auth_token_env` is set.
//!
//! The gateway mounts [`router`] via [`mount`]; `bobaclaw channel web start` runs
//! [`serve`] as a standalone server. See `harness/channels/web.md`.

mod api;
pub mod auth;
pub mod events;
mod state;

use std::sync::Arc;

use axum::middleware::from_fn_with_state;
use axum::routing::{get, post};
use axum::Router;
use bobaclaw_agent::AgentDispatcher;
use bobaclaw_core::BobaConfig;

pub use api::{ApiMessage, ApiSession};
pub use auth::{check_bind_policy, WebAuth};
pub use events::WebEvent;
pub use state::{TurnRunner, WebState};

/// Web UI routes backed by the agent dispatcher. State is internal; the result can be
/// `.merge`d into any `Router<()>` (layers applied afterwards cover these routes too).
pub fn router(dispatcher: Arc<AgentDispatcher>, config: &BobaConfig) -> Router {
    let pool = dispatcher.pool().clone();
    let state = WebState::new(
        dispatcher as Arc<dyn TurnRunner>,
        pool,
        config.default_agent_group.clone(),
        config.channels.web.title.clone(),
        WebAuth::from_config(&config.channels.web),
    );
    router_with_state(state)
}

/// Routes over an explicit [`WebState`] (tests, custom runners).
pub fn router_with_state(state: WebState) -> Router {
    let api = Router::new()
        .route("/api/web/config", get(api::api_config))
        .route(
            "/api/web/sessions",
            get(api::list_sessions).post(api::create_session),
        )
        .route(
            "/api/web/sessions/{id}/messages",
            get(api::list_messages).post(api::send_message),
        )
        .route("/api/web/sessions/{id}/interrupt", post(api::interrupt))
        .route_layer(from_fn_with_state(state.clone(), auth::require_auth))
        .with_state(state.clone());
    Router::new()
        .route("/ui", get(api::ui_index))
        .route("/ui/", get(api::ui_index))
        .with_state(state)
        .merge(api)
}

/// Merge the web routes into the gateway app when `channels.web.enabled`.
///
/// Fails if the gateway binds a non-loopback address and no web token is configured.
pub fn mount(
    app: Router,
    dispatcher: Arc<AgentDispatcher>,
    config: &BobaConfig,
) -> anyhow::Result<Router> {
    if !config.channels.web.enabled {
        return Ok(app);
    }
    check_bind_policy(
        &config.gateway.bind,
        &WebAuth::from_config(&config.channels.web),
    )?;
    tracing::info!(
        "web UI mounted at http://{}:{}/ui",
        config.gateway.bind,
        config.gateway.port
    );
    Ok(app.merge(router(dispatcher, config)))
}

/// Standalone server for `bobaclaw channel web start`: `/ui`, `/api/web/*`, `/health`
/// on `channels.web.bind:port`. Stops on Ctrl+C.
pub async fn serve(dispatcher: Arc<AgentDispatcher>, config: &BobaConfig) -> anyhow::Result<()> {
    let web = &config.channels.web;
    if !web.enabled {
        anyhow::bail!("enable channels.web.enabled in config.yaml");
    }
    check_bind_policy(&web.bind, &WebAuth::from_config(web))?;
    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .merge(router(dispatcher, config));
    let host = if web.bind.contains(':') && !web.bind.starts_with('[') {
        format!("[{}]", web.bind)
    } else {
        web.bind.clone()
    };
    let addr = format!("{host}:{}", web.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("web UI listening on http://{addr}/ui");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;
    use axum::body::{to_bytes, Body};
    use axum::http::{header, Request, StatusCode};
    use bobaclaw_agent::{AgentEvent, AgentProgress, AgentResponse};
    use bobaclaw_core::{IngressKind, NormalizedRequest};
    use bobaclaw_state::{SessionStore, StateDb};
    use tower::ServiceExt;

    use super::*;

    #[derive(Default)]
    struct FakeRunner {
        fail: bool,
        seen: Mutex<Vec<NormalizedRequest>>,
        interrupted: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl TurnRunner for FakeRunner {
        async fn run_turn(
            &self,
            req: NormalizedRequest,
            progress: &dyn AgentProgress,
        ) -> anyhow::Result<AgentResponse> {
            self.seen.lock().unwrap().push(req.clone());
            if self.fail {
                anyhow::bail!("provider unreachable");
            }
            progress.on_event(AgentEvent::LlmThinking { iteration: 1 });
            progress.on_event(AgentEvent::ToolStart {
                name: "exec".into(),
                label: "ls".into(),
            });
            progress.on_event(AgentEvent::ToolEnd {
                name: "exec".into(),
                exit_code: 0,
                preview: "a.txt".into(),
            });
            Ok(AgentResponse {
                text: "Here <b>you</b> go\n\n<!-- tool-results -->\n[exec exit=0]".into(),
                session_id: req.session_id.unwrap_or_default(),
                run_id: Some("run_1".into()),
                executed: true,
                interrupted: false,
                auto_saved_skill: None,
                auto_saved_memory: None,
            })
        }

        async fn interrupt_session(&self, session_id: &str) -> bool {
            self.interrupted.lock().unwrap().push(session_id.into());
            true
        }
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        db: StateDb,
        runner: Arc<FakeRunner>,
        app: Router,
    }

    async fn fixture(token: Option<&str>, fail: bool) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let db = StateDb::open(&dir.path().join("state.db")).await.unwrap();
        let runner = Arc::new(FakeRunner {
            fail,
            ..Default::default()
        });
        let state = WebState::new(
            runner.clone() as Arc<dyn TurnRunner>,
            db.pool().clone(),
            "home",
            "Boba <UI>",
            WebAuth::new(token.map(str::to_string)),
        );
        Fixture {
            _dir: dir,
            db,
            runner,
            app: router_with_state(state),
        }
    }

    fn req(method: &str, uri: &str, token: Option<&str>, body: Option<&str>) -> Request<Body> {
        let mut b = Request::builder().method(method).uri(uri);
        if let Some(t) = token {
            b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }
        match body {
            Some(json) => b
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json.to_string()))
                .unwrap(),
            None => b.body(Body::empty()).unwrap(),
        }
    }

    async fn body_string(resp: axum::response::Response) -> String {
        let bytes = to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn ui_served_without_auth_with_security_headers() {
        let f = fixture(Some("tok"), false).await;
        let resp = f.app.oneshot(req("GET", "/ui", None, None)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let h = resp.headers();
        assert!(h[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/html"));
        assert_eq!(h[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
        let csp = h[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .unwrap()
            .to_string();
        assert!(csp.contains("connect-src 'self'"));
        assert!(csp.contains("default-src 'none'"));
        assert!(h.get(header::ACCESS_CONTROL_ALLOW_ORIGIN).is_none());
        let html = body_string(resp).await;
        assert!(html.contains("<title>Boba &lt;UI&gt;</title>"));
        assert!(!html.contains("__CSP_NONCE__"));
        let nonce = csp
            .split("'nonce-")
            .nth(1)
            .and_then(|s| s.split('\'').next())
            .unwrap();
        assert!(html.contains(&format!("<script nonce=\"{nonce}\">")));
    }

    #[tokio::test]
    async fn api_requires_token_when_configured() {
        let f = fixture(Some("tok"), false).await;
        let resp = f
            .app
            .clone()
            .oneshot(req("GET", "/api/web/sessions", None, None))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(resp.headers()[header::WWW_AUTHENTICATE], "Bearer");

        let resp = f
            .app
            .clone()
            .oneshot(req("GET", "/api/web/sessions", Some("nope"), None))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let resp = f
            .app
            .clone()
            .oneshot(req("POST", "/api/web/sessions", None, None))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let resp = f
            .app
            .oneshot(req("GET", "/api/web/sessions", Some("tok"), None))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(body_string(resp).await, "[]");
    }

    #[tokio::test]
    async fn open_without_token() {
        let f = fixture(None, false).await;
        let resp = f
            .app
            .oneshot(req("GET", "/api/web/config", None, None))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&body_string(resp).await).unwrap();
        assert_eq!(v["agent_group"], "home");
    }

    #[tokio::test]
    async fn create_list_and_history() {
        let f = fixture(Some("tok"), false).await;
        let resp = f
            .app
            .clone()
            .oneshot(req("POST", "/api/web/sessions", Some("tok"), None))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let created: ApiSession = serde_json::from_str(&body_string(resp).await).unwrap();
        assert!(created.id.starts_with("sess_"));

        // Non-web sessions are invisible to the web API.
        let store = SessionStore::new(f.db.pool());
        let cli = store.get_or_create_cli("home").await.unwrap();
        store
            .append_message(&cli, "user", "cli only")
            .await
            .unwrap();
        store
            .append_message(&created.id, "user", "first question")
            .await
            .unwrap();
        store
            .append_message(
                &created.id,
                "assistant",
                "answer\n\n<!-- tool-results -->\n[exec exit=0]\nraw",
            )
            .await
            .unwrap();

        let resp = f
            .app
            .clone()
            .oneshot(req("GET", "/api/web/sessions", Some("tok"), None))
            .await
            .unwrap();
        let list: Vec<ApiSession> = serde_json::from_str(&body_string(resp).await).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, created.id);
        assert_eq!(list[0].title, "first question");

        let uri = format!("/api/web/sessions/{}/messages", created.id);
        let resp = f
            .app
            .clone()
            .oneshot(req("GET", &uri, Some("tok"), None))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let msgs: Vec<ApiMessage> = serde_json::from_str(&body_string(resp).await).unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[1].text, "answer");

        let resp = f
            .app
            .oneshot(req(
                "GET",
                &format!("/api/web/sessions/{cli}/messages"),
                Some("tok"),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn send_message_streams_sse_until_done() {
        let f = fixture(None, false).await;
        let sid = SessionStore::new(f.db.pool())
            .create_session("home", IngressKind::Web)
            .await
            .unwrap();
        let uri = format!("/api/web/sessions/{sid}/messages");
        let resp = f
            .app
            .clone()
            .oneshot(req(
                "POST",
                &uri,
                None,
                Some(r#"{"text":"  list files  "}"#),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(resp.headers()[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/event-stream"));
        let body = body_string(resp).await;
        let names: Vec<&str> = body
            .lines()
            .filter_map(|l| l.strip_prefix("event: "))
            .collect();
        assert_eq!(
            names,
            vec!["start", "thinking", "tool_start", "tool_end", "done"]
        );
        let done = body
            .lines()
            .filter_map(|l| l.strip_prefix("data: "))
            .next_back()
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(done).unwrap();
        assert_eq!(v["reply"], "Here <b>you</b> go");
        assert_eq!(v["run_id"], "run_1");

        let seen = f.runner.seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].ingress, IngressKind::Web);
        assert_eq!(seen[0].session_id.as_deref(), Some(sid.as_str()));
        assert_eq!(seen[0].user_text, "list files");
    }

    #[tokio::test]
    async fn send_message_error_event() {
        let f = fixture(None, true).await;
        let sid = SessionStore::new(f.db.pool())
            .create_session("home", IngressKind::Web)
            .await
            .unwrap();
        let uri = format!("/api/web/sessions/{sid}/messages");
        let resp = f
            .app
            .oneshot(req("POST", &uri, None, Some(r#"{"text":"hi"}"#)))
            .await
            .unwrap();
        let body = body_string(resp).await;
        assert!(body.contains("event: error"));
        assert!(body.contains("provider unreachable"));
        assert!(!body.contains("event: done"));
    }

    #[tokio::test]
    async fn send_message_validation() {
        let f = fixture(None, false).await;
        let sid = SessionStore::new(f.db.pool())
            .create_session("home", IngressKind::Web)
            .await
            .unwrap();
        let uri = format!("/api/web/sessions/{sid}/messages");
        let resp = f
            .app
            .clone()
            .oneshot(req("POST", &uri, None, Some(r#"{"text":"   "}"#)))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let resp = f
            .app
            .oneshot(req(
                "POST",
                "/api/web/sessions/sess_missing/messages",
                None,
                Some(r#"{"text":"hi"}"#),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        assert!(f.runner.seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn interrupt_calls_runner() {
        let f = fixture(Some("tok"), false).await;
        let sid = SessionStore::new(f.db.pool())
            .create_session("home", IngressKind::Web)
            .await
            .unwrap();
        let uri = format!("/api/web/sessions/{sid}/interrupt");
        let resp = f
            .app
            .clone()
            .oneshot(req("POST", &uri, None, None))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let resp = f
            .app
            .oneshot(req("POST", &uri, Some("tok"), None))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(body_string(resp).await, r#"{"interrupted":true}"#);
        assert_eq!(*f.runner.interrupted.lock().unwrap(), vec![sid]);
    }
}
