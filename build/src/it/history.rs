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

/// A UTC instant `days` before now, to the second: (basic iCalendar form, the history's `at` form)
fn ago(days: i64) -> (String, String) {
    let t = chrono::Utc::now() - chrono::Duration::days(days);
    (t.format("%Y%m%dT%H%M%SZ").to_string(), t.format("%Y-%m-%d %H:%M:%S").to_string())
}

/// title → (src, at, acts) of every recorded change of one item (the history tables, unfiltered)
async fn dated(db: &sqlx::SqlitePool) -> std::collections::BTreeMap<String, (String, String, String)> {
    use sqlx::Row;
    let rows = sqlx::query("SELECT c.src, c.at, group_concat(i.act) AS acts, MIN(i.title) AS title, COUNT(*) AS n FROM practical_history_changes c JOIN practical_history_items i ON i.change_id = c.id GROUP BY c.id HAVING n = 1").fetch_all(db).await.unwrap();
    rows.into_iter().map(|r| (r.get::<String, _>("title"), (r.get("src"), r.get("at"), r.get("acts")))).collect()
}

/// A calendar purged from the trash writes nothing to its change log: the history must still let go of what it kept
/// of it (heads, cursor, versions nothing else refers to), or its objects count as existing for good and the
/// versions pile up (2026-11-03: 15 224 heads of four purged calendars). One that is only in the trash keeps all.
#[tokio::test]
async fn a_purged_calendar_leaves_nothing_behind_in_the_history() {
    use sqlx::Row;
    let s = seed().await;
    let db = &s.app.state.db;
    let mut c = s.app.login("alice").await;
    let heads = |coll: String| async move { sqlx::query("SELECT count(*) AS n FROM practical_history_heads WHERE coll = ?").bind(coll).fetch_one(db).await.unwrap().get::<i64, _>("n") };
    crate::domain::history::poll(&s.app.state).await;
    let (privat, arbeit) = (s.privat.clone(), s.arbeit.clone());
    assert!(heads(privat.clone()).await > 0 && heads(arbeit.clone()).await > 0);
    for id in [&privat, &arbeit] {
        c.post_form(&format!("/settings/calendars/alice/{id}/delete"), &[]).await;
    }
    c.post_form(&format!("/settings/calendars/alice/{privat}/purge"), &[]).await;
    crate::domain::history::purge(&s.app.state).await.unwrap();
    assert_eq!(heads(privat.clone()).await, 0, "the purged calendar's heads stayed");
    assert!(heads(arbeit.clone()).await > 0, "a calendar in the trash lost its heads");
    let cursors: i64 = sqlx::query("SELECT count(*) AS n FROM practical_history_cursors WHERE coll = ?").bind(&privat).fetch_one(db).await.unwrap().get("n");
    assert_eq!(cursors, 0);
    let orphans: i64 = sqlx::query("SELECT count(*) AS n FROM practical_history_blobs WHERE hash NOT IN (SELECT hash FROM practical_history_heads) AND hash NOT IN (SELECT before_hash FROM practical_history_items WHERE before_hash IS NOT NULL) AND hash NOT IN (SELECT after_hash FROM practical_history_items WHERE after_hash IS NOT NULL)").fetch_one(db).await.unwrap().get("n");
    assert_eq!(orphans, 0, "versions nothing refers to were kept");
}

