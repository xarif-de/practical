//! The change history through its routes: UI writes are recorded as one change each, DAV writes
//! found by the poll are recorded per object, undo writes the old version back and is a change too.

use super::seed::seed;
use axum::http::{Method, StatusCode};
use serde_json::json;

fn changes(h: &serde_json::Value) -> Vec<(String, String, String)> {
    h["changes"].as_array().unwrap().iter().map(|c| (c["src"].as_str().unwrap().to_string(), c["items"][0]["act"].as_str().unwrap_or("").to_string(), c["items"][0]["title"].as_str().unwrap_or("").to_string())).collect()
}

#[tokio::test]
async fn ui_writes_are_recorded_and_undone() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    // the seeded objects are the starting point, not changes
    assert_eq!(c.get_json("/api/history").await.ok().json()["changes"], json!([]));
    let input = json!({ "principal": "alice", "cal_id": s.privat, "summary": "Yoga", "start": "2026-10-23T18:00:00", "end": "2026-10-23T19:00:00" });
    let id = c.json(Method::POST, "/api/events", input.clone()).await.ok().json()["id"].as_str().unwrap().to_string();
    let path = format!("/api/events/alice/{}/{id}", s.privat);
    let mut upd = input.clone();
    upd["summary"] = json!("Yoga im Park");
    upd["location"] = json!("Großer Garten");
    upd["etag"] = c.get_json(&path).await.json()["etag"].clone();
    c.json(Method::PUT, &path, upd).await.ok();
    let h = c.get_json("/api/history").await.json();
    assert_eq!(changes(&h), [("ui".to_string(), "create".to_string(), "Yoga".to_string()), ("ui".into(), "update".into(), "Yoga im Park".into())]);
    let upd_change = &h["changes"][1];
    assert_eq!(upd_change["who"], "alice");
    assert_eq!(upd_change["undo"]["ok"], true);
    let attrs: Vec<(String, String, String)> = upd_change["items"][0]["attrs"].as_array().unwrap().iter().map(|a| (a[0].as_str().unwrap().into(), a[2].as_str().unwrap().into(), a[3].as_str().unwrap().into())).collect();
    assert!(attrs.contains(&("SUMMARY".into(), "Yoga".into(), "Yoga im Park".into())), "{attrs:?}");
    assert!(attrs.contains(&("LOCATION".into(), "".into(), "Großer Garten".into())), "{attrs:?}");
    assert_eq!(h["people"]["alice"], json!({ "name": "Alice Beispiel", "ini": "AB" }));
    let el = format!("cal:{id}");
    assert_eq!(h["els"][&el]["exists"], true, "{}", h["els"]);
    let n = upd_change["id"].as_i64().unwrap();

    // the plan of an undo, then the undo: the old title is back, as a change of its own
    let plan = c.get_json(&format!("/api/history/{n}/plan")).await.ok().json();
    assert_eq!((plan["ok"].as_bool(), plan["warns"].as_array().map(|w| w.len())), (Some(true), Some(0)));
    let r = c.json(Method::POST, &format!("/api/history/{n}/undo"), json!({})).await;
    let undo_id = r.ok().json()["id"].as_i64().unwrap();
    assert_eq!(c.get_json(&path).await.json()["summary"], "Yoga");
    let h = c.get_json("/api/history").await.json();
    let last = h["changes"].as_array().unwrap().last().unwrap();
    assert_eq!((last["src"].as_str(), last["reverts"].as_i64()), (Some("undo"), Some(n)));
    assert_eq!(h["changes"][1]["undoneBy"], undo_id);
    // undoing it again: nothing left to undo
    let r = c.json(Method::POST, &format!("/api/history/{n}/undo"), json!({})).await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert_eq!(r.json()["ok"], false);
    assert_eq!(c.get_json("/api/history/999/plan").await.status, StatusCode::NOT_FOUND);

    // deleting is recorded; undoing the delete restores the event
    c.delete(&path).await.ok();
    let h = c.get_json("/api/history").await.json();
    let del = h["changes"].as_array().unwrap().last().unwrap();
    assert_eq!(del["items"][0]["act"], "delete");
    assert_eq!(h["els"][&el]["exists"], false);
    let d = del["id"].as_i64().unwrap();
    c.json(Method::POST, &format!("/api/history/{d}/undo"), json!({})).await.ok();
    assert_eq!(c.get_json(&path).await.ok().json()["summary"], "Yoga");
    let h = c.get_json("/api/history").await.json();
    assert_eq!(h["changes"].as_array().unwrap().last().unwrap()["items"][0]["act"], "restore");
}

