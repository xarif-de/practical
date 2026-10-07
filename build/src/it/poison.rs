//! Tier-0 chaos corpus: stored objects and inputs that once
//! dropped the connection (a handler panic) or blew a request's memory up, run through every view and API path.
//! The router runs in-process, so a panic anywhere fails the test; the memory-heavy cases are held to a bound
//! measured on the request's own thread (`testkit::peak_of`).

use super::seed::{Seed, seed};
use crate::testkit::{ics, peak_of, vevent, vtodo};
use axum::http::{Method, StatusCode};
use rustical_store::CalendarReadStore;
use serde_json::json;

const MIB: usize = 1024 * 1024;

/// The status of one request, or "panic" when the handler panicked (the connection drop a browser saw).
async fn probe(c: &mut crate::testkit::Client, path: &str, json: bool) -> String {
    use futures_util::FutureExt;
    let r = if json { std::panic::AssertUnwindSafe(c.get_json(path)).catch_unwind().await } else { std::panic::AssertUnwindSafe(c.get(path)).catch_unwind().await };
    r.map_or_else(|_| "panic".to_string(), |r| r.status.as_u16().to_string())
}

fn alarm(trigger: &str) -> String {
    format!("BEGIN:VALARM\nACTION:DISPLAY\nDESCRIPTION:x\nTRIGGER:{trigger}\nEND:VALARM")
}

/// A note of `n` KB (no line breaks: one content line, folded on store).
fn note(n: usize) -> String {
    "Notiz ".repeat(n * 1024 / 6)
}

const AT: &str = "DTSTART;TZID=Europe/Berlin:20261014T100000\nDTEND;TZID=Europe/Berlin:20261014T110000";

/// R1: alarm offsets caldata's parse_duration panics on (i64 overflow, TimeDelta range, non-ASCII digits).
async fn alarm_poison(s: &Seed) {
    let (app, cal, todo) = (&s.app, &s.arbeit, &s.aufgaben);
    app.put_ics("alice", cal, "trig-overflow", &vevent("trig-overflow", "Gift Überlauf", &format!("{AT}\n{}", alarm("-PT99999999999999999999M")))).await;
    app.put_ics("alice", cal, "trig-1e15", &vevent("trig-1e15", "Gift 1e15", &format!("{AT}\n{}", alarm("-PT1000000000000000M")))).await;
    app.put_ics("alice", cal, "trig-digits", &vevent("trig-digits", "Gift Ziffern", &format!("{AT}\n{}", alarm("-PT٣M")))).await;
    app.put_ics("alice", cal, "alarm-dur", &vevent("alarm-dur", "Gift Dauer", &format!("{AT}\nBEGIN:VALARM\nACTION:DISPLAY\nDESCRIPTION:x\nTRIGGER:-PT5M\nDURATION:PT99999999999999999999M\nREPEAT:3\nEND:VALARM"))).await;
    app.put_ics("alice", todo, "todo-trig", &vtodo("todo-trig", "Gift Aufgabe", &format!("DUE;TZID=Europe/Berlin:20261015T170000\n{}", alarm("-PT99999999999999999999M")))).await;
    app.put_ics("alice", todo, "todo-trig-rel", &vtodo("todo-trig-rel", "Gift Aufgabe 2", "DUE;TZID=Europe/Berlin:20261015T170000\nBEGIN:VALARM\nACTION:DISPLAY\nDESCRIPTION:x\nTRIGGER;RELATED=END:-PT1000000000000000M\nEND:VALARM")).await;
}

/// R7a / R3: an all-day event until 9999, an event in the year's last hour.
async fn date_poison(s: &Seed) {
    s.app.put_ics("alice", &s.arbeit, "forever", &vevent("forever", "Für immer", "DTSTART;VALUE=DATE:20261001\nDTEND;VALUE=DATE:99991230")).await;
    s.app.put_ics("alice", &s.arbeit, "y9999", &vevent("y9999", "Ende der Zeit", "DTSTART:99991231T230000Z\nDTEND:99991231T235900Z")).await;
}

