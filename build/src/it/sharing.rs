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
    // the owner sees whom it is shared with and manages it; the sharee gets neither the list nor its forms
    let mut bob = app.login("bob").await;
    let page = bob.get("/settings").await.ok().body.clone();
    let own = &page[page.find(&format!("id=\"cal-bob-{}\"", w.team)).expect("Team's page")..];
    let own = &own[..own.find("</section>").unwrap()];
    assert!(own.contains("share-row") && own.contains("value=\"group.team\"") && own.contains("/unshare\"") && own.contains("share-add") && own.contains("/visibility\""), "{own}");
    let mut erin_c = app.login("erin").await;
    let page = erin_c.get("/settings").await.ok().body.clone();
    let copy = &page[page.find(&format!("id=\"cal-group.team-{}\"", w.team_copy)).expect("the copy's page")..];
    let copy = &copy[..copy.find("</section>").unwrap()];
    assert!(copy.contains("Bob Muster"), "who shared it: {copy}");
    for f in ["share-row", "share-add", "/unshare", "/share\"", "/visibility", "vis-form"] {
        assert!(!copy.contains(f), "no {f} for the copy: {copy}");
    }
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
    let tpath = format!("/api/tasks/group.team/{}/todo", w.team_copy);
    let t = erin.get_json(&tpath).await.ok().json();
    assert_eq!(t["summary"], "Teamaufgabe");
    let mut body = task_body(&t);
    body["summary"] = json!("Teamaufgabe von Erin");
    erin.json(Method::PUT, &tpath, body).await.ok();
    assert!(stored(app, "bob", &w.team, "todo").await.contains("SUMMARY:Teamaufgabe von Erin"));
    erin.json(Method::PUT, &tpath, json!({ "principal": "group.team", "cal_id": w.team_copy, "summary": "", "completed": true })).await.ok();
    assert!(stored(app, "bob", &w.team, "todo").await.contains("STATUS:COMPLETED"), "the tick box too");
    let cpath = format!("/api/contacts/group.team/{}/tina", w.team_ab_copy);
    let c = erin.get_json(&cpath).await.ok().json();
    assert_eq!((c["full_name"].as_str(), c["read_only"].as_bool()), (Some("Tina Team"), Some(false)));
    let mut body = c.clone();
    body["full_name"] = json!("Tina Teamleitung");
    erin.json(Method::PUT, &cpath, body).await.ok();
    assert!(stored_card(app, "bob", &w.team_ab, "tina").await.contains("FN:Tina Teamleitung"));
    let id = erin.json(Method::POST, "/api/contacts", json!({ "principal": "group.team", "ab_id": w.team_ab_copy, "full_name": "Neu im Team" })).await.ok().json()["id"].as_str().unwrap().to_string();
    assert!(stored_card(app, "bob", &w.team_ab, &id).await.contains("FN:Neu im Team"), "a new contact lands in bob's book");
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
    // the levels are the calendar's: bob lowers Team's, private now hidden too; confidential above private is refused
    let levels = format!("/settings/calendars/bob/{}/visibility", w.team);
    let team_levels = || async {
        let bob = app.principal("bob").await;
        let a = crate::domain::collections::ensure_calendar(&app.state, &bob, "bob", &w.team, crate::state::Need::Own).await.unwrap();
        app.state.collections().visibility(&a).await.unwrap()
    };
    bob.get("/settings").await.ok().has(&format!("action=\"{levels}\""));
    bob.post_form(&levels, &[("private", "0"), ("confidential", "0")]).await;
    let busy = format!("/api/events/group.team/{}/busy", w.team_copy);
    assert_eq!(erin.get_json(&busy).await.status, StatusCode::NOT_FOUND);
    bob.post_form(&levels, &[("private", "1"), ("confidential", "3")]).await;
    assert_eq!(team_levels().await, crate::state::Policy::new(0, 0).unwrap());
    // nobody sets another's levels, and a calendar without a setting hides both
    erin.post_form(&levels, &[("private", "4"), ("confidential", "4")]).await;
    assert_eq!(team_levels().await, crate::state::Policy::new(0, 0).unwrap());
    let alice_p = app.principal("alice").await;
    let arbeit = crate::domain::collections::ensure_calendar(&app.state, &alice_p, "alice", &w.s.arbeit, crate::state::Need::Own).await.unwrap();
    assert_eq!(app.state.collections().visibility(&arbeit).await.unwrap(), crate::state::Policy::new(0, 0).unwrap());
    // unshare: the copy is gone for its sharee
    let mut alice = app.login("alice").await;
    alice.post_form(&format!("/settings/calendars/alice/{}/unshare", w.s.privat), &[("sharee", "dave")]).await;
    let mut dave = app.login("dave").await;
    assert_eq!(dave.get_json(&format!("/api/events/dave/{}/urlaub", w.privat_copy)).await.status, StatusCode::NOT_FOUND);
    // the access switch of a share line from before changes nothing: the ended share stays ended
    alice.post_form(&format!("/settings/calendars/alice/{}/access", w.s.privat), &[("sharee", "dave"), ("access", "write")]).await;
    // only the owner shares
    erin.post_form(&format!("/settings/calendars/bob/{}/share", w.team), &[("sharee", "dave"), ("access", "write")]).await;
    let dave_p = app.principal("dave").await;
    assert!(crate::domain::collections::calendars_for(&app.state, &dave_p, false).await.unwrap().iter().all(|c| c.owner == "dave"), "nothing shared with dave now");
    // on a share that exists the switch changes its access, and only the owner's does
    let team_ro = || async {
        let erin_p = app.principal("erin").await;
        let cals = crate::domain::collections::calendars_for(&app.state, &erin_p, false).await.unwrap();
        cals.iter().find(|c| c.principal == "group.team" && c.id == w.team_copy).map(|c| c.read_only)
    };
    assert_eq!(team_ro().await, Some(false));
    erin.post_form(&format!("/settings/calendars/bob/{}/access", w.team), &[("sharee", "group.team"), ("access", "read")]).await;
    assert_eq!(team_ro().await, Some(false), "a sharee does not change the access");
    bob.post_form(&format!("/settings/calendars/bob/{}/access", w.team), &[("sharee", "group.team"), ("access", "read")]).await;
    assert_eq!(team_ro().await, Some(true));
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
    bob.post_form(&format!("/settings/calendars/bob/{}/visibility", w.team), &[("private", "2"), ("confidential", "0")]).await;
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

