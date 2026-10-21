use super::*;

const STORED: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VTODO\r\nUID:u1\r\nDTSTAMP:20260901T100000Z\r\nCREATED:20260801T080000Z\r\nSEQUENCE:4\r\nSUMMARY:Alt\r\nDESCRIPTION:Beschreibung\r\nDUE;VALUE=DATE:20260918\r\nCATEGORIES:Arbeit,Familie\r\nLOCATION:Stadtbücherei\r\nURL:https://example.org/t\r\nCLASS:PRIVATE\r\nRELATED-TO;RELTYPE=PARENT:parent-uid\r\nX-APPLE-SORT-ORDER:12\r\nRRULE:FREQ=WEEKLY\r\nATTENDEE;CN=Max:mailto:max@example.org\r\nSTATUS:NEEDS-ACTION\r\nBEGIN:VALARM\r\nACTION:DISPLAY\r\nTRIGGER:-PT2H\r\nDESCRIPTION:Erinnerung\r\nEND:VALARM\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";

fn input() -> TaskInput {
    TaskInput { principal: "p".into(), cal_id: "c".into(), summary: "Neu".into(), due_all_day: true, ..Default::default() }
}
fn tz() -> chrono_tz::Tz {
    chrono_tz::Europe::Berlin
}

/// The regression this guards: a save used to rebuild the VTODO from six fields,
/// so every property another client had written was gone after one click.
#[test]
fn save_keeps_every_unmanaged_property() {
    let (raw, extra) = events::split_component(STORED, "VTODO");
    assert!(extra.is_empty());
    let mut i = input();
    // alarms are modelled, so they travel through the input — as they do when the editor saves
    i.alarms = raw.alarms.iter().enumerate().map(|(n, a)| events::parse_alarm(n, a, tz())).collect();
    let out = build_vtodo("u1", 5, &i, tz(), Some(&raw)).unwrap().join("\n");
    for line in ["CREATED:20260801T080000Z", "X-APPLE-SORT-ORDER:12", "RRULE:FREQ=WEEKLY", "ATTENDEE;CN=Max:mailto:max@example.org"] {
        assert!(out.contains(line), "lost: {line}\n{out}");
    }
    assert!(out.contains("TRIGGER:-PT2H"), "alarm lost\n{out}");
    // managed properties come from the input, not from the stored copy
    assert!(out.contains("SUMMARY:Neu") && !out.contains("SUMMARY:Alt"));
    assert!(!out.contains("CATEGORIES") && !out.contains("LOCATION") && !out.contains("CLASS:PRIVATE"));
    assert_eq!(out.matches("SEQUENCE:5").count(), 1);
    assert_eq!(out.matches("LAST-MODIFIED:").count(), 1);
}

#[test]
fn without_raw_nothing_extra_is_written() {
    let out = build_vtodo("u1", 0, &input(), tz(), None).unwrap().join("\n");
    assert!(!out.contains("CATEGORIES") && !out.contains("VALARM"));
    assert!(out.contains("SUMMARY:Neu"));
}

#[test]
fn full_input_round_trips_through_the_reader() {
    let mut i = input();
    i.summary = "Steuer".into();
    i.description = "Belege".into();
    i.location = "Büro".into();
    i.start = Some(NaiveDate::from_ymd_opt(2026, 9, 21).unwrap().and_hms_opt(9, 0, 0).unwrap());
    i.start_all_day = false;
    i.due = Some(NaiveDate::from_ymd_opt(2026, 9, 24).unwrap().and_hms_opt(17, 30, 0).unwrap());
    i.due_all_day = false;
    i.status = "IN-PROCESS".into();
    i.percent = Some(60);
    i.priority = Some(2);
    i.class = "PRIVATE".into();
    i.categories = vec!["Arbeit".into(), "Steuer".into()];
    i.color = "tomato".into();
    i.geo = Some((49.45, 11.08));
    normalize(&mut i, NaiveDate::from_ymd_opt(2026, 9, 22).unwrap().and_hms_opt(12, 0, 0).unwrap());
    let p = build_vtodo("u1", 1, &i, tz(), None).unwrap();
    assert_eq!(stamp_of(&p, "DUE", tz()), (i.due, false));
    assert_eq!(stamp_of(&p, "DTSTART", tz()), (i.start, false));
    assert_eq!(status_of(&p), "IN-PROCESS");
    assert_eq!(num_of(&p, "PERCENT-COMPLETE"), Some(60));
    assert_eq!(num_of(&p, "PRIORITY"), Some(2));
    assert_eq!(class_of(&p), "PRIVATE");
    assert_eq!(cats_of(&p), vec!["Arbeit".to_string(), "Steuer".into()]);
    assert_eq!(text_of(&p, "COLOR"), "tomato");
    assert_eq!(geo_of(&p), Some((49.45, 11.08)));
    assert_eq!(text_of(&p, "LOCATION"), "Büro");
}

/// The consistency rule the format does not give us.
#[test]
fn progress_fields_stay_consistent() {
    let now = NaiveDate::from_ymd_opt(2026, 9, 22).unwrap().and_hms_opt(12, 0, 0).unwrap();
    let mut done = input();
    done.status = "COMPLETED".into();
    normalize(&mut done, now);
    assert_eq!((done.percent, done.completed_at), (Some(100), Some(now)));

    let mut reopened = input();
    reopened.status = "NEEDS-ACTION".into();
    reopened.percent = Some(100);
    reopened.completed_at = Some(now);
    normalize(&mut reopened, now);
    assert_eq!((reopened.percent, reopened.completed_at), (None, None));

    // progress on an "open" task means it is being worked on
    let mut started = input();
    started.status = "NEEDS-ACTION".into();
    started.percent = Some(40);
    normalize(&mut started, now);
    assert_eq!((started.status.as_str(), started.percent), ("IN-PROCESS", Some(40)));

    // a bogus status never reaches the file
    let mut junk = input();
    junk.status = "DONE-ISH".into();
    normalize(&mut junk, now);
    assert_eq!(junk.status, "");
}

