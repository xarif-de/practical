//! First start (`domain::onboarding`, `routes::settings::onboarding`): which steps a fresh user is
//! asked, that a confirmation writes a mark and no preference, that the users of an existing
//! instance are carried over as through, and that only the server ends the setup.

use super::seed::seed;
use crate::domain::{onboarding, prefs};
use axum::http::{Method, StatusCode};
use serde_json::json;

/// The state the page frame hands the browser.
async fn state(c: &mut crate::testkit::Client) -> serde_json::Value {
    c.json(Method::GET, "/api/onboarding", json!(null)).await.ok().json()
}

#[tokio::test]
async fn a_fresh_user_is_asked_every_step_and_the_seeded_one_only_what_is_missing() {
    let s = seed().await;
    // carol has no collections and no preferences at all
    let mut carol = s.app.login("carol").await;
    assert_eq!(state(&mut carol).await["open"], json!(["lang", "tz", "regions", "school", "calendar", "addressbook"]));
    // the language gate is part of the page, so it works without JavaScript
    carol.get("/calendar").await.ok().has(r#"action="/settings/onboarding/lang""#).has(r#"id="onb-gate""#);

    // alice has collections AND named her holiday regions: only language and time zone are left
    let mut alice = s.app.login("alice").await;
    assert_eq!(state(&mut alice).await["open"], json!(["lang", "tz"]));
    // and her page carries the state the hints read
    assert_eq!(alice.get("/calendar").await.ok().island("onboarding")["open"], json!(["lang", "tz"]));
}

#[tokio::test]
async fn the_language_gate_stores_the_choice_and_returns_to_the_page_it_blocked() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let r = c.post_form("/settings/onboarding/lang", &[("lang", "fr"), ("back", "/tasks")]).await;
    assert_eq!(r.location().as_deref(), Some("/tasks"), "the gate sits on whatever page was opened");
    assert_eq!(prefs::get(&s.app.state.db, "alice").await.unwrap().lang.as_deref(), Some("fr"));
    let rec = onboarding::get(&s.app.state.db, "alice").await.unwrap();
    assert!(rec.has(onboarding::Step::Lang), "the confirmation is recorded, not only the value");
    assert_eq!(state(&mut c).await["open"], json!(["tz"]));
    // an unknown language is refused and changes nothing
    let mut b = s.app.login("bob").await;
    b.post_form("/settings/onboarding/lang", &[("lang", "kl"), ("back", "/calendar")]).await;
    assert_eq!(prefs::get(&s.app.state.db, "bob").await.unwrap().lang, None);
    assert!(!onboarding::get(&s.app.state.db, "bob").await.unwrap().has(onboarding::Step::Lang));
}

#[tokio::test]
async fn an_off_site_back_target_is_not_followed() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    // the control characters matter: a browser drops them and reads what is left as the host
    for bad in ["https://example.org/", "//example.org/", "/\\example.org", "/\texample.org", "/\nexample.org", "/login"] {
        let r = c.post_form("/settings/onboarding/lang", &[("lang", "de"), ("back", bad)]).await;
        assert_eq!(r.location().as_deref(), Some("/calendar"), "{bad}");
    }
}

/// The point of the record: a user who only confirms the proposal is remembered, and the proposal
/// becomes an explicit preference. Taking the time-zone picker's own "default" row writes an empty
/// zone, so without this the step could never be closed by accepting what was proposed.
#[tokio::test]
async fn confirming_a_default_records_the_mark_and_writes_nothing() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    assert_eq!(prefs::get(&s.app.state.db, "alice").await.unwrap().timezone, None);

    let r = c.json(Method::POST, "/settings/onboarding/confirm", json!({ "step": "tz" })).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json()["open"], json!(["lang"]));
    let p = prefs::get(&s.app.state.db, "alice").await.unwrap();
    // Taking the picker's "Standard" row means following the server's default, so nothing is
    // written: the column stays empty and the mark carries the decision.
    assert_eq!(p.timezone, None, "accepting the proposal does not freeze today's default");
    assert!(onboarding::get(&s.app.state.db, "alice").await.unwrap().has(onboarding::Step::Tz));

    // The region point has no value to keep: confirming it means "I want none", and writes nothing.
    // carol has no region, so the point is open for her.
    let mut carol = s.app.login("carol").await;
    assert!(state(&mut carol).await["open"].as_array().unwrap().iter().any(|v| v == "regions"));
    carol.json(Method::POST, "/settings/onboarding/confirm", json!({ "step": "regions" })).await;
    assert!(!state(&mut carol).await["open"].as_array().unwrap().iter().any(|v| v == "regions"), "declining closes it");
    let cp = prefs::get(&s.app.state.db, "carol").await.unwrap();
    assert_eq!((cp.public_regions, cp.school_regions), (None, None), "declining writes no region into the column");

    // a zone the user chose themselves closes the point on its own, with no mark at all
    let mut b = s.app.login("bob").await;
    assert!(state(&mut b).await["open"].as_array().unwrap().iter().any(|v| v == "tz"));
    let mut p = prefs::get(&s.app.state.db, "bob").await.unwrap();
    p.timezone = Some("America/New_York".into());
    prefs::set(&s.app.state.db, "bob", &p).await.unwrap();
    assert!(!state(&mut b).await["open"].as_array().unwrap().iter().any(|v| v == "tz"), "a picked city is the answer");
    assert!(!onboarding::get(&s.app.state.db, "bob").await.unwrap().has(onboarding::Step::Tz), "and needs no mark");
}

