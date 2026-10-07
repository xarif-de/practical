use super::*;

#[test]
fn escaping_roundtrip() {
    let s = "a, b; c\\d\nline2";
    assert_eq!(unescape(&escape(s)), s);
    assert_eq!(split_list("Reise\\, privat,Arbeit, ,Urlaub"), vec!["Reise, privat", "Arbeit", "Urlaub"]);
}

#[test]
fn folding() {
    let long = "DESCRIPTION:".to_string() + &"ä".repeat(100);
    let folded = fold(&long);
    for l in folded.split("\r\n") {
        assert!(l.len() <= 75, "{}", l.len());
    }
    assert_eq!(folded.replace("\r\n ", ""), long);
}

/// The same language as caldata's DURATION regex where caldata answers at all, bounded where it panicked.
#[test]
fn bounded_durations() {
    for v in ["P", "PT", "-P", "+PT0S", "P1W", "-P2W", "P1D", "P1DT", "PT1H", "PT15M", "-PT30S", "P1DT2H3M4S", "PT1H4S", "P15DT5H0M20S", "P146000D", "-PT210000000M"] {
        assert_eq!(parse_duration(v), caldata::types::parse_duration(v).ok(), "{v}");
    }
    for v in ["", "1D", "P1", "PT1", "P1W1D", "PT1M1H", "P1H", "PT1D", "P1DT2", "P-1D", " P1D", "P1D ", "PT1H30", "PxD"] {
        assert_eq!((parse_duration(v), caldata::types::parse_duration(v).is_err()), (None, true), "{v}");
    }
    // caldata panics on these: an i64 overflow, a TimeDelta past its range, non-ASCII digits
    for v in ["-PT99999999999999999999M", "-PT1000000000000000M", "P99999999999W", "-PT٣M", "P1000000000D", "P999999999D", "-PT999999999M"] {
        assert_eq!(parse_duration(v), None, "{v}");
    }
    assert_eq!(parse_duration("P20000W"), Some(chrono::Duration::weeks(20000)), "383 years");
    assert_eq!(parse_duration("P30000W"), None, "575 years");
}

#[test]
fn control_characters_never_reach_a_line() {
    assert_eq!(fold("ATTENDEE:mailto:x@y.de\r\nRRULE:FREQ=SECONDLY"), "ATTENDEE:mailto:x@y.deRRULE:FREQ=SECONDLY");
    assert_eq!(fold("SUMMARY:a\u{0}b\u{1b}[31m\u{7f}c\td"), "SUMMARY:ab[31mc\td", "HTAB is the one control a line may hold");
    let long = format!("DESCRIPTION:{}\n{}", "x".repeat(80), "y".repeat(80));
    assert!(!fold(&long).replace("\r\n ", "").contains('\n'));
}

#[test]
fn parameters_are_quoted_and_cleaned() {
    assert_eq!(param("X-Q", "a:b;c"), ";X-Q=\"a:b;c\"");
    assert_eq!(param("CN", "Doe, John"), ";CN=\"Doe, John\"");
    assert_eq!(param("CN", "Max \"M\" Muster"), ";CN=Max M Muster");
    assert_eq!(param("ROLE", "CHAIR"), ";ROLE=CHAIR");
    assert_eq!(param("X-B", "v\r\nRRULE:FREQ=SECONDLY"), ";X-B=\"vRRULE:FREQ=SECONDLY\"");
    assert_eq!(param("DELEGATED-FROM", "mailto:a@x.org,mailto:b@x.org"), ";DELEGATED-FROM=\"mailto:a@x.org\",\"mailto:b@x.org\"");
    assert_eq!(param("X-A\r\nRRULE", "1"), "", "a name that is no token is dropped");
    assert_eq!(param("", "1"), "");
    // what split_line reads back is what was written
    let line = format!("URL{}{}:https://example.org/", param("X-Q", "a:b;c"), param("LABEL", "x"));
    assert_eq!(split_line(&line), ("URL".into(), vec![("X-Q".into(), "a:b;c".into()), ("LABEL".into(), "x".into())], "https://example.org/".into()));
}

#[test]
fn multi_byte_input_is_never_cut_inside_a_character() {
    assert_eq!(rrule_canon("aäää"), "Aäää");
    assert_eq!(rrule_canon("rrulÄ:x"), "RRULÄ:X");
    assert_eq!(rrule_canon("RRULE:freq=daily"), "FREQ=DAILY");
    assert!(year_ok(NaiveDate::from_ymd_opt(9999, 12, 31).unwrap()) && !year_ok(NaiveDate::from_ymd_opt(10000, 1, 1).unwrap()) && !year_ok(NaiveDate::from_ymd_opt(0, 12, 31).unwrap()));
    assert!(!write_date_ok(NaiveDate::from_ymd_opt(9999, 12, 31).unwrap()) && write_date_ok(NaiveDate::from_ymd_opt(9999, 12, 30).unwrap()));
}
