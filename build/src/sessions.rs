//! The in-memory session store, with expired records dropped. tower-sessions' `MemoryStore` only
//! hides an expired record on `load` and never removes it, so every session ever started — each
//! anonymous visit of /login (bots, scanners, a browser that never logs in) makes one for its CSRF
//! token — stayed in memory until the process ended. Sweeping rides on writes, at most once per
//! `SWEEP_EVERY`: no timer, nothing wakes up while nobody uses the app.
//!
//! Also the session's inactivity expiry (`activity`): every request that renders a page context
//! (`PageCtx`) counts as use and moves the expiry to now + `session_hours`. tower-sessions moves it
//! only when it saves a modified session, which used to happen as a side effect of the unconditional
//! `remove(flash)` on every page view; now the flash is removed only when there is one, and the
//! extension is an explicit expiry-only save (`TOUCH`) at most once per `REFRESH_AFTER`.
//! A prefetch (`is_prefetch`) is answered only while the session holds no one-shot value and
//! never changes the session's data.

use async_trait::async_trait;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tower_sessions::Session;
use tower_sessions::cookie::time::{Duration, OffsetDateTime};
use tower_sessions::session::{Id, Record};
use tower_sessions::session_store::{self, SessionStore};

const SWEEP_EVERY: Duration = Duration::minutes(10);
/// An extension is written once the last write is this old: the expiry lags the last page view by
/// less than this (12 h window: 0.14 %), and a burst of page/fragment/prefetch requests costs one
/// store write and one Set-Cookie instead of one per request.
pub const REFRESH_AFTER: Duration = Duration::seconds(60);
/// Data key of an expiry-only save: `save` moves the stored record's expiry and keeps its data, so an
/// extension never overwrites what a concurrent request wrote (a flash, a login, a logout).
const TOUCH: &str = "__touch";
/// Session values shown once and then removed by the page that shows them: a prefetch must not
/// consume them, so it is refused while one is pending.
const ONE_SHOT: [&str; 2] = [crate::ctx::SESSION_FLASH, crate::routes::settings::SESSION_NEW_TOKEN];

#[derive(Clone, Debug, Default)]
pub struct MemoryStore(Arc<Mutex<Records>>);

#[derive(Debug, Default)]
struct Records {
    by_id: HashMap<Id, Record>,
    swept: Option<OffsetDateTime>,
}

impl MemoryStore {
    /// How long the stored record `id` has left; `None` when there is none (expired or deleted).
    fn remaining(&self, id: &Id, now: OffsetDateTime) -> Option<Duration> {
        self.0.lock().unwrap().by_id.get(id).map(|r| r.expiry_date - now).filter(|d| d.is_positive())
    }

    /// Test clock: the time moves on by `d` for every stored record (each comparison is a stored
    /// instant against now, and every new expiry is now + lifetime, so this equals now + `d`).
    #[cfg(test)]
    pub fn pass(&self, d: Duration) {
        let mut g = self.0.lock().unwrap();
        g.by_id.values_mut().for_each(|r| r.expiry_date -= d);
        g.swept = g.swept.map(|t| t - d);
    }

    #[cfg(test)]
    pub fn expiry(&self, id: &Id) -> Option<OffsetDateTime> {
        self.0.lock().unwrap().by_id.get(id).map(|r| r.expiry_date)
    }

    #[cfg(test)]
    pub fn data(&self, id: &Id) -> Option<HashMap<String, serde_json::Value>> {
        self.0.lock().unwrap().by_id.get(id).map(|r| r.data.clone())
    }

    #[cfg(test)]
    pub fn ids(&self) -> Vec<Id> {
        self.0.lock().unwrap().by_id.keys().copied().collect()
    }
}

impl Records {
    fn sweep(&mut self, now: OffsetDateTime) {
        if self.swept.is_some_and(|t| now - t < SWEEP_EVERY) {
            return;
        }
        self.by_id.retain(|_, r| r.expiry_date > now);
        self.swept = Some(now);
    }
}

