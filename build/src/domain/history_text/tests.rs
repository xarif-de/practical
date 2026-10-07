use super::*;

const EV: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Apple Inc.//iOS 26.0//EN\r\nBEGIN:VEVENT\r\nUID:u1\r\nDTSTAMP:20260901T100000Z\r\nSEQUENCE:2\r\nSUMMARY:Zahnreinigung\r\nDTSTART:20261006T080000Z\r\nDESCRIPTION:Bonusheft\r\nATTENDEE;CN=Anna:mailto:a@x\r\nATTENDEE;CN=Ben:mailto:b@x\r\nBEGIN:VALARM\r\nACTION:DISPLAY\r\nTRIGGER:-PT1H\r\nEND:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

fn edit(s: &str, from: &str, to: &str) -> String {
    assert!(s.contains(from), "{from}");
    s.replace(from, to)
}

#[test]
fn attributes_group_lines_and_order_does_not_count() {
    let a = attrs(Store::Cal, EV);
    assert_eq!(a["SUMMARY"], ":Zahnreinigung");
    assert_eq!(a["ATTENDEE"].lines().count(), 2);
    assert!(a["VALARM"].contains("TRIGGER:-PT1H"));
    let swapped = edit(EV, "ATTENDEE;CN=Anna:mailto:a@x\r\nATTENDEE;CN=Ben:mailto:b@x", "ATTENDEE;CN=Ben:mailto:b@x\r\nATTENDEE;CN=Anna:mailto:a@x");
    assert!(diff(&a, &attrs(Store::Cal, &swapped)).is_empty());
}

#[test]
fn a_dismissed_reminder_is_technical() {
    let acked = edit(EV, "TRIGGER:-PT1H\r\n", "TRIGGER:-PT1H\r\nACKNOWLEDGED:20261006T090000Z\r\n");
    let acked = edit(&acked, "DTSTAMP:20260901T100000Z", "DTSTAMP:20261006T090000Z");
    let d = diff(&attrs(Store::Cal, EV), &attrs(Store::Cal, &acked));
    assert_eq!(d.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), ["ACKNOWLEDGED", "DTSTAMP"]);
    assert!(d.iter().all(|x| is_tech(&x.0)));
}

#[test]
fn revert_puts_back_only_the_named_attributes() {
    // before → after changed the note; later the title changed too; undo the note only
    let after = edit(EV, "DESCRIPTION:Bonusheft", "DESCRIPTION:Bonusheft + Karte");
    let current = edit(&after, "SUMMARY:Zahnreinigung", "SUMMARY:Zahnarzt");
    let out = revert(Store::Cal, &current, EV, &["DESCRIPTION".into()]);
    let a = attrs(Store::Cal, &out);
    assert_eq!(a["DESCRIPTION"], ":Bonusheft");
    assert_eq!(a["SUMMARY"], ":Zahnarzt");
    assert_eq!(a["SEQUENCE"], ":3");
    assert!(rustical_ical::CalendarObject::import(&out, None).is_ok(), "{out}");
}

#[test]
fn revert_restores_removed_attributes_and_alarms() {
    let after = edit(EV, "DESCRIPTION:Bonusheft\r\n", "");
    let after = edit(&after, "BEGIN:VALARM\r\nACTION:DISPLAY\r\nTRIGGER:-PT1H\r\nEND:VALARM\r\n", "");
    let out = revert(Store::Cal, &after, EV, &["DESCRIPTION".into(), "VALARM".into()]);
    let a = attrs(Store::Cal, &out);
    assert_eq!(a["DESCRIPTION"], ":Bonusheft");
    assert!(a["VALARM"].contains("TRIGGER:-PT1H"));
    assert!(rustical_ical::CalendarObject::import(&out, None).is_ok(), "{out}");
}

#[test]
fn vcards_revert_by_name_without_group() {
    let v = "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:c1\r\nFN:Jana Weber\r\nitem1.TEL;TYPE=cell:+49 170 1\r\nNOTE:alt\r\nEND:VCARD\r\n";
    let after = edit(v, "item1.TEL;TYPE=cell:+49 170 1", "item1.TEL;TYPE=cell:+49 160 2");
    let current = edit(&after, "NOTE:alt", "NOTE:neu");
    let a = attrs(Store::Ab, &after);
    assert_eq!(a["TEL"], ";TYPE=cell:+49 160 2");
    let out = revert(Store::Ab, &current, v, &["TEL".into()]);
    assert!(out.contains(".TEL;TYPE=cell:+49 170 1") && out.contains("NOTE:neu") && out.contains("REV:"), "{out}");
}

