//! The in-memory session store, with expired records dropped. tower-sessions' `MemoryStore` only
//! hides an expired record on `load` and never removes it, so every session ever started — each
//! anonymous visit of /login (bots, scanners, a browser that never logs in) makes one for its CSRF
//! token — stayed in memory until the process ended. Sweeping rides on writes, at most once per
//! `SWEEP_EVERY`: no timer, nothing wakes up while nobody uses the app.

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tower_sessions::cookie::time::{Duration, OffsetDateTime};
use tower_sessions::session::{Id, Record};
use tower_sessions::session_store::{self, SessionStore};

const SWEEP_EVERY: Duration = Duration::minutes(10);

#[derive(Clone, Debug, Default)]
pub struct MemoryStore(Arc<Mutex<Records>>);

#[derive(Debug, Default)]
struct Records {
    by_id: HashMap<Id, Record>,
    swept: Option<OffsetDateTime>,
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
        g.by_id.insert(record.id, record.clone());
        Ok(())
    }

    async fn save(&self, record: &Record) -> session_store::Result<()> {
        let mut g = self.0.lock().unwrap();
        g.sweep(OffsetDateTime::now_utc());
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