/// S1: a minutely series with a 20 KB note, and one with an override.
async fn heavy_series(s: &Seed) {
    s.app.put_ics("alice", &s.arbeit, "minutely", &vevent("minutely", "Jede Minute", &format!("DTSTART;TZID=Europe/Berlin:20261012T000000\nDTEND;TZID=Europe/Berlin:20261012T000500\nRRULE:FREQ=MINUTELY\nDESCRIPTION:{}\nX-BALLAST:{}", note(20), note(4)))).await;
    let over = ics(&format!(
        "BEGIN:VEVENT\nUID:minutely-ov\nDTSTAMP:20260901T000000Z\nSUMMARY:Minute mit Ausnahme\nDTSTART;TZID=Europe/Berlin:20261013T000000\nDTEND;TZID=Europe/Berlin:20261013T000100\nRRULE:FREQ=MINUTELY;INTERVAL=2\nDESCRIPTION:{}\nEND:VEVENT\nBEGIN:VEVENT\nUID:minutely-ov\nDTSTAMP:20260901T000000Z\nRECURRENCE-ID;TZID=Europe/Berlin:20261013T000400\nSUMMARY:Die Ausnahme\nDTSTART;TZID=Europe/Berlin:20261013T000500\nDTEND;TZID=Europe/Berlin:20261013T000600\nEND:VEVENT",
        note(20)
    ));
    s.app.put_ics("alice", &s.arbeit, "minutely-ov", &over).await;
}

/// S6 / #7: a script stored as an attachment, quoted parameter values a DAV client wrote.
async fn foreign_params(s: &Seed) {
    s.app.put_ics("alice", &s.arbeit, "attach-js", &vevent("attach-js", "Anhang", &format!("{AT}\nATTACH;FMTTYPE=text/javascript;FILENAME=x.js;ENCODING=BASE64;VALUE=BINARY:YWxlcnQoMSk="))).await;
    s.app.put_ics("alice", &s.arbeit, "quoted", &vevent("quoted", "Zitat", &format!("{AT}\nURL;X-Q=\"a:b;c\":https://example.org/q\nATTENDEE;CN=\"Doe, John\";DELEGATED-FROM=\"mailto:a@example.org\",\"mailto:b@example.org\":mailto:john@example.org"))).await;
}

/// R2: multi-byte partial dates in a card (CardDAV writes what it likes).
async fn card_poison(s: &Seed) {
    s.app.put_vcf("alice", &s.freunde, "mb", "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:mb\r\nFN:Multi Byte\r\nN:Byte;Multi;;;\r\nX-ABDATE:--1ä1\r\nX-ABDATE:123ä567\r\nEND:VCARD\r\n").await;
}

/// The seed world plus every poison object, stored as a CalDAV client stores them (nothing validated them).
async fn poisoned() -> Seed {
    let s = seed().await;
    alarm_poison(&s).await;
    date_poison(&s).await;
    heavy_series(&s).await;
    foreign_params(&s).await;
    card_poison(&s).await;
    s
}

