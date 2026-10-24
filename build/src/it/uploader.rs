//! PractiCalUploader (`domain::uploader`): the device tile and its setup only with `PRACTICAL__UPLOADER`, the
//! upload of a device's selection into managed read-only calendars (create, take over, restore, trash), the
//! device's deletion, and the read-only calendar against every PractiCal write path.

use crate::testkit::{Client, Resp, TestApp, vevent};
use axum::http::{Method, StatusCode};
use rustical_store::auth::AuthenticationProvider;
use serde_json::{Value, json};

async fn app(on: bool) -> TestApp {
    let app = TestApp::with(|c| c.uploader = on).await;
    app.user("alice", "Alice").await;
    app.user("bob", "Bob").await;
    app.group("team", "Team", &["alice"]).await;
    app
}

/// The text of `<code id="{id}">…</code>` (or `<pre id=…>`) on a page.
fn code(body: &str, id: &str) -> String {
    for tag in ["code", "pre"] {
        if let Some(at) = body.find(&format!("<{tag} id=\"{id}\">")) {
            let at = at + id.len() + tag.len() + 8;
            return body[at..at + body[at..].find(&format!("</{tag}>")).unwrap()].to_string();
        }
    }
    panic!("no {id}")
}

/// Sets up a device of the logged-in user for `owner`; returns its token from the block shown once.
async fn device(c: &mut Client, owner: &str) -> String {
    let r = c.post_form("/settings/uploader", &[("name", "PractiCalUploader"), ("owner", owner)]).await;
    assert_eq!(r.location().as_deref(), Some("/settings#add-uploader-wait"), "{}", r.body);
    let toml = code(&c.get("/settings").await.body, "up-toml").replace("&#34;", "\"");
    assert!(!toml.contains("calendar"), "{toml}");
    toml.lines().find_map(|l| l.strip_prefix("token = \"")).map(|v| v.trim_end_matches('"').to_string()).unwrap()
}

fn ev(uid: &str, summary: &str, start: &str) -> Value {
    json!({ "uid": uid, "ics": vevent(uid, summary, &format!("DTSTART:{start}\nDTEND:{start}")) })
}

fn cal(key: &str, name: &str, events: Vec<Value>) -> Value {
    json!({ "key": key, "name": name, "window": { "start": "2026-07-01T00:00:00Z", "end": "2027-10-01T00:00:00Z" }, "events": events })
}

async fn put(app: &TestApp, user: &str, token: &str, cals: Vec<Value>) -> Resp {
    use base64ct::Encoding;
    let auth = format!("Basic {}", base64ct::Base64::encode_string(format!("{user}:{token}").as_bytes()));
    let mut b = Client::for_router(app.router.clone());
    b.send(Method::PUT, "/api/uploader", &[("authorization", &auth), ("content-type", "application/json")], axum::body::Body::from(json!({ "calendars": cals }).to_string())).await
}

/// Refused credentials: /api/uploader checks them itself (`uploader_api::Device`) and answers 401.
fn refused(r: Resp) {
    assert_eq!(r.status, StatusCode::UNAUTHORIZED, "{}", r.body);
}

async fn ids(app: &TestApp, owner: &str, cal: &str) -> Vec<String> {
    let mut v: Vec<String> = rustical_store::CalendarReadStore::get_objects(&*app.state.calendars, owner, cal).await.unwrap().into_iter().map(|(id, _)| id).collect();
    v.sort();
    v
}

async fn calendar(app: &TestApp, owner: &str, id: &str) -> rustical_store::Calendar {
    rustical_store::CalendarReadStore::get_calendar(&*app.state.calendars, owner, id, true).await.unwrap()
}

