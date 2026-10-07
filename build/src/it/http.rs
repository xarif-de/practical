//! The HTTP layer's own guarantees (w3/http): what an error answer may say, the database-busy answer,
//! panics, caching headers, the CSRF exemption of app tokens, the input gate and the write preconditions.

use super::seed::seed;
use crate::testkit::TestApp;
use axum::body::Body;
use axum::http::{Method, StatusCode};
use axum::response::IntoResponse;
use serde_json::json;

/// Fails on the code before: a server error answered with its own text ("database error: …", a path, an
/// upstream URL) — which every client without a browser session read verbatim.
#[tokio::test]
async fn a_server_error_answers_the_generic_message_and_logs_the_rest() {
    let r = crate::error::AppError::Internal("open /var/lib/secret.sqlite3: no such file".into()).into_response();
    assert_eq!(r.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = String::from_utf8(axum::body::to_bytes(r.into_body(), 1 << 16).await.unwrap().to_vec()).unwrap();
    assert_eq!(body, "That did not work – the details are in the server log.");
    // a client error keeps its own (non-internal) text: what app-token clients have always read
    let r = crate::error::AppError::Forbidden.into_response();
    assert_eq!(String::from_utf8(axum::body::to_bytes(r.into_body(), 1 << 16).await.unwrap().to_vec()).unwrap(), "forbidden");
}

/// Fails on the code before: /healthz answered `db: <sqlx error text>` to anyone.
#[tokio::test]
async fn healthz_names_no_internals() {
    let app = TestApp::new().await;
    assert_eq!(app.anonymous().get("/healthz").await.body, "ok");
    app.state.db.close().await;
    let r = app.anonymous().get("/healthz").await;
    assert_eq!((r.status, r.body.as_str()), (StatusCode::SERVICE_UNAVAILABLE, "db unavailable"));
}

/// Fails on the code before: a write waiting longer than the busy timeout (another process holds the
/// write lock — an import, the backup sidecar, RustiCal) answered a generic 500.
#[tokio::test]
async fn a_locked_database_answers_503_with_retry_after_in_the_users_language() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    // another connection (as another process would) takes the write lock and keeps it
    let path = s.app.state.config.db_path.clone();
    let mut other = <sqlx::SqliteConnection as sqlx::Connection>::connect(&format!("sqlite://{path}")).await.unwrap();
    sqlx::query("BEGIN IMMEDIATE").execute(&mut other).await.unwrap();
    let r = c.json(Method::PUT, "/api/alarm-presets", json!([])).await;
    sqlx::query("ROLLBACK").execute(&mut other).await.unwrap();
    assert_eq!(r.status, StatusCode::SERVICE_UNAVAILABLE, "{}", r.body);
    assert_eq!(r.header("retry-after").as_deref(), Some("2"));
    assert_eq!(r.body, "Der Server ist gerade beschäftigt – bitte gleich noch einmal versuchen.");
    // free again: the same write succeeds
    c.json(Method::PUT, "/api/alarm-presets", json!([])).await.ok();
}

/// Fails on the code before: a panicking handler dropped the connection (no answer at all). Here the
/// OIDC callback's own `assert_eq!(iss, issuer)` (rustical_oidc routes.rs) panics on a foreign issuer.
#[tokio::test]
async fn a_panicking_handler_answers_500_without_internals() {
    let app = TestApp::with(|c| {
        c.oidc = Some(crate::config::OidcSettings { name: "SSO".into(), issuer: "https://auth.example.org".into(), client_id: "practical".into(), client_secret: None, scopes: vec!["openid".into()] });
    })
    .await;
    let r = app.anonymous().send(Method::GET, "/login/oidc/callback?code=x&state=y&iss=https://evil.example.org", &[("host", "localhost:4001")], Body::empty()).await;
    assert_eq!((r.status, r.body.as_str()), (StatusCode::INTERNAL_SERVER_ERROR, "That did not work – the details are in the server log."));
    // inside the protected routes the answer speaks the user's language, like every other error
    let router = axum::Router::new()
        .route("/boom", axum::routing::get(|| async { if true { panic!("secret detail") } }))
        .route_layer(axum::middleware::from_fn(crate::error::catch_panic));
    let mut c = crate::testkit::Client::for_router(router);
    let r = c.get("/boom").await;
    assert_eq!((r.status, r.body.contains("secret")), (StatusCode::INTERNAL_SERVER_ERROR, false));
}