/// Each group alone: the views over it answer (the old code dropped the connection for each of these).
#[tokio::test]
async fn each_poison_alone_is_survived() {
    let mut bad = vec![];
    for (name, n) in [("alarms", 0), ("dates", 1), ("cards", 2)] {
        let s = seed().await;
        match n {
            0 => alarm_poison(&s).await,
            1 => date_poison(&s).await,
            _ => card_poison(&s).await,
        }
        let mut c = s.app.login("alice").await;
        for (v, json) in [("/calendar?view=month&date=2026-10-14", false), ("/tasks", false), ("/contacts", false), ("/api/events?start=2026-10-01&end=2026-11-01", true), ("/api/search?q=multi", true), ("/api/mentions", true), ("/api/density?year=2026", true)] {
            let st = probe(&mut c, v, json).await;
            if st != "200" {
                bad.push(format!("{name}: {v} → {st}"));
            }
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

const VIEWS: &[&str] = &[
    "/calendar?view=month&date=2026-10-14",
    "/calendar?view=week&date=2026-10-14",
    "/calendar?view=day&date=2026-10-14",
    "/calendar?view=year&date=2026-10-14",
    "/calendar?view=planner&date=2026-10-14",
    "/calendar?view=weekends&date=2026-10-14",
    "/calendar?view=vacation&date=2026-10-14",
    "/calendar?view=countdown&date=2026-10-14",
    "/calendar?view=numbers&date=2026-10-14",
    "/calendar?view=month&date=9999-12-14",
    "/tasks",
    "/contacts",
    "/history",
    "/settings",
];

const APIS: &[&str] = &[
    "/api/events?start=2026-10-01&end=2026-11-01",
    "/api/events?start=9999-11-01&end=9999-12-31",
    "/api/days?start=2026-10-01&count=30",
    "/api/weeks?start=2026-10-05&count=8",
    "/api/density?year=2026",
    "/api/density?year=9999",
    "/api/search?q=gift",
    "/api/search?q=multi",
    "/api/search?q=1.1.",
    "/api/events/suggest?q=gi",
    "/api/mentions",
    "/api/categories",
    "/api/titles",
    "/api/colors",
];

/// Every view and API of the poisoned world answers, and the healthy objects next to the poison still show.
#[tokio::test]
async fn every_view_survives_the_poison_corpus() {
    let s = poisoned().await;
    let mut c = s.app.login("alice").await;
    let mut bad = vec![];
    for (v, json) in VIEWS.iter().map(|v| (v, false)).chain(APIS.iter().map(|a| (a, true))) {
        let st = probe(&mut c, v, json).await;
        if st != "200" {
            bad.push(format!("{v} → {st}"));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
    let month = c.get_json("/api/events?start=2026-10-01&end=2026-11-01").await.json();
    let titles: Vec<&str> = month.as_array().unwrap().iter().filter_map(|i| i["summary"].as_str()).collect();
    for t in ["Standup", "Quartalsreview", "Gift Überlauf", "Gift 1e15", "Gift Ziffern", "Für immer", "Jede Minute", "Die Ausnahme"] {
        assert!(titles.contains(&t), "{t} missing from the month");
    }
    // the poison alarms are skipped, the good ones stay
    let ev = |id: &str| month.as_array().unwrap().iter().find(|i| i["object_id"] == id).cloned().unwrap();
    assert_eq!(ev("trig-overflow")["alarms"], json!([]));
    assert_eq!(ev("alarm-dur")["alarms"], json!([5]));
    // the editor reads every poison object
    for id in ["trig-overflow", "trig-1e15", "trig-digits", "alarm-dur", "forever", "y9999", "minutely", "minutely-ov", "attach-js", "quoted"] {
        let r = c.get_json(&format!("/api/events/alice/{}/{id}", s.arbeit)).await;
        assert_eq!(r.status, StatusCode::OK, "{id}: {}", r.body);
    }
    for id in ["todo-trig", "todo-trig-rel"] {
        c.get_json(&format!("/api/tasks/alice/{}/{id}", s.aufgaben)).await.ok();
    }
    c.get_json(&format!("/api/contacts/alice/{}/mb", s.freunde)).await.ok();
    // the reminder loop's two reads over the push window
    let alice = s.app.principal("alice").await;
    let cals = crate::domain::events::visible_calendars(&s.app.state, &alice, None).await.unwrap();
    let tz = chrono_tz::Europe::Berlin;
    let (from, to) = (crate::testkit::date("2026-10-13"), crate::testkit::date("2026-10-18"));
    let inst = crate::domain::events::instances(&s.app.state, &cals, from, to, tz).await.unwrap();
    assert!(inst.iter().any(|i| &*i.summary == "Standup"));
    // an unreadable TRIGGER is "at the anchor" for a task, as before (`parse_alarm`); it must not panic
    crate::domain::tasks::reminders(&s.app.state, &cals, tz, from, to).await.unwrap();
}

/// S1: a minutely series with a 20 KB note expands 2 048 occurrences per window. Each used to carry its own
/// copy of the note twice over (caldata's per-occurrence property clone, then `Instance.description`): +60 MiB
/// per week view in a release build. Now they share one, and the request stays small.
#[tokio::test]
async fn a_minutely_series_with_a_big_note_stays_small() {
    let s = seed().await;
    heavy_series(&s).await;
    date_poison(&s).await;
    let mut c = s.app.login("alice").await;
    c.get("/calendar?view=week&date=2026-10-07").await.ok(); // warm-up: caches, templates, i18n
    let alice = s.app.principal("alice").await;
    let cals = crate::domain::events::visible_calendars(&s.app.state, &alice, None).await.unwrap();
    let (inst, peak) = peak_of(crate::domain::events::instances(&s.app.state, &cals, crate::testkit::date("2026-10-12"), crate::testkit::date("2026-10-19"), chrono_tz::Europe::Berlin)).await;
    let inst = inst.unwrap();
    assert!(peak < 24 * MIB, "instances of a week: {} MiB", peak / MIB);
    let minutely: Vec<_> = inst.iter().filter(|i| i.object_id == "minutely").collect();
    assert_eq!(minutely.len(), 2048, "caldata's cap per series and window");
    assert!(minutely.iter().all(|i| std::sync::Arc::ptr_eq(&i.description, &minutely[0].description)), "one shared note");
    assert_eq!(minutely[0].description.chars().count(), crate::domain::events::VIEW_NOTE_MAX + 1, "a view's note is cut");
    let ov: Vec<_> = inst.iter().filter(|i| i.object_id == "minutely-ov").collect();
    assert!(ov.iter().any(|i| &*i.summary == "Die Ausnahme" && i.description.is_empty()) && ov.iter().filter(|i| &*i.summary == "Minute mit Ausnahme").count() > 1000);
    let (r, peak) = peak_of(c.get("/calendar?view=week&date=2026-10-14")).await;
    r.ok();
    assert!(peak < 48 * MIB, "week view: {} MiB", peak / MIB);
    let (r, peak) = peak_of(c.get("/calendar?view=month&date=2026-10-14")).await;
    r.ok();
    assert!(peak < 48 * MIB, "month view with an all-day event until 9999: {} MiB", peak / MIB);
}

/// R1/R2/R3 inputs: rejected or stored harmlessly — never a dropped connection.
#[tokio::test]
async fn hostile_inputs_are_answered() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let ev = |extra: serde_json::Value| {
        let mut e = json!({ "principal": "alice", "cal_id": s.arbeit, "summary": "fz", "start": "2026-10-20T10:00:00", "end": "2026-10-20T11:00:00" });
        e.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        e
    };
    let bad = [
        json!({ "alarms": [{ "action": "DISPLAY", "minutes": 1e15 as i64 }] }),
        json!({ "alarms": [{ "action": "DISPLAY", "minutes": i64::MAX }] }),
        json!({ "alarms": [{ "action": "DISPLAY", "minutes": 10, "repeat": 3, "duration_minutes": i64::MAX }] }),
        json!({ "alarms": [{ "action": "DISPLAY", "at": "+262142-12-31T23:59:59" }] }),
        json!({ "start": "+262142-12-31T23:30:00", "end": "+262142-12-31T23:30:00" }),
        json!({ "start": "+262142-12-31T00:00:00", "end": "+262142-12-31T00:00:00", "all_day": true }),
        json!({ "start": "9999-12-31T00:00:00", "end": "9999-12-31T00:00:00", "all_day": true }),
        json!({ "rrule": "aäää" }),
        json!({ "rrule": "FREQ=Ü" }),
        json!({ "rrule": "FREQ=DAILY;UNTIL=2026103€" }),
        json!({ "rrule": "FREQ=DAILY;UNTIL=2026-10€" }),
        json!({ "summary": "x".repeat(1001) }),
        json!({ "description": "x".repeat(100_001) }),
        json!({ "alarms": [{ "action": "DISPLAY", "minutes": i64::MIN }] }),
    ];
    use futures_util::FutureExt;
    let mut wrong = vec![];
    for b in bad {
        let r = std::panic::AssertUnwindSafe(c.json(Method::POST, "/api/events", ev(b.clone()))).catch_unwind().await;
        let st = r.map_or_else(|_| "panic".to_string(), |r| r.status.as_u16().to_string());
        if st != "400" {
            wrong.push(format!("POST {b} → {st}"));
        }
    }
    // stored, with the unreadable value left out
    let ok = [json!({ "all_day": true, "rrule": "FREQ=DAILY;COUNT=5", "exdates": ["2026-10-2€x"] }), json!({ "all_day": true, "rrule": "FREQ=DAILY;COUNT=5", "rdates": ["ääääää"] }), json!({ "alarms": [{ "minutes": 400 * 365 * 1440 }] })];
    for b in ok {
        let r = std::panic::AssertUnwindSafe(c.json(Method::POST, "/api/events", ev(b.clone()))).catch_unwind().await;
        let st = r.map_or_else(|_| "panic".to_string(), |r| r.status.as_u16().to_string());
        if st != "200" {
            wrong.push(format!("POST {b} → {st}"));
        }
    }
    let r = std::panic::AssertUnwindSafe(c.json(Method::POST, "/api/contacts", json!({ "principal": "alice", "ab_id": s.freunde, "full_name": "B", "birthday": "--1ä1" }))).catch_unwind().await;
    if r.as_ref().map_or(true, |r| r.status != StatusCode::OK) {
        wrong.push(format!("POST contact birthday --1ä1 → {}", r.map_or("panic".into(), |r| r.status.as_u16().to_string())));
    }
    // years outside 1–9999 are refused by the input gate (extract.rs) before any date arithmetic
    for (path, want) in [
        ("/api/density?year=262143", StatusCode::BAD_REQUEST),
        ("/api/density?year=-300000", StatusCode::BAD_REQUEST),
        ("/api/events?start=%2B262142-12-01&end=%2B262142-12-31", StatusCode::BAD_REQUEST),
        ("/api/events?start=-262143-01-01&end=-262143-01-31", StatusCode::BAD_REQUEST),
        ("/api/days?start=%2B262142-12-31&count=120", StatusCode::BAD_REQUEST),
        ("/api/weeks?start=%2B262142-12-20&count=16", StatusCode::BAD_REQUEST),
        ("/calendar?view=month&date=%2B262142-12-31", StatusCode::BAD_REQUEST),
        ("/calendar?view=planner&date=%2B262142-06-15", StatusCode::BAD_REQUEST),
        ("/calendar?view=year&date=9999-12-31", StatusCode::OK),
        ("/api/search?q=17.11.99999", StatusCode::OK),
    ] {
        let st = probe(&mut c, path, true).await;
        if st != want.as_u16().to_string() {
            wrong.push(format!("GET {path} → {st}"));
        }
    }
    // an unbounded query is answered (searched or refused by the input gate as 400), never a 5xx or a panic
    for q in ["a".repeat(20_000), "é".repeat(500), ".*(+[\\".into()] {
        let u = format!("/api/search?q={}", url::form_urlencoded::byte_serialize(q.as_bytes()).collect::<String>());
        let st = probe(&mut c, &u, true).await;
        if st != "200" && st != "400" {
            wrong.push(format!("GET /api/search?q=<{} chars> → {st}", q.chars().count()));
        }
    }
    let u = format!("/api/events/suggest?q={}", url::form_urlencoded::byte_serialize("ä".repeat(5000).as_bytes()).collect::<String>());
    let st = probe(&mut c, &u, true).await;
    if st != "200" && st != "400" {
        wrong.push(format!("GET /api/events/suggest?q=<5000 chars> → {st}"));
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// A parameter value with `:`/`;` survives an editor round trip, and a CR/LF in a
/// non-TEXT field never starts a property of its own.
#[tokio::test]
async fn params_round_trip_and_crlf_stays_inside_its_line() {
    let s = seed().await;
    foreign_params(&s).await;
    let mut c = s.app.login("alice").await;
    let path = format!("/api/events/alice/{}/quoted", s.arbeit);
    let d = c.get_json(&path).await.ok().json();
    c.json(Method::PUT, &path, d).await.ok();
    let stored = s.app.state.calendars.get_object("alice", &s.arbeit, "quoted", false).await.unwrap().get_ics().replace("\r\n ", "");
    assert!(stored.contains("URL;X-Q=\"a:b;c\":https://example.org/q"), "{stored}");
    assert!(stored.contains("CN=\"Doe, John\"") && stored.contains("DELEGATED-FROM=\"mailto:a@example.org\",\"mailto:b@example.org\""), "{stored}");
    let again = c.get_json(&path).await.ok().json();
    assert_eq!(again["links"][0]["uri"], "https://example.org/q");

    let r = c.json(Method::POST, "/api/events", json!({
        "principal": "alice", "cal_id": s.arbeit, "summary": "inj", "start": "2026-10-20T10:00:00", "end": "2026-10-20T11:00:00",
        "attendees": [{ "email": "x@y.de\r\nRRULE:FREQ=SECONDLY", "name": "N\r\nX-INJ:1", "extra": [["X-A\r\nRRULE", "1"], ["X-B", "v\r\nRRULE:FREQ=SECONDLY"]] }],
        "links": [{ "kind": "url", "uri": "https://example.org/", "label": "l\r\nRRULE:FREQ=SECONDLY", "extra": [["X-C", "a\nRRULE:FREQ=SECONDLY"]] }],
        "alarms": [{ "action": "EMAIL", "minutes": 5, "attendees": ["m@x.de\r\nRRULE:FREQ=SECONDLY"] }]
    })).await;
    let id = r.ok().json()["id"].as_str().unwrap().to_string();
    let stored = s.app.state.calendars.get_object("alice", &s.arbeit, &id, false).await.unwrap().get_ics().to_string();
    let vevent = &stored[stored.find("BEGIN:VEVENT").unwrap()..];
    assert!(!vevent.split("\r\n").any(|l| l.starts_with("RRULE") || l.starts_with("X-INJ")), "{stored}");
    assert_eq!(c.get_json(&format!("/api/events/alice/{}/{id}", s.arbeit)).await.ok().json()["rrule"], json!(null));
}

/// An attachment is never served as a script or a page of this origin.
#[tokio::test]
async fn a_stored_script_attachment_is_a_download() {
    let s = seed().await;
    foreign_params(&s).await;
    let mut c = s.app.login("alice").await;
    let r = c.get(&format!("/api/events/alice/{}/attach-js/attach/0", s.arbeit)).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.header("content-type").as_deref(), Some("application/octet-stream"));
    assert_eq!(r.header("x-content-type-options").as_deref(), Some("nosniff"));
    assert!(r.header("content-security-policy").is_some_and(|v| v.contains("sandbox")));
    assert!(r.header("content-disposition").is_some_and(|v| v.starts_with("attachment")));
    assert_eq!(r.bytes, b"alert(1)");
}

/// R7b / R15: imports are bounded and all-or-nothing.
#[tokio::test]
async fn imports_are_bounded_and_atomic() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    // a value caldata's parser panics on: a 400, not a dropped connection
    let dur = ics("BEGIN:VEVENT\nUID:d\nDTSTAMP:20260101T000000Z\nDTSTART:20261020T100000Z\nDURATION:P99999999999W\nSUMMARY:d\nEND:VEVENT");
    let r = c.post_raw(&format!("/settings/import/calendars/alice/{}", s.arbeit), &dur).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.body);
    // a stored poison alarm imports (a DAV client could store it too) and the views live on
    let trig = ics(&format!("BEGIN:VEVENT\nUID:t\nDTSTAMP:20260101T000000Z\nDTSTART:20261020T100000Z\nDTEND:20261020T110000Z\nSUMMARY:t\n{}\nEND:VEVENT", alarm("-PT99999999999999999999M")));
    c.post_raw(&format!("/settings/import/calendars/alice/{}", s.arbeit), &trig).await.ok();
    c.get("/calendar?view=month&date=2026-10-14").await.ok();
    // a broken third card: nothing of the file is stored
    let vcf = "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:imp-ok-1\r\nFN:Eins\r\nEND:VCARD\r\nBEGIN:VCARD\r\nVERSION:3.0\r\nUID:imp-ok-2\r\nFN:Zwei\r\nEND:VCARD\r\nBEGIN:VCARD\r\nGARBAGE\r\n";
    let r = c.post_raw(&format!("/settings/import/addressbooks/alice/{}", s.freunde), vcf).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.body);
    for id in ["imp-ok-1", "imp-ok-2"] {
        assert!(crate::domain::contacts::stored_card(&s.app.state, "alice", &s.freunde, id).await.is_err(), "{id} was stored");
    }
    // the same for a calendar file whose last object is broken
    let cal = ics("BEGIN:VEVENT\nUID:i1\nDTSTAMP:20260101T000000Z\nDTSTART:20261020T100000Z\nSUMMARY:eins\nEND:VEVENT\nBEGIN:VEVENT\nUID:i2\nDTSTAMP:20260101T000000Z\nDTSTART:garbage\nSUMMARY:zwei\nEND:VEVENT");
    let r = c.post_raw(&format!("/settings/import/calendars/alice/{}", s.arbeit), &cal).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.body);
    assert!(s.app.state.calendars.get_object("alice", &s.arbeit, "i1", false).await.is_err());
    // the object limit answers before anything is parsed
    let many: String = (0..=crate::domain::transfer::IMPORT_MAX_OBJECTS).map(|i| format!("BEGIN:VTODO\nUID:m{i}\nSUMMARY:m\nEND:VTODO\n")).collect();
    let r = c.post_raw("/settings/import/calendars?owner=alice&name=Viele", &ics(&many)).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.body);
    assert!(r.body.contains(&crate::domain::transfer::IMPORT_MAX_OBJECTS.to_string()), "{}", r.body);
    // the size limit answers before anything is parsed (the route into an existing calendar has axum's 2 MB)
    let big = "X".repeat(crate::domain::transfer::IMPORT_MAX_BYTES + 1);
    let r = c.post_raw("/settings/import/calendars?owner=alice&name=Gross", &big).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert!(r.body.contains(&crate::domain::transfer::IMPORT_MAX_BYTES.to_string()), "{}", r.body);
}

