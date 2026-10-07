//! `AppError`: every failure a handler returns, its HTTP status, and the user message key (`Msg`) that
//! ctx::localize_errors renders in the page language.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("not found")]
    NotFound,
    #[error("forbidden")]
    Forbidden,
    #[error("unauthorized")]
    Unauthorized,
    #[error("bad request: {0}")]
    BadRequest(Msg),
    #[error("conflict: {0}")]
    Conflict(Msg),
    #[error("store error: {0}")]
    Store(#[from] rustical_store::Error),
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error("session error: {0}")]
    Session(#[from] tower_sessions::session::Error),
    #[error("ical error: {0}")]
    Ical(#[from] rustical_ical::Error),
    #[error("{0}")]
    Internal(String),
    /// A request refused before it is looked at, with its own status (413 body too large, 415 content type,
    /// 428 write without the version it is based on).
    #[error("{1}")]
    Rejected(StatusCode, Msg),
}

/// A message for a user: a Fluent key and its arguments. PractiCal's own errors (BadRequest, Conflict) carry
/// one; every other error maps to one (`AppError::user_text`). Its Display is the English text (en.ftl) —
/// what app-token / Basic-auth clients read; `ctx::localize_errors` and the settings answers render it in
/// the user's language. On every error response (`IntoResponse`); a store's/database's own English text
/// stays in the log.
#[derive(Debug, Clone)]
pub struct Msg {
    pub key: &'static str,
    pub args: Vec<(&'static str, String)>,
}

impl Msg {
    pub fn render(&self, i18n: &crate::i18n::I18n, lang: &str) -> String {
        let mut a = fluent_bundle::FluentArgs::new();
        for (k, v) in &self.args {
            a.set(*k, v.clone());
        }
        i18n.t_args(lang, self.key, Some(&a))
    }
}

impl std::fmt::Display for Msg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.render(crate::i18n::I18n::shared(), "en"))
    }
}

/// Tests compare a message with its English text (what an API client reads).
#[cfg(test)]
impl PartialEq<&str> for Msg {
    fn eq(&self, other: &&str) -> bool {
        self.to_string() == *other
    }
}

impl AppError {
    /// A bad request with PractiCal's message `key` (en.ftl holds its English text).
    pub fn bad(key: &'static str) -> Self {
        Self::BadRequest(Msg { key, args: vec![] })
    }
    /// The same with one argument (`{ $name }` in the message).
    pub fn bad1(key: &'static str, name: &'static str, value: impl ToString) -> Self {
        Self::BadRequest(Msg { key, args: vec![(name, value.to_string())] })
    }
    pub fn conflict(key: &'static str) -> Self {
        Self::Conflict(Msg { key, args: vec![] })
    }

    /// The message for a user; None where no user reads it (Unauthorized: a client without a login).
    pub fn user_text(&self) -> Option<Msg> {
        let key = match self {
            Self::BadRequest(m) | Self::Conflict(m) | Self::Rejected(_, m) => return Some(m.clone()),
            Self::NotFound | Self::Store(rustical_store::Error::NotFound) => "flash-not-found",
            Self::Forbidden | Self::Store(rustical_store::Error::ReadOnly) => "flash-forbidden",
            Self::Store(rustical_store::Error::AlreadyExists) => "flash-exists",
            Self::Ical(e) => return Some(Msg { key: "flash-bad-data", args: vec![("detail", e.to_string())] }),
            Self::Unauthorized => return None,
            _ if self.is_busy() => "flash-busy",
            Self::Store(_) | Self::Db(_) | Self::Session(_) | Self::Internal(_) => "flash-failed",
        };
        Some(Msg { key, args: vec![] })
    }

    /// The database was busy (another writer held the lock past sqlx's 5 s busy timeout, or the pool had no
    /// free connection in time): worth a retry, answered 503 + Retry-After instead of a generic 500.
    pub fn is_busy(&self) -> bool {
        fn sqlx_busy(e: &sqlx::Error) -> bool {
            match e {
                // SQLITE_BUSY (5) / SQLITE_LOCKED (6) and their extended codes (low byte)
                sqlx::Error::Database(d) => d.code().and_then(|c| c.parse::<i32>().ok()).is_some_and(|c| matches!(c & 0xff, 5 | 6)),
                sqlx::Error::PoolTimedOut => true,
                _ => false,
            }
        }
        match self {
            Self::Db(e) => sqlx_busy(e),
            // RustiCal's store wraps its sqlx errors in anyhow (rustical_store_sqlite error.rs)
            Self::Store(rustical_store::Error::Other(a)) => a.downcast_ref::<sqlx::Error>().is_some_and(sqlx_busy),
            _ => false,
        }
    }

    /// The message for a user in `lang` (`user_text`, else the error's own text).
    pub fn user_message(&self, i18n: &crate::i18n::I18n, lang: &str) -> String {
        self.user_text().map_or_else(|| self.to_string(), |u| u.render(i18n, lang))
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match &self {
            Self::NotFound | Self::Store(rustical_store::Error::NotFound) => StatusCode::NOT_FOUND,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Rejected(s, _) => *s,
            Self::BadRequest(_) | Self::Ical(_) => StatusCode::BAD_REQUEST,
            Self::Conflict(_) | Self::Store(rustical_store::Error::AlreadyExists) => StatusCode::CONFLICT,
            _ if self.is_busy() => StatusCode::SERVICE_UNAVAILABLE,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        if status.is_server_error() {
            tracing::error!("{self}");
        } else {
            tracing::debug!("{self}");
        }
        let text = self.user_text();
        // a server error's own text (SQL, file paths, upstream URLs) stays in the log: every client — app
        // tokens and anonymous ones too, which `ctx::localize_errors` leaves alone — reads the English message
        let body = match (&text, status.is_server_error()) {
            (Some(t), true) => t.to_string(),
            _ => self.to_string(),
        };
        let mut r = (status, body).into_response();
        if status == StatusCode::SERVICE_UNAVAILABLE {
            r.headers_mut().insert(axum::http::header::RETRY_AFTER, axum::http::HeaderValue::from_static("2"));
        }
        if let Some(t) = text {
            r.extensions_mut().insert(t);
        }
        r
    }
}

pub type AppResult<T> = Result<T, AppError>;

/// Middleware: a panicking handler answers 500 with the generic message (`flash-failed`, localized like any
/// error inside `ctx::localize_errors`) instead of dropping the connection — which a browser reports as
/// "Failed to fetch" and Traefik as 502. The panic and the request line go to the log; the client reads
/// nothing of them. Covers the handler and the middleware inside this layer, not a streaming body.
pub async fn catch_panic(req: axum::extract::Request, next: axum::middleware::Next) -> Response {
    use futures_util::FutureExt;
    let line = format!("{} {}", req.method(), req.uri().path());
    match std::panic::AssertUnwindSafe(next.run(req)).catch_unwind().await {
        Ok(r) => r,
        Err(p) => {
            let msg = p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_default();
            AppError::Internal(format!("{line}: a handler panicked: {msg}")).into_response()
        }
    }
}