#[tokio::test]
async fn without_the_flag_there_is_no_uploader() {
    let app = app(false).await;
    let mut c = app.login("alice").await;
    assert!(!c.get("/settings").await.body.contains("add-uploader"), "no tile, no page");
    assert_eq!(c.post_form("/settings/uploader", &[("name", "U"), ("owner", "alice")]).await.status, StatusCode::NOT_FOUND);
    let mut b = app.basic("alice").await;
    assert_eq!(b.json(Method::PUT, "/api/uploader", json!({ "calendars": [] })).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn setup_shows_the_block_once_and_makes_no_calendar() {
    let app = app(true).await;
    let mut c = app.login("alice").await;
    c.get("/settings").await.ok().has("href=\"#add-uploader\"").has("<select name=\"owner\">").has("<option value=\"team\">");
    let before = crate::domain::collections::calendars_for(&app.state, &app.principal("alice").await, false).await.unwrap().len();
    let token = device(&mut c, "alice").await;
    assert!(!c.get("/settings").await.body.contains(&token), "shown once");
    assert_eq!(crate::domain::collections::calendars_for(&app.state, &app.principal("alice").await, false).await.unwrap().len(), before);
    // a plain app token is no device; nobody else's either
    let mut b = app.basic("alice").await;
    refused(b.json(Method::PUT, "/api/uploader", json!({ "calendars": [] })).await);
    refused(put(&app, "bob", &token, vec![]).await);
    assert_eq!(app.anonymous().json(Method::PUT, "/api/uploader", json!({ "calendars": [] })).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(put(&app, "alice", &token, vec![]).await.ok().json(), json!({ "calendars": [], "trashed": [] }));
}

#[tokio::test]
async fn uploads_create_calendars_that_keep_their_practical_name() {
    let app = app(true).await;
    let mut c = app.login("alice").await;
    let token = device(&mut c, "alice").await;
    let old = ev("old@x", "Vergangen", "20260301T090000Z");
    let r = put(&app, "alice", &token, vec![
        json!({ "key": "k-work", "name": "Kalender", "window": { "start": "2026-01-01T00:00:00Z", "end": "2027-10-01T00:00:00Z" }, "events": [old, ev("a@x", "A", "20261020T090000Z"), ev("b/x", "B", "20261021T090000Z")] }),
        cal("k-hol", "Feiertage", vec![ev("h@x", "Feiertag", "20261003T000000Z")]),
    ]).await;
    let j = r.ok().json();
    assert_eq!(j["calendars"][0]["created"], 3, "{j}");
    let (work, hol) = (j["calendars"][0]["calendar"].as_str().unwrap().to_string(), j["calendars"][1]["calendar"].as_str().unwrap().to_string());
    assert_eq!(calendar(&app, "alice", &work).await.meta.displayname.as_deref(), Some("Kalender"));
    assert!(crate::domain::collections::is_read_only(&app.state, "alice", &work).await.unwrap());
    let infos = crate::domain::collections::calendars_for(&app.state, &app.principal("alice").await, false).await.unwrap();
    assert!(infos.iter().find(|i| i.id == hol).is_some_and(|i| i.managed && i.read_only));
    // renamed and recoloured in PractiCal: the next upload keeps that; same events → nothing written
    crate::domain::collections::update_calendar(&app.state, &app.principal("alice").await, "alice", &work, "Arbeit", "#123456", "").await.unwrap();
    let r = put(&app, "alice", &token, vec![
        json!({ "key": "k-work", "name": "Kalender", "window": { "start": "2026-01-01T00:00:00Z", "end": "2027-10-01T00:00:00Z" }, "events": [ev("old@x", "Vergangen", "20260301T090000Z"), ev("a@x", "A", "20261020T090000Z"), ev("b/x", "B", "20261021T090000Z")] }),
        cal("k-hol", "Feiertage", vec![ev("h@x", "Feiertag", "20261003T000000Z")]),
    ]).await;
    assert_eq!(r.ok().json()["calendars"][0]["unchanged"], 3);
    let w = calendar(&app, "alice", &work).await;
    assert_eq!((w.meta.displayname.as_deref(), w.meta.color.as_deref()), (Some("Arbeit"), Some("#123456")));
    // A changed, B gone, the old one before the (moved) window kept
    let r = put(&app, "alice", &token, vec![cal("k-work", "Kalender", vec![ev("a@x", "A neu", "20261020T090000Z")]), cal("k-hol", "Feiertage", vec![ev("h@x", "Feiertag", "20261003T000000Z")])]).await;
    assert_eq!(r.ok().json()["calendars"][0], json!({ "key": "k-work", "calendar": work, "created": 0, "updated": 1, "deleted": 1, "unchanged": 0 }));
    assert_eq!(ids(&app, "alice", &work).await, ["a@x", "old@x"], "a UID that is no plain path segment gets a digest id; it is gone with B");
    let h = c.get_json("/api/history").await.ok().json();
    assert!(h["changes"].as_array().unwrap().iter().all(|x| x["src"] == "uploader"), "{h}");
}

#[tokio::test]
async fn a_bad_upload_writes_nothing() {
    let app = app(true).await;
    let mut c = app.login("alice").await;
    let token = device(&mut c, "alice").await;
    let n = || async { crate::domain::collections::calendars_for(&app.state, &app.principal("alice").await, false).await.unwrap().len() };
    let before = n().await;
    let bad = json!({ "uid": "c@x", "ics": "BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n" });
    let r = put(&app, "alice", &token, vec![cal("k1", "Eins", vec![ev("z@x", "Z", "20261101T090000Z")]), cal("k2", "Zwei", vec![bad])]).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.body);
    let wrong_uid = json!({ "uid": "y@x", "ics": vevent("other@x", "Y", "DTSTART:20261101T090000Z") });
    assert_eq!(put(&app, "alice", &token, vec![cal("k1", "Eins", vec![wrong_uid])]).await.status, StatusCode::BAD_REQUEST);
    assert_eq!(put(&app, "alice", &token, vec![cal("k1", "Eins", vec![ev("d@x", "D", "20261101T090000Z"), ev("d@x", "D", "20261101T090000Z")])]).await.status, StatusCode::BAD_REQUEST);
    assert_eq!(put(&app, "alice", &token, vec![cal("k1", "Eins", vec![]), cal("k1", "Doppelt", vec![])]).await.status, StatusCode::BAD_REQUEST);
    let backwards = json!({ "key": "k1", "name": "Eins", "window": { "start": "2027-01-01T00:00:00Z", "end": "2026-01-01T00:00:00Z" }, "events": [] });
    assert_eq!(put(&app, "alice", &token, vec![backwards]).await.status, StatusCode::BAD_REQUEST);
    assert_eq!(n().await, before, "no calendar made");
}

#[tokio::test]
async fn deselected_calendars_go_to_the_trash_and_come_back() {
    let app = app(true).await;
    let mut c = app.login("alice").await;
    let token = device(&mut c, "alice").await;
    let j = put(&app, "alice", &token, vec![cal("a", "A", vec![]), cal("b", "B", vec![ev("e@x", "E", "20261020T090000Z")])]).await.ok().json();
    let b = j["calendars"][1]["calendar"].as_str().unwrap().to_string();
    let j = put(&app, "alice", &token, vec![cal("a", "A", vec![])]).await.ok().json();
    assert_eq!(j["trashed"], json!([b]));
    assert!(calendar(&app, "alice", &b).await.deleted_at.is_some());
    // purging a deselected one from the trash is allowed
    // selected again: restored, same calendar, events applied
    let j = put(&app, "alice", &token, vec![cal("a", "A", vec![]), cal("b", "B", vec![ev("e@x", "E", "20261020T090000Z"), ev("f@x", "F", "20261021T090000Z")])]).await.ok().json();
    assert_eq!(j["calendars"][1]["calendar"], json!(b));
    assert!(calendar(&app, "alice", &b).await.deleted_at.is_none());
    assert_eq!(ids(&app, "alice", &b).await, ["e@x", "f@x"]);
    // deselected and purged from the trash: selecting it again starts a new calendar
    put(&app, "alice", &token, vec![cal("a", "A", vec![])]).await.ok();
    c.post_form(&format!("/settings/calendars/alice/{b}/purge"), &[]).await;
    assert!(rustical_store::CalendarReadStore::get_calendar(&*app.state.calendars, "alice", &b, true).await.is_err(), "purged");
    let j = put(&app, "alice", &token, vec![cal("a", "A", vec![]), cal("b", "B", vec![])]).await.ok().json();
    assert_ne!(j["calendars"][1]["calendar"], json!(b));
    assert_eq!(j["calendars"][1]["created"], 0);
}

#[tokio::test]
async fn another_device_is_refused_and_an_orphan_is_taken_over() {
    let app = app(true).await;
    let mut c = app.login("alice").await;
    let one = device(&mut c, "alice").await;
    let two = device(&mut c, "alice").await;
    let j = put(&app, "alice", &one, vec![cal("k", "Kalender", vec![ev("e@x", "E", "20261020T090000Z")])]).await.ok().json();
    let k = j["calendars"][0]["calendar"].as_str().unwrap().to_string();
    let r = put(&app, "alice", &two, vec![cal("k", "Kalender", vec![])]).await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.body);
    assert_eq!(ids(&app, "alice", &k).await, ["e@x"], "nothing written");
    // a managed calendar cannot be deleted here
    c.post_form(&format!("/settings/calendars/alice/{k}/delete"), &[]).await;
    assert!(calendar(&app, "alice", &k).await.deleted_at.is_none(), "refused while managed");
    // device one goes, its calendar stays (read-only, no device) …
    let dev_one = sqlx::query_scalar::<_, String>("SELECT id FROM practical_uploader_devices ORDER BY created_at, rowid LIMIT 1").fetch_one(&app.state.db).await.unwrap();
    let page = c.get("/settings").await;
    page.ok().has(&format!("href=\"#upl-del-{dev_one}\"")).has(&format!("name=\"trash-{k}\""));
    c.post_form(&format!("/settings/uploader/{dev_one}/delete"), &[]).await;
    refused(put(&app, "alice", &one, vec![]).await); // its token is gone
    assert!(crate::domain::collections::is_read_only(&app.state, "alice", &k).await.unwrap());
    // … and device two takes it over: same calendar, PractiCal's name kept
    crate::domain::collections::update_calendar(&app.state, &app.principal("alice").await, "alice", &k, "Arbeit", "", "").await.unwrap();
    let j = put(&app, "alice", &two, vec![cal("k", "Kalender", vec![])]).await.ok().json();
    assert_eq!(j["calendars"][0]["calendar"], json!(k));
    assert_eq!(calendar(&app, "alice", &k).await.meta.displayname.as_deref(), Some("Arbeit"));
}

#[tokio::test]
async fn deleting_a_device_trashes_the_ticked_calendars_and_frees_the_rest() {
    let app = app(true).await;
    let mut c = app.login("alice").await;
    let token = device(&mut c, "alice").await;
    let j = put(&app, "alice", &token, vec![cal("a", "A", vec![]), cal("b", "B", vec![])]).await.ok().json();
    let (a, b) = (j["calendars"][0]["calendar"].as_str().unwrap().to_string(), j["calendars"][1]["calendar"].as_str().unwrap().to_string());
    let dev = sqlx::query_scalar::<_, String>("SELECT id FROM practical_uploader_devices").fetch_one(&app.state.db).await.unwrap();
    // another user cannot delete it
    let mut bob = app.login("bob").await;
    bob.post_form(&format!("/settings/uploader/{dev}/delete"), &[(&format!("trash-{a}"), "1")]).await;
    assert!(calendar(&app, "alice", &a).await.deleted_at.is_none());
    c.post_form(&format!("/settings/uploader/{dev}/delete"), &[(&format!("trash-{a}"), "1")]).await;
    assert!(calendar(&app, "alice", &a).await.deleted_at.is_some(), "ticked: in the trash");
    assert!(calendar(&app, "alice", &b).await.deleted_at.is_none(), "not ticked: stays");
    assert!(crate::domain::collections::is_read_only(&app.state, "alice", &b).await.unwrap(), "read-only still");
    refused(put(&app, "alice", &token, vec![]).await);
    // without a device it can be deleted
    c.post_form(&format!("/settings/calendars/alice/{b}/delete"), &[]).await;
    assert!(calendar(&app, "alice", &b).await.deleted_at.is_some());
    // a new device with the same keys brings both back from the trash
    let token = device(&mut c, "alice").await;
    let j = put(&app, "alice", &token, vec![cal("a", "A", vec![]), cal("b", "B", vec![])]).await.ok().json();
    assert_eq!((j["calendars"][0]["calendar"].clone(), j["calendars"][1]["calendar"].clone()), (json!(a), json!(b)));
    assert!(calendar(&app, "alice", &a).await.deleted_at.is_none() && calendar(&app, "alice", &b).await.deleted_at.is_none());
}

#[tokio::test]
async fn a_group_owns_what_its_device_uploads() {
    let app = app(true).await;
    let mut c = app.login("alice").await;
    let token = device(&mut c, "team").await;
    let j = put(&app, "alice", &token, vec![cal("t", "Team-Termine", vec![ev("e@x", "E", "20261020T090000Z")])]).await.ok().json();
    let t = j["calendars"][0]["calendar"].as_str().unwrap().to_string();
    assert_eq!(calendar(&app, "team", &t).await.meta.displayname.as_deref(), Some("Team-Termine"));
    assert_eq!(ids(&app, "team", &t).await, ["e@x"]);
    assert!(crate::domain::collections::is_read_only(&app.state, "team", &t).await.unwrap());
    // bob is no member: he cannot set up a device for the group
    let mut bob = app.login("bob").await;
    bob.post_form("/settings/uploader", &[("name", "U"), ("owner", "team")]).await;
    assert_eq!(sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM practical_uploader_devices WHERE principal = 'bob'").fetch_one(&app.state.db).await.unwrap(), 0);
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
    refused(c.json(Method::POST, &format!("{path}/move"), json!({ "days": 1 })).await);
    refused(c.delete(&path).await);
    refused(c.json(Method::POST, "/api/tasks", json!({ "principal": "alice", "cal_id": cal, "summary": "Neu" })).await);
    refused(c.delete(&format!("/api/tasks/alice/{cal}/t")).await);
    assert_eq!(c.get_json(&format!("/api/tasks/alice/{cal}/t")).await.ok().json()["read_only"], true);
    // a move out of it would delete it here
    let other = app.calendar("alice", "Privat", "#000000", &["VEVENT"]).await;
    // with a writable calendar beside it, a create aimed at the read-only one names the read-only one
    refused(c.json(Method::POST, "/api/events", json!({ "principal": "alice", "cal_id": cal, "summary": "Neu", "start": "2026-10-21T09:00:00", "end": "2026-10-21T10:00:00" })).await);
    let d = c.get_json(&path).await.json();
    refused(c.json(Method::PUT, &path, json!({ "principal": "alice", "cal_id": other, "summary": "Termin 2", "start": "2026-10-20T11:00:00", "end": "2026-10-20T12:00:00", "etag": d["etag"] })).await);
    assert!(ids(&app, "alice", &other).await.is_empty(), "no copy left in the target");
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

async fn get_info(app: &TestApp, user: &str, token: &str) -> Resp {
    use base64ct::Encoding;
    let auth = format!("Basic {}", base64ct::Base64::encode_string(format!("{user}:{token}").as_bytes()));
    let mut b = Client::for_router(app.router.clone());
    b.send(Method::GET, "/api/uploader", &[("authorization", &auth)], axum::body::Body::empty()).await
}

#[tokio::test]
async fn the_device_reads_its_own_status() {
    let app = app(true).await;
    let mut c = app.login("alice").await;
    let token = device(&mut c, "alice").await;
    let j = get_info(&app, "alice", &token).await.ok().json();
    assert_eq!((j["device"].as_str(), j["owner"].as_str(), j["calendars"].as_array().map(Vec::len)), (Some("PractiCalUploader"), Some("alice"), Some(0)));
    let up = put(&app, "alice", &token, vec![cal("a", "A", vec![]), cal("b", "B", vec![])]).await.ok().json();
    put(&app, "alice", &token, vec![cal("a", "A", vec![])]).await.ok();
    let j = get_info(&app, "alice", &token).await.ok().json();
    assert_eq!(j["calendars"], json!([
        { "key": "a", "calendar": up["calendars"][0]["calendar"], "name": "A", "trashed": false },
        { "key": "b", "calendar": up["calendars"][1]["calendar"], "name": "B", "trashed": true },
    ]));
    // reading changes nothing
    assert_eq!(get_info(&app, "alice", &token).await.ok().json(), j);
    // another app password of the user is no device; a wrong token is refused; without the flag: 404
    let r = c.post_form("/settings/tokens", &[("name", "phone")]).await;
    let other = code(&c.get("/settings").await.body, "new-token");
    assert_eq!(get_info(&app, "alice", &other).await.status, StatusCode::UNAUTHORIZED, "{}", r.body);
    refused(get_info(&app, "alice", "nope_nope").await);
    let off = self::app(false).await;
    let mut c = off.login("alice").await;
    c.post_form("/settings/tokens", &[("name", "phone")]).await;
    let valid = code(&c.get("/settings").await.body, "new-token");
    assert_eq!(get_info(&off, "alice", &valid).await.status, StatusCode::NOT_FOUND, "the flag is off");
}

#[tokio::test]
async fn the_wizard_waits_for_the_devices_first_request() {
    let app = app(true).await;
    let mut c = app.login("alice").await;
    let r = c.post_form("/settings/uploader", &[("name", "Mac"), ("owner", "alice")]).await;
    assert_eq!(r.location().as_deref(), Some("/settings#add-uploader-wait"));
    let page = c.get("/settings").await;
    let page = page.ok();
    let toml = code(&page.body, "up-toml").replace("&#34;", "\"");
    let token = toml.lines().find_map(|l| l.strip_prefix("token = \"")).map(|v| v.trim_end_matches('"').to_string()).unwrap();
    let dev = page.body.split("data-upl-device=\"").nth(1).and_then(|s| s.split('"').next()).unwrap().to_string();
    let path = format!("/settings/uploader/{dev}/status");
    assert_eq!(c.get(&path).await.ok().json(), json!({ "connected": false, "name": "Mac" }), "not before its first request");
    get_info(&app, "alice", &token).await.ok();
    assert_eq!(c.get(&path).await.ok().json(), json!({ "connected": true, "name": "Mac" }), "after it");
    // the details show once: a second view of step 3 says so instead
    assert!(!c.get("/settings").await.body.contains(&token));
    // another user cannot ask; a refused form goes back to the form, not to step 3
    let mut b = app.login("bob").await;
    assert_eq!(b.get(&format!("/settings/uploader/{dev}/status")).await.status, StatusCode::NOT_FOUND);
    assert_eq!(c.post_form("/settings/uploader", &[("name", " "), ("owner", "alice")]).await.location().as_deref(), Some("/settings#add-uploader"));
}

/// The page `#cal-<principal>-<id>` of the settings (its `<section>`).
fn cal_page<'a>(body: &'a str, principal: &str, cal: &str) -> &'a str {
    let at = body.find(&format!("id=\"cal-{principal}-{cal}\"")).expect("the calendar's page");
    &body[at..at + body[at..].find("</section>").unwrap()]
}

#[tokio::test]
async fn without_a_writable_calendar_nothing_is_created_or_imported() {
    let app = app(false).await;
    let cal = app.calendar("alice", "Arbeit", "#000000", &["VEVENT"]).await;
    let mut c = app.login("alice").await;
    let new = |cal: &str| json!({ "principal": "alice", "cal_id": cal, "summary": "Neu", "start": "2026-10-21T09:00:00", "end": "2026-10-21T10:00:00" });
    assert!(cal_page(&c.get("/settings").await.ok().body, "alice", &cal).contains("data-import="), "a writable calendar takes an import");
    crate::domain::collections::set_read_only(&app.state, "alice", &cal, "uploader").await.unwrap();
    assert!(!cal_page(&c.get("/settings").await.ok().body, "alice", &cal).contains("data-import="), "a read-only one offers none");
    // only read-only calendars: a create says that there is no calendar to write into
    let no_cal = |r: Resp| { assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.body); assert!(r.body.contains("einen Kalender an") || r.body.contains("Create a calendar"), "{}", r.body) };
    no_cal(c.json(Method::POST, "/api/events", new(&cal)).await);
    // ... as it does for a user without any calendar
    let mut b = app.login("bob").await;
    no_cal(b.json(Method::POST, "/api/events", json!({ "principal": "bob", "cal_id": "none", "summary": "Neu", "start": "2026-10-21T09:00:00", "end": "2026-10-21T10:00:00" })).await);
    // a writable calendar again: creating works
    let other = app.calendar("alice", "Privat", "#000000", &["VEVENT"]).await;
    c.json(Method::POST, "/api/events", new(&other)).await.ok();
}

