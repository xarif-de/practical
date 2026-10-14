//! The session's inactivity expiry and prefetches (sessions::activity): which requests extend the
//! session, how far, and that a prefetch never consumes a one-shot value or writes session data.
//! Time passes through `DbStore::pass` (every stored instant moves back by the same amount).

use crate::testkit::{Client, Resp, TestApp};
use axum::body::Body;
use axum::http::{Method, StatusCode};
use tower_sessions::cookie::time::{Duration, OffsetDateTime};
use tower_sessions::session::Id;

const LIFETIME: Duration = Duration::hours(12); // testkit config: session_hours = 12
const REFRESH: Duration = Duration::seconds(crate::sessions::REFRESH_AFTER.whole_seconds() + 1);

async fn alice() -> (TestApp, Client, Id) {
    let app = TestApp::new().await;
    app.user("alice", "Alice").await;
    let c = app.login("alice").await;
    let ids = app.sessions.ids();
    assert_eq!(ids.len(), 1, "the login cycles the id: one live session");
    let id = ids[0];
    (app, c, id)
}

async fn prefetch(c: &mut Client, method: Method, path: &str) -> Resp {
    c.send(method, path, &[("accept", "text/html"), ("purpose", "prefetch")], Body::empty()).await
}

/// The stored expiry, as seconds left from now.
fn left(app: &TestApp, id: &Id) -> i64 {
    (app.sessions.expiry(id).expect("live session") - OffsetDateTime::now_utc()).whole_seconds()
}

fn sets_cookie(r: &Resp) -> bool {
    r.header("set-cookie").is_some_and(|c| c.starts_with("practical_session=") && c.contains("Max-Age=43200"))
}

#[tokio::test]
async fn a_user_active_every_hour_stays_and_one_idle_for_12_hours_is_logged_out() {
    let (app, mut c, id) = alice().await;
    for hour in 1..=30 {
        app.sessions.pass(Duration::hours(1));
        let r = c.get("/calendar?fragment=1").await;
        assert_eq!(r.status, StatusCode::OK, "hour {hour}: {:?}", r.location());
        assert!(sets_cookie(&r), "hour {hour}: the cookie's Max-Age moves with the stored expiry");
        assert!((LIFETIME.whole_seconds() - left(&app, &id)).abs() <= 2, "hour {hour}: expiry = now + 12 h");
    }
    // a JSON API without a page context does not count as use (as before: it never wrote the session)
    app.sessions.pass(Duration::hours(11) + Duration::minutes(59));
    assert!(!sets_cookie(c.get_json("/api/titles").await.ok()));
    assert!(left(&app, &id) <= 61);
    // one page view inside the last minute keeps it for another 12 h
    assert!(sets_cookie(c.get("/tasks").await.ok()));
    app.sessions.pass(LIFETIME + Duration::seconds(1));
    let r = c.get("/calendar").await;
    assert_eq!((r.status, r.location().as_deref()), (StatusCode::SEE_OTHER, Some("/login?redirect_uri=%2Fcalendar")), "idle for 12 h + 1 s");
    assert_eq!(c.get_json("/api/titles").await.status, StatusCode::UNAUTHORIZED);
}

/// Fails on the code before sessions::activity: every page view removed the flash unconditionally,
/// so each one saved the session and sent a cookie.
#[tokio::test]
async fn page_views_inside_a_minute_write_the_session_once() {
    let (app, mut c, id) = alice().await;
    app.sessions.pass(Duration::minutes(5));
    assert!(sets_cookie(c.get("/calendar").await.ok()), "5 min after the last write");
    let after = app.sessions.expiry(&id).unwrap();
    for (i, p) in ["/calendar?fragment=1", "/calendar?view=week&date=2026-10-14&fragment=1", "/settings", "/contacts", "/history"].iter().enumerate() {
        app.sessions.pass(Duration::seconds(10));
        let r = c.get(p).await;
        assert_eq!((r.status, r.header("set-cookie")), (StatusCode::OK, None), "{p}");
        assert_eq!(app.sessions.expiry(&id).unwrap(), after - Duration::seconds(10 * (i as i64 + 1)), "{p}: no store write");
    }
    // 50 s after the write the expiry lags the last page view by 50 s; past REFRESH_AFTER the next view writes
    assert!((LIFETIME.whole_seconds() - 50 - left(&app, &id)).abs() <= 2);
    app.sessions.pass(Duration::seconds(11));
    assert!(sets_cookie(c.get("/calendar?fragment=1").await.ok()));
    assert!((LIFETIME.whole_seconds() - left(&app, &id)).abs() <= 2);
}

