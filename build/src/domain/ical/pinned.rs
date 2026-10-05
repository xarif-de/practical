//! Characterisation of the iCalendar text helpers.

use super::*;

#[test]
fn text_values() {
    assert_eq!(unescape(r"a\,b\;c\\d\ne\Nf\x\"), "a,b;c\\d\ne\nfx");
    assert_eq!(escape("a,b;c\\d\r\ne"), r"a\,b\;c\\d\ne");
    assert_eq!(split_list(""), Vec::<String>::new());
    assert_eq!(split_list(r"a\\,b"), ["a\\", "b"]);
    assert_eq!(split_list(" x , y\\,z ,"), ["x", "y,z"]);
}

#[test]
fn folding_at_75_octets() {
    assert_eq!(fold(&"a".repeat(75)), "a".repeat(75));
    assert_eq!(fold(&"a".repeat(76)), format!("{}\r\n a", "a".repeat(75)));
    let f = fold(&"b".repeat(75 + 74 + 1));
    assert_eq!(f.split("\r\n").map(str::len).collect::<Vec<_>>(), [75, 75, 2], "continuations carry 74 octets plus the space");
    // a multi-byte character is never split
    let f = fold(&format!("{}€", "c".repeat(74)));
    assert_eq!(f, format!("{}\r\n €", "c".repeat(74)));
}

#[test]
fn lines_and_zones() {
    assert_eq!(split_line("item1.tel;type=\"a:b;c\";X=1:+49:1"), ("TEL".to_string(), vec![("TYPE".to_string(), "a:b;c".to_string()), ("X".into(), "1".into())], "+49:1".to_string()));
    assert_eq!(split_line("SUMMARY"), ("SUMMARY".to_string(), vec![], String::new()));
    assert_eq!(split_line("X;NOVALUE:v").1, Vec::<(String, String)>::new(), "a parameter without '=' is dropped");
    assert_eq!(fmt_dt(chrono::NaiveDate::from_ymd_opt(2026, 1, 2).unwrap().and_hms_opt(3, 4, 5).unwrap()), "20260102T030405");
    assert_eq!(fmt_date(chrono::NaiveDate::from_ymd_opt(2026, 1, 2).unwrap()), "20260102");
    assert_eq!(fmt_utc_now().len(), 16);
    assert!(parse_tz("Europe/Berlin").is_some() && parse_tz("Mars/Base").is_none() && parse_tz("").is_none());
    let v = vtimezone("Europe/Berlin").unwrap();
    assert!(v.starts_with("BEGIN:VTIMEZONE\r\nTZID:Europe/Berlin\r\n") && v.ends_with("END:VTIMEZONE") && !v.contains("VCALENDAR"));
    assert!(!v.replace("\r\n", "").contains('\n'), "CRLF only");
    assert!(vtimezone("Mars/Base").is_none());
    let tz: chrono_tz::Tz = "Europe/Berlin".parse().unwrap();
    let at = |s: &str| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M").unwrap();
    assert_eq!(to_utc(at("2026-07-01T12:00"), tz).to_rfc3339(), "2026-07-01T10:00:00+00:00");
    assert_eq!(to_utc(at("2026-03-29T02:30"), tz).to_rfc3339(), "2026-03-29T02:30:00+00:00", "a time in the spring gap is read as UTC");
    assert_eq!(to_utc(at("2026-10-25T02:30"), tz).to_rfc3339(), "2026-10-25T00:30:00+00:00", "the autumn overlap takes the first (summer) mapping");
}

#[test]
fn local_times_and_component_reads() {
    let tz: chrono_tz::Tz = "Europe/Berlin".parse().unwrap();
    let obj = rustical_ical::CalendarObject::from_ics(crate::testkit::vevent("u", "S\\, x", "DTSTART;TZID=America/New_York:20261014T090000\nDTEND:20261014T150000Z\nCATEGORIES:A\nCATEGORIES:B\\,C,D\nLOCATION:")).unwrap();
    let caldata::component::CalendarInnerData::Event(ev, _) = obj.get_inner().get_inner() else { panic!() };
    assert_eq!(local(&ev.dtstart.0, tz), (chrono::NaiveDate::from_ymd_opt(2026, 10, 14).unwrap().and_hms_opt(15, 0, 0).unwrap(), false));
    assert_eq!(local_date(&ev.dtend.as_ref().unwrap().0, tz), chrono::NaiveDate::from_ymd_opt(2026, 10, 14).unwrap());
    assert_eq!(text(ev, "SUMMARY").as_deref(), Some("S, x"));
    assert_eq!(text(ev, "LOCATION"), None, "an empty value is none");
    assert_eq!(list(ev, "CATEGORIES"), ["A", "B,C", "D"]);
    assert_eq!(text_line(ev.get_property("SUMMARY").unwrap()), "S, x");
    if let caldata::types::CalDateOrDateTime::DateTime(d) = &ev.dtstart.0 {
        assert_eq!(tz_of(&d.timezone()).map(|t| t.name().to_string()).as_deref(), Some("America/New_York"));
    }
}
