//! The session store: SQLite (`practical_sessions`, migration 14) behind a write-through memory cache of the
//! logged-in sessions. A deploy, an OOM kill or a node reboot no longer logs everybody out (before: an
//! in-memory store), and a request reads its session from memory as before — the database is read only for a
//! session this process has not seen yet (the first request after a restart) and for the short anonymous ones.
//!
//! Stored: the SHA-256 of the session id (a copy of the database — the backup sidecar's — holds no usable
//! cookie), the data as JSON, the expiry in Unix seconds. Expired rows go on the hourly purge
//! (`tasks::spawn_trash_purger` → `purge`); the cache drops expired records on writes, at most once per
//! `SWEEP_EVERY`: no timer, nothing wakes up while nobody uses the app.
//!
//! No session for anonymous requests: the login form's CSRF token is a cookie of its own (`auth::LOGIN_CSRF`),
//! so only a login creates a record — and the OIDC start (POST /login/oidc keeps its state in the session):
//! such a record without a user lives at most `ANON_TTL` and is never cached in memory.
//!
//! Also the session's inactivity expiry (`activity`): every request that renders a page context
//! (`PageCtx`) counts as use and moves the expiry to now + `session_hours`. tower-sessions moves it
//! only when it saves a modified session, which used to happen as a side effect of the unconditional
//! `remove(flash)` on every page view; now the flash is removed only when there is one, and the
//! extension is an explicit expiry-only save (`TOUCH`) at most once per `REFRESH_AFTER` — one UPDATE per
//! active session and minute at most.
//! A prefetch (`is_prefetch`) is answered only while the session holds no one-shot value and
//! never changes the session's data.

use async_trait::async_trait;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use sqlx::{Row, SqlitePool};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
/// A record without a user (the OIDC round trip's state) lives this long at most, whatever the cookie says.
pub const ANON_TTL: Duration = Duration::minutes(30);
/// Data key of an expiry-only save: `save` moves the stored record's expiry and keeps its data, so an
/// extension never overwrites what a concurrent request wrote (a flash, a login, a logout).
const TOUCH: &str = "__touch";
/// Session values shown once and then removed by the page that shows them: a prefetch must not
/// consume them, so it is refused while one is pending.
const ONE_SHOT: [&str; 2] = [crate::ctx::SESSION_FLASH, crate::routes::settings::SESSION_NEW_TOKEN];

#[derive(Clone, Debug)]
pub struct DbStore {
    db: SqlitePool,
    cache: Arc<Mutex<Records>>,
    /// database writes so far (create/save/delete statements): the write rate is a stated property
    writes: Arc<AtomicU64>,
}

#[derive(Debug, Default)]
struct Records {
    by_id: HashMap<Id, Record>,
    swept: Option<OffsetDateTime>,
    /// test clock (`pass`): not yet applied to the database rows
    #[cfg(test)]
    shift: Duration,
}

