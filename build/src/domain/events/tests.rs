use super::*;
use chrono::Datelike;

/// The real calendar's 60 recurring series, anonymised, against frozen
/// python-dateutil expectations (first 60 instances each, tz-normalised).
#[test]
fn recurrence_matches_dateutil_oracle() {
    #[derive(serde::Deserialize)]
    struct Fx {
        name: String,
        ics: String,
        expected: Vec<String>,
        allday: bool,
    }
    let fx: Vec<Fx> = serde_json::from_str(include_str!("../../../tests/fixtures/recurrence-real.json")).unwrap();
    assert_eq!(fx.len(), 60);
    let mut checked = 0;
    for f in &fx {
        let obj = CalendarObject::from_ics(f.ics.clone()).unwrap_or_else(|e| panic!("{}: {e}", f.name));
        let CalendarInnerData::Event(main, _) = obj.get_inner().get_inner() else { panic!("{} not an event", f.name) };
        let set = main.get_rruleset().unwrap_or_else(|| panic!("{} has no rrule set", f.name));
        let got: Vec<String> = set
            .all(60)
            .dates
            .iter()
            .map(|d| if f.allday { d.date_naive().format("%Y-%m-%d").to_string() } else { d.with_timezone(&chrono::Utc).format("%Y-%m-%dT%H:%M:%SZ").to_string() })
            .filter(|s| s[..4].parse::<i32>().unwrap() < 2100)
            .collect();
        let exp: Vec<String> = f.expected.iter().filter(|s| s[..4].parse::<i32>().unwrap() < 2100).cloned().collect();
        assert_eq!(got, exp, "series {}", f.name);
        checked += got.len();
    }
    assert!(checked > 2000, "only {checked} instances compared");
}

/// An all-day yearly series: caldata expands DATE + duration into a UTC DateTime end; the
/// instance must still be exactly one day (summer instance → zone offset must not leak).
#[test]
fn allday_series_instance_is_one_day() {
    let tz: chrono_tz::Tz = "Europe/Berlin".parse().unwrap();
    let ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//t//EN\r\nBEGIN:VEVENT\r\nUID:bday\r\nDTSTAMP:20240901T000000Z\r\nSUMMARY:Geburtstag\r\nDTSTART;VALUE=DATE:20240911\r\nDTEND;VALUE=DATE:20240912\r\nRRULE:FREQ=YEARLY;WKST=MO\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let obj = CalendarObject::from_ics(ics.to_string()).unwrap();
    let CalendarInnerData::Event(main, overrides) = obj.get_inner().get_inner() else { panic!("not an event") };
    let cal = CalInfo {
        principal: "p".into(),
        owner_label: "p".into(),
        is_group: false,
        id: "c".into(),
        displayname: "c".into(),
        color: "#000".into(),
        description: String::new(),
        components: vec!["VEVENT".into()],
        timezone_id: "Europe/Berlin".into(),
        subscription_url: None,
        deleted_at: None,
        synctoken: 0,
        read_only: false,
        managed: false,
        ..CalInfo::bare(crate::state::Authorized::test_own("p", "c"))
    };
    let ws = ical::to_utc(NaiveDate::from_ymd_opt(2026, 9, 1).unwrap().and_hms_opt(0, 0, 0).unwrap(), tz);
    let we = ical::to_utc(NaiveDate::from_ymd_opt(2026, 9, 30).unwrap().and_hms_opt(0, 0, 0).unwrap(), tz);
    let expanded = main.expand_recurrence(Some(ws), Some(we), overrides);
    let inst: Vec<Instance> = expanded.iter().map(|ev| instance_from(ev, &cal, "bday", tz, true, vec![])).collect();
    assert_eq!(inst.len(), 1);
    let i = &inst[0];
    assert!(i.all_day);
    assert_eq!(i.start, NaiveDate::from_ymd_opt(2026, 9, 11).unwrap().and_hms_opt(0, 0, 0).unwrap());
    assert_eq!(i.end, NaiveDate::from_ymd_opt(2026, 9, 12).unwrap().and_hms_opt(0, 0, 0).unwrap());
    assert_eq!(i.last_date(), NaiveDate::from_ymd_opt(2026, 9, 11).unwrap());
    assert!(!i.multi_day());
}

