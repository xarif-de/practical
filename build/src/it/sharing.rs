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
    // a group is no member, nor is nobody
    alice.post_form("/settings/groups/group.familie/members", &[("member", "group.team")]).await;
    alice.post_form("/settings/groups/group.familie/members", &[("member", "niemand@example.org")]).await;
    assert_eq!(app.state.principals().list_members("group.familie").await.unwrap().len(), 4);
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
    assert!(!page.contains("contenteditable"), "no table cell to edit in place");
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

/// Every GET that renders calendar data, as `who` sees it: none may carry a marker of what the share hides
async fn canary(app: &crate::testkit::TestApp, who: &str, home: &str, copy: &str, ids: &[&str], queries: &[&str], markers: &[&str]) {
    let mut c = app.login(who).await;
    let mut paths: Vec<String> = vec![WINDOW.into(), "/api/mentions".into(), "/api/history".into(), "/api/categories".into(), format!("/settings/export/calendars/{home}/{copy}"), "/tasks".into(), "/calendar".into(), "/settings".into()];
    for v in ["day", "week", "month", "planner", "weekends", "vacation", "countdown", "numbers"] {
        for d in ["2026-10-15", "2026-10-16"] {
            paths.push(format!("/calendar?view={v}&date={d}"));
            paths.push(format!("/calendar?view={v}&date={d}&fragment=1"));
        }
    }
    for v in ["groups", "table", "board", "timeline", "deck"] {
        paths.push(format!("/tasks?view={v}"));
    }
    for q in queries {
        paths.extend([format!("/api/search?q={q}"), format!("/api/events/suggest?q={q}"), format!("/api/titles?q={q}")]);
    }
    for id in ids {
        paths.extend([format!("/api/events/{home}/{copy}/{id}"), format!("/api/tasks/{home}/{copy}/{id}"), format!("/api/events/{home}/{copy}/{id}/attach/0")]);
    }
    for p in &paths {
        for json in [false, true] {
            let r = if json { c.get_json(p).await } else { c.get(p).await };
            assert!(!r.status.is_server_error(), "{who} {p}: {} {}", r.status, r.body);
            for m in markers {
                assert!(!r.body.contains(m), "{who} {p} (json: {json}) shows {m}: {}", r.body);
            }
        }
    }
}

#[tokio::test]
async fn no_page_or_answer_shows_a_sharee_what_the_share_hides() {
    let w = seed_shared().await;
    let app = &w.s.app;
    // the owner sees them where the sharees must not: the loop's routes reach the data
    let mut bob = app.login("bob").await;
    let att = bob.get(&format!("/api/events/bob/{}/busy/attach/0", w.team)).await.ok().body.clone();
    assert_eq!(att, "Marker7");
    assert!(bob.get_json(&format!("/api/tasks/bob/{}/geheim", w.team)).await.ok().body.contains("Notiz Marker7"));
    assert!(bob.get(&format!("/settings/export/calendars/bob/{}", w.team)).await.ok().body.contains("Verborgen Marker7"));
    assert!(bob.get("/calendar?view=day&date=2026-10-15").await.ok().body.contains("Geheimprojekt"));
    // erin: private Team events show their time only, confidential ones nothing; dave: alice's private dentist nothing
    canary(app, "erin", "group.team", &w.team_copy, &["busy", "gone", "geheim"], &["geheim", "verborgen", "keller", "notiz"], &["Marker7", "Geheim", "Verborgen"]).await;
    canary(app, "dave", "dave", &w.privat_copy, &["zahnarzt"], &["zahn", "weiß"], &["Zahnarzt", "Weiß"]).await;
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
