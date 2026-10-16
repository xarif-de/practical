//! /settings: groups with their trash, collections with theirs, app tokens, preferences, the
//! small JSON saves of the other pages, import and export.

use super::seed::seed;
use crate::domain::prefs;
use rustical_store::auth::AuthenticationProvider;
use axum::http::{Method, StatusCode};
use serde_json::json;

#[tokio::test]
async fn groups_life_cycle() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let r = c.post_form("/settings/groups", &[("name", "Verein & Sport")]).await;
    assert_eq!(r.location().as_deref(), Some("/settings#groups"));
    let alice = s.app.principal("alice").await;
    assert!(alice.memberships.contains(&"group.verein-sport".to_string()));
    // the same name again: a conflict, shown as a flash
    c.post_form("/settings/groups", &[("name", "Verein & Sport")]).await;
    c.get("/settings").await.has("<div class=\"toast error\" role=\"alert\"><span>Die Gruppe group.verein-sport gibt es schon.</span>");
    // members
    c.post_form("/settings/groups/group.verein-sport/members", &[("member", "bob")]).await;
    assert_eq!(s.app.state.principals_list("group.verein-sport").await, ["alice", "bob"]);
    c.post_form("/settings/groups/group.verein-sport/members", &[("member", "group.familie")]).await;
    c.get("/settings").await.has("Nur Personen können einer Gruppe beitreten.");
    c.post_form("/settings/groups/group.verein-sport/members/bob/remove", &[]).await;
    assert_eq!(s.app.state.principals_list("group.verein-sport").await, ["alice"]);
    // carol is no member: forbidden
    let mut carol = s.app.login("carol").await;
    carol.post_form("/settings/groups/group.verein-sport/members", &[("member", "carol")]).await;
    carol.get("/settings").await.has("Dafür fehlt die Berechtigung.");
    // delete needs the name typed
    let r = c.post_form_json("/settings/groups/group.verein-sport/delete", &[("confirm", "falsch")]).await;
    assert_eq!(r.ok().json()["ok"], false);
    c.post_form("/settings/groups/group.verein-sport/delete", &[("confirm", "Verein & Sport")]).await;
    let page = c.get("/settings").await;
    page.ok().has("/settings/trash/groups/group.verein-sport/restore");
    assert!(!s.app.principal("alice").await.memberships.contains(&"group.verein-sport".to_string()));
    // restore, then the last member leaves: trashed again with a message
    c.post_form("/settings/trash/groups/group.verein-sport/restore", &[]).await;
    assert!(s.app.principal("alice").await.memberships.contains(&"group.verein-sport".to_string()));
    c.post_form("/settings/groups/group.verein-sport/leave", &[]).await;
    c.get("/settings").await.has("toast ok");
    // purge needs the id typed
    c.post_form("/settings/trash/groups/group.verein-sport/purge", &[("confirm", "Verein")]).await;
    assert!(s.app.state.principals.get_principal_opt("group.verein-sport").await);
    c.post_form("/settings/trash/groups/group.verein-sport/purge", &[("confirm", "group.verein-sport")]).await;
    assert!(!s.app.state.principals.get_principal_opt("group.verein-sport").await);
}

