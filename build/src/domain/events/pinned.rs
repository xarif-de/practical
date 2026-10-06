//! Characterisation of the event model: expansion into instances (zones, all-day, overrides, alarms)
//! through the real store, and the exact lines the editor's save writes.

use super::*;
use crate::testkit::{TestApp, date, fixture, vevent};
use chrono::NaiveDate;

fn berlin() -> chrono_tz::Tz {
    "Europe/Berlin".parse().unwrap()
}

fn dt(s: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M").unwrap()
}

fn rows(v: &[Instance]) -> Vec<String> {
    v.iter().map(|i| format!("{} {}→{} {}{}{}", i.summary, i.start.format("%Y-%m-%dT%H:%M"), i.end.format("%Y-%m-%dT%H:%M"), if i.all_day { "allday " } else { "" }, if i.recurring { "series " } else { "" }, i.alarms.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(","))).collect()
}

async fn with_calendar() -> (TestApp, Vec<crate::domain::collections::CalInfo>) {
    let app = TestApp::new().await;
    app.user("alice", "Alice").await;
    let id = app.calendar("alice", "Kal", "#0a84ff", &["VEVENT"]).await;
    let user = app.principal("alice").await;
    let _ = id;
    let cals = crate::domain::collections::calendars_for(&app.state, &user, false).await.unwrap();
    (app, cals)
}

#[tokio::test]
async fn instances_through_zones_series_and_overrides() {
    let (app, cals) = with_calendar().await;
    let cal = &cals[0].id;
    // a series across the DST change with a moved and a cancelled occurrence
    let series = "BEGIN:VEVENT\nUID:serie\nDTSTAMP:20260901T000000Z\nSUMMARY:Serie\nDTSTART;TZID=Europe/Berlin:20261020T090000\nDTEND;TZID=Europe/Berlin:20261020T100000\nRRULE:FREQ=WEEKLY;COUNT=3\nBEGIN:VALARM\nACTION:DISPLAY\nTRIGGER;RELATED=END:-PT5M\nEND:VALARM\nEND:VEVENT\nBEGIN:VEVENT\nUID:serie\nDTSTAMP:20260901T000000Z\nRECURRENCE-ID;TZID=Europe/Berlin:20261027T090000\nSUMMARY:Serie verschoben\nDTSTART;TZID=Europe/Berlin:20261027T150000\nDTEND;TZID=Europe/Berlin:20261027T160000\nEND:VEVENT";
    app.put_ics("alice", cal, "serie", &crate::testkit::ics(series)).await;
    // New York wall clock, UTC, floating, all-day multi-day, a zero-length event, no DTEND
    app.put_ics("alice", cal, "ny", &vevent("ny", "New York", "DTSTART;TZID=America/New_York:20261021T090000\nDTEND;TZID=America/New_York:20261021T100000")).await;
    app.put_ics("alice", cal, "utc", &vevent("utc", "UTC spät", "DTSTART:20261021T230000Z\nDTEND:20261022T003000Z")).await;
    app.put_ics("alice", cal, "float", &vevent("float", "Floating", "DTSTART:20261022T070000\nDTEND:20261022T080000")).await;
    app.put_ics("alice", cal, "tage", &vevent("tage", "Drei Tage", "DTSTART;VALUE=DATE:20261023\nDTEND;VALUE=DATE:20261026")).await;
    app.put_ics("alice", cal, "punkt", &vevent("punkt", "Punkt", "DTSTART:20261024T100000Z")).await;
    app.put_ics("alice", cal, "dauer", &vevent("dauer", "Mit Dauer", "DTSTART;TZID=Europe/Berlin:20261024T180000\nDURATION:PT90M\nCOLOR:tomato\nSTATUS:CANCELLED\nCATEGORIES:A,B\nCATEGORIES:C")).await;
    let all = instances(&app.state, &cals, date("2026-10-19"), date("2026-11-08"), berlin()).await.unwrap();
    assert_eq!(rows(&all), [
        "Serie 2026-10-20T09:00→2026-10-20T10:00 series -55",
        "New York 2026-10-21T15:00→2026-10-21T16:00 ",
        "UTC spät 2026-10-22T01:00→2026-10-22T02:30 ",
        "Floating 2026-10-22T07:00→2026-10-22T08:00 ",
        "Drei Tage 2026-10-23T00:00→2026-10-26T00:00 allday ",
        "Punkt 2026-10-24T12:00→2026-10-24T12:00 ",
        "Mit Dauer 2026-10-24T18:00→2026-10-24T19:30 ",
        "Serie verschoben 2026-10-27T15:00→2026-10-27T16:00 series -55",
        "Serie 2026-11-03T09:00→2026-11-03T10:00 series -55",
    ]);
    let d = all.iter().find(|i| i.summary == "Mit Dauer").unwrap();
    assert_eq!((d.color.as_str(), d.cancelled, d.categories.clone()), ("tomato", true, vec!["A".to_string(), "B".into(), "C".into()]));
    assert_eq!(all[0].alarm_minutes, Some(-55), "RELATED=END 5 minutes before a 60-minute event");
    // in another zone the same objects move
    let ny: chrono_tz::Tz = "America/New_York".parse().unwrap();
    let there = instances(&app.state, &cals, date("2026-10-21"), date("2026-10-22"), ny).await.unwrap();
    assert_eq!(rows(&there), ["New York 2026-10-21T09:00→2026-10-21T10:00 ", "UTC spät 2026-10-21T19:00→2026-10-21T20:30 "]);
    // the window: an instance touching it counts, the edges are exclusive
    let one = instances(&app.state, &cals, date("2026-10-25"), date("2026-10-26"), berlin()).await.unwrap();
    assert_eq!(rows(&one), ["Drei Tage 2026-10-23T00:00→2026-10-26T00:00 allday "]);
    assert!(instances(&app.state, &cals, date("2026-10-26"), date("2026-10-27"), berlin()).await.unwrap().is_empty());
    // the year's density and a calendar without VEVENT
    assert_eq!(month_density(&app.state, &cals, 2026, berlin()).await.unwrap(), [0, 0, 0, 0, 0, 0, 0, 0, 0, 8, 1, 0]);
    let mut todo_only = cals.clone();
    todo_only[0].components = vec!["VTODO".into()];
    assert!(instances(&app.state, &todo_only, date("2026-10-01"), date("2026-11-01"), berlin()).await.unwrap().is_empty());
}

