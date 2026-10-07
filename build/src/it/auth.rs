//! Login, logout, the guard in front of every page and API, and the CSRF layers.

use super::seed::seed;
use crate::testkit::{PASSWORD, TestApp};
use axum::body::Body;
use axum::http::{Method, StatusCode};

#[tokio::test]
async fn anonymous_browsers_go_to_the_login_and_scripts_get_401() {
    let app = TestApp::new().await;
    let mut a = app.anonymous();
    for path in ["/calendar", "/contacts", "/tasks", "/history", "/settings", "/calendar?view=week&date=2026-10-14"] {
        let r = a.get(path).await;
        assert_eq!(r.status, StatusCode::SEE_OTHER, "{path}");
        assert_eq!(r.location().unwrap(), format!("/login?redirect_uri={}", crate::auth::urlenc(path)));
    }
    for path in ["/api/events?start=2026-10-01&end=2026-10-02", "/api/search?q=x", "/api/colors", "/api/history"] {
        let r = a.get_json(path).await;
        assert_eq!((r.status, r.body.as_str()), (StatusCode::UNAUTHORIZED, "login required"), "{path}");
    }
    // a POST is never redirected, it is refused by the origin guard (no browser metadata) first
    let r = a.send(Method::POST, "/api/events", &[("content-type", "application/json")], Body::from("{}")).await;
    assert_eq!((r.status, r.body.as_str()), (StatusCode::FORBIDDEN, "cross-site request rejected"));
    let r = a.send(Method::POST, "/api/events", &[("content-type", "application/json"), ("sec-fetch-site", "same-origin")], Body::from("{}")).await;
    assert_eq!((r.status, r.body.as_str()), (StatusCode::UNAUTHORIZED, "login required"));
}

#[tokio::test]
async fn password_login_success_failure_and_redirect_targets() {
    let app = TestApp::new().await;
    app.user("alice", "Alice Beispiel").await;
    let mut a = app.anonymous();
    let page = a.get("/login?redirect_uri=/tasks").await;
    page.ok().has("name=\"redirect_uri\" value=\"/tasks\"").has("<form method=\"post\" action=\"/login\" class=\"pwform\">");
    assert!(!page.body.contains("oidc-form"), "no OIDC configured");
    let csrf = page.meta_csrf();
    // wrong password: back to the form with the error and the target
    let r = a.post_form_raw("/login", &[("username", "alice"), ("password", "nope"), ("csrf", &csrf), ("redirect_uri", "/tasks")]).await;
    assert_eq!(r.location().as_deref(), Some("/login?error=1&redirect_uri=%2Ftasks"));
    a.get("/login?error=1").await.ok().has("class=\"flash error\"");
    // wrong CSRF token
    let r = a.post_form_raw("/login", &[("username", "alice"), ("password", PASSWORD), ("csrf", "x"), ("redirect_uri", "/tasks")]).await;
    assert_eq!((r.status, r.body.as_str()), (StatusCode::FORBIDDEN, "forbidden"));
    // off-site and login targets fall back to the calendar
    for (target, expected) in [("//evil.example.org/x", "/calendar"), ("https://evil.example.org", "/calendar"), ("/login?x", "/calendar"), ("/contacts?view=table", "/contacts?view=table")] {
        let mut b = app.anonymous();
        let csrf = b.get("/login").await.meta_csrf();
        let r = b.post_form_raw("/login", &[("username", "alice"), ("password", PASSWORD), ("csrf", &csrf), ("redirect_uri", target)]).await;
        assert_eq!(r.location().as_deref(), Some(expected), "{target}");
    }
    // a redirect_uri that is not a path is not even put into the form
    let page = a.get("/login?redirect_uri=https://evil.example.org").await;
    assert!(!page.body.contains("evil.example.org"));
}

