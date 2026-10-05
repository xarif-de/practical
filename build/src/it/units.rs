//! Small public helpers of the web layer: CSRF token checks, the page context, error statuses,
//! the calendar page's query parsing.

use crate::ctx::PageCtx;
use crate::error::AppError;
use axum::http::StatusCode;
use axum::response::IntoResponse;

#[tokio::test]
async fn csrf_token_check() {
    let store = std::sync::Arc::new(tower_sessions::MemoryStore::default());
    let session = tower_sessions::Session::new(None, store, None);
    assert!(matches!(crate::csrf::check(&session, "").await, Err(AppError::Forbidden)), "no token in the session");
    session.insert(crate::ctx::SESSION_CSRF, "").await.unwrap();
    assert!(matches!(crate::csrf::check(&session, "").await, Err(AppError::Forbidden)), "an empty one never matches");
    session.insert(crate::ctx::SESSION_CSRF, "abc").await.unwrap();
    assert!(crate::csrf::check(&session, "abc").await.is_ok());
    assert!(matches!(crate::csrf::check(&session, "abd").await, Err(AppError::Forbidden)));
    let mut h = axum::http::HeaderMap::new();
    assert_eq!(crate::csrf::header_token(&h), "");
    h.insert("x-csrf-token", "t1".parse().unwrap());
    assert_eq!(crate::csrf::header_token(&h), "t1");
    let t = crate::ctx::random_token();
    assert!(t.len() == 32 && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
}

fn ctx(name: Option<&str>, id: &str, path: &str) -> PageCtx {
    let user = rustical_store::auth::Principal { id: id.into(), displayname: name.map(str::to_string), principal_type: Default::default(), password: None, memberships: vec![] };
    PageCtx { lang: "de".into(), user: Some(user), csrf: String::new(), path: path.into(), flash: None, i18n: crate::i18n::I18n::load(), public_dav_url: String::new(), push_enabled: false, asset_v: "", dev: false, wants_json: false, scheme: "auto".into() }
}

#[test]
fn page_context_helpers() {
    assert_eq!(ctx(Some("Anna Maria Schmidt"), "a", "/").initials(), "AM");
    assert_eq!(ctx(Some("jean-luc.picard"), "a", "/").initials(), "JL");
    assert_eq!(ctx(Some("X"), "a", "/").initials(), "X");
    assert_eq!(ctx(None, "bob", "/").initials(), "BO");
    assert_eq!(ctx(Some("Olaf"), "a", "/").initials(), "OL", "one word: its first two letters");
    // BUG?: the length check counts bytes, so a one-word name starting with a non-ASCII letter keeps
    // a single initial ("Ö", 2 bytes) where "Olaf" gets two
    assert_eq!(ctx(Some("Ölaf"), "a", "/").initials(), "Ö");
    let c = ctx(None, "bob", "/settings/prefs");
    assert!(c.is_active("/settings") && !c.is_active("/set") && ctx(None, "b", "/settings").is_active("/settings"));
    assert_eq!((c.user_id(), c.locale(), c.version(), c.rustical_version()), ("bob", "de-DE", env!("CARGO_PKG_VERSION"), env!("RUSTICAL_TAG")));
    assert_eq!(c.rustical_source_url(), format!("https://github.com/lennart-k/rustical/tree/v{}", env!("RUSTICAL_TAG")));
    assert_eq!(c.t1("login-with", "name", "SSO"), "Anmelden mit SSO");
    assert_eq!(c.tn("set-regions-n", "n", 4), "4 Regionen");
    let js = c.js_i18n();
    assert!(!js.contains('<'));
    let v: serde_json::Value = serde_json::from_str(&js).unwrap();
    assert!(v.as_object().unwrap().keys().all(|k| k.starts_with("js-")));
    assert_eq!(c.languages().len(), 4);
    assert_eq!(PageCtx { user: None, ..c }.user_id(), "");
}

#[tokio::test]
async fn error_statuses_and_texts() {
    for (e, status, body) in [
        (AppError::NotFound, StatusCode::NOT_FOUND, "not found"),
        (AppError::Store(rustical_store::Error::NotFound), StatusCode::NOT_FOUND, "store error: Not found"),
        (AppError::Forbidden, StatusCode::FORBIDDEN, "forbidden"),
        (AppError::Unauthorized, StatusCode::UNAUTHORIZED, "unauthorized"),
        (AppError::BadRequest("x".into()), StatusCode::BAD_REQUEST, "bad request: x"),
        (AppError::Conflict("y".into()), StatusCode::CONFLICT, "conflict: y"),
        (AppError::Store(rustical_store::Error::AlreadyExists), StatusCode::CONFLICT, "store error: Resource already exists and overwrite=false"),
        (AppError::Internal("z".into()), StatusCode::INTERNAL_SERVER_ERROR, "z"),
        (AppError::Db(sqlx::Error::RowNotFound), StatusCode::INTERNAL_SERVER_ERROR, "database error: no rows returned by a query that expected to return at least one row"),
    ] {
        let r = e.into_response();
        assert_eq!(r.status(), status);
        let b = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
        assert_eq!(String::from_utf8_lossy(&b), body);
    }
}

#[test]
fn calendar_query_helpers() {
    use crate::routes::calendar::{parse_cals, view_label};
    assert_eq!(parse_cals(None), None);
    assert_eq!(parse_cals(Some("")), None);
    assert_eq!(parse_cals(Some("a/1,b/2,a/1")).unwrap().len(), 2);
    let l = crate::testkit::lang("de");
    let labels: Vec<(String, bool)> = ["day", "week", "month", "planner", "weekends", "vacation", "countdown", "numbers", "x"].iter().map(|v| view_label(v, &l)).collect();
    assert_eq!(labels, [("Tag".to_string(), false), ("Woche".into(), false), ("Monat".into(), false), ("Jahresplaner".into(), false), ("Freie Wochenenden".into(), true), ("Urlaubsplaner".into(), true), ("Countdown-Wand".into(), true), ("Jahr in Zahlen".into(), true), ("Monat".into(), false)]);
}
