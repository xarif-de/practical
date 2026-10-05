//! Every page and HTML fragment answers 200 with the markers the client and the eye rely on.

use super::seed::seed;
use crate::testkit::TestApp;
use axum::http::StatusCode;

#[tokio::test]
async fn calendar_views_render_their_body() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    for (view, title, marker) in [
        ("month", "<title>Oktober 2026 · PractiCal</title>", "class=\"cal v-month on\""),
        ("week", "<title>KW 42 2026 · PractiCal</title>", "class=\"cal v-week on\""),
        ("day", "<title>14. Oktober 2026 · PractiCal</title>", "class=\"cal v-day on\""),
        ("planner", "<title>2026 · PractiCal</title>", "class=\"cal v-planner on\""),
        ("weekends", "<title>2026 · PractiCal</title>", "class=\"cal v-weekends on\""),
        ("vacation", "<title>2026 · PractiCal</title>", "class=\"cal v-vacation on\""),
        ("countdown", "<title>Countdown-Wand · PractiCal</title>", "class=\"cal v-countdown on\""),
        ("numbers", "<title>2026 · PractiCal</title>", "class=\"cal v-numbers on\""),
        // an unknown view is the month
        ("nonsense", "<title>Oktober 2026 · PractiCal</title>", "class=\"cal v-month on\""),
    ] {
        let r = c.get(&format!("/calendar?view={view}&date=2026-10-14")).await;
        r.ok().has(title).has(marker).has("<meta name=\"csrf\" content=\"").has("id=\"picker\"").has("data-user=\"alice\"");
        assert!(r.header("content-type").unwrap().starts_with("text/html"), "{view}");
        // the page's JSON islands parse
        for island in ["i18n"] {
            r.island(island);
        }
    }
}

#[tokio::test]
async fn month_view_shows_events_holidays_and_group_calendars() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let r = c.get("/calendar?view=month&date=2026-10-14").await;
    r.ok()
        .has("<b class=\"fname pub\" title=\"Tag der Deutschen Einheit\">Tag der Deutschen Einheit</b>")
        .has("<b class=\"fname\" title=\"Herbstferien\">Herbstferien</b>")
        .has("Quartalsreview 14:00")
        .has("Familientreffen")
        .has("class=\"ev allday span start\"")
        .has("class=\"ev cancelled\"")
        .has("Abendessen 20:00") // 18:00Z in Europe/Berlin (CEST)
        .has("data-date=\"2026-10-14\"");
    // the filter lists every calendar alice sees, the group's included
    for name in ["Arbeit", "Privat", "Aufgaben", "Familienkalender"] {
        r.has(name);
    }
    // carol sees none of it
    let mut carol = s.app.login("carol").await;
    let r = carol.get("/calendar?view=month&date=2026-10-14").await;
    r.ok();
    assert!(!r.body.contains("Quartalsreview") && !r.body.contains("Familientreffen"));
}

#[tokio::test]
async fn calendar_filter_and_fragment() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let only_privat = format!("alice/{}", s.privat);
    let r = c.get(&format!("/calendar?view=month&date=2026-10-14&cals={only_privat}&fragment=1")).await;
    r.ok().has("Zahnarzt Dr. Weiß").has("class=\"cal v-month on\"");
    assert!(!r.body.contains("<html") && !r.body.contains("Quartalsreview"), "a fragment is the toolbar and body only, filtered");
    // month= turns the grid without moving the picked day
    let r = c.get("/calendar?view=month&date=2026-10-14&month=2026-11&fragment=1").await;
    r.ok().has("November 2026").has("Buß- und Bettag");
    // week= picks the week on screen
    let r = c.get("/calendar?view=week&date=2026-10-14&week=2026-10-19&fragment=1").await;
    r.ok().has("KW 43 2026").has("data-focus=\"2026-10-19\"");
}

#[tokio::test]
async fn day_view_document_carries_the_details() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let r = c.get("/calendar?view=day&date=2026-10-14").await;
    r.ok()
        .has("<section class=\"doc\" data-date=\"2026-10-14\">")
        .has("Raum 4.12, Hauptgebäude")
        .has("51.05041, 13.73726")
        .has("Zahlen mitbringen")
        .has("https://example.org/review")
        .has("Wöchentlich")
        .has("10 Minuten vorher");
}

#[tokio::test]
async fn contacts_list_and_table() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let r = c.get("/contacts").await;
    r.ok().has("<title>Kontakte · PractiCal</title>").has("Anna Schmidt").has("ACME GmbH").has("+49 160 1234567").has("anna@example.org");
    let t = c.get("/contacts?view=table").await;
    t.ok().has("Anna Schmidt");
    assert_ne!(r.body, t.body);
    let filtered = c.get(&format!("/contacts?abs=alice/{}", "does-not-exist")).await;
    filtered.ok();
    assert!(!filtered.body.contains("Anna Schmidt"));
}