/// A one-off import from Nextcloud is the user's import like a file import: each object it takes over dated at its
/// creation. Before, the remote import ran outside a history guard, and the history found its objects later as device
/// changes, all at the import's minute (2026-11-03, a real Nextcloud calendar of 3806 objects).
#[tokio::test]
async fn a_remote_import_is_recorded_as_the_users_import() {
    let s = seed().await;
    let db = &s.app.state.db;
    let alice = s.app.principal("alice").await;
    let (d3, d200) = (ago(3), ago(200));
    let part = |uid: &str, stamp: &str| crate::testkit::ics(&[format!("BEGIN:VEVENT\nUID:{uid}\nSUMMARY:{uid}\nDTSTART:20261110T100000Z\nDTEND:20261110T110000Z\nDTSTAMP:{stamp}\nEND:VEVENT\n")].concat());
    let m = crate::domain::remote::dav::Collection { href: "/remote.php/dav/calendars/alice/personal/".into(), name: "Personal".into(), color: "#0082c9".into(), description: String::new(), timezone: String::new(), components: vec!["VEVENT".into()], writable: true, ctag: String::new() };
    let (_, n) = crate::domain::remote::take::import_fetched(&s.app.state, &alice, "alice", crate::domain::remote::dav::Kind::Cal, &m, "", "", &[part("remote-new", &d3.0), part("remote-old", &d200.0)]).await.unwrap();
    assert_eq!(n, 2);
    let d = dated(db).await;
    assert_eq!(d.get("remote-new").map(|x| (x.0.as_str(), x.1.as_str(), x.2.as_str())), Some(("import", d3.1.as_str(), "create")), "{d:?}");
    assert!(!d.contains_key("remote-old"), "{d:?}");
    // and nothing of it turns up later as a device's change
    crate::domain::history::poll(&s.app.state).await;
    assert!(!dated(db).await.values().any(|x| x.0 == "dav"), "the import was found again as device changes");
}

