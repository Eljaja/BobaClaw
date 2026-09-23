//! Bearer-token gate for `/api/web/*`.

use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use bobaclaw_core::{is_loopback_bind, WebConfig};

use crate::state::WebState;

/// Token policy for the web API. `None` = no token configured (loopback-only use).
#[derive(Debug, Clone, Default)]
pub struct WebAuth {
    token: Option<String>,
}

impl WebAuth {
    pub fn new(token: Option<String>) -> Self {
        Self {
            token: token.filter(|t| !t.is_empty()),
        }
    }

    /// Read the token from `channels.web.auth_token_env`.
    pub fn from_config(cfg: &WebConfig) -> Self {
        Self::new(cfg.resolve_auth_token())
    }

    pub fn token_configured(&self) -> bool {
        self.token.is_some()
    }

    /// `true` if the request may proceed. With no token configured every request is allowed
    /// (the bind policy in [`check_bind_policy`] guarantees loopback in that case).
    pub fn authorize(&self, headers: &HeaderMap) -> bool {
        let Some(expected) = self.token.as_deref() else {
            return true;
        };
        let Some(presented) = bearer_token(headers) else {
            return false;
        };
        constant_time_eq(presented.as_bytes(), expected.as_bytes())
    }
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.trim().split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = token.trim();
    (!token.is_empty()).then_some(token)
}

/// Compare without early exit on the first differing byte (length is not secret).
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    std::hint::black_box(diff) == 0
}

/// Refuse to expose the web API on a non-loopback address without a token.
pub fn check_bind_policy(bind: &str, auth: &WebAuth) -> anyhow::Result<()> {
    if auth.token_configured() {
        return Ok(());
    }
    if !is_loopback_bind(bind) {
        anyhow::bail!(
            "channels.web: refusing to serve the web UI on non-loopback bind {bind} without a token; \
             set the env var named by channels.web.auth_token_env or bind to 127.0.0.1"
        );
    }
    tracing::warn!(
        "channels.web: no auth token configured (channels.web.auth_token_env); \
         /api/web/* is open to any local process on {bind}"
    );
    Ok(())
}

pub(crate) async fn require_auth(
    State(state): State<WebState>,
    req: Request,
    next: Next,
) -> Response {
    if state.auth.authorize(req.headers()) {
        return next.run(req).await;
    }
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, "Bearer")],
        Json(serde_json::json!({ "error": "unauthorized" })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers(auth: Option<&str>) -> HeaderMap {
        let mut h = HeaderMap::new();
        if let Some(v) = auth {
            h.insert(header::AUTHORIZATION, HeaderValue::from_str(v).unwrap());
        }
        h
    }

    #[test]
    fn constant_time_eq_basics() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn token_required_when_configured() {
        let auth = WebAuth::new(Some("s3cret".into()));
        assert!(!auth.authorize(&headers(None)));
        assert!(!auth.authorize(&headers(Some("Bearer wrong"))));
        assert!(!auth.authorize(&headers(Some("Basic s3cret"))));
        assert!(!auth.authorize(&headers(Some("Bearer"))));
        assert!(auth.authorize(&headers(Some("Bearer s3cret"))));
        assert!(auth.authorize(&headers(Some("bearer  s3cret "))));
    }

    #[test]
    fn open_when_no_token() {
        let auth = WebAuth::new(None);
        assert!(auth.authorize(&headers(None)));
        let empty = WebAuth::new(Some(String::new()));
        assert!(!empty.token_configured());
    }

    #[test]
    fn bind_policy() {
        let none = WebAuth::new(None);
        let tok = WebAuth::new(Some("t".into()));
        assert!(check_bind_policy("127.0.0.1", &none).is_ok());
        assert!(check_bind_policy("::1", &none).is_ok());
        assert!(check_bind_policy("0.0.0.0", &none).is_err());
        assert!(check_bind_policy("192.168.0.2", &none).is_err());
        assert!(check_bind_policy("0.0.0.0", &tok).is_ok());
    }
}