#[tokio::test]
async fn detail_of_the_full_demo_event_and_save_round_trip() {
    let (app, cals) = with_calendar().await;
    let cal = cals[0].id.clone();
    app.put_ics("alice", &cal, "demo", &fixture("demo-event-full.ics")).await;
    let user = app.principal("alice").await;
    let d = get_event(&app.state, &user, "alice", &cal, "demo", berlin()).await.unwrap();
    let mut v = serde_json::to_value(&d).unwrap();
    v["etag"] = serde_json::json!("<etag>");
    v["cal_id"] = serde_json::json!("<cal>");
    crate::testkit::assert_snapshot("events/demo_detail.json", &serde_json::to_string_pretty(&v).unwrap());
    // the detail is what the editor sends back: saving it unchanged keeps the instance set
    let mut input: EventInput = serde_json::from_value(serde_json::to_value(&d).unwrap()).unwrap();
    input.timezone = Some("Europe/Berlin".into());
    update_event(&app.state, &user, "demo", input, Some(&d.etag), berlin()).await.unwrap();
    let again = get_event(&app.state, &user, "alice", &cal, "demo", berlin()).await.unwrap();
    let strip = |x: &EventDetail| { let mut v = serde_json::to_value(x).unwrap(); v["etag"] = serde_json::json!(""); v };
    assert_eq!(strip(&again), strip(&d), "an unchanged save reads back the same");
    // a stale etag
    let input2: EventInput = serde_json::from_value(serde_json::to_value(&again).unwrap()).unwrap();
    assert!(matches!(update_event(&app.state, &user, "demo", input2, Some("stale"), berlin()).await, Err(AppError::Conflict(_))));
}