#[test]
fn completed_stamp_is_read_as_utc() {
    let p = vec!["COMPLETED:20260920T143200Z".to_string()];
    let local = completed_of(&p, tz()).unwrap();
    assert_eq!(local.format("%Y-%m-%d %H:%M").to_string(), "2026-09-20 16:32"); // CEST = UTC+2
}

fn cal() -> CalInfo {
    CalInfo {
        principal: "p".into(),
        owner_label: String::new(),
        is_group: false,
        id: "c".into(),
        displayname: "Persönlicher Kalender".into(),
        color: "#3478f6".into(),
        description: String::new(),
        components: vec!["VTODO".into()],
        timezone_id: String::new(),
        subscription_url: None,
        deleted_at: None,
        synctoken: 0,
        read_only: false,
        managed: false,
    }
}
fn with(extra: &str) -> String {
    format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VTODO\r\nUID:u1\r\nDTSTAMP:20260901T100000Z\r\nSUMMARY:Zahnarzt anrufen\r\nDUE;TZID=Europe/Berlin:20260924T170000\r\n{extra}BEGIN:VALARM\r\nACTION:DISPLAY\r\nTRIGGER;RELATED=END:-PT2H\r\nDESCRIPTION:x\r\nEND:VALARM\r\nEND:VTODO\r\nEND:VCALENDAR\r\n")
}

/// Reminders on tasks were stored and shown but never fired; this is the path that fires them.
#[test]
fn only_an_open_task_in_the_window_reminds() {
    let win = (NaiveDate::from_ymd_opt(2026, 9, 21).unwrap(), NaiveDate::from_ymd_opt(2026, 9, 26).unwrap());
    let r = reminders_of(&with(""), &cal(), "o1", tz(), win.0, win.1);
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].at.format("%Y-%m-%d %H:%M").to_string(), "2026-09-24 15:00");
    assert_eq!((r[0].summary.as_str(), r[0].object_id.as_str(), r[0].cal_name.as_str()), ("Zahnarzt anrufen", "o1", "Persönlicher Kalender"));

    // finished or cancelled: nothing to remind about
    assert!(reminders_of(&with("STATUS:COMPLETED\r\n"), &cal(), "o1", tz(), win.0, win.1).is_empty());
    assert!(reminders_of(&with("COMPLETED:20260923T080000Z\r\n"), &cal(), "o1", tz(), win.0, win.1).is_empty());
    assert!(reminders_of(&with("STATUS:CANCELLED\r\n"), &cal(), "o1", tz(), win.0, win.1).is_empty());
    // outside the window
    assert!(reminders_of(&with(""), &cal(), "o1", tz(), NaiveDate::from_ymd_opt(2026, 9, 25).unwrap(), win.1).is_empty());
    // a task without any alarm contributes nothing at all
    let bare = "BEGIN:VCALENDAR\r\nBEGIN:VTODO\r\nUID:u2\r\nSUMMARY:x\r\nDUE;VALUE=DATE:20260924\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";
    assert!(reminders_of(bare, &cal(), "o2", tz(), win.0, win.1).is_empty());
}

/// The anchor rule for a component that has no duration.
#[test]
fn a_reminder_anchors_on_what_the_task_has() {
    let d = |h: u32| NaiveDate::from_ymd_opt(2026, 9, 24).unwrap().and_hms_opt(h, 0, 0).unwrap();
    let al = |minutes: i64, related_end: bool| Alarm { minutes, related_end, ..Default::default() };

    // both dates: each side counts from its own anchor
    assert_eq!(reminder_at(&al(120, false), Some(d(9)), Some(d(17))), Some(d(7)));
    assert_eq!(reminder_at(&al(120, true), Some(d(9)), Some(d(17))), Some(d(15)));
    // only a due date: both sides fall back to it
    assert_eq!(reminder_at(&al(60, false), None, Some(d(17))), Some(d(16)));
    assert_eq!(reminder_at(&al(60, true), None, Some(d(17))), Some(d(16)));
    // only a start: likewise
    assert_eq!(reminder_at(&al(60, true), Some(d(9)), None), Some(d(8)));
    // a negative offset is "after"
    assert_eq!(reminder_at(&al(-60, true), None, Some(d(17))), Some(d(18)));
    // no date at all: nothing to count from
    assert_eq!(reminder_at(&al(60, false), None, None), None);
    // an absolute trigger wins over every anchor
    let abs = Alarm { at: Some(d(6)), minutes: 999, ..Default::default() };
    assert_eq!(reminder_at(&abs, Some(d(9)), Some(d(17))), Some(d(6)));
}

/// A DUE with a foreign TZID must arrive as the right wall-clock in the user's zone.
#[test]
fn due_with_a_foreign_tzid_is_converted() {
    let p = vec!["DUE;TZID=America/New_York:20260924T090000".to_string()];
    let (dt, all_day) = stamp_of(&p, "DUE", tz());
    assert!(!all_day);
    assert_eq!(dt.unwrap().format("%Y-%m-%d %H:%M").to_string(), "2026-09-24 15:00");
}
