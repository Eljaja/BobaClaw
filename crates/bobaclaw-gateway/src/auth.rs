//! Bearer-token authentication for the gateway HTTP API.
//!
//! Every route except `GET /health` and the web UI shell page (`/ui`) requires `Authorization: Bearer <token>` when a token is
//! configured (`gateway.auth_token` or the `gateway.auth_token_env` variable). Startup fails
//! closed: a non-loopback bind without a token is refused.

use std::net::IpAddr;
use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use bobaclaw_core::GatewayConfig;

/// Paths reachable without a token: health probes and the web UI shell page (it holds no
/// data; its `/api/web/*` calls carry the bearer token).
const PUBLIC_PATHS: &[&str] = &["/health", "/ui", "/ui/"];

#[derive(Clone)]
pub struct GatewayAuth {
    token: Option<Arc<[u8]>>,
}

impl std::fmt::Debug for GatewayAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GatewayAuth")
            .field("required", &self.token.is_some())
            .finish()
    }
}

impl GatewayAuth {
    pub fn with_token(token: &str) -> Self {
        Self {
            token: Some(Arc::from(token.as_bytes())),
        }
    }

    /// No authentication (loopback-only deployments without a configured token).
    pub fn disabled() -> Self {
        Self { token: None }
    }

    pub fn is_required(&self) -> bool {
        self.token.is_some()
    }

    /// Resolve the startup auth policy from config (fail closed on non-loopback binds).
    pub fn from_config(cfg: &GatewayConfig) -> anyhow::Result<Self> {
        Self::from_parts(&cfg.bind, &cfg.auth_token_env, cfg.resolve_auth_token())
    }

    fn from_parts(bind: &str, token_env: &str, token: Option<String>) -> anyhow::Result<Self> {
        if let Some(token) = token {
            return Ok(Self::with_token(&token));
        }
        if is_loopback_bind(bind) {
            tracing::warn!(
                "gateway auth disabled: no token configured (set env {token_env} or \
                 gateway.auth_token); allowed only because bind {bind} is loopback"
            );
            return Ok(Self::disabled());
        }
        anyhow::bail!(
            "refusing to start gateway on non-loopback bind '{bind}' without an auth token: \
             set env {token_env} (gateway.auth_token_env) or gateway.auth_token, \
             or bind to 127.0.0.1"
        )
    }
}

/// Wrap the whole router (including the 404 fallback) with the bearer-token check.
pub fn with_auth<S>(router: Router<S>, auth: GatewayAuth) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router.layer(middleware::from_fn_with_state(auth, require_bearer))
}

async fn require_bearer(State(auth): State<GatewayAuth>, req: Request, next: Next) -> Response {
    let Some(expected) = auth.token.as_deref() else {
        return next.run(req).await;
    };
    if PUBLIC_PATHS.contains(&req.uri().path()) {
        return next.run(req).await;
    }
    let presented = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_bearer);
    match presented {
        Some(token) if constant_time_eq(token.as_bytes(), expected) => next.run(req).await,
        _ => unauthorized(),
    }
}

fn parse_bearer(value: &str) -> Option<&str> {
    let (scheme, rest) = value.trim().split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = rest.trim();
    (!token.is_empty()).then_some(token)
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, "Bearer")],
        Json(serde_json::json!({ "error": "unauthorized" })),
    )
        .into_response()
}

/// Compare without early exit on the first differing byte. Runtime depends only on the
/// length of `expected`, so token contents are not leaked through timing.
fn constant_time_eq(presented: &[u8], expected: &[u8]) -> bool {
    let mut diff = (presented.len() ^ expected.len()) as u64;
    for (i, e) in expected.iter().enumerate() {
        let p = presented.get(i).copied().unwrap_or(0);
        diff |= u64::from(p ^ e);
    }
    std::hint::black_box(diff) == 0
}

