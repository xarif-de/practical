//! Characterisation of every generator: golden files in tests/snapshots/render/ plus the exact
//! strings of the small helpers, in several languages.

use super::*;

use crate::testkit::{assert_snapshot, date, instance, lang, lines_of_html, marks};

/// October 2026 with everything a month cell can hold: overlapping multi-day bars in lanes, a bar
/// across a week break, five singles on one day ("+N"), a cancelled entry, a coloured one, public
/// holidays and a school block.
fn october() -> (Vec<Instance>, Hols) {
    let mut v = vec![
        instance("a", "messe", "Messe", "2026-10-20", "2026-10-23"),
        instance("a", "reise", "Reise", "2026-10-21", "2026-10-27"),
        instance("b", "kurz", "Kurztrip", "2026-10-22", "2026-10-23"),
        instance("a", "nacht", "Nachtschicht", "2026-10-09T22:00", "2026-10-10T06:00"),
        instance("a", "s1", "Eins", "2026-10-19T08:00", "2026-10-19T09:00"),
        instance("a", "s2", "Zwei", "2026-10-19T09:00", "2026-10-19T10:00"),
        instance("b", "s3", "Drei", "2026-10-19T10:00", "2026-10-19T11:00"),
        instance("b", "s4", "Vier <&>", "2026-10-19T11:00", "2026-10-19T12:00"),
        instance("a", "s5", "Fünf \"q\"", "2026-10-19T12:00", "2026-10-19T12:00"),
        instance("a", "review", "Review", "2026-10-14T14:00", "2026-10-14T15:30"),
    ];
    let mut c = instance("a", "abgesagt", "Abgesagt", "2026-10-15T10:00", "2026-10-15T11:00");
    c.cancelled = true;
    v.push(c);
    let mut t = instance("b", "tomato", "Zahnarzt", "2026-10-16T08:30", "2026-10-16T09:15");
    t.color = "tomato".into();
    t.location = "Praxis, Hauptstr. 1".into();
    t.alarms = vec![15, 1440];
    t.categories = vec!["Gesundheit".into()];
    t.recurring = true;
    v.push(t);
    let hol = marks(&[
        ("2026-10-03", "2026-10-03", "Tag der Deutschen Einheit", false, &[]),
        ("2026-10-31", "2026-10-31", "Reformationstag", false, &["SN"]),
        ("2026-10-05", "2026-10-17", "Herbstferien", true, &[]),
        ("2026-10-26", "2026-10-31", "Herbstferien", true, &["BY"]),
    ]);
    (v, hol)
}

#[test]
fn month_grid_golden() {
    let (v, hol) = october();
    let html = month_grid(2026, 10, date("2026-10-14"), &v, &lang("de"), 3, date("2026-10-16"), &hol);
    assert_snapshot("render/month_grid_de.html", &lines_of_html(&html));
    // the picked day is the selection only when it is not today
    let same = month_grid(2026, 10, date("2026-10-14"), &v, &lang("de"), 3, date("2026-10-14"), &hol);
    assert!(!same.contains(" sel\"") && same.contains("class=\"day today fer\""));
    // "+N": three slots, the bar lanes count against them; when not all fit, the "+N" takes the last one (never "+1"),
    // the singles past it are there but hidden (`.xt`, unfolded in place: no link to the day view)
    let d19 = html.split("data-date=\"2026-10-19\">").nth(1).and_then(|r| r.split("<div class=\"day").next()).unwrap_or_default();
    assert!(d19.ends_with("<button class=\"dmore\" type=\"button\" aria-expanded=\"false\">+3 weitere</button></div>"), "{d19}");
    assert_eq!((d19.matches("<div class=\"ev").count(), d19.matches("<div class=\"ev xt\"").count()), (5, 3), "{d19}");
    assert!(html.contains("<div class=\"month\" data-plus=\"weitere\">"));
    let en = month_grid(2026, 10, date("2026-10-14"), &v, &lang("en"), 3, date("2026-10-16"), &hol);
    assert!(en.contains("+3 more") && en.contains("<div class=\"dow\">Mon</div>"));
}

