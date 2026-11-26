//! View configurations through `/api/views` and the three views they drive (`domain::views`): own rules, what a
//! sharer offers, what a sharee adopts, and that an offered rule never reaches past the sharer's calendars.

use super::seed::{seed, seed_shared};
use crate::testkit::vevent;
use axum::http::{Method, StatusCode};
use serde_json::json;

const PLANNER: &str = "/calendar?view=planner&date=2026-10-14";

#[tokio::test]
async fn own_rules_drive_year_view_countdown_and_vacation_planner() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    // no row yet: the former default, the category "Urlaub", in each view
    let d = c.get_json("/api/views").await.ok().json();
    assert_eq!((d["config"]["year"]["rules"][0]["categories"][0].as_str(), d["config"]["countdown"]["rules"][0]["icon"].as_str(), d["config"]["vacation"]["share"].as_bool()), (Some("Urlaub"), Some("luggage"), Some(false)));
    assert!(d["calendars"].as_array().unwrap().iter().any(|x| x["key"] == format!("alice/{}", s.privat) && x["name"] == "Privat"), "{d}");
    c.get(PLANNER).await.ok().has("data-date=\"2026-10-27\" title=\"Urlaub Ostsee\"><b>27</b><s>D</s><span class=\"u\" style=\"--u:#32ade6\">");

    let mut cfg = d["config"].clone();
    cfg["year"]["rules"] = json!([{ "name": "Ferien", "categories": ["urlaub", "Reise"], "color": "#FF9F0A", "icon": "plane" }]);
    cfg["countdown"]["rules"] = json!([{ "categories": [], "cals": [format!("alice/{}", s.arbeit)], "icon": "heart" }]);
    cfg["vacation"]["days"] = json!(30);
    let r = c.json(Method::PUT, "/api/views", cfg.clone()).await.ok().json();
    let rule = &r["config"]["year"]["rules"][0];
    assert_eq!((rule["color"].as_str(), rule["id"].as_str().map(str::len)), (Some("#ff9f0a"), Some(8)), "colour normalised, an id made");
    let p = c.get(PLANNER).await.ok().body.clone();
    assert!(p.contains("<span class=\"u\" style=\"--u:#ff9f0a\"><svg class=\"ico\""), "{p}");
    assert!(p.contains("i-plane\"/></svg>Ferien</span>"), "the legend names the rule: {p}");
    // countdown: the events of Arbeit (the calendar rule) with its icon; the weekly standup once, at its next date
    let cd = c.get("/calendar?view=countdown&date=2026-10-14").await.ok().body.clone();
    assert!(cd.contains("i-heart") && cd.matches("<h3>Standup</h3>").count() == 1, "{cd}");
    let v = c.get("/calendar?view=vacation&date=2026-10-14").await.ok().body.clone();
    assert!(v.contains("<span>Resturlaub von 30</span>") && v.contains("<a href=\"/settings#views-vacation\">"), "{}", &v[v.find("class=\"grid2\"").unwrap_or(0)..]);

    // a rule that names neither a category nor a calendar would mark everything: refused, nothing stored
    let mut bad = r["config"].clone();
    bad["year"]["rules"] = json!([{ "categories": [" "] }]);
    let r2 = c.json(Method::PUT, "/api/views", bad).await;
    assert_eq!(r2.status, StatusCode::BAD_REQUEST, "{}", r2.body);
    assert_eq!(c.get_json("/api/views").await.ok().json()["config"], r["config"]);
    // the preferences' autosave rewrites its own row only
    c.post_form_json("/settings/prefs", &[("lang", "de"), ("scheme", "light")]).await.ok();
    assert_eq!(c.get_json("/api/views").await.ok().json()["config"], r["config"]);
    // the settings page counts the rules per view
    c.get("/settings").await.ok().has("id=\"v-views-year\">1 Regel</span>").has("id=\"v-views\">3 Regeln</span>");
}