/// Sends `method path` with HTTP Basic `user:token` and nothing else (no cookie, no CSRF header).
async fn basic_send(app: &TestApp, method: Method, path: &str, user: &str, token: &str, body: Option<Value>) -> Resp {
    use base64ct::Encoding;
    let auth = format!("Basic {}", base64ct::Base64::encode_string(format!("{user}:{token}").as_bytes()));
    let mut b = Client::for_router(app.router.clone());
    let body = body.map_or_else(axum::body::Body::empty, |v| axum::body::Body::from(v.to_string()));
    b.send(method, path, &[("authorization", &auth), ("content-type", "application/json"), ("accept", "application/json")], body).await
}

#[tokio::test]
async fn the_device_secret_opens_the_uploader_endpoint_and_nothing_else() {
    let app = app(true).await;
    let cal = app.calendar("alice", "Privat", "#000000", &["VEVENT"]).await;
    app.put_ics("alice", &cal, "e", &vevent("e", "Termin", "DTSTART:20261020T090000Z\nDTEND:20261020T100000Z")).await;
    let mut c = app.login("alice").await;
    let token = device(&mut c, "alice").await;
    // no RustiCal app token: CalDAV/CardDAV (same store) cannot accept it
    assert!(app.state.principals.get_app_tokens("alice").await.unwrap().is_empty(), "no app token made");
    assert!(app.state.principals.validate_app_token("alice", &token).await.unwrap().is_none());
    // the rest of PractiCal's API refuses it, reads and writes alike
    assert_eq!(basic_send(&app, Method::GET, "/api/history", "alice", &token, None).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(basic_send(&app, Method::GET, &format!("/api/events/alice/{cal}/e"), "alice", &token, None).await.status, StatusCode::UNAUTHORIZED);
    let r = basic_send(&app, Method::DELETE, &format!("/api/events/alice/{cal}/e"), "alice", &token, None).await;
    assert!(matches!(r.status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN), "{} {}", r.status, r.body);
    assert_eq!(ids(&app, "alice", &cal).await, ["e"], "nothing deleted");
    // the endpoint takes it; a cookie session alone does not count there, nor a user's app token
    get_info(&app, "alice", &token).await.ok();
    refused(c.send(Method::GET, "/api/uploader", &[("accept", "application/json")], axum::body::Body::empty()).await);
    refused(c.json(Method::PUT, "/api/uploader", json!({ "calendars": [] })).await);
    let app_token = crate::domain::tokens::create(&app.state, &app.principal("alice").await, "phone").await.unwrap();
    refused(get_info(&app, "alice", &app_token).await);
    refused(put(&app, "alice", &app_token, vec![]).await);
    // the right secret under another user name, or a wrong secret with the right prefix: refused
    refused(get_info(&app, "bob", &token).await);
    let (prefix, _) = token.split_once('_').unwrap();
    refused(get_info(&app, "alice", &format!("{prefix}_{}", "x".repeat(64))).await);
}

#[tokio::test]
async fn a_device_that_never_connects_is_swept_after_a_day() {
    let app = app(true).await;
    let mut c = app.login("alice").await;
    let used = device(&mut c, "alice").await;
    get_info(&app, "alice", &used).await.ok();
    let unused = device(&mut c, "alice").await;
    // both made 25 h ago: only the one that never connected goes
    sqlx::query("UPDATE practical_uploader_devices SET created_at = datetime('now', '-25 hours')").execute(&app.state.db).await.unwrap();
    assert_eq!(crate::domain::uploader::sweep(&app.state).await.unwrap(), 1);
    get_info(&app, "alice", &used).await.ok();
    refused(get_info(&app, "alice", &unused).await);
    // the connected one is listed with its last use, as a device row of its own
    let page = c.get("/settings").await;
    let dev = sqlx::query_scalar::<_, String>("SELECT id FROM practical_uploader_devices").fetch_one(&app.state.db).await.unwrap();
    assert!(page.body.contains("vor Kurzem") || page.body.contains("recently"), "its last use");
    page.ok().has(&format!("href=\"#upl-del-{dev}\""));
}

#[tokio::test]
async fn a_device_of_a_member_who_left_the_group_is_dead() {
    let app = app(true).await;
    app.state.principals.add_membership("bob", "team").await.unwrap();
    let mut c = app.login("alice").await;
    let token = device(&mut c, "team").await;
    let j = put(&app, "alice", &token, vec![cal("t", "Team-Termine", vec![])]).await.ok().json();
    let t = j["calendars"][0]["calendar"].as_str().unwrap().to_string();
    let dev = sqlx::query_scalar::<_, String>("SELECT id FROM practical_uploader_devices").fetch_one(&app.state.db).await.unwrap();
    assert!(crate::domain::uploader::managed(&app.state).await.unwrap().contains(&("team".to_string(), t.clone())));
    app.state.principals.remove_membership("alice", "team").await.unwrap();
    // alice's device: no upload, no status, its page offers no group calendar, a forged tick trashes nothing
    assert_eq!(put(&app, "alice", &token, vec![cal("t", "Team-Termine", vec![])]).await.status, StatusCode::FORBIDDEN);
    assert_eq!(get_info(&app, "alice", &token).await.status, StatusCode::FORBIDDEN);
    let page = c.get("/settings").await;
    page.ok().has(&format!("href=\"#upl-del-{dev}\""));
    assert!(!page.body.contains(&format!("name=\"trash-{t}\"")), "not hers to trash");
    c.post_form(&format!("/settings/uploader/{dev}/delete"), &[(&format!("trash-{t}"), "1")]).await;
    assert!(calendar(&app, "team", &t).await.deleted_at.is_none(), "the group's calendar stays");
    // the remaining member is not stuck with a calendar nobody can free
    assert!(!crate::domain::uploader::managed(&app.state).await.unwrap().contains(&("team".to_string(), t.clone())));
    let mut bob = app.login("bob").await;
    bob.post_form(&format!("/settings/calendars/team/{t}/delete"), &[]).await;
    assert!(calendar(&app, "team", &t).await.deleted_at.is_some(), "bob may delete it");
}

/// Overlapping uploads of one device (a retry while the first still runs): the history lock is taken before the
/// targets are looked up, so a later one finds the earlier one's calendar instead of making a second (old code:
/// the UNIQUE (principal, source_key) index refused its row → 500 and a stray writable calendar; these 4 parallel
/// uploads failed it in 5 of 5 runs, measured 2026-10-24).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn overlapping_uploads_make_one_calendar() {
    let app = app(true).await;
    let mut c = app.login("alice").await;
    let token = device(&mut c, "alice").await;
    let (user, dev) = crate::domain::uploader::authenticate(&app.state, "alice", &token).await.unwrap().unwrap();
    let body = || serde_json::from_value::<crate::domain::uploader::Upload>(json!({ "calendars": [cal("k", "Kalender", vec![ev("e@x", "E", "20261020T090000Z")])] })).unwrap();
    let runs: Vec<_> = (0..4).map(|_| { let (st, u, d, b) = (app.state.clone(), user.clone(), dev.clone(), body()); tokio::spawn(async move { crate::domain::uploader::upload(&st, &u, &d, b).await }) }).collect();
    let mut cals = std::collections::HashSet::new();
    for r in runs {
        let out = r.await.unwrap().expect("every upload succeeds");
        cals.insert(out.calendars[0].calendar.clone());
    }
    assert_eq!(cals.len(), 1, "one calendar for one key");
    let n = crate::domain::collections::calendars_for(&app.state, &app.principal("alice").await, false).await.unwrap().len();
    assert_eq!(n, 1, "no stray calendar");
}

