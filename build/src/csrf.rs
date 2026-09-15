//! CSRF defence. Two layers: (1) a middleware rejecting state-changing requests
//! whose browser-provided origin metadata is not same-origin (Sec-Fetch-Site /
//! Origin), unless the request authenticates with HTTP Basic (app token; no cookie
//! session involved); (2) a per-session token in every HTML form and in the
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
    let has_basic = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("Basic "));
    if has_basic {
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

pub async fn check(session: &Session, token: &str) -> Result<(), AppError> {
    let expected: Option<String> = session.get(SESSION_CSRF).await?;
    match expected {
        Some(e) if !e.is_empty() && e == token => Ok(()),
        _ => Err(AppError::Forbidden),
    }
}

/// Extracts the token from `X-CSRF-Token` for JSON/fetch requests.
pub fn header_token(headers: &axum::http::HeaderMap) -> String {
    headers
        .get("x-csrf-token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string()
}