fn is_loopback_bind(bind: &str) -> bool {
    let host = bind.trim().trim_start_matches('[').trim_end_matches(']');
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.parse::<IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request as HttpRequest;
    use axum::routing::{get, post};
    use tower::ServiceExt;

    fn app(auth: GatewayAuth) -> Router {
        let router = Router::new()
            .route("/health", get(|| async { "ok" }))
            .route("/ui", get(|| async { "page" }))
            .route("/api/web/sessions", get(|| async { "sessions" }))
            .route("/api/agent", post(|| async { "agent" }))
            .route("/v1/chat/completions", post(|| async { "chat" }));
        with_auth(router, auth)
    }

    async fn status(app: Router, method: &str, path: &str, auth: Option<&str>) -> StatusCode {
        let mut req = HttpRequest::builder().method(method).uri(path);
        if let Some(value) = auth {
            req = req.header(header::AUTHORIZATION, value);
        }
        app.oneshot(req.body(Body::empty()).unwrap())
            .await
            .unwrap()
            .status()
    }

    #[tokio::test]
    async fn health_is_public() {
        let app = app(GatewayAuth::with_token("t0ken"));
        assert_eq!(status(app, "GET", "/health", None).await, StatusCode::OK);
    }

    #[tokio::test]
    async fn web_ui_page_is_public_but_web_api_is_not() {
        let app = app(GatewayAuth::with_token("t0ken"));
        assert_eq!(
            status(app.clone(), "GET", "/ui", None).await,
            StatusCode::OK
        );
        assert_eq!(
            status(app.clone(), "GET", "/api/web/sessions", None).await,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            status(app, "GET", "/api/web/sessions", Some("Bearer t0ken")).await,
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn api_requires_bearer_token() {
        let app = app(GatewayAuth::with_token("t0ken"));
        for path in ["/api/agent", "/v1/chat/completions"] {
            assert_eq!(
                status(app.clone(), "POST", path, None).await,
                StatusCode::UNAUTHORIZED
            );
            assert_eq!(
                status(app.clone(), "POST", path, Some("Bearer wrong")).await,
                StatusCode::UNAUTHORIZED
            );
            assert_eq!(
                status(app.clone(), "POST", path, Some("Bearer t0ke")).await,
                StatusCode::UNAUTHORIZED
            );
            assert_eq!(
                status(app.clone(), "POST", path, Some("Basic t0ken")).await,
                StatusCode::UNAUTHORIZED
            );
            assert_eq!(
                status(app.clone(), "POST", path, Some("Bearer t0ken")).await,
                StatusCode::OK
            );
            assert_eq!(
                status(app.clone(), "POST", path, Some("bearer  t0ken ")).await,
                StatusCode::OK
            );
        }
    }

    #[tokio::test]
    async fn unknown_routes_are_also_protected() {
        let app = app(GatewayAuth::with_token("t0ken"));
        assert_eq!(
            status(app.clone(), "GET", "/api/spawn/jobs", None).await,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            status(app, "GET", "/nope", Some("Bearer t0ken")).await,
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn unauthorized_sets_www_authenticate() {
        let app = app(GatewayAuth::with_token("t0ken"));
        let resp = app
            .oneshot(HttpRequest::post("/api/agent").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.headers()[header::WWW_AUTHENTICATE], "Bearer");
    }

    #[tokio::test]
    async fn disabled_auth_passes_through() {
        let app = app(GatewayAuth::disabled());
        assert_eq!(
            status(app, "POST", "/api/agent", None).await,
            StatusCode::OK
        );
    }

    #[test]
    fn startup_policy_fails_closed_on_non_loopback() {
        let env = "BOBACLAW_GATEWAY_TOKEN";
        for bind in ["0.0.0.0", "::", "192.0.2.10", "gateway.example"] {
            let err = GatewayAuth::from_parts(bind, env, None).unwrap_err();
            assert!(err.to_string().contains("refusing to start"), "{bind}");
            assert!(err.to_string().contains(env));
        }
        for bind in ["127.0.0.1", "::1", "[::1]", "localhost", "127.0.0.2"] {
            assert!(!GatewayAuth::from_parts(bind, env, None)
                .unwrap()
                .is_required());
        }
        assert!(GatewayAuth::from_parts("0.0.0.0", env, Some("x".into()))
            .unwrap()
            .is_required());
    }

    #[test]
    fn constant_time_eq_basics() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abd", b"abc"));
        assert!(!constant_time_eq(b"ab", b"abc"));
        assert!(!constant_time_eq(b"abcd", b"abc"));
        assert!(!constant_time_eq(b"", b"abc"));
    }
}