#[tokio::test]
async fn levels_belong_to_the_calendar_not_its_owner() {
    let w = seed_shared().await;
    let app = &w.s.app;
    let mut alice = app.login("alice").await;
    alice.post_form(&format!("/settings/calendars/alice/{}/share", w.s.arbeit), &[("sharee", "dave"), ("access", "read")]).await;
    alice.post_form(&format!("/settings/calendars/alice/{}/visibility", w.s.arbeit), &[("private", "2"), ("confidential", "1")]).await;
    // the editor's hint per calendar: Arbeit as set, Privat (shared too) hidden
    let a = app.principal("alice").await;
    let cals = crate::domain::collections::calendars_for(&app.state, &a, false).await.unwrap();
    let vis = |id: &str| cals.iter().find(|c| c.id == id).unwrap().vis;
    assert_eq!((vis(&w.s.arbeit), vis(&w.s.privat)), (Some((2, 1)), Some((0, 0))));
    // what dave sees follows each calendar: the confidential review shows its time, the private dentist nothing
    let mut dave = app.login("dave").await;
    let win = dave.get_json(WINDOW).await.ok().body.clone();
    assert!(!win.contains("Zahnarzt") && !win.contains("Quartalsreview"), "{win}");
    let arbeit_copy: String = {
        let d = app.principal("dave").await;
        crate::domain::collections::calendars_for(&app.state, &d, false).await.unwrap().into_iter().find(|c| c.owner == "alice" && c.auth.id() == w.s.arbeit).unwrap().id
    };
    assert_eq!(dave.get_json(&format!("/api/events/dave/{arbeit_copy}/review")).await.status, StatusCode::OK);
    assert_eq!(dave.get_json(&format!("/api/events/dave/{}/zahnarzt", w.privat_copy)).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn suggestions_only_by_what_people_let_others_find() {
    use rustical_store::auth::AuthenticationProvider;
    let w = seed_shared().await;
    let app = &w.s.app;
    app.state.principals().set_principal_email("erin", Some("erin@example.org")).await.unwrap();
    let mut bob = app.login("bob").await;
    let values = |r: serde_json::Value| r.as_array().unwrap().iter().map(|s| s["value"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    // from two characters on; by id, by a word of the name, by the address; never the user, never a group
    assert_eq!(bob.get_json("/api/sharee/suggest?q=e").await.ok().json(), json!([]));
    let r = bob.get_json("/api/sharee/suggest?q=er").await.ok().json();
    assert_eq!(r, json!([{ "value": "erin", "name": "Erin Schreiber", "id": "erin", "email": "erin@example.org" }]));
    assert_eq!(values(bob.get_json("/api/sharee/suggest?q=Schr").await.ok().json()), ["erin"]);
    assert_eq!(values(bob.get_json("/api/sharee/suggest?q=erin%40ex").await.ok().json()), ["erin"]);
    assert!(values(bob.get_json("/api/sharee/suggest?q=bo").await.ok().json()).is_empty(), "not the user");
    assert!(values(bob.get_json("/api/sharee/suggest?q=gr").await.ok().json()).is_empty(), "no groups");
    // erin keeps name and id out: found by the address only, which then goes into the field and stands for the name
    let mut erin = app.login("erin").await;
    erin.post_form("/settings/privacy", &[("listed_by_email", "1")]).await;
    assert!(values(bob.get_json("/api/sharee/suggest?q=er").await.ok().json()) == ["erin@example.org"], "by the address prefix");
    assert!(values(bob.get_json("/api/sharee/suggest?q=Schr").await.ok().json()).is_empty(), "not by the name");
    let r = bob.get_json("/api/sharee/suggest?q=erin%40").await.ok().json();
    assert_eq!(r, json!([{ "value": "erin@example.org", "name": "erin@example.org", "id": null, "email": "erin@example.org" }]));
    // and the address out as well: not suggested at all, yet sharing by the whole id still works
    erin.post_form("/settings/privacy", &[]).await;
    assert!(values(bob.get_json("/api/sharee/suggest?q=er").await.ok().json()).is_empty());
    assert_eq!(bob.get_json("/api/sharee?q=erin").await.ok().json()["id"], "erin");
    // back on: the default
    erin.post_form("/settings/privacy", &[("listed_by_id", "1"), ("listed_by_email", "1")]).await;
    let p = crate::domain::prefs::get(&app.state.db, "erin").await.unwrap();
    assert_eq!((p.unlisted_by_id, p.unlisted_by_email), (false, false));
}

#[tokio::test]
async fn privacy_page_lists_each_calendars_levels() {
    let w = seed_shared().await;
    let app = &w.s.app;
    let mut bob = app.login("bob").await;
    let page = bob.get("/settings").await.ok().body.clone();
    let privacy = &page[page.find("id=\"privacy\"").expect("the privacy page")..];
    let privacy = &privacy[..privacy.find("</section>").unwrap()];
    // Team is set to 1/0 (seed); the row leads to the calendar's page, where the levels are changed
    assert!(privacy.contains(&format!("href=\"#cal-bob-{}\"", w.team)), "{privacy}");
    assert!(privacy.contains("privat: Nur die Zeit · vertraulich: Gar nicht"), "{privacy}");
    assert!(privacy.contains("name=\"listed_by_id\" value=\"1\" checked") && privacy.contains("name=\"listed_by_email\" value=\"1\" checked"), "both on by default");
}

/// The task editor's save body for a task detail (`GET /api/tasks/…`), as `taskBody()` sends it
fn task_body(d: &serde_json::Value) -> serde_json::Value {
    json!({
        "principal": d["principal"], "cal_id": d["cal_id"], "summary": d["summary"], "description": d["description"], "location": d["location"],
        "start": d["start"], "start_all_day": d["start_all_day"], "due": d["due"], "due_all_day": d["due_all_day"], "status": d["status"],
        "percent": d["percent"], "completed_at": d["completed_at"], "priority": d["priority"], "class": d["class"], "categories": d["categories"],
        "color": d["color"], "geo": d["geo"], "alarms": d["alarms"], "links": d["links"], "related": d["related"], "etag": d["etag"],
    })
}

#[tokio::test]
async fn an_unknown_class_survives_every_save() {
    let w = seed_shared().await;
    let app = &w.s.app;
    app.put_ics("bob", &w.team, "xs", &crate::testkit::vevent("xs", "Geheim Marker7", "DTSTART;TZID=Europe/Berlin:20261019T100000\nDTEND;TZID=Europe/Berlin:20261019T110000\nCLASS:X-SECRET")).await;
    app.put_ics("bob", &w.team, "xt", &crate::testkit::vtodo("xt", "Geheimaufgabe Marker7", "DUE;VALUE=DATE:20261021\nCLASS:X-SECRET")).await;
    let mut bob = app.login("bob").await;
    let mut erin = app.login("erin").await;
    let hidden = |s: &str| !s.contains("Marker7");
    // the editor reads the value as written and sends it back: kept through a change of the title
    let path = format!("/api/events/bob/{}/xs", w.team);
    let d = bob.get_json(&path).await.ok().json();
    assert_eq!(d["class"], "X-SECRET");
    let mut body = event_body(&d, "bob", &w.team);
    body["summary"] = json!("Geheim neu Marker7");
    bob.json(Method::PUT, &path, body).await.ok();
    assert!(stored(app, "bob", &w.team, "xs").await.contains("CLASS:X-SECRET"));
    assert!(hidden(&erin.get_json(WINDOW).await.ok().body), "private to erin as before (Team: time only)");
    // a task: the editor's save and the tick box
    let tpath = format!("/api/tasks/bob/{}/xt", w.team);
    let t = bob.get_json(&tpath).await.ok().json();
    assert_eq!(t["class"], "X-SECRET");
    let mut body = task_body(&t);
    body["summary"] = json!("Geheimaufgabe neu Marker7");
    bob.json(Method::PUT, &tpath, body).await.ok();
    assert!(stored(app, "bob", &w.team, "xt").await.contains("CLASS:X-SECRET"));
    bob.json(Method::PUT, &tpath, json!({ "principal": "bob", "cal_id": w.team, "summary": "", "completed": true })).await.ok();
    let ics = stored(app, "bob", &w.team, "xt").await;
    assert!(ics.contains("CLASS:X-SECRET") && ics.contains("STATUS:COMPLETED"), "{ics}");
    let tasks = erin.get_json(&format!("/api/tasks/group.team/{}/xt", w.team_copy)).await;
    assert!(hidden(&tasks.body), "{}", tasks.body);
    // the lists mark it private, as sharees get it: the task row's lock, the day view's mark
    let page = bob.get("/calendar?view=day&date=2026-10-19").await.ok().body.clone();
    assert!(page.contains("Geheim neu Marker7") && page.contains("#i-lock\"/></svg>Privat</span>"), "the day view marks it private");
    // a malformed CLASS saves as PRIVATE, not as nothing (= PUBLIC)
    app.put_ics("bob", &w.team, "kaputt", &crate::testkit::vevent("kaputt", "Kaputt Marker7", "DTSTART;TZID=Europe/Berlin:20261019T120000\nDTEND;TZID=Europe/Berlin:20261019T130000\nCLASS:X_SECRET")).await;
    let kpath = format!("/api/events/bob/{}/kaputt", w.team);
    let k = bob.get_json(&kpath).await.ok().json();
    assert_eq!(k["class"], "PRIVATE");
    let mut body = event_body(&k, "bob", &w.team);
    body["summary"] = json!("Kaputt neu Marker7");
    bob.json(Method::PUT, &kpath, body).await.ok();
    assert!(stored(app, "bob", &w.team, "kaputt").await.contains("CLASS:PRIVATE"));
    assert!(hidden(&erin.get_json(WINDOW).await.ok().body));
    // an explicit pick replaces it
    let d = bob.get_json(&path).await.ok().json();
    let mut body = event_body(&d, "bob", &w.team);
    body["class"] = json!("PUBLIC");
    bob.json(Method::PUT, &path, body).await.ok();
    let ics = stored(app, "bob", &w.team, "xs").await;
    assert!(ics.contains("CLASS:PUBLIC") && !ics.contains("X-SECRET"), "{ics}");
}

#[tokio::test]
async fn the_hint_knows_the_strictest_class_of_a_series() {
    let w = seed_shared().await;
    let app = &w.s.app;
    let ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//t//EN\r\nBEGIN:VEVENT\r\nUID:ser\r\nDTSTAMP:20261001T000000Z\r\nSUMMARY:Serie\r\nDTSTART:20261005T100000Z\r\nDTEND:20261005T110000Z\r\nRRULE:FREQ=WEEKLY;COUNT=4\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:ser\r\nDTSTAMP:20261001T000000Z\r\nRECURRENCE-ID:20261012T100000Z\r\nSUMMARY:Serie\r\nDTSTART:20261012T120000Z\r\nDTEND:20261012T130000Z\r\nCLASS:CONFIDENTIAL\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    app.put_ics("bob", &w.team, "ser", ics).await;
    let mut bob = app.login("bob").await;
    let d = bob.get_json(&format!("/api/events/bob/{}/ser", w.team)).await.ok().json();
    assert_eq!((d["class"].as_str(), d["class_overrides"].as_str()), (Some(""), Some("CONFIDENTIAL")));
    // RustiCal agrees: the whole series is confidential to erin (Team hides confidential)
    let mut erin = app.login("erin").await;
    assert!(!erin.get_json(WINDOW).await.ok().body.contains("Serie"));
}

#[tokio::test]
async fn whom_an_address_names_and_the_member_field_keep_to_what_people_allow() {
    use rustical_store::auth::AuthenticationProvider;
    let w = seed_shared().await;
    let app = &w.s.app;
    app.state.principals().set_principal_email("erin", Some("erin@example.org")).await.unwrap();
    let mut bob = app.login("bob").await;
    let mut erin = app.login("erin").await;
    let values = |r: serde_json::Value| r.as_array().unwrap().iter().map(|s| s["value"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    // every typed word starts a word of the name
    assert_eq!(values(bob.get_json("/api/sharee/suggest?q=erin%20sch").await.ok().json()), ["erin"]);
    assert_eq!(values(bob.get_json("/api/sharee/suggest?q=Schreiber%20Er").await.ok().json()), ["erin"]);
    assert!(values(bob.get_json("/api/sharee/suggest?q=erin%20x").await.ok().json()).is_empty());
    // listed by id only: an address finds nothing, the id finds her without the address
    erin.post_form("/settings/privacy", &[("listed_by_id", "1")]).await;
    assert!(values(bob.get_json("/api/sharee/suggest?q=erin%40").await.ok().json()).is_empty());
    assert_eq!(bob.get_json("/api/sharee/suggest?q=er").await.ok().json(), json!([{ "value": "erin", "name": "Erin Schreiber", "id": "erin", "email": null }]));
    // listed by address only: the confirmation names the address, not her name or id; the share goes by the address
    erin.post_form("/settings/privacy", &[("listed_by_email", "1")]).await;
    assert_eq!(bob.get_json("/api/sharee?q=erin%40example.org").await.ok().json(), json!({ "id": null, "name": "erin@example.org", "group": false, "members": null }));
    assert_eq!(bob.get_json("/api/sharee?q=erin").await.ok().json()["id"], "erin", "the whole id still names her");
    let cal = app.calendar("bob", "Notizen", "#112233", &["VEVENT"]).await;
    let r = bob.post_form(&format!("/settings/calendars/bob/{cal}/share"), &[("sharee", "erin@example.org"), ("access", "read")]).await;
    assert_eq!(r.status, StatusCode::SEE_OTHER, "{}", r.body);
    let erin_p = app.principal("erin").await;
    assert!(crate::domain::collections::calendars_for(&app.state, &erin_p, false).await.unwrap().iter().any(|c| c.owner == "bob" && c.auth.id() == cal));
    // what a page may show of her: never her id (as a word: "lettering" holds "erin" too); unlisted by id the address
    // and not her name, unlisted by both her name and not the address
    let no_id = |page: &str| !page.replace("erin@example.org", "").split(|c: char| !c.is_alphanumeric()).any(|w| w == "erin");
    // a line's or a row's key in a page: the first person key after `at`
    let key_after = |page: &str, at: &str| { let sec = &page[page.find(at).unwrap_or_else(|| panic!("no {at}"))..]; let i = sec.find("~").unwrap(); sec[i..].split(['"', '/']).next().unwrap().to_string() };
    let page = bob.get("/settings").await.ok().body.clone();
    assert!(no_id(&page) && page.contains("erin@example.org") && !page.contains("Erin Schreiber"), "bob's settings, unlisted by id");
    let key = key_after(&page, &format!("id=\"cal-bob-{cal}\""));
    assert!(key.len() == 33 && key[1..].bytes().all(|b| b.is_ascii_hexdigit()), "{key}");
    // unlisted by both: her name, not the address
    erin.post_form("/settings/privacy", &[]).await;
    let page = bob.get("/settings").await.ok().body.clone();
    assert!(no_id(&page) && !page.contains("erin@example.org") && page.contains("Erin Schreiber"), "bob's settings, unlisted by both");
    assert_eq!(key_after(&page, &format!("id=\"cal-bob-{cal}\"")), key, "the same key for the same session");
    erin.post_form("/settings/privacy", &[("listed_by_email", "1")]).await;
    // its line's forms work with the key: the access switch, then the end of the share
    let a = crate::domain::collections::ensure_calendar(&app.state, &app.principal("bob").await, "bob", &cal, crate::state::Need::Own).await.unwrap();
    bob.post_form(&format!("/settings/calendars/bob/{cal}/access"), &[("sharee", &key), ("access", "write")]).await;
    assert_eq!(app.state.collections().calendar_shares(&a).await.unwrap().iter().map(|s| (s.sharee.clone(), s.access)).collect::<Vec<_>>(), [("erin".to_string(), crate::state::Access::Write)]);
    // a key of another collection's line, or of another session, does nothing
    let zweit = app.calendar("bob", "Zweit", "#223344", &["VEVENT"]).await;
    bob.post_form(&format!("/settings/calendars/bob/{zweit}/share"), &[("sharee", "dave"), ("access", "read")]).await;
    let r = bob.post_form_json(&format!("/settings/calendars/bob/{zweit}/unshare"), &[("sharee", &key)]).await;
    assert!(r.json()["ok"] == false && r.body.contains("nicht mehr"), "{} {}", r.status, r.body);
    let other = crate::domain::groups::person_key("another session", "erin");
    let r = bob.post_form_json(&format!("/settings/calendars/bob/{cal}/unshare"), &[("sharee", &other)]).await;
    assert!(r.json()["ok"] == false, "{} {}", r.status, r.body);
    assert_eq!(app.state.collections().calendar_shares(&a).await.unwrap().len(), 1);
    bob.post_form_json(&format!("/settings/calendars/bob/{cal}/unshare"), &[("sharee", &key)]).await.ok();
    assert!(app.state.collections().calendar_shares(&a).await.unwrap().is_empty());
    // a stale line (the share has ended): told it is gone, not reported as done
    let r = bob.post_form_json(&format!("/settings/calendars/bob/{cal}/unshare"), &[("sharee", &key)]).await;
    assert!(r.json()["ok"] == false && r.body.contains("nicht mehr"), "{} {}", r.status, r.body);
    let r = bob.post_form_json(&format!("/settings/calendars/bob/{cal}/access"), &[("sharee", &key), ("access", "read")]).await;
    assert!(r.json()["ok"] == false, "{} {}", r.status, r.body);
    // a group's member field lists nobody; a member is added by id or by address
    let mut alice = app.login("alice").await;
    let page = alice.get("/settings").await.ok().body.clone();
    assert!(!page.contains("value=\"carol\"") && !page.contains("Carol Test"), "nobody listed for the picking");
    let r = alice.post_form("/settings/groups/group.familie/members", &[("member", "erin@example.org")]).await;
    assert_eq!(r.status, StatusCode::SEE_OTHER, "{}", r.body);
    alice.post_form("/settings/groups/group.familie/members", &[("member", "carol")]).await;
    let mut members = app.state.principals().list_members("group.familie").await.unwrap();
    members.sort();
    assert_eq!(members, ["alice", "bob", "carol", "erin"]);
    // the other members' pages list her by the address, without her id; unlisted by both by her name only
    for who in ["alice", "bob"] {
        let page = app.login(who).await.get("/settings").await.ok().body.clone();
        assert!(no_id(&page) && page.contains("erin@example.org") && !page.contains("Erin Schreiber"), "{who}'s settings");
    }
    erin.post_form("/settings/privacy", &[]).await;
    let page = alice.get("/settings").await.ok().body.clone();
    assert!(no_id(&page) && !page.contains("erin@example.org") && page.contains("Erin Schreiber"), "alice's settings, unlisted by both");
    // a group is no member, nor is nobody
    alice.post_form("/settings/groups/group.familie/members", &[("member", "group.team")]).await;
    alice.post_form("/settings/groups/group.familie/members", &[("member", "niemand@example.org")]).await;
    assert_eq!(app.state.principals().list_members("group.familie").await.unwrap().len(), 4);
    // the row's remove form: a key of another session names nobody; the page's own removes her
    let row_key = key_after(&page, "id=\"groups\"");
    alice.post_form(&format!("/settings/groups/group.familie/members/{other}/remove"), &[]).await;
    assert_eq!(app.state.principals().list_members("group.familie").await.unwrap().len(), 4);
    alice.post_form(&format!("/settings/groups/group.familie/members/{row_key}/remove"), &[]).await;
    assert!(!app.state.principals().list_members("group.familie").await.unwrap().contains(&"erin".to_string()));
    // a person nobody may find: "someone" when they have no name either
    app.state.principals().insert_principal(rustical_store::auth::Principal { displayname: None, ..app.principal("erin").await }, true).await.unwrap();
    alice.post_form("/settings/groups/group.familie/members", &[("member", "erin")]).await;
    let page = alice.get("/settings").await.ok().body.clone();
    assert!(no_id(&page) && !page.contains("erin@example.org") && page.contains("eine Person"), "the neutral word");
}

#[test]
fn hmac_rfc4231() {
    // RFC 4231 test case 2
    let mac = crate::domain::groups::hmac_sha256(b"Jefe", b"what do ya want for nothing?");
    assert_eq!(mac.iter().map(|b| format!("{b:02x}")).collect::<String>(), "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843");
}

/// The stored card as its owner's store holds it
async fn stored_card(app: &crate::testkit::TestApp, owner: &str, ab: &str, id: &str) -> String {
    rustical_store::AddressbookReadStore::get_object(app.state.raw().addressbooks, owner, ab, id, false).await.unwrap().get_vcf().to_string()
}

#[tokio::test]
async fn a_read_share_takes_no_write_of_any_kind() {
    let w = seed_shared().await;
    let app = &w.s.app;
    let mut alice = app.login("alice").await;
    alice.post_form(&format!("/settings/calendars/alice/{}/share", w.s.aufgaben), &[("sharee", "dave"), ("access", "read")]).await;
    let mut dave = app.login("dave").await;
    let dave_p = app.principal("dave").await;
    let tasks_copy = crate::domain::collections::calendars_for(&app.state, &dave_p, false).await.unwrap().into_iter().find(|c| c.owner == "alice" && c.auth.id() == w.s.aufgaben).unwrap().id;
    let (card0, ics0, task0) = (stored_card(app, "bob", &w.team_ab, "tina").await, stored(app, "alice", &w.s.privat, "urlaub").await, stored(app, "alice", &w.s.aufgaben, "steuer").await);
    let refused = |r: crate::testkit::Resp| {
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.body);
        assert!(r.body.contains("nur zum Lesen"), "the share's message, not a bare 403: {}", r.body);
    };
    // the contact: read-only in its detail, and every write says why it is refused
    let cpath = format!("/api/contacts/dave/{}/tina", w.team_ab_dave_copy);
    let c = dave.get_json(&cpath).await.ok().json();
    assert_eq!(c["read_only"], true);
    let mut body = c.clone();
    body["full_name"] = json!("Von Dave");
    body["ab_id"] = json!(w.team_ab_dave_copy);
    refused(dave.json(Method::PUT, &cpath, body.clone()).await);
    refused(dave.json(Method::POST, "/api/contacts", body).await);
    refused(dave.delete(&cpath).await);
    refused(dave.post_raw(&format!("/settings/import/addressbooks/dave/{}", w.team_ab_dave_copy), "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:tina\r\nFN:Überschrieben\r\nEND:VCARD\r\n").await);
    // the calendar: drag and drop, the tick box, an .ics import
    let r = dave.json(Method::POST, &format!("/api/events/dave/{}/urlaub/move", w.privat_copy), json!({ "days": 1 })).await;
    assert!(r.status.is_client_error(), "{} {}", r.status, r.body);
    let r = dave.json(Method::PUT, &format!("/api/tasks/dave/{tasks_copy}/steuer"), json!({ "principal": "dave", "cal_id": tasks_copy, "summary": "", "completed": true })).await;
    assert!(r.status.is_client_error(), "{} {}", r.status, r.body);
    let r = dave.post_raw(&format!("/settings/import/calendars/dave/{}", w.privat_copy), &crate::testkit::vevent("urlaub", "Überschrieben", "DTSTART;VALUE=DATE:20261026\nDTEND;VALUE=DATE:20261031")).await;
    assert!(r.status.is_client_error(), "{} {}", r.status, r.body);
    assert_eq!((stored_card(app, "bob", &w.team_ab, "tina").await, stored(app, "alice", &w.s.privat, "urlaub").await, stored(app, "alice", &w.s.aufgaben, "steuer").await), (card0, ics0, task0), "nothing changed");
}

#[tokio::test]
async fn shares_are_listed_with_their_owner_and_are_not_mine() {
    let w = seed_shared().await;
    let app = &w.s.app;
    let mut dave = app.login("dave").await;
    // the filter rows and the editor's select name the owner; "only mine" is offered (there are shares)
    let cal = dave.get("/calendar?date=2026-10-14").await.ok().body.clone();
    assert!(cal.contains("<small>Alice Beispiel</small>") && cal.contains("data-filter=\"mine\""), "{cal}");
    assert!(cal.contains(&format!("value=\"dave/{}\"", w.privat_copy)) && cal.contains("(Alice Beispiel)</option>"), "the editor's select");
    // the contacts: the read copy is labelled, and no new or moved contact can be put there
    let page = dave.get("/contacts").await.ok().body.clone();
    assert!(page.contains("<small>Bob Muster</small>"), "the filter row");
    assert!(page.contains(&format!("value=\"dave/{}\" disabled>Teamkontakte (Bob Muster)</option>", w.team_ab_dave_copy)), "{page}");
    // the table view (only it has cells to edit in place): none for the read copy's contact; erin's write copy has them
    let table = dave.get("/contacts?view=table").await.ok().body.clone();
    assert!(table.contains("Tina Team") && !table.contains("contenteditable"), "no table cell to edit in place: {table}");
    let mut erin = app.login("erin").await;
    let table = erin.get("/contacts?view=table").await.ok().body.clone();
    assert!(table.contains("Tina Team") && table.contains("contenteditable data-f=\"full_name\""), "a writable row has them");
    // the settings' list says who shared it, not the user's own name
    let set = dave.get("/settings").await.ok().body.clone();
    assert!(!set.contains("Dave Leser · "), "{set}");
}

#[tokio::test]
async fn reminders_into_a_share_are_asked_about_first() {
    let w = seed_shared().await;
    let app = &w.s.app;
    let eigen = app.calendar("erin", "Eigen", "#123456", &["VEVENT"]).await;
    app.put_ics("erin", &eigen, "mit", &crate::testkit::vevent("mit", "Mit Erinnerung", "DTSTART;TZID=Europe/Berlin:20261022T100000\nDTEND;TZID=Europe/Berlin:20261022T110000\nBEGIN:VALARM\nACTION:DISPLAY\nDESCRIPTION:Erinalarm\nTRIGGER:-PT5M\nEND:VALARM")).await;
    let mut erin = app.login("erin").await;
    let asked = |r: &crate::testkit::Resp| r.status == StatusCode::UNPROCESSABLE_ENTITY && r.json() == json!({ "code": "confirm-alarms" });
    // moving her own event with a reminder into the share: asked, nothing moved
    let path = format!("/api/events/erin/{eigen}/mit");
    let d = erin.get_json(&path).await.ok().json();
    assert_eq!(d["alarms"].as_array().map(Vec::len), Some(1));
    let mut body = event_body(&d, "group.team", &w.team_copy);
    let r = erin.json(Method::PUT, &path, body.clone()).await;
    assert!(asked(&r), "{} {}", r.status, r.body);
    assert!(stored(app, "erin", &eigen, "mit").await.contains("Erinalarm"), "still hers");
    // confirmed: moved, without her reminder
    body["confirm_alarms"] = json!(true);
    let id = erin.json(Method::PUT, &path, body).await.ok().json()["id"].as_str().unwrap().to_string();
    let ics = stored(app, "bob", &w.team, &id).await;
    assert!(ics.contains("SUMMARY:Mit Erinnerung") && !ics.contains("Erinalarm"), "{ics}");
    // a new one in the share with a reminder: the same question; without one: none
    let mut new = event_body(&d, "group.team", &w.team_copy);
    new["summary"] = json!("Neu geteilt");
    let r = erin.json(Method::POST, "/api/events", new.clone()).await;
    assert!(asked(&r), "{} {}", r.status, r.body);
    new["alarms"] = json!([]);
    erin.json(Method::POST, "/api/events", new).await.ok();
    // a task too
    let r = erin.json(Method::POST, "/api/tasks", json!({ "principal": "group.team", "cal_id": w.team_copy, "summary": "Geteilte Aufgabe", "alarms": [{ "minutes": 10 }] })).await;
    assert!(asked(&r), "{} {}", r.status, r.body);
    // an own calendar asks nothing
    let mut own = event_body(&d, "erin", &eigen);
    own["summary"] = json!("Eigen mit Erinnerung");
    erin.json(Method::POST, "/api/events", own).await.ok();
}

#[tokio::test]
async fn an_import_through_a_share_skips_what_the_share_refuses() {
    let w = seed_shared().await;
    let app = &w.s.app;
    let mut erin = app.login("erin").await;
    // two new events; "busy" erin sees only as its time (refused: Level), "gone" not at all (refused: ForeignUid)
    let ev = |uid: &str, title: &str| format!("BEGIN:VEVENT\r\nUID:{uid}\r\nDTSTAMP:20261001T000000Z\r\nSUMMARY:{title}\r\nDTSTART:20261023T100000Z\r\nDTEND:20261023T110000Z\r\nEND:VEVENT\r\n");
    let file = format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//t//EN\r\n{}{}{}{}END:VCALENDAR\r\n", ev("neu1", "Import eins"), ev("busy", "Überschrieben"), ev("gone", "Überschrieben"), ev("neu2", "Import zwei"));
    let r = erin.post_raw(&format!("/settings/import/calendars/group.team/{}", w.team_copy), &file).await;
    assert_eq!(r.ok().json(), json!({ "imported": 2, "refused": 2 }));
    assert!(stored(app, "bob", &w.team, "neu1").await.contains("Import eins") && stored(app, "bob", &w.team, "neu2").await.contains("Import zwei"), "the ones after a refused one too");
    assert!(stored(app, "bob", &w.team, "busy").await.contains("Marker7") && stored(app, "bob", &w.team, "gone").await.contains("CLASS:CONFIDENTIAL"));
}

#[tokio::test]
async fn the_history_keys_a_path_as_ensure_resolves_it() {
    let w = seed_shared().await;
    let app = &w.s.app;
    let c = app.state.collections();
    // a share: the owner's calendar; an own one: itself
    assert_eq!(c.history_key(false, "group.team", &w.team_copy).await, ("bob".to_string(), w.team.clone()));
    assert_eq!(c.history_key(true, "group.team", &w.team_ab_copy).await, ("bob".to_string(), w.team_ab.clone()));
    assert_eq!(c.history_key(false, "alice", &w.s.arbeit).await, ("alice".to_string(), w.s.arbeit.clone()));
    // an own calendar under the name of a share row in the same home is what the path means (ensure: own first).
    // The store refuses to create one (rustical_store_sqlite calendar_store `calendar_name_shared`): written by SQL here
    let mut tx = app.state.db.begin().await.unwrap();
    for q in ["CREATE TEMP TABLE c AS SELECT * FROM calendars WHERE principal = 'alice' AND id = ?1", "UPDATE c SET principal = 'dave', id = ?2, push_topic = 'own-under-share-name'", "INSERT INTO calendars SELECT * FROM c", "DROP TABLE c"] {
        sqlx::query(q).bind(&w.s.arbeit).bind(&w.privat_copy).execute(&mut *tx).await.unwrap();
    }
    tx.commit().await.unwrap();
    assert_eq!(c.history_key(false, "dave", &w.privat_copy).await, ("dave".to_string(), w.privat_copy.clone()));
    // nothing there any more (a purged collection): the path itself
    assert_eq!(c.history_key(false, "dave", "weg").await, ("dave".to_string(), "weg".to_string()));
}

/// A GET of the canary loop: the path with `{h}`/`{c}` for the collection's home and name, and the status the sharee
/// and the owner must get
struct Probe {
    path: String,
    sharee: StatusCode,
    owner: StatusCode,
}

fn ok(path: impl Into<String>) -> Probe {
    Probe { path: path.into(), sharee: StatusCode::OK, owner: StatusCode::OK }
}

/// Every route group that renders calendar data, read by the sharee (`who` in `home`/`copy`) and by the owner (in
/// `ohome`/`ocal`): each answer has its expected status; no sharee answer carries a marker of what the share hides,
/// and in every group some owner answer does (the routes reach the data). HTML and JSON both, the status for the
/// one the route speaks (`/api/…` JSON, else HTML).
async fn canary(app: &crate::testkit::TestApp, (who, home, copy): (&str, &str, &str), (owner, ohome, ocal): (&str, &str, &str), groups: Vec<(&str, Vec<Probe>)>, markers: &[&str]) {
    let mut s = app.login(who).await;
    let mut o = app.login(owner).await;
    for (group, probes) in groups {
        let mut seen = false;
        for p in &probes {
            for (c, h, cal, want, sharee) in [(&mut s, home, copy, p.sharee, true), (&mut o, ohome, ocal, p.owner, false)] {
                let path = p.path.replace("{h}", h).replace("{c}", cal);
                for json in [false, true] {
                    let r = if json { c.get_json(&path).await } else { c.get(&path).await };
                    if json == path.starts_with("/api/") {
                        assert_eq!(r.status, want, "{} {path} (json: {json}): {}", if sharee { who } else { owner }, r.body);
                    }
                    let hit = markers.iter().find(|m| r.body.contains(**m));
                    if sharee {
                        assert!(hit.is_none(), "{who} {path} (json: {json}) shows {hit:?}: {}", r.body);
                    } else {
                        seen |= hit.is_some();
                    }
                }
            }
        }
        assert!(seen, "{group}: no answer of the owner shows a marker, so the group proves nothing");
    }
}

/// The route groups of the loop, `q` the search words, `objects` the probes of single objects
fn groups(q: &[&str], objects: Vec<Probe>, plans: Vec<Probe>) -> Vec<(&'static str, Vec<Probe>)> {
    let mut views = vec![];
    for v in ["day", "week", "month", "planner", "weekends", "vacation", "countdown", "numbers"] {
        for d in ["2026-10-15", "2026-10-16"] {
            views.extend([ok(format!("/calendar?view={v}&date={d}")), ok(format!("/calendar?view={v}&date={d}&fragment=1"))]);
        }
    }
    let mut search = vec![ok("/api/mentions"), ok("/api/categories"), ok("/api/search/history")];
    for w in q {
        search.extend([ok(format!("/api/search?q={w}")), ok(format!("/api/events/suggest?q={w}")), ok(format!("/api/titles?q={w}"))]);
    }
    let mut tasks = vec![ok("/tasks"), ok("/api/tasks/done?back=all")];
    tasks.extend(["groups", "table", "board", "timeline", "deck"].map(|v| ok(format!("/tasks?view={v}"))));
    let redirect = Probe { path: "/".into(), sharee: StatusCode::SEE_OTHER, owner: StatusCode::SEE_OTHER };
    let mut history = vec![ok("/api/history"), ok("/history")];
    history.extend(plans);
    vec![
        ("pages", vec![redirect, ok("/calendar?date=2026-10-15"), ok("/settings"), ok("/editor?kind=event"), ok("/editor?kind=task"), ok("/contacts"), ok("/contacts?view=table"), ok("/contacts/part"), ok("/contacts/part?view=table")]),
        ("views", views),
        ("windows", vec![ok(WINDOW), ok("/api/days?start=2026-10-12&count=7"), ok("/api/weeks?start=2026-10-12&count=3"), ok("/api/density?year=2026")]),
        ("tasks", tasks),
        ("search", search),
        ("history", history),
        ("export", vec![ok("/settings/export/calendars/{h}/{c}")]),
        ("objects", objects),
    ]
}

/// The change ids `/api/history` lists for the user
async fn change_ids(c: &mut crate::testkit::Client) -> Vec<i64> {
    c.get_json("/api/history").await.ok().json()["changes"].as_array().unwrap().iter().filter_map(|x| x["id"].as_i64()).collect()
}

#[tokio::test]
async fn no_page_or_answer_shows_a_sharee_what_the_share_hides() {
    let w = seed_shared().await;
    let app = &w.s.app;
    let (n404, n200) = (StatusCode::NOT_FOUND, StatusCode::OK);
    let hidden = |path: &str| Probe { path: path.into(), sharee: n404, owner: n200 };
    // history: bob changes his confidential event (a change only he sees), erin a public one (one both see), bob searches
    let mut bob = app.login("bob").await;
    let gone = format!("/api/events/bob/{}/gone", w.team);
    let mut body = event_body(&bob.get_json(&gone).await.ok().json(), "bob", &w.team);
    body["summary"] = json!("Verborgen Marker7 neu");
    bob.json(Method::PUT, &gone, body).await.ok();
    let bob_only = change_ids(&mut bob).await.into_iter().max().unwrap();
    bob.json(Method::POST, "/api/search/history", json!({ "q": "Marker7 gesucht" })).await.ok();
    let mut erin = app.login("erin").await;
    let plan = format!("/api/events/group.team/{}/plan", w.team_copy);
    let mut body = event_body(&erin.get_json(&plan).await.ok().json(), "group.team", &w.team_copy);
    body["summary"] = json!("Teamplan von Erin");
    erin.json(Method::PUT, &plan, body).await.ok();
    let mut plans: Vec<Probe> = change_ids(&mut erin).await.into_iter().map(|id| ok(format!("/api/history/{id}/plan"))).collect();
    plans.push(hidden(&format!("/api/history/{bob_only}/plan")));
    let objects = vec![ok("/api/events/{h}/{c}/busy"), hidden("/api/events/{h}/{c}/gone"), ok("/api/tasks/{h}/{c}/geheim"), hidden("/api/events/{h}/{c}/busy/attach/0")];
    // erin: private Team events and tasks show their time only, confidential ones nothing
    canary(app, ("erin", "group.team", &w.team_copy), ("bob", "bob", &w.team), groups(&["geheim", "verborgen", "keller", "notiz", "erledigt"], objects, plans), &["Marker7", "Geheim", "Verborgen"]).await;
    // dave: alice's private dentist not at all (Privat's levels are unset: hidden)
    let mut alice = app.login("alice").await;
    let zahn = format!("/api/events/alice/{}/zahnarzt", w.s.privat);
    let mut body = event_body(&alice.get_json(&zahn).await.ok().json(), "alice", &w.s.privat);
    body["summary"] = json!("Zahnarzt Dr. Weiß, neu");
    alice.json(Method::PUT, &zahn, body).await.ok();
    let alice_only = change_ids(&mut alice).await.into_iter().max().unwrap();
    alice.json(Method::POST, "/api/search/history", json!({ "q": "Zahnarzt" })).await.ok();
    let objects = vec![hidden("/api/events/{h}/{c}/zahnarzt")];
    // (alice's Privat holds no tasks: that group has nothing to show the owner either)
    let g = groups(&["zahn", "wei"], objects, vec![hidden(&format!("/api/history/{alice_only}/plan"))]).into_iter().filter(|(g, _)| *g != "tasks").collect();
    canary(app, ("dave", "dave", &w.privat_copy), ("alice", "alice", &w.s.privat), g, &["Zahnarzt", "Weiß"]).await;
}

#[tokio::test]
async fn a_sharee_changes_only_its_own_view_and_never_the_share() {
    let w = seed_shared().await;
    let app = &w.s.app;
    let bob_p = app.principal("bob").await;
    let erin_p = app.principal("erin").await;
    let owners_team = || async { rustical_store::CalendarReadStore::get_calendar(app.state.raw().calendars, "bob", &w.team, false).await.unwrap() };
    let mut erin = app.login("erin").await;
    // rename and recolour the copy: hers changes, bob's calendar stays as it is
    erin.post_form(&format!("/settings/calendars/group.team/{}/update", w.team_copy), &[("name", "Meins"), ("color", "#000000"), ("description", "von Erin")]).await;
    let team = owners_team().await;
    assert_eq!((team.meta.displayname.as_deref(), team.meta.color.as_deref(), team.meta.description.as_deref()), (Some("Team"), Some("#5856d6"), None));
    let copy = crate::domain::collections::calendars_for(&app.state, &erin_p, false).await.unwrap().into_iter().find(|c| c.id == w.team_copy).unwrap();
    assert_eq!((copy.displayname.as_str(), copy.color.as_str()), ("Meins", "#000000"));
    // ending a share is the owner's: from the owner's path and from the copy's, refused, the share stays
    erin.post_form(&format!("/settings/calendars/bob/{}/unshare", w.team), &[("sharee", "group.team")]).await;
    erin.post_form(&format!("/settings/calendars/group.team/{}/unshare", w.team_copy), &[("sharee", "group.team")]).await;
    erin.post_form(&format!("/settings/addressbooks/group.team/{}/unshare", w.team_ab_copy), &[("sharee", "group.team")]).await;
    let a = crate::domain::collections::ensure_calendar(&app.state, &bob_p, "bob", &w.team, crate::state::Need::Own).await.unwrap();
    assert_eq!(app.state.collections().calendar_shares(&a).await.unwrap().len(), 1, "still shared");
    let ab = crate::domain::collections::ensure_addressbook(&app.state, &bob_p, "bob", &w.team_ab, crate::state::Need::Own).await.unwrap();
    assert_eq!(app.state.collections().addressbook_shares(&ab).await.unwrap().len(), 2);
    // deleting the copy, and purging the address book's: each only ends the share for her
    erin.post_form(&format!("/settings/calendars/group.team/{}/delete", w.team_copy), &[]).await;
    erin.post_form(&format!("/settings/addressbooks/group.team/{}/purge", w.team_ab_copy), &[]).await;
    assert!(crate::domain::collections::calendars_for(&app.state, &erin_p, false).await.unwrap().iter().all(|c| c.owner != "bob"));
    assert!(crate::domain::collections::addressbooks_for(&app.state, &erin_p, false).await.unwrap().iter().all(|a| a.owner != "bob"));
    assert!(owners_team().await.deleted_at.is_none() && stored(app, "bob", &w.team, "plan").await.contains("Teamplan"), "bob's calendar and its objects stay");
    assert!(stored_card(app, "bob", &w.team_ab, "tina").await.contains("Tina Team"), "and his address book");
    assert_eq!(app.state.collections().addressbook_shares(&ab).await.unwrap().len(), 1, "dave's share is untouched");
}
