//! Characterisation of the change history's text layer: devices, labels, readable values.

use super::*;

const EV: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Mozilla.org/NONSGML Mozilla Calendar V1.1//EN\r\nBEGIN:VEVENT\r\nUID:u1\r\nDTSTAMP:20260901T100000Z\r\nSUMMARY:Probe\r\nDTSTART;TZID=Europe/Berlin:20261014T190000\r\nDTEND;TZID=Europe/Berlin:20261014T210000\r\nRRULE:FREQ=WEEKLY;INTERVAL=2;BYDAY=TU,TH;UNTIL=20261231T225959Z\r\nLOCATION:Aula\r\nSTATUS:TENTATIVE\r\nCLASS:PRIVATE\r\nPRIORITY:1\r\nCATEGORIES:Chor,Musik\r\nATTENDEE;CN=Anna;PARTSTAT=ACCEPTED;ROLE=OPT-PARTICIPANT:mailto:anna@example.org\r\nORGANIZER:mailto:leitung@example.org\r\nGEO:51.05;13.74\r\nX-MOZ-GENERATION:3\r\nATTACH;ENCODING=BASE64;VALUE=BINARY:aGVsbG8=\r\nBEGIN:VALARM\r\nACTION:DISPLAY\r\nTRIGGER:-PT90M\r\nEND:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

#[test]
fn devices() {
    for (prodid, dev) in [
        ("-//Apple Inc.//iOS 18.0//EN", Some("Apple iOS")),
        ("-//Apple Inc.//macOS 15.0//EN", Some("Apple macOS")),
        ("-//Apple Inc.//Something//EN", Some("Apple")),
        ("+//IDN bitfire.at//DAVx5/4.4 ical4j/3.2//EN", Some("DAVx⁵")),
        ("-//Mozilla.org/NONSGML Mozilla Calendar V1.1//EN", Some("Thunderbird")),
        ("-//xarif.de//PractiCal//DE", Some("PractiCal")),
        ("-//xarif.de//rustical-ui//DE", Some("PractiCal")),
        ("-//Example Corp//Planner 7//EN", Some("Example Corp Planner 7")),
        ("-//x//y//EN", None),
        ("", None),
    ] {
        assert_eq!(device(Some(prodid)).as_deref(), dev, "{prodid}");
    }
    assert_eq!(device(None), None);
}

#[test]
fn reading_an_object() {
    let a = attrs(Store::Cal, EV);
    // UID is the object's identity, not an attribute
    assert_eq!(a.keys().map(String::as_str).collect::<Vec<_>>(), ["ATTACH", "ATTENDEE", "CATEGORIES", "CLASS", "DTEND", "DTSTAMP", "DTSTART", "GEO", "LOCATION", "ORGANIZER", "PRIORITY", "RRULE", "STATUS", "SUMMARY", "VALARM", "X-MOZ-GENERATION"]);
    assert_eq!(a["ATTACH"], ";ENCODING=BASE64;VALUE=BINARY:…", "binary payloads are shortened");
    assert_eq!((title(Store::Cal, EV), uid(EV), prodid(EV), kind(Store::Cal, EV)), ("Probe".to_string(), Some("u1".to_string()), Some("-//Mozilla.org/NONSGML Mozilla Calendar V1.1//EN".to_string()), "event"));
    assert!(is_tech("X-MOZ-GENERATION") && is_tech("X-APPLE-X") && is_tech("REV") && !is_tech("SUMMARY") && !is_tech("X-FOO"));
    assert_eq!(split_value(";ALTREP=\"https://a:b\";X=1:wert:mit"), (";ALTREP=\"https://a:b\";X=1", "wert:mit"));
    assert_eq!(split_value("ohne"), ("ohne", ""));
    assert_eq!(param(";CN=\"Anna B\";ROLE=CHAIR", "cn"), Some("Anna B"));
    assert_eq!(param(";CN=x", "ROLE"), None);
    assert_eq!(Store::from_tag("ab"), Store::Ab);
    assert_eq!(Store::from_tag("anything"), Store::Cal);
}

#[test]
fn values_read_in_each_language() {
    let i18n = crate::i18n::I18n::load();
    let mut out = String::new();
    let a = attrs(Store::Cal, EV);
    for lang in ["de", "en"] {
        let t = |k: &str| i18n.lookup(lang, k);
        let show = Show { t: &t, lang, tz: "Europe/Berlin".parse().unwrap() };
        let colls = |k: &str| format!("<{k}>");
        for (k, v) in &a {
            out.push_str(&format!("{lang} {} = {}\n", show.label(k), show.value_in(k, v, &colls, &["Probe"]).replace('\n', " / ")));
        }
        out.push_str(&format!("{lang} {} = {}\n", show.label(COLL_KEY), show.value(COLL_KEY, "alice/cal", &colls)));
        out.push_str(&format!("{lang} {}\n", show.label("RECURRENCE-ID:20261021T170000Z")));
        out.push_str(&format!("{lang} raw = {}\n", show.raw(":a\\,b\n;X=1:c")));
    }
    crate::testkit::assert_snapshot("history_text/values.txt", &out);
    // an attribute without a label (any X- property) reads as its own name, never as a "⟦key⟧" placeholder
    assert!(out.contains("de X-MOZ-GENERATION = 3") && !out.contains('⟦'), "{out}");
}