#[test]
fn build_and_reparse_event() {
    let tz: chrono_tz::Tz = "Europe/Berlin".parse().unwrap();
    let input = EventInput {
        principal: "p".into(),
        cal_id: "c".into(),
        summary: "Zahnarzt, Kontrolle".into(),
        location: "Praxis".into(),
        description: "Karte\nmitnehmen".into(),
        start: NaiveDate::from_ymd_opt(2026, 9, 3).unwrap().and_hms_opt(8, 30, 0).unwrap(),
        end: NaiveDate::from_ymd_opt(2026, 9, 3).unwrap().and_hms_opt(9, 15, 0).unwrap(),
        all_day: false,
        rrule: Some("FREQ=WEEKLY;COUNT=3".into()),
        alarms: vec![Alarm { minutes: 15, ..Default::default() }],
        exdates: vec![],
        rdates: vec![],
        organizer: None,
        attendees: vec![],
        categories: vec!["Arzt".into()],
        status: String::new(),
        transp: String::new(),
        class: String::new(),
        priority: None,
        color: String::new(),
        geo: None,
        links: vec![],
        related: vec![],
        timezone: None,
        confirm_visibility: false,
        confirm_alarms: false,
    };
    let lines = build_vevent("uid-1", 0, &input, tz, None).unwrap();
    let ics = ical::assemble(ical::vtimezone("Europe/Berlin"), "VEVENT", lines, &[]);
    let obj = CalendarObject::import(&ics, None).unwrap();
    let CalendarInnerData::Event(main, overrides) = obj.get_inner().get_inner() else { panic!() };
    assert!(main.has_rruleset());
    assert_eq!(alarm_offsets(main), vec![15]);
    let ex = main.expand_recurrence(None, None, overrides);
    assert_eq!(ex.len(), 3);
    let cal = CalInfo { principal: "p".into(), owner_label: String::new(), is_group: false, id: "c".into(), displayname: "Test".into(), color: "#000".into(), description: String::new(), components: vec![], timezone_id: String::new(), subscription_url: None, deleted_at: None, synctoken: 0, read_only: false, managed: false, ..CalInfo::bare(crate::state::Authorized::test_own("p", "c")) };
    let i = instance_from(&ex[1], &cal, "uid-1", tz, true, alarm_offsets(main));
    assert_eq!(i.start.date().day(), 10);
    assert_eq!(&*i.summary, "Zahnarzt, Kontrolle");
    assert_eq!(&*i.description, "Karte\nmitnehmen");
    assert_eq!((i.end - i.start).num_minutes(), 45);
    assert!(validate_rrule("DAILY").is_err());
}

/// Property histogram of a VEVENT block (nested VALARMs prefixed), CATEGORIES merged.
fn histogram(ics: &str) -> std::collections::BTreeMap<String, usize> {
    let (m, _) = split_ics(ics);
    let mut h = std::collections::BTreeMap::new();
    for l in &m.props {
        let n = prop_name(l);
        if n != "LAST-MODIFIED" && n != "DTSTAMP" {
            *h.entry(n).or_insert(0) += 1;
        }
    }
    for a in &m.alarms {
        for l in a {
            *h.entry(format!("VALARM:{}", prop_name(l))).or_insert(0) += 1;
        }
    }
    h.entry("CATEGORIES".into()).and_modify(|v| *v = 1);
    h
}

/// The demo fixture carries every VEVENT property there is (RFC 5545/7986/9073/9074 + X-);
/// an unchanged round trip through the editor's model must keep all of them, alarm by alarm.

