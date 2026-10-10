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
        c.oidc = Some(crate::testkit::oidc_settings());
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
        c.oidc = Some(crate::config::OidcSettings { name: "Authelia".into(), ..crate::testkit::oidc_settings() });
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

/// RustiCal's own frontend creates the principal at a first OIDC sign-in; with `OIDC__ALLOW_SIGN_UP`
/// PractiCal does the same write (`SignInStore`), and refuses it in the three cases that would make a
/// broken or foreign account. Drives the store directly: the way in is an identity provider's callback.
#[tokio::test]
async fn sign_up_creates_the_principal_only_when_it_is_switched_on() {
    use crate::auth::{SignInError, SignInStore};
    use rustical_oidc::UserStore;
    use rustical_store::auth::{AuthenticationProvider, PrincipalType};

    let app = TestApp::new().await;
    app.user("alice", "Alice Beispiel").await;
    app.group("group.familie", "Familie", &["alice"]).await;
    let off = SignInStore { principals: app.state.principals.clone(), allow_sign_up: false };
    let on = SignInStore { principals: app.state.principals.clone(), allow_sign_up: true };

    // an existing user is found by both, and neither writes to it
    assert!(off.user_exists("alice").await.unwrap() && on.user_exists("alice").await.unwrap());
    on.ensure_user("alice", &[]).await.unwrap();
    let alice = app.principal("alice").await;
    assert_eq!((alice.displayname.as_deref(), alice.password.is_some()), (Some("Alice Beispiel"), true), "an existing account is never overwritten");

    // sign-up off: an unknown name stays unknown and the browser is told so on the login page
    assert!(!off.user_exists("mallory").await.unwrap());
    let e = off.ensure_user("mallory", &[]).await.unwrap_err();
    assert!(matches!(&e, SignInError::NoAccount));
    assert_eq!(location(e), "/login?error=3");
    assert!(app.state.principals.get_principal("mallory").await.unwrap().is_none());

    // sign-up on: the principal appears, as an individual without a password, and the second sign-in
    // of the same person creates nothing more
    on.ensure_user("mallory", &[]).await.unwrap();
    let fresh = app.principal("mallory").await;
    assert_eq!((fresh.principal_type, fresh.displayname.clone(), fresh.password.is_some(), fresh.memberships.clone()), (PrincipalType::Individual, None, false, vec![]));
    on.ensure_user("mallory", &[]).await.unwrap();
    assert_eq!(app.state.principals.get_principals().await.unwrap().iter().filter(|p| p.id == "mallory").count(), 1);

    // A group is no account to sign in to: users and groups share one table, and RustiCal's
    // authentication middleware signs in whatever the session names (middleware.rs:77). `rustical
    // principals create --principal-type group` takes any id, so the one WITHOUT PractiCal's
    // `group.` prefix is the case the prefix check cannot catch: it is caught by reading the row
    // back after `insert_principal` answers AlreadyExists.
    app.group("verein", "Verein", &[]).await;
    for g in ["group.familie", "verein"] {
        assert!(!on.user_exists(g).await.unwrap(), "{g}");
        assert_eq!(location(on.ensure_user(g, &[]).await.unwrap_err()), "/login?error=4", "{g}");
        assert_eq!(app.principal(g).await.principal_type, PrincipalType::Group, "{g} is untouched");
    }

    // an id that cannot be a principal, or a path segment, is refused instead of written
    for bad in ["", "group.neu", "a:b", "a$b", "a/b", "a b", "a?b", "a#b", "a%b", "a\\b", "a\nb"] {
        let e = on.ensure_user(bad, &[]).await.unwrap_err();
        assert!(matches!(&e, SignInError::IdRejected), "{bad:?}");
        assert!(app.state.principals.get_principal(bad).await.unwrap().is_none(), "{bad:?}");
    }
}

/// The login page tells a visitor without an account what to do, and does NOT submit itself to the
/// identity provider again (`auto_oidc` is off while `error` is set) — that would be a redirect loop.
#[tokio::test]
async fn the_login_page_offers_the_way_to_rustical_when_sign_up_is_off() {
    let app = TestApp::with(|c| {
        c.oidc = Some(crate::testkit::oidc_settings());
        c.allow_password_login = false;
        c.public_dav_url = "https://dav.example.org".into();
    })
    .await;
    let page = app.anonymous().get("/login?error=3").await;
    page.ok()
        .has("class=\"flash error\"")
        .has("Zu dieser Anmeldung gibt es noch kein Konto.")
        .has("href=\"https://dav.example.org/frontend\"")
        .has("Ein neues Konto entsteht bei der ersten Anmeldung unter https://dav.example.org.");
    assert!(!page.body.contains("data-auto"), "a page that reports an error must not submit itself");
    // the other new code: an id the identity provider cannot have as an account
    app.anonymous().get("/login?error=4").await.ok().has("als Kontoname nicht erlaubt");
}

/// With sign-up on, the closing hint names this site, not RustiCal's, and no link to RustiCal is offered.
#[tokio::test]
async fn with_sign_up_the_login_page_says_the_account_is_created_here() {
    let app = TestApp::with(|c| {
        c.oidc = Some(crate::config::OidcSettings { allow_sign_up: true, ..crate::testkit::oidc_settings() });
        c.allow_password_login = false;
    })
    .await;
    let page = app.anonymous().get("/login?manual=1").await;
    page.ok().has("Ein neues Konto entsteht hier bei der ersten Anmeldung.");
    assert!(!page.body.contains("/frontend"), "nothing to do at RustiCal");
}

/// Where a `SignInError` sends the browser (`rustical_oidc` hands our response back unchanged).
fn location(e: crate::auth::SignInError) -> String {
    use axum::response::IntoResponse;
    let r = e.into_response();
    assert_eq!(r.status(), StatusCode::SEE_OTHER);
    r.headers().get("location").unwrap().to_str().unwrap().to_owned()
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

/// The CSRF extractors (`csrf::Token`, `csrf::TokenOrBasic`) run after the path and query and before the body:
/// a bad token is refused before the body is read, a bad path still answers 400.
#[tokio::test]
async fn the_csrf_token_is_checked_after_the_path_and_before_the_body() {
    async fn status(c: &mut crate::testkit::Client, token: &str, method: Method, path: &str, body: &'static str) -> StatusCode {
        c.send(method, path, &[("content-type", "application/json"), ("sec-fetch-site", "same-origin"), ("x-csrf-token", token)], Body::from(body)).await.status
    }
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let good = c.csrf.clone();
    for (method, path) in [(Method::PUT, "/api/alarm-presets"), (Method::POST, "/settings/task-columns")] {
        assert_eq!(status(&mut c, "wrong", method.clone(), path, "{not json").await, StatusCode::FORBIDDEN, "{path}: bad token and bad body");
        assert_eq!(status(&mut c, &good, method, path, "{not json").await, StatusCode::BAD_REQUEST, "{path}: bad body");
    }
    let path = format!("/api/events/alice/{}/o", "x".repeat(1025));
    assert_eq!(status(&mut c, "wrong", Method::DELETE, &path, "").await, StatusCode::BAD_REQUEST, "bad path and bad token");
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
            (Method::POST, "/settings/onboarding/confirm", r#"{"step":"tz"}"#),
            (Method::POST, "/settings/onboarding/complete", "null"),
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
