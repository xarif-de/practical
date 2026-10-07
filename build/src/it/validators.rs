//! ETag/304 of pages, fragments and JSON (etag.rs) and their content coding: an unchanged answer is
//! 304, and nothing that changes what the page shows — a write, the zone, the language, a flash,
//! another user — is ever answered 304.

use super::seed::seed;
use crate::testkit::{Client, Resp};
use axum::body::Body;
use axum::http::{Method, StatusCode};
use serde_json::json;

async fn get_if(c: &mut Client, path: &str, etag: &str) -> Resp {
    c.send(Method::GET, path, &[("accept", "text/html"), ("if-none-match", etag)], Body::empty()).await
}

fn etag(r: &Resp) -> String {
    r.header("etag").unwrap_or_else(|| panic!("no ETag: {:?}", r.headers))
}

#[tokio::test]
async fn pages_fragments_and_json_answer_304_when_unchanged() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    for path in [
        "/calendar?view=month&date=2026-10-14",
        "/calendar?view=week&date=2026-10-14&fragment=1",
        "/calendar?view=planner&date=2026-10-14",
        "/contacts",
        "/tasks",
        "/history",
        "/settings",
        "/api/history",
        "/api/events?start=2026-10-01&end=2026-11-01",
        "/api/search?q=Yoga",
    ] {
        let r = c.get(path).await;
        r.ok();
        let tag = etag(&r);
        assert!(tag.starts_with("W/\"") && tag.len() == 2 + 18, "{path}: {tag}");
        assert_eq!(r.header("cache-control").as_deref(), Some("private, no-cache"), "{path}: kept, but asked again every time");
        assert_eq!(r.header("vary").as_deref(), Some("accept-encoding"), "{path}");
        assert_eq!(etag(&c.get(path).await), tag, "{path}: the same answer, the same tag");
        let again = get_if(&mut c, path, &tag).await;
        assert_eq!((again.status, again.bytes.len()), (StatusCode::NOT_MODIFIED, 0), "{path}");
        assert_eq!((again.header("etag"), again.header("cache-control").as_deref(), again.header("content-type")), (Some(tag.clone()), Some("private, no-cache"), None), "{path}");
        // a list, the strong form of the weak tag (weak comparison), and a stale tag
        assert_eq!(get_if(&mut c, path, &format!("\"x\", {tag}")).await.status, StatusCode::NOT_MODIFIED, "{path}");
        assert_eq!(get_if(&mut c, path, &tag[2..]).await.status, StatusCode::NOT_MODIFIED, "{path}");
        let stale = get_if(&mut c, path, "W/\"AAAAAAAAAAAAAAAA\"").await;
        assert_eq!((stale.status, stale.bytes), (StatusCode::OK, r.bytes.clone()), "{path}");
    }
}

/// The tag is of the bytes sent: a write moves it, so the refetch after a live change and the next
/// visit get the new body — also /api/history and the history page (whose body names every change).
#[tokio::test]
async fn a_write_is_never_answered_304() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let paths = ["/calendar?view=month&date=2026-10-14", "/calendar?view=week&date=2026-10-14&fragment=1", "/history", "/api/history", "/api/events?start=2026-10-01&end=2026-11-01"];
    let mut tags = vec![];
    for p in paths {
        tags.push(etag(&c.get(p).await));
    }
    let input = json!({ "principal": "alice", "cal_id": s.privat, "summary": "Yoga", "start": "2026-10-23T18:00:00", "end": "2026-10-23T19:00:00" });
    let id = c.json(Method::POST, "/api/events", input.clone()).await.ok().json()["id"].as_str().unwrap().to_string();
    for (p, t) in paths.iter().zip(&tags) {
        let r = get_if(&mut c, p, t).await;
        r.ok().has("Yoga");
        assert_ne!(&etag(&r), t, "{p}");
    }
    // an edit of that event: again a new body for the views that show it
    let before = etag(&c.get("/calendar?view=month&date=2026-10-14").await);
    let mut upd = input;
    upd["summary"] = json!("Yoga im Park");
    upd["etag"] = c.get_json(&format!("/api/events/alice/{}/{id}", s.privat)).await.json()["etag"].clone();
    c.json(Method::PUT, &format!("/api/events/alice/{}/{id}", s.privat), upd).await.ok();
    get_if(&mut c, "/calendar?view=month&date=2026-10-14", &before).await.ok().has("Yoga im Park");
}