#[tokio::test]
async fn an_import_dates_each_object_it_takes_over_when_it_was_made() {
    let s = seed().await;
    let db = &s.app.state.db;
    let mut c = s.app.login("alice").await;
    c.get("/settings").await.ok();
    let ev = |uid: &str, extra: &str| format!("BEGIN:VEVENT\nUID:{uid}\nSUMMARY:{uid}\nDTSTART:20261110T100000Z\nDTEND:20261110T110000Z\n{extra}\nEND:VEVENT\n");
    let (d1, d5, d10, d40, d80, d200) = (ago(1), ago(5), ago(10), ago(40), ago(80), ago(200));
    // floating: in the server's default zone (Europe/Berlin)
    let fl = chrono::Utc::now() - chrono::Duration::days(30);
    let fl_local = fl.with_timezone(&chrono_tz::Europe::Berlin).format("%Y%m%dT%H%M%S").to_string();
    let file = crate::testkit::ics(&[
        // CREATED wins over LAST-MODIFIED and DTSTAMP; an alarm's CREATED is not the event's
        ev("created", &format!("DTSTAMP:{}\nLAST-MODIFIED:{}\nCREATED:{}\nBEGIN:VALARM\nACTION:DISPLAY\nDESCRIPTION:x\nTRIGGER:-PT5M\nCREATED:{}\nEND:VALARM", d1.0, d5.0, d10.0, d80.0)),
        ev("modified", &format!("DTSTAMP:{}\nLAST-MODIFIED:{}", d1.0, d40.0)),
        ev("stamped", &format!("DTSTAMP:{}", d80.0)),
        ev("floating", &format!("DTSTAMP:{}\nCREATED:{fl_local}", d1.0)),
        // after the import itself (a wrong clock): now
        ev("future", &format!("DTSTAMP:{}\nCREATED:20991231T000000Z", d1.0)),
        // made before the retention (90 days): no change at all
        ev("ancient", &format!("DTSTAMP:{}", d200.0)),
    ]
    .concat());
    let t0 = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    assert_eq!(c.post_raw(&format!("/settings/import/calendars/alice/{}", s.privat), &file).await.ok().json(), json!({ "imported": 6 }));
    let t1 = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let d = dated(db).await;
    let imp = |t: &str| d.get(t).filter(|x| x.0 == "import" && x.2 == "create").map(|x| x.1.clone());
    assert_eq!(imp("created"), Some(d10.1.clone()));
    assert_eq!(imp("modified"), Some(d40.1.clone()));
    assert_eq!(imp("stamped"), Some(d80.1.clone()));
    assert_eq!(imp("floating"), Some(fl.format("%Y-%m-%d %H:%M:%S").to_string()));
    assert!(imp("future").is_some_and(|a| a >= t0 && a <= t1), "{d:?}");
    assert!(!d.contains_key("ancient"), "{d:?}");
    assert!(c.get_json(&format!("/api/events/alice/{}/ancient", s.privat)).await.status.is_success(), "the object itself is there");

    // a card: REV; without one, now
    c.post_raw(&format!("/settings/import/addressbooks/alice/{}", s.freunde), &format!("BEGIN:VCARD\r\nVERSION:3.0\r\nUID:rev\r\nFN:Mit Rev\r\nREV:{}\r\nEND:VCARD\r\nBEGIN:VCARD\r\nVERSION:3.0\r\nUID:norev\r\nFN:Ohne Rev\r\nEND:VCARD\r\n", d5.0)).await.ok();
    let d = dated(db).await;
    assert_eq!(d["Mit Rev"].1, d5.1);
    assert!(d["Ohne Rev"].1 >= t0);

    // the document: a change each, at its time, its create read as taken over; `born` only where it says more than `at`
    let h = c.get_json("/api/history").await.ok().json();
    let doc: Vec<&serde_json::Value> = h["changes"].as_array().unwrap().iter().filter(|x| x["src"] == "import").collect();
    assert_eq!(doc.len(), 7, "{h}");
    assert!(doc.iter().all(|x| x["items"].as_array().unwrap().len() == 1 && x["items"][0]["act"] == "create" && x["items"][0].get("born").is_none()), "{h}");
    // listed oldest first by time, not by id
    let ats: Vec<&str> = h["changes"].as_array().unwrap().iter().map(|x| x["at"].as_str().unwrap()).collect();
    assert!(ats.windows(2).all(|w| w[0] <= w[1]), "{ats:?}");

    // a sync round: its new objects dated likewise (one before the retention: none), its update one change at now
    {
        use crate::domain::history::{self, Actor};
        use crate::domain::history_text::Store;
        let mut g = history::begin(&s.app.state, Actor::sync("alice")).await;
        s.app.put_ics("alice", &s.privat, "remote", &crate::testkit::ics(&ev("remote", &format!("DTSTAMP:{}\nCREATED:{}", d1.0, d5.0)))).await;
        s.app.put_ics("alice", &s.privat, "remote-old", &crate::testkit::ics(&ev("remote-old", &format!("DTSTAMP:{}", d200.0)))).await;
        s.app.put_ics("alice", &s.privat, "zahnarzt", &crate::testkit::vevent("zahnarzt", "Zahnarzt verlegt", "DTSTART;TZID=Europe/Berlin:20261016T093000\nDTEND;TZID=Europe/Berlin:20261016T101500")).await;
        s.app.put_ics("alice", &s.privat, "utc", &crate::testkit::vevent("utc", "Abendessen später", "DTSTART:20261014T190000Z\nDTEND:20261014T210000Z")).await;
        for id in ["remote", "remote-old", "zahnarzt", "utc"] {
            g.object(Store::Cal, "alice", &s.privat, id);
        }
        g.commit().await.expect("a change");
    }
    let d = dated(db).await;
    assert_eq!((d["remote"].0.as_str(), d["remote"].1.as_str()), ("sync", d5.1.as_str()));
    assert!(!d.contains_key("remote-old"));
    use sqlx::Row;
    let upd = sqlx::query("SELECT c.at, COUNT(*) AS n, group_concat(DISTINCT i.act) AS acts FROM practical_history_changes c JOIN practical_history_items i ON i.change_id = c.id WHERE c.src = 'sync' AND i.act != 'create' GROUP BY c.id").fetch_all(db).await.unwrap();
    assert_eq!(upd.len(), 1, "the updates: one change");
    assert_eq!((upd[0].get::<i64, _>("n"), upd[0].get::<String, _>("acts").as_str()), (2, "update"));
    assert!(upd[0].get::<String, _>("at") >= t0);
    // a create of this UI is new, not taken over: now, and one change
    c.json(Method::POST, "/api/events", json!({ "principal": "alice", "cal_id": s.privat, "summary": "Neu hier", "start": "2026-10-23T18:00:00", "end": "2026-10-23T19:00:00" })).await.ok();
    assert!(dated(db).await["Neu hier"].1 >= t0);

    // undo stays by the order of recording: an edit after the import is later than the import's change dated back
    let taken: i64 = sqlx::query_scalar("SELECT c.id FROM practical_history_changes c JOIN practical_history_items i ON i.change_id = c.id WHERE i.title = 'created' AND c.src = 'import'").fetch_one(db).await.unwrap();
    let path = format!("/api/events/alice/{}/created", s.privat);
    let mut e = c.get_json(&path).await.ok().json();
    e["summary"] = json!("created, geändert");
    let r = c.json(Method::PUT, &path, e).await;
    r.ok();
    let edit: i64 = sqlx::query_scalar("SELECT MAX(id) FROM practical_history_changes WHERE src = 'ui'").fetch_one(db).await.unwrap();
    let plan = c.get_json(&format!("/api/history/{taken}/plan")).await.ok().json();
    assert_eq!(plan["warns"], json!([{ "kind": "later", "title": "created", "by": null, "ids": [edit], "label": null, "cur": null, "old": null }]), "{plan}");
    c.json(Method::POST, &format!("/api/history/{taken}/undo"), json!({ "confirm": true })).await.ok();
    assert_eq!(c.get_json(&path).await.status, StatusCode::NOT_FOUND);
    let h = c.get_json("/api/history").await.ok().json();
    let by_id = |id: i64| h["changes"].as_array().unwrap().iter().find(|x| x["id"] == id).cloned().unwrap();
    assert!(by_id(taken)["undoneBy"].is_i64() && by_id(edit)["inactiveBy"].is_i64(), "{h}");
}