/// The "+N" guess with no room left by three bar lanes: a lone single is shown (a "+1" would stand in for nothing),
/// two give "+2" with both hidden.
#[test]
fn month_grid_plus_guess_without_room() {
    let mut v = vec![
        instance("a", "b1", "Bar 1", "2026-10-05", "2026-10-08"),
        instance("a", "b2", "Bar 2", "2026-10-05", "2026-10-08"),
        instance("a", "b3", "Bar 3", "2026-10-05", "2026-10-08"),
        instance("a", "one", "Einzeln", "2026-10-06T09:00", "2026-10-06T10:00"),
    ];
    let tile = |html: &str, d: &str| html.split(&format!("data-date=\"{d}\">")).nth(1).and_then(|r| r.split("<div class=\"day").next()).unwrap_or_default().to_string();
    let html = month_grid(2026, 10, date("2026-10-14"), &v, &lang("de"), 3, date("2026-10-14"), &marks(&[]));
    let d6 = tile(&html, "2026-10-06");
    assert!(d6.contains("title=\"Einzeln\"") && !d6.contains(" xt") && !d6.contains("dmore"), "{d6}");
    v.push(instance("a", "two", "Zweitens", "2026-10-06T11:00", "2026-10-06T12:00"));
    let html = month_grid(2026, 10, date("2026-10-14"), &v, &lang("de"), 3, date("2026-10-14"), &marks(&[]));
    let d6 = tile(&html, "2026-10-06");
    assert_eq!(d6.matches("<div class=\"ev xt\"").count(), 2, "{d6}");
    assert!(d6.contains(">+2 weitere</button>"), "{d6}");
}

#[test]
fn month_view_and_agenda_golden() {
    let (v, hol) = october();
    let l = lang("de");
    assert_snapshot("render/month_view_de.html", &lines_of_html(&month_view(date("2026-10-14"), date("2026-10-14"), date("2026-10-14"), &v, &l, date("2026-10-14"), &hol)));
    // the agenda title says "Heute" only on today
    let other = month_view(date("2026-10-14"), date("2026-10-19"), date("2026-10-14"), &v, &l, date("2026-10-19"), &hol);
    assert!(other.contains("<h2>Montag, 19. Oktober</h2>"), "{other}");
    let today = month_view(date("2026-10-14"), date("2026-10-14"), date("2026-10-14"), &v, &l, date("2026-10-14"), &hol);
    assert!(today.contains("<h2>Heute, Mittwoch, 14. Oktober</h2>"), "{today}");
    let empty = agenda(date("2026-10-01"), &v, &l, "T", &hol);
    assert_eq!(empty, "<section class=\"agenda\"><h2>T</h2><ul><li><time></time><span class=\"muted\">Keine Termine</span></li></ul></section>");
    let a = agenda(date("2026-10-31"), &v, &lang("en"), "Sat", &hol);
    assert_eq!(a, "<section class=\"agenda\"><h2>Sat</h2><div class=\"hday\">Reformationstag (SN)</div><div class=\"hday school\">Herbstferien (BY)</div><ul><li><time></time><span class=\"muted\">No events</span></li></ul></section>");
}