#[async_trait]
impl SessionStore for MemoryStore {
    async fn create(&self, record: &mut Record) -> session_store::Result<()> {
        let mut g = self.0.lock().unwrap();
        g.sweep(OffsetDateTime::now_utc());
        // session id collision: a new id, as tower-sessions' own store does
        while g.by_id.contains_key(&record.id) {
            record.id = Id::default();
        }
        record.data.remove(TOUCH);
        g.by_id.insert(record.id, record.clone());
        Ok(())
    }

    async fn save(&self, record: &Record) -> session_store::Result<()> {
        let mut g = self.0.lock().unwrap();
        let now = OffsetDateTime::now_utc();
        g.sweep(now);
        if record.data.contains_key(TOUCH) {
            // an extension: the live record's expiry moves, nothing else; an expired or deleted one stays gone
            if let Some(r) = g.by_id.get_mut(&record.id).filter(|r| r.expiry_date > now) {
                r.expiry_date = r.expiry_date.max(record.expiry_date);
            }
            return Ok(());
        }
        g.by_id.insert(record.id, record.clone());
        Ok(())
    }

    async fn load(&self, session_id: &Id) -> session_store::Result<Option<Record>> {
        let now = OffsetDateTime::now_utc();
        Ok(self.0.lock().unwrap().by_id.get(session_id).filter(|r| r.expiry_date > now).cloned())
    }

    async fn delete(&self, session_id: &Id) -> session_store::Result<()> {
        self.0.lock().unwrap().by_id.remove(session_id);
        Ok(())
    }
}

/// A request for later use (a browser prefetch sends `Sec-Purpose: prefetch`, app.js `Purpose: prefetch`).
pub fn is_prefetch(headers: &HeaderMap) -> bool {
    ["sec-purpose", "purpose"].iter().any(|h| headers.get(*h).and_then(|v| v.to_str().ok()).is_some_and(|v| v.trim_start().starts_with("prefetch")))
}

/// `activity`'s state: the store (to read the stored expiry) and the inactivity window.
#[derive(Clone)]
pub struct Activity {
    store: MemoryStore,
    lifetime: Duration,
}

impl Activity {
    pub fn new(store: MemoryStore, lifetime: Duration) -> Self {
        Self { store, lifetime }
    }
}

/// Request extension: `PageCtx` marks the request as use of the session.
#[derive(Clone, Default)]
pub struct Used(Arc<AtomicBool>);

