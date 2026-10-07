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
        // app.js' strings come as the language's script (no JSON block in the page any more)
        r.has(&format!("<script src=\"{}\" defer></script>", crate::routes::assets::url("i18n-de.js").unwrap()));
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

/// One view per page (perf3 #8): the URL's, else the browser's last pick (cookie `ctview`), else the list; the other
/// comes from /contacts/part as the page would render it.
#[tokio::test]
async fn contacts_page_renders_its_view_and_the_part_the_other() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let list = c.get("/contacts").await;
    list.ok().has("<li tabindex=\"0\" data-key=\"alice/").has("<table class=\"sheet\">").has("data-jump=\"A\"");
    assert!(!list.body.contains("<tr data-key="), "the list page has no table rows");
    let table = c.get("/contacts?view=table").await;
    table.ok().has("<tr data-key=\"alice/").has("data-name=\"Anna Schmidt\" data-given=\"Anna\" data-family=\"Schmidt\" data-bday=\"1985-04-02\" data-email=\"anna@example.org\"").has("data-jump=\"A\"");
    assert!(!table.body.contains("<li tabindex=\"0\" data-key="), "the table page has no list entries");
    // the browser's pick; the URL wins over it
    c.cookie("ctview", "table");
    assert_eq!(c.get("/contacts").await.ok().body, table.body);
    assert_eq!(c.get("/contacts?view=list").await.ok().body, list.body);
    // the parts: exactly what the page of the other view renders (the rows without the sort keys: the list has them)
    let lp = c.get("/contacts/part?view=list").await;
    lp.ok();
    assert!(lp.body.starts_with("<section class=\"lpanel\" data-letter=\"A\">") && lp.body.ends_with("</section>"), "{}", &lp.body[..80]);
    assert!(list.body.contains(&format!("<div class=\"panels\">\n{}\n", lp.body)));
    let tp = c.get("/contacts/part?view=table").await;
    tp.ok();
    let strip = |h: &str| ["data-name", "data-given", "data-family", "data-bday", "data-email"].iter().fold(h.to_string(), |h, a| {
        let mut out = String::new();
        let mut rest = h.as_str();
        while let Some(i) = rest.find(&format!(" {a}=\"")) {
            out.push_str(&rest[..i]);
            let v = &rest[i + a.len() + 3..];
            rest = &v[v.find('"').unwrap() + 1..];
        }
        out + rest
    });
    assert!(tp.body.starts_with("<tr data-key=") && !tp.body.contains("data-name="));
    assert!(strip(&table.body).contains(&format!("<tbody>\n{}\n</tbody>", tp.body)));
    assert!(list.body.contains("<tbody>\n\n</tbody>") && table.body.contains("<div class=\"panels\">\n\n"));
    // the page's address book filter applies
    let none = c.get("/contacts/part?view=table&abs=alice/none").await;
    assert_eq!(none.ok().body, "");
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
    st.has(&format!("<script src=\"{}\" defer></script>", crate::routes::assets::url("i18n-de.js").unwrap()));
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

/// B19: every HTML answer (pages, fragments, the login page, errors) carries the Content-Security-Policy;
/// JSON and the static files do not need it.
#[tokio::test]
async fn html_responses_carry_the_content_security_policy() {
    let s = seed().await;
    let want = crate::content_security_policy(&s.app.state.config);
    assert!(want.contains("script-src 'self';") && !want.contains("unsafe-eval") && want.contains("form-action 'self';"), "{want}");
    let mut c = s.app.login("alice").await;
    for p in ["/calendar", "/calendar?view=week&date=2026-10-14&fragment=1", "/contacts", "/tasks", "/history", "/settings"] {
        let r = c.get(p).await;
        assert!(r.header("content-type").unwrap().starts_with("text/html"), "{p}");
        assert_eq!(r.header("content-security-policy").as_deref(), Some(want.as_str()), "{p}");
    }
    for p in ["/api/events?start=2026-10-01&end=2026-11-01", "/assets/app.js", "/assets/icons.svg", "/healthz"] {
        assert_eq!(c.get(p).await.header("content-security-policy"), None, "{p}");
    }
    let login = s.app.anonymous().get("/login").await;
    assert_eq!(login.header("content-security-policy").as_deref(), Some(want.as_str()));
}