#[test]
fn build_vevent_lines() {
    let tz = berlin();
    let input: EventInput = serde_json::from_value(serde_json::json!({
        "principal": "p", "cal_id": "c", "summary": " Titel; mit, Zeichen ", "location": "Ort\nzwei", "description": "",
        "start": "2026-10-14T09:00:00", "end": "2026-10-14T09:00:00",
        "categories": ["A,B", " ", "C"], "status": "tentative", "transp": "nope", "class": "Public", "priority": 10, "color": "bad color!",
        "geo": [91.0, 0.0], "rrule": "freq=daily;until=20261031", "exdates": ["2026-10-15T09:00:00", "garbage"], "rdates": ["2026-11-02"],
        "organizer": { "name": "Bob; Chef", "email": "bob@example.org", "role": "chair" }, "attendees": [{ "name": "", "email": "" }, { "email": "a@example.org", "status": "accepted", "extra": [["RSVP", "TRUE"]] }],
        "alarms": [{ "minutes": 0 }, { "action": "email", "minutes": -90, "summary": "S", "attendees": ["x@example.org", "kein"] }, { "action": "audio", "related_end": true, "minutes": 1440, "attach": "https://example.org/a.wav", "repeat": 2, "duration_minutes": 5, "proximity": "arrive" }, { "at": "2026-10-13T20:00:00", "acknowledged": "2026-10-13T20:05:00" }]
    })).unwrap();
    let lines: Vec<String> = build_vevent("uid-1", 3, &input, tz, None).unwrap().into_iter().filter(|l| !l.starts_with("DTSTAMP:") && !l.starts_with("LAST-MODIFIED:")).collect();
    assert_eq!(lines, [
        "UID:uid-1",
        "SEQUENCE:3",
        "SUMMARY:Titel\\; mit\\, Zeichen",
        "DTSTART;TZID=Europe/Berlin:20261014T090000",
        "DTEND;TZID=Europe/Berlin:20261014T100000",
        "LOCATION:Ort\\nzwei",
        "CATEGORIES:A\\,B,C",
        // a lower-case rule (RFC 5545: case-insensitive) gets its DATE UNTIL turned into the UTC end of
        // that local day like an upper-case one — a timed event wants a UTC date-time there
        "RRULE:FREQ=DAILY;UNTIL=20261031T225959Z",
        "EXDATE;TZID=Europe/Berlin:20261015T090000",
        // a value of date length is a DATE even on a timed event
        "RDATE;VALUE=DATE:20261102",
        "ORGANIZER;CN=\"Bob; Chef\";ROLE=CHAIR:mailto:bob@example.org",
        "ATTENDEE;PARTSTAT=ACCEPTED;RSVP=TRUE:mailto:a@example.org",
        "STATUS:TENTATIVE",
        "CLASS:PUBLIC",
        "BEGIN:VALARM",
        "ACTION:DISPLAY",
        "TRIGGER:PT0S",
        r"DESCRIPTION:Titel\; mit\, Zeichen",
        "END:VALARM",
        "BEGIN:VALARM",
        "ACTION:EMAIL",
        "TRIGGER:PT90M",
        r"DESCRIPTION:Titel\; mit\, Zeichen",
        "SUMMARY:S",
        "ATTENDEE:mailto:x@example.org",
        "END:VALARM",
        "BEGIN:VALARM",
        "ACTION:AUDIO",
        "TRIGGER;RELATED=END:-P1D",
        "ATTACH:https://example.org/a.wav",
        "DURATION:PT5M",
        "REPEAT:2",
        "PROXIMITY:ARRIVE",
        "END:VALARM",
        "BEGIN:VALARM",
        "ACTION:DISPLAY",
        "TRIGGER;VALUE=DATE-TIME:20261013T180000Z",
        r"DESCRIPTION:Titel\; mit\, Zeichen",
        "ACKNOWLEDGED:20261013T180500Z",
        "END:VALARM",
    ]);
    // all-day: DATE values, an empty or inverted end becomes one day; UNTIL a date
    let mut a = input.clone();
    a.all_day = true;
    a.end = dt("2026-10-14T00:00");
    a.alarms = vec![];
    // the order check compares the times even for an all-day event
    assert!(matches!(build_vevent("u", 0, &a, tz, None), Err(AppError::BadRequest(m)) if m == "end before start"));
    a.end = a.start;
    a.exdates = vec!["2026-10-15".into()];
    let l = build_vevent("u", 0, &a, tz, None).unwrap();
    assert!(l.contains(&"DTSTART;VALUE=DATE:20261014".to_string()) && l.contains(&"DTEND;VALUE=DATE:20261015".to_string()), "{l:?}");
    assert!(l.contains(&"RRULE:FREQ=DAILY;UNTIL=20261031".to_string()) && l.contains(&"EXDATE;VALUE=DATE:20261015".to_string()), "{l:?}");
    // the RRULE and the dates are dropped without a rule
    let mut n = a.clone();
    n.rrule = Some("  ".into());
    assert!(!build_vevent("u", 0, &n, tz, None).unwrap().iter().any(|l| l.starts_with("RRULE") || l.starts_with("EXDATE") || l.starts_with("RDATE")));
    // geo in range, a valid colour, priority 0
    let mut g = a.clone();
    g.geo = Some((51.0, 13.5));
    g.color = "#AbC123".into();
    g.priority = Some(0);
    let l = build_vevent("u", 0, &g, tz, None).unwrap();
    assert!(l.contains(&"GEO:51.000000;13.500000".to_string()) && l.contains(&"COLOR:#AbC123".to_string()) && l.contains(&"PRIORITY:0".to_string()), "{l:?}");
    // errors
    let mut bad = a.clone();
    bad.rrule = Some("FREQ=SOMETIMES".into());
    assert!(build_vevent("u", 0, &bad, tz, None).is_err());
}