/// Fails on the code before: the change stream never ended, so a graceful shutdown waited for it until
/// the pod's SIGKILL.
#[tokio::test]
async fn the_change_stream_ends_when_the_server_stops() {
    use tower::ServiceExt;
    let s = seed().await;
    let c = s.app.login("alice").await;
    let req = axum::http::Request::builder().uri("/api/changes").header("cookie", c.cookie_header()).body(Body::empty()).unwrap();
    let resp = s.app.router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = tokio::spawn(axum::body::to_bytes(resp.into_body(), 1 << 20));
    s.app.state.guards.shutdown.send_replace(true);
    let ended = tokio::time::timeout(std::time::Duration::from_secs(5), body).await;
    assert!(ended.is_ok_and(|r| r.unwrap().is_ok()), "the stream ended after the stop signal");
}

/// Fails on the code before (no check): a database whose practical_* schema differs from what the
/// migrations build, or that a newer binary migrated, is refused at start.
#[tokio::test]
async fn the_schema_check_refuses_a_foreign_schema() {
    let app = TestApp::new().await;
    let db = &app.state.db;
    let mut conn = db.acquire().await.unwrap();
    assert_eq!(crate::db::check_schema(db).await, Ok(vec![]), "a freshly migrated database passes");
    sqlx::raw_sql("ALTER TABLE practical_user_settings DROP COLUMN alarm_snaps_json").execute(&mut *conn).await.unwrap();
    let e = crate::db::check_schema(db).await.unwrap_err();
    assert!(e.contains("missing practical_user_settings.alarm_snaps_json"), "{e}");
    sqlx::raw_sql("ALTER TABLE practical_user_settings ADD COLUMN alarm_snaps_json INTEGER").execute(&mut *conn).await.unwrap();
    let e = crate::db::check_schema(db).await.unwrap_err();
    assert!(e.contains("practical_user_settings.alarm_snaps_json is INTEGER"), "{e}");
    sqlx::raw_sql("ALTER TABLE practical_user_settings DROP COLUMN alarm_snaps_json").execute(&mut *conn).await.unwrap();
    sqlx::raw_sql("ALTER TABLE practical_user_settings ADD COLUMN alarm_snaps_json TEXT").execute(&mut *conn).await.unwrap();
    assert_eq!(crate::db::check_schema(db).await, Ok(vec![]), "repaired");
    sqlx::raw_sql("CREATE TABLE practical_extra (x)").execute(&mut *conn).await.unwrap();
    assert!(crate::db::check_schema(db).await.unwrap_err().contains("unexpected practical_extra.x"));
    sqlx::raw_sql("DROP TABLE practical_extra").execute(&mut *conn).await.unwrap();
    // production's drift (2026-10-05 copy): category_rules keyed (principal, category) + a unique index —
    // reported, not refused
    sqlx::raw_sql("CREATE TABLE cr (principal TEXT NOT NULL, category TEXT NOT NULL, rule_json TEXT NOT NULL, updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, scope TEXT NOT NULL DEFAULT 'event', PRIMARY KEY (principal, category));
        DROP TABLE practical_category_rules; ALTER TABLE cr RENAME TO practical_category_rules;
        CREATE UNIQUE INDEX practical_category_rules_scope ON practical_category_rules (principal, scope, category);").execute(&mut *conn).await.unwrap();
    let drift = crate::db::check_schema(db).await.unwrap();
    assert!(drift.len() == 2 && drift[0].starts_with("practical_category_rules.category is TEXT notnull=1 default=None pk=2"), "{drift:?}");
    sqlx::query("INSERT INTO practical_migrations (version) VALUES (9999)").execute(&mut *conn).await.unwrap();
    assert!(crate::db::check_schema(db).await.unwrap_err().contains("has practical migration 9999"));
}

/// Fails on the code before (TESTED there: 200): any `Authorization: Basic …` header — even a wrong one —
/// skipped the origin guard AND the CSRF token while the cookie session stayed logged in.
#[tokio::test]
async fn an_invalid_basic_header_does_not_skip_csrf_for_a_cookie_session() {
    use base64ct::Encoding;
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let bad = format!("Basic {}", base64ct::Base64::encode_string(b"nobody:wrong"));
    let body = || Body::from(json!({ "principal": "alice", "cal_id": s.aufgaben, "summary": "csrf" }).to_string());
    let cross = [("content-type", "application/json"), ("authorization", bad.as_str()), ("origin", "https://evil.example"), ("sec-fetch-site", "cross-site")];
    let r = c.send(Method::POST, "/api/tasks", &cross, body()).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{}", r.body);
    // same-origin metadata but no CSRF token: the API's own check refuses it too
    let r = c.send(Method::POST, "/api/tasks", &[("content-type", "application/json"), ("authorization", bad.as_str()), ("sec-fetch-site", "same-origin")], body()).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{}", r.body);
    // a valid app token without a cookie session stays exempt (scripts, DAV-side tools)
    let mut b = s.app.basic("alice").await;
    let r = b.send(Method::POST, "/api/tasks", &[("content-type", "application/json")], body()).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
}

#[test]
fn csrf_tokens_compare_whole() {
    use crate::csrf::same;
    assert!(same(b"abc", b"abc") && !same(b"abc", b"abd") && !same(b"abc", b"ab") && !same(b"", b"x"));
}

/// Fails on the code before: pages and JSON had no Cache-Control at all (a shared cache could keep them).
#[tokio::test]
async fn pages_and_json_are_private_and_revalidated_assets_stay_immutable() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    for p in ["/calendar", "/settings"] {
        assert_eq!(c.get(p).await.header("cache-control").as_deref(), Some("private, no-cache"), "{p}");
    }
    assert_eq!(s.app.anonymous().get("/login").await.header("cache-control").as_deref(), Some("private, no-cache"));
    for p in ["/api/events?start=2026-10-01&end=2026-10-02", "/api/colors"] {
        assert_eq!(c.get_json(p).await.header("cache-control").as_deref(), Some("private, no-cache"), "{p}");
    }
    let asset = crate::routes::assets::url("app.js").unwrap();
    let r = c.get(asset).await;
    assert!(r.header("cache-control").is_some_and(|v| v.contains("immutable")), "{:?}", r.header("cache-control"));
}