/// R7b: a large import is read and written in chunks — many objects, one master with overrides far apart in
/// the file, the request's peak a fraction of what holding the whole parsed file took.
#[tokio::test]
async fn a_large_import_is_chunked() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let n = 3000;
    let mut body = String::new();
    for i in 0..n {
        body.push_str(&format!("BEGIN:VEVENT\nUID:bulk-{i}\nDTSTAMP:20260101T000000Z\nDTSTART:2026{:02}{:02}T100000Z\nDTEND:2026{:02}{:02}T110000Z\nSUMMARY:Bulk {i} {}\nEND:VEVENT\n", 1 + i % 12, 1 + i % 28, 1 + i % 12, 1 + i % 28, "d".repeat(300)));
        if i == 10 {
            body.push_str("BEGIN:VEVENT\nUID:series\nDTSTAMP:20260101T000000Z\nDTSTART:20261001T080000Z\nDTEND:20261001T090000Z\nRRULE:FREQ=DAILY;COUNT=10\nSUMMARY:Serie\nEND:VEVENT\n");
        }
    }
    body.push_str("BEGIN:VEVENT\nUID:series\nDTSTAMP:20260101T000000Z\nRECURRENCE-ID:20261003T080000Z\nDTSTART:20261003T120000Z\nDTEND:20261003T130000Z\nSUMMARY:Serie verschoben\nEND:VEVENT\n");
    let file = ics(&body);
    let cal = s.app.calendar("alice", "Import", "#000000", &["VEVENT"]).await;
    let alice = s.app.principal("alice").await;
    let (r, peak) = peak_of(crate::domain::transfer::import_calendar(&s.app.state, &alice, "alice", &cal, file.as_bytes())).await;
    assert_eq!(r.unwrap(), n + 1);
    // the route (history guard, JSON answer) on top: a second import of the same file overwrites
    assert_eq!(c.post_raw(&format!("/settings/import/calendars/alice/{cal}"), &file).await.ok().json()["imported"], n + 1);
    let series = s.app.state.calendars.get_object("alice", &cal, "series", false).await.unwrap();
    assert!(series.get_ics().contains("RECURRENCE-ID"), "the override joined its master");
    assert!(peak < 12 * MIB, "peak {} KiB for a {} KiB file", peak / 1024, file.len() / 1024);
}