#[tokio::test]
async fn tasks_page_ships_its_json_island() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    for view in ["groups", "table", "board", "timeline", "deck", "history", "bogus"] {
        let r = c.get(&format!("/tasks?view={view}&back=all")).await;
        r.ok().has("<title>Aufgaben · PractiCal</title>");
        let d = r.island("tasks-data");
        let open: Vec<&str> = d["open"].as_array().unwrap().iter().map(|t| t["summary"].as_str().unwrap()).collect();
        let mut open_sorted = open.clone();
        open_sorted.sort();
        assert_eq!(open_sorted, ["Keller sortieren", "Steuererklärung"], "{view}");
        assert_eq!(d["done_total"], 2);
        assert_eq!(d["completed_total"], 1);
        assert_eq!(d["cancelled_total"], 1);
        assert_eq!(d["per_day"], serde_json::json!({ "2026-10-01": 1 }));
        // newest completion (or due date) first
        let done: Vec<&str> = d["done"].as_array().unwrap().iter().map(|t| t["summary"].as_str().unwrap()).collect();
        assert_eq!(done, ["Garage ausmisten", "Fenster putzen"]);
    }
    // only VTODO calendars are offered, the filter narrows to one
    let r = c.get(&format!("/tasks?back=all&cals=alice/{}", s.arbeit)).await;
    let d = r.ok().island("tasks-data");
    assert_eq!(d["open"].as_array().unwrap().len(), 0);
    assert!(!r.body.contains(">Privat<"), "Privat holds no VTODO");
}

#[tokio::test]
async fn task_lookback_comes_from_the_query_then_the_cookie() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    c.cookie("tk_back", "all");
    let r = c.get("/tasks").await;
    assert_eq!(r.ok().island("tasks-data")["done_total"], 2);
    r.has("<option value=\"all\" selected>");
}

#[tokio::test]
async fn history_settings_and_login_pages() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let h = c.get("/history").await;
    h.ok().has("<title>Änderungen · PractiCal</title>");
    let d = h.island("history-data");
    assert_eq!(d["me"], "alice");
    assert_eq!(d["retention"], 90);
    assert!(d["colls"].as_object().unwrap().len() >= 5);
    let st = c.get("/settings").await;
    st.ok().has("<title>Einstellungen · PractiCal</title>").has("Sachsen").has("Familie").has("Freunde").has("action=\"/settings/prefs\"").has("action=\"/settings/tokens\"");
    st.island("i18n");
    // logged in, /login sends to the calendar
    let l = c.get("/login").await;
    assert_eq!((l.status, l.location().as_deref()), (StatusCode::SEE_OTHER, Some("/calendar")));
}

#[tokio::test]
async fn languages_follow_preference_then_accept_language() {
    let s = seed().await;
    let mut c = s.app.login("bob").await;
    let r = c.send(axum::http::Method::GET, "/settings", &[("accept", "text/html"), ("accept-language", "fr-FR,fr;q=0.9")], axum::body::Body::empty()).await;
    r.ok().has("<html lang=\"fr\"").has("data-locale=\"fr-FR\"");
    // a saved preference wins over the header
    c.post_form("/settings/prefs", &[("lang", "en")]).await;
    let r = c.send(axum::http::Method::GET, "/settings", &[("accept", "text/html"), ("accept-language", "fr-FR,fr;q=0.9")], axum::body::Body::empty()).await;
    r.ok().has("<html lang=\"en\"").has("<title>Settings · PractiCal</title>");
    let anon = s.app.anonymous().send(axum::http::Method::GET, "/login", &[("accept-language", "es")], axum::body::Body::empty()).await;
    anon.ok().has("<html lang=\"es\"");
}

#[tokio::test]
async fn root_health_and_unknown_routes() {
    let app = TestApp::new().await;
    let mut a = app.anonymous();
    let r = a.get("/").await;
    assert_eq!((r.status, r.location().as_deref()), (StatusCode::SEE_OTHER, Some("/calendar")));
    let r = a.get("/healthz").await;
    assert_eq!((r.status, r.body.as_str()), (StatusCode::OK, "ok"));
    let r = a.get("/no/such/page").await;
    assert_eq!((r.status, r.body.as_str()), (StatusCode::NOT_FOUND, ""));
    // /dev/reload exists only in development mode
    assert_eq!(a.get("/dev/reload").await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn html_fragments_for_the_endless_lists() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let days = c.get_json("/api/days?start=2026-10-13&count=3").await;
    days.ok().has("<a class=\"lrow fer\" href=\"/calendar?view=day&date=2026-10-14\" data-date=\"2026-10-14\"><b>14</b><span>Mi</span><i style=\"--n:3\"></i><em>Standup, Quartalsreview, Abendessen</em>");
    assert_eq!(days.body.matches("class=\"lrow").count(), 3);
    // count is clamped to 1..=120
    assert_eq!(c.get_json("/api/days?start=2026-10-13&count=0").await.body.matches("class=\"lrow").count(), 1);
    assert_eq!(c.get_json("/api/days?start=2026-10-13&count=999").await.body.matches("class=\"lrow").count(), 120);
    let weeks = c.get_json("/api/weeks?start=2026-10-14&count=1").await;
    weeks.ok().has("<div class=\"week\" data-week=\"2026-10-12\"><h2 class=\"wk\">KW 42 <span>12.10. – 18.10.</span> <b class=\"fname\">Herbstferien</b></h2>");
    assert_eq!(c.get_json("/api/weeks?start=2026-10-12&count=99").await.body.matches("<div class=\"week").count(), 16);
    // a missing parameter is axum's 400
    assert_eq!(c.get_json("/api/days").await.status, StatusCode::BAD_REQUEST);
}