/// Fails on the code before (in-memory store): every restart — deploy, OOM kill, node reboot — logged
/// everybody out. A new process (new store, new router) on the same database keeps the session.
#[tokio::test]
async fn a_session_survives_a_restart() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    c.get("/calendar").await.ok();
    let restarted = crate::build_router(&s.app.state).unwrap();
    let mut c2 = crate::testkit::Client::for_router(restarted);
    c2.cookie("practical_session", c.cookie_header().split("practical_session=").nth(1).unwrap().split(';').next().unwrap());
    let r = c2.get("/settings").await;
    r.ok().has("data-user=\"alice\"");
    // and its CSRF token: a write from the open tab still passes
    c2.csrf = c.csrf.clone();
    c2.json(Method::PUT, "/api/alarm-presets", json!([])).await.ok();
    // a logout ends it in the database too
    let csrf = c.csrf.clone();
    c.post_form_raw("/logout", &[("csrf", &csrf)]).await;
    let mut c3 = crate::testkit::Client::for_router(crate::build_router(&s.app.state).unwrap());
    c3.cookie("practical_session", c2.cookie_header().split("practical_session=").nth(1).unwrap().split(';').next().unwrap());
    assert_eq!(c3.get("/settings").await.status, StatusCode::SEE_OTHER);
}

/// The database write rate of a busy session: a page view every 10 s for an hour writes at most once a minute
/// (`sessions::REFRESH_AFTER`) — 360 page views, ≤ 61 writes.
#[tokio::test]
async fn an_active_session_writes_the_database_at_most_once_a_minute() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let before = s.app.sessions.writes();
    for _ in 0..360 {
        s.app.sessions.pass(tower_sessions::cookie::time::Duration::seconds(10));
        c.get("/calendar?fragment=1").await.ok();
    }
    let n = s.app.sessions.writes() - before;
    assert!((55..=61).contains(&n), "{n} writes for 360 page views in an hour");
}

