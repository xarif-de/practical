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
            Self::BadRequest(m) | Self::Conflict(m) => return Some(m.clone()),
            Self::NotFound | Self::Store(rustical_store::Error::NotFound) => "flash-not-found",
            Self::Forbidden | Self::Store(rustical_store::Error::ReadOnly) => "flash-forbidden",
            Self::Store(rustical_store::Error::AlreadyExists) => "flash-exists",
            Self::Ical(e) => return Some(Msg { key: "flash-bad-data", args: vec![("detail", e.to_string())] }),
            Self::Unauthorized => return None,
            Self::Store(_) | Self::Db(_) | Self::Session(_) | Self::Internal(_) => "flash-failed",
        };
        Some(Msg { key, args: vec![] })
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
            Self::BadRequest(_) | Self::Ical(_) => StatusCode::BAD_REQUEST,
            Self::Conflict(_) | Self::Store(rustical_store::Error::AlreadyExists) => StatusCode::CONFLICT,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        if status.is_server_error() {
            tracing::error!("{self}");
        } else {
            tracing::debug!("{self}");
        }
        let text = self.user_text();
        let mut r = (status, self.to_string()).into_response();
        if let Some(t) = text {
            r.extensions_mut().insert(t);
        }
        r
    }
}

pub type AppResult<T> = Result<T, AppError>;