/// The row key: no cookie value is stored.
fn key(id: &Id) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(id.0.to_le_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

fn has_user(r: &Record) -> bool {
    r.data.contains_key(crate::ctx::SESSION_USER)
}

fn store_err(e: impl std::fmt::Display) -> session_store::Error {
    session_store::Error::Backend(e.to_string())
}

impl DbStore {
    pub fn new(db: SqlitePool) -> Self {
        Self { db, cache: Default::default(), writes: Default::default() }
    }

    /// How long the cached record `id` has left; `None` when there is none (expired, deleted, anonymous).
    fn remaining(&self, id: &Id, now: OffsetDateTime) -> Option<Duration> {
        self.cache.lock().unwrap().by_id.get(id).map(|r| r.expiry_date - now).filter(|d| d.is_positive())
    }

    #[cfg(test)]
    pub fn writes(&self) -> u64 {
        self.writes.load(Ordering::Relaxed)
    }

    /// The expiry written to the database: a record without a user lives `ANON_TTL` at most.
    fn stored_expiry(r: &Record, now: OffsetDateTime) -> i64 {
        let e = if has_user(r) { r.expiry_date } else { r.expiry_date.min(now + ANON_TTL) };
        e.unix_timestamp()
    }

    async fn write(&self, r: &Record, now: OffsetDateTime) -> session_store::Result<()> {
        self.apply_shift().await?;
        let mut data = r.data.clone();
        data.remove(TOUCH);
        sqlx::query("INSERT INTO practical_sessions (id, data, expiry) VALUES (?, ?, ?) ON CONFLICT(id) DO UPDATE SET data = excluded.data, expiry = excluded.expiry")
            .bind(key(&r.id))
            .bind(serde_json::to_string(&data).map_err(store_err)?)
            .bind(Self::stored_expiry(r, now))
            .execute(&self.db)
            .await
            .map_err(store_err)?;
        self.writes.fetch_add(1, Ordering::Relaxed);
        let mut g = self.cache.lock().unwrap();
        g.sweep(now);
        if has_user(r) {
            g.by_id.insert(r.id, Record { data, ..r.clone() });
        } else {
            g.by_id.remove(&r.id);
        }
        Ok(())
    }

    #[cfg(test)]
    async fn apply_shift(&self) -> session_store::Result<()> {
        let d = std::mem::take(&mut self.cache.lock().unwrap().shift);
        if d.is_zero() {
            return Ok(());
        }
        sqlx::query("UPDATE practical_sessions SET expiry = expiry - ?").bind(d.whole_seconds()).execute(&self.db).await.map_err(store_err)?;
        Ok(())
    }

    #[cfg(not(test))]
    async fn apply_shift(&self) -> session_store::Result<()> {
        Ok(())
    }

    /// Test clock: the time moves on by `d` for every stored record (each comparison is a stored
    /// instant against now, and every new expiry is now + lifetime, so this equals now + `d`).
    #[cfg(test)]
    pub fn pass(&self, d: Duration) {
        let mut g = self.cache.lock().unwrap();
        g.by_id.values_mut().for_each(|r| r.expiry_date -= d);
        g.swept = g.swept.map(|t| t - d);
        g.shift += d;
    }

    /// Test: a process restart — the memory is gone, the database stays.
    #[cfg(test)]
    pub fn forget(&self) {
        let mut g = self.cache.lock().unwrap();
        g.by_id.clear();
        g.swept = None;
    }

    #[cfg(test)]
    pub fn expiry(&self, id: &Id) -> Option<OffsetDateTime> {
        self.cache.lock().unwrap().by_id.get(id).map(|r| r.expiry_date)
    }

    #[cfg(test)]
    pub fn data(&self, id: &Id) -> Option<HashMap<String, serde_json::Value>> {
        self.cache.lock().unwrap().by_id.get(id).map(|r| r.data.clone())
    }

    #[cfg(test)]
    pub fn ids(&self) -> Vec<Id> {
        self.cache.lock().unwrap().by_id.keys().copied().collect()
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

/// The hourly purge of expired rows; the number removed.
pub async fn purge(db: &SqlitePool) -> Result<u64, sqlx::Error> {
    Ok(sqlx::query("DELETE FROM practical_sessions WHERE expiry <= ?").bind(OffsetDateTime::now_utc().unix_timestamp()).execute(db).await?.rows_affected())
}

#[async_trait]
impl SessionStore for DbStore {
    async fn create(&self, record: &mut Record) -> session_store::Result<()> {
        let now = OffsetDateTime::now_utc();
        self.apply_shift().await?;
        record.data.remove(TOUCH);
        // a new id until it is free, as tower-sessions' own store does (2^-128 per try)
        loop {
            let cached = self.cache.lock().unwrap().by_id.contains_key(&record.id);
            let taken = cached || sqlx::query("SELECT 1 FROM practical_sessions WHERE id = ?").bind(key(&record.id)).fetch_optional(&self.db).await.map_err(store_err)?.is_some();
            if !taken {
                break;
            }
            record.id = Id::default();
        }
        self.write(record, now).await
    }

    async fn save(&self, record: &Record) -> session_store::Result<()> {
        let now = OffsetDateTime::now_utc();
        if record.data.contains_key(TOUCH) {
            // an extension: the live record's expiry moves, nothing else; an expired or deleted one stays gone
            self.apply_shift().await?;
            sqlx::query("UPDATE practical_sessions SET expiry = max(expiry, ?) WHERE id = ? AND expiry > ?")
                .bind(Self::stored_expiry(record, now))
                .bind(key(&record.id))
                .bind(now.unix_timestamp())
                .execute(&self.db)
                .await
                .map_err(store_err)?;
            self.writes.fetch_add(1, Ordering::Relaxed);
            let mut g = self.cache.lock().unwrap();
            g.sweep(now);
            if let Some(r) = g.by_id.get_mut(&record.id).filter(|r| r.expiry_date > now) {
                r.expiry_date = r.expiry_date.max(record.expiry_date);
            }
            return Ok(());
        }
        self.write(record, now).await
    }

    async fn load(&self, session_id: &Id) -> session_store::Result<Option<Record>> {
        let now = OffsetDateTime::now_utc();
        if let Some(r) = self.cache.lock().unwrap().by_id.get(session_id) {
            return Ok(Some(r.clone()).filter(|r| r.expiry_date > now));
        }
        self.apply_shift().await?;
        let Some(row) = sqlx::query("SELECT data, expiry FROM practical_sessions WHERE id = ? AND expiry > ?")
            .bind(key(session_id))
            .bind(now.unix_timestamp())
            .fetch_optional(&self.db)
            .await
            .map_err(store_err)?
        else {
            return Ok(None);
        };
        let data: HashMap<String, serde_json::Value> = serde_json::from_str(row.get::<&str, _>("data")).map_err(store_err)?;
        let expiry_date = OffsetDateTime::from_unix_timestamp(row.get("expiry")).map_err(store_err)?;
        let r = Record { id: *session_id, data, expiry_date };
        if has_user(&r) {
            self.cache.lock().unwrap().by_id.insert(r.id, r.clone());
        }
        Ok(Some(r))
    }

    async fn delete(&self, session_id: &Id) -> session_store::Result<()> {
        self.cache.lock().unwrap().by_id.remove(session_id);
        self.apply_shift().await?;
        sqlx::query("DELETE FROM practical_sessions WHERE id = ?").bind(key(session_id)).execute(&self.db).await.map_err(store_err)?;
        self.writes.fetch_add(1, Ordering::Relaxed);
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
    store: DbStore,
    lifetime: Duration,
}

impl Activity {
    pub fn new(store: DbStore, lifetime: Duration) -> Self {
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
        let mut r = Record { id: Id::default(), data: Default::default(), expiry_date };
        r.data.insert(crate::ctx::SESSION_USER.into(), "alice".into());
        r
    }

    async fn rows(db: &SqlitePool) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM practical_sessions").fetch_one(db).await.unwrap()
    }

    #[tokio::test]
    async fn expired_records_go_on_the_next_sweep_and_the_purge() {
        let app = crate::testkit::TestApp::new().await;
        let store = DbStore::new(app.state.db.clone());
        let now = OffsetDateTime::now_utc();
        let (mut live, mut dead) = (record(now + Duration::hours(1)), record(now - Duration::seconds(1)));
        store.create(&mut live).await.unwrap();
        store.create(&mut dead).await.unwrap();
        // an expired record is never handed out, swept or not, from memory or from the database
        assert_eq!(store.load(&dead.id).await.unwrap(), None);
        assert_eq!(store.load(&live.id).await.unwrap(), Some(live.clone()));
        assert_eq!(store.cache.lock().unwrap().by_id.len(), 2, "the first create swept before the second existed");
        store.forget();
        assert_eq!(store.load(&dead.id).await.unwrap(), None);
        // within SWEEP_EVERY nothing is swept; after it, the expired one goes and the live one stays
        store.create(&mut dead).await.unwrap(); // its id is taken (the row is still there): a new one
        store.cache.lock().unwrap().sweep(now + Duration::minutes(5));
        assert_eq!(store.cache.lock().unwrap().by_id.len(), 1, "swept at the create");
        assert_eq!(rows(&app.state.db).await, 3);
        assert_eq!(purge(&app.state.db).await.unwrap(), 2);
        assert_eq!(rows(&app.state.db).await, 1);
        store.save(&live).await.unwrap();
        store.delete(&live.id).await.unwrap();
        assert_eq!(store.load(&live.id).await.unwrap(), None);
        assert_eq!(rows(&app.state.db).await, 0);
    }

    /// An extension saved after a concurrent write (here: a flash set after this request loaded the
    /// record) keeps that write; one for a record deleted meanwhile (a logout) does not bring it back.
    #[tokio::test]
    async fn an_expiry_only_save_never_overwrites_or_resurrects() {
        let app = crate::testkit::TestApp::new().await;
        let store = DbStore::new(app.state.db.clone());
        let now = OffsetDateTime::now_utc();
        let mut r = record(now + Duration::hours(1));
        store.create(&mut r).await.unwrap();
        let mut loaded = store.load(&r.id).await.unwrap().unwrap();
        let mut flashed = r.clone();
        flashed.data.insert("flash".into(), serde_json::json!({ "kind": "ok", "text": "saved" }));
        store.save(&flashed).await.unwrap();
        loaded.data.insert(TOUCH.into(), true.into());
        loaded.expiry_date = now + Duration::hours(12);
        store.save(&loaded).await.unwrap();
        let stored = store.load(&r.id).await.unwrap().unwrap();
        assert_eq!((stored.data.clone(), stored.expiry_date), (flashed.data.clone(), now + Duration::hours(12)));
        // the same from the database (a restart): data and expiry (whole seconds there)
        store.forget();
        let stored = store.load(&r.id).await.unwrap().unwrap();
        assert_eq!((stored.data, stored.expiry_date.unix_timestamp()), (flashed.data.clone(), (now + Duration::hours(12)).unix_timestamp()));
        store.delete(&r.id).await.unwrap();
        store.save(&loaded).await.unwrap();
        assert_eq!(store.load(&r.id).await.unwrap(), None);
        // a full save still replaces, and a record never stores the marker
        let mut fresh = record(now + Duration::hours(1));
        fresh.data.insert(TOUCH.into(), true.into());
        store.create(&mut fresh).await.unwrap();
        store.forget();
        assert!(!store.load(&fresh.id).await.unwrap().unwrap().data.contains_key(TOUCH));
    }

    /// A record without a user (the OIDC round trip) is kept for ANON_TTL at most and never in memory; the
    /// database holds no cookie value.
    #[tokio::test]
    async fn anonymous_records_are_short_lived_and_ids_are_stored_hashed() {
        let app = crate::testkit::TestApp::new().await;
        let store = DbStore::new(app.state.db.clone());
        let now = OffsetDateTime::now_utc();
        let mut anon = Record { id: Id::default(), data: [("oidc".to_string(), serde_json::json!("state"))].into(), expiry_date: now + Duration::hours(12) };
        store.create(&mut anon).await.unwrap();
        assert!(store.ids().is_empty(), "not cached");
        let expiry: i64 = sqlx::query_scalar("SELECT expiry FROM practical_sessions").fetch_one(&app.state.db).await.unwrap();
        assert!((expiry - (now + ANON_TTL).unix_timestamp()).abs() <= 1, "{expiry}");
        assert!(store.load(&anon.id).await.unwrap().is_some());
        let stored: String = sqlx::query_scalar("SELECT id FROM practical_sessions").fetch_one(&app.state.db).await.unwrap();
        assert_eq!(stored.len(), 64);
        assert!(!stored.contains(&anon.id.to_string()));
    }

    #[test]
    fn prefetch_headers() {
        let h = |k: &'static str, v: &'static str| HeaderMap::from_iter([(axum::http::HeaderName::from_static(k), axum::http::HeaderValue::from_static(v))]);
        assert!(is_prefetch(&h("sec-purpose", "prefetch")) && is_prefetch(&h("sec-purpose", "prefetch;prerender")) && is_prefetch(&h("purpose", "prefetch")));
        assert!(!is_prefetch(&h("purpose", "other")) && !is_prefetch(&HeaderMap::new()));
    }

    /// Fails on the code before (the login page started a session for its CSRF token): no request
    /// without a login starts a session — probes, assets, pages, streams, the login page itself.
    #[tokio::test]
    async fn no_anonymous_request_starts_a_session() {
        let app = crate::testkit::TestApp::new().await;
        for path in ["/healthz", "/assets/app.js", "/assets/icon.svg", "/calendar", "/api/changes", "/login", "/login?manual=1", "/"] {
            let r = app.anonymous().get(path).await;
            assert!(!r.header("set-cookie").is_some_and(|c| c.starts_with("practical_session=")), "{path} → {}", r.status);
        }
        // the login page's token is its own cookie, on /login only
        let c = app.anonymous().get("/login").await.header("set-cookie").unwrap();
        assert!(c.starts_with("practical_login_csrf=") && c.contains("Path=/login") && c.contains("HttpOnly") && c.contains("SameSite=Strict"), "{c}");
        assert_eq!(rows(&app.state.db).await, 0);
    }
}
