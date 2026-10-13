use super::*;

#[test]
fn script_json_cannot_close_its_block() {
    let j = script_json(serde_json::json!({ "title": "</script><script>alert(1)</script>" }).to_string());
    assert!(!j.contains('<'));
    let back: serde_json::Value = serde_json::from_str(&j).unwrap();
    assert_eq!(back["title"], "</script><script>alert(1)</script>");
}

#[test]
fn script_uris_are_no_hrefs() {
    for ok in ["https://example.org/a?b=1&c=2", "http://x", "mailto:a@b.de", "tel:+4912345", "geo:50.1,8.6", "zoommtg://zoom.us/join?confno=1", "sip:a@b", "relative/path", "", "Javascript-guide.html"] {
        assert!(safe_href(ok), "{ok}");
    }
    for bad in ["javascript:alert(1)", "JavaScript:alert(1)", " javascript:alert(1)", "\u{1}javascript:x", "java\tscript:x", "jav\nascript:x", "vbscript:msgbox(1)", "data:text/html,<script>alert(1)</script>", "DATA:text/html;base64,PHNjcmlwdD4="] {
        assert!(!safe_href(bad), "{bad:?}");
    }
}

/// Every text of an event comes from any CalDAV client or a shared group calendar: each view
/// prints it as text, and a script URI among its links is no link.
#[test]
fn foreign_event_data_stays_text() {
    use crate::domain::events::Link;
    let x = "<img src=x onerror=alert(1)>\"'&";
    let d = NaiveDate::from_ymd_opt(2026, 9, 15).unwrap();
    let i = Instance {
        principal: "p".into(), cal_id: "c".into(), cal_name: x.into(), color: "#112233".into(), object_id: "o\"><xss>.ics".into(), uid: x.into(),
        summary: x.into(), location: x.into(), description: x.into(), start: d.and_hms_opt(9, 0, 0).unwrap(), end: d.and_hms_opt(10, 0, 0).unwrap(),
        all_day: false, recurring: false, categories: vec![x.into()], alarm_minutes: None, alarms: vec![], is_group: true, cancelled: false,
    };
    let link = |uri: &str| Link { kind: "url".into(), uri: uri.into(), label: x.into(), ..Default::default() };
    let det = EventDetail {
        principal: "p".into(), cal_id: "c".into(), object_id: i.object_id.clone(), uid: x.into(), summary: x.into(), location: x.into(), description: x.into(),
        start: i.start, end: i.end, all_day: false, rrule: None, rrule_until: None, exdates: vec![], rdates: vec![], organizer: None, attendees: vec![], alarms: vec![],
        categories: vec![x.into()], status: String::new(), transp: String::new(), class: String::new(), priority: None, color: String::new(), geo: None,
        links: vec![link("javascript:alert(1)"), link(" JAVA\tSCRIPT:alert(1)"), link("data:text/html,<script>alert(1)</script>"), link("https://example.org/?a=1&b=2")],
        related: vec![], etag: String::new(), has_overrides: false, timezone: String::new(),
    };
    let l = L::new("de", crate::i18n::I18n::load());
    let details = HashMap::from([(i.key(), det)]);
    let insts = [i];
    for html in [month_view(d, d, d, &insts, &l, d, &HashMap::new()), week_view(d, d, &insts, &l, &HashMap::new()), day_view(d, d, &insts, &l, x, &details, &HashMap::new())] {
        assert!(!html.contains("<img") && !html.contains("<xss") && !html.contains("<script"), "{html}");
        for (at, _) in html.match_indices("href=\"") {
            let v = &html[at + 6..];
            assert!(v.starts_with('/') || v.starts_with('#') || v.starts_with("https://example.org/"), "{}", &v[..v.len().min(40)]);
        }
    }
    let day = day_view(d, d, &insts, &l, x, &details, &HashMap::new());
    assert!(day.contains("&lt;img src=x onerror=alert(1)&gt;&quot;&#39;&amp;"));
    // the three script links are kept as rows without href, the web link stays one
    assert_eq!(day.matches("<a class=\"lk\" target=").count(), 3);
    assert!(day.contains("<a class=\"lk\" href=\"https://example.org/?a=1&amp;b=2\" target=\"_blank\""));
}

