//! Shared calendars and address books through PractiCal's routes (`seed::seed_shared`): what a sharee lists,
//! reads and writes, what it never learns of the hidden parts, the visibility question before a write after
//! which its writer would see less, and the settings that share, unshare and set the levels.

use super::seed::seed_shared;
use axum::http::{Method, StatusCode};
use rustical_store::CalendarReadStore;
use serde_json::json;

const WINDOW: &str = "/api/events?start=2026-10-01&end=2026-11-01";

/// The stored object as its owner's store holds it (no view)
async fn stored(app: &crate::testkit::TestApp, owner: &str, cal: &str, id: &str) -> String {
    app.state.raw().calendars.get_object(owner, cal, id, false).await.unwrap().get_ics().to_string()
}

/// The editor's save body for an event detail (`GET /api/events/…`), as `eventBody()` sends it
fn event_body(d: &serde_json::Value, home: &str, name: &str) -> serde_json::Value {
    json!({
        "principal": home, "cal_id": name, "summary": d["summary"], "location": d["location"], "description": d["description"],
        "start": d["start"], "end": d["end"], "all_day": d["all_day"], "alarms": d["alarms"], "categories": d["categories"],
        "status": d["status"], "transp": d["transp"], "class": d["class"], "timezone": d["timezone"], "etag": d["etag"],
    })
}

#[tokio::test]
async fn sharees_list_their_copies() {
    let w = seed_shared().await;
    let app = &w.s.app;
    let erin = app.principal("erin").await;
    let cals = crate::domain::collections::calendars_for(&app.state, &erin, false).await.unwrap();
    let c = cals.iter().find(|c| c.auth.home() == "group.team").expect("the copy in the group's home");
    assert_eq!((c.id.as_str(), c.owner.as_str(), c.shared_by.as_deref(), c.read_only), (w.team_copy.as_str(), "bob", Some("Bob Muster"), false));
    let dave = app.principal("dave").await;
    let cals = crate::domain::collections::calendars_for(&app.state, &dave, false).await.unwrap();
    let c = cals.iter().find(|c| c.id == w.privat_copy).unwrap();
    assert!(c.read_only, "a read share");
    // the owner sees whom it is shared with; the sharee does not get the list
    let mut bob = app.login("bob").await;
    bob.get("/settings").await.ok().has("Team");
    // connecting a device: the group's account now shows a calendar (the copy), so erin is offered it
    let groups = crate::domain::groups::list_for_user(&app.state, &erin).await.unwrap();
    assert!(groups.iter().find(|g| g.id == "group.team").unwrap().needs_cal_account(), "bob → group.team: the group's account carries the copy");
}

#[tokio::test]
async fn a_read_copy_shows_only_what_the_owner_allows_and_takes_no_writes() {
    let w = seed_shared().await;
    let app = &w.s.app;
    let mut dave = app.login("dave").await;
    let win = dave.get_json(WINDOW).await.ok().body.clone();
    assert!(win.contains("Urlaub Ostsee"), "public: in full");
    assert!(!win.contains("Zahnarzt"), "private: the time only");
    assert!(dave.get_json("/api/search?q=zahnarzt").await.ok().json()["totals"]["events"] == 0);
    let path = format!("/api/events/dave/{}/urlaub", w.privat_copy);
    let d = dave.get_json(&path).await.ok().json();
    assert_eq!(d["read_only"], true);
    let mut body = event_body(&d, "dave", &w.privat_copy);
    body["summary"] = json!("Von Dave");
    let r = dave.json(Method::PUT, &path, body.clone()).await;
    assert!(r.status.is_client_error(), "{} {}", r.status, r.body);
    assert_ne!(r.status, StatusCode::CONFLICT, "no keep-mine question");
    let r = dave.json(Method::POST, "/api/events", body).await;
    assert!(r.status.is_client_error(), "{} {}", r.status, r.body);
    assert!(dave.delete(&path).await.status.is_client_error());
    assert!(stored(app, "alice", &w.s.privat, "urlaub").await.contains("SUMMARY:Urlaub Ostsee"));
}