#[tokio::test]
async fn calendars_and_addressbooks_life_cycle() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let user = s.app.principal("alice").await;
    let cals = |deleted| { let st = s.app.state.clone(); let u = user.clone(); async move { crate::domain::collections::calendars_for(&st, &u, deleted).await.unwrap() } };
    let r = c.post_form("/settings/calendars", &[("owner", "alice"), ("name", " Sport "), ("color", "#123456"), ("description", "Training"), ("comp_todo", "on"), ("timezone", "")]).await;
    assert_eq!(r.location().as_deref(), Some("/settings#collections"));
    let sport = cals(false).await.into_iter().find(|c| c.displayname == "Sport").unwrap();
    assert_eq!((sport.color.as_str(), sport.description.as_str(), sport.components.clone(), sport.timezone_id.as_str()), ("#123456", "Training", vec!["VTODO".to_string()], "Europe/Berlin"));
    // no type ticked: events
    c.post_form("/settings/calendars", &[("owner", "alice"), ("name", "Leer")]).await;
    assert_eq!(cals(false).await.into_iter().find(|c| c.displayname == "Leer").unwrap().components, ["VEVENT"]);
    // someone else's principal
    c.post_form("/settings/calendars", &[("owner", "bob"), ("name", "Fremd")]).await;
    assert!(!cals(false).await.iter().any(|c| c.displayname == "Fremd"));
    // the page's autosave answers JSON
    let r = c.post_form_json(&format!("/settings/calendars/alice/{}/update", sport.id), &[("name", "Sport & Spiel"), ("color", ""), ("description", "")]).await;
    assert_eq!(r.ok().json(), json!({ "ok": true, "msg": "Gespeichert." }));
    let sport2 = cals(false).await.into_iter().find(|c| c.id == sport.id).unwrap();
    assert_eq!(sport2.displayname, "Sport & Spiel");
    assert_eq!(sport2.color, crate::domain::collections::default_color(&sport.id), "no colour: the stable fallback");
    // a collection that does not exist: the translated message, not the store's English text
    let r = c.post_form_json("/settings/calendars/alice/missing/update", &[("name", "x")]).await;
    assert_eq!(r.json(), json!({ "ok": false, "msg": "Nicht gefunden." }));
    // trash, restore, purge
    c.post_form(&format!("/settings/calendars/alice/{}/delete", sport.id), &[]).await;
    assert!(cals(true).await.iter().any(|c| c.id == sport.id));
    c.get("/settings").await.has(&format!("/settings/calendars/alice/{}/restore", sport.id));
    c.post_form(&format!("/settings/calendars/alice/{}/restore", sport.id), &[]).await;
    assert!(cals(false).await.iter().any(|c| c.id == sport.id));
    c.post_form(&format!("/settings/calendars/alice/{}/delete", sport.id), &[]).await;
    c.post_form(&format!("/settings/calendars/alice/{}/purge", sport.id), &[]).await;
    assert!(!cals(true).await.iter().any(|c| c.id == sport.id) && !cals(false).await.iter().any(|c| c.id == sport.id));

    // address books: the colour is ours, required
    let abs = |deleted| { let st = s.app.state.clone(); let u = user.clone(); async move { crate::domain::collections::addressbooks_for(&st, &u, deleted).await.unwrap() } };
    let r = c.post_form_json("/settings/addressbooks", &[("owner", "alice"), ("name", "Kunden"), ("color", "blue")]).await;
    assert_eq!(r.json(), json!({ "ok": false, "msg": "Die Farbe muss #rrggbb sein." }));
    c.post_form("/settings/addressbooks", &[("owner", "alice"), ("name", "Kunden"), ("color", "#abcdef")]).await;
    let k = abs(false).await.into_iter().find(|a| a.displayname == "Kunden").unwrap();
    assert_eq!(k.color, "#abcdef");
    let r = c.post_form_json(&format!("/settings/addressbooks/alice/{}/update", k.id), &[("name", "Kundschaft"), ("color", "#000000")]).await;
    assert_eq!(r.json()["ok"], true);
    c.post_form(&format!("/settings/addressbooks/alice/{}/delete", k.id), &[]).await;
    assert!(abs(true).await.iter().any(|a| a.id == k.id));
    c.post_form(&format!("/settings/addressbooks/alice/{}/restore", k.id), &[]).await;
    assert!(abs(false).await.iter().any(|a| a.id == k.id && a.displayname == "Kundschaft"));
    c.post_form(&format!("/settings/addressbooks/alice/{}/delete", k.id), &[]).await;
    c.post_form(&format!("/settings/addressbooks/alice/{}/purge", k.id), &[]).await;
    assert!(!abs(true).await.iter().any(|a| a.id == k.id));
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM practical_addressbook_colors WHERE id = ?").bind(&k.id).fetch_one(&s.app.state.db).await.unwrap();
    assert_eq!(left, 0, "a purge drops the colour row too");
}

