//! The five "more" views on a fixed year (today injected): golden files plus the numbers they show.
use super::*;
use crate::testkit::{assert_snapshot, date, instance, lang, lines_of_html};

fn hol(name: &str, start: &str, end: &str, tags: &[&str]) -> Holiday {
    Holiday { start: date(start), end: date(end), name: name.into(), kind: "public".into(), nationwide: tags.is_empty(), source: "test".into(), regions: vec![], tags: tags.iter().map(|t| t.to_string()).collect() }
}

fn person(uid: &str, name: &str, bday: &str) -> Contact {
    Contact { uid: uid.into(), full_name: name.into(), birthday: bday.into(), ..Default::default() }
}

fn year(today: &str) -> YearData {
    let mut instances = vec![
        instance("a", "standup", "Standup", "2026-10-14T09:00", "2026-10-14T09:15"),
        instance("a", "abend", "Theater", "2026-10-16T19:30", "2026-10-16T22:00"),
        instance("a", "sa", "Radtour", "2026-10-17T10:00", "2026-10-17T16:00"),
        instance("b", "so", "Brunch", "2026-10-18T11:00", "2026-10-18T13:00"),
        instance("b", "silvester", "Silvester", "2026-12-31T20:00", "2027-01-01T02:00"),
        instance("a", "alt", "Vorjahr", "2025-12-31T10:00", "2025-12-31T11:00"),
    ];
    let urlaub = instance("b", "urlaub", "Urlaub Ostsee", "2026-10-26", "2026-11-02");
    let sommer = instance("b", "sommer", "Urlaub Alpen", "2026-07-06", "2026-07-18");
    instances.push(urlaub.clone());
    instances.push(sommer.clone());
    let public = vec![hol("Neujahr", "2026-01-01", "2026-01-01", &[]), hol("Christi Himmelfahrt", "2026-05-14", "2026-05-14", &[]), hol("Tag der Deutschen Einheit", "2026-10-03", "2026-10-03", &[]), hol("Reformationstag", "2026-10-31", "2026-10-31", &["SN"]), hol("Buß- und Bettag", "2026-11-18", "2026-11-18", &["SN"]), hol("1. Weihnachtsfeiertag", "2026-12-25", "2026-12-25", &[])];
    let mut school = vec![hol("Herbstferien", "2026-10-05", "2026-10-17", &[])];
    school[0].kind = "school".into();
    YearData {
        year: 2026,
        today: date(today),
        own: public.clone(),
        public,
        school,
        vacations: vec![sommer, urlaub],
        birthdays: vec![person("anna", "Anna Schmidt", "1985-10-20"), person("ben", "Ben", "--10-02"), person("leap", "Lea Schaltjahr", "--02-29"), person("bad", "Ohne Datum", "irgendwann")],
        vacation_days: Some(30),
        vacation_rule: "Kategorie „Urlaub“".into(),
        school_regions: "DE-SN".into(),
        instances,
    }
}

#[test]
fn planner_golden() {
    let html = planner(&year("2026-10-14"), &lang("de"));
    assert_snapshot("more/planner_de.html", &lines_of_html(&html));
    assert_eq!(html.matches("<div class=\"mcol\">").count(), 12);
    assert_eq!(html.matches("<div class=\"c x\"></div>").count(), 31 * 12 - 365, "the days a month lacks");
    // a holiday names the cell before a vacation, a vacation before an event
    assert!(html.contains("<div class=\"c we hol\" data-date=\"2026-10-31\" title=\"Reformationstag (SN)\">"), "{html}");
    assert!(html.contains("data-date=\"2026-10-27\" title=\"Urlaub Ostsee\"><b>27</b><s>D</s><span class=\"u\">Urlaub Ostsee</span>"), "{html}");
    assert!(html.contains("<div class=\"c t fer\"") || html.contains("<div class=\"c fer t\""), "today is marked");
}