#[test]
fn assembled_object_shape() {
    let tz = berlin();
    let input: EventInput = serde_json::from_value(serde_json::json!({ "principal": "p", "cal_id": "c", "summary": "x".repeat(100), "start": "2026-10-14T09:00:00", "end": "2026-10-14T10:00:00" })).unwrap();
    let ics = assemble(ical::vtimezone(tz.name()), vec![build_vevent("u", 0, &input, tz, None).unwrap()], &["BEGIN:VEVENT\r\nRECURRENCE-ID:20261021T070000Z\r\nEND:VEVENT\r\n".to_string()]);
    assert!(ics.starts_with("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//xarif.de//PractiCal//DE\r\nCALSCALE:GREGORIAN\r\nBEGIN:VTIMEZONE\r\nTZID:Europe/Berlin\r\n"), "{ics}");
    assert!(ics.ends_with("END:VEVENT\r\nBEGIN:VEVENT\r\nRECURRENCE-ID:20261021T070000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"), "{ics}");
    assert!(ics.lines().all(|l| l.len() <= 76), "folded at 75 octets");
    assert!(ics.contains("\r\n x"), "a continuation line");
}

#[test]
fn date_and_person_lines() {
    let tz = berlin();
    let props = vec![
        "EXDATE;TZID=America/New_York:20261021T090000,20261028T090000".to_string(),
        "EXDATE:20261101T080000Z".into(),
        "EXDATE;VALUE=DATE:20261224".into(),
        "EXDATE:20261225".into(),
        "RDATE;VALUE=PERIOD:20261101T080000Z/PT1H".into(),
        "RDATE:20261102T120000".into(),
    ];
    assert_eq!(dates_of(&props, "EXDATE", tz), ["2026-10-21T15:00:00", "2026-10-28T14:00:00", "2026-11-01T09:00:00", "2026-12-24", "2026-12-25"]);
    assert_eq!(dates_of(&props, "RDATE", tz), ["2026-11-02T12:00:00"], "a PERIOD is not modelled");
    assert_eq!(date_lines("EXDATE", &["2026-10-21T15:00:00".into(), "2026-10-22T08:30".into(), "2026-10-23".into(), "x".into()], false, tz), ["EXDATE;TZID=Europe/Berlin:20261021T150000", "EXDATE;TZID=Europe/Berlin:20261022T083000", "EXDATE;VALUE=DATE:20261023"]);
    assert_eq!(date_lines("RDATE", &["2026-10-21T15:00:00".into()], true, tz), ["RDATE;VALUE=DATE:20261021"]);
    let people = persons_of(&["ATTENDEE;CN=\"Müller, Anna\";ROLE=opt-participant;PARTSTAT=tentative;CUTYPE=room;X-NUM=1:MAILTO:raum@example.org".to_string(), "ORGANIZER:mailto:o@example.org".into()], "ATTENDEE");
    assert_eq!(people.len(), 1);
    let p = &people[0];
    assert_eq!((p.name.as_str(), p.email.as_str(), p.role.as_str(), p.status.as_str(), p.cutype.as_str()), ("Müller, Anna", "raum@example.org", "OPT-PARTICIPANT", "TENTATIVE", "ROOM"));
    assert_eq!(p.extra, [("X-NUM".to_string(), "1".to_string())]);
    assert_eq!(person_line("ATTENDEE", p).unwrap(), "ATTENDEE;CN=\"Müller, Anna\";ROLE=OPT-PARTICIPANT;PARTSTAT=TENTATIVE;CUTYPE=ROOM;X-NUM=1:mailto:raum@example.org");
    assert_eq!(person_line("ATTENDEE", &Person { name: "Sa\"m".into(), email: " s@example.org ".into(), ..Default::default() }).unwrap(), "ATTENDEE;CN=Sam:mailto:s@example.org");
    assert_eq!(person_line("ATTENDEE", &Person { name: "x".into(), ..Default::default() }), None);
}