#[tokio::test]
async fn hidden_parts_never_reach_the_sharee() {
    let w = seed_shared().await;
    let app = &w.s.app;
    let mut erin = app.login("erin").await;
    let win = erin.get_json(WINDOW).await.ok().body.clone();
    assert!(win.contains("Teamplan") && !win.contains("Marker7"), "{win}");
    let busy = format!("/api/events/group.team/{}/busy", w.team_copy);
    let d = erin.get_json(&busy).await;
    d.ok();
    assert!(!d.body.contains("Marker7"), "{}", d.body);
    assert_eq!(d.json()["read_only"], true, "seen shortened: not writable");
    assert_eq!(erin.get_json(&format!("/api/events/group.team/{}/gone", w.team_copy)).await.status, StatusCode::NOT_FOUND);
    let r = erin.get_json("/api/search?q=marker7").await.ok().json();
    assert_eq!(r["totals"]["events"], 0, "{r}");
    // the owner sees all of it
    let mut bob = app.login("bob").await;
    assert_eq!(bob.get_json("/api/search?q=marker7").await.ok().json()["totals"]["events"], 2);
}

#[tokio::test]
async fn a_write_copy_keeps_the_owners_reminder_and_asks_before_hiding() {
    let w = seed_shared().await;
    let app = &w.s.app;
    let mut erin = app.login("erin").await;
    let path = format!("/api/events/group.team/{}/plan", w.team_copy);
    let d = erin.get_json(&path).await.ok().json();
    assert_eq!((d["summary"].as_str(), d["read_only"].as_bool()), (Some("Teamplan"), Some(false)));
    assert!(!d.to_string().contains("Bobalarm"), "bob's reminder is his");
    let mut body = event_body(&d, "group.team", &w.team_copy);
    body["summary"] = json!("Teamplan neu");
    erin.json(Method::PUT, &path, body).await.ok();
    let ics = stored(app, "bob", &w.team, "plan").await;
    assert!(ics.contains("SUMMARY:Teamplan neu") && ics.contains("Bobalarm"), "{ics}");
    // private → erin would see only its time: asked first (JSON for the editor's question), nothing written
    let d = erin.get_json(&path).await.ok().json();
    let mut body = event_body(&d, "group.team", &w.team_copy);
    body["class"] = json!("PRIVATE");
    let r = erin.json(Method::PUT, &path, body.clone()).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{}", r.body);
    assert_eq!(r.json(), json!({ "code": "confirm-visibility", "level": 1 }));
    assert!(!stored(app, "bob", &w.team, "plan").await.contains("CLASS:PRIVATE"));
    body["confirm_visibility"] = json!(true);
    erin.json(Method::PUT, &path, body).await.ok();
    assert!(stored(app, "bob", &w.team, "plan").await.contains("CLASS:PRIVATE"));
    assert!(!erin.get_json(&path).await.ok().body.contains("Teamplan neu"), "shortened now");
    // an own calendar asks nothing
    let mut alice = app.login("alice").await;
    let own = format!("/api/events/alice/{}/utc", w.s.privat);
    let d = alice.get_json(&own).await.ok().json();
    let mut body = event_body(&d, "alice", &w.s.privat);
    body["class"] = json!("CONFIDENTIAL");
    alice.json(Method::PUT, &own, body).await.ok();
}

#[tokio::test]
async fn a_write_through_a_share_against_its_rules_is_told_not_asked() {
    let w = seed_shared().await;
    let app = &w.s.app;
    let mut erin = app.login("erin").await;
    // the private event erin sees only as a time: writing it whole would overwrite what she cannot see
    let path = format!("/api/events/group.team/{}/busy", w.team_copy);
    let d = erin.get_json(&path).await.ok().json();
    let mut body = event_body(&d, "group.team", &w.team_copy);
    body["class"] = json!("");
    body["summary"] = json!("Überschrieben");
    let r = erin.json(Method::PUT, &path, body).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{}", r.body);
    assert!(r.header("content-type").unwrap_or_default().starts_with("text/plain"), "a message, not the question");
    assert!(!r.body.contains("confirm-visibility") && !r.body.is_empty());
    assert!(stored(app, "bob", &w.team, "busy").await.contains("Marker7"));
}

