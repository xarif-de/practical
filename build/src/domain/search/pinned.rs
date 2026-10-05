//! Characterisation of the top-bar search and the editor's suggestions over the synthetic world
//! (it::seed), with "today" fixed: the whole answer of each query is a golden file.

use super::*;
use crate::testkit::date;

fn tz() -> chrono_tz::Tz {
    "Europe/Berlin".parse().unwrap()
}

#[test]
fn date_parts_table() {
    for (q, parts) in [
        ("2027", Some((Some(2027), None, None))),
        ("märz", Some((None, Some(3), None))),
        ("mar", Some((None, Some(3), None))),
        ("ma", None),
        ("décembre 2026", Some((Some(2026), Some(12), None))),
        ("15.9.", Some((None, Some(9), Some(15)))),
        ("15.09.26", Some((Some(2026), Some(9), Some(15)))),
        ("2026-09-15", Some((Some(2026), Some(9), Some(15)))),
        ("3 mai", Some((None, Some(5), Some(3)))),
        ("40", None),
        ("", None),
        ("  ", None),
        ("zahnarzt 3", None),
    ] {
        assert_eq!(date_parts(q), parts.map(|(y, m, d)| (y, m, d)), "{q:?}");
    }
    let today = date("2026-10-14");
    let j = |q: &str| parse_date_query(q, today).map(|j| (j.date.to_string(), j.label, j.view));
    assert_eq!(j("31.2."), None, "no such day");
    assert_eq!(j("15"), Some(("2026-10-15".into(), "15.10.2026".into(), "day")));
    assert_eq!(j("oktober"), Some(("2026-10-01".into(), "Oktober 2026".into(), "month")));
    assert_eq!(j("may 2027"), Some(("2027-05-01".into(), "Mai 2027".into(), "month")), "the label is German whatever the query's language");
    assert_eq!(j("1999"), Some(("1999-01-01".into(), "1999".into(), "planner")));
    assert_eq!(fold("ÆØÅ Œuvre Łódź Řeka ß"), "aeoa oeuvre lodz reka ss");
    assert_eq!(split_scope("  Aufgaben:Steuer"), (Some("task"), "Steuer"));
    assert_eq!(split_scope("Événements: x"), (Some("event"), " x"));
    assert_eq!(split_scope("foo: x"), (None, "foo: x"));
}

#[tokio::test]
async fn answers_over_the_seed() {
    let s = crate::it::seed::seed().await;
    let alice = s.app.principal("alice").await;
    let ids = [(&s.arbeit, "<arbeit>"), (&s.privat, "<privat>"), (&s.aufgaben, "<aufgaben>"), (&s.familie, "<familie>"), (&s.freunde, "<freunde>")];
    let mut out = String::new();
    for (lang, q, limit) in [
        ("de", "zahn", 5),
        ("de", "zahnartz", 5),
        ("de", "quartal review", 5),
        ("de", "raum 4.12", 5),
        ("de", "anna", 5),
        ("de", "kontakt: anna", 5),
        ("de", "termin: anna", 5),
        ("de", "steuer", 5),
        ("de", "familie", 5),
        ("de", "acme", 5),
        ("de", "2.4.", 5),
        ("de", "14.10.2026", 5),
        ("de", "standup", 1),
        ("de", "e", 200),
        ("en", "urlaub", 5),
        ("de", "x", 5),
    ] {
        let r = search(&s.app.state, &alice, q, date("2026-10-14"), tz(), lang, limit).await.unwrap();
        let mut json = serde_json::to_string_pretty(&r).unwrap();
        for (id, name) in ids {
            json = json.replace(id.as_str(), name);
        }
        out.push_str(&format!("=== {lang} {q:?} limit {limit}\n{json}\n"));
    }
    crate::testkit::assert_snapshot("search/answers.txt", &out);
    // the editor's templates: past occurrences only (now = the 20th), the most used title first
    let now = date("2026-10-20").and_hms_opt(12, 0, 0).unwrap();
    let sug = suggest(&s.app.state, &alice, "st", now, tz(), "de", 3).await.unwrap();
    assert_eq!(sug.iter().map(|x| (x.summary.as_str(), x.count, x.last.to_string(), x.time.as_str())).collect::<Vec<_>>(), [("Standup", 6, "2026-10-14".to_string(), "09:00")]);
    let sug = suggest(&s.app.state, &alice, "zahn", now, tz(), "de", 3).await.unwrap();
    assert_eq!(sug.iter().map(|x| x.summary.as_str()).collect::<Vec<_>>(), ["Zahnarzt Dr. Weiß"]);
    assert!(suggest(&s.app.state, &alice, "urlaub", now, tz(), "de", 3).await.unwrap().is_empty(), "only in the future");
    assert!(suggest(&s.app.state, &alice, "abgesagter", now, tz(), "de", 3).await.unwrap().is_empty(), "cancelled");
    assert!(suggest(&s.app.state, &alice, "z", now, tz(), "de", 3).await.unwrap().is_empty());
    // carol finds nothing of alice's
    let carol = s.app.principal("carol").await;
    let r = search(&s.app.state, &carol, "zahn", date("2026-10-14"), tz(), "de", 5).await.unwrap();
    assert_eq!((r.totals.events, r.totals.contacts, r.totals.tasks), (0, 0, 0));
}
