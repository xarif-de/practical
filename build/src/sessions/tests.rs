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