#[test]
fn changed_description_drops_stale_html_description() {
    let tz: chrono_tz::Tz = "Europe/Berlin".parse().unwrap();
    let (raw, _) = split_ics(include_str!("../../../tests/fixtures/demo-event-full.ics"));
    let mut input: EventInput = serde_json::from_str(r#"{"principal":"p","cal_id":"c","summary":"s","description":"Zeilen mit Umbruch\nund Sonderzeichen, Komma; Semikolon.\nZweiter Absatz.","start":"2026-09-22T19:00:00","end":"2026-09-22T20:00:00"}"#).unwrap();
    let same = build_vevent("u", 1, &input, tz, Some(&raw)).unwrap().join("\n");
    assert!(same.contains("X-ALT-DESC") && same.contains("STYLED-DESCRIPTION"), "{same}");
    input.description = "neuer Text".into();
    let changed = build_vevent("u", 1, &input, tz, Some(&raw)).unwrap().join("\n");
    assert!(!changed.contains("X-ALT-DESC") && !changed.contains("STYLED-DESCRIPTION") && changed.contains("DESCRIPTION:neuer Text"), "{changed}");
}

#[test]
fn links_related_and_embedded_attachments() {
    let tz: chrono_tz::Tz = "Europe/Berlin".parse().unwrap();
    let raw = RawMaster { props: vec![
        "ATTACH;FMTTYPE=application/pdf;FILENAME=/x.pdf;X-NC-FILE-ID=1:https://cloud.example.org/x.pdf".into(),
        "ATTACH;FMTTYPE=image/jpeg;X-FILENAME=scan.jpg;ENCODING=BASE64;VALUE=BINARY:aGVsbG8=".into(),
        "ATTACH;ENCODING=BASE64;VALUE=BINARY:d29ybGQ=".into(),
        "CONFERENCE;VALUE=URI;FEATURE=audio,phone:tel:+49691234".into(),
        "RELATED-TO:abc".into(),
        "RELATED-TO;RELTYPE=CHILD;X-FOO=1:def".into(),
    ], alarms: vec![] };
    let links = links_of(&raw.props);
    assert_eq!(links.len(), 4);
    assert_eq!((links[0].fmt.as_str(), links[0].filename.as_str(), links[0].extra.len(), links[0].embedded), ("application/pdf", "x.pdf", 1, false));
    assert_eq!((links[1].embedded, links[1].bin_index, links[1].size, links[1].filename.as_str(), links[1].keep), (true, Some(0), Some(5), "scan.jpg", true));
    assert_eq!((links[2].bin_index, links[2].size), (Some(1), Some(5)));
    assert_eq!(links[3].features, vec!["AUDIO", "PHONE"]);
    let (fmt, name, bytes) = embedded_attachment(&raw.props, 1).unwrap();
    assert_eq!((fmt.as_str(), name.as_str(), bytes.as_slice()), ("application/octet-stream", "anhang-2", b"world".as_slice()));
    assert_eq!(embedded_attachment(&raw.props, 0).unwrap().1, "scan.jpg");
    assert!(embedded_attachment(&raw.props, 2).is_none());
    let related = related_of(&raw.props);
    assert_eq!(related.len(), 2);
    assert_eq!((related[1].reltype.as_str(), related[1].extra.len()), ("CHILD", 1));
    // save: drop the first embedded file, keep the second, add a new one, keep the RELATED-TOs
    let mut input: EventInput = serde_json::from_str(r#"{"principal":"p","cal_id":"c","summary":"s","description":"","start":"2026-09-22T19:00:00","end":"2026-09-22T20:00:00"}"#).unwrap();
    input.links = links.clone();
    input.links[1].keep = false;
    input.links.push(Link { kind: "attach".into(), embedded: true, data: Some("aGk=".into()), fmt: "text/plain".into(), filename: "new file.txt".into(), ..Default::default() });
    input.related = related;
    let lines = build_vevent("u", 1, &input, tz, Some(&raw)).unwrap();
    let flat = lines.join("\n");
    assert!(!flat.contains("aGVsbG8="), "{flat}");
    assert!(flat.contains("ATTACH;ENCODING=BASE64;VALUE=BINARY:d29ybGQ="), "{flat}");
    assert!(flat.contains("ATTACH;FMTTYPE=text/plain;FILENAME=\"new file.txt\";ENCODING=BASE64;VALUE=BINARY:aGk="), "{flat}");
    assert!(flat.contains("ATTACH;FMTTYPE=application/pdf;FILENAME=x.pdf;X-NC-FILE-ID=1:https://cloud.example.org/x.pdf"), "{flat}");
    assert!(flat.contains("CONFERENCE;VALUE=URI;FEATURE=AUDIO,PHONE:tel:+49691234"), "{flat}");
    assert!(flat.contains("RELATED-TO:abc") && flat.contains("RELATED-TO;RELTYPE=CHILD;X-FOO=1:def"), "{flat}");
    input.links.push(Link { kind: "url".into(), uri: "https://a.example.org/".into(), ..Default::default() });
    input.links.push(Link { kind: "url".into(), uri: "https://b.example.org/".into(), label: "B".into(), ..Default::default() });
    let flat2 = build_vevent("u", 1, &input, tz, Some(&raw)).unwrap().join("\n");
    assert!(flat2.contains("URL:https://a.example.org/") && flat2.contains("ATTACH;LABEL=\"B\":https://b.example.org/"), "{flat2}");
    input.links.truncate(input.links.len() - 2);
    assert_eq!(flat.matches("RELATED-TO").count(), 2);
    // limits
    let big = "A".repeat(4 * (EMBED_FILE_MAX / 3 + 10));
    input.links.push(Link { kind: "attach".into(), embedded: true, data: Some(big), ..Default::default() });
    assert!(build_vevent("u", 1, &input, tz, Some(&raw)).is_err());
    input.links.pop();
    input.links.push(Link { kind: "attach".into(), embedded: true, data: Some("not base64!".into()), ..Default::default() });
    assert!(build_vevent("u", 1, &input, tz, Some(&raw)).is_err());
}

#[test]
fn unchanged_save_keeps_every_property() {
    let tz: chrono_tz::Tz = "Europe/Berlin".parse().unwrap();
    let ics = include_str!("../../../tests/fixtures/demo-event-full.ics");
    let (raw, overrides) = split_ics(ics);
    assert_eq!(raw.alarms.len(), 5);
    assert_eq!(overrides.len(), 1);
    let alarms: Vec<Alarm> = raw.alarms.iter().enumerate().map(|(n, a)| parse_alarm(n, a, tz)).collect();
    assert_eq!(alarms[0].minutes, 15);
    assert_eq!((alarms[1].minutes, alarms[1].repeat, alarms[1].duration_minutes), (1440, 2, 10));
    assert!(alarms[2].related_end && alarms[2].action == "AUDIO");
    assert!(alarms[3].at.is_some() && alarms[3].action == "EMAIL");
    let input = EventInput {
        principal: "p".into(),
        cal_id: "c".into(),
        summary: "Demo-Termin: alle Eigenschaften".into(),
        location: "Kulturpalast, Schloßstraße 2, 01067 Dresden".into(),
        description: "Zeilen mit Umbruch\nund Sonderzeichen, Komma; Semikolon.\nZweiter Absatz.".into(),
        start: NaiveDate::from_ymd_opt(2026, 9, 22).unwrap().and_hms_opt(19, 0, 0).unwrap(),
        end: NaiveDate::from_ymd_opt(2026, 9, 22).unwrap().and_hms_opt(21, 30, 0).unwrap(),
        all_day: false,
        rrule: Some("FREQ=WEEKLY;BYDAY=TU;COUNT=6".into()),
        alarms,
        exdates: dates_of(&raw.props, "EXDATE", tz),
        rdates: dates_of(&raw.props, "RDATE", tz),
        organizer: persons_of(&raw.props, "ORGANIZER").into_iter().next(),
        attendees: persons_of(&raw.props, "ATTENDEE"),
        categories: vec!["Familie".into(), "Kultur".into(), "Demo".into()],
        status: "CONFIRMED".into(),
        transp: "OPAQUE".into(),
        class: "PRIVATE".into(),
        priority: Some(5),
        color: "tomato".into(),
        geo: Some((51.050407, 13.737262)),
        links: links_of(&raw.props),
        related: related_of(&raw.props),
        timezone: Some("Europe/Berlin".into()),
        confirm_visibility: false,
        confirm_alarms: false,
    };
    assert_eq!(input.links.iter().map(|l| (l.kind.as_str(), l.embedded)).collect::<Vec<_>>(), vec![("url", false), ("conference", false), ("attach", false), ("attach", true)]);
    assert_eq!(input.links[1].features, vec!["AUDIO", "VIDEO"]);
    assert_eq!((input.related.len(), input.related[0].reltype.as_str(), input.related[0].uid.as_str()), (1, "PARENT", "parent-event@xarif.de"));
    assert_eq!(input.links[1].label, "Videocall");
    assert_eq!(input.exdates, vec!["2026-10-06T19:00:00"]);
    assert_eq!(input.rdates, vec!["2026-10-10T19:00:00"]);
    assert_eq!(input.attendees.len(), 3);
    assert_eq!(input.attendees[0].extra, vec![("RSVP".to_string(), "TRUE".to_string())]);
    assert_eq!(input.organizer.as_ref().map(|o| o.name.as_str()), Some("Thomas Steinbach"));
    let lines = build_vevent("demo-event-full@xarif.de", 4, &input, tz, Some(&raw)).unwrap();
    let out = ical::assemble(ical::vtimezone("Europe/Berlin"), "VEVENT", lines, &overrides);
    let (before, after) = (histogram(ics), histogram(&out));
    let lost: Vec<String> = before.iter().filter(|(k, v)| after.get(*k).copied().unwrap_or(0) < **v).map(|(k, v)| format!("{k} {v}->{}", after.get(k).copied().unwrap_or(0))).collect();
    assert!(lost.is_empty(), "lost on save: {lost:?}");
    // and the result is still a valid object with the same series, override and alarms
    let obj = CalendarObject::import(&out, None).unwrap();
    let CalendarInnerData::Event(main, overrides) = obj.get_inner().get_inner() else { panic!() };
    assert_eq!(overrides.len(), 1);
    assert_eq!(main.get_alarms().len(), 5);
    assert_eq!(alarm_offsets(main), vec![15, 1440, 5 - 150, 60]);
    let flat = unfold(&out.replace("\r\n", "\n")).join("\n"); // long lines are folded in `out`
    assert!(flat.contains("ACKNOWLEDGED:20260915T170000Z") && flat.contains("PROXIMITY:DEPART"), "{flat}");
    assert!(flat.contains("ATTENDEE;CN=Erika Musterfrau;ROLE=REQ-PARTICIPANT;PARTSTAT=ACCEPTED;CUTYPE=INDIVIDUAL;RSVP=TRUE:mailto:erika@example.org"), "{flat}");
    assert!(flat.contains("EXDATE;TZID=Europe/Berlin:20261006T190000") && flat.contains("RDATE;TZID=Europe/Berlin:20261010T190000"), "{flat}");
    assert!(flat.contains("CONFERENCE;VALUE=URI;FEATURE=AUDIO,VIDEO;LABEL=\"Videocall\":https://meet.example.org/demo"), "{flat}");
    assert!(flat.contains("ATTACH;FMTTYPE=text/plain;ENCODING=BASE64;VALUE=BINARY:SGFsbG8gRGVtbyE=") && flat.contains("GEO:51.050407;13.737262") && flat.contains("COLOR:tomato"), "{flat}");
    assert_eq!(flat.matches("\nURL:").count(), 1);
    assert_eq!(main.expand_recurrence(None, None, overrides).len(), 6, "5 rule instances after the exclusion + the RDATE");
}

#[test]
fn alarm_fields_round_trip() {
    let tz: chrono_tz::Tz = "Europe/Berlin".parse().unwrap();
    let ics = include_str!("../../../tests/fixtures/demo-event-full.ics");
    let (raw, _) = split_ics(ics);
    let mut alarms: Vec<Alarm> = raw.alarms.iter().enumerate().map(|(n, a)| parse_alarm(n, a, tz)).collect();
    // the fixture's fields land in the model
    assert_eq!(alarms[2].attach, "https://example.org/ping.au");
    assert_eq!((alarms[3].summary.as_str(), alarms[3].attendees.as_slice()), ("Erinnerung: Demo-Termin", &["thomas@example.org".to_string()][..]));
    assert_eq!(alarms[4].proximity, "DEPART");
    assert_eq!(alarms[1].acknowledged, NaiveDate::from_ymd_opt(2026, 9, 15).unwrap().and_hms_opt(19, 0, 0)); // 17:00Z in Berlin
    // edited: a second recipient and a new subject, a new sound, a proximity change, an acknowledgement now
    alarms[3].attendees.push("anna@example.org".into());
    alarms[3].summary = "Neuer Betreff".into();
    alarms[2].attach = "https://example.org/pong.au".into();
    alarms[4].proximity = "ARRIVE".into();
    alarms[0].acknowledged = NaiveDate::from_ymd_opt(2026, 9, 17).unwrap().and_hms_opt(14, 0, 0);
    let out: Vec<String> = alarms.iter().flat_map(|a| build_valarm(a, "Titel", Some(&raw), tz)).collect();
    let flat = out.join("\n");
    assert!(flat.contains("ATTENDEE:mailto:thomas@example.org\nATTENDEE:mailto:anna@example.org"), "{flat}");
    assert!(flat.contains("SUMMARY:Neuer Betreff") && flat.contains("DESCRIPTION:Morgen Abend im Kulturpalast."), "{flat}");
    assert!(flat.contains("ATTACH:https://example.org/pong.au") && !flat.contains("ping.au"), "{flat}");
    assert!(flat.contains("PROXIMITY:ARRIVE") && !flat.contains("DEPART"), "{flat}");
    assert!(flat.contains("ACKNOWLEDGED:20260917T120000Z") && flat.contains("ACKNOWLEDGED:20260915T170000Z"), "{flat}");
    assert_eq!(flat.matches("UID:demo-alarm-").count(), 2, "alarm UIDs ride along: {flat}");
    // unchanged: the sound keeps its FMTTYPE, the edited alarm still parses to the same model
    let kept: Vec<String> = build_valarm(&parse_alarm(2, &raw.alarms[2], tz), "Titel", Some(&raw), tz);
    assert!(kept.iter().any(|l| l == "ATTACH;FMTTYPE=audio/basic:https://example.org/ping.au"), "{kept:?}");
    let again = parse_alarm(0, &out[out.iter().position(|l| l == "BEGIN:VALARM").unwrap() + 1..out.iter().position(|l| l == "END:VALARM").unwrap()], tz);
    assert_eq!((again.minutes, again.acknowledged), (15, alarms[0].acknowledged));
    assert!(validate_presets(&[AlarmPreset { action: "EMAIL".into(), minutes: 30, ..Default::default() }]).is_ok());
    assert!(validate_presets(&[AlarmPreset { action: "PROCEDURE".into(), ..Default::default() }]).is_err());
}

#[test]
fn snap_point_lists() {
    assert_eq!(parse_offsets("2 min, 1 h, 1 Std., 1 Tag, 2 Tage, 1 Wo, 30, 1.5h"), Ok(vec![2, 30, 60, 90, 1440, 2880, 10080]));
    assert_eq!(parse_offsets(""), Ok(vec![]));
    assert_eq!(parse_offsets("5 min, x"), Err("x".into()));
    assert_eq!(parse_offsets("0 min"), Err("0 min".into()));
    assert_eq!(fmt_offsets(&[2, 90, 60, 1440, 20160]), "2 min, 90 min, 1 h, 1 d, 2 w");
    assert_eq!(alarm_snaps(None), AlarmSnaps::default());
    assert_eq!(alarm_snaps(Some("{\"start\":[],\"end\":[]}")), AlarmSnaps::default(), "an empty start list means the default");
    assert_eq!(alarm_snaps(Some("{\"start\":[10],\"end\":[]}")).start, vec![10]);
}

#[test]
fn until_forms() {
    let tz: chrono_tz::Tz = "Europe/Berlin".parse().unwrap();
    assert_eq!(normalize_until("FREQ=DAILY;UNTIL=20260930", false, tz), "FREQ=DAILY;UNTIL=20260930T215959Z");
    assert_eq!(normalize_until("FREQ=DAILY;UNTIL=20260930T215959Z", true, tz), "FREQ=DAILY;UNTIL=20260930");
    assert_eq!(rrule_until_local("FREQ=DAILY;UNTIL=20260930T215959Z", tz), NaiveDate::from_ymd_opt(2026, 9, 30));
    // multi-byte rule text: no panic, no date
    assert_eq!(rrule_until_local("FREQ=DAILY;UNTIL=2026103€", tz), None);
    assert_eq!(rrule_until_local("FREQ=DAILY;UNTIL=20€", tz), None);
    assert!(date_lines("EXDATE", &["2026-10-2€x".into(), "ääääää".into(), "+10000-01-01".into()], true, tz).is_empty());
}

/// Both expansions of a series — from the stored master, and from its light copy (`light_master`, taken for
/// a heavy master) — give the same instances: the 60 real series, overrides of both ranges, a VTIMEZONE of
/// a non-Olson name, alarms on the master and on an override.
#[test]
fn light_expansion_equals_the_full_one() {
    #[derive(serde::Deserialize)]
    struct Fx {
        ics: String,
    }
    let mut files: Vec<String> = serde_json::from_str::<Vec<Fx>>(include_str!("../../../tests/fixtures/recurrence-real.json")).unwrap().into_iter().map(|f| f.ics).collect();
    let note = format!("DESCRIPTION:{}\r\nX-ALT-DESC;FMTTYPE=text/html:<p>x</p>\r\nCATEGORIES:A,B\r\nLOCATION:Ort\r\nCOLOR:tomato\r\nSTATUS:CANCELLED\r\n", "Notiz ".repeat(500));
    let master = |extra: &str| format!("BEGIN:VEVENT\r\nUID:s\r\nDTSTAMP:20260901T000000Z\r\nSUMMARY:Serie\r\nDTSTART;TZID=Europe/Berlin:20261001T090000\r\nDTEND;TZID=Europe/Berlin:20261001T100000\r\nRRULE:FREQ=DAILY;COUNT=40\r\nEXDATE;TZID=Europe/Berlin:20261005T090000\r\n{note}BEGIN:VALARM\r\nACTION:DISPLAY\r\nDESCRIPTION:x\r\nTRIGGER;RELATED=END:-PT15M\r\nEND:VALARM\r\n{extra}END:VEVENT\r\n");
    let over = |rid: &str, range: &str, summary: &str| format!("BEGIN:VEVENT\r\nUID:s\r\nDTSTAMP:20260901T000000Z\r\nRECURRENCE-ID;TZID=Europe/Berlin{range}:{rid}\r\nSUMMARY:{summary}\r\nDTSTART;TZID=Europe/Berlin:{rid}\r\nDTEND;TZID=Europe/Berlin:20261020T120000\r\nDESCRIPTION:eigene Notiz\r\nBEGIN:VALARM\r\nACTION:DISPLAY\r\nDESCRIPTION:x\r\nTRIGGER:-PT1H\r\nEND:VALARM\r\nEND:VEVENT\r\n");
    let cal = |body: String| format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//t//EN\r\n{}\r\n{body}END:VCALENDAR\r\n", ical::vtimezone("Europe/Berlin").unwrap());
    files.push(cal(master("")));
    files.push(cal(master("") + &over("20261010T090000", "", "Ausnahme")));
    files.push(cal(master("") + &over("20261010T090000", ";RANGE=THISANDFUTURE", "Ab hier anders")));
    files.push(cal(master("TRANSP:TRANSPARENT\r\n").replace("Europe/Berlin", "W. Europe Standard Time")).replace("BEGIN:VCALENDAR\r\n", "BEGIN:VCALENDAR\r\n").replacen(&ical::vtimezone("Europe/Berlin").unwrap(), &ical::vtimezone("Europe/Berlin").unwrap().replace("TZID:Europe/Berlin", "TZID:W. Europe Standard Time"), 1));
    let tz: chrono_tz::Tz = "Europe/Berlin".parse().unwrap();
    let ci = CalInfo { principal: "p".into(), owner_label: String::new(), is_group: false, id: "c".into(), displayname: String::new(), color: "#000".into(), description: String::new(), components: vec!["VEVENT".into()], timezone_id: String::new(), subscription_url: None, deleted_at: None, synctoken: 0, read_only: false, managed: false, ..CalInfo::bare(crate::state::Authorized::test_own("p", "c")) };
    let (start, end) = (NaiveDate::from_ymd_opt(2026, 9, 1).unwrap(), NaiveDate::from_ymd_opt(2026, 12, 1).unwrap());
    let ws = ical::to_utc(start.and_time(chrono::NaiveTime::MIN), tz) - Duration::days(1);
    let we = ical::to_utc(end.and_time(chrono::NaiveTime::MIN), tz) + Duration::days(1);
    let win = (start.and_time(chrono::NaiveTime::MIN), end.and_time(chrono::NaiveTime::MIN));
    let shown = |v: &[Instance]| v.iter().map(|i| format!("{} {} {} {} {} {:?} {:?} {} {} {} {}", i.start, i.end, i.all_day, i.summary, i.description.len(), i.categories, i.alarms, i.location, i.color, i.cancelled, i.uid)).collect::<Vec<_>>();
    let mut light_taken = 0;
    for f in &files {
        let obj = CalendarObject::from_ics(f.clone()).unwrap_or_else(|e| panic!("{e}: {f}"));
        let (mut full, mut light) = (vec![], vec![]);
        expand_object(&obj, &ci, "o", tz, (ws, we), win, usize::MAX, &mut full);
        expand_object(&obj, &ci, "o", tz, (ws, we), win, 0, &mut light);
        assert_eq!(shown(&light), shown(&full), "{f}");
        light_taken += usize::from(light_master(&obj.get_ics()).is_some());
    }
    assert_eq!(light_taken, files.len(), "every light copy parsed");
    assert!(files.len() > 60);
}