#[tokio::test]
async fn logout_ends_the_session() {
    let app = TestApp::new().await;
    app.user("alice", "Alice Beispiel").await;
    let mut c = app.login("alice").await;
    let csrf = c.csrf.clone();
    let r = c.post_form_raw("/logout", &[("csrf", "wrong")]).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let r = c.post_form_raw("/logout", &[("csrf", &csrf)]).await;
    assert_eq!(r.location().as_deref(), Some("/login?manual=1"));
    assert_eq!(c.get("/calendar").await.status, StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn logout_hands_over_to_the_identity_provider_portal() {
    let app = TestApp::with(|c| {
        c.oidc = Some(crate::config::OidcSettings { name: "SSO".into(), issuer: "https://auth.example.org".into(), client_id: "practical".into(), client_secret: None, scopes: vec!["openid".into()] });
        c.logout_url = "https://auth.example.org/logout?rd={url}".into();
    })
    .await;
    app.user("alice", "Alice Beispiel").await;
    let mut c = app.login("alice").await;
    let csrf = c.csrf.clone();
    let r = c.post_form_raw("/logout", &[("csrf", &csrf)]).await;
    assert_eq!(r.location().as_deref(), Some("https://auth.example.org/logout?rd=http%3A%2F%2Flocalhost%3A4001"));
    // with both methods the login page offers both and does not jump to the IdP
    let page = app.anonymous().get("/login").await;
    page.ok().has("id=\"oidc-form\">").has("class=\"pwform\"");
    assert!(!page.body.contains("data-auto"));
}

#[tokio::test]
async fn oidc_only_login_page_submits_itself_unless_asked_not_to() {
    let app = TestApp::with(|c| {
        c.oidc = Some(crate::config::OidcSettings { name: "Authelia".into(), issuer: "https://auth.example.org".into(), client_id: "practical".into(), client_secret: None, scopes: vec!["openid".into()] });
        c.allow_password_login = false;
    })
    .await;
    let mut a = app.anonymous();
    // app.js submits a form marked data-auto (an inline script would break the Content-Security-Policy)
    a.get("/login").await.ok().has("id=\"oidc-form\" data-auto>").has("Authelia");
    let manual = a.get("/login?manual=1").await;
    manual.ok();
    assert!(!manual.body.contains("data-auto") && !manual.body.contains("pwform"));
    let csrf = manual.meta_csrf();
    let r = a.post_form_raw("/login", &[("username", "x"), ("password", "y"), ("csrf", &csrf)]).await;
    assert_eq!((r.status, r.body.as_str()), (StatusCode::FORBIDDEN, "forbidden"), "password login is off");
}

#[tokio::test]
async fn dev_login_signs_in_without_a_password() {
    let assets = concat!(env!("CARGO_MANIFEST_DIR"), "/assets");
    let app = TestApp::with(|c| {
        c.dev_assets_dir = Some(assets.into());
        c.dev_login = Some("alice".into());
    })
    .await;
    app.user("alice", "Alice Beispiel").await;
    let mut a = app.anonymous();
    let r = a.get("/login?redirect_uri=/tasks").await;
    assert_eq!(r.location().as_deref(), Some("/tasks"));
    a.get("/settings").await.ok().has("data-dev=\"1\"");
    let (status, headers) = a.open("/dev/reload").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers.get("content-type").unwrap(), "text/event-stream");
}

#[tokio::test]
async fn basic_auth_with_an_app_token() {
    let s = seed().await;
    let mut b = s.app.basic("alice").await;
    let r = b.get_json("/api/events?start=2026-10-14&end=2026-10-15").await;
    assert_eq!(r.ok().json().as_array().unwrap().len(), 3);
    // pages work too
    b.get("/calendar?date=2026-10-14").await.ok().has("data-user=\"alice\"");
    // Basic skips the origin guard and the API's CSRF header
    let r = b.send(Method::PUT, "/api/alarm-presets", &[("content-type", "application/json")], Body::from("[]")).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    // but /settings JSON endpoints and undo check the session token
    let r = b.send(Method::POST, "/settings/scheme", &[("content-type", "application/json")], Body::from(r#"{"scheme":"dark"}"#)).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    // a wrong token is anonymous
    use base64ct::Encoding;
    let bad = format!("Basic {}", base64ct::Base64::encode_string(b"alice:abcd_wrong"));
    let r = s.app.anonymous().send(Method::GET, "/api/colors", &[("authorization", &bad)], Body::empty()).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    // user$group acts as the group: its calendars only
    let mut alice = s.app.login("alice").await;
    let tok = alice.post_form("/settings/tokens", &[("name", "dav")]).await;
    assert_eq!(tok.location().as_deref(), Some("/settings#account"));
    let shown = alice.get("/settings").await;
    let token = shown.body.split("<code id=\"new-token\">").nth(1).map(|r| r[..r.find('<').unwrap()].to_string());
    if let Some(token) = token.filter(|t| t.contains('_')) {
        let h = format!("Basic {}", base64ct::Base64::encode_string(format!("alice$group.familie:{token}").as_bytes()));
        let r = s.app.anonymous().send(Method::GET, "/api/events?start=2026-10-01&end=2026-10-31", &[("authorization", &h)], Body::empty()).await;
        let names: Vec<String> = r.ok().json().as_array().unwrap().iter().map(|i| i["summary"].as_str().unwrap().to_string()).collect();
        assert_eq!(names, ["Familientreffen"]);
    } else {
        panic!("the new token is not on the settings page");
    }
}

#[tokio::test]
async fn origin_guard_matrix() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let csrf = c.csrf.clone();
    let body = || Body::from(r#"{"scheme":"dark"}"#);
    let base = [("content-type", "application/json"), ("x-csrf-token", csrf.as_str())];
    let try_with = |extra: &'static [(&'static str, &'static str)]| {
        let mut h: Vec<(&str, &str)> = base.to_vec();
        h.extend_from_slice(extra);
        h
    };
    for (extra, expected) in [
        (&[("sec-fetch-site", "same-origin")][..], StatusCode::NO_CONTENT),
        (&[("sec-fetch-site", "none")][..], StatusCode::NO_CONTENT),
        (&[("sec-fetch-site", "cross-site")][..], StatusCode::FORBIDDEN),
        (&[("sec-fetch-site", "same-site")][..], StatusCode::FORBIDDEN),
        (&[("origin", "http://localhost:4001")][..], StatusCode::NO_CONTENT),
        (&[("origin", "HTTP://LOCALHOST:4001")][..], StatusCode::NO_CONTENT),
        (&[("origin", "http://evil.example.org")][..], StatusCode::FORBIDDEN),
        // Sec-Fetch-Site wins over Origin when it says cross-site… no: any Origin equal to ours passes
        (&[("sec-fetch-site", "cross-site"), ("origin", "http://localhost:4001")][..], StatusCode::NO_CONTENT),
        (&[][..], StatusCode::FORBIDDEN),
    ] {
        let r = c.send(Method::POST, "/settings/scheme", &try_with(extra), body()).await;
        assert_eq!(r.status, expected, "{extra:?}: {}", r.body);
    }
    // GET/HEAD/OPTIONS are never checked
    let r = c.send(Method::HEAD, "/settings", &[("sec-fetch-site", "cross-site")], Body::empty()).await;
    assert_ne!(r.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn session_csrf_token_is_required_for_session_writes() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let good = c.csrf.clone();
    for token in ["", "wrong"] {
        c.csrf = token.into();
        for (method, path, body) in [
            (Method::PUT, "/api/alarm-presets", "[]"),
            (Method::POST, "/settings/scheme", r#"{"scheme":"dark"}"#),
            (Method::POST, "/api/search/history", r#"{"q":"x"}"#),
            (Method::POST, "/api/history/1/undo", "{}"),
        ] {
            let r = c.json(method.clone(), path, serde_json::from_str(body).unwrap()).await;
            assert_eq!((r.status, r.body.as_str()), (StatusCode::FORBIDDEN, "Dafür fehlt die Berechtigung."), "{method} {path} with {token:?}");
        }
        let r = c.post_form("/settings/groups", &[("name", "X")]).await;
        assert_eq!(r.location().as_deref(), Some("/settings#groups"), "a form error is a flash, not a status");
    }
    c.csrf = good;
    let r = c.json(Method::PUT, "/api/alarm-presets", serde_json::json!([])).await;
    assert_eq!(r.status, StatusCode::OK);
    // the forbidden form above left a flash for the next page
    c.get("/settings").await.ok().has("<div class=\"toast error\" role=\"alert\"><span>Dafür fehlt die Berechtigung.</span>");
}

#[tokio::test]
async fn session_cookie_attributes() {
    // the session starts with the login (an anonymous page view stores nothing, sessions.rs)
    async fn session_cookie(app: &TestApp) -> String {
        app.user("alice", "Alice").await;
        let mut a = app.anonymous();
        let login_cookie = a.get("/login").await.header("set-cookie").unwrap();
        let csrf = a.get("/login").await.meta_csrf();
        let r = a.post_form_raw("/login", &[("username", "alice"), ("password", PASSWORD), ("csrf", &csrf)]).await;
        let all: Vec<String> = r.headers.get_all("set-cookie").iter().map(|v| v.to_str().unwrap().to_string()).collect();
        assert!(all.iter().any(|c| c.starts_with("practical_login_csrf=;") && c.contains("Max-Age=0")), "{all:?}");
        format!("{login_cookie}\n{}", all.into_iter().find(|c| c.starts_with("practical_session=")).unwrap())
    }
    let app = TestApp::new().await;
    for cookie in session_cookie(&app).await.lines() {
        let lower = cookie.to_ascii_lowercase();
        assert!(lower.contains("httponly") && (lower.contains("samesite=lax") || lower.contains("samesite=strict")) && !lower.contains("secure"), "{cookie}");
    }
    let https = TestApp::with(|c| c.public_url = "https://plan.example.org".into()).await;
    for cookie in session_cookie(&https).await.lines() {
        assert!(cookie.to_ascii_lowercase().contains("secure"), "{cookie}");
    }
}

#[test]
fn safe_redirect_and_urlenc() {
    use crate::auth::{safe_redirect, urlenc};
    assert_eq!(safe_redirect(None), "/calendar");
    assert_eq!(safe_redirect(Some("")), "/calendar");
    assert_eq!(safe_redirect(Some("/tasks?view=board")), "/tasks?view=board");
    assert_eq!(safe_redirect(Some("//evil.example.org")), "/calendar");
    assert_eq!(safe_redirect(Some("https://evil.example.org")), "/calendar");
    assert_eq!(safe_redirect(Some("/login")), "/calendar");
    assert_eq!(safe_redirect(Some("/loginx")), "/calendar");
    assert_eq!(safe_redirect(Some("calendar")), "/calendar");
    // browsers read "/\evil.example.org" as "//evil.example.org": refused since 36c9ca6
    assert_eq!(safe_redirect(Some("/\\evil.example.org")), "/calendar");
    assert_eq!(urlenc("/calendar?view=week&date=2026-10-14"), "%2Fcalendar%3Fview%3Dweek%26date%3D2026-10-14");
    assert_eq!(urlenc("a b+ä"), "a+b%2B%C3%A4");
    assert_eq!(urlenc(""), "");
}