#[test]
fn week_view_and_blocks_golden() {
    let (v, hol) = october();
    let l = lang("de");
    assert_snapshot("render/week_view_de.html", &lines_of_html(&week_view(date("2026-10-21"), date("2026-10-14"), &v, &l, &hol)));
    assert_eq!(week_window(date("2026-10-21")), (date("2026-09-21"), date("2026-11-30")));
    let one = week_blocks(date("2026-10-15"), 1, date("2026-10-14"), &v, &l, &hol);
    assert_snapshot("render/week_blocks_de.html", &lines_of_html(&one));
    assert!(one.starts_with("<div class=\"week now\" data-week=\"2026-10-12\"><h2 class=\"wk\">KW 42 <span>12.10. – 18.10.</span> <b class=\"fname\">Herbstferien</b></h2>"), "{one}");
    assert_eq!(week_blocks(date("2026-10-15"), 3, date("2026-10-14"), &v, &l, &hol).matches("<div class=\"week").count(), 3);
    // a multi-day event's bar stands on its first day of the week (app.js reads the day block's date there); the
    // later days carry its line only
    let w = week_blocks(date("2026-10-19"), 1, date("2026-10-14"), &v, &l, &hol);
    assert!(w.contains("<section class=\"dayblock\" data-date=\"2026-10-20\" style=\"--ln:1\"><div class=\"dl\"><b>20</b><span>Di</span></div><ul><li class=\"mday run\" style=\"--k:0\"><time>ganztags</time><span class=\"ev allday\" style=\"--c:#0a84ff\" data-key=\"alice/a/messe\">Messe</span>"), "{w}");
    assert_eq!(w.matches("data-key=\"alice/a/messe\"").count(), 1, "{w}");
    assert!(w.contains("data-date=\"2026-10-21\" style=\"--ln:2\"><div class=\"dl\"><b>21</b><span>Mi</span></div><ul><li class=\"vh\">Messe</li><li class=\"mday run\" style=\"--k:1\">") && w.contains("</ul><i class=\"ln end\" style=\"--k:0;--c:#0a84ff\"></i><i class=\"ln\" style=\"--k:1;"), "21 and 22 October: {w}");
    assert!(w.contains("data-key=\"alice/a/s1\">Eins</span>"), "{w}");
}

#[test]
fn day_view_and_rows_golden() {
    let (v, hol) = october();
    let l = lang("de");
    let mut details = HashMap::new();
    let d = EventDetail {
        principal: "alice".into(), cal_id: "b".into(), object_id: "tomato".into(), uid: "tomato".into(), summary: "Zahnarzt".into(),
        location: "Praxis, Hauptstr. 1".into(), description: String::new(), start: v[11].start, end: v[11].end, all_day: false,
        rrule: Some("FREQ=MONTHLY;BYDAY=3FR;COUNT=4".into()), rrule_until: None, exdates: vec![], rdates: vec![], organizer: None, attendees: vec![],
        alarms: vec![], categories: vec!["Gesundheit".into()], status: "TENTATIVE".into(), transp: "TRANSPARENT".into(), class: "CONFIDENTIAL".into(),
        priority: Some(2), color: "tomato".into(), geo: Some((51.0504, 13.7373)),
        links: vec![
            crate::domain::events::Link { kind: "conference".into(), uri: "https://meet.example.org/abc".into(), label: "Videocall".into(), ..Default::default() },
            crate::domain::events::Link { kind: "url".into(), uri: "https://www.example.org/a/very/long/path/that/goes/on/and/on/and/on".into(), ..Default::default() },
            crate::domain::events::Link { kind: "attach".into(), embedded: true, filename: "befund.pdf".into(), bin_index: Some(0), ..Default::default() },
        ],
        related: vec![], etag: String::new(), has_overrides: false, timezone: "Europe/Berlin".into(),
    };
    details.insert("alice/b/tomato".to_string(), d);
    assert_snapshot("render/day_view_de.html", &lines_of_html(&day_view(date("2026-10-16"), date("2026-10-14"), &v, &l, "Arbeit, Privat", &details, &hol)));
    // without details the facts come from the instance alone
    let plain = day_view(date("2026-10-16"), date("2026-10-14"), &v, &l, "Arbeit", &HashMap::new(), &hol);
    assert!(plain.contains("<span>Serie</span>") && plain.contains("<span>15 Minuten vorher</span><span>1 Tag vorher</span>"), "{plain}");
    // the count is a Fluent plural: one event is "1 Termin"
    assert!(plain.contains("<div class=\"sub\">1 Termin · Arbeit</div>"), "{plain}");
    let empty = day_view(date("2026-10-02"), date("2026-10-14"), &v, &lang("en"), "x", &HashMap::new(), &hol);
    assert!(empty.contains("<p class=\"muted\">No events on this day.</p>") && empty.contains("<div class=\"sub\">0 events · x</div>"), "{empty}");
    assert_eq!(day_window(date("2026-10-16")), (date("2026-09-16"), date("2026-11-16")));
    let rows = day_rows(date("2026-10-02"), 3, date("2026-10-03"), Some(date("2026-10-04")), &v, &l, &hol);
    assert_eq!(rows, "<a class=\"lrow\" href=\"/calendar?view=day&date=2026-10-02\" data-date=\"2026-10-02\"><b>02</b><span>Fr</span><i style=\"--n:0\"></i><em></em><button class=\"add\" type=\"button\" title=\"+\" aria-label=\"Neuen Termin anlegen\">+</button></a><a class=\"lrow today we hol\" href=\"/calendar?view=day&date=2026-10-03\" data-date=\"2026-10-03\"><b>03</b><span>Sa</span><i style=\"--n:0\"></i><em>Tag der Deutschen Einheit</em><button class=\"add\" type=\"button\" title=\"+\" aria-label=\"Neuen Termin anlegen\">+</button></a><a class=\"lrow sel we\" href=\"/calendar?view=day&date=2026-10-04\" data-date=\"2026-10-04\"><b>04</b><span>So</span><i style=\"--n:0\"></i><em></em><button class=\"add\" type=\"button\" title=\"+\" aria-label=\"Neuen Termin anlegen\">+</button></a>");
    let rows = day_rows(date("2026-10-05"), 2, date("2026-10-14"), None, &v, &l, &hol);
    assert!(rows.contains("<em>Herbstferien</em>") && rows.matches("Herbstferien").count() == 1, "a school block is named on its first day only: {rows}");
}