#[tokio::test]
async fn offered_rules_reach_only_the_sharers_calendars_and_only_when_adopted() {
    let w = seed_shared().await;
    let app = &w.s.app;
    // dave's own calendar holds an event of the same category: alice's rule must never mark it
    let own = app.calendar("dave", "Daves", "#5856d6", &["VEVENT"]).await;
    app.put_ics("dave", &own, "eigen", &vevent("eigen", "Daves Urlaub", "DTSTART;VALUE=DATE:20261110\nDTEND;VALUE=DATE:20261112\nCATEGORIES:Urlaub")).await;
    // nothing is offered until alice chooses to (her row does not exist yet: the default)
    let mut dave = app.login("dave").await;
    assert_eq!(dave.get_json("/api/views").await.ok().json()["people"][0]["offer"]["year"], json!([]));
    let mut alice_c = app.login("alice").await;
    let mut a = alice_c.get_json("/api/views").await.ok().json()["config"].clone();
    a["year"]["share"] = json!(true);
    alice_c.json(Method::PUT, "/api/views", a).await.ok();
    let d = dave.get_json("/api/views").await.ok().json();
    let alice = d["people"].as_array().unwrap().iter().find(|p| p["id"] == "alice").expect("alice shares Privat with dave").clone();
    assert_eq!(alice["label"], "Alice Beispiel");
    assert_eq!(alice["offer"]["year"][0]["categories"], json!(["Urlaub"]));
    assert!(alice["offer"]["vacation"].is_null(), "vacation is offered only when alice says so: {alice}");
    // dave without own year rules, alice's adopted with his own colour
    let mut cfg = d["config"].clone();
    cfg["year"]["rules"] = json!([]);
    cfg["year"]["adopt"] = json!({ "alice": { alice["offer"]["year"][0]["id"].as_str().unwrap(): { "on": true, "color": "#123456" } }, "carol": { "x": { "on": true } } });
    let r = dave.json(Method::PUT, "/api/views", cfg).await.ok().json();
    assert!(r["config"]["year"]["adopt"].get("carol").is_none(), "a choice about someone who shares nothing with dave goes");
    let p = dave.get(PLANNER).await.ok().body.clone();
    assert!(p.contains("title=\"Urlaub Ostsee\"><b>27</b><s>D</s><span class=\"u\" style=\"--u:#123456\">"), "{p}");
    assert!(p.contains("Urlaub <small>(Alice Beispiel)</small></span>"), "{p}");
    let nov = dave.get("/calendar?view=planner&date=2026-11-10").await.ok().body.clone();
    assert!(nov.contains("title=\"Daves Urlaub\"><b>10</b><s>D</s><span>Daves Urlaub</span>"), "alice's rule leaves dave's own calendar alone: {nov}");

    // vacation: offered once alice shares it, shown once dave picks her
    let mut a = alice_c.get_json("/api/views").await.ok().json()["config"].clone();
    a["vacation"]["share"] = json!(true);
    a["vacation"]["days"] = json!(28);
    alice_c.json(Method::PUT, "/api/views", a.clone()).await.ok();
    let vac = "/calendar?view=vacation&date=2026-10-14";
    assert!(!dave.get(vac).await.ok().body.contains("data-l=\"Urlaub · Alice Beispiel\""), "not picked yet");
    let mut cfg = r["config"].clone();
    cfg["vacation"]["people"] = json!({ "alice": true });
    dave.json(Method::PUT, "/api/views", cfg).await.ok();
    // 26.10.–30.10.: five workdays (Reformationstag is a Saturday)
    dave.get(vac).await.ok().has("data-l=\"Urlaub · Alice Beispiel\"").has("<li><b>Alice Beispiel</b> — 5 Urlaubstage geplant · 23 Resturlaub von 28</li>");
    a["vacation"]["share"] = json!(false);
    alice_c.json(Method::PUT, "/api/views", a).await.ok();
    assert!(!dave.get(vac).await.ok().body.contains("data-l=\"Urlaub · Alice Beispiel\""), "withdrawn");

    // someone with no share from alice learns nothing of her rules
    let mut carol = app.login("carol").await;
    assert_eq!(carol.get_json("/api/views").await.ok().json()["people"], json!([]));
}
