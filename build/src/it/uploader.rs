//! PractiCalUploader (`domain::uploader`): the device tile and its setup only with `PRACTICAL__UPLOADER`, the
//! snapshot upload into its read-only calendar, and the read-only calendar against every PractiCal write path.

use crate::testkit::{TestApp, vevent};
use axum::http::{Method, StatusCode};
use serde_json::{Value, json};

async fn app(on: bool) -> TestApp {
    let app = TestApp::with(|c| c.uploader = on).await;
    app.user("alice", "Alice").await;
    app.user("bob", "Bob").await;
    app
}

/// The text of `<code id="{id}">…</code>` on a page.
fn code(body: &str, id: &str) -> String {
    let at = body.find(&format!("<code id=\"{id}\">")).unwrap_or_else(|| panic!("no {id}")) + id.len() + 12;
    body[at..at + body[at..].find("</code>").unwrap()].to_string()
}

fn ev(uid: &str, summary: &str, start: &str) -> Value {
    json!({ "uid": uid, "ics": vevent(uid, summary, &format!("DTSTART:{start}\nDTEND:{start}")) })
}

fn snap(events: Vec<Value>) -> Value {
    json!({ "window": { "start": "2026-07-01T00:00:00Z", "end": "2027-10-01T00:00:00Z" }, "events": events })
}