#[test]
fn picker_golden() {
    let l = lang("de");
    let html = picker(2026, 10, &[0, 3, 0, 1, 0, 0, 7, 0, 0, 2, 0, 9], &l);
    assert_snapshot("render/picker_de.html", &lines_of_html(&html));
    assert!(html.contains("<g class=\"ynode cur\" data-year=\"2026\">") && html.contains("data-year=\"2034\"") && html.contains("data-year=\"2018\""));
    // no density at all: every month node the smallest
    let flat = picker(2026, 1, &[0; 12], &l);
    assert_eq!(flat.matches("r=\"14\"").count(), 11);
    assert_eq!(tl_weeks(date("2026-10-14"), date("2026-10-01")), "<div class=\"tl\" data-kind=\"weeks\" data-center=\"2026-10-14\" data-today=\"2026-10-01\"></div>");
}

#[test]
fn rrule_sentences() {
    let start = date("2026-10-14");
    for (l, rule, text) in [
        ("de", "FREQ=DAILY", "Täglich"),
        ("de", "FREQ=WEEKLY", "Wöchentlich"),
        ("de", "FREQ=WEEKLY;BYDAY=MO,WE,FR", "Wöchentlich am Montag, Mittwoch, Freitag"),
        ("de", "FREQ=WEEKLY;INTERVAL=2;BYDAY=TU", "Alle 2 Wochen am Dienstag"),
        ("de", "FREQ=DAILY;INTERVAL=3;COUNT=5", "Alle 3 Tage, 5 Termine"),
        ("de", "FREQ=MONTHLY;BYDAY=2TH", "Monatlich am 2. Donnerstag"),
        ("de", "FREQ=MONTHLY;BYDAY=-1FR", "Monatlich am letzten Freitag"),
        ("de", "FREQ=MONTHLY;BYMONTHDAY=15;UNTIL=20271231T225959Z", "Monatlich am 15., bis 31.12.2027"),
        ("de", "FREQ=YEARLY", "Jährlich am 14. Oktober"),
        ("de", "FREQ=YEARLY;BYDAY=1SU;BYMONTH=5", "Jährlich am 1. Sonntag im Mai"),
        ("de", "FREQ=YEARLY;INTERVAL=4", "Alle 4 Jahre am 14. Oktober"),
        ("de", "FREQ=MONTHLY;INTERVAL=6", "Alle 6 Monate"),
        // RFC 5545 rule parts are case-insensitive: a rule another client wrote in lower case reads the same
        ("de", "freq=weekly;byday=sa", "Wöchentlich am Samstag"),
        ("de", "RRULE:Freq=Monthly;ByDay=-1fr;Count=1", "Monatlich am letzten Freitag, 1 Termin"),
        ("en", "FREQ=DAILY;COUNT=1", "Daily, once"),
        ("de", "FREQ=HOURLY", "Serie"),
        ("de", "FREQ=MONTHLY;BYDAY=MO,TU;BYSETPOS=1", "Monatlich"),
        ("de", "", "Serie"),
        ("en", "FREQ=WEEKLY;BYDAY=MO,WE;COUNT=10", "Weekly on Monday, Wednesday, 10 times"),
        ("en", "FREQ=MONTHLY;BYDAY=-1FR", "Monthly on the last Friday"),
        ("en", "FREQ=YEARLY;UNTIL=20300101", "Yearly on October 14, until 2030-01-01"),
        ("fr", "FREQ=WEEKLY;INTERVAL=2", "Toutes les 2 semaines"),
        ("es", "FREQ=DAILY", "Cada día"),
    ] {
        assert_eq!(rrule_text(rule, start, &lang(l)), text, "{l} {rule}");
    }
}

