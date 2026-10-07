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
    crate::db::check_schema(db).await.expect("a freshly migrated database passes");
    sqlx::raw_sql("ALTER TABLE practical_user_settings DROP COLUMN alarm_snaps_json").execute(&mut *conn).await.unwrap();
    let e = crate::db::check_schema(db).await.unwrap_err();
    assert!(e.contains("missing practical_user_settings.alarm_snaps_json"), "{e}");
    sqlx::raw_sql("ALTER TABLE practical_user_settings ADD COLUMN alarm_snaps_json INTEGER").execute(&mut *conn).await.unwrap();
    let e = crate::db::check_schema(db).await.unwrap_err();
    assert!(e.contains("practical_user_settings.alarm_snaps_json is INTEGER"), "{e}");
    sqlx::raw_sql("ALTER TABLE practical_user_settings DROP COLUMN alarm_snaps_json").execute(&mut *conn).await.unwrap();
    sqlx::raw_sql("ALTER TABLE practical_user_settings ADD COLUMN alarm_snaps_json TEXT").execute(&mut *conn).await.unwrap();
    crate::db::check_schema(db).await.expect("repaired");
    sqlx::raw_sql("CREATE TABLE practical_extra (x)").execute(&mut *conn).await.unwrap();
    assert!(crate::db::check_schema(db).await.unwrap_err().contains("unexpected practical_extra.x"));
    sqlx::raw_sql("DROP TABLE practical_extra").execute(&mut *conn).await.unwrap();
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