#[test]
fn until_and_rrule_validation() {
    let tz = berlin();
    for (rule, all_day, out) in [
        ("FREQ=DAILY;UNTIL=20260330", false, "FREQ=DAILY;UNTIL=20260330T215959Z"),
        ("FREQ=DAILY;UNTIL=20260130", false, "FREQ=DAILY;UNTIL=20260130T225959Z"),
        ("FREQ=DAILY;UNTIL=20260130T120000", false, "FREQ=DAILY;UNTIL=20260130T120000"),
        ("FREQ=DAILY;UNTIL=20260130T230000Z", true, "FREQ=DAILY;UNTIL=20260131"),
        // the rule comes out canonical (upper-case) even where UNTIL does not read
        ("FREQ=DAILY;UNTIL=garbage", false, "FREQ=DAILY;UNTIL=GARBAGE"),
        // RFC 5545 rule parts are case-insensitive: a lower-case UNTIL is normalised too
        ("freq=daily;until=20260130", false, "FREQ=DAILY;UNTIL=20260130T225959Z"),
        ("FREQ=DAILY;COUNT=3", false, "FREQ=DAILY;COUNT=3"),
    ] {
        assert_eq!(normalize_until(rule, all_day, tz), out, "{rule}");
    }
    assert_eq!(rrule_until_local("FREQ=DAILY;UNTIL=20260130T230000Z", tz), Some(date("2026-01-31")));
    assert_eq!(rrule_until_local("FREQ=DAILY;UNTIL=20260130", tz), Some(date("2026-01-30")));
    assert_eq!(rrule_until_local("FREQ=DAILY", tz), None);
    assert_eq!(rrule_until_local("freq=daily;until=20260130t230000z", tz), Some(date("2026-01-31")));
    assert_eq!(validate_rrule(" RRULE:freq=weekly;byday=mo ").unwrap(), "FREQ=WEEKLY;BYDAY=MO");
    for bad in ["", "RRULE:", "BYDAY=MO", "FREQ=DAILY\nX", "FREQ=YEARLY;BYMONTH=13", "FREQ=FORTNIGHTLY"] {
        assert!(validate_rrule(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn alarms_offsets_and_presets() {
    assert_eq!(fmt_trigger_offset(0), "PT0S");
    assert_eq!(fmt_trigger_offset(15), "-PT15M");
    assert_eq!(fmt_trigger_offset(120), "-PT2H");
    assert_eq!(fmt_trigger_offset(2880), "-P2D");
    assert_eq!(fmt_trigger_offset(-90), "PT90M");
    let tz = berlin();
    let a = parse_alarm(3, &["ACTION:audio".to_string(), "ATTACH;FMTTYPE=audio/basic:https://x/a.au".into(), "TRIGGER:20261013T180000Z".into(), "REPEAT:x".into(), "PROXIMITY:nowhere".into()], tz);
    assert_eq!((a.idx, a.action.as_str(), a.attach.as_str(), a.at, a.repeat, a.proximity.as_str()), (Some(3), "AUDIO", "https://x/a.au", Some(dt("2026-10-13T20:00")), 0, ""));
    let d = parse_alarm(0, &["TRIGGER;RELATED=END:PT10M".to_string(), "ATTACH:https://x/y".into()], tz);
    assert_eq!((d.action.as_str(), d.minutes, d.related_end, d.attach.as_str()), ("DISPLAY", -10, true, ""), "a DISPLAY alarm keeps no sound");
    assert_eq!(parse_offsets("1 w; 2 Tage\n3 St"), Ok(vec![180, 2880, 10080]));
    assert_eq!(parse_offsets("367 d"), Err("367 d".into()));
    assert_eq!(parse_offsets("5 x"), Err("5 x".into()));
    assert_eq!(parse_offsets(&(1..=31).map(|n| n.to_string()).collect::<Vec<_>>().join(",")), Err("too many".into()));
    assert_eq!(fmt_offsets(&[]), "");
    assert_eq!(fmt_offsets(&AlarmSnaps::default().start), "2 min, 5 min, 15 min, 30 min, 1 h, 3 h, 8 h, 1 d, 2 d, 3 d, 1 w, 2 w");
    let p = |j: serde_json::Value| validate_presets(&serde_json::from_value::<Vec<AlarmPreset>>(j).unwrap()).is_ok();
    assert!(p(serde_json::json!([{ "action": "display", "minutes": 527040 }])));
    assert!(!p(serde_json::json!([{ "action": "DISPLAY", "minutes": 527041 }])));
    assert!(!p(serde_json::json!([{ "action": "DISPLAY", "repeat": 101 }])));
    assert!(!p(serde_json::json!([{ "action": "DISPLAY", "duration_minutes": -1 }])));
    assert!(!p(serde_json::json!([{ "action": "DISPLAY", "proximity": "near" }])));
    assert!(p(serde_json::json!([{ "action": "DISPLAY", "proximity": "connect" }])));
    assert!(!p(serde_json::Value::Array((0..51).map(|_| serde_json::json!({ "action": "DISPLAY" })).collect())));
    assert_eq!(alarm_snaps(Some("not json")), AlarmSnaps::default());
}

#[test]
fn raw_split_and_small_helpers() {
    let ics = fixture("demo-event-full.ics");
    let vtz = vtimezone_blocks(&ics);
    assert_eq!(vtz.len(), 1);
    assert!(vtz[0].starts_with("BEGIN:VTIMEZONE\r\nTZID:Europe/Berlin") && vtz[0].ends_with("END:VTIMEZONE"));
    let (todo, _) = split_component(&crate::testkit::vtodo("t", "Aufgabe", "DUE;VALUE=DATE:20261015\nBEGIN:VALARM\nTRIGGER:-PT1H\nEND:VALARM"), "VTODO");
    assert_eq!(todo.props.iter().map(|l| prop_name(l)).collect::<Vec<_>>(), ["UID", "DTSTAMP", "SUMMARY", "DUE"]);
    assert_eq!(todo.alarms, [vec!["TRIGGER:-PT1H".to_string()]]);
    let (none, ov) = split_ics("BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n");
    assert!(none.props.is_empty() && ov.is_empty());
    assert_eq!(unfold("A:1\r\n 2\r\n\t3\r\nB:x"), ["A:123", "B:x"]);
    assert_eq!(unfold(" lead\nA"), [" lead", "A"], "a continuation without a line before it stays");
    assert_eq!(prop_name("dtstart;tzid=x:1"), "DTSTART");
    assert!(valid_color("#123") && valid_color("tomato") && !valid_color("") && !valid_color("rgb(1,2,3)") && !valid_color(&"a".repeat(33)));
    assert_eq!(one_of(" confirmed ", &["CONFIRMED"]), Some("CONFIRMED"));
    assert_eq!(one_of("x", &["CONFIRMED"]), None);
    assert_eq!(base64_len("aGVsbG8="), 5);
    assert_eq!(base64_len("aGVs\r\nbG8="), 5);
    assert_eq!(base64_len(""), 0);
    // padding alone, short and unpadded texts: no underflow, the bytes the data characters hold
    for (b64, n) in [("==", 0), ("=", 0), ("Y", 0), ("YQ", 1), ("YQ==", 1), ("YWI", 2), ("YWI=", 2), ("aGVsbG8", 5), ("aGVs bG8 =", 5)] {
        assert_eq!(base64_len(b64), n, "{b64}");
    }
    assert!(is_binary_attach(&[("ENCODING".into(), "BASE64".into())]) && is_binary_attach(&[("VALUE".into(), "binary".into())]) && !is_binary_attach(&[("FMTTYPE".into(), "x".into())]));
    assert!(is_managed("SUMMARY:x") && !is_managed("X-FOO:y") && !is_managed("ATTACH;ENCODING=BASE64;VALUE=BINARY:aGk=") && is_managed("ATTACH:https://x"));
    // instance helpers
    let i = crate::testkit::instance("a", "x", "X", "2026-10-14T23:00", "2026-10-15T00:00");
    assert_eq!((i.last_date(), i.multi_day(), i.time_label(), i.end_label(), i.key()), (date("2026-10-14"), false, "23:00".to_string(), "00:00".to_string(), "alice/a/x".to_string()));
    let z = crate::testkit::instance("a", "z", "Z", "2026-10-14T23:00", "2026-10-14T23:00");
    assert_eq!(z.last_date(), date("2026-10-14"));
    let ad = crate::testkit::instance("a", "d", "D", "2026-10-14", "2026-10-16");
    assert_eq!((ad.last_date(), ad.multi_day(), ad.time_label()), (date("2026-10-15"), true, String::new()));
    let _ = NaiveDate::MIN;
}

#[test]
fn links_and_embedded_limits() {
    let links = vec![
        Link { kind: "url".into(), uri: "https://a/".into(), ..Default::default() },
        Link { kind: "url".into(), uri: "https://b/".into(), label: "B \"x\"".into(), ..Default::default() },
        Link { kind: "conference".into(), uri: "https://c/".into(), features: vec!["video".into(), "a;b".into()], ..Default::default() },
        Link { kind: "attach".into(), uri: "https://d/f.pdf".into(), fmt: "application/pdf".into(), filename: "Mein Bericht.pdf".into(), size: Some(42), extra: vec![("X-A".into(), "1".into())], ..Default::default() },
        Link { kind: "attach".into(), uri: "line\nbreak".into(), ..Default::default() },
        Link { kind: "other".into(), uri: "https://e/".into(), ..Default::default() },
        Link { kind: "url".into(), uri: "  ".into(), ..Default::default() },
    ];
    assert_eq!(link_lines(&links), [
        "URL:https://a/",
        "ATTACH;LABEL=\"B x\":https://b/",
        "CONFERENCE;VALUE=URI;FEATURE=VIDEO,AB:https://c/",
        "ATTACH;FMTTYPE=application/pdf;FILENAME=\"Mein Bericht.pdf\";SIZE=42;X-A=1:https://d/f.pdf",
    ]);
    let ok = embedded_lines(&[Link { kind: "attach".into(), embedded: true, data: Some(" aGk= ".into()), ..Default::default() }, Link { embedded: true, ..Default::default() }]).unwrap();
    assert_eq!(ok, ["ATTACH;ENCODING=BASE64;VALUE=BINARY:aGk="]);
    let two_mb = "A".repeat(EMBED_FILE_MAX / 3 * 4);
    assert!(embedded_lines(&[Link { embedded: true, data: Some(two_mb.clone()), ..Default::default() }]).is_ok());
    let three: Vec<Link> = (0..4).map(|_| Link { embedded: true, data: Some(two_mb.clone()), ..Default::default() }).collect();
    assert!(matches!(embedded_lines(&three), Err(AppError::BadRequest(m)) if m.starts_with("attachment too large")));
    assert_eq!(related_lines(&[Related { uid: " p ".into(), reltype: "parent".into(), ..Default::default() }, Related { uid: "a;b".into(), ..Default::default() }, Related { uid: "c".into(), reltype: "COUSIN".into(), ..Default::default() }]), ["RELATED-TO;RELTYPE=PARENT:p", "RELATED-TO:c"]);
}
