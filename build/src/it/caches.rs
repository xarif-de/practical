//! The request caches (`state::Caches`, `history::PlanMemo`): every answer read from a cache must equal
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

/// The history page plans every change with one `PlanMemo`; each plan must equal a plan made alone.
#[tokio::test]
async fn shared_plan_memo_plans_like_a_fresh_one() {
    let s = crate::it::seed::seed().await;
    let mut c = s.app.login("alice").await;
    let input = json!({ "principal": "alice", "cal_id": s.privat, "summary": "Yoga", "start": "2026-10-23T18:00:00", "end": "2026-10-23T19:00:00" });
    let id = c.json(Method::POST, "/api/events", input.clone()).await.ok().json()["id"].as_str().unwrap().to_string();
    let path = format!("/api/events/alice/{}/{id}", s.privat);
    for (n, place) in ["Park", "Halle", "Garten"].iter().enumerate() {
        let mut upd = input.clone();
        upd["location"] = json!(place);
        upd["summary"] = json!(format!("Yoga {n}"));
        upd["etag"] = c.get_json(&path).await.json()["etag"].clone();
        c.json(Method::PUT, &path, upd).await.ok();
    }
    let other = c.json(Method::POST, "/api/events", json!({ "principal": "alice", "cal_id": s.privat, "summary": "Weg", "start": "2026-10-24T18:00:00", "end": "2026-10-24T19:00:00" })).await.ok().json()["id"].as_str().unwrap().to_string();
    c.delete(&format!("/api/events/alice/{}/{other}", s.privat)).await.ok();
    let user = s.app.principal("alice").await;
    let all = crate::domain::history::visible(&s.app.state, &user).await.unwrap();
    // an undone change and a change made inactive by it are in the set
    crate::domain::history::undo(&s.app.state, &user, all[2].id, true).await.unwrap().ok().expect("undone");
    let all = crate::domain::history::visible(&s.app.state, &user).await.unwrap();
    assert!(all.len() >= 7, "{}", all.len());
    let show = |p: &crate::domain::history::Plan| format!("{} {:?} {:?} {:?} {:?}", p.ok, p.why, p.warns, p.skips, p.now);
    let mut shared = crate::domain::history::PlanMemo::default();
    shared.prefetch_blobs(&s.app.state, &all).await.unwrap();
    let mut seen = std::collections::HashSet::new();
    for ch in &all {
        let a = crate::domain::history::plan(&s.app.state, &user, ch, &all, &mut shared).await.unwrap();
        let b = crate::domain::history::plan(&s.app.state, &user, ch, &all, &mut Default::default()).await.unwrap();
        assert_eq!(show(&a), show(&b), "change {}", ch.id);
        seen.insert(format!("{} {:?}", a.ok, a.why));
    }
    assert!(seen.len() >= 3, "plans of several kinds: {seen:?}");
    // undoing the create warns about exactly the active later changes of that element (the scan the index replaces)
    let create = all.iter().find(|x| x.items.iter().any(|i| i.object_id == id && i.act == "create")).unwrap();
    let uid = &create.items[0].uid;
    let expected: Vec<i64> = all.iter().filter(|x| x.id > create.id && x.active() && x.items.iter().any(|i| &i.uid == uid)).map(|x| x.id).collect();
    assert!(!expected.is_empty());
    let p = crate::domain::history::plan(&s.app.state, &user, create, &all, &mut shared).await.unwrap();
    assert_eq!(p.warns.iter().filter(|w| w.kind == "later").map(|w| w.ids.clone()).collect::<Vec<_>>(), [expected]);
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

/// The five year views from `routes::more::YearCache`: each answer equals the uncached one (cache emptied)
/// after a DAV write, an override with its own title, an own colour, a rename + recolour (no synctoken move),
/// a zone change and a deletion; a second request is served from the cache.
#[tokio::test]
async fn year_views_follow_writes_renames_recolours_and_zones() {
    let (app, a, b) = app().await;
    let user = app.principal("alice").await;
    app.put_ics("alice", &a, "y1", &vevent("y1", "Serie", "DTSTART;TZID=Europe/Berlin:20260105T090000\nDTEND;TZID=Europe/Berlin:20260105T100000\nRRULE:FREQ=WEEKLY;COUNT=60\nCOLOR:tomato")).await;
    app.put_ics("alice", &b, "y2", &vevent("y2", "Urlaub Meer", "DTSTART;VALUE=DATE:20260803\nDTEND;VALUE=DATE:20260815\nCATEGORIES:Urlaub")).await;
    app.put_ics("alice", &b, "y3", &vevent("y3", "Spät", "DTSTART:20261231T233000Z\nDTEND:20261231T234500Z")).await;
    let mut c = app.login("alice").await;
    let views = ["planner", "weekends", "vacation", "countdown", "numbers"];
    async fn both(app: &TestApp, c: &mut crate::testkit::Client, views: &[&str]) -> Vec<String> {
        let mut out = vec![];
        for v in views {
            let cached = c.get(&format!("/calendar?view={v}&date=2026-10-14&fragment=1")).await;
            cached.ok();
            app.state.caches.years.clear();
            let fresh = c.get(&format!("/calendar?view={v}&date=2026-10-14&fragment=1")).await;
            assert_eq!(cached.body, fresh.body, "{v}: cached = uncached");
            out.push(fresh.body.clone());
        }
        out
    }
    let first = both(&app, &mut c, &views).await;
    assert!(first[0].contains("title=\"Serie\"") && first[2].contains("Urlaub Meer") && first[4].contains("--c:tomato"), "{}", first[4]);
    assert_eq!(app.state.caches.years.len(), 2, "one entry per calendar");
    let n0 = first.clone();
    assert_eq!(both(&app, &mut c, &views).await, n0, "served again from the cache");
    // an override of one occurrence with its own title, in the same object
    let series = "BEGIN:VEVENT\nUID:y1\nDTSTAMP:20260901T000000Z\nSUMMARY:Serie\nDTSTART;TZID=Europe/Berlin:20260105T090000\nDTEND;TZID=Europe/Berlin:20260105T100000\nRRULE:FREQ=WEEKLY;COUNT=60\nCOLOR:tomato\nEND:VEVENT";
    let over = "BEGIN:VEVENT\nUID:y1\nDTSTAMP:20260901T000000Z\nRECURRENCE-ID;TZID=Europe/Berlin:20261012T090000\nSUMMARY:Ausnahme\nDTSTART;TZID=Europe/Berlin:20261012T090000\nDTEND;TZID=Europe/Berlin:20261012T100000\nEND:VEVENT";
    app.put_ics("alice", &a, "y1", &crate::testkit::ics(&format!("{series}\n{over}"))).await;
    let after = both(&app, &mut c, &views).await;
    assert!(after[0].contains("title=\"Ausnahme\"") && after[0].contains("title=\"Serie\""), "{}", after[0]);
    // rename and recolour move no synctoken
    crate::domain::collections::update_calendar(&app.state, &user, "alice", &b, "Freizeit", "#123456", "").await.unwrap();
    let renamed = both(&app, &mut c, &views).await;
    assert!(renamed[4].contains("Freizeit") && renamed[4].contains("--c:#123456"), "{}", renamed[4]);
    // another zone: 23:30 UTC on 31 December is 1 January 2027 in Berlin, still 2026 in New York
    c.post_form_json("/settings/prefs", &[("timezone", "America/New_York")]).await.ok();
    let ny = both(&app, &mut c, &views).await;
    assert_ne!(ny[4], renamed[4]);
    // a deletion
    app.state.calendars.delete_object("alice", &b, "y2", true).await.unwrap();
    let gone = both(&app, &mut c, &views).await;
    assert!(!gone[2].contains("Urlaub Meer"));
}

/// The history document's cached part (`routes::history::HistoryCache`): after every kind of change that
/// shows in it — a UI write, a DAV write before and after the poll finds it, an undo, a collection rename,
/// a principal's new name, a group membership, a purge, a zone or language switch — the answer equals the
/// uncached one; an unchanged second request is served from the cache.
#[tokio::test]
async fn history_document_follows_every_change() {
    let s = crate::it::seed::seed().await;
    let app = &s.app;
    let mut c = app.login("alice").await;
    crate::domain::history::poll(&app.state).await;
    async fn check(app: &TestApp, c: &mut crate::testkit::Client, what: &str) -> serde_json::Value {
        let strip = |mut v: serde_json::Value| {
            v.as_object_mut().unwrap().remove("now");
            v
        };
        let cached = strip(c.get_json("/api/history").await.ok().json());
        app.state.caches.history.clear();
        let fresh = strip(c.get_json("/api/history").await.ok().json());
        assert_eq!(cached, fresh, "{what}: cached = uncached");
        // the page carries the same document
        let page = c.get("/history").await;
        assert_eq!(strip(page.island("history-data")), fresh, "{what}: page");
        fresh
    }
    let input = json!({ "principal": "alice", "cal_id": s.privat, "summary": "Yoga", "start": "2026-10-23T18:00:00", "end": "2026-10-23T19:00:00" });
    let id = c.json(Method::POST, "/api/events", input.clone()).await.ok().json()["id"].as_str().unwrap().to_string();
    let h = check(app, &mut c, "UI write").await;
    assert_eq!(app.state.caches.history.len(), 1);
    let before = app.state.history.generation();
    assert_eq!(check(app, &mut c, "nothing changed").await, h);
    assert_eq!(app.state.history.generation(), before, "reading moves nothing");
    // defaults are left out (history.js puts them back)
    let ch = &h["changes"][0];
    for k in ["dev", "prodid", "label", "reverts", "undoneBy", "inactiveBy", "tech"] {
        assert!(ch.get(k).is_none(), "{k}: {ch}");
    }
    assert_eq!(ch["undo"], json!({ "ok": true }));
    assert_eq!(ch["who"], "alice");
    // a DAV write: seen at once in the objects (synctoken), then as a change after the poll
    let path = format!("/api/events/alice/{}/{id}", s.privat);
    let mut upd = input.clone();
    upd["summary"] = json!("Yoga im Park");
    upd["etag"] = c.get_json(&path).await.json()["etag"].clone();
    c.json(Method::PUT, &path, upd).await.ok();
    app.put_ics("alice", &s.privat, "dav1", &vevent("dav1", "Vom Telefon", "DTSTART;TZID=Europe/Berlin:20261024T090000\nDTEND;TZID=Europe/Berlin:20261024T100000")).await;
    check(app, &mut c, "DAV write before the poll").await;
    crate::domain::history::poll(&app.state).await;
    let h = check(app, &mut c, "DAV write after the poll").await;
    let dav = h["changes"].as_array().unwrap().last().unwrap();
    assert_eq!((dav["src"].as_str(), dav["who"].is_null(), dav["prodid"].as_str()), (Some("dav"), true, Some("-//test//test//EN")));
    // an undo: the undone change names its undo, the plan of the others changes
    let upd_id = h["changes"][1]["id"].as_i64().unwrap();
    let u = c.json(Method::POST, &format!("/api/history/{upd_id}/undo"), json!({ "confirm": true })).await.ok().json()["id"].as_i64().unwrap();
    let h = check(app, &mut c, "undo").await;
    assert_eq!(h["changes"][1]["undoneBy"], u);
    // a rename moves no synctoken; a principal's new name; a new group member sees the group's changes
    let user = app.principal("alice").await;
    crate::domain::collections::update_calendar(&app.state, &user, "alice", &s.privat, "Freizeit", "#123456", "").await.unwrap();
    assert_eq!(check(app, &mut c, "rename").await["colls"][format!("alice/{}", s.privat)]["name"], "Freizeit");
    sqlx::query("UPDATE principals SET displayname = 'Alice Neu' WHERE id = 'alice'").execute(&app.state.db).await.unwrap();
    assert_eq!(check(app, &mut c, "principal name").await["people"]["alice"]["name"], "Alice Neu");
    let mut carol = app.login("carol").await;
    assert_eq!(check(app, &mut carol, "carol").await["changes"], json!([]));
    app.put_ics("group.familie", &s.familie, "fam1", &vevent("fam1", "Grillen", "DTSTART;VALUE=DATE:20261025\nDTEND;VALUE=DATE:20261026")).await;
    crate::domain::history::poll(&app.state).await;
    check(app, &mut carol, "carol, group change").await;
    rustical_store::auth::AuthenticationProvider::add_membership(&*app.state.principals, "carol", "group.familie").await.unwrap();
    let mut carol = app.login("carol").await;
    assert!(check(app, &mut carol, "carol joined").await["changes"].as_array().unwrap().iter().any(|x| x["items"][0]["title"] == "Grillen"));
    // zone and language
    c.post_form_json("/settings/prefs", &[("timezone", "America/New_York"), ("lang", "en")]).await.ok();
    let ny = check(app, &mut c, "zone + language").await;
    assert_ne!(ny["changes"][0]["at"], h["changes"][0]["at"]);
    // a purge
    sqlx::query("UPDATE practical_history_changes SET at = '2000-01-01 00:00:00' WHERE id = ?").bind(h["changes"][0]["id"].as_i64()).execute(&app.state.db).await.unwrap();
    // (that SQL write bypasses the history module: the entry made now is stale until the purge moves the generation)
    c.get_json("/api/history").await.ok();
    let first = h["changes"][0]["id"].clone();
    assert!(crate::domain::history::purge(&app.state).await.unwrap() > 0);
    assert!(!check(app, &mut c, "purge").await["changes"].as_array().unwrap().iter().any(|x| x["id"] == first));
}

/// The search corpus is brought up to date per object (`search::index`): after a UI write, a DAV write, a
/// series override, a deletion, a RELATED-TO target's new title, a calendar rename, a contact change and a
/// zone change, every query answers exactly as a corpus built from scratch; objects nobody touched keep
/// their document.
#[tokio::test]
async fn search_corpus_follows_writes_per_object() {
    let s = crate::it::seed::seed().await;
    let app = &s.app;
    let mut c = app.login("alice").await;
    let queries = ["standup", "review", "zahn", "anna", "steuer", "garten", "ostsee", "lauf", "familie", "14.10.", "ev: arbeit", "tk: keller", "partner"];
    async fn both(app: &TestApp, c: &mut crate::testkit::Client, qs: &[&str], what: &str) {
        for q in qs {
            let path = format!("/api/search?q={}&limit=200", url::form_urlencoded::byte_serialize(q.as_bytes()).collect::<String>());
            let kept = c.get_json(&path).await.ok().json();
            app.state.search_index.clear();
            let fresh = c.get_json(&path).await.ok().json();
            assert_eq!(kept, fresh, "{what}: {q}");
        }
    }
    both(app, &mut c, &queries, "start").await;
    let ptrs = app.state.search_index.doc_ptrs("alice");
    assert!(ptrs.len() > 5, "{ptrs:?}");
    // a UI write: one new object, every other document kept
    let input = json!({ "principal": "alice", "cal_id": s.privat, "summary": "Lauftreff im Garten", "start": "2026-10-23T18:00:00", "end": "2026-10-23T19:00:00" });
    let id = c.json(Method::POST, "/api/events", input.clone()).await.ok().json()["id"].as_str().unwrap().to_string();
    c.get_json("/api/search?q=lauf").await.ok();
    let now = app.state.search_index.doc_ptrs("alice");
    assert_eq!(now.len(), ptrs.len() + 1);
    // (one with RELATED-TO gets its titles filled in anew on every rebuild: "review" names the standup)
    assert!(ptrs.iter().filter(|(k, _)| !k.ends_with("/review")).all(|(k, p)| now.get(k) == Some(p)), "untouched objects keep their documents");
    assert_ne!(ptrs.iter().find(|(k, _)| k.ends_with("/review")).map(|x| x.1), None);
    both(app, &mut c, &queries, "UI write").await;
    // a DAV write with RELATED-TO the new event, then the event's title changes: the relation follows
    app.put_ics("alice", &s.arbeit, "partner", &vevent("partner", "Partnertermin", &format!("DTSTART;TZID=Europe/Berlin:20261026T090000\nDTEND;TZID=Europe/Berlin:20261026T100000\nRELATED-TO:{id}"))).await;
    both(app, &mut c, &queries, "DAV write").await;
    let mut upd = input.clone();
    upd["summary"] = json!("Lauftreff an der Elbe");
    upd["etag"] = c.get_json(&format!("/api/events/alice/{}/{id}", s.privat)).await.json()["etag"].clone();
    c.json(Method::PUT, &format!("/api/events/alice/{}/{id}", s.privat), upd).await.ok();
    c.get_json("/api/search?q=partner").await.ok();
    both(app, &mut c, &["partner", "elbe", "lauf", "garten"], "related title").await;
    let r = c.get_json("/api/search?q=partner%20elbe").await.json();
    assert_eq!(r["events"][0]["summary"], "Partnertermin", "{r}");
    // an override of one standup occurrence; a deletion
    let series = "BEGIN:VEVENT\nUID:standup\nDTSTAMP:20260901T000000Z\nSUMMARY:Standup\nDTSTART;TZID=Europe/Berlin:20260902T090000\nDTEND;TZID=Europe/Berlin:20260902T091500\nRRULE:FREQ=WEEKLY;BYDAY=WE\nEND:VEVENT";
    let over = "BEGIN:VEVENT\nUID:standup\nDTSTAMP:20260901T000000Z\nRECURRENCE-ID;TZID=Europe/Berlin:20261021T090000\nSUMMARY:Standup im Garten\nDTSTART;TZID=Europe/Berlin:20261021T100000\nDTEND;TZID=Europe/Berlin:20261021T101500\nEND:VEVENT";
    app.put_ics("alice", &s.arbeit, "standup", &crate::testkit::ics(&format!("{series}\n{over}"))).await;
    both(app, &mut c, &queries, "override").await;
    app.state.calendars.delete_object("alice", &s.privat, "zahnarzt", true).await.unwrap();
    both(app, &mut c, &queries, "deletion").await;
    // a rename moves no synctoken; a contact; another zone
    let user = app.principal("alice").await;
    crate::domain::collections::update_calendar(&app.state, &user, "alice", &s.arbeit, "Büro", "#123456", "").await.unwrap();
    both(app, &mut c, &["ev: büro", "review", "standup"], "rename").await;
    assert_eq!(c.get_json("/api/search?q=review").await.json()["events"][0]["cal_name"], "Büro");
    app.put_vcf("alice", &s.freunde, "anna", "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:anna\r\nFN:Anna Gärtner\r\nN:Gärtner;Anna;;;\r\nEND:VCARD\r\n").await;
    both(app, &mut c, &["anna", "gärtner", "schmidt"], "contact").await;
    c.post_form_json("/settings/prefs", &[("timezone", "America/New_York")]).await.ok();
    both(app, &mut c, &queries, "zone").await;
    assert_eq!(app.state.search_index.users(), 1);
}
