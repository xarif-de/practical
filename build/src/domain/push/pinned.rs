//! The reminder loop's fire set over a simulated day: which reminders go out in which minute, with
//! edits from devices and the UI in between. The loop computes its reminders only when the database
//! moved (`Plan`); a second app that computes them afresh every minute — the loop as it was — must
//! fire exactly the same, and both must fire the pinned list.

use super::*;
use crate::testkit::{TestApp, ics, vevent, vtodo};

const VALARM: &str = "BEGIN:VALARM\nACTION:DISPLAY\nDESCRIPTION:x\nTRIGGER";

fn alarm(trigger: &str) -> String {
    format!("{VALARM}{trigger}\nEND:VALARM")
}

async fn setup() -> TestApp {
    let app = TestApp::new().await;
    app.user("alice", "Alice").await;
    app.user("bob", "Bob").await;
    app.group("group.fam", "Familie", &["alice", "bob"]).await;
    prefs::set(&app.state.db, "bob", &prefs::Prefs { timezone: Some("Europe/London".into()), ..prefs::Prefs::defaults() }).await.unwrap();
    let plan = app.calendar("alice", "Plan", "#3478f6", &["VEVENT", "VTODO"]).await;
    let fam = app.calendar("group.fam", "Familie", "#af52de", &["VEVENT"]).await;
    let b = "DTSTART;TZID=Europe/Berlin:2026101";
    app.put_ics("alice", &plan, "e1", &vevent("e1", "Termin", &format!("{b}4T100000\nDTEND;TZID=Europe/Berlin:20261014T103000\n{}", alarm(":-PT15M")))).await;
    app.put_ics("alice", &plan, "e2", &vevent("e2", "Täglich", &format!("DTSTART;TZID=Europe/Berlin:20261001T080000\nDTEND;TZID=Europe/Berlin:20261001T081500\nRRULE:FREQ=DAILY\n{}", alarm(":-PT5M")))).await;
    app.put_ics("alice", &plan, "e3", &vevent("e3", "Verschoben", &format!("{b}4T150000\nDTEND;TZID=Europe/Berlin:20261014T160000\n{}\n{}\n{}", alarm(":-PT30M"), alarm(":-PT30M"), alarm(":-PT1H")))).await;
    app.put_ics("alice", &plan, "e4", &vevent("e4", "Ganztag", &format!("DTSTART;VALUE=DATE:20261015\nDTEND;VALUE=DATE:20261016\n{}", alarm(":-PT10H")))).await;
    app.put_ics("alice", &plan, "e6", &vevent("e6", "Nachts", &format!("{b}5T013000\nDTEND;TZID=Europe/Berlin:20261015T020000\n{}", alarm(":-PT30M")))).await;
    app.put_ics("alice", &plan, "t1", &vtodo("t1", "Aufgabe", &format!("DUE;TZID=Europe/Berlin:20261014T170000\n{}", alarm(";RELATED=END:-PT2H")))).await;
    app.put_ics("alice", &plan, "t2", &vtodo("t2", "Erledigt", &format!("STATUS:COMPLETED\nDUE;TZID=Europe/Berlin:20261014T120000\n{}", alarm(":-PT1H")))).await;
    app.put_ics("alice", &plan, "t3", &vtodo("t3", "Wird erledigt", &format!("DUE;TZID=Europe/Berlin:20261014T200000\n{}", alarm(":-PT1H")))).await;
    app.put_ics("group.fam", &fam, "g1", &vevent("g1", "Familie-Termin", &format!("{b}4T180000\nDTEND;TZID=Europe/Berlin:20261014T190000\n{}", alarm(":-PT20M")))).await;
    app.put_ics("group.fam", &fam, "g2", &vevent("g2", "Spät", &format!("{b}5T004500\nDTEND;TZID=Europe/Berlin:20261015T010000\n{}", alarm(":-PT15M")))).await;
    for (id, p) in [("sa", "alice"), ("sb", "bob")] {
        sqlx::query("INSERT INTO practical_push_subscriptions (id, principal, endpoint, p256dh, auth) VALUES (?, ?, ?, ?, ?)").bind(id).bind(p).bind(format!("https://fcm.googleapis.com/fcm/send/{id}")).bind(super::guard::P256DH).bind(super::guard::AUTH).execute(&app.state.db).await.unwrap();
    }
    app
}