#[test]
fn weekends_golden() {
    let y = year("2026-10-14");
    let html = weekends(&y, &lang("de"));
    assert_snapshot("more/weekends_de.html", &lines_of_html(&html));
    assert_eq!(html.matches("<div class=\"we ").count(), 52, "Saturdays of 2026");
    // 17./18.10. is busy (two titles), the next free one after today is the 24th
    assert!(html.contains("<div class=\"we busy\" data-date=\"2026-10-17\"><b>17.10.</b><small>KW 42</small><span>Radtour, Brunch</span></div>"), "{html}");
    assert!(html.contains("<div class=\"we free next\" data-date=\"2026-10-24\">"), "{html}");
    // a holiday on the Friday before (25.12.) or the Monday after makes it long; one ON the Saturday does not
    assert!(html.contains("<div class=\"we free long\" data-date=\"2026-12-26\">"), "{html}");
    assert!(html.contains("<div class=\"we free past\" data-date=\"2026-10-03\">"), "{html}");
    assert!(html.starts_with("<div class=\"kpi\"><div><b>49</b><span>freie Wochenenden</span></div><div><b>3</b><span>verplant</span></div><div><b>1</b><span>verlängert durch Feiertag</span></div></div>"), "{html}");
}

#[test]
fn vacation_golden() {
    let y = year("2026-10-14");
    let html = vacation(&y, &lang("de"));
    assert_snapshot("more/vacation_de.html", &lines_of_html(&html));
    // summer: 06.07.–17.07. = 10 workdays; autumn: 26.10.–01.11. = 5 (the 31st is a Saturday anyway)
    assert!(html.contains("<div class=\"kpi\"><div><b>15</b><span>Urlaubstage geplant</span></div><div><b>15</b><span>Resturlaub von 30</span></div><div><b>2</b><span>Reisen</span></div></div>"), "{html}");
    // bridge days: Himmelfahrt (Thursday) lies before today → skipped; none after today in 2026
    assert!(html.contains("Kategorie „Urlaub“"));
    let early = vacation(&year("2026-01-02"), &lang("de"));
    assert!(early.contains("Christi Himmelfahrt"), "before the day the bridge day is offered: {early}");
    let none = vacation(&YearData { own: vec![], ..year("2026-10-14") }, &lang("en"));
    assert!(none.contains("<li class=\"muted\">No bridge days this year.</li>"));
    let unknown = vacation(&YearData { vacation_days: None, ..year("2026-10-14") }, &lang("de"));
    assert!(unknown.contains("<b>–</b><span>Resturlaub von ?</span>"), "{unknown}");
}

#[test]
fn countdown_golden() {
    let y = year("2026-10-14");
    let html = countdown(&y, &lang("de"));
    assert_snapshot("more/countdown_de.html", &lines_of_html(&html));
    // Ben's birthday was 12 days ago: in "just passed", and next year's in the coming list
    assert!(html.contains("<b>−12</b>"), "{html}");
    assert!(html.contains("Anna Schmidt · 41"), "the age it turns: {html}");
    // BUG?: a 29 February birthday in a non-leap year falls back to the 28th of THIS year; when that
    // is past, next year has no 29 February either and the tile counts negative days ("−228").
    assert!(html.contains("<b>-228</b>"), "{html}");
}

#[test]
fn numbers_golden() {
    let y = year("2026-10-14");
    let html = numbers(&y, &lang("de"));
    assert_snapshot("more/numbers_de.html", &lines_of_html(&html));
    // instances of 2026 only: 7 of the 8 (the 2025 one is out)
    assert!(html.starts_with("<div class=\"kpis\"><div><b>7</b>"), "{html}");
    // BUG?: the spark bars get their arguments shifted — `--h` receives the week label and the
    // title the percentage and the count ("--h:KW 1%", title "0: 0").
    assert!(html.contains("<i style=\"--h:KW 1%\" title=\"0: 0\"></i>"), "{html}");
}

#[test]
fn helpers() {
    let y = year("2026-10-14");
    assert!(in_hol(&y.public, date("2026-10-03")).is_some() && in_hol(&y.public, date("2026-10-04")).is_none());
    assert!(!workday(date("2026-10-03"), &y.public), "a Saturday");
    assert!(!workday(date("2026-12-25"), &y.public), "a holiday");
    assert!(workday(date("2026-10-14"), &y.public));
}
