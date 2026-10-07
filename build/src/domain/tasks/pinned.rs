//! Characterisation of the task model: reading VTODO lines, the progress invariant, the lines a save
//! writes, the reminder anchors, and the per-calendar cache through the real store.

use super::*;
use crate::testkit::{TestApp, date, vtodo};

fn tz() -> chrono_tz::Tz {
    "Europe/Berlin".parse().unwrap()
}

fn dt(s: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M").unwrap()
}

#[test]
fn reading_lines() {
    let p: Vec<String> = [
        "SUMMARY:Steuer\\, Belege",
        "STATUS:in-process",
        "CLASS:secret",
        "PRIORITY:12",
        "PERCENT-COMPLETE:x",
        "GEO:51.1;13.2",
        "CATEGORIES:A\\,B,C",
        "CATEGORIES:D",
        "DTSTART;TZID=America/New_York:20261014T090000",
        "DUE:20261015T120000Z",
        "COMPLETED:20261001T100000",
        "CREATED;VALUE=DATE:20260101",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert_eq!(text_of(&p, "SUMMARY"), "Steuer, Belege");
    assert_eq!(status_of(&p), "IN-PROCESS");
    assert_eq!(class_of(&p), "");
    assert_eq!(num_of(&p, "PRIORITY"), Some(12));
    assert_eq!(num_of(&p, "PERCENT-COMPLETE"), None);
    assert_eq!(geo_of(&p), Some((51.1, 13.2)));
    assert_eq!(cats_of(&p), ["A,B", "C", "D"]);
    assert_eq!(stamp_of(&p, "DTSTART", tz()), (Some(dt("2026-10-14T15:00")), false));
    assert_eq!(stamp_of(&p, "DUE", tz()), (Some(dt("2026-10-15T14:00")), false));
    assert_eq!(stamp_of(&p, "CREATED", tz()), (Some(dt("2026-01-01T00:00")), true));
    assert_eq!(stamp_of(&p, "MISSING", tz()), (None, false));
    assert_eq!(completed_of(&p, tz()), Some(dt("2026-10-01T12:00")), "COMPLETED is UTC even without Z");
    let same_zone = vec!["DUE;TZID=Europe/Berlin:20261015T120000".to_string()];
    assert_eq!(stamp_of(&same_zone, "DUE", tz()), (Some(dt("2026-10-15T12:00")), false));
    let floating = vec!["DUE:20261015T120000".to_string()];
    assert_eq!(stamp_of(&floating, "DUE", tz()), (Some(dt("2026-10-15T12:00")), false));
}

#[test]
fn progress_invariant() {
    let now = dt("2026-10-14T10:00");
    let case = |status: &str, percent: Option<i64>, completed: Option<&str>, due: bool| {
        let mut i = TaskInput { status: status.into(), percent, completed_at: completed.map(dt), due: due.then(|| dt("2026-10-20T00:00")), due_all_day: false, ..Default::default() };
        normalize(&mut i, now);
        (i.status, i.percent, i.completed_at.map(|c| c.format("%Y-%m-%dT%H:%M").to_string()), i.due_all_day)
    };
    assert_eq!(case("COMPLETED", None, None, true), ("COMPLETED".into(), Some(100), Some("2026-10-14T10:00".into()), false));
    assert_eq!(case("COMPLETED", Some(20), Some("2026-10-01T08:00"), true), ("COMPLETED".into(), Some(100), Some("2026-10-01T08:00".into()), false));
    assert_eq!(case("NEEDS-ACTION", Some(100), Some("2026-10-01T08:00"), true), ("NEEDS-ACTION".into(), None, None, false));
    assert_eq!(case("NEEDS-ACTION", Some(30), None, true), ("IN-PROCESS".into(), Some(30), None, false));
    assert_eq!(case("", Some(130), None, true), ("IN-PROCESS".into(), Some(100), None, false), "clamped after the 100 check");
    assert_eq!(case("CANCELLED", Some(30), None, true), ("CANCELLED".into(), Some(30), None, false));
    assert_eq!(case("bogus", Some(0), None, false), ("".into(), None, None, true), "no due date: all-day");
}

#[test]
fn json_input_dates() {
    let i: TaskInput = serde_json::from_str(r#"{"principal":"p","cal_id":"c","summary":"s","due":"2026-09-25","start":"2026-09-24T17:30","completed_at":""}"#).unwrap();
    assert_eq!((i.due, i.start, i.completed_at, i.due_all_day), (Some(dt("2026-09-25T00:00")), Some(dt("2026-09-24T17:30")), None, true));
    let i: TaskInput = serde_json::from_str(r#"{"principal":"p","cal_id":"c","summary":"s","due":"2026-09-25T08:00:00","due_all_day":false}"#).unwrap();
    assert_eq!((i.due, i.due_all_day), (Some(dt("2026-09-25T08:00")), false));
    assert!(serde_json::from_str::<TaskInput>(r#"{"principal":"p","cal_id":"c","summary":"s","due":"25.09.2026"}"#).is_err());
}

#[test]
fn vtodo_lines() {
    let i: TaskInput = serde_json::from_value(serde_json::json!({
        "principal": "p", "cal_id": "c", "summary": "Steuer", "description": "a\nb", "location": "Amt",
        "start": "2026-10-01T09:00", "start_all_day": false, "due": "2026-10-15", "status": "in-process", "percent": 40,
        "class": "private", "priority": 0, "categories": ["X"], "color": "#ff0000", "geo": [1.5, 2.5],
        "links": [{ "kind": "url", "uri": "https://example.org" }], "related": [{ "uid": "p1", "reltype": "parent" }],
        "alarms": [{ "minutes": 60, "related_end": true }]
    }))
    .unwrap();
    let l: Vec<String> = build_vtodo("t1", 2, &i, tz(), None).unwrap().into_iter().filter(|l| !l.starts_with("DTSTAMP") && !l.starts_with("LAST-MODIFIED")).collect();
    assert_eq!(l, [
        "UID:t1", "SEQUENCE:2", "SUMMARY:Steuer", "DTSTART;TZID=Europe/Berlin:20261001T090000", "DUE;VALUE=DATE:20261015", "DESCRIPTION:a\\nb", "LOCATION:Amt",
        "CATEGORIES:X", "STATUS:IN-PROCESS", "PERCENT-COMPLETE:40", "CLASS:PRIVATE", "COLOR:#ff0000", "GEO:1.500000;2.500000", "URL:https://example.org",
        "RELATED-TO;RELTYPE=PARENT:p1", "BEGIN:VALARM", "ACTION:DISPLAY", "TRIGGER;RELATED=END:-PT1H", "DESCRIPTION:Steuer", "END:VALARM",
    ]);
    let mut bad = i.clone();
    bad.due = Some(dt("2026-09-01T00:00"));
    assert!(matches!(build_vtodo("t", 0, &bad, tz(), None), Err(AppError::BadRequest(m)) if m == "due before start"));
    bad.summary = " ".into();
    assert!(matches!(build_vtodo("t", 0, &bad, tz(), None), Err(AppError::BadRequest(m)) if m == "summary required"));
    let ics = ical::assemble(None, "VTODO", vec!["UID:x".into()], &["BEGIN:VTODO\r\nRECURRENCE-ID:1\r\nEND:VTODO".into()]);
    assert_eq!(ics, "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//xarif.de//PractiCal//DE\r\nCALSCALE:GREGORIAN\r\nBEGIN:VTODO\r\nUID:x\r\nEND:VTODO\r\nBEGIN:VTODO\r\nRECURRENCE-ID:1\r\nEND:VTODO\r\nEND:VCALENDAR\r\n");
    assert!(is_managed("RRULE:FREQ=DAILY") == false && is_managed("PERCENT-COMPLETE:1") && !is_managed("ATTACH;VALUE=BINARY;ENCODING=BASE64:aGk="));
    assert_eq!(stamp_line("DUE", dt("2026-10-15T08:00"), true, tz()), "DUE;VALUE=DATE:20261015");
}

#[test]
fn reminder_anchors() {
    let a = |minutes: i64, related_end: bool| Alarm { minutes, related_end, ..Default::default() };
    let (s, d) = (Some(dt("2026-10-14T09:00")), Some(dt("2026-10-15T00:00")));
    assert_eq!(reminder_at(&a(60, false), s, d), Some(dt("2026-10-14T08:00")));
    assert_eq!(reminder_at(&a(60, true), s, d), Some(dt("2026-10-14T23:00")));
    assert_eq!(reminder_at(&a(60, true), s, None), Some(dt("2026-10-14T08:00")), "END falls back to the start");
    assert_eq!(reminder_at(&a(-30, false), None, d), Some(dt("2026-10-15T00:30")));
    assert_eq!(reminder_at(&a(10, false), None, None), None);
    assert_eq!(reminder_at(&Alarm { at: Some(dt("2026-01-01T00:00")), ..Default::default() }, None, None), Some(dt("2026-01-01T00:00")));
    let t = Task { principal: "p".into(), cal_id: "c".into(), cal_name: String::new(), color: String::new(), object_id: "o".into(), uid: "u".into(), summary: String::new(), description: String::new(), location: String::new(), start: None, start_all_day: false, due: Some(dt("2026-10-13T08:00")), due_all_day: false, status: String::new(), percent: None, completed_at: None, priority: 0, class: String::new(), categories: vec![], alarms: 0, links: 0, created: None, extra: TaskExtra::default() };
    assert_eq!((t.completed(), t.open(), t.key()), (false, true, "p/c/o".to_string()));
    let done = Task { completed_at: Some(dt("2026-10-13T09:00")), due_all_day: true, ..t.clone() };
    assert_eq!((done.completed(), done.open()), (true, false));
    let explicit = Task { status: "NEEDS-ACTION".into(), ..done.clone() };
    assert!(!explicit.completed(), "a stamp counts only without a STATUS");
    let cancelled = Task { status: "CANCELLED".into(), ..t };
    assert!(!cancelled.open() && !cancelled.completed());
}

#[tokio::test]
async fn cache_follows_the_store() {
    let app = TestApp::new().await;
    app.user("alice", "Alice").await;
    let cal = app.calendar("alice", "T", "#ff9f0a", &["VEVENT", "VTODO"]).await;
    let user = app.principal("alice").await;
    let cals = || async { crate::domain::collections::calendars_for(&app.state, &user, false).await.unwrap() };
    app.put_ics("alice", &cal, "a", &vtodo("a", "A", "DUE;VALUE=DATE:20261020\nCOLOR:bad value!")).await;
    app.put_ics("alice", &cal, "b", &vtodo("b", "B", "STATUS:COMPLETED\nCOMPLETED:20261001T100000Z")).await;
    app.put_ics("alice", &cal, "ev", &crate::testkit::vevent("ev", "Kein Task", "DTSTART:20261020T100000Z")).await;
    let list = tasks_for(&app.state, &cals().await, tz()).await.unwrap();
    assert_eq!(list.iter().map(|t| (t.summary.as_str(), t.color.as_str(), t.cal_name.as_str())).collect::<Vec<_>>(), [("A", "#ff9f0a", "T"), ("B", "#ff9f0a", "T")], "open first; an invalid COLOR falls back");
    // an edit moves the token: only the change is read again
    app.put_ics("alice", &cal, "a", &vtodo("a", "A2", "DUE;VALUE=DATE:20261019")).await;
    crate::domain::collections::delete_object(&app.state, &user, crate::domain::history_text::Store::Cal, "alice", &cal, "b").await.unwrap();
    let list = tasks_for(&app.state, &cals().await, tz()).await.unwrap();
    assert_eq!(list.iter().map(|t| t.summary.as_str()).collect::<Vec<_>>(), ["A2"]);
    // another zone is a cache entry of its own
    let utc = tasks_for(&app.state, &cals().await, chrono_tz::UTC).await.unwrap();
    assert_eq!(utc.len(), 1);
    // reminders of open tasks only, inside the window
    app.put_ics("alice", &cal, "r", &vtodo("r", "Erinnern", "DUE;TZID=Europe/Berlin:20261020T120000\nBEGIN:VALARM\nACTION:DISPLAY\nTRIGGER:-PT1H\nEND:VALARM")).await;
    app.put_ics("alice", &cal, "rd", &vtodo("rd", "Erledigt", "STATUS:COMPLETED\nDUE;TZID=Europe/Berlin:20261020T120000\nBEGIN:VALARM\nACTION:DISPLAY\nTRIGGER:-PT1H\nEND:VALARM")).await;
    let r = reminders(&app.state, &cals().await, tz(), date("2026-10-20"), date("2026-10-20")).await.unwrap();
    assert_eq!(r.iter().map(|x| (x.summary.as_str(), x.at)).collect::<Vec<_>>(), [("Erinnern", dt("2026-10-20T11:00"))]);
    assert!(reminders(&app.state, &cals().await, tz(), date("2026-10-21"), date("2026-10-30")).await.unwrap().is_empty());
}

/// A VTODO without STATUS is listed as open (the board's NEEDS-ACTION column, the table's status cell, the
/// search's status words), or as done with a COMPLETED stamp; the editor and a save keep it without one.
#[tokio::test]
async fn a_task_without_status_is_listed_open_and_saved_without_one() {
    let app = TestApp::new().await;
    app.user("alice", "Alice").await;
    let cal = app.calendar("alice", "T", "#ff9f0a", &["VTODO"]).await;
    let user = app.principal("alice").await;
    app.put_ics("alice", &cal, "n", &vtodo("n", "Ohne", "DUE;VALUE=DATE:20261020")).await;
    app.put_ics("alice", &cal, "s", &vtodo("s", "Gestempelt", "COMPLETED:20261001T100000Z")).await;
    let cals = crate::domain::collections::calendars_for(&app.state, &user, false).await.unwrap();
    let list = tasks_for(&app.state, &cals, tz()).await.unwrap();
    assert_eq!(list.iter().map(|t| (t.summary.as_str(), t.status.as_str(), t.open())).collect::<Vec<_>>(), [("Ohne", "NEEDS-ACTION", true), ("Gestempelt", "COMPLETED", false)]);
    let d = get_task(&app.state, &user, "alice", &cal, "n", tz()).await.unwrap();
    assert_eq!(d.status, "", "the editor sees what is stored");
    let i = TaskInput { principal: "alice".into(), cal_id: cal.clone(), summary: "Ohne, umbenannt".into(), due: d.due, due_all_day: true, status: d.status, ..Default::default() };
    update_task(&app.state, &user, "n", i, Some(&d.etag), tz()).await.unwrap();
    let obj = app.state.calendars.get_object("alice", &cal, "n", false).await.unwrap();
    let ics = obj.get_ics();
    assert!(ics.contains("SUMMARY:Ohne\\, umbenannt") && !ics.contains("STATUS"), "a save adds no STATUS the user did not pick:\n{ics}");
}