/// The history of an import is one change of all its objects; recording it holds a chunk of texts and a small
/// record per object, not every text and diff at once (901d09e: each text twice + all diffs until the end, +198 MiB
/// RSS for 35 000 objects — w3-data report). Both ways in: a new calendar (its objects are read whole, first sight)
/// and an existing one (RustiCal's change log, every object an update).
#[tokio::test]
async fn recording_a_large_import_holds_a_chunk_not_the_import() {
    use crate::domain::history::{self, Actor};
    use crate::domain::history_text::Store;
    use sqlx::Row;
    let s = seed().await;
    let alice = s.app.principal("alice").await;
    let n = 2000;
    let file = |tag: &str| {
        let mut body = String::new();
        for i in 0..n {
            body.push_str(&format!("BEGIN:VEVENT\nUID:bulk-{i}\nDTSTAMP:20260101T000000Z\nDTSTART:2026{:02}{:02}T100000Z\nDTEND:2026{:02}{:02}T110000Z\nSUMMARY:{tag} {i}\nDESCRIPTION:{}\nEND:VEVENT\n", 1 + i % 12, 1 + i % 28, 1 + i % 12, 1 + i % 28, "d".repeat(3000)));
        }
        ics(&body)
    };
    let (one, two) = (file("Bulk"), file("Neu"));
    let n_new = crate::domain::collections::NewCalendar { owner: "alice".into(), name: "Bulk".into(), color: "#000000".into(), description: String::new(), components: vec![], timezone: "Europe/Berlin".into() };
    let mut g = history::begin(&s.app.state, Actor::import(&alice, "Bulk")).await;
    let (cal, imported) = crate::domain::transfer::import_new_calendar(&s.app.state, &alice, n_new, one.as_bytes()).await.unwrap();
    assert_eq!(imported, n);
    g.coll(Store::Cal, "alice", &cal);
    let (first, peak_new) = peak_of(g.commit()).await;
    let mut g = history::begin(&s.app.state, Actor::import(&alice, "")).await;
    g.coll(Store::Cal, "alice", &cal);
    assert_eq!(crate::domain::transfer::import_calendar(&s.app.state, &alice, "alice", &cal, two.as_bytes()).await.unwrap(), n);
    let (second, peak_log) = peak_of(g.commit()).await;
    // what was recorded: one change each, every object in it
    let db = &s.app.state.db;
    let row = |id: Option<i64>| async move {
        let r = sqlx::query("SELECT c.src, COUNT(i.idx) AS n, MIN(i.act) AS act, MIN(i.title) AS title, SUM(i.after_hash IN (SELECT hash FROM practical_history_blobs)) AS blobs FROM practical_history_changes c JOIN practical_history_items i ON i.change_id = c.id WHERE c.id = ? GROUP BY c.id").bind(id).fetch_one(db).await.unwrap();
        (r.get::<String, _>("src"), r.get::<i64, _>("n") as usize, r.get::<String, _>("act"), r.get::<String, _>("title"), r.get::<i64, _>("blobs") as usize)
    };
    assert_eq!(row(first).await, ("import".into(), n, "create".into(), "Bulk 0".into(), n));
    assert_eq!(row(second).await, ("import".into(), n, "update".into(), "Neu 0".into(), n));
    let text = one.len();
    // a record per object stays (keys, hashes, title: < 1 KiB), the texts do not: 901d09e: 21 MiB = 3.5 × the texts, here 1.4 MiB (T, debug)
    assert!(peak_new < text / 3 && peak_log < text / 3, "peak {} / {} KiB for {} KiB of texts", peak_new / 1024, peak_log / 1024, text / 1024);
}
