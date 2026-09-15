use crate::config::Config;
use crate::i18n::I18n;
use rustical_dav_push::VapidKeypair;
use rustical_store_sqlite::{SqliteAddressbookStore, SqliteCalendarStore, SqlitePrincipalStore};
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::broadcast;

/// A change in a collection, fanned out to SSE clients (plan NFR "Live-Updates").
#[derive(Debug, Clone, serde::Serialize)]
pub struct ChangeEvent {
    pub kind: &'static str, // "calendar" | "addressbook"
    pub principal: String,
    pub id: String,
    pub synctoken: i64,
}

pub struct AppState {
    pub config: Config,
    pub db: SqlitePool,
    pub principals: Arc<SqlitePrincipalStore>,
    pub calendars: Arc<SqliteCalendarStore>,
    pub addressbooks: Arc<SqliteAddressbookStore>,
    pub i18n: Arc<I18n>,
    pub changes: broadcast::Sender<ChangeEvent>,
    pub http: reqwest::Client,
    pub vapid: Option<VapidKeypair>,
    pub started_at: chrono::DateTime<chrono::Utc>,
}

pub type SharedState = Arc<AppState>;
