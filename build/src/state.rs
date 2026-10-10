//! [`AppState`]: the process-wide state every handler gets (config, DB pool, RustiCal stores, caches, guards).
//!
//! The caches and their keys are tabled in the crate docs, section "Caches".

use crate::config::Config;
use crate::i18n::I18n;
use rustical_dav_push::VapidKeypair;
use rustical_store_sqlite::{SqliteAddressbookStore, SqliteCalendarStore, SqlitePrincipalStore};
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::broadcast;

/// A change in a collection, fanned out to SSE clients.
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
    /// Development only (`config.dev_assets_dir`): fired when an asset file changes → `/dev/reload`.
    pub dev_reload: broadcast::Sender<()>,
    pub http: reqwest::Client,
    /// Separate from `http`: a remote DAV server needs a 60 s request timeout (a batched multiget
    /// does not fit in push's 10 s) and must not follow redirects.
    pub remote_http: reqwest::Client,
    /// Logins a user started but has not finished. In process on purpose: an app password reaches
    /// the whole Nextcloud account, so a one-off import must not put one at rest.
    pub remote_logins: Arc<crate::domain::remote::login::Logins>,
    /// Per-link round locks and the echo filter of the sync loop.
    pub remote_runner: Arc<crate::domain::remote::sched::Runner>,
    /// geocoder proxy: throttle + cache (`domain::geo`)
    pub geo: crate::domain::geo::Geo,
    /// the top-bar search's per-user corpus (`domain::search`)
    pub search_index: crate::domain::search::IndexCache,
    /// parsed tasks per calendar and synctoken (`domain::tasks::tasks_for`)
    pub task_cache: crate::domain::tasks::TaskCache,
    /// derived data between requests, each keyed by synctoken (and zone/year/day/language where it depends on them)
    pub caches: Caches,
    /// the change history's lock between UI writes and the DAV poll (`domain::history`)
    pub history: crate::domain::history::Hist,
    pub vapid: Option<VapidKeypair>,
    pub started_at: chrono::DateTime<chrono::Utc>,
    /// the HTTP layer's process-wide state (shutdown signal, write locks, login throttle)
    pub guards: Guards,
}

pub type SharedState = Arc<AppState>;

/// Request results kept between requests (see each type); all synctoken-keyed, so a write is never
/// answered from before it.
#[derive(Default)]
pub struct Caches {
    pub instances: crate::domain::events::InstanceCache,
    pub density: crate::domain::events::DensityCache,
    pub obj_meta: crate::domain::events::ObjMetaCache,
    pub contacts: crate::domain::contacts::ContactCache,
    pub mentions: crate::routes::api::MentionCache,
    pub holidays: crate::domain::holidays::MemCache,
    /// a year of instances per calendar for the five year views (`routes::more::YearCache`)
    pub years: crate::routes::more::YearCache,
    /// the history document's changes and elements per user (`routes::history::HistoryCache`)
    pub history: crate::routes::history::HistoryCache,
}

/// Process-wide state of the HTTP layer; every part starts empty.
#[derive(Default)]
pub struct Guards {
    /// `true` once the server stops (SIGTERM / Ctrl-C): open streams (SSE) end, so the graceful
    /// shutdown is not held up by connections that never finish on their own
    pub shutdown: tokio::sync::watch::Sender<bool>,
    /// one lock per user around the read-modify-write of `practical_user_settings` (`prefs::get` → change
    /// a field → `prefs::set` writes the whole row): two saves at once otherwise lose one of them
    pub prefs: std::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<tokio::sync::Mutex<()>>>>,
    /// failed password logins (`auth::LoginThrottle`)
    pub login: crate::auth::LoginThrottle,
}

impl Guards {
    /// The user's settings lock; hold the guard from `prefs::get` until after `prefs::set`.
    pub async fn prefs(&self, user: &str) -> tokio::sync::OwnedMutexGuard<()> {
        let m = self.prefs.lock().unwrap().entry(user.to_string()).or_default().clone();
        m.lock_owned().await
    }

    /// Resolves once the server is stopping.
    pub fn stopping(&self) -> impl std::future::Future<Output = ()> + Send + use<> {
        let mut rx = self.shutdown.subscribe();
        async move {
            let _ = rx.wait_for(|v| *v).await;
        }
    }
}