/// Fails on the code before (TESTED there: 30 wrong attempts all answered, no slow-down): failed password
/// logins are throttled per client and name; the answer is the same for known and unknown names.
#[tokio::test]
async fn failed_password_logins_are_throttled_and_answer_the_same_for_any_name() {
    let app = TestApp::new().await;
    app.user("alice", "Alice").await;
    app.user("bob", "Bob").await;
    let mut a = app.anonymous();
    let csrf = a.get("/login").await.meta_csrf();
    let mut login = async |user: &str, pw: &str| a.post_form_raw("/login", &[("username", user), ("password", pw), ("csrf", &csrf)]).await.location().unwrap_or_default();
    // an unknown name and a wrong password: one answer
    assert_eq!(login("nobody", "x").await, login("alice", "x").await);
    for _ in 0..4 {
        assert_eq!(login("alice", "x").await, "/login?error=1&redirect_uri=%2Fcalendar");
    }
    // the 6th try for this name is refused even with the right password; another name still gets in
    assert_eq!(login("alice", crate::testkit::PASSWORD).await, "/login?error=2&redirect_uri=%2Fcalendar");
    assert_eq!(login("bob", crate::testkit::PASSWORD).await, "/calendar");
    let mut a = app.anonymous();
    a.get("/login?error=2").await.ok().has("Zu viele Fehlversuche");
    // a minute later one more try
    app.state.guards.login.pass(std::time::Duration::from_secs(61));
    let csrf = a.get("/login").await.meta_csrf();
    let r = a.post_form_raw("/login", &[("username", "alice"), ("password", crate::testkit::PASSWORD), ("csrf", &csrf)]).await;
    assert_eq!(r.location().as_deref(), Some("/calendar"));
}

/// The login form's token: cookie and field must match (double submit); a logged-in session's own token
/// also signs a switch of users (e2e loginAs).
#[tokio::test]
async fn the_login_form_needs_its_cookie_or_the_sessions_token() {
    let app = TestApp::new().await;
    app.user("alice", "Alice").await;
    app.user("bob", "Bob").await;
    let mut a = app.anonymous();
    let csrf = a.get("/login").await.meta_csrf();
    let mut b = app.anonymous(); // the token without its cookie
    let r = b.post_form_raw("/login", &[("username", "alice"), ("password", crate::testkit::PASSWORD), ("csrf", &csrf)]).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let mut c = app.login("alice").await;
    let token = c.csrf.clone();
    let r = c.post_form_raw("/login", &[("username", "bob"), ("password", crate::testkit::PASSWORD), ("csrf", &token)]).await;
    assert_eq!(r.location().as_deref(), Some("/calendar"));
    c.get("/settings").await.ok().has("data-user=\"bob\"");
}

/// Fails on the code before (no cycling): the OIDC callback put the user into the session id the browser
/// brought (fixation). Here a stand-in for rustical_oidc's callback behind the same middleware.
#[tokio::test]
async fn an_oidc_login_gets_a_new_session_id() {
    use tower_sessions::Session;
    let app = TestApp::new().await;
    let store = crate::sessions::DbStore::new(app.state.db.clone());
    let router = axum::Router::new()
        .route("/start", axum::routing::get(|s: Session| async move { s.insert("oidc_state", "x").await.unwrap(); "started" }))
        .route("/callback", axum::routing::get(|s: Session| async move { s.insert(crate::ctx::SESSION_USER, "alice").await.unwrap(); "in" }))
        .layer(axum::middleware::from_fn(crate::auth::cycle_on_login))
        .layer(tower_sessions::SessionManagerLayer::new(store.clone()).with_name("practical_session"));
    let mut c = crate::testkit::Client::for_router(router);
    c.get("/start").await;
    let planted = c.cookie_header();
    c.get("/callback").await;
    let after = c.cookie_header();
    assert!(planted.starts_with("practical_session=") && after.starts_with("practical_session=") && planted != after, "{planted} → {after}");
    let old: tower_sessions::session::Id = planted["practical_session=".len()..].parse().unwrap();
    assert_eq!(tower_sessions::SessionStore::load(&store, &old).await.unwrap(), None, "the planted id is gone");
}

