//! The event API the editor uses: create, read, update (ETag), move, delete, attachments, the views' move by days
//! (drag and drop), and the read-only JSON the calendar page fetches.

use super::seed::seed;
use axum::http::{Method, StatusCode};
use serde_json::json;

#[tokio::test]
async fn event_create_read_update_move_delete() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let input = json!({
        "principal": "alice", "cal_id": s.privat, "summary": "Kino", "location": "Schauburg",
        "description": "Karten\nabholen", "start": "2026-10-22T19:30:00", "end": "2026-10-22T22:00:00",
        "alarms": [{ "minutes": 30 }], "categories": ["Freizeit"], "status": "confirmed", "class": "private",
        "priority": 5, "color": "#ff0000", "geo": [51.0, 13.7],
        "links": [{ "kind": "url", "uri": "https://example.org/kino" }]
    });
    let r = c.json(Method::POST, "/api/events", input.clone()).await;
    let id = r.ok().json()["id"].as_str().unwrap().to_string();
    assert_eq!(id.len(), 36, "a new UUID is the object id and the UID");
    let path = format!("/api/events/alice/{}/{id}", s.privat);
    let d = c.get_json(&path).await.ok().json();
    for (k, v) in [
        ("summary", json!("Kino")),
        ("location", json!("Schauburg")),
        ("description", json!("Karten\nabholen")),
        ("start", json!("2026-10-22T19:30:00")),
        ("end", json!("2026-10-22T22:00:00")),
        ("all_day", json!(false)),
        ("status", json!("CONFIRMED")),
        ("class", json!("PRIVATE")),
        ("priority", json!(5)),
        ("color", json!("#ff0000")),
        ("geo", json!([51.0, 13.7])),
        ("categories", json!(["Freizeit"])),
        ("timezone", json!("Europe/Berlin")),
        ("uid", json!(id)),
        ("rrule", json!(null)),
        ("has_overrides", json!(false)),
    ] {
        assert_eq!(d[k], v, "{k}");
    }
    assert_eq!(d["alarms"][0]["minutes"], 30);
    assert_eq!(d["alarms"][0]["action"], "DISPLAY");
    assert_eq!(d["links"][0]["uri"], "https://example.org/kino");
    let etag = d["etag"].as_str().unwrap().to_string();

    // update with the current ETag: a weekly series now
    let mut upd = input.clone();
    upd["summary"] = json!("Kino (wöchentlich)");
    upd["rrule"] = json!("FREQ=WEEKLY;COUNT=3");
    upd["etag"] = json!(etag);
    c.json(Method::PUT, &path, upd.clone()).await.ok();
    let d2 = c.get_json(&path).await.ok().json();
    assert_eq!((d2["summary"].as_str(), d2["rrule"].as_str()), (Some("Kino (wöchentlich)"), Some("FREQ=WEEKLY;COUNT=3")));
    assert_ne!(d2["etag"], d["etag"]);
    let win = c.get_json("/api/events?start=2026-10-20&end=2026-11-10").await.ok().json();
    assert_eq!(win.as_array().unwrap().iter().filter(|i| i["object_id"] == id.as_str()).count(), 3);
    // the stale ETag is a conflict
    let r = c.json(Method::PUT, &path, upd.clone()).await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert_eq!(r.body, "Der Termin wurde anderswo geändert – bitte neu laden.");

    // moving to another calendar: new object there (same UID), gone here
    let mut mv = upd.clone();
    mv["cal_id"] = json!(s.arbeit);
    mv["etag"] = c.get_json(&path).await.json()["etag"].clone(); // a write names the version it is based on (w3/http)
    let new_id = c.json(Method::PUT, &path, mv).await.ok().json()["id"].as_str().unwrap().to_string();
    assert_eq!(new_id, id, "the UID is kept, so is the object id");
    assert_eq!(c.get_json(&path).await.status, StatusCode::NOT_FOUND);
    let moved = c.get_json(&format!("/api/events/alice/{}/{id}", s.arbeit)).await.ok().json();
    assert_eq!(moved["uid"], id.as_str());

    let r = c.delete(&format!("/api/events/alice/{}/{id}", s.arbeit)).await;
    assert_eq!(r.ok().json(), json!({ "id": id }));
    assert_eq!(c.get_json(&format!("/api/events/alice/{}/{id}", s.arbeit)).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn event_input_errors() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let base = json!({ "principal": "alice", "cal_id": s.privat, "summary": "x", "start": "2026-10-22T19:30:00", "end": "2026-10-22T20:00:00" });
    let with = |k: &str, v: serde_json::Value| { let mut b = base.clone(); b[k] = v; b };
    for (body, status, text) in [
        (with("summary", json!("  ")), StatusCode::BAD_REQUEST, "Bitte einen Titel angeben."),
        (with("end", json!("2026-10-22T19:00:00")), StatusCode::BAD_REQUEST, "Das Ende liegt vor dem Beginn."),
        (with("rrule", json!("DAILY")), StatusCode::BAD_REQUEST, "Die Wiederholungsregel muss mit FREQ= beginnen."),
        // a browser session reads the translated message, never the store's own English text
        (with("principal", json!("bob")), StatusCode::FORBIDDEN, "Dafür fehlt die Berechtigung."),
        (with("cal_id", json!("missing")), StatusCode::NOT_FOUND, "Nicht gefunden."),
    ] {
        let r = c.json(Method::POST, "/api/events", body).await;
        assert_eq!((r.status, r.body.as_str()), (status, text));
    }
    // a body of the wrong shape: 400 naming the field (extract.rs; axum answered 422 in English)
    let r = c.json(Method::POST, "/api/events", json!({ "summary": "x" })).await;
    assert_eq!((r.status, r.body.as_str()), (StatusCode::BAD_REQUEST, "Ungültige Angabe im Feld „principal“."));
    // another principal's object is forbidden, an unknown one not found
    assert_eq!(c.get_json(&format!("/api/events/bob/{}/x", s.privat)).await.status, StatusCode::FORBIDDEN);
    assert_eq!(c.get_json(&format!("/api/events/alice/{}/missing", s.privat)).await.status, StatusCode::NOT_FOUND);
    // a group member writes into the group's calendar; carol may not
    let r = c.json(Method::POST, "/api/events", with("principal", json!("group.familie")).as_object().map(|o| { let mut o = o.clone(); o.insert("cal_id".into(), json!(s.familie)); serde_json::Value::Object(o) }).unwrap()).await;
    r.ok();
    let mut carol = s.app.login("carol").await;
    assert_eq!(carol.get_json(&format!("/api/events/group.familie/{}/treffen", s.familie)).await.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn event_detail_of_the_seeded_review() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let d = c.get_json(&format!("/api/events/alice/{}/review", s.arbeit)).await.ok().json();
    assert_eq!(d["summary"], "Quartalsreview");
    assert_eq!(d["location"], "Raum 4.12, Hauptgebäude");
    assert_eq!((d["status"].as_str(), d["transp"].as_str(), d["class"].as_str()), (Some("TENTATIVE"), Some("TRANSPARENT"), Some("CONFIDENTIAL")));
    assert_eq!(d["priority"], 1);
    assert_eq!(d["geo"], json!([51.050407, 13.737262]));
    assert_eq!(d["organizer"]["name"], "Bob Muster");
    assert_eq!(d["organizer"]["email"], "bob@example.org");
    assert_eq!(d["attendees"][0]["status"], "ACCEPTED");
    // the attendee is matched with the address book by e-mail
    assert_eq!(d["attendees"][0]["contacts"][0]["name"], "Anna Schmidt");
    assert!(d["organizer"].get("contacts").is_none(), "no match: the field is left out");
    // RELATED-TO is resolved to the series it names
    assert_eq!(d["related"][0]["uid"], "standup");
    assert_eq!(d["related"][0]["target"]["kind"], "event");
    assert_eq!(d["related"][0]["target"]["summary"], "Standup");
    assert_eq!(d["related"][0]["target"]["date"], "2026-09-02");
    // the series: rule, exception as a local date-time, reminder
    let st = c.get_json(&format!("/api/events/alice/{}/standup", s.arbeit)).await.ok().json();
    assert_eq!(st["rrule"], "FREQ=WEEKLY;BYDAY=WE");
    assert_eq!(st["exdates"], json!(["2026-10-07T09:00:00"]));
    assert_eq!(st["alarms"][0]["minutes"], 10);
    assert_eq!(st["start"], "2026-09-02T09:00:00");
    // an all-day birthday series
    let b = c.get_json(&format!("/api/events/alice/{}/bday", s.privat)).await.ok().json();
    assert_eq!((b["all_day"].as_bool(), b["start"].as_str(), b["end"].as_str()), (Some(true), Some("1950-03-12T00:00:00"), Some("1950-03-13T00:00:00")));
}

#[tokio::test]
async fn embedded_attachment_download() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let input = json!({
        "principal": "alice", "cal_id": s.privat, "summary": "Mit Anhang", "start": "2026-10-22T19:30:00", "end": "2026-10-22T20:00:00",
        "links": [{ "kind": "attach", "embedded": true, "data": "SGFsbG8=", "fmt": "text/plain", "filename": "gruß.txt" }]
    });
    let id = c.json(Method::POST, "/api/events", input).await.ok().json()["id"].as_str().unwrap().to_string();
    let d = c.get_json(&format!("/api/events/alice/{}/{id}", s.privat)).await.json();
    assert_eq!(d["links"][0]["embedded"], true);
    assert_eq!(d["links"][0]["size"], 5);
    let r = c.get_json(&format!("/api/events/alice/{}/{id}/attach/0", s.privat)).await;
    r.ok();
    assert_eq!(r.bytes, b"Hallo");
    assert_eq!(r.header("content-type").as_deref(), Some("text/plain"));
    // a name outside ASCII: an ASCII stand-in and the name itself (RFC 6266 filename*)
    assert_eq!(r.header("content-disposition").as_deref(), Some("attachment; filename=\"gru_.txt\"; filename*=UTF-8''gru%C3%9F.txt"));
    assert_eq!(r.header("cache-control").as_deref(), Some("private, max-age=0"));
    assert_eq!(c.get_json(&format!("/api/events/alice/{}/{id}/attach/1", s.privat)).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn window_density_and_errors() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let w = c.get_json("/api/events?start=2026-10-14&end=2026-10-15").await.ok().json();
    let rows: Vec<(String, String, String)> = w.as_array().unwrap().iter().map(|i| (i["summary"].as_str().unwrap().into(), i["start"].as_str().unwrap().into(), i["end"].as_str().unwrap().into())).collect();
    assert_eq!(rows, [
        ("Standup".to_string(), "2026-10-14T09:00:00".to_string(), "2026-10-14T09:15:00".to_string()),
        ("Quartalsreview".into(), "2026-10-14T14:00:00".into(), "2026-10-14T15:30:00".into()),
        ("Abendessen".into(), "2026-10-14T20:00:00".into(), "2026-10-14T22:00:00".into()),
    ]);
    assert_eq!(w[0]["alarms"], json!([10]), "the series' reminder reaches every instance");
    assert_eq!(w[0]["recurring"], true);
    // the exception date is skipped, the all-day fair spans three days
    let oct = c.get_json("/api/events?start=2026-10-01&end=2026-11-01").await.json();
    let starts: Vec<&str> = oct.as_array().unwrap().iter().filter(|i| i["summary"] == "Standup").map(|i| i["start"].as_str().unwrap()).collect();
    assert_eq!(starts, ["2026-10-14T09:00:00", "2026-10-21T09:00:00", "2026-10-28T09:00:00"]);
    let messe = oct.as_array().unwrap().iter().find(|i| i["summary"] == "Messe").unwrap();
    assert_eq!((messe["start"].as_str(), messe["end"].as_str(), messe["all_day"].as_bool()), (Some("2026-10-20T00:00:00"), Some("2026-10-23T00:00:00"), Some(true)));
    // filter by calendar
    let only = c.get_json(&format!("/api/events?start=2026-10-01&end=2026-11-01&cals=group.familie/{}", s.familie)).await.json();
    assert_eq!(only.as_array().unwrap().len(), 1);
    // limits
    let r = c.get_json("/api/events?start=2026-01-01&end=2027-02-06").await;
    assert_eq!((r.status, r.body.as_str()), (StatusCode::BAD_REQUEST, "Der Zeitraum ist zu groß."));
    let r = c.get_json("/api/events?start=2026-10-02&end=2026-10-01").await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(c.get_json("/api/density?year=2026").await.ok().json(), json!([0, 0, 1, 0, 0, 0, 0, 0, 5, 10, 4, 5]));
    assert_eq!(c.get_json("/api/density?year=2025").await.ok().json(), json!([0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
}

#[tokio::test]
async fn categories_titles_rules_presets() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    assert_eq!(c.get_json("/api/categories?scope=event").await.ok().json(), json!([{ "name": "Arbeit", "count": 2, "rule": false }, { "name": "Meeting", "count": 1, "rule": false }, { "name": "Urlaub", "count": 1, "rule": false }]));
    assert_eq!(c.get_json("/api/categories?scope=task").await.ok().json(), json!([{ "name": "Haushalt", "count": 1, "rule": false }]));
    assert_eq!(c.get_json("/api/categories").await.json(), c.get_json("/api/categories?scope=bogus").await.json(), "anything but task is the event scope");
    let titles = c.get_json("/api/titles").await.ok().json();
    assert_eq!(titles[0], json!(["Abendessen", 1]));
    assert_eq!(titles.as_array().unwrap().len(), 9);

    // a rule for a category nobody uses yet shows up with count 0
    let rule = json!({ "when": { "color": { "mode": "default", "value": "#ff0000" } } });
    let r = c.json(Method::PUT, "/api/category-rules/Sport?scope=event", rule.clone()).await;
    r.ok();
    let rules = c.get_json("/api/category-rules?scope=event").await.ok().json();
    assert!(rules.get("Sport").is_some(), "{rules}");
    assert_eq!(c.get_json("/api/category-rules?scope=task").await.json(), json!({}), "rules are per scope");
    let cats = c.get_json("/api/categories?scope=event").await.json();
    assert_eq!(cats.as_array().unwrap().last().unwrap(), &json!({ "name": "Sport", "count": 0, "rule": true }));
    assert_eq!(c.delete("/api/category-rules/Sport?scope=event").await.ok().json(), json!({ "id": "Sport" }));
    assert_eq!(c.get_json("/api/category-rules?scope=event").await.json(), json!({}));

    // reminder presets: null until saved, validated
    assert_eq!(c.get_json("/api/alarm-presets").await.ok().json(), json!(null));
    let presets = json!([{ "action": "DISPLAY", "minutes": 15 }, { "action": "EMAIL", "minutes": 1440, "attendees": ["a@example.org"] }]);
    c.json(Method::PUT, "/api/alarm-presets", presets).await.ok();
    let back = c.get_json("/api/alarm-presets").await.json();
    assert_eq!(back[0], json!({ "action": "DISPLAY", "minutes": 15, "related_end": false, "repeat": 0, "duration_minutes": 0, "attendees": [], "attach": "", "proximity": "" }));
    let r = c.json(Method::PUT, "/api/alarm-presets", json!([{ "action": "PROCEDURE" }])).await;
    assert_eq!((r.status, r.body.as_str()), (StatusCode::BAD_REQUEST, "Ungültige Erinnerungsvorlage."));
    // the calendar page carries them for the editor
    c.get("/calendar?date=2026-10-14").await.ok().has("\"action\":\"EMAIL\"");
}

#[tokio::test]
async fn colours_api() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let src = c.get_json("/api/colors").await.ok().json();
    let kinds: Vec<&str> = src.as_array().unwrap().iter().map(|x| x["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["calendar", "calendar", "calendar", "calendar", "addressbook", "holiday", "school", "event"]);
    assert_eq!(src[7]["name"], "Zahnarzt Dr. Weiß");
    assert_eq!(src[7]["color"], "tomato");
    // a calendar's colour, an address book's, an event's
    let r = c.json(Method::PUT, &format!("/api/colors/calendar/alice/{}", s.privat), json!({ "color": " #AABBCC " })).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let r = c.json(Method::PUT, &format!("/api/colors/addressbook/alice/{}", s.freunde), json!({ "color": "#112233" })).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let r = c.json(Method::PUT, &format!("/api/colors/addressbook/alice/{}", s.freunde), json!({ "color": "red" })).await;
    assert_eq!((r.status, r.body.as_str()), (StatusCode::BAD_REQUEST, "Die Farbe muss #rrggbb sein."));
    let r = c.json(Method::PUT, &format!("/api/colors/event/alice/{}/review", s.arbeit), json!({ "color": "#00ff00" })).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let r = c.json(Method::PUT, "/api/colors/planet/x", json!({ "color": "#00ff00" })).await;
    assert_eq!((r.status, r.body.as_str()), (StatusCode::BAD_REQUEST, "Unbekanntes Ziel für die Farbe."));
    let after = c.get_json("/api/colors").await.json();
    let find = |kind: &str, name: &str| after.as_array().unwrap().iter().find(|x| x["kind"] == kind && x["name"] == name).unwrap()["color"].clone();
    assert_eq!(find("calendar", "Privat"), json!("#aabbcc"));
    assert_eq!(find("addressbook", "Freunde"), json!("#112233"));
    assert_eq!(find("event", "Quartalsreview"), json!("#00ff00"));
    assert_eq!(c.get_json(&format!("/api/events/alice/{}/review", s.arbeit)).await.json()["color"], "#00ff00");

    // recolouring a category: the objects with that category still carrying the old colour (an
    // object without a COLOR of its own never matches) — count only, then apply
    let r = c.json(Method::POST, "/api/colors/category", json!({ "scope": "event", "name": "Arbeit", "from": "", "to": "#123456" })).await;
    assert_eq!(r.ok().json()["count"], 0);
    let r = c.json(Method::POST, "/api/colors/category", json!({ "scope": "event", "name": "Arbeit", "from": "#00FF00", "to": "#123456" })).await;
    assert_eq!(r.ok().json()["count"], 1, "the review");
    assert_eq!(c.get_json(&format!("/api/events/alice/{}/review", s.arbeit)).await.json()["color"], "#00ff00", "counting changes nothing");
    let r = c.json(Method::POST, "/api/colors/category", json!({ "scope": "event", "name": "Arbeit", "from": "#00ff00", "to": "#123456", "apply": true })).await;
    assert_eq!(r.json()["count"], 1);
    assert_eq!(c.get_json(&format!("/api/events/alice/{}/review", s.arbeit)).await.json()["color"], "#123456");
    assert_eq!(c.get_json(&format!("/api/events/alice/{}/standup", s.arbeit)).await.json()["color"], "");

    // the optimiser's batch: one good change, one for an object that does not exist
    let r = c.json(Method::POST, "/api/colors/batch", json!({ "changes": [
        { "kind": "event", "key": format!("alice/{}/zahnarzt", s.privat), "color": "#654321" },
        { "kind": "event", "key": format!("alice/{}/missing", s.privat), "color": "#654321" },
    ] })).await;
    assert_eq!(r.ok().json(), json!({ "count": 1, "failed": 1 }));
}

#[tokio::test]
async fn mentions_index_shape() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let m = c.get_json("/api/mentions").await.ok().json();
    assert_eq!(m["contacts"], json!([
        { "name": "ACME GmbH", "email": "", "key": format!("alice/{}/acme", s.freunde) },
        { "name": "Anna Schmidt", "email": "anna@example.org", "key": format!("alice/{}/anna", s.freunde) },
    ]));
    assert_eq!(m["categories"][0], json!({ "name": "Arbeit", "count": 2 }));
    let tasks: Vec<&str> = m["tasks"].as_array().unwrap().iter().map(|t| t["summary"].as_str().unwrap()).collect();
    assert_eq!(tasks.len(), 2, "open tasks only: {tasks:?}");
    assert!(m["events"].is_array());
}

/// `POST …/move` (the views' drag and drop): whole days, times kept; one occurrence or the whole series; the group's
/// calendar as the user's own; refusals.
#[tokio::test]
async fn event_move_by_days() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let starts = |w: &serde_json::Value, id: &str| -> Vec<String> { w.as_array().unwrap().iter().filter(|i| i["object_id"] == id).map(|i| i["start"].as_str().unwrap().to_string()).collect() };
    let win = "/api/events?start=2026-10-01&end=2026-11-01";
    let from14 = |w: &serde_json::Value| starts(w, "standup").into_iter().filter(|x| x.as_str() >= "2026-10-14").collect::<Vec<_>>();
    // a three-day fair two days later, whole
    c.json(Method::POST, &format!("/api/events/alice/{}/messe/move", s.arbeit), json!({ "days": 2 })).await.ok();
    let w = c.get_json(win).await.ok().json();
    let m = w.as_array().unwrap().iter().find(|i| i["object_id"] == "messe").unwrap();
    assert_eq!((m["start"].as_str(), m["end"].as_str()), (Some("2026-10-22T00:00:00"), Some("2026-10-25T00:00:00")));
    // one standup (Wednesdays 09:00) to Thursday: only that one moves
    let sp = format!("/api/events/alice/{}/standup/move", s.arbeit);
    c.json(Method::POST, &sp, json!({ "days": 1, "at": "2026-10-14T09:00:00" })).await.ok();
    // the moved one again, two days on and back: its override is found however far it lies from its own day
    c.json(Method::POST, &sp, json!({ "days": 2, "at": "2026-10-15T09:00:00" })).await.ok();
    c.json(Method::POST, &sp, json!({ "days": -2, "at": "2026-10-17T09:00:00" })).await.ok();
    let w = c.get_json(win).await.ok().json();
    assert_eq!(from14(&w), ["2026-10-15T09:00:00", "2026-10-21T09:00:00", "2026-10-28T09:00:00"]);
    // the whole series a day earlier: Tuesdays, the moved one with it (to the 14th), the exception too (no 6th)
    c.json(Method::POST, &sp, json!({ "days": -1 })).await.ok();
    let w = c.get_json(win).await.ok().json();
    assert_eq!(from14(&w), ["2026-10-14T09:00:00", "2026-10-20T09:00:00", "2026-10-27T09:00:00"]);
    assert!(!starts(&w, "standup").contains(&"2026-10-06T09:00:00".to_string()), "the exception moved along");
    // the group's calendar is hers to change; bob's own calendars are not
    c.json(Method::POST, &format!("/api/events/group.familie/{}/treffen/move", s.familie), json!({ "days": -3 })).await.ok();
    assert_eq!(starts(&c.get_json(win).await.ok().json(), "treffen"), vec!["2026-10-15T00:00:00".to_string()]);
    assert_eq!(c.json(Method::POST, "/api/events/bob/x/y/move", json!({ "days": 1 })).await.status, StatusCode::FORBIDDEN);
    // nothing to do, an occurrence that is not there, a rule that cannot follow
    assert_eq!(c.json(Method::POST, &sp, json!({ "days": 0 })).await.status, StatusCode::BAD_REQUEST);
    assert_eq!(c.json(Method::POST, &sp, json!({ "days": i64::MIN })).await.status, StatusCode::BAD_REQUEST, "no overflow: a 400, not a panic");
    assert_eq!(c.json(Method::POST, &sp, json!({ "days": 1, "at": "2026-10-16T09:00:00" })).await.status, StatusCode::CONFLICT);
    s.app.put_ics("alice", &s.arbeit, "letzter", &crate::testkit::vevent("letzter", "Letzter Werktag", "DTSTART;TZID=Europe/Berlin:20261030T090000\nDTEND;TZID=Europe/Berlin:20261030T100000\nRRULE:FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1")).await;
    let r = c.json(Method::POST, &format!("/api/events/alice/{}/letzter/move", s.arbeit), json!({ "days": 1 })).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.body);
}
