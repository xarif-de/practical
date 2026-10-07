//! CSRF defence. Two layers: (1) a middleware rejecting state-changing requests
//! whose browser-provided origin metadata is not same-origin (Sec-Fetch-Site /
//! Origin), unless the request authenticates with HTTP Basic (a valid app token, no
//! cookie session involved: `token_authenticated`); (2) a per-session token in every HTML form and in the
//! `X-CSRF-Token` header of fetch() calls, checked by the handlers.

use crate::ctx::SESSION_CSRF;
use crate::error::AppError;
use crate::state::SharedState;
use axum::extract::{Request, State};
use axum::http::{Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use tower_sessions::Session;

pub async fn origin_guard(State(state): State<SharedState>, request: Request, next: Next) -> Response {
    let m = request.method();
    if matches!(*m, Method::GET | Method::HEAD | Method::OPTIONS) {
        return next.run(request).await;
    }
    if token_authenticated(&request).await {
        return next.run(request).await;
    }
    let fetch_site = request.headers().get("sec-fetch-site").and_then(|v| v.to_str().ok());
    let origin = request.headers().get(header::ORIGIN).and_then(|v| v.to_str().ok());
    let ok = match (fetch_site, origin) {
        (Some("same-origin" | "none"), _) => true,
        (_, Some(o)) => o.eq_ignore_ascii_case(&state.config.public_origin()),
        (Some(_), None) => false,
        // no metadata at all: not a modern browser → curl & co. must use Basic auth
        (None, None) => false,
    };
    if ok {
        next.run(request).await
    } else {
        tracing::warn!("cross-site request rejected: {} {}", m, request.uri().path());
        (StatusCode::FORBIDDEN, "cross-site request rejected").into_response()
    }
}

/// The request is authenticated by an app token (HTTP Basic) and by nothing else: exempt from both CSRF
/// layers, as no browser sends Basic credentials on its own (no WWW-Authenticate anywhere). RustiCal's
/// AuthenticationLayer keeps the cookie session's principal when the Basic credentials are invalid, so the
/// header alone proves nothing: a request with a cookie session (a user in it) is checked like any other.
pub fn token_authenticated(request: &Request) -> impl Future<Output = bool> + Send + use<> {
    let has_basic = request.headers().get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).is_some_and(|v| v.starts_with("Basic "));
    let candidate = has_basic && request.extensions().get::<rustical_store::auth::Principal>().is_some();
    let session = request.extensions().get::<Session>().cloned();
    async move {
        match (candidate, session) {
            (false, _) => false,
            (true, Some(s)) => s.get::<String>(crate::ctx::SESSION_USER).await.ok().flatten().is_none(),
            (true, None) => true,
        }
    }
}

pub async fn check(session: &Session, token: &str) -> Result<(), AppError> {
    let expected: Option<String> = session.get(SESSION_CSRF).await?;
    match expected {
        Some(e) if !e.is_empty() && same(e.as_bytes(), token.as_bytes()) => Ok(()),
        _ => Err(AppError::Forbidden),
    }
}

/// Equal in a time that does not depend on where the first difference is (the length is not secret).
pub fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Extracts the token from `X-CSRF-Token` for JSON/fetch requests.
pub fn header_token(headers: &axum::http::HeaderMap) -> String {
    headers
        .get("x-csrf-token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string()
}