/// Logout with this device's push endpoint removes that subscription (and only that one).
#[tokio::test]
async fn logout_ends_this_devices_push_subscription() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    for (who, ep) in [("alice", "https://push.example.org/this"), ("alice", "https://push.example.org/other"), ("bob", "https://push.example.org/bob")] {
        sqlx::query("INSERT INTO practical_push_subscriptions (id, principal, endpoint, p256dh, auth) VALUES (?, ?, ?, 'k', 'a')").bind(ep).bind(who).bind(ep).execute(&s.app.state.db).await.unwrap();
    }
    let csrf = c.csrf.clone();
    let r = c.post_form_raw("/logout", &[("csrf", &csrf), ("push_endpoint", "https://push.example.org/this")]).await;
    assert_eq!(r.status, StatusCode::SEE_OTHER);
    let left: Vec<String> = sqlx::query_scalar("SELECT endpoint FROM practical_push_subscriptions ORDER BY endpoint").fetch_all(&s.app.state.db).await.unwrap();
    assert_eq!(left, ["https://push.example.org/bob", "https://push.example.org/other"]);
}

/// Fails on the code before (TESTED there with HTTP: 39 of 40 rounds lost a write): two settings saves of
/// different fields at the same time both land.
#[tokio::test]
async fn concurrent_settings_saves_lose_nothing() {
    let s = seed().await;
    let c = s.app.login("alice").await;
    for round in 0..40 {
        let (scheme, cols) = if round % 2 == 0 { ("dark", vec!["email"]) } else { ("light", vec!["phone"]) };
        let (mut a, mut b) = (c.clone(), c.clone());
        let (ra, rb) = tokio::join!(a.json(Method::POST, "/settings/scheme", json!({ "scheme": scheme })), b.json(Method::POST, "/settings/columns", json!(cols)));
        assert_eq!((ra.status, rb.status), (StatusCode::NO_CONTENT, StatusCode::OK));
        let p = crate::domain::prefs::get(&s.app.state.db, "alice").await.unwrap();
        assert_eq!((p.scheme.as_deref(), p.contact_columns_json), (Some(scheme), Some(serde_json::to_string(&cols).unwrap())), "round {round}");
    }
}

/// Fails on the code before (TESTED there: a PUT without `etag` overwrote blindly, 200): a write names the
/// version it is based on — the body's `etag` or `If-Match` — else 428.
#[tokio::test]
async fn a_write_without_its_version_is_refused() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let path = format!("/api/events/alice/{}/review", s.arbeit);
    let d = c.get_json(&path).await.ok().json();
    let mut upd = json!({ "principal": "alice", "cal_id": s.arbeit, "summary": "Blind", "start": "2026-10-14T14:00:00", "end": "2026-10-14T15:30:00" });
    let r = c.json(Method::PUT, &path, upd.clone()).await;
    assert_eq!((r.status, r.body.as_str()), (StatusCode::PRECONDITION_REQUIRED, "Die Version, auf der die Änderung beruht, fehlt – bitte neu laden und erneut speichern."));
    assert_eq!(c.get_json(&path).await.json()["summary"], "Quartalsreview", "nothing written");
    // If-Match carries it as well
    let csrf = c.csrf.clone();
    let etag = d["etag"].as_str().unwrap().to_string();
    let r = c.send(Method::PUT, &path, &[("content-type", "application/json"), ("sec-fetch-site", "same-origin"), ("x-csrf-token", &csrf), ("if-match", &etag)], Body::from(upd.to_string())).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    upd["etag"] = json!(etag);
    assert_eq!(c.json(Method::PUT, &path, upd).await.status, StatusCode::CONFLICT, "the version is gone now");
    // tasks and contacts alike; the tick box (`completed`) needs none
    let tpath = format!("/api/tasks/alice/{}/steuer", s.aufgaben);
    let r = c.json(Method::PUT, &tpath, json!({ "principal": "alice", "cal_id": s.aufgaben, "summary": "Blind" })).await;
    assert_eq!(r.status, StatusCode::PRECONDITION_REQUIRED);
    c.json(Method::PUT, &tpath, json!({ "principal": "alice", "cal_id": s.aufgaben, "summary": "", "completed": true })).await.ok();
    let r = c.json(Method::PUT, &tpath, json!({ "principal": "alice", "cal_id": s.aufgaben, "summary": "", "completed": false, "etag": "\"stale\"" })).await;
    assert_eq!(r.status, StatusCode::CONFLICT, "a version given with the tick is checked");
    let cpath = format!("/api/contacts/alice/{}/anna", s.freunde);
    let r = c.json(Method::PUT, &cpath, json!({ "principal": "alice", "ab_id": s.freunde, "full_name": "Blind" })).await;
    assert_eq!(r.status, StatusCode::PRECONDITION_REQUIRED);
}