#[tokio::test]
async fn zone_language_and_user_change_the_tag() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let (page, json) = ("/calendar?view=week&date=2026-10-14", "/api/events?start=2026-10-14&end=2026-10-15");
    let (tp, tj) = (etag(&c.get(page).await), etag(&c.get_json(json).await));
    c.post_form_json("/settings/prefs", &[("timezone", "America/New_York")]).await.ok();
    get_if(&mut c, page, &tp).await.ok();
    let r = c.send(Method::GET, json, &[("accept", "application/json"), ("if-none-match", &tj)], Body::empty()).await;
    assert_eq!(r.ok().json()[0]["start"], "2026-10-14T03:00:00", "the standup at 09:00 Berlin, now in New York");
    let tp = etag(&c.get(page).await);
    c.post_form_json("/settings/prefs", &[("lang", "en")]).await.ok();
    get_if(&mut c, page, &tp).await.ok().has("<html lang=\"en\"");
    // the same URL of another user (bob shares the group calendar) is another body
    let tp = etag(&c.get(page).await);
    let mut bob = s.app.login("bob").await;
    get_if(&mut bob, page, &tp).await.ok().has("data-user=\"bob\"");
}

/// A flash (the toast after a form post) is part of the body: the page that shows it is never a 304
/// of the page without it, and the visit after it (toast gone) is not a 304 of the page with it.
#[tokio::test]
async fn a_flash_is_never_hidden_or_repeated_by_a_304() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let plain = etag(&c.get("/settings").await);
    let r = c.post_form("/settings/prefs", &[("vacation_days", "999")]).await;
    assert_eq!(r.location().as_deref(), Some("/settings#prefs"));
    let with = get_if(&mut c, "/settings", &plain).await;
    with.ok().has("<div class=\"toast ok\" role=\"status\"><span>Gespeichert.</span>");
    let after = get_if(&mut c, "/settings", &etag(&with)).await;
    assert_eq!(after.status, StatusCode::OK);
    assert!(!after.body.contains("class=\"toast"), "the flash is shown once");
    assert_eq!(get_if(&mut c, "/settings", &etag(&after)).await.status, StatusCode::NOT_MODIFIED);
}

#[tokio::test]
async fn brotli_with_gzip_fallback_and_one_weak_tag_for_all_codings() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let path = "/calendar?view=week&date=2026-10-14";
    let plain = c.get(path).await;
    let tag = etag(&plain);
    for (accept, coding) in [("gzip, deflate, br, zstd", Some("br")), ("br;q=1, gzip;q=0.5", Some("br")), ("gzip", Some("gzip")), ("identity", None)] {
        let r = c.send(Method::GET, path, &[("accept", "text/html"), ("accept-encoding", accept)], Body::empty()).await;
        assert_eq!((r.status, r.header("content-encoding").as_deref(), r.header("etag")), (StatusCode::OK, coding, Some(tag.clone())), "{accept}");
        let body = match coding {
            Some("br") => {
                let mut out = vec![];
                brotli::BrotliDecompress(&mut &r.bytes[..], &mut out).unwrap();
                out
            }
            Some(_) => {
                use std::io::Read;
                let mut out = vec![];
                flate2::read::GzDecoder::new(&r.bytes[..]).read_to_end(&mut out).unwrap();
                out
            }
            None => r.bytes.clone(),
        };
        // the CSRF token is the session's: the same bytes as the identity answer
        assert_eq!(body, plain.bytes, "{accept}");
        let nm = c.send(Method::GET, path, &[("accept", "text/html"), ("accept-encoding", accept), ("if-none-match", &tag)], Body::empty()).await;
        assert_eq!((nm.status, nm.bytes.len(), nm.header("content-encoding")), (StatusCode::NOT_MODIFIED, 0, None), "{accept}");
    }
}

/// Not touched: the event stream (never buffered), writes, errors, downloads, and the static files
/// (their own strong tags, routes/assets.rs).
#[tokio::test]
async fn streams_writes_errors_and_assets_keep_their_own_headers() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let (status, headers) = c.open("/api/changes").await;
    assert_eq!((status, headers.get("etag")), (StatusCode::OK, None), "the stream answers at once, unbuffered");
    let r = c.json(Method::POST, "/api/events", json!({ "principal": "alice", "cal_id": s.privat, "summary": "x", "start": "2026-10-23T18:00:00", "end": "2026-10-23T19:00:00" })).await;
    assert_eq!((r.status, r.header("etag")), (StatusCode::OK, None));
    let r = c.get_json("/api/events/alice/nope/nope").await;
    assert_eq!((r.status, r.header("etag")), (StatusCode::NOT_FOUND, None));
    let r = c.get(&format!("/settings/export/calendars/alice/{}", s.privat)).await;
    assert_eq!((r.status, r.header("etag")), (StatusCode::OK, None), "{:?}", r.header("content-type"));
    let r = c.get("/assets/app.js").await;
    assert!(r.header("etag").is_some_and(|t| !t.starts_with("W/")) && r.header("cache-control").as_deref() == Some("public, max-age=86400, must-revalidate"));
}