#[tokio::test]
async fn tasks_and_contacts_through_a_write_copy() {
    let w = seed_shared().await;
    let app = &w.s.app;
    let mut erin = app.login("erin").await;
    let t = erin.get_json(&format!("/api/tasks/group.team/{}/todo", w.team_copy)).await.ok().json();
    assert_eq!(t["summary"], "Teamaufgabe");
    let cpath = format!("/api/contacts/group.team/{}/tina", w.team_ab_copy);
    let c = erin.get_json(&cpath).await.ok().json();
    assert_eq!(c["full_name"], "Tina Team");
    let erin_p = app.principal("erin").await;
    let abs = crate::domain::collections::addressbooks_for(&app.state, &erin_p, false).await.unwrap();
    assert!(abs.iter().any(|a| a.auth.home() == "group.team" && a.auth.owner() == "bob" && a.auth.id() == w.team_ab), "the copy is bob's book");
    // dave has no share of these: forbidden (not his home), not just empty
    let mut dave = app.login("dave").await;
    assert_eq!(dave.get_json(&cpath).await.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn settings_share_unshare_and_levels() {
    let w = seed_shared().await;
    let app = &w.s.app;
    // the sharee lookup names a group's members only to its members
    let mut erin = app.login("erin").await;
    assert_eq!(erin.get_json("/api/sharee?q=group.team").await.ok().json()["members"], json!(["erin"]));
    let mut bob = app.login("bob").await;
    let r = bob.get_json("/api/sharee?q=group.team").await.ok().json();
    assert_eq!((r["group"].as_bool(), r["members"].clone()), (Some(true), json!(null)));
    assert_eq!(bob.get_json("/api/sharee?q=nobody").await.status, StatusCode::BAD_REQUEST, "nobody by that name: told, not shared");
    // bob lowers his levels: private now hidden too; confidential above private is refused
    bob.post_form("/settings/visibility", &[("owner", "bob"), ("private", "0"), ("confidential", "0")]).await;
    let busy = format!("/api/events/group.team/{}/busy", w.team_copy);
    assert_eq!(erin.get_json(&busy).await.status, StatusCode::NOT_FOUND);
    bob.post_form("/settings/visibility", &[("owner", "bob"), ("private", "1"), ("confidential", "3")]).await;
    assert_eq!(app.state.collections().visibility("bob").await.unwrap(), crate::state::Policy::new(0, 0).unwrap());
    // nobody sets another's levels
    erin.post_form("/settings/visibility", &[("owner", "bob"), ("private", "4"), ("confidential", "4")]).await;
    assert_eq!(app.state.collections().visibility("bob").await.unwrap(), crate::state::Policy::new(0, 0).unwrap());
    // unshare: the copy is gone for its sharee
    let mut alice = app.login("alice").await;
    alice.post_form(&format!("/settings/calendars/alice/{}/unshare", w.s.privat), &[("sharee", "dave")]).await;
    let mut dave = app.login("dave").await;
    assert_eq!(dave.get_json(&format!("/api/events/dave/{}/urlaub", w.privat_copy)).await.status, StatusCode::NOT_FOUND);
    // only the owner shares
    erin.post_form(&format!("/settings/calendars/bob/{}/share", w.team), &[("sharee", "dave"), ("access", "write")]).await;
    let dave_p = app.principal("dave").await;
    assert!(crate::domain::collections::calendars_for(&app.state, &dave_p, false).await.unwrap().iter().all(|c| c.owner == "dave"), "nothing shared with dave now");
}

#[tokio::test]
async fn caches_follow_writes_on_both_sides_and_the_owners_levels() {
    let w = seed_shared().await;
    let app = &w.s.app;
    let mut erin = app.login("erin").await;
    let mut bob = app.login("bob").await;
    // read twice (the second from the cache), then the owner writes over DAV: the sharee sees it
    for _ in 0..2 {
        assert!(erin.get_json(WINDOW).await.ok().body.contains("Teamplan"));
    }
    assert_eq!(erin.get_json("/api/search?q=teamplan").await.ok().json()["totals"]["events"], 1);
    app.put_ics("bob", &w.team, "plan", &crate::testkit::vevent("plan", "Teamplan verschoben", "DTSTART;TZID=Europe/Berlin:20261021T100000\nDTEND;TZID=Europe/Berlin:20261021T110000")).await;
    assert!(erin.get_json(WINDOW).await.ok().body.contains("Teamplan verschoben"));
    // the sharee writes: the owner's caches follow
    bob.get_json(WINDOW).await.ok();
    let path = format!("/api/events/group.team/{}/plan", w.team_copy);
    let d = erin.get_json(&path).await.ok().json();
    let mut body = event_body(&d, "group.team", &w.team_copy);
    body["summary"] = json!("Teamplan von Erin");
    erin.json(Method::PUT, &path, body).await.ok();
    assert!(bob.get_json(WINDOW).await.ok().body.contains("Teamplan von Erin"));
    // the owner raises the CLASS: gone from the sharee's window and search
    app.put_ics("bob", &w.team, "plan", &crate::testkit::vevent("plan", "Teamplan von Erin", "DTSTART;TZID=Europe/Berlin:20261021T100000\nDTEND;TZID=Europe/Berlin:20261021T110000\nCLASS:CONFIDENTIAL")).await;
    assert!(!erin.get_json(WINDOW).await.ok().body.contains("Teamplan"));
    assert_eq!(erin.get_json("/api/search?q=teamplan").await.ok().json()["totals"]["events"], 0);
    // a change of the owner's levels changes what the cached views show
    assert!(!erin.get_json(WINDOW).await.ok().body.contains("Geheimprojekt"));
    bob.post_form("/settings/visibility", &[("owner", "bob"), ("private", "2"), ("confidential", "0")]).await;
    let win = erin.get_json(WINDOW).await.ok().body.clone();
    assert!(win.contains("Geheimprojekt Marker7") && !win.contains("Marker7-Keller"), "time and title, not the place: {win}");
}

#[tokio::test]
async fn history_names_the_sharee_and_tells_hidden_objects_to_nobody() {
    let w = seed_shared().await;
    let app = &w.s.app;
    let mut erin = app.login("erin").await;
    let path = format!("/api/events/group.team/{}/plan", w.team_copy);
    let d = erin.get_json(&path).await.ok().json();
    let mut body = event_body(&d, "group.team", &w.team_copy);
    body["summary"] = json!("Teamplan neu");
    erin.json(Method::PUT, &path, body).await.ok();
    // the owner's history: the change is erin's
    let mut bob = app.login("bob").await;
    let h = bob.get_json("/api/history").await.ok().json();
    let c = h["changes"].as_array().unwrap().iter().find(|c| c["items"][0]["title"] == "Teamplan neu").unwrap_or_else(|| panic!("{h}"));
    assert_eq!(c["who"], "erin");
    // the owner deletes the private and the confidential event: erin's history tells nothing of them
    bob.delete(&format!("/api/events/bob/{}/busy", w.team)).await.ok();
    bob.delete(&format!("/api/events/bob/{}/gone", w.team)).await.ok();
    let h = erin.get_json("/api/history").await.ok().body.clone();
    assert!(!h.contains("Marker7") && !h.contains("Verborgen"), "{h}");
}

#[tokio::test]
async fn personal_colour_actions_leave_a_shares_objects_alone() {
    let w = seed_shared().await;
    let app = &w.s.app;
    app.put_ics("bob", &w.team, "bunt", &crate::testkit::vevent("bunt", "Bunt", "DTSTART:20261022T100000Z\nDTEND:20261022T110000Z\nCOLOR:#112233")).await;
    let mut erin = app.login("erin").await;
    // the optimiser lists the object (it is in erin's view) but not as hers to recolour, and refuses a change to it
    let src = erin.get_json("/api/colors").await.ok().json();
    let it = src.as_array().or(src["sources"].as_array()).unwrap().iter().find(|s| s["key"].as_str().is_some_and(|k| k.ends_with("/bunt"))).cloned().unwrap_or_else(|| panic!("{src}"));
    assert_eq!(it["editable"], false, "{it}");
    let r = erin.json(Method::POST, "/api/colors/batch", json!({ "changes": [{ "kind": "event", "key": format!("group.team/{}/bunt", w.team_copy), "color": "#654321" }] })).await;
    assert_eq!(r.ok().json(), json!({ "count": 0, "failed": 1 }));
    assert!(stored(app, "bob", &w.team, "bunt").await.contains("COLOR:#112233"));
}

#[tokio::test]
async fn a_shortened_object_is_refused_with_a_message_not_asked_about() {
    let w = seed_shared().await;
    let app = &w.s.app;
    app.put_ics("bob", &w.team, "ptask", &crate::testkit::vtodo("ptask", "Private Aufgabe", "CLASS:PRIVATE")).await;
    let mut erin = app.login("erin").await;
    // the tick box of the task list on a task erin sees only shortened: the share's rule, as a message
    let r = erin.json(Method::PUT, &format!("/api/tasks/group.team/{}/ptask", w.team_copy), json!({ "principal": "group.team", "cal_id": w.team_copy, "summary": "", "completed": true })).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{}", r.body);
    assert!(!r.body.contains("confirm-visibility") && !r.body.is_empty(), "{}", r.body);
    assert!(!stored(app, "bob", &w.team, "ptask").await.contains("COMPLETED"));
}