#[test]
fn small_helpers() {
    let de = lang("de");
    for (m, text) in [(0, "zum Termin"), (15, "15 Minuten vorher"), (60, "1 Stunde vorher"), (120, "2 Stunden vorher"), (1440, "1 Tag vorher"), (2880, "2 Tage vorher"), (90, "90 Minuten vorher"), (-30, "30 Minuten danach"), (-1440, "1 Tag danach")] {
        assert_eq!(alarm_label(m, &de), text, "{m}");
    }
    assert_eq!(alarm_label(1, &lang("en")), "1 minute before");
    assert_eq!(short_uri("https://example.org/"), "example.org");
    assert_eq!(short_uri("mailto:x@example.org"), "mailto:x@example.org");
    assert_eq!(short_uri("https://www.example.org/a/very/long/path/that/goes/on/and/on"), "www.example.org/a/very/long/path/that/goe…");
    assert_eq!(e("<a href=\"x\">'&'</a>"), "&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt;");
    assert_eq!(ico("pin"), format!("<svg class=\"ico\" aria-hidden=\"true\"><use href=\"{}#i-pin\"/></svg>", crate::routes::assets::icons()));
    assert_eq!(monday_of(date("2026-10-18")), date("2026-10-12"));
    assert_eq!(monday_of(date("2026-10-12")), date("2026-10-12"));
    // every month: 4 to 6 rows, Monday first, the 1st in the first row
    for y in [2026, 2027, 2028] {
        for m in 1..=12 {
            let w = month_weeks(y, m);
            assert!((4..=6).contains(&w.len()), "{y}-{m}");
            assert!(w.iter().all(|r| r[0].weekday() == chrono::Weekday::Mon));
            assert!(w[0].contains(&NaiveDate::from_ymd_opt(y, m, 1).unwrap()));
        }
    }
    assert_eq!(month_weeks(2027, 2).len(), 4, "February 2027 starts on a Monday and has 28 days");
    assert_eq!(month_weeks(2026, 3).len(), 6);
    // L: dates in each language
    let d = date("2026-03-05");
    assert_eq!((de.long_date(d), de.dm(d), de.date(d), de.week_title(d), de.week_short(10)), ("Donnerstag, 5. März".to_string(), "5.3.".to_string(), "05.03.2026".to_string(), "KW 10 2026".to_string(), "KW 10".to_string()));
    let en = lang("en");
    assert_eq!((en.long_date(d), en.dm(d), en.date(d), en.mon3(3)), ("Thursday, March 5".to_string(), "3/5".to_string(), "2026-03-05".to_string(), "Mar".to_string()));
    // by_day: every touched day, an end at midnight is exclusive
    let v = vec![instance("a", "x", "X", "2026-10-09T22:00", "2026-10-11T00:00"), instance("a", "y", "Y", "2026-10-09T10:00", "2026-10-09T10:00")];
    let idx = by_day(&v, date("2026-01-01"), date("2027-01-01"));
    assert!(by_day(&v, date("2026-10-10"), date("2026-10-11")).keys().eq([&date("2026-10-10")]), "only the window's days");
    let mut days: Vec<NaiveDate> = idx.keys().copied().collect();
    days.sort();
    assert_eq!(days, [date("2026-10-09"), date("2026-10-10")]);
    assert_eq!(idx[&date("2026-10-09")].len(), 2);
    // hol_class / hol_labels
    let hol = marks(&[("2026-10-03", "2026-10-03", "Einheit", false, &[]), ("2026-10-03", "2026-10-04", "Ferien", true, &["SN"])]);
    assert_eq!(hol_class(&hol, date("2026-10-03")), " hol fer");
    assert_eq!(hol_class(&hol, date("2026-10-04")), " fer");
    assert_eq!(hol_class(&hol, date("2026-10-05")), "");
    assert_eq!(hol_labels(&hol, date("2026-10-03"), "span", true), "<span class=\"hday\">Einheit</span><span class=\"hday school\">Ferien (SN)</span>");
    assert_eq!(hol_labels(&hol, date("2026-10-03"), "span", false), "<span class=\"hday\">Einheit</span>");
}