/// Exactly the requests that extended the session before still do: those with a page context. The
/// table is the result of the same requests against the code before (8904762), where a cookie on
/// the answer meant a session save.
#[tokio::test]
async fn the_same_requests_extend_the_session_as_before() {
    let (app, mut c, _) = alice().await;
    let table = [
        ("/", false), ("/login", true), ("/calendar", true), ("/calendar?fragment=1", true), ("/calendar?view=week&date=2026-10-14&fragment=1", true),
        ("/contacts", true), ("/tasks", true), ("/history", true), ("/settings", true), ("/api/events?start=2026-10-01&end=2026-11-01", true),
        ("/api/density?year=2026", true), ("/api/weeks?from=2026-10-12&n=2", true), ("/api/days?from=2026-10-12&n=2", true), ("/api/tasks/done", true),
        ("/api/history", true), ("/api/search?q=x", true), ("/api/mentions", true), ("/api/categories", true), ("/api/titles", false), ("/api/colors", false),
        ("/api/category-rules", false), ("/api/push/key", false), ("/api/push/subscriptions", false), ("/api/alarm-presets", false),
        ("/api/search/history", false), ("/api/holidays/countries", true), ("/api/events/suggest?q=x", true),
    ];
    for (p, extends) in table {
        app.sessions.pass(REFRESH);
        assert_eq!(sets_cookie(&c.get(p).await), extends, "{p} (html)");
        app.sessions.pass(REFRESH);
        assert_eq!(sets_cookie(&c.get_json(p).await), extends, "{p} (json)");
    }
}

#[tokio::test]
async fn a_flash_is_removed_by_the_page_that_shows_it_and_never_by_a_prefetch() {
    let (app, mut c, id) = alice().await;
    let r = c.post_form("/settings/tokens", &[("name", " ")]).await;
    assert_eq!(r.location().as_deref(), Some("/settings#manual"));
    assert!(app.sessions.data(&id).unwrap().contains_key("flash"));
    for p in ["/calendar?fragment=1", "/settings", "/tasks"] {
        let r = prefetch(&mut c, Method::GET, p).await;
        assert_eq!((r.status, r.header("cache-control").as_deref(), r.header("set-cookie")), (StatusCode::NO_CONTENT, Some("no-store"), None), "{p}");
    }
    assert!(app.sessions.data(&id).unwrap().contains_key("flash"), "still pending");
    c.get("/settings").await.ok().has("<div class=\"toast error\"");
    assert!(!app.sessions.data(&id).unwrap().contains_key("flash"));
    assert!(!c.get("/settings").await.ok().body.contains("class=\"toast"), "shown once");
    // without a pending flash a prefetch gets the page a click would get
    let p = "/calendar?view=month&date=2026-10-14&fragment=1";
    let (pre, real) = (prefetch(&mut c, Method::GET, p).await, c.get(p).await);
    assert_eq!((pre.status, &pre.body), (StatusCode::OK, &real.body));
    // never anything with side effects
    let r = prefetch(&mut c, Method::POST, "/settings/tokens").await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_new_app_token_waits_for_the_page_view_not_for_a_prefetch() {
    let (app, mut c, id) = alice().await;
    c.post_form("/settings/tokens", &[("name", "Thunderbird")]).await;
    assert!(app.sessions.data(&id).unwrap().contains_key("new_token"));
    assert_eq!(prefetch(&mut c, Method::GET, "/settings").await.status, StatusCode::NO_CONTENT);
    c.get("/settings").await.ok().has("token-reveal");
    assert!(!app.sessions.data(&id).unwrap().contains_key("new_token"));
    assert!(!c.get("/settings").await.ok().body.contains("token-reveal"));
}

#[tokio::test]
async fn a_prefetch_extends_the_session_but_never_writes_its_data() {
    let (app, mut c, id) = alice().await;
    let before = app.sessions.data(&id).unwrap();
    app.sessions.pass(Duration::hours(2));
    let r = prefetch(&mut c, Method::GET, "/calendar?fragment=1").await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(sets_cookie(&r), "hovering is use");
    assert!((LIFETIME.whole_seconds() - left(&app, &id)).abs() <= 2);
    assert_eq!(app.sessions.data(&id).unwrap(), before, "an expiry-only save");
    // a Basic-auth client has no session, and a page view no longer makes one for a CSRF token
    let mut b = app.basic("alice").await;
    let n = app.sessions.ids().len();
    let r = prefetch(&mut b, Method::GET, "/calendar?fragment=1").await;
    assert_eq!((r.status, r.header("set-cookie")), (StatusCode::OK, None));
    assert_eq!(app.sessions.ids().len(), n, "no session created");
}

#[tokio::test]
async fn anonymous_and_stream_requests_still_set_no_cookie() {
    let app = TestApp::new().await;
    // idle's no-cookie paths stay cookie-free, prefetched or not
    for path in ["/healthz", "/assets/app.js", "/calendar", "/api/changes"] {
        let r = prefetch(&mut app.anonymous(), Method::GET, path).await;
        assert_eq!(r.header("set-cookie"), None, "{path} → {}", r.status);
    }
    app.user("alice", "Alice").await;
    let mut c = app.login("alice").await;
    let (status, headers) = c.open("/api/changes").await;
    assert_eq!((status, headers.get("set-cookie")), (StatusCode::OK, None));
}
