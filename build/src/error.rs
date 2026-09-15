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
        (status, self.to_string()).into_response()
    }
}

pub type AppResult<T> = Result<T, AppError>;