#[test]
fn event_chip_classes() {
    let ev_html = |i: &Instance, role: Option<&str>| {
        let mut out = String::new();
        super::ev_html(&mut out, i, role);
        out
    };
    let mut i = instance("a", "x", "Titel", "2026-10-01T09:00", "2026-10-01T10:00");
    assert_eq!(ev_html(&i, None), "<div class=\"ev\" style=\"--c:#0a84ff\" data-key=\"alice/a/x\" data-date=\"2026-10-01\" title=\"Titel\">Titel 09:00</div>");
    i.cancelled = true;
    i.color = "\"><script>".into();
    assert_eq!(ev_html(&i, Some("mid")), "<div class=\"ev span mid cancelled\" style=\"--c:&quot;&gt;&lt;script&gt;\" data-key=\"alice/a/x\" data-date=\"2026-10-01\" title=\"Titel\">Titel 09:00</div>");
    let a = instance("a", "y", "Ganztägig", "2026-10-01", "2026-10-02");
    assert_eq!(ev_html(&a, None), "<div class=\"ev allday\" style=\"--c:#0a84ff\" data-key=\"alice/a/y\" data-date=\"2026-10-01\" title=\"Ganztägig\">Ganztägig</div>");
}

#[test]
fn escape_on_write_equals_e() {
    for s in ["", "plain", "a&b<c>d\"e'f", "&&<<>>", "äöü ✓ \u{1F600}<'>", "'", "x&"] {
        assert_eq!(super::E(s).to_string(), e(s), "{s:?}");
    }
    let mut i = instance("a", "x", "T", "2026-10-01T09:00", "2026-10-01T10:00");
    i.principal = "group.a&b".into();
    i.object_id = "<id>'".into();
    assert_eq!(super::EKey(&i).to_string(), e(&i.key()));
}