#[tokio::test]
async fn app_tokens_are_shown_once() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    c.post_form("/settings/tokens", &[("name", "Thunderbird")]).await;
    let page = c.get("/settings").await;
    let token = page.body.split("<code id=\"new-token\">").nth(1).map(|r| r[..r.find('<').unwrap()].to_string()).expect("token shown");
    let (prefix, secret) = token.split_once('_').unwrap();
    assert_eq!((prefix.len(), secret.len()), (4, 64));
    assert!(!c.get("/settings").await.body.contains("new-token\">"), "only once");
    let tokens = s.app.state.principals.get_app_tokens("alice").await.unwrap();
    let t = tokens.iter().find(|t| t.name == "Thunderbird").unwrap();
    assert!(t.id.starts_with(prefix));
    // it authenticates
    use base64ct::Encoding;
    let h = format!("Basic {}", base64ct::Base64::encode_string(format!("alice:{token}").as_bytes()));
    assert_eq!(s.app.anonymous().send(Method::GET, "/api/colors", &[("authorization", &h)], axum::body::Body::empty()).await.status, StatusCode::OK);
    // empty name, someone else's id, then the real delete
    let r = c.post_form_json("/settings/tokens", &[("name", " ")]).await;
    assert_eq!(r.json(), json!({ "ok": false, "msg": "Bitte einen Namen angeben." }));
    let r = c.post_form_json("/settings/tokens/not-mine/delete", &[]).await;
    assert_eq!(r.json(), json!({ "ok": false, "msg": "Nicht gefunden." }));
    c.post_form(&format!("/settings/tokens/{}/delete", t.id), &[]).await;
    assert!(!s.app.state.principals.get_app_tokens("alice").await.unwrap().iter().any(|x| x.id == t.id));
    assert_eq!(s.app.anonymous().send(Method::GET, "/api/colors", &[("authorization", &h)], axum::body::Body::empty()).await.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_device_shows_once_it_connected_and_one_that_never_did_is_swept() {
    use crate::domain::tokens::{self, Seen};
    let s = seed().await;
    let st = &s.app.state;
    let mut c = s.app.login("alice").await;
    let alice = st.principals.get_principal("alice").await.unwrap().unwrap();
    let named = |n: &'static str| async move { tokens::devices(st, &st.principals.get_principal("alice").await.unwrap().unwrap()).await.unwrap().into_iter().find(|(t, _)| t.name == n) };
    // made, not yet used: no connected device
    c.post_form("/settings/tokens", &[("name", "Thunderbird")]).await;
    let token = c.get("/settings").await.body.split("<code id=\"new-token\">").nth(1).map(|r| r[..r.find('<').unwrap()].to_string()).expect("token shown");
    assert!(named("Thunderbird").await.is_none(), "never connected → hidden");
    // its first use (RustiCal's middleware records it) makes it one
    use base64ct::Encoding;
    let h = format!("Basic {}", base64ct::Base64::encode_string(format!("alice:{token}").as_bytes()));
    assert_eq!(s.app.anonymous().send(Method::GET, "/api/colors", &[("authorization", &h)], axum::body::Body::empty()).await.status, StatusCode::OK);
    assert!(matches!(named("Thunderbird").await, Some((_, Seen::At(_)))));
    assert!(c.get("/settings").await.body.contains("zuletzt verbunden vor Kurzem"));
    // a day later: the never-used one goes, the used one and one from before recording stay
    tokens::create(st, &alice, "Halbfertig").await.unwrap();
    tokens::create(st, &alice, "Alt").await.unwrap();
    tokens::create(st, &alice, "Frisch").await.unwrap(); // never used either, but younger than a day
    for (q, n) in [
        ("UPDATE practical_token_use_since SET since = datetime('now', '-30 hours')", ""),
        ("UPDATE app_tokens SET created_at = datetime('now', '-25 hours') WHERE displayname IN (?, 'Thunderbird')", "Halbfertig"),
        ("UPDATE app_tokens SET created_at = datetime('now', '-40 hours') WHERE displayname = ?", "Alt"),
    ] {
        sqlx::query(q).bind(n).execute(&st.db).await.unwrap();
    }
    assert_eq!(tokens::sweep(st).await.unwrap(), 1);
    let left: Vec<String> = st.principals.get_app_tokens("alice").await.unwrap().into_iter().map(|t| t.name).collect();
    assert!(!left.contains(&"Halbfertig".to_string()) && ["Thunderbird", "Alt", "Frisch"].iter().all(|n| left.contains(&n.to_string())), "{left:?}");
    assert!(matches!(named("Alt").await, Some((_, Seen::Unknown))), "made before uses were recorded: shown, nothing known");
    assert!(c.get("/settings").await.body.contains("verbunden seit"));
}

#[tokio::test]
async fn preferences_form() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let fields = [
        ("lang", "en"), ("scheme", "dark"), ("timezone", " America/New_York "), ("holiday_color", "#AA0000"), ("school_color", "red"),
        ("public_regions", "de-sn, DE-SN,de-by,bad code,x"), ("school_regions", ""), ("vacation_days", "30"), ("vacation_kind", "title"),
        ("vacation_title", "^Urlaub"), ("alarm_snaps_start", "5 min, 1 h"), ("alarm_snaps_end", ""), ("search_history_days", "30"),
    ];
    let r = c.post_form_json("/settings/prefs", &fields).await;
    let answer = r.ok().json();
    assert_eq!(answer["ok"], json!(true));
    assert_eq!(answer["msg"], json!("Saved."), "the answer speaks the language just chosen");
    // the save echoes both holiday lists back, because it may change one the page did not touch
    assert_eq!(answer["regions"], json!({ "public": "DE-SN,DE-BY", "school": "" }));
    let p = prefs::get(&s.app.state.db, "alice").await.unwrap();
    assert_eq!(p.lang.as_deref(), Some("en"));
    assert_eq!(p.scheme.as_deref(), Some("dark"));
    assert_eq!(p.timezone.as_deref(), Some("America/New_York"));
    assert_eq!((p.holiday_color.as_deref(), p.school_color.as_deref()), (Some("#aa0000"), None));
    assert_eq!((p.public_regions.as_deref(), p.school_regions.as_deref()), (Some("DE-SN,DE-BY"), Some("")));
    assert_eq!(p.vacation_days, Some(30));
    assert_eq!(p.vacation_rule_json.as_deref(), Some(r#"{"v":2,"kind":"title","value":"^Urlaub"}"#));
    assert_eq!(p.alarm_snaps_json.as_deref(), Some(r#"{"start":[5,60],"end":[]}"#));
    assert_eq!(p.search_history_days, Some(30));
    assert_eq!(p.holiday_css(), ":root{--hol:#aa0000;}");
    // the page now: English, dark, the zone in the events
    let page = c.get("/settings").await;
    page.ok().has("<html lang=\"en\" data-pref-scheme=\"dark\" data-scheme=\"dark\">").has("America/New York");
    let ev = c.get_json("/api/events?start=2026-10-14&end=2026-10-15").await.json();
    assert_eq!(ev[0]["start"], "2026-10-14T03:00:00", "the standup at 09:00 Berlin is 03:00 in New York");

    // bad values: the snap point names the token, a title pattern must parse; nothing is saved
    let r = c.post_form_json("/settings/prefs", &[("alarm_snaps_start", "5 min, soon")]).await;
    assert_eq!(r.json(), json!({ "ok": false, "msg": "snap point: soon" }));
    let r = c.post_form_json("/settings/prefs", &[("lang", "de"), ("vacation_kind", "title"), ("vacation_title", "(Urlaub")]).await;
    assert_eq!(r.json()["ok"], false);
    assert!(r.json()["msg"].as_str().unwrap().contains("regex parse error"), "{}", r.body);
    assert_eq!(prefs::get(&s.app.state.db, "alice").await.unwrap().lang.as_deref(), Some("en"));
    // an unknown language, scheme and zone fall back; an empty start list is the built-in one, an
    // empty end list stays empty (so it is stored), the built-in pair is stored as NULL
    c.post_form("/settings/prefs", &[("lang", "xx"), ("scheme", "blue"), ("timezone", "Mars/Base"), ("alarm_snaps_start", "")]).await;
    let p = prefs::get(&s.app.state.db, "alice").await.unwrap();
    assert_eq!((p.lang, p.scheme, p.timezone, p.alarm_snaps_json.as_deref()), (None, None, None, Some(r#"{"start":[2,5,15,30,60,180,480,1440,2880,4320,10080,20160],"end":[]}"#)));
    c.post_form("/settings/prefs", &[("alarm_snaps_start", ""), ("alarm_snaps_end", "5, 10, 15, 30, 60")]).await;
    assert_eq!(prefs::get(&s.app.state.db, "alice").await.unwrap().alarm_snaps_json, None);
    // a form post (no JSON) redirects with a flash
    let r = c.post_form("/settings/prefs", &[("vacation_days", "999")]).await;
    assert_eq!(r.location().as_deref(), Some("/settings#prefs"));
    assert_eq!(prefs::get(&s.app.state.db, "alice").await.unwrap().vacation_days, None, "outside 0..=366");
    // the preference went back to "automatic" (NULL): the browser's language (none sent: the default,
    // German) at once — in the answer of that save too — not the "en" saved earlier
    c.get("/settings").await.has("<html lang=\"de\"").has("<div class=\"toast ok\" role=\"status\"><span>Gespeichert.</span>");
}

#[tokio::test]
async fn the_stored_language_wins_and_automatic_follows_the_browser() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let en = [("accept-language", "en-GB,en;q=0.9")];
    let lang_of = |body: &str| body.split("<html lang=\"").nth(1).and_then(|r| r.split('"').next()).unwrap_or("").to_string();
    // no stored choice: the browser's language, else the default
    assert_eq!(lang_of(&c.send(Method::GET, "/settings", &en, axum::body::Body::empty()).await.body), "en");
    assert_eq!(lang_of(&c.get("/settings").await.body), "de");
    // a stored choice beats the browser's
    c.post_form("/settings/prefs", &[("lang", "fr")]).await;
    assert_eq!(lang_of(&c.send(Method::GET, "/settings", &en, axum::body::Body::empty()).await.body), "fr");
    // back to "automatic": at once the browser's language — the save's own answer too
    let csrf = c.csrf.clone();
    let body = format!("lang=&csrf={csrf}");
    let r = c.send(Method::POST, "/settings/prefs", &[("content-type", "application/x-www-form-urlencoded"), ("accept", "application/json"), ("sec-fetch-site", "same-origin"), en[0]], axum::body::Body::from(body)).await;
    assert_eq!((r.json()["ok"].clone(), r.json()["msg"].clone()), (json!(true), json!("Saved.")));
    assert_eq!(prefs::get(&s.app.state.db, "alice").await.unwrap().lang, None);
    assert_eq!(lang_of(&c.send(Method::GET, "/settings", &en, axum::body::Body::empty()).await.body), "en");
    assert_eq!(lang_of(&c.get("/settings").await.body), "de");
    // a fresh login (password or OIDC: the same session afterwards) reads the stored choice again
    c.post_form("/settings/prefs", &[("lang", "es")]).await;
    let mut again = s.app.login("alice").await;
    assert_eq!(lang_of(&again.send(Method::GET, "/calendar", &en, axum::body::Body::empty()).await.body), "es");
}

#[tokio::test]
async fn small_json_saves() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    assert_eq!(c.json(Method::POST, "/settings/holiday-display", json!({ "public": false })).await.status, StatusCode::NO_CONTENT);
    let p = prefs::get(&s.app.state.db, "alice").await.unwrap();
    assert_eq!((p.holidays_enabled, p.school_holidays_enabled), (false, true), "an absent switch is kept");
    // the month view loses its public holidays, keeps the school holidays
    let m = c.get("/calendar?date=2026-10-14").await;
    assert!(!m.body.contains("Tag der Deutschen Einheit"));
    m.has("Herbstferien");
    c.json(Method::POST, "/settings/holiday-display", json!({ "public": true, "school": false })).await;
    let p = prefs::get(&s.app.state.db, "alice").await.unwrap();
    assert_eq!((p.holidays_enabled, p.school_holidays_enabled), (true, false));

    assert_eq!(c.json(Method::POST, "/settings/scheme", json!({ "scheme": "light" })).await.status, StatusCode::NO_CONTENT);
    c.get("/settings").await.has("data-pref-scheme=\"light\" data-scheme=\"light\"");
    c.json(Method::POST, "/settings/scheme", json!({ "scheme": "auto" })).await;
    c.get("/settings").await.has("<html lang=\"de\" data-pref-scheme=\"auto\">");

    // column choices keep known names only, in the given order
    assert_eq!(c.json(Method::POST, "/settings/columns", json!(["email", "bogus", "phone"])).await.ok().json(), json!(["email", "phone"]));
    assert_eq!(c.json(Method::POST, "/settings/list-fields", json!(["note", "x"])).await.ok().json(), json!(["note"]));
    assert_eq!(c.json(Method::POST, "/settings/task-columns", json!(["due", "links", "nope"])).await.ok().json(), json!(["due", "links"]));
    let p = prefs::get(&s.app.state.db, "alice").await.unwrap();
    assert_eq!((p.contact_columns_json.as_deref(), p.contact_list_fields_json.as_deref(), p.task_columns_json.as_deref()), (Some(r#"["email","phone"]"#), Some(r#"["note"]"#), Some(r#"["due","links"]"#)));
}

#[tokio::test]
async fn export_and_import() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let r = c.get(&format!("/settings/export/calendars/alice/{}", s.privat)).await;
    r.ok().has("BEGIN:VCALENDAR").has("PRODID:-//xarif.de//PractiCal//DE").has("SUMMARY:Zahnarzt Dr. Weiß").has("UID:bday");
    assert_eq!(r.header("content-type").as_deref(), Some("text/calendar; charset=utf-8"));
    assert_eq!(r.header("content-disposition").as_deref(), Some("attachment; filename=\"Privat.ics\""));
    assert_eq!(r.body.matches("BEGIN:VEVENT").count(), 4);
    let v = c.get(&format!("/settings/export/addressbooks/alice/{}", s.freunde)).await;
    v.ok().has("FN:Anna Schmidt").has("FN:ACME GmbH");
    assert_eq!(v.header("content-disposition").as_deref(), Some("attachment; filename=\"Freunde.vcf\""));
    assert_eq!(c.get(&format!("/settings/export/calendars/bob/{}", s.privat)).await.status, StatusCode::FORBIDDEN);

    // the export imports into a new calendar: same objects, same UIDs
    let r = c.post_raw("/settings/import/calendars?owner=alice&name=Kopie&color=%23101010&components=VEVENT", &r.body).await;
    let j = r.ok().json();
    assert_eq!((j["owner"].as_str(), j["imported"].as_u64()), (Some("alice"), Some(4)));
    let copy = j["id"].as_str().unwrap().to_string();
    let user = s.app.principal("alice").await;
    let cal = crate::domain::collections::calendars_for(&s.app.state, &user, false).await.unwrap().into_iter().find(|c| c.id == copy).unwrap();
    assert_eq!((cal.displayname.as_str(), cal.color.as_str()), ("Kopie", "#101010"));
    assert_eq!(c.get_json(&format!("/api/events/alice/{copy}/zahnarzt")).await.ok().json()["summary"], "Zahnarzt Dr. Weiß");
    // into an existing calendar: merged by UID
    let one = crate::testkit::vevent("zahnarzt", "Zahnarzt (verschoben)", "DTSTART:20261017T080000Z\nDTEND:20261017T090000Z");
    assert_eq!(c.post_raw(&format!("/settings/import/calendars/alice/{copy}"), &one).await.ok().json(), json!({ "imported": 1 }));
    assert_eq!(c.get_json(&format!("/api/events/alice/{copy}/zahnarzt")).await.json()["summary"], "Zahnarzt (verschoben)");
    let r = c.post_raw(&format!("/settings/import/calendars/alice/{copy}"), "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nEND:VCALENDAR\r\n").await;
    assert_eq!((r.status, r.body.as_str()), (StatusCode::BAD_REQUEST, "Die Datei enthält keine Termine, Aufgaben oder Journaleinträge."));
    let r = c.post_raw(&format!("/settings/import/calendars/alice/{copy}"), "not a calendar").await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    // address books
    let r = c.post_raw("/settings/import/addressbooks?owner=alice&name=Kopie&color=%23202020", &v.body).await;
    let j = r.ok().json();
    assert_eq!(j["imported"], 2);
    let ab = j["id"].as_str().unwrap().to_string();
    let more = "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:neu\r\nFN:Neu Person\r\nN:Person;Neu;;;\r\nEND:VCARD\r\n";
    assert_eq!(c.post_raw(&format!("/settings/import/addressbooks/alice/{ab}"), more).await.ok().json(), json!({ "imported": 1 }));
    assert_eq!(c.get_json(&format!("/api/contacts/alice/{ab}/neu")).await.ok().json()["full_name"], "Neu Person");
    // the import is one change in the history
    let h = c.get_json("/api/history").await.json();
    let imports: Vec<&serde_json::Value> = h["changes"].as_array().unwrap().iter().filter(|x| x["src"] == "import").collect();
    assert_eq!(imports.len(), 4, "{h}");
    assert_eq!(imports[0]["label"], "Kopie");
    assert_eq!(imports[0]["items"].as_array().unwrap().len(), 4);
}

#[tokio::test]
async fn settings_page_reflects_the_vacation_rule() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    // no rule stored: the legacy default "Urlaub" is a category in use → category
    c.get("/settings").await.ok().has("„Urlaub“");
    c.post_form("/settings/prefs", &[("vacation_kind", "calendar"), ("vacation_calendar", &format!("alice/{}", s.privat))]).await;
    c.get("/settings").await.ok().has("„Privat“");
    c.post_form("/settings/prefs", &[("vacation_kind", "calendar"), ("vacation_calendar", "alice/gone")]).await;
    c.get("/settings").await.ok().has("„alice/gone“");
    // the vacation planner follows the rule: by calendar, everything in Privat counts
    c.post_form("/settings/prefs", &[("vacation_kind", "category"), ("vacation_category", "Urlaub"), ("vacation_days", "30")]).await;
    let v = c.get("/calendar?view=vacation&date=2026-10-14").await;
    v.ok().has("title=\"Urlaub Ostsee: 26.10. – 30.10.\"").has("Kategorie „Urlaub“");
}

// ---- small helpers on the state, for read-backs ----

impl crate::state::AppState {
    async fn principals_list(&self, gid: &str) -> Vec<String> {
        let mut m = self.principals.list_members(gid).await.unwrap();
        m.sort();
        m
    }
}

trait PrincipalOpt {
    async fn get_principal_opt(&self, id: &str) -> bool;
}

impl PrincipalOpt for rustical_store_sqlite::SqlitePrincipalStore {
    async fn get_principal_opt(&self, id: &str) -> bool {
        self.get_principal(id).await.unwrap().is_some()
    }
}

/// A filter row's star pins it as a quick switch next to the filter button: one key per save, so the calendar page and
/// the contacts page (each knows only its own rows) keep each other's pins; a key no filter row can have is refused.
#[tokio::test]
async fn filter_pins_are_saved_one_key_at_a_time_and_drawn_on_their_page() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let (cal, ab) = (format!("cal:alice/{}", s.arbeit), format!("ab:alice/{}", s.freunde));
    for key in [&cal, &ab, &"hol:school".to_string()] {
        c.json(Method::POST, "/settings/filter-pin", json!({ "key": key, "on": true })).await.ok();
    }
    assert_eq!(c.json(Method::POST, "/settings/filter-pin", json!({ "key": "hol:school", "on": false })).await.ok().json(), json!([cal, ab]));
    // past MAX_PINS the oldest pin goes (stale pins of deleted collections must not block new ones)
    for n in 0..64 {
        c.json(Method::POST, "/settings/filter-pin", json!({ "key": format!("cal:alice/old{n}"), "on": true })).await.ok();
    }
    let full = c.json(Method::POST, "/settings/filter-pin", json!({ "key": "hol:school", "on": true })).await.ok().json();
    assert_eq!((full.as_array().unwrap().len(), full[0].as_str(), full[63].as_str()), (64, Some("cal:alice/old1"), Some("hol:school")), "{full}");
    for n in 1..64 {
        c.json(Method::POST, "/settings/filter-pin", json!({ "key": format!("cal:alice/old{n}"), "on": false })).await.ok();
    }
    c.json(Method::POST, "/settings/filter-pin", json!({ "key": "hol:school", "on": false })).await.ok();
    for key in [&cal, &ab] {
        c.json(Method::POST, "/settings/filter-pin", json!({ "key": key, "on": true })).await.ok();
    }
    for bad in ["cal:alice", "cal:/x", "hol:other", "x:alice/y", ""] {
        assert_eq!(c.json(Method::POST, "/settings/filter-pin", json!({ "key": bad, "on": true })).await.status, StatusCode::BAD_REQUEST, "{bad}");
    }
    assert_eq!(prefs::get(&s.app.state.db, "alice").await.unwrap().pins(), [cal.clone(), ab.clone()]);
    // each page draws its own rows' switches and stars, nothing of the other page's
    let page = c.get("/calendar").await;
    page.ok().has(&format!("<button class=\"qs\" type=\"button\" data-pin=\"{cal}\" style=\"--c:#0a84ff\" aria-pressed=\"true\" title=\"Arbeit\" aria-label=\"Arbeit\">"));
    page.has(&format!("<button class=\"pinstar\" type=\"button\" data-pin=\"{cal}\" aria-pressed=\"true\""));
    page.has(&format!("<button class=\"pinstar\" type=\"button\" data-pin=\"cal:alice/{}\" aria-pressed=\"false\"", s.privat));
    // every row has its switch, hidden unless pinned (the star shows or hides it in place)
    assert!(!page.body.contains(&ab) && page.body.matches("<button class=\"qs\" ").count() == page.body.matches("\" hidden title=").count() + 1, "{}", page.body);
    page.has(&format!("data-pin=\"cal:alice/{}\" style=\"--c:#30b46c\" aria-pressed=\"true\" hidden title=\"Privat\"", s.privat));
    let page = c.get("/contacts").await;
    page.ok().has(&format!("<button class=\"qs\" type=\"button\" data-pin=\"{ab}\" style=\"--c:#ff2d55\""));
    assert!(!page.body.contains(&cal));
    c.get("/tasks").await.ok().has(&format!("data-pin=\"{cal}\" style=\"--c:#0a84ff\""));
}
