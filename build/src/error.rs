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
    BadRequest(String),
    #[error("conflict: {0}")]
    Conflict(String),
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

/// What a user reads for an error: a translated message (Fluent key, with the error's detail as `$detail`
/// where the key takes one). On every error response (`IntoResponse`), so `ctx::localize_errors` can put
/// it in the user's language; the error's own text (the store's / database's English) stays in the log.
#[derive(Debug, Clone)]
pub struct UserText {
    pub key: &'static str,
    pub detail: String,
}

impl AppError {
    /// The message for a user; None where the error's own text is that message (BadRequest and Conflict
    /// carry the handler's words) or no user reads it (Unauthorized: a client without a login).
    pub fn user_text(&self) -> Option<UserText> {
        let key = match self {
            Self::NotFound | Self::Store(rustical_store::Error::NotFound) => "flash-not-found",
            Self::Forbidden | Self::Store(rustical_store::Error::ReadOnly) => "flash-forbidden",
            Self::Store(rustical_store::Error::AlreadyExists) => "flash-exists",
            Self::Ical(e) => return Some(UserText { key: "flash-bad-data", detail: e.to_string() }),
            Self::BadRequest(_) | Self::Conflict(_) | Self::Unauthorized => return None,
            Self::Store(_) | Self::Db(_) | Self::Session(_) | Self::Internal(_) => "flash-failed",
        };
        Some(UserText { key, detail: String::new() })
    }

    /// The message for a user in `lang` (`user_text`, else the error's own text).
    pub fn user_message(&self, i18n: &crate::i18n::I18n, lang: &str) -> String {
        match (self.user_text(), self) {
            (Some(u), _) => u.render(i18n, lang),
            (None, Self::BadRequest(m) | Self::Conflict(m)) => m.clone(),
            (None, e) => e.to_string(),
        }
    }
}

impl UserText {
    pub fn render(&self, i18n: &crate::i18n::I18n, lang: &str) -> String {
        i18n.t1(lang, self.key, "detail", self.detail.clone())
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