#[tokio::test]
async fn only_the_confirmable_steps_can_be_confirmed() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    for step in ["calendar", "addressbook", "nonsense"] {
        let r = c.json(Method::POST, "/settings/onboarding/confirm", json!({ "step": step })).await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{step}");
    }
    assert!(onboarding::get(&s.app.state.db, "alice").await.unwrap().confirmed.is_empty());
}

/// A collection closes its own step by existing — created or imported, no mark involved.
#[tokio::test]
async fn a_created_collection_closes_its_step() {
    let s = seed().await;
    let mut c = s.app.login("carol").await;
    assert_eq!(state(&mut c).await["open"], json!(["lang", "tz", "regions", "school", "calendar", "addressbook"]));
    c.post_form("/settings/calendars", &[("name", "Privat"), ("owner", "carol"), ("color", "#0a84ff"), ("comp_event", "on")]).await;
    assert_eq!(state(&mut c).await["open"], json!(["lang", "tz", "regions", "school", "addressbook"]));
    c.post_form("/settings/addressbooks", &[("name", "Kontakte"), ("owner", "carol"), ("color", "#30b46c")]).await;
    assert_eq!(state(&mut c).await["open"], json!(["lang", "tz", "regions", "school"]));
}

/// Trying a language on is not answering: the gate reloads in it and stays up, so the user can read
/// what they are choosing. Only the button records the mark.
#[tokio::test]
async fn trying_a_language_on_stores_it_without_answering() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let r = c.post_form("/settings/onboarding/lang", &[("lang", "en"), ("back", "/calendar"), ("try_only", "1")]).await;
    assert_eq!(r.location().as_deref(), Some("/calendar"));
    assert_eq!(prefs::get(&s.app.state.db, "alice").await.unwrap().lang.as_deref(), Some("en"), "the language is stored at once");
    assert!(!onboarding::get(&s.app.state.db, "alice").await.unwrap().has(onboarding::Step::Lang));
    let page = c.get("/calendar").await;
    assert!(page.body.contains(r#"id="onb-gate""#), "the gate is still there, now in English");
    assert!(page.body.contains(r#"<html lang="en""#));
    // and no toast: nothing was decided
    assert!(!page.body.contains(r#"class="toast ok""#));

    // the button answers
    c.post_form("/settings/onboarding/lang", &[("lang", "de"), ("back", "/calendar")]).await;
    assert!(onboarding::get(&s.app.state.db, "alice").await.unwrap().has(onboarding::Step::Lang));
    assert!(!c.get("/calendar").await.body.contains(r#"id="onb-gate""#));
}

/// Migration 17 carries every user who predates the first start over as "through". Without it this
/// release would greet an existing instance with the language gate on every page.
#[tokio::test]
async fn the_users_of_an_existing_instance_are_carried_over_as_through() {
    let s = seed().await;
    let db = &s.app.state.db;
    // back to the state right before the carry-over: the column is there, no record is
    sqlx::raw_sql("DELETE FROM practical_migrations WHERE version = 17; UPDATE practical_user_settings SET onboarding_json = NULL;")
        .execute(db)
        .await
        .unwrap();
    let mut c = s.app.login("alice").await;
    assert!(c.get("/calendar").await.body.contains(r#"id="onb-gate""#), "without the carry-over the gate blocks the page");

    crate::db::migrate(db).await.unwrap();
    assert!(onboarding::get(db, "alice").await.unwrap().completed, "alice owns collections: she was here before");
    assert!(!c.get("/calendar").await.body.contains(r#"id="onb-gate""#));
    // a principal without a single collection is taken at face value as a new user
    for who in ["bob", "carol"] {
        assert!(!onboarding::get(db, who).await.unwrap().completed, "{who} owns nothing");
    }
}

/// The browser asks for the end, so a tab working from a stale list must not get it: the server
/// counts for itself and refuses while a point is open.
#[tokio::test]
async fn the_end_is_refused_while_a_point_is_open() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;   // alice has collections and regions: language and zone are open
    assert_eq!(state(&mut c).await["open"], json!(["lang", "tz"]));
    let r = c.json(Method::POST, "/settings/onboarding/complete", json!(null)).await;
    assert_eq!(r.status, StatusCode::CONFLICT, "two points are still open");
    assert!(!onboarding::get(&s.app.state.db, "alice").await.unwrap().completed);
    assert_eq!(state(&mut c).await["open"], json!(["lang", "tz"]), "and the hints stay");

    c.post_form("/settings/onboarding/lang", &[("lang", "de"), ("back", "/calendar")]).await;
    assert_eq!(c.json(Method::POST, "/settings/onboarding/complete", json!(null)).await.status, StatusCode::CONFLICT, "one still is");
    c.json(Method::POST, "/settings/onboarding/confirm", json!({ "step": "tz" })).await;
    assert_eq!(c.json(Method::POST, "/settings/onboarding/complete", json!(null)).await.status, StatusCode::NO_CONTENT);
    assert!(onboarding::get(&s.app.state.db, "alice").await.unwrap().completed);
}

#[tokio::test]
async fn the_settings_bring_the_hints_back_at_once_and_keep_the_confirmations() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    c.json(Method::POST, "/settings/onboarding/confirm", json!({ "step": "tz" })).await;
    c.post_form("/settings/onboarding/lang", &[("lang", "de"), ("back", "/calendar")]).await;
    assert_eq!(c.json(Method::POST, "/settings/onboarding/complete", json!(null)).await.status, StatusCode::NO_CONTENT);
    assert_eq!(state(&mut c).await["open"], json!([]));
    let r = c.post_form("/settings/onboarding/reopen", &[]).await;
    assert_eq!(r.location().as_deref(), Some("/calendar"));
    assert_eq!(state(&mut c).await["open"], json!([]), "the confirmations stay, so nothing is open again");
    // what reopen gives back is the asking, which a user without collections then sees
    let mut carol = s.app.login("carol").await;
    carol.json(Method::POST, "/settings/onboarding/complete", json!(null)).await;
    carol.post_form("/settings/onboarding/reopen", &[]).await;
    assert_eq!(state(&mut carol).await["open"], json!(["lang", "tz", "regions", "school", "calendar", "addressbook"]));
}

#[tokio::test]
async fn a_finished_setup_reports_once_and_then_stays_quiet() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    for step in ["tz", "regions", "school"] {
        c.json(Method::POST, "/settings/onboarding/confirm", json!({ "step": step })).await;
    }
    c.post_form("/settings/onboarding/lang", &[("lang", "de"), ("back", "/calendar")]).await;
    assert_eq!(state(&mut c).await["open"], json!([]), "alice already has collections");
    // the browser reports it, so no page view has to write
    assert_eq!(c.json(Method::POST, "/settings/onboarding/complete", json!(null)).await.status, StatusCode::NO_CONTENT);
    let page = c.get("/calendar").await;
    assert!(!page.body.contains(r#"id="onboarding""#));
    // and deleting every calendar does not ask again: this was a FIRST start
    assert!(!onboarding::get(&s.app.state.db, "alice").await.unwrap().active());
}

#[tokio::test]
async fn the_hints_need_a_session_of_their_own_user() {
    let s = seed().await;
    let mut anon = s.app.anonymous();
    let r = anon.send(Method::GET, "/api/onboarding", &[], axum::body::Body::empty()).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    // the login page carries no state and no gate
    let page = anon.send(Method::GET, "/login", &[], axum::body::Body::empty()).await;
    assert!(!page.body.contains(r#"id="onb-gate""#));
}

/// A JSON island is `<script type="application/json">…`, and a browser ends that element at the
/// first `</script` in its text — whatever the JSON quoting says. Every island in this app carries
/// user text (task titles, calendar names), so the escape has to hold for all of them.
#[tokio::test]
async fn a_json_island_cannot_close_its_own_script_element() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let bad = "</script><img src=x onerror=alert(1)>";
    c.post_form("/settings/calendars", &[("name", bad), ("owner", "alice"), ("color", "#0a84ff"), ("comp_event", "on"), ("comp_todo", "on")]).await;
    let mut carried = 0;
    for path in ["/tasks", "/settings", "/calendar"] {
        let page = c.get(path).await;
        page.ok();
        assert!(!page.body.to_lowercase().contains("</script><img"), "{path} let a name out of its element");
        // and the name really is on the page, so this is not a test of nothing
        carried += usize::from(page.body.contains("onerror"));
    }
    assert!(carried > 0, "the name reached no page: the test proves nothing");
}

/// A collection of a group the user is in closes its step too (decided 2026-10-12): "create", an
/// import and a sync can all put the first calendar there, and a user whose first calendar is the
/// family's has somewhere to put an appointment. Only the kind the group has counts.
#[tokio::test]
async fn a_group_collection_closes_the_step_of_its_kind() {
    let s = seed().await;
    // bob owns nothing, but group.familie has a calendar and no address book
    let mut bob = s.app.login("bob").await;
    assert_eq!(state(&mut bob).await["open"], json!(["lang", "tz", "regions", "school", "addressbook"]));

    // carol owns nothing and is in no group, until alice adds her
    let mut carol = s.app.login("carol").await;
    assert!(state(&mut carol).await["open"].as_array().unwrap().contains(&json!("calendar")));
    crate::domain::groups::add_member(&s.app.state, &s.app.principal("alice").await, "group.familie", "carol").await.unwrap();
    assert!(!state(&mut carol).await["open"].as_array().unwrap().contains(&json!("calendar")), "the family calendar did not count");
}