#[tokio::test]
async fn a_deselected_calendar_restored_in_practical_stays() {
    let app = app(true).await;
    let mut c = app.login("alice").await;
    let token = device(&mut c, "alice").await;
    let j = put(&app, "alice", &token, vec![cal("a", "A", vec![]), cal("b", "B", vec![ev("e@x", "E", "20261020T090000Z")])]).await.ok().json();
    let b = j["calendars"][1]["calendar"].as_str().unwrap().to_string();
    put(&app, "alice", &token, vec![cal("a", "A", vec![])]).await.ok();
    assert!(calendar(&app, "alice", &b).await.deleted_at.is_some(), "deselected: in the trash");
    c.post_form(&format!("/settings/calendars/alice/{b}/restore"), &[]).await;
    assert!(calendar(&app, "alice", &b).await.deleted_at.is_none());
    // the next upload (still without it) leaves it alone: read-only, without a device, deletable
    assert_eq!(put(&app, "alice", &token, vec![cal("a", "A", vec![])]).await.ok().json()["trashed"], json!([]));
    assert!(calendar(&app, "alice", &b).await.deleted_at.is_none(), "not trashed again");
    assert!(crate::domain::collections::is_read_only(&app.state, "alice", &b).await.unwrap());
    assert!(!crate::domain::uploader::managed(&app.state).await.unwrap().contains(&("alice".to_string(), b.clone())));
    // selected again: the device takes it back, events and all
    let j = put(&app, "alice", &token, vec![cal("a", "A", vec![]), cal("b", "B", vec![ev("e@x", "E", "20261020T090000Z")])]).await.ok().json();
    assert_eq!(j["calendars"][1]["calendar"], json!(b));
    assert!(crate::domain::uploader::managed(&app.state).await.unwrap().contains(&("alice".to_string(), b.clone())));
}

#[tokio::test]
async fn a_page_with_a_secret_shown_once_is_never_cached() {
    let app = app(true).await;
    let mut c = app.login("alice").await;
    let cc = |r: &Resp| r.headers.get("cache-control").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    c.post_form("/settings/uploader", &[("name", "PractiCalUploader"), ("owner", "alice")]).await;
    let r = c.get("/settings").await;
    assert!(r.body.contains("id=\"up-toml\""), "the block");
    assert_eq!(cc(&r), "no-store");
    assert_eq!(cc(&c.get("/settings").await), "private, no-cache", "without a secret: the default");
    c.post_form("/settings/tokens", &[("name", "phone")]).await;
    let r = c.get("/settings").await;
    assert!(r.body.contains("id=\"new-token\""), "the app password");
    assert_eq!(cc(&r), "no-store");
}