#[test]
fn weeks_of_september_2026() {
    let w = month_weeks(2026, 9);
    assert_eq!(w.len(), 5);
    assert_eq!(w[0][0], NaiveDate::from_ymd_opt(2026, 8, 31).unwrap());
    assert_eq!(w[4][6], NaiveDate::from_ymd_opt(2026, 10, 4).unwrap());
}

#[test]
fn same_named_blocks_share_one_label() {
    use crate::domain::holidays::Mark;
    let d = NaiveDate::from_ymd_opt(2026, 10, 5).unwrap();
    let m = |name: &str, tags: &[&str]| Mark { name: name.into(), tags: tags.iter().map(|t| t.to_string()).collect(), school: true, start: d };
    let v = [m("Herbstferien", &["HE"]), m("Herbstferien", &["SH"]), m("Herbstferien", &["RP", "SL"]), m("Brückentag", &["SN"])];
    assert_eq!(joined(v.iter()), ["Herbstferien (HE, SH, RP, SL)", "Brückentag (SN)"]);
    // one of them applies everywhere: the name alone
    let v = [m("Herbstferien", &["HE"]), m("Herbstferien", &[])];
    assert_eq!(joined(v.iter()), ["Herbstferien"]);
}

#[test]
fn month_names_a_school_block_on_its_first_day_that_is_no_public_holiday() {
    use crate::domain::holidays::Mark;
    let d = |m, day| NaiveDate::from_ymd_opt(2026, m, day).unwrap();
    let mut hol: Hols = HashMap::new();
    // Weihnachtsferien from 25.12., which is also a public holiday; the block goes on into January
    for day in 25..=31 {
        hol.entry(d(12, day)).or_default().push(Mark { name: "Weihnachtsferien".into(), tags: vec![], school: true, start: d(12, 25) });
    }
    hol.entry(d(12, 25)).or_default().push(Mark { name: "1. Weihnachtsfeiertag".into(), tags: vec![], school: false, start: d(12, 25) });
    let l = L::new("de", crate::i18n::I18n::load());
    let html = month_grid(2026, 12, d(9, 1), &[], &l, 3, d(12, 1), &hol);
    let tile = |day: u32| { let k = format!("data-date=\"{}\"", d(12, day)); let i = html.find(&k).unwrap(); html[i..].split("<div class=\"day").next().unwrap().to_string() };
    assert!(tile(25).contains("fname pub\"") && tile(25).contains(">1. Weihnachtsfeiertag<") && !tile(25).contains(">Weihnachtsferien<"));
    assert!(tile(26).contains("class=\"fname\"") && tile(26).contains(">Weihnachtsferien<"));
    assert!(!tile(27).contains("fname"));
    assert!(html.contains("day hol fer\" data-date=\"2026-12-25"), "a public holiday that is also a school-holiday day is both, `hol` wins in the CSS");
}