#[tokio::test]
async fn without_the_flag_there_is_no_uploader() {
    let app = app(false).await;
    let mut c = app.login("alice").await;
    let page = c.get("/settings").await;
    assert!(!page.body.contains("add-uploader"), "no tile, no page");
    assert_eq!(c.post_form("/settings/uploader", &[("name", "U"), ("calendar", "Arbeit")]).await.status, StatusCode::NOT_FOUND);
    let cal = app.calendar("alice", "Arbeit", "#000000", &["VEVENT"]).await;
    crate::domain::collections::set_read_only(&app.state, "alice", &cal, crate::domain::uploader::SOURCE).await.unwrap();
    let mut b = app.basic("alice").await;
    assert_eq!(b.json(Method::PUT, &format!("/api/uploader/calendars/{cal}"), snap(vec![])).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn setup_shows_the_values_once_and_the_upload_mirrors_the_snapshot() {
    let app = app(true).await;
    let mut c = app.login("alice").await;
    c.get("/settings").await.ok().has("href=\"#add-uploader\"").has("action=\"/settings/uploader\"");
    let r = c.post_form("/settings/uploader", &[("name", "PractiCalUploader"), ("calendar", "Arbeit")]).await;
    assert_eq!(r.location().as_deref(), Some("/settings#add-uploader"), "{}", r.body);
    let page = c.get("/settings").await;
    // the page shows only the configuration block (escaped quotes in HTML)
    let toml = code(&page.body, "up-toml").replace("&#34;", "\"");
    let val = |k: &str| toml.lines().find_map(|l| l.strip_prefix(&format!("{k} = \""))).map(|v| v.trim_end_matches('"').to_string()).unwrap();
    let (token, cal) = (val("token"), val("calendar"));
    assert_eq!(val("user"), "alice");
    assert!(!page.body.contains("id=\"up-token\""), "no separate values");
    assert!(!c.get("/settings").await.body.contains(&token), "shown once");
    assert!(crate::domain::collections::is_read_only(&app.state, "alice", &cal).await.unwrap());

    use base64ct::Encoding;
    let mut b = crate::testkit::Client::for_router(app.router.clone());
    let auth = format!("Basic {}", base64ct::Base64::encode_string(format!("alice:{token}").as_bytes()));
    let path = format!("/api/uploader/calendars/{cal}");
    let put = |b: &mut crate::testkit::Client, body: Value| {
        let (b, auth, path) = (b.clone(), auth.clone(), path.clone());
        async move { let mut b = b; b.send(Method::PUT, &path, &[("authorization", &auth), ("content-type", "application/json")], axum::body::Body::from(body.to_string())).await }
    };
    let old = ev("old@x", "Vergangen", "20260301T090000Z");
    let r = put(&mut b, json!({ "window": { "start": "2026-01-01T00:00:00Z", "end": "2027-10-01T00:00:00Z" }, "events": [old, ev("a@x", "A", "20261020T090000Z"), ev("b/x", "B", "20261021T090000Z")] })).await;
    assert_eq!(r.ok().json(), json!({ "created": 3, "updated": 0, "deleted": 0, "unchanged": 0 }));
    // the same again: nothing written; then A changed, B gone, the old one outside the (moved) window kept
    let r = put(&mut b, json!({ "window": { "start": "2026-01-01T00:00:00Z", "end": "2027-10-01T00:00:00Z" }, "events": [ev("old@x", "Vergangen", "20260301T090000Z"), ev("a@x", "A", "20261020T090000Z"), ev("b/x", "B", "20261021T090000Z")] })).await;
    assert_eq!(r.ok().json()["unchanged"], 3);
    let r = put(&mut b, snap(vec![ev("a@x", "A neu", "20261020T090000Z")])).await;
    assert_eq!(r.ok().json(), json!({ "created": 0, "updated": 1, "deleted": 1, "unchanged": 0 }));
    let ids: Vec<String> = rustical_store::CalendarReadStore::get_objects(&*app.state.calendars, "alice", &cal).await.unwrap().into_iter().map(|(id, _)| id).collect();
    let mut ids = ids;
    ids.sort();
    assert_eq!(ids, ["a@x", "old@x"], "a UID that is no plain path segment gets a digest id; it is gone with B");
    assert_eq!(crate::domain::uploader::object_id("b/x").len(), 66);
    // one change per upload in the history, source "uploader"
    let h = c.get_json("/api/history").await.ok().json();
    assert!(h["changes"].as_array().unwrap().iter().all(|x| x["src"] == "uploader"), "{h}");

    // a bad snapshot writes nothing
    let bad = json!({ "uid": "c@x", "ics": "BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n" });
    let r = put(&mut b, snap(vec![ev("z@x", "Z", "20261101T090000Z"), bad])).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.body);
    let wrong_uid = json!({ "uid": "y@x", "ics": vevent("other@x", "Y", "DTSTART:20261101T090000Z") });
    assert_eq!(put(&mut b, snap(vec![wrong_uid])).await.status, StatusCode::BAD_REQUEST);
    assert_eq!(put(&mut b, snap(vec![ev("d@x", "D", "20261101T090000Z"), ev("d@x", "D", "20261101T090000Z")])).await.status, StatusCode::BAD_REQUEST);
    assert_eq!(put(&mut b, json!({ "window": { "start": "2027-01-01T00:00:00Z", "end": "2026-01-01T00:00:00Z" }, "events": [] })).await.status, StatusCode::BAD_REQUEST);
    assert_eq!(rustical_store::CalendarReadStore::get_objects(&*app.state.calendars, "alice", &cal).await.unwrap().len(), 2);

    // only the user's own uploader calendar: another calendar of hers, and bob, get 404
    let plain = app.calendar("alice", "Privat", "#000000", &["VEVENT"]).await;
    let mut a = app.basic("alice").await;
    assert_eq!(a.json(Method::PUT, &format!("/api/uploader/calendars/{plain}"), snap(vec![])).await.status, StatusCode::NOT_FOUND);
    let mut bob = app.basic("bob").await;
    assert_eq!(bob.json(Method::PUT, &path, snap(vec![])).await.status, StatusCode::NOT_FOUND);
    assert_eq!(app.anonymous().json(Method::PUT, &path, snap(vec![])).await.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_read_only_calendar_takes_no_practical_write() {
    let app = app(true).await;
    let cal = app.calendar("alice", "Arbeit", "#000000", &["VEVENT", "VTODO"]).await;
    app.put_ics("alice", &cal, "e", &vevent("e", "Termin", "DTSTART:20261020T090000Z\nDTEND:20261020T100000Z")).await;
    app.put_ics("alice", &cal, "t", &crate::testkit::vtodo("t", "Aufgabe", "")).await;
    let mut c = app.login("alice").await;
    // writable before the mark: an update and its undo work (the undo is checked after the mark)
    let path = format!("/api/events/alice/{cal}/e");
    let d = c.get_json(&path).await.ok().json();
    assert_eq!(d["read_only"], false);
    let upd = json!({ "principal": "alice", "cal_id": cal, "summary": "Termin 2", "start": "2026-10-20T11:00:00", "end": "2026-10-20T12:00:00", "etag": d["etag"] });
    c.json(Method::PUT, &path, upd.clone()).await.ok();
    crate::domain::collections::set_read_only(&app.state, "alice", &cal, "uploader").await.unwrap();

    let d = c.get_json(&path).await.ok().json();
    assert_eq!(d["read_only"], true, "the editor opens it in view mode only");
    let refused = |r: crate::testkit::Resp| { assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.body); assert!(r.body.contains("read-only") || r.body.contains("schreibgeschützt"), "{}", r.body) };
    let mut upd = upd;
    upd["etag"] = d["etag"].clone();
    refused(c.json(Method::PUT, &path, upd).await);
    refused(c.json(Method::POST, "/api/events", json!({ "principal": "alice", "cal_id": cal, "summary": "Neu", "start": "2026-10-21T09:00:00", "end": "2026-10-21T10:00:00" })).await);
    refused(c.json(Method::POST, &format!("{path}/move"), json!({ "days": 1 })).await);
    refused(c.delete(&path).await);
    refused(c.json(Method::POST, "/api/tasks", json!({ "principal": "alice", "cal_id": cal, "summary": "Neu" })).await);
    refused(c.delete(&format!("/api/tasks/alice/{cal}/t")).await);
    assert_eq!(c.get_json(&format!("/api/tasks/alice/{cal}/t")).await.ok().json()["read_only"], true);
    // a move out of it would delete it here
    let other = app.calendar("alice", "Privat", "#000000", &["VEVENT"]).await;
    let d = c.get_json(&path).await.json();
    refused(c.json(Method::PUT, &path, json!({ "principal": "alice", "cal_id": other, "summary": "Termin 2", "start": "2026-10-20T11:00:00", "end": "2026-10-20T12:00:00", "etag": d["etag"] })).await);
    assert!(crate::domain::colors::set_object_color(&app.state, &app.principal("alice").await, "alice", &cal, "e", "#ff0000").await.is_err());
    assert!(crate::domain::transfer::import_calendar(&app.state, &app.principal("alice").await, "alice", &cal, vevent("i", "Import", "DTSTART:20261022T090000Z").as_bytes()).await.is_err());
    // the undo of the earlier update is blocked: no write access
    let h = c.get_json("/api/history").await.ok().json();
    let n = h["changes"].as_array().unwrap().last().unwrap()["id"].as_i64().unwrap();
    let plan = c.get_json(&format!("/api/history/{n}/plan")).await.ok().json();
    assert_eq!(plan["ok"], false, "{plan}");
    assert_eq!(c.get_json(&path).await.json()["summary"], "Termin 2", "nothing changed");
    // the editor's calendar picker offers it disabled, the calendar page marks it for calendar.js
    c.get("/calendar").await.ok().has(&format!("data-ro=\"alice/{cal}\""));
}