#[tokio::test]
async fn a_later_change_makes_the_undo_warn() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let base = json!({ "principal": "alice", "cal_id": s.privat, "summary": "A", "start": "2026-10-23T18:00:00", "end": "2026-10-23T19:00:00" });
    let id = c.json(Method::POST, "/api/events", base.clone()).await.ok().json()["id"].as_str().unwrap().to_string();
    let path = format!("/api/events/alice/{}/{id}", s.privat);
    let mut b = base.clone();
    b["summary"] = json!("B");
    b["etag"] = c.get_json(&path).await.json()["etag"].clone();
    c.json(Method::PUT, &path, b.clone()).await.ok();
    let mut cc = base.clone();
    cc["summary"] = json!("C");
    cc["etag"] = c.get_json(&path).await.json()["etag"].clone();
    c.json(Method::PUT, &path, cc).await.ok();
    let h = c.get_json("/api/history").await.json();
    let first_update = h["changes"][1]["id"].as_i64().unwrap();
    // undoing A→B while C is current overwrites C: a warning, 409 until confirmed
    let r = c.json(Method::POST, &format!("/api/history/{first_update}/undo"), json!({})).await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    let plan = r.json();
    assert_eq!(plan["ok"], true);
    assert!(!plan["warns"].as_array().unwrap().is_empty(), "{plan}");
    let r = c.json(Method::POST, &format!("/api/history/{first_update}/undo"), json!({ "confirm": true })).await;
    r.ok();
    assert_eq!(c.get_json(&path).await.json()["summary"], "A");
}

#[tokio::test]
async fn dav_writes_are_found_per_object_and_group_members_see_group_changes() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    // first sight of every collection
    crate::domain::history::poll(&s.app.state).await;
    // a DAV client writes into the group calendar (an Apple PRODID names the device)
    let ics = crate::testkit::vevent("geburtstag", "Geburtstag Opa", "DTSTART;VALUE=DATE:20261024\nDTEND;VALUE=DATE:20261025").replace("PRODID:-//test//test//EN", "PRODID:-//Apple Inc.//iPhone OS 18.0//EN");
    s.app.put_ics("group.familie", &s.familie, "geburtstag", &ics).await;
    crate::domain::history::poll(&s.app.state).await;
    let h = c.get_json("/api/history").await.json();
    assert_eq!(changes(&h), [("dav".to_string(), "create".to_string(), "Geburtstag Opa".to_string())]);
    assert_eq!(h["changes"][0]["who"], json!(null));
    assert_eq!(h["changes"][0]["prodid"], "-//Apple Inc.//iPhone OS 18.0//EN");
    assert!(h["changes"][0]["dev"].is_string(), "{}", h["changes"][0]);
    // bob is in the group too; carol is not
    let mut bob = s.app.login("bob").await;
    assert_eq!(bob.get_json("/api/history").await.json()["changes"].as_array().unwrap().len(), 1);
    let mut carol = s.app.login("carol").await;
    assert_eq!(carol.get_json("/api/history").await.json()["changes"], json!([]));
    // the page carries the same document
    let page = c.get("/history").await;
    assert_eq!(page.ok().island("history-data")["changes"].as_array().unwrap().len(), 1);
}