/// The week's multi-day lanes, as sketched for the feature: two bars on Monday (the longer one outermost), a third
/// on Wednesday left of both lines, a fourth on Saturday back in the free outer lane; it runs on into the next week,
/// whose Monday shows its bar again. Every multi-day bar stands once per week; the days its line passes carry it.
#[test]
fn week_draws_each_multi_day_bar_once_and_lines_in_lanes() {
    let day = |d: u32| NaiveDate::from_ymd_opt(2026, 11, d).unwrap();
    let ev = |s: &str, from: NaiveDate, to: NaiveDate, all_day: bool| Instance {
        principal: "p".into(), cal_id: "c".into(), cal_name: "".into(), color: "#112233".into(), object_id: format!("{s}.ics"), uid: s.into(),
        summary: s.into(), location: "".into(), description: "".into(), start: from.and_hms_opt(if all_day { 0 } else { 10 }, 0, 0).unwrap(),
        end: if all_day { to.and_hms_opt(0, 0, 0).unwrap() } else { to.and_hms_opt(11, 0, 0).unwrap() }, all_day, recurring: false, categories: vec![],
        alarm_minutes: None, alarms: vec![], is_group: false, cancelled: false,
    };
    let insts = [ev("M1", day(9), day(13), true), ev("M2", day(9), day(12), true), ev("A", day(9), day(9), false), ev("M3", day(11), day(14), true), ev("C", day(11), day(11), false), ev("M4", day(14), day(18), true)];
    let l = L::new("de", crate::i18n::I18n::load());
    let html = week_blocks(day(9), 2, day(1), &insts, &l, &HashMap::new());
    let block = |d: u32| {
        let at = html.find(&format!("data-date=\"2026-11-{d:02}\"")).unwrap();
        let s = &html[html[..at].rfind("<section").unwrap()..];
        s[..s.find("</section>").unwrap()].to_string()
    };
    for (s, n) in [("M1", 1), ("M2", 1), ("M3", 1), ("M4", 2)] {
        assert_eq!(html.matches(&format!(">{s}</span>")).count(), n, "{s}");
    }
    let bar = |d: u32, s: &str, k: usize| assert!(block(d).contains(&format!("<li class=\"mday run\" style=\"--k:{k}\"><time>")) && block(d).contains(&format!(">{s}</span>")), "{s} on {d}: {}", block(d));
    bar(9, "M1", 0);
    bar(9, "M2", 1);
    bar(11, "M3", 2);
    bar(14, "M4", 0);
    bar(16, "M4", 0);
    let lines = |d: u32| block(d).match_indices("<i class=\"ln").map(|(at, _)| block(d)[at..].split("\"></i>").next().unwrap().to_string()).collect::<Vec<_>>();
    let ln = |cls: &str, k: usize| format!("<i class=\"{cls}\" style=\"--k:{k};--c:#112233");
    assert_eq!(lines(9), Vec::<String>::new());
    assert_eq!(lines(10), [ln("ln", 0), ln("ln", 1)]);
    assert_eq!(lines(11), [ln("ln", 0), ln("ln end", 1)]);
    assert_eq!(lines(12), [ln("ln end", 0), ln("ln", 2)]);
    assert_eq!(lines(13), [ln("ln end", 2)]);
    assert_eq!(lines(15), [ln("ln end", 0)]);
    assert_eq!(lines(17), [ln("ln end", 0)]);
    // later days name the event for screen readers only (and the density panel counts it): no "–" beside a line
    assert!(block(10).contains("<li class=\"vh\">M1</li><li class=\"vh\">M2</li>") && block(12).contains("<li class=\"vh\">M3</li>"), "{}", block(10));
    assert!(!block(12).contains("class=\"none\""));
    // the day's other entries keep the lanes clear whose lines pass it
    for (d, n) in [(9, 2), (10, 2), (11, 3), (12, 3), (13, 3), (14, 1), (18, 0)] {
        assert_eq!(block(d).contains(&format!("style=\"--ln:{n}\"")), n > 0, "{d}: {}", block(d));
    }
}

/// A week crowded with overlapping multi-day events shares the outermost lane instead of squeezing its bars to nothing.
#[test]
fn week_lanes_stop_at_max_lane() {
    let day = |d: u32| NaiveDate::from_ymd_opt(2026, 11, d).unwrap();
    let ev = |n: usize| Instance {
        principal: "p".into(), cal_id: "c".into(), cal_name: "".into(), color: "#112233".into(), object_id: format!("e{n}.ics"), uid: format!("e{n}"),
        summary: format!("E{n}").into(), location: "".into(), description: "".into(), start: day(9).and_hms_opt(0, 0, 0).unwrap(), end: day(13).and_hms_opt(0, 0, 0).unwrap(),
        all_day: true, recurring: false, categories: vec![], alarm_minutes: None, alarms: vec![], is_group: false, cancelled: false,
    };
    let insts: Vec<Instance> = (0..8).map(ev).collect();
    let html = week_blocks(day(9), 1, day(1), &insts, &L::new("de", crate::i18n::I18n::load()), &HashMap::new());
    assert_eq!((0..=3).map(|k| html.matches(&format!("<li class=\"mday run\" style=\"--k:{k}\">")).count()).collect::<Vec<_>>(), [1, 1, 1, 5], "{html}");
    assert!(html.contains("style=\"--ln:4\"") && !html.contains("--ln:5"), "{html}");
}