/// The edits of the day, at local (Berlin) wall-clock minutes; the same on both apps.
async fn edit(app: &TestApp, hm: &str) {
    let plan: String = sqlx::query("SELECT id FROM calendars WHERE principal = 'alice'").fetch_one(&app.state.db).await.unwrap().get("id");
    match hm {
        // a DAV client adds an event that reminds 20 minutes later
        "09:00" => app.put_ics("alice", &plan, "e5", &vevent("e5", "Neu", &format!("DTSTART;TZID=Europe/Berlin:20261014T093000\nDTEND;TZID=Europe/Berlin:20261014T094500\n{}", alarm(":-PT10M")))).await,
        // e3 moves an hour later, before either of its reminders was due
        "12:00" => app.put_ics("alice", &plan, "e3", &vevent("e3", "Verschoben", &format!("DTSTART;TZID=Europe/Berlin:20261014T160000\nDTEND;TZID=Europe/Berlin:20261014T170000\n{}\n{}\n{}", alarm(":-PT30M"), alarm(":-PT30M"), alarm(":-PT1H")))).await,
        // a rename moves no synctoken
        "13:00" => {
            sqlx::query("UPDATE calendars SET displayname = 'Planung' WHERE principal = 'alice'").execute(&app.state.db).await.unwrap();
        }
        // done before its reminder
        "16:00" => app.put_ics("alice", &plan, "t3", &vtodo("t3", "Wird erledigt", &format!("STATUS:COMPLETED\nDUE;TZID=Europe/Berlin:20261014T200000\n{}", alarm(":-PT1H")))).await,
        "20:00" => prefs::set(&app.state.db, "bob", &prefs::Prefs { timezone: Some("America/New_York".into()), ..prefs::Prefs::defaults() }).await.unwrap(),
        _ => {}
    }
}

/// One fired reminder as a line, the calendar ids (random per app) as `P` (alice's) and `F` (the group's).
async fn lines(app: &TestApp, at: &str, fired: Vec<Fired>) -> Vec<String> {
    let mut out = vec![];
    for f in fired {
        let mut l = format!("{at} {} {}", f.principal, f.payload);
        for (p, short) in [("alice", "P"), ("group.fam", "F")] {
            let id: String = sqlx::query("SELECT id FROM calendars WHERE principal = ?").bind(p).fetch_one(&app.state.db).await.unwrap().get("id");
            l = l.replace(&id, short);
        }
        let v: serde_json::Value = serde_json::from_str(&l[l.find('{').unwrap()..]).unwrap();
        out.push(format!("{} {} | {} | {} | {}", &l[..l.find('{').unwrap() - 1], v["title"].as_str().unwrap(), v["body"].as_str().unwrap(), v["url"].as_str().unwrap(), v["tag"].as_str().unwrap()));
    }
    out
}

#[tokio::test]
async fn the_same_reminders_fire_in_the_same_minutes() {
    let (cached, fresh) = (setup().await, setup().await);
    let (mut plan_c, mut plan_f) = (Plan::default(), Plan::default());
    let mut watch_c = crate::db::Watch::new(cached.state.db.clone());
    let mut watch_f = crate::db::Watch::new(fresh.state.db.clone());
    let berlin = chrono_tz::Europe::Berlin;
    // 26 hours of rounds, at second 17 of every minute, from 00:00 local on 2026-10-14
    let start = chrono::DateTime::parse_from_rfc3339("2026-10-13T22:00:17Z").unwrap().with_timezone(&Utc);
    let mut log: Vec<String> = vec![];
    for m in 0..26 * 60 {
        let now = start + Duration::minutes(m);
        let hm = now.with_timezone(&berlin).format("%H:%M").to_string();
        edit(&cached, &hm).await;
        edit(&fresh, &hm).await;
        let (fired_c, _) = tick(&cached.state, &mut plan_c, watch_c.stamp().await, now).await.unwrap();
        // the loop of before: everything computed again, nothing cached
        plan_f = Plan { builds: plan_f.builds, ..Plan::default() };
        fresh.state.task_cache.clear();
        let (fired_f, _) = tick(&fresh.state, &mut plan_f, watch_f.stamp().await, now).await.unwrap();
        let at = now.with_timezone(&berlin).format("%d. %H:%M").to_string();
        let (lc, lf) = (lines(&cached, &at, fired_c).await, lines(&fresh, &at, fired_f).await);
        assert_eq!(lc, lf, "round at {hm}");
        log.extend(lc);
    }
    let expected: Vec<String> = [
        "14. 07:54 alice Täglich | 08:00 · Plan | /calendar?view=day&date=2026-10-14 | alice/P/e2",
        "14. 09:19 alice Neu | 09:30 · Plan | /calendar?view=day&date=2026-10-14 | alice/P/e5",
        "14. 09:44 alice Termin | 10:00 · Plan | /calendar?view=day&date=2026-10-14 | alice/P/e1",
        "14. 13:59 alice Ganztag | 15.10. · Planung | /calendar?view=day&date=2026-10-15 | alice/P/e4",
        "14. 14:59 alice Verschoben | 16:00 · Planung | /calendar?view=day&date=2026-10-14 | alice/P/e3",
        "14. 14:59 alice Aufgabe | 14.10. 17:00 · Planung | /tasks | alice/P/t1",
        "14. 15:29 alice Verschoben | 16:00 · Planung | /calendar?view=day&date=2026-10-14 | alice/P/e3",
        "14. 17:39 alice Familie-Termin | 18:00 · Familie | /calendar?view=day&date=2026-10-14 | group.fam/F/g1",
        "14. 17:39 bob Familie-Termin | 17:00 · Familie | /calendar?view=day&date=2026-10-14 | group.fam/F/g1",
        "15. 00:29 alice Spät | 00:45 · Familie | /calendar?view=day&date=2026-10-15 | group.fam/F/g2",
        "15. 00:29 bob Spät | 18:45 · Familie | /calendar?view=day&date=2026-10-14 | group.fam/F/g2",
        "15. 00:59 alice Nachts | 01:30 · Planung | /calendar?view=day&date=2026-10-15 | alice/P/e6",
    ]
    .iter()
    .map(|l| l.to_string())
    .collect();
    assert_eq!(log, expected, "\n{}", log.join("\n"));
    // the fresh app computed every minute; the cached one only after a write (the 5 edits, its own
    // sent-log rows in the 9 minutes after a fire) or a subscriber's new day (bob's in London at
    // 01:00, alice's at midnight), plus the first round
    assert_eq!(plan_f.builds, 26 * 60);
    assert_eq!(plan_c.builds, 1 + 5 + 9 + 2, "rounds that computed the reminders");
}