/// GET /editor?kind= (routes/editor.rs): the dialog and the islands of the page that holds it for that kind — equal to
/// the calendar's (event) and the tasks page's (task), their own rules included — and nothing of a page around them;
/// behind the login, never consuming a pending flash.
#[tokio::test]
async fn editor_fragment_per_kind_matches_its_page_behind_the_login() {
    let s = seed().await;
    let r = s.app.anonymous().get("/editor?kind=event").await;
    assert_eq!((r.status, r.location().as_deref()), (StatusCode::SEE_OTHER, Some("/login?redirect_uri=%2Feditor%3Fkind%3Devent")));
    assert_eq!(s.app.anonymous().get_json("/editor?kind=task").await.status, StatusCode::UNAUTHORIZED);
    let mut c = s.app.login("alice").await;
    for (kind, scope) in [("event", "Sport"), ("task", "Putzen")] {
        c.json(axum::http::Method::PUT, &format!("/api/category-rules/{scope}?scope={kind}"), serde_json::json!({ "when": { "color": { "mode": "default", "value": "#ff0000" } } })).await.ok();
    }
    let select = |b: &str| { let at = b.find("id=\"ed-cal\"").expect("calendar choice"); b[at..at + b[at..].find("</select>").unwrap()].to_string() };
    for (kind, page, own, other) in [("event", "/calendar?view=month&date=2026-10-14", "Sport", "Putzen"), ("task", "/tasks", "Putzen", "Sport")] {
        let (r, p) = (c.get(&format!("/editor?kind={kind}")).await, c.get(page).await);
        r.ok().has(&format!("<div id=\"ed-host\" data-kind=\"{kind}\">")).has("<form id=\"edform\"").has("id=\"geomap\"");
        assert_eq!(r.header("cache-control").as_deref(), Some("private, no-cache"), "{kind}");
        assert!(!r.body.contains("<html") && !r.body.contains("id=\"viewroot\"") && !r.body.contains("id=\"tasks-data\""), "{kind}: the dialog only");
        assert_eq!(r.body.contains("class=\"ed taskmode\""), kind == "task");
        assert_eq!(r.body.contains("data-card=\"repeat\""), kind == "event", "{kind}: the kind's cards");
        for id in ["alarm-presets-data", "category-rules-data", "alarm-snaps-data"] {
            assert_eq!(r.island(id), p.island(id), "{kind}: #{id} as on {page}");
        }
        let rules = r.island("category-rules-data");
        assert!(rules.get(own).is_some() && rules.get(other).is_none(), "{kind}: its scope's rules: {rules}");
        assert_eq!(select(&r.body), select(&p.body), "{kind}: the calendar choice of {page}");
    }
    assert!(!select(&c.get("/editor?kind=task").await.body).contains(">Privat<"), "a task's calendars hold VTODOs");
    assert_eq!(c.get("/editor?kind=journal").await.status, StatusCode::BAD_REQUEST);
    assert_eq!(c.get("/editor").await.status, StatusCode::BAD_REQUEST);
    // a flash waiting for the next page stays for it
    c.post_form("/settings/tokens", &[("name", " ")]).await;
    c.get("/editor?kind=event").await.ok();
    c.get("/settings").await.ok().has("<div class=\"toast error\"");
    // also past a rejected query (the query is read before the page context takes the flash)
    c.post_form("/settings/tokens", &[("name", " ")]).await;
    assert_eq!(c.get("/editor?kind=journal").await.status, StatusCode::BAD_REQUEST);
    c.get("/settings").await.ok().has("<div class=\"toast error\"");
}