#[test]
fn values_read_like_the_editor() {
    let t = |k: &str| Some(match k {
        "dow-2" => "Di".to_string(),
        "hx-unit-h" => "Std.".into(),
        "hx-al-before" => "%n vorher".into(),
        "hx-rr-weekly" => "wöchentlich".into(),
        "hx-rr-until" => "bis %date".into(),
        "dow-3" => "Mi".into(),
        "hx-v-action-display" => "Anzeige".into(),
        "hx-v-accepted" => "zugesagt".into(),
        _ => return None,
    });
    let s = Show { t: &t, lang: "de", tz: chrono_tz::Europe::Berlin };
    let none = |x: &str| x.to_string();
    assert_eq!(s.value("DTSTART", ";TZID=Europe/Berlin:20261006T100000", &none), "Di 6.10.2026 10:00");
    assert_eq!(s.value("DTSTART", ":20261006T080000Z", &none), "Di 6.10.2026 10:00");
    assert_eq!(s.value("VALARM", "BEGIN:VALARM\nACTION:DISPLAY\nTRIGGER:-PT1H\nEND:VALARM", &none), "1 Std. vorher · Anzeige");
    assert_eq!(s.value("RRULE", ":FREQ=WEEKLY;BYDAY=WE;UNTIL=20261216T225900Z", &none), "wöchentlich, Mi, bis Mi 16.12.2026 23:59");
    assert_eq!(s.value("ATTENDEE", ";CN=Anna:mailto:a@x\n;CN=Ben;PARTSTAT=ACCEPTED:mailto:b@x", &none), "Anna <a@x>\nBen <b@x> (zugesagt)");
    assert_eq!(device(Some("-//Apple Inc.//iOS 26.0//EN")).as_deref(), Some("Apple iOS"));
    assert_eq!(device(Some("-//Acme Corp//Planner 3//EN")).as_deref(), Some("Acme Corp Planner 3"));
}

/// Every attribute a version can differ in reads differently before and after: each property
/// the full demo event carries is edited once (alarm kind, trigger, attendee answer, …) and the
/// changed attribute must show both a label of its own and two different readable values.
#[test]
fn every_edit_of_the_full_event_reads_differently() {
    let i18n = crate::i18n::I18n::load();
    let t = |k: &str| i18n.lookup("de", k);
    let show = Show { t: &t, lang: "de", tz: chrono_tz::Europe::Berlin };
    let none = |x: &str| x.to_string();
    let base = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/demo-event-full.ics")).unwrap().replace("\r\n", "\n");
    let a = attrs(Store::Cal, &base);
    // every attribute has a label of its own (technical ones included, X- ones of other apps may not)
    for k in a.keys() {
        let l = show.label(k);
        assert!(l != *k || k.starts_with("X-") || k.starts_with("RECURRENCE-ID:"), "no label for {k}");
        assert!(!show.value(k, &a[k], &none).is_empty() || a[k].is_empty(), "no value for {k}");
    }
    let edits: &[(&str, &str)] = &[
        ("ACTION:DISPLAY\nTRIGGER:-PT15M", "ACTION:AUDIO\nTRIGGER:-PT15M"),
        ("ACTION:DISPLAY\nTRIGGER:-PT15M", "ACTION:EMAIL\nTRIGGER:-PT15M"),
        ("TRIGGER:-PT15M", "TRIGGER:-PT20M"),
        ("REPEAT:2", "REPEAT:3"),
        ("PROXIMITY:DEPART", "PROXIMITY:ARRIVE"),
        ("PARTSTAT=ACCEPTED;RSVP=TRUE;CUTYPE=INDIVIDUAL", "PARTSTAT=DECLINED;RSVP=TRUE;CUTYPE=INDIVIDUAL"),
        ("ROLE=OPT-PARTICIPANT", "ROLE=REQ-PARTICIPANT"),
        ("LABEL=Videocall", "LABEL=Telefon"),
        ("RELTYPE=PARENT", "RELTYPE=SIBLING"),
        ("GEO:51.050407;13.737262", "GEO:51.1;13.8"),
        ("EXDATE;TZID=Europe/Berlin:20261006T190000", "EXDATE;TZID=Europe/Berlin:20261013T190000"),
        ("RESOURCES:Beamer,Flipchart", "RESOURCES:Beamer"),
        ("CONTACT:Kasse", "CONTACT:Abendkasse"),
        ("URL:https://www.kulturpalast", "URL:https://www.semperoper"),
        ("ATTACH:https://example.org/programm.pdf", "ATTACH:https://example.org/programm-2.pdf"),
        ("COLOR:tomato", "COLOR:red"),
        ("CLASS:PRIVATE", "CLASS:CONFIDENTIAL"),
        ("TRANSP:OPAQUE", "TRANSP:TRANSPARENT"),
        ("STATUS:CONFIRMED", "STATUS:CANCELLED"),
        ("PRIORITY:5", "PRIORITY:1"),
    ];
    for (from, to) in edits {
        assert!(base.contains(from), "fixture lacks {from:?}");
        let b = attrs(Store::Cal, &base.replacen(from, to, 1));
        let d: Vec<_> = diff(&a, &b).into_iter().filter(|(k, _, _)| !k.starts_with("X-OLD-")).collect();
        assert!(!d.is_empty(), "{from:?}: no difference found");
        for (k, x, y) in d {
            assert_ne!(show.value_in(&k, &x, &none, &["Demo"]), show.value_in(&k, &y, &none, &["Demo"]), "{from:?} → {to:?}: {k} reads the same");
        }
    }
}