/// Fails on the code before (VERIFIED there): a move read the source object before any owner check (an
/// existence/UID oracle), copied it into the mover's calendar and only then failed on the delete — an orphan
/// copy — and ignored the ETag.
#[tokio::test]
async fn a_move_checks_the_owner_first_and_honours_the_version() {
    let s = seed().await;
    let carols = s.app.calendar("carol", "Carol", "#000000", &["VEVENT"]).await;
    let mut carol = s.app.login("carol").await;
    let path = format!("/api/events/alice/{}/review", s.arbeit);
    let mv = json!({ "principal": "carol", "cal_id": carols, "summary": "Mitgenommen", "start": "2026-10-14T14:00:00", "end": "2026-10-14T15:00:00", "etag": "\"x\"" });
    assert_eq!(carol.json(Method::PUT, &path, mv).await.status, StatusCode::FORBIDDEN);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM calendarobjects WHERE cal_id = ?").bind(&carols).fetch_one(&s.app.state.db).await.unwrap();
    assert_eq!(n, 0, "no copy in carol's calendar");
    // alice moving with a stale version: refused, nothing created, the source stays
    let mut c = s.app.login("alice").await;
    let mv = json!({ "principal": "alice", "cal_id": s.privat, "summary": "Quartalsreview", "start": "2026-10-14T14:00:00", "end": "2026-10-14T15:30:00", "etag": "\"stale\"" });
    let r = c.json(Method::PUT, &path, mv).await;
    assert_eq!((r.status, r.body.as_str()), (StatusCode::CONFLICT, "Der Termin wurde anderswo geändert – bitte neu laden."));
    c.get_json(&path).await.ok();
    assert_eq!(c.get_json(&format!("/api/events/alice/{}/review", s.privat)).await.status, StatusCode::NOT_FOUND);
    // contacts alike
    let cpath = format!("/api/contacts/alice/{}/anna", s.freunde);
    let other = s.app.addressbook("carol", "C", "#000000").await;
    let r = carol.json(Method::PUT, &cpath, json!({ "principal": "carol", "ab_id": other, "full_name": "Anna", "etag": "\"x\"" })).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM addressobjects WHERE addressbook_id = ?").bind(&other).fetch_one(&s.app.state.db).await.unwrap();
    assert_eq!(n, 0);
}