impl Used {
    pub fn mark(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// Middleware right inside the session layer: refuses a prefetch that could consume a one-shot value
/// or write the session, and extends a used session (see the module comment).
pub async fn activity(State(a): State<Activity>, mut req: Request, next: Next) -> Response {
    let Some(session) = req.extensions().get::<Session>().cloned() else { return next.run(req).await };
    let prefetch = is_prefetch(req.headers());
    if prefetch {
        if req.method() != Method::GET {
            return (StatusCode::BAD_REQUEST, "a prefetch is a GET").into_response();
        }
        // loads the record this request works on: the page context reads the same one
        for key in ONE_SHOT {
            if session.get_value(key).await.ok().flatten().is_some() {
                return (StatusCode::NO_CONTENT, [(header::CACHE_CONTROL, "no-store")]).into_response();
            }
        }
    }
    let used = Used::default();
    req.extensions_mut().insert(used.clone());
    let res = next.run(req).await;
    if prefetch && session.is_modified() {
        // the session layer saves nothing after a server error: the change is dropped with the answer
        tracing::warn!("a prefetch tried to change the session; answered 503");
        return (StatusCode::SERVICE_UNAVAILABLE, [(header::CACHE_CONTROL, "no-store")]).into_response();
    }
    if used.0.load(Ordering::Relaxed)
        && !session.is_modified()
        && let Some(id) = session.id()
        && a.store.remaining(&id, OffsetDateTime::now_utc()).is_some_and(|left| left < a.lifetime - REFRESH_AFTER)
    {
        let _ = session.insert(TOUCH, true).await;
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(expiry_date: OffsetDateTime) -> Record {
        Record { id: Id::default(), data: Default::default(), expiry_date }
    }

    #[tokio::test]
    async fn expired_records_go_on_the_next_sweep() {
        let store = MemoryStore::default();
        let now = OffsetDateTime::now_utc();
        let (mut live, mut dead) = (record(now + Duration::hours(1)), record(now - Duration::seconds(1)));
        store.create(&mut live).await.unwrap();
        store.create(&mut dead).await.unwrap();
        // an expired record is never handed out, swept or not
        assert_eq!(store.load(&dead.id).await.unwrap(), None);
        assert_eq!(store.load(&live.id).await.unwrap(), Some(live.clone()));
        assert_eq!(store.0.lock().unwrap().by_id.len(), 2, "the first create swept before the second existed");
        // within SWEEP_EVERY nothing is swept; after it, the expired one goes and the live one stays
        store.0.lock().unwrap().sweep(now + Duration::minutes(5));
        assert_eq!(store.0.lock().unwrap().by_id.len(), 2);
        store.0.lock().unwrap().sweep(now + Duration::minutes(11));
        assert_eq!(store.0.lock().unwrap().by_id.keys().collect::<Vec<_>>(), [&live.id]);
        store.save(&live).await.unwrap();
        store.delete(&live.id).await.unwrap();
        assert_eq!(store.load(&live.id).await.unwrap(), None);
    }

    /// An extension saved after a concurrent write (here: a flash set after this request loaded the
    /// record) keeps that write; one for a record deleted meanwhile (a logout) does not bring it back.
    #[tokio::test]
    async fn an_expiry_only_save_never_overwrites_or_resurrects() {
        let store = MemoryStore::default();
        let now = OffsetDateTime::now_utc();
        let mut r = record(now + Duration::hours(1));
        r.data.insert("user".into(), "alice".into());
        store.create(&mut r).await.unwrap();
        let mut loaded = store.load(&r.id).await.unwrap().unwrap();
        let mut flashed = r.clone();
        flashed.data.insert("flash".into(), serde_json::json!({ "kind": "ok", "text": "saved" }));
        store.save(&flashed).await.unwrap();
        loaded.data.insert(TOUCH.into(), true.into());
        loaded.expiry_date = now + Duration::hours(12);
        store.save(&loaded).await.unwrap();
        let stored = store.load(&r.id).await.unwrap().unwrap();
        assert_eq!((stored.data, stored.expiry_date), (flashed.data.clone(), now + Duration::hours(12)));
        store.delete(&r.id).await.unwrap();
        store.save(&loaded).await.unwrap();
        assert_eq!(store.load(&r.id).await.unwrap(), None);
        // a full save still replaces, and a record never stores the marker
        let mut fresh = record(now + Duration::hours(1));
        fresh.data.insert(TOUCH.into(), true.into());
        store.create(&mut fresh).await.unwrap();
        assert!(!store.load(&fresh.id).await.unwrap().unwrap().data.contains_key(TOUCH));
    }

    #[test]
    fn prefetch_headers() {
        let h = |k: &'static str, v: &'static str| HeaderMap::from_iter([(axum::http::HeaderName::from_static(k), axum::http::HeaderValue::from_static(v))]);
        assert!(is_prefetch(&h("sec-purpose", "prefetch")) && is_prefetch(&h("sec-purpose", "prefetch;prerender")) && is_prefetch(&h("purpose", "prefetch")));
        assert!(!is_prefetch(&h("purpose", "other")) && !is_prefetch(&HeaderMap::new()));
    }

    /// Probes, assets and an anonymous page request start no session (no record, no cookie); the
    /// login page does — its form needs the CSRF token.
    #[tokio::test]
    async fn only_the_login_page_starts_an_anonymous_session() {
        let app = crate::testkit::TestApp::new().await;
        for path in ["/healthz", "/assets/app.js", "/assets/icon.svg", "/calendar", "/api/changes"] {
            let r = app.anonymous().get(path).await;
            assert_eq!(r.header("set-cookie"), None, "{path} → {}", r.status);
        }
        assert!(app.anonymous().get("/login").await.header("set-cookie").is_some_and(|c| c.starts_with("practical_session=")));
    }
}
