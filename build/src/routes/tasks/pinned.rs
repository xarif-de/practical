//! The task page's choices: lookback windows, the completion order, the calendar filter.

use super::*;
use crate::testkit::date;

fn task(summary: &str, completed: Option<&str>, due: Option<&str>) -> Task {
    let at = |s: &str| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M").unwrap();
    Task { principal: "p".into(), cal_id: "c".into(), cal_name: String::new(), color: String::new(), object_id: summary.into(), uid: summary.into(), summary: summary.into(), description: String::new(), location: String::new(), start: None, start_all_day: false, due: due.map(at), due_all_day: false, status: "COMPLETED".into(), percent: None, completed_at: completed.map(at), priority: 0, class: String::new(), categories: vec![], alarms: 0, links: 0, created: None, extra: Default::default() }
}

#[test]
fn lookbacks_and_order() {
    assert_eq!(LOOKBACKS.iter().map(|l| lookback_days(l)).collect::<Vec<_>>(), [Some(1), Some(7), Some(31), Some(92), Some(365), None]);
    assert_eq!(lookback_days("bogus"), None);
    let today = date("2026-10-14");
    let t = task("a", Some("2026-10-13T23:00"), None);
    assert!(in_lookback(&t, today, Some(1)) && !in_lookback(&task("b", Some("2026-10-12T10:00"), None), today, Some(1)));
    assert!(in_lookback(&task("c", None, Some("2026-10-07T00:00")), today, Some(7)), "no stamp: the due date counts");
    assert!(!in_lookback(&task("d", None, None), today, Some(365)) && in_lookback(&task("d", None, None), today, None));
    assert!(in_lookback(&task("e", Some("2026-11-01T00:00"), None), today, Some(1)), "a future stamp is inside");
    let mut v = vec![task("b", Some("2026-10-01T10:00"), None), task("a", Some("2026-10-01T10:00"), None), task("z", None, Some("2026-10-05T00:00")), task("n", None, None), task("y", Some("2026-09-01T10:00"), None)];
    v.sort_by(by_completion);
    assert_eq!(v.iter().map(|t| t.summary.as_str()).collect::<Vec<_>>(), ["z", "a", "b", "y", "n"], "newest first, ties by title, no date last");
    assert_eq!(one_of(Some(&"board".to_string()), VIEWS, "groups"), "board");
    assert_eq!(one_of(Some(&"Board".to_string()), VIEWS, "groups"), "groups");
    assert_eq!(one_of(None, VIEWS, "groups"), "groups");
}

#[test]
fn calendar_filter() {
    let cal = |p: &str, id: &str, comps: &[&str]| CalInfo { principal: p.into(), owner_label: String::new(), is_group: false, id: id.into(), displayname: id.into(), color: String::new(), description: String::new(), components: comps.iter().map(|c| c.to_string()).collect(), timezone_id: String::new(), subscription_url: None, deleted_at: None, synctoken: 0, read_only: false };
    let all = vec![cal("a", "1", &["VEVENT"]), cal("a", "2", &["VTODO"]), cal("g", "3", &["VEVENT", "VTODO"])];
    let ids = |v: Vec<CalInfo>| v.into_iter().map(|c| c.id).collect::<Vec<_>>();
    assert_eq!(ids(pick_calendars(&all, None)), ["2", "3"]);
    assert_eq!(ids(pick_calendars(&all, Some(""))), ["2", "3"]);
    assert_eq!(ids(pick_calendars(&all, Some("g/3,a/1"))), ["3"], "a calendar without VTODO stays out");
    assert!(pick_calendars(&all, Some("x/9")).is_empty());
}