/// Fails on the code before (each TESTED there by the audit's fuzzer): dates at chrono's range edge dropped
/// the connection (panic), and every malformed input got axum's English text with 400/415/422 mixed. Now
/// one gate (extract.rs): 400 naming the field, 413 / 415 with their own status, all in the user's language.
#[tokio::test]
async fn malformed_input_gets_one_kind_of_answer() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let long_q = format!("/api/search?q={}", "x".repeat(201));
    for (path, field) in [
        ("/api/density?year=262143", "year"),
        ("/calendar?date=%2B262142-12-31&fragment=1", "date"),
        ("/calendar?view=week&date=2026-10-14&week=0000-12-31&fragment=1", "week"),
        ("/api/events?start=2026-10-01&end=%2B262142-01-01", "end"),
        ("/api/days?start=9999-99-99", "start"),
        ("/api/weeks?start=-0001-01-01", "start"),
        (long_q.as_str(), "q"),
    ] {
        let r = c.get_json(path).await;
        assert_eq!((r.status, r.body.as_str()), (StatusCode::BAD_REQUEST, format!("Ungültige Angabe im Feld „{field}“.").as_str()), "{path}");
    }
    // the edge that stays valid
    c.get_json("/api/density?year=9999").await.ok();
    c.get("/calendar?date=9999-12-31&fragment=1").await.ok();
    c.get("/calendar?view=month&month=%2B262142-12&fragment=1").await.ok();
    // bodies: malformed, wrong type, too large
    let csrf = c.csrf.clone();
    let h = |ct: &'static str| vec![("content-type", ct), ("sec-fetch-site", "same-origin"), ("x-csrf-token", csrf.as_str())];
    let r = c.send(Method::PUT, "/api/alarm-presets", &h("application/json"), Body::from("[{")).await;
    assert_eq!((r.status, r.body.as_str()), (StatusCode::BAD_REQUEST, "Die Anfrage ließ sich nicht lesen."));
    let r = c.send(Method::PUT, "/api/alarm-presets", &h("text/plain"), Body::from("[]")).await;
    assert_eq!((r.status, r.body.as_str()), (StatusCode::UNSUPPORTED_MEDIA_TYPE, "Dieser Inhaltstyp wird nicht unterstützt."));
    let r = c.send(Method::POST, "/api/tasks", &h("application/json"), Body::from(format!("{{\"summary\":\"{}\"}}", "x".repeat(9 << 20)))).await;
    assert_eq!((r.status, r.body.as_str()), (StatusCode::PAYLOAD_TOO_LARGE, "Zu groß."));
    // path segments: no control characters, no encoded slash, no endless ids
    for p in ["/api/events/alice/a%0Ab/x", "/api/events/alice/a%2Fb/x", &format!("/api/events/alice/{}/x", "a".repeat(1100))] {
        assert_eq!(c.get_json(p).await.status, StatusCode::BAD_REQUEST, "{p}");
    }
    let r = c.get_json("/api/history/notanumber/plan").await;
    assert_eq!((r.status, r.body.as_str()), (StatusCode::BAD_REQUEST, "Ungültige Angabe im Feld „id“."));
}

/// Fails on the code before (TESTED there): a settings form with a bad or missing field showed a bare text
/// page ("Failed to deserialize form body: missing field `csrf`"); a calendar took any time zone and colour
/// text (CR/LF included). Now the settings answer: a flash and back, or {ok, msg} for the autosave.
#[tokio::test]
async fn a_rejected_settings_form_answers_like_the_settings_page() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let r = c.post_form_raw("/settings/groups", &[("name", "ohne Token")]).await;
    assert_eq!((r.status, r.location().as_deref()), (StatusCode::SEE_OTHER, Some("/settings")));
    c.get("/settings").await.ok().has("<div class=\"toast error\" role=\"alert\"><span>Ungültige Angabe im Feld „csrf“.</span>");
    for (k, v) in [("timezone", "Mars/Phobos"), ("color", "#fff\r\nX-EVIL:1"), ("name", &"n".repeat(201))] {
        let mut f = vec![("owner", "alice"), ("name", "Neu"), ("comp_event", "on")];
        f.retain(|(x, _)| *x != k);
        f.push((k, v));
        let r = c.post_form_json("/settings/calendars", &f).await;
        assert_eq!(r.json(), json!({ "ok": false, "msg": format!("Ungültige Angabe im Feld „{k}“.") }), "{k}");
    }
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM calendars WHERE principal = 'alice' AND displayname = 'Neu'").fetch_one(&s.app.state.db).await.unwrap();
    assert_eq!(n, 0);
    // what the colour picker and DAV clients send stays accepted
    for color in ["#A1b2C3", "#a1b2c3ff", ""] {
        c.post_form_json("/settings/calendars", &[("owner", "alice"), ("name", "Neu"), ("comp_event", "on"), ("color", color), ("timezone", "Europe/Berlin")]).await.json()["ok"].as_bool().filter(|ok| *ok).expect(color);
    }
}