#[test]
fn every_edit_of_a_contact_reads_differently() {
    let i18n = crate::i18n::I18n::load();
    let t = |k: &str| i18n.lookup("de", k);
    let show = Show { t: &t, lang: "de", tz: chrono_tz::Europe::Berlin };
    let none = |x: &str| x.to_string();
    let v = "BEGIN:VCARD\nVERSION:3.0\nUID:c1\nN:Weber;Jana;;Dr.;\nFN:Dr. Jana Weber\nitem1.TEL;TYPE=CELL:+49 170 1\nitem1.X-ABLabel:Dienst\nEMAIL;TYPE=HOME:jana@example.org\nADR;TYPE=HOME:;;Lindenweg 7;Zwickau;;08056;DE\nBDAY:1988-03-14\nitem2.X-ABDATE:2010-06-01\nitem2.X-ABLabel:_$!<Anniversary>!$_\nRELATED;TYPE=spouse:Max\nIMPP;X-SERVICE-TYPE=Signal:signal:+49170\nORG:Praxis;Empfang\nNOTE:alt\nEND:VCARD\n";
    let a = attrs(Store::Ab, v);
    for k in a.keys() {
        assert_ne!(show.label(k), *k, "no label for {k}");
    }
    assert_eq!(show.value("TEL", &a["TEL"], &none), "+49 170 1 (Dienst, mobil)");
    let edits: &[(&str, &str)] = &[
        ("TYPE=CELL:+49 170 1", "TYPE=WORK:+49 170 1"),
        ("X-ABLabel:Dienst", "X-ABLabel:Privat"),
        ("TYPE=HOME:jana", "TYPE=WORK:jana"),
        ("Lindenweg 7", "Lindenweg 9"),
        ("N:Weber;Jana;;Dr.;", "N:Weber;Jana;;Prof.;"),
        ("BDAY:1988-03-14", "BDAY:1988-03-15"),
        ("X-ABDATE:2010-06-01", "X-ABDATE:2011-06-01"),
        ("TYPE=spouse", "TYPE=child"),
        ("X-SERVICE-TYPE=Signal", "X-SERVICE-TYPE=Threema"),
        ("ORG:Praxis;Empfang", "ORG:Praxis;Labor"),
    ];
    for (from, to) in edits {
        assert!(v.contains(from), "fixture lacks {from:?}");
        let b = attrs(Store::Ab, &v.replacen(from, to, 1));
        let d = diff(&a, &b);
        assert!(!d.is_empty(), "{from:?}: no difference found");
        for (k, x, y) in d {
            assert_ne!(show.value(&k, &x, &none), show.value(&k, &y, &none), "{from:?} → {to:?}: {k} reads the same");
        }
    }
    // the label travels with its number when that is undone
    let after = v.replace("item1.TEL;TYPE=CELL:+49 170 1\nitem1.X-ABLabel:Dienst\n", "");
    let back = revert(Store::Ab, &after, v, &["TEL".into()]);
    assert_eq!(attrs(Store::Ab, &back)["TEL"], a["TEL"], "{back}");
}
