//! The request caches (`state::Caches`): every answer read from a cache must equal
//! what the uncached read gives after any write — a DAV client's, the app's own, a rename or recolour
//! that leaves the synctoken where it is, and a change of the user's zone. Each test reads, writes,
//! and reads back.

use crate::testkit::{TestApp, vevent};
use axum::http::Method;
use rustical_store::{AddressbookWriteStore, CalendarWriteStore};
use serde_json::json;

async fn app() -> (TestApp, String, String) {
    let app = TestApp::new().await;
    app.user("alice", "Alice").await;
    let a = app.calendar("alice", "Arbeit", "#0a84ff", &["VEVENT", "VTODO"]).await;
    let b = app.calendar("alice", "Privat", "#30b46c", &["VEVENT"]).await;
    (app, a, b)
}

#[tokio::test]
async fn density_follows_writes_the_zone_and_the_filter() {
    let (app, a, b) = app().await;
    app.put_ics("alice", &a, "a1", &vevent("a1", "Eins", "DTSTART;TZID=Europe/Berlin:20261005T090000\nDTEND;TZID=Europe/Berlin:20261005T100000")).await;
    app.put_ics("alice", &b, "b1", &vevent("b1", "Zwei", "DTSTART;TZID=Europe/Berlin:20260310T090000\nDTEND;TZID=Europe/Berlin:20260310T100000")).await;
    let mut c = app.login("alice").await;
    let density = |v: [u64; 12]| json!(v);
    assert_eq!(c.get_json("/api/density?year=2026").await.ok().json(), density([0, 0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0]));
    // twice: the second answer comes from the cache
    assert_eq!(c.get_json("/api/density?year=2026").await.ok().json(), density([0, 0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0]));
    // a DAV write and a weekly series in one calendar; the other calendar's counts stay cached
    app.put_ics("alice", &a, "a2", &vevent("a2", "Serie", "DTSTART;TZID=Europe/Berlin:20261102T090000\nDTEND;TZID=Europe/Berlin:20261102T100000\nRRULE:FREQ=WEEKLY;COUNT=3")).await;
    assert_eq!(c.get_json("/api/density?year=2026").await.ok().json(), density([0, 0, 1, 0, 0, 0, 0, 0, 0, 1, 3, 0]));
    // moved by an edit, then deleted through the app
    app.put_ics("alice", &a, "a1", &vevent("a1", "Eins", "DTSTART;TZID=Europe/Berlin:20261205T090000\nDTEND;TZID=Europe/Berlin:20261205T100000")).await;
    assert_eq!(c.get_json("/api/density?year=2026").await.ok().json(), density([0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 3, 1]));
    c.delete(&format!("/api/events/alice/{a}/a1")).await.ok();
    assert_eq!(c.get_json("/api/density?year=2026").await.ok().json(), density([0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 3, 0]));
    // the filter sums only its calendars
    assert_eq!(c.get_json(&format!("/api/density?year=2026&cals=alice/{b}")).await.ok().json(), density([0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
    // New Year's Eve 23:30 UTC is 2027 in Berlin and 2026 in New York: the zone is part of the key
    app.put_ics("alice", &b, "b2", &vevent("b2", "Silvester", "DTSTART:20261231T233000Z\nDTEND:20261231T234500Z")).await;
    assert_eq!(c.get_json("/api/density?year=2026").await.ok().json(), density([0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 3, 0]));
    assert_eq!(c.get_json("/api/density?year=2027").await.ok().json(), density([1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
    c.post_form("/settings/prefs", &[("lang", "de"), ("timezone", "America/New_York"), ("vacation_kind", "title"), ("vacation_title", "Urlaub")]).await;
    assert_eq!(c.get_json("/api/density?year=2026").await.ok().json(), density([0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 3, 1]));
    assert_eq!(c.get_json("/api/density?year=2027").await.ok().json(), density([0; 12]));
}

#[tokio::test]
async fn category_title_and_colour_scans_follow_writes() {
    let (app, a, _) = app().await;
    let mut c = app.login("alice").await;
    app.put_ics("alice", &a, "e1", &vevent("e1", "Laufen", "DTSTART:20261005T090000Z\nCATEGORIES:Sport,Draußen\nCOLOR:tomato")).await;
    app.put_ics("alice", &a, "t1", &crate::testkit::vtodo("t1", "Schuhe kaufen", "CATEGORIES:Sport\nCOLOR:gold")).await;
    let cats = |v: serde_json::Value| v.as_array().unwrap().iter().map(|x| format!("{}:{}", x["name"].as_str().unwrap(), x["count"])).collect::<Vec<_>>();
    assert_eq!(cats(c.get_json("/api/categories?scope=event").await.ok().json()), ["Draußen:1", "Sport:1"]);
    assert_eq!(c.get_json("/api/titles").await.ok().json(), json!([["Laufen", 1]]));
    let colours = |v: serde_json::Value| v.as_array().unwrap().iter().filter(|x| x["kind"] == "event" || x["kind"] == "task").map(|x| format!("{} {} {} {}", x["kind"].as_str().unwrap(), x["name"].as_str().unwrap(), x["color"].as_str().unwrap(), x["cal"].as_str().unwrap())).collect::<Vec<_>>();
    assert_eq!(colours(c.get_json("/api/colors").await.ok().json()), ["event Laufen tomato Arbeit", "task Schuhe kaufen gold Arbeit"]);
    // an edit (same object id), a second event sorting before the first by UID, a deletion
    app.put_ics("alice", &a, "e1", &vevent("e1", "Radfahren", "DTSTART:20261005T090000Z\nCATEGORIES:Rad")).await;
    app.put_ics("alice", &a, "e0", &vevent("a-first", "Schwimmen", "DTSTART:20261006T090000Z\nCATEGORIES:Sport\nCOLOR:navy")).await;
    assert_eq!(cats(c.get_json("/api/categories?scope=event").await.ok().json()), ["Rad:1", "Sport:1"]);
    assert_eq!(c.get_json("/api/titles").await.ok().json(), json!([["Radfahren", 1], ["Schwimmen", 1]]));
    assert_eq!(colours(c.get_json("/api/colors").await.ok().json()), ["event Schwimmen navy Arbeit", "task Schuhe kaufen gold Arbeit"]);
    // a rename moves no synctoken: the calendar's name is read fresh
    let user = app.principal("alice").await;
    crate::domain::collections::update_calendar(&app.state, &user, "alice", &a, "Büro", "#0a84ff", "").await.unwrap();
    assert_eq!(colours(c.get_json("/api/colors").await.ok().json()), ["event Schwimmen navy Büro", "task Schuhe kaufen gold Büro"]);
    app.state.calendars.delete_object("alice", &a, "e0", true).await.unwrap();
    assert_eq!(cats(c.get_json("/api/categories?scope=event").await.ok().json()), ["Rad:1"]);
    assert_eq!(colours(c.get_json("/api/colors").await.ok().json()), ["task Schuhe kaufen gold Büro"]);
    // the colour optimiser's category count reads the same records
    let n = c.json(Method::POST, "/api/colors/category", json!({ "scope": "task", "name": "Sport", "from": "gold", "to": "#ff0000" })).await.ok().json();
    assert_eq!(n, json!({ "count": 1 }));
}

#[tokio::test]
async fn contacts_follow_writes_and_address_book_renames() {
    let app = TestApp::new().await;
    let user = app.user("alice", "Alice").await;
    let ab = app.addressbook("alice", "Freunde", "#0a84ff").await;
    let vcf = |uid: &str, fname: &str| format!("BEGIN:VCARD\r\nVERSION:3.0\r\nUID:{uid}\r\nFN:{fname}\r\nN:;{fname};;;\r\nEND:VCARD\r\n");
    app.put_vcf("alice", &ab, "c1", &vcf("c1", "Anna")).await;
    app.put_vcf("alice", &ab, "c2", &vcf("c2", "Bert")).await;
    let read = || async {
        let abs = crate::domain::collections::addressbooks_for(&app.state, &user, false).await.unwrap();
        crate::domain::contacts::contacts_for(&app.state, &abs).await.unwrap().iter().map(|c| format!("{} ({}, {})", c.full_name, c.ab_name, c.ab_color)).collect::<Vec<_>>()
    };
    assert_eq!(read().await, ["Anna (Freunde, #0a84ff)", "Bert (Freunde, #0a84ff)"]);
    assert_eq!(read().await, ["Anna (Freunde, #0a84ff)", "Bert (Freunde, #0a84ff)"]);
    app.put_vcf("alice", &ab, "c1", &vcf("c1", "Zora")).await;
    AddressbookWriteStore::delete_object(&*app.state.addressbooks, "alice", &ab, "c2", true).await.unwrap();
    app.put_vcf("alice", &ab, "c3", &vcf("c3", "Carl")).await;
    assert_eq!(read().await, ["Carl (Freunde, #0a84ff)", "Zora (Freunde, #0a84ff)"]);
    // name and colour change without the synctoken
    crate::domain::collections::update_addressbook(&app.state, &user, "alice", &ab, "Familie", "#ff3b30", "").await.unwrap();
    assert_eq!(read().await, ["Carl (Familie, #ff3b30)", "Zora (Familie, #ff3b30)"]);
    // the page and the note editor's index read the same
    let mut c = app.login("alice").await;
    c.get("/contacts").await.ok().has("Zora").has("Carl");
    let names: Vec<String> = c.get_json("/api/mentions").await.ok().json()["contacts"].as_array().unwrap().iter().map(|x| x["name"].as_str().unwrap().to_string()).collect();
    assert_eq!(names, ["Carl", "Zora"]);
}

#[tokio::test]
async fn mention_events_follow_writes_and_the_zone() {
    let (app, a, _) = app().await;
    let mut c = app.login("alice").await;
    let today = crate::clock::today_in(&chrono_tz::Europe::Berlin);
    let at = |d: i64| (today + chrono::Duration::days(d)).format("%Y%m%d").to_string();
    app.put_ics("alice", &a, "m1", &vevent("m1", "Treffen", &format!("DTSTART;VALUE=DATE:{}", at(3)))).await;
    let events = |v: serde_json::Value| v["events"].as_array().unwrap().iter().map(|x| format!("{} {}", x["summary"].as_str().unwrap(), x["date"].as_str().unwrap())).collect::<Vec<_>>();
    let d = |n: i64| (today + chrono::Duration::days(n)).to_string();
    assert_eq!(events(c.get_json("/api/mentions").await.ok().json()), [format!("Treffen {}", d(3))]);
    app.put_ics("alice", &a, "m2", &vevent("m2", "Kino", &format!("DTSTART;VALUE=DATE:{}", at(1)))).await;
    assert_eq!(events(c.get_json("/api/mentions").await.ok().json()), [format!("Kino {}", d(1)), format!("Treffen {}", d(3))]);
    app.state.calendars.delete_object("alice", &a, "m1", true).await.unwrap();
    assert_eq!(events(c.get_json("/api/mentions").await.ok().json()), [format!("Kino {}", d(1))]);
    // 23:30 UTC the day after tomorrow is the next day in Berlin, the same day in New York
    app.put_ics("alice", &a, "m3", &vevent("m3", "Spät", &format!("DTSTART:{}T233000Z", at(2)))).await;
    assert_eq!(events(c.get_json("/api/mentions").await.ok().json()), [format!("Kino {}", d(1)), format!("Spät {}", d(3))]);
    c.post_form("/settings/prefs", &[("lang", "de"), ("timezone", "America/New_York"), ("vacation_kind", "title"), ("vacation_title", "Urlaub")]).await;
    let ny_today = crate::clock::today_in(&chrono_tz::America::New_York);
    let got = events(c.get_json("/api/mentions").await.ok().json());
    assert_eq!(got.last().unwrap(), &format!("Spät {}", today + chrono::Duration::days(2)), "{got:?} (New York today {ny_today})");
}

#[tokio::test]
async fn holiday_rows_are_reread_after_a_refresh() {
    let app = TestApp::new().await;
    let names = |h: Vec<crate::domain::holidays::Holiday>| h.into_iter().map(|x| x.name).collect::<Vec<_>>();
    let first = names(crate::domain::holidays::get(&app.state, "public", "DE", "DE-SN", 2026, "de").await.unwrap());
    assert_eq!(first.len(), 4);
    assert_eq!(names(crate::domain::holidays::get(&app.state, "public", "DE", "DE-SN", 2026, "de").await.unwrap()), first, "the cached answer");
    crate::domain::holidays::cache_put_for_test(&app.state, "DE", "DE-SN", 2026, "public2-de", &json!([{ "start": "2026-05-01", "end": "2026-05-01", "name": "Maifeiertag", "kind": "public" }]).to_string()).await;
    assert_eq!(names(crate::domain::holidays::get(&app.state, "public", "DE", "DE-SN", 2026, "de").await.unwrap()), ["Maifeiertag"]);
}

#[tokio::test]
async fn instances_follow_writes_renames_recolours_and_zones() {
    let (app, a, b) = app().await;
    let user = app.principal("alice").await;
    app.put_ics("alice", &a, "i1", &vevent("i1", "Eigenfarbe", "DTSTART;TZID=Europe/Berlin:20261005T090000\nDTEND;TZID=Europe/Berlin:20261005T100000\nCOLOR:tomato")).await;
    app.put_ics("alice", &a, "i2", &vevent("i2", "Kalenderfarbe", "DTSTART;TZID=Europe/Berlin:20261006T090000\nDTEND;TZID=Europe/Berlin:20261006T100000")).await;
    app.put_ics("alice", &b, "i3", &vevent("i3", "Spät", "DTSTART:20261006T233000Z\nDTEND:20261006T234500Z")).await;
    let read = |tz: chrono_tz::Tz| {
        let (app, user) = (&app, &user);
        async move {
            let cals = crate::domain::collections::calendars_for(&app.state, user, false).await.unwrap();
            crate::domain::events::instances(&app.state, &cals, crate::testkit::date("2026-10-01"), crate::testkit::date("2026-11-01"), tz).await.unwrap().iter().map(|i| format!("{} {} {} {} {}", i.summary, i.start.format("%d %H:%M"), i.cal_name, i.color, i.is_group)).collect::<Vec<_>>()
        }
    };
    let berlin = chrono_tz::Europe::Berlin;
    let first = read(berlin).await;
    assert_eq!(first, ["Eigenfarbe 05 09:00 Arbeit tomato false", "Kalenderfarbe 06 09:00 Arbeit #0a84ff false", "Spät 07 01:30 Privat #30b46c false"]);
    assert_eq!(read(berlin).await, first, "the cached answer");
    // rename and recolour move no synctoken
    crate::domain::collections::update_calendar(&app.state, &user, "alice", &a, "Büro", "#ff9f0a", "").await.unwrap();
    assert_eq!(read(berlin).await, ["Eigenfarbe 05 09:00 Büro tomato false", "Kalenderfarbe 06 09:00 Büro #ff9f0a false", "Spät 07 01:30 Privat #30b46c false"]);
    // another zone is another entry
    assert_eq!(read(chrono_tz::America::New_York).await, ["Eigenfarbe 05 03:00 Büro tomato false", "Kalenderfarbe 06 03:00 Büro #ff9f0a false", "Spät 06 19:30 Privat #30b46c false"]);
    // a write in one calendar; an edit that drops the own colour; a deletion
    app.put_ics("alice", &a, "i1", &vevent("i1", "Ohne Farbe", "DTSTART;TZID=Europe/Berlin:20261005T080000\nDTEND;TZID=Europe/Berlin:20261005T090000")).await;
    app.state.calendars.delete_object("alice", &b, "i3", true).await.unwrap();
    assert_eq!(read(berlin).await, ["Ohne Farbe 05 08:00 Büro #ff9f0a false", "Kalenderfarbe 06 09:00 Büro #ff9f0a false"]);
}