/// Without subscribers a round costs the stamp and nothing else.
#[tokio::test]
async fn no_subscribers_no_work() {
    let app = TestApp::new().await;
    let mut plan = Plan::default();
    let mut watch = crate::db::Watch::new(app.state.db.clone());
    let now = chrono::DateTime::parse_from_rfc3339("2026-10-14T08:00:17Z").unwrap().with_timezone(&Utc);
    for m in 0..120 {
        assert!(tick(&app.state, &mut plan, watch.stamp().await, now + Duration::minutes(m)).await.unwrap().0.is_empty());
    }
    assert_eq!(plan.builds, 1);
    // a new subscription is a write: the next round looks again
    app.user("alice", "Alice").await;
    sqlx::query("INSERT INTO practical_push_subscriptions (id, principal, endpoint, p256dh, auth) VALUES ('s', 'alice', 'https://fcm.googleapis.com/fcm/send/s', ?, ?)").bind(super::guard::P256DH).bind(super::guard::AUTH).execute(&app.state.db).await.unwrap();
    tick(&app.state, &mut plan, watch.stamp().await, now + Duration::minutes(120)).await.unwrap();
    assert_eq!((plan.builds, plan.users.len()), (2, 1));
}

/// The sent-log keeps a fired reminder from going out twice, also across a rebuilt plan (a restart).
#[tokio::test]
async fn a_sent_reminder_is_not_sent_again() {
    let app = TestApp::new().await;
    app.user("alice", "Alice").await;
    let cal = app.calendar("alice", "Plan", "#3478f6", &["VEVENT"]).await;
    app.put_ics("alice", &cal, "e", &ics(&format!("BEGIN:VEVENT\nUID:e\nDTSTAMP:20260901T000000Z\nSUMMARY:x\nDTSTART:20261014T100000Z\nDTEND:20261014T110000Z\n{}\nEND:VEVENT", alarm(":-PT10M")))).await;
    sqlx::query("INSERT INTO practical_push_subscriptions (id, principal, endpoint, p256dh, auth) VALUES ('s', 'alice', 'https://fcm.googleapis.com/fcm/send/s', ?, ?)").bind(super::guard::P256DH).bind(super::guard::AUTH).execute(&app.state.db).await.unwrap();
    let mut watch = crate::db::Watch::new(app.state.db.clone());
    let t = |s: &str| chrono::DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc);
    let mut plan = Plan::default();
    // the window is [now-2min, now+1min]: four rounds see the trigger, one sends
    let mut n = 0;
    for at in ["2026-10-14T09:49:30Z", "2026-10-14T09:50:00Z", "2026-10-14T09:51:00Z", "2026-10-14T09:51:59Z"] {
        n += tick(&app.state, &mut plan, watch.stamp().await, t(at)).await.unwrap().0.len();
    }
    assert_eq!(n, 1);
    let mut restarted = Plan::default();
    assert!(tick(&app.state, &mut restarted, None, t("2026-10-14T09:51:30Z")).await.unwrap().0.is_empty());
}