/// The ids of a history document's changes, as listed (oldest first).
fn ids(h: &serde_json::Value) -> Vec<i64> {
    h["changes"].as_array().unwrap().iter().map(|c| c["id"].as_i64().unwrap()).collect()
}

#[tokio::test]
async fn the_document_pages_by_time_and_never_shows_another_users_changes() {
    use crate::routes::history::{PAGE, PAGE_ITEMS};
    use sqlx::Row;
    let s = seed().await;
    let app = &s.app;
    let bobs = app.calendar("bob", "Bobs", "#123456", &["VEVENT"]).await;
    crate::domain::history::poll(&app.state).await; // first sight: no changes
    dav(app, &s.privat, &bobs, "a").await;
    // between them an import of 150 objects made over the last 75 days: ids in the middle, times before both
    let mut alice = app.login("alice").await;
    alice.get("/settings").await.ok();
    let file = |tag: &str| crate::testkit::ics(&(0..150).map(|i| format!("BEGIN:VEVENT\nUID:bulk-{i}\nDTSTAMP:{}\nDTSTART:20261111T100000Z\nDTEND:20261111T110000Z\nSUMMARY:{tag} {i}\nEND:VEVENT\n", ago(1 + i / 2).0)).collect::<String>());
    alice.post_raw(&format!("/settings/import/calendars/alice/{}", s.privat), &file("Bulk")).await.ok();
    dav(app, &s.privat, &bobs, "c").await;
    // the same file again, changed: every object an update, one change
    alice.post_raw(&format!("/settings/import/calendars/alice/{}", s.privat), &file("Neu")).await.ok();
    // what the user may see, newest first by (at, id)
    let order = |who: &'static str| async move {
        let me = if who == "bob" { "bob" } else { "alice" };
        let mut v: Vec<(String, i64)> = sqlx::query("SELECT DISTINCT c.at, c.id FROM practical_history_changes c JOIN practical_history_items i ON i.change_id = c.id WHERE i.principal = ?").bind(me).fetch_all(&app.state.db).await.unwrap().into_iter().map(|r| (r.get("at"), r.get("id"))).collect();
        v.sort();
        v.reverse();
        v.into_iter().map(|x| x.1).collect::<Vec<i64>>()
    };
    let (all_alice, all_bob) = (order("alice").await, order("bob").await);
    assert_eq!((all_alice.len(), all_bob.len()), (60 + 150 + 1, 6));
    let mut by_id = all_alice.clone();
    by_id.sort_by(|a, b| b.cmp(a));
    assert_ne!(by_id[..PAGE], all_alice[..PAGE], "ids and times disagree");

    // page by page, newest first by time: no gaps, no duplicates; each page listed oldest first
    let first = alice.get_json("/api/history").await.ok().json();
    let page = alice.get("/history").await.ok().island("history-data");
    assert_eq!((ids(&page), &page["more"]), (ids(&first), &json!(true)));
    let (mut got, mut doc, mut pages) = (vec![], first.clone(), 0);
    loop {
        let mut p = ids(&doc);
        assert!(p.len() <= PAGE);
        let ats: Vec<&str> = doc["changes"].as_array().unwrap().iter().map(|x| x["at"].as_str().unwrap()).collect();
        assert!(ats.windows(2).all(|w| w[0] <= w[1]), "{ats:?}");
        p.reverse();
        got.extend(p);
        pages += 1;
        if doc["more"] == false {
            break;
        }
        doc = alice.get_json(&format!("/api/history?before={}", urlencoding(doc["cursor"].as_str().unwrap()))).await.ok().json();
    }
    assert_eq!((got, pages), (all_alice.clone(), 3));
    // the bulk update: PAGE_ITEMS items and its count; a change sent whole has none
    let bulk = first["changes"].as_array().unwrap().iter().find(|x| x.get("n").is_some()).unwrap();
    assert_eq!((bulk["items"].as_array().unwrap().len(), bulk["n"].as_i64(), bulk["items"][0]["act"].as_str()), (PAGE_ITEMS, Some(150), Some("update")));
    assert!(first["changes"][0].get("n").is_none());
    // from a place: everything from it on, newest by time
    let second = alice.get_json(&format!("/api/history?before={}", urlencoding(first["cursor"].as_str().unwrap()))).await.ok().json();
    let from = alice.get_json(&format!("/api/history?from={}", urlencoding(second["cursor"].as_str().unwrap()))).await.ok().json();
    let mut want = all_alice[..2 * PAGE].to_vec();
    want.reverse();
    assert_eq!((ids(&from), &from["more"], &from["cursor"]), (want, &json!(true), &second["cursor"]));
    assert_eq!(alice.get_json("/api/history?before=x").await.status, StatusCode::BAD_REQUEST);

    // bob: his own only, through every form, also with alice's places
    let mut bob = app.login("bob").await;
    let mut seen = vec![];
    for q in ["".to_string(), format!("?before={}", urlencoding(first["cursor"].as_str().unwrap())), format!("?from={}", urlencoding(second["cursor"].as_str().unwrap())), "?from=0000|0".into(), "?before=9999|0".into()] {
        let h = bob.get_json(&format!("/api/history{q}")).await.ok().json();
        assert!(ids(&h).iter().all(|i| all_bob.contains(i)), "{q}: {:?}", ids(&h));
        assert!(!h.to_string().contains("cal:a0-0") && !h.to_string().contains("cal:bulk-"), "{q}");
        seen.extend(ids(&h));
    }
    assert!(all_bob.iter().all(|i| seen.contains(i)));
    let mut carol = app.login("carol").await;
    for q in ["", "?before=9999|0", "?from=0000|0"] {
        let h = carol.get_json(&format!("/api/history{q}")).await.ok().json();
        assert_eq!((&h["changes"], &h["more"]), (&json!([]), &json!(false)), "{q}");
    }
}

/// 30 DAV changes of alice's with 3 of bob's in between (one poll per 10 writes)
async fn dav(app: &crate::testkit::TestApp, privat: &str, bobs: &str, tag: &str) {
    for round in 0..3 {
        for k in 0..10 {
            let uid = format!("{tag}{round}-{k}");
            app.put_ics("alice", privat, &uid, &crate::testkit::vevent(&uid, &uid, "DTSTART:20261110T100000Z\nDTEND:20261110T110000Z")).await;
        }
        let uid = format!("b{tag}{round}");
        app.put_ics("bob", bobs, &uid, &crate::testkit::vevent(&uid, &uid, "DTSTART:20261110T100000Z\nDTEND:20261110T110000Z")).await;
        crate::domain::history::poll(&app.state).await;
    }
}

/// A cursor ("at|id", with a space and a colon) as a query value.
fn urlencoding(s: &str) -> String {
    s.replace('%', "%25").replace(' ', "%20").replace(':', "%3A").replace('|', "%7C")
}

/// A plan from `visible_for` (the change and the later ones of its elements) is the plan from every visible change.
#[tokio::test]
async fn plans_from_the_bounded_read_match_plans_from_all() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let input = json!({ "principal": "alice", "cal_id": s.privat, "summary": "Yoga", "start": "2026-10-23T18:00:00", "end": "2026-10-23T19:00:00" });
    let id = c.json(Method::POST, "/api/events", input.clone()).await.ok().json()["id"].as_str().unwrap().to_string();
    let path = format!("/api/events/alice/{}/{id}", s.privat);
    for (n, place) in ["Park", "Halle"].iter().enumerate() {
        let mut upd = input.clone();
        upd["location"] = json!(place);
        upd["summary"] = json!(format!("Yoga {n}"));
        upd["etag"] = c.get_json(&path).await.json()["etag"].clone();
        c.json(Method::PUT, &path, upd).await.ok();
    }
    let other = c.json(Method::POST, "/api/events", json!({ "principal": "alice", "cal_id": s.privat, "summary": "Weg", "start": "2026-10-24T18:00:00", "end": "2026-10-24T19:00:00" })).await.ok().json()["id"].as_str().unwrap().to_string();
    c.delete(&format!("/api/events/alice/{}/{other}", s.privat)).await.ok();
    c.get("/settings").await.ok();
    c.post_raw(&format!("/settings/import/calendars/alice/{}", s.privat), &crate::testkit::vevent(&id, "Yoga importiert", "DTSTART:20261023T160000Z\nDTEND:20261023T170000Z")).await.ok();
    let user = s.app.principal("alice").await;
    let all = crate::domain::history::visible(&s.app.state, &user).await.unwrap();
    crate::domain::history::undo(&s.app.state, &user, all[1].id, true).await.unwrap().ok().expect("undone");
    let all = crate::domain::history::visible(&s.app.state, &user).await.unwrap();
    assert!(all.len() >= 7, "{}", all.len());
    let show = |p: &crate::domain::history::Plan| format!("{} {:?} {:?} {:?} {:?}", p.ok, p.why, p.warns, p.skips, p.now);
    let mut warned = false;
    for ch in &all {
        let a = crate::domain::history::plan(&s.app.state, &user, ch, &all, &mut Default::default()).await.unwrap();
        let some = crate::domain::history::visible_for(&s.app.state, &user, &[ch.id]).await.unwrap();
        assert!(some.len() < all.len() || ch.id == all[0].id, "change {}: read {} of {}", ch.id, some.len(), all.len());
        let b = crate::domain::history::plan(&s.app.state, &user, some.iter().find(|x| x.id == ch.id).unwrap(), &some, &mut Default::default()).await.unwrap();
        assert_eq!(show(&a), show(&b), "change {}", ch.id);
        warned |= !a.warns.is_empty();
    }
    assert!(warned, "some plan warns about later changes");
}
