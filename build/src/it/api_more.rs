//! Tasks, contacts, search, search history, push, geocoder, live updates, holiday regions.

use super::seed::seed;
use crate::testkit::TestApp;
use axum::http::{Method, StatusCode};
use serde_json::json;

#[tokio::test]
async fn task_create_tick_update_delete() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let input = json!({ "principal": "alice", "cal_id": s.aufgaben, "summary": "Reifen wechseln", "due": "2026-10-30", "priority": 3, "categories": ["Auto"], "percent": 40 });
    let id = c.json(Method::POST, "/api/tasks", input.clone()).await.ok().json()["id"].as_str().unwrap().to_string();
    let path = format!("/api/tasks/alice/{}/{id}", s.aufgaben);
    let d = c.get_json(&path).await.ok().json();
    assert_eq!((d["summary"].as_str(), d["due"].as_str(), d["due_all_day"].as_bool()), (Some("Reifen wechseln"), Some("2026-10-30T00:00:00"), Some(true)));
    // percent above 0 with no status: in process
    assert_eq!((d["status"].as_str(), d["percent"].as_i64()), (Some("IN-PROCESS"), Some(40)));
    assert_eq!(d["priority"], 3);
    assert_eq!(d["has_rrule"], false);
    // the tick box: completed, then open again
    c.json(Method::PUT, &path, json!({ "principal": "alice", "cal_id": s.aufgaben, "summary": "", "completed": true })).await.ok();
    let done = c.get_json(&path).await.json();
    assert_eq!((done["status"].as_str(), done["percent"].as_i64()), (Some("COMPLETED"), Some(100)));
    assert!(done["completed_at"].is_string());
    assert_eq!(done["summary"], "Reifen wechseln", "a tick keeps everything else");
    c.json(Method::PUT, &path, json!({ "principal": "alice", "cal_id": s.aufgaben, "summary": "", "completed": false })).await.ok();
    let open = c.get_json(&path).await.json();
    assert_eq!(open["completed_at"], json!(null));
    assert_ne!(open["status"], "COMPLETED");
    // a full save; a stale ETag conflicts; another calendar is refused
    let mut upd = input.clone();
    upd["summary"] = json!("Winterreifen");
    upd["etag"] = open["etag"].clone();
    c.json(Method::PUT, &path, upd.clone()).await.ok();
    assert_eq!(c.json(Method::PUT, &path, upd.clone()).await.status, StatusCode::CONFLICT);
    upd["cal_id"] = json!(s.arbeit);
    upd["etag"] = c.get_json(&path).await.json()["etag"].clone();
    let r = c.json(Method::PUT, &path, upd).await;
    assert_eq!((r.status, r.body.as_str()), (StatusCode::BAD_REQUEST, "Eine Aufgabe kann den Kalender nicht wechseln."));
    c.delete(&path).await.ok();
    assert_eq!(c.get_json(&path).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn done_tasks_are_paged() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let p = c.get_json("/api/tasks/done?back=all&offset=0&limit=1").await.ok().json();
    assert_eq!((p["total"].as_u64(), p["offset"].as_u64()), (Some(2), Some(0)));
    assert_eq!(p["items"].as_array().unwrap().len(), 1);
    assert_eq!(p["items"][0]["summary"], "Garage ausmisten");
    let p = c.get_json("/api/tasks/done?back=all&offset=1&limit=1").await.json();
    assert_eq!(p["items"][0]["summary"], "Fenster putzen");
    assert_eq!(p["items"][0]["completed_at"], "2026-10-01T12:00:00", "COMPLETED is UTC, shown in the user's zone");
    // offset beyond the end is clamped, limit 0 becomes 1
    let p = c.get_json("/api/tasks/done?back=all&offset=99&limit=0").await.json();
    assert_eq!((p["offset"].as_u64(), p["items"].as_array().unwrap().len()), (Some(2), 0));
    // the task detail of a seeded VTODO
    let d = c.get_json(&format!("/api/tasks/alice/{}/steuer", s.aufgaben)).await.ok().json();
    assert_eq!((d["priority"].as_i64(), d["categories"].clone()), (Some(1), json!(["Haushalt"])));
}

#[tokio::test]
async fn contact_create_read_vcf_update_move_delete() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let input = json!({ "principal": "alice", "ab_id": s.freunde, "given": "Jörg", "family": "Öztürk", "emails": [{ "kind": "work", "value": "joerg@example.org", "pref": true }], "phones": [{ "kind": "cell", "value": "+49 151 000" }], "birthday": "--05-17", "note": "Zeile 1\nZeile 2", "categories": ["Verein"] });
    let id = c.json(Method::POST, "/api/contacts", input.clone()).await.ok().json()["id"].as_str().unwrap().to_string();
    let path = format!("/api/contacts/alice/{}/{id}", s.freunde);
    let d = c.get_json(&path).await.ok().json();
    assert_eq!((d["full_name"].as_str(), d["given"].as_str(), d["family"].as_str()), (Some("Jörg Öztürk"), Some("Jörg"), Some("Öztürk")));
    assert_eq!(d["emails"], json!([{ "kind": "work", "value": "joerg@example.org", "pref": true }]));
    assert_eq!(d["birthday"], "--05-17");
    assert_eq!(d["note"], "Zeile 1\nZeile 2");
    assert_eq!(d["ab_name"], "Freunde");
    let vcf = c.get_json(&format!("{path}/vcf")).await;
    vcf.ok().has("BEGIN:VCARD\r\nVERSION:3.0\r\n").has("EMAIL;TYPE=WORK,PREF:joerg@example.org").has("BDAY:--0517").has("NOTE:Zeile 1\\nZeile 2");
    assert_eq!(vcf.header("content-type").as_deref(), Some("text/vcard; charset=utf-8"));
    assert_eq!(vcf.header("content-disposition"), Some(format!("attachment; filename=\"{id}.vcf\"")));
    // update keeps lines it does not model (a photo written by another client)
    let with_photo = vcf.body.replace("END:VCARD", "PHOTO;ENCODING=b;TYPE=JPEG:/9j/4AAQ\r\nEND:VCARD");
    s.app.put_vcf("alice", &s.freunde, &id, &with_photo).await;
    let mut upd = input.clone();
    upd["nickname"] = json!("Jo");
    upd["etag"] = c.get_json(&path).await.json()["etag"].clone();
    c.json(Method::PUT, &path, upd.clone()).await.ok();
    c.get_json(&format!("{path}/vcf")).await.has("NICKNAME:Jo").has("PHOTO;ENCODING=b;TYPE=JPEG:/9j/4AAQ");
    upd["etag"] = json!("stale");
    assert_eq!(c.json(Method::PUT, &path, upd.clone()).await.status, StatusCode::CONFLICT);
    // move to a second address book keeps the UID
    let other = s.app.addressbook("alice", "Arbeit", "#000000").await;
    upd["ab_id"] = json!(other);
    upd["etag"] = c.get_json(&path).await.json()["etag"].clone();
    let new_id = c.json(Method::PUT, &path, upd).await.ok().json()["id"].as_str().unwrap().to_string();
    assert_eq!(new_id, id);
    assert_eq!(c.get_json(&path).await.status, StatusCode::NOT_FOUND);
    c.delete(&format!("/api/contacts/alice/{other}/{id}")).await.ok();
    // errors
    let r = c.json(Method::POST, "/api/contacts", json!({ "principal": "alice", "ab_id": s.freunde })).await;
    assert_eq!((r.status, r.body.as_str()), (StatusCode::BAD_REQUEST, "Bitte einen Namen angeben."));
    let r = c.json(Method::POST, "/api/contacts", json!({ "principal": "bob", "ab_id": s.freunde, "given": "x" })).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn search_api_and_history() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    let r = c.get_json("/api/search?q=zahn").await.ok().json();
    assert_eq!(r["totals"], json!({ "events": 1, "contacts": 0, "tasks": 0 }));
    assert_eq!(r["events"][0]["summary"], "Zahnarzt Dr. Weiß");
    assert_eq!(r["events"][0]["date"], "2026-10-16");
    assert_eq!(r["events"][0]["time"], "08:30");
    assert_eq!(r["events"][0]["why"][0]["hits"][0], json!({ "w": "zahn", "k": "prefix" }));
    let r = c.get_json("/api/search?q=anna").await.json();
    assert_eq!(r["contacts"][0]["name"], "Anna Schmidt");
    let r = c.get_json("/api/search?q=steuer").await.json();
    assert_eq!(r["tasks"][0]["summary"], "Steuererklärung");
    // a date jumps
    let r = c.get_json("/api/search?q=14.10.2026").await.json();
    assert_eq!(r["jump"]["date"], "2026-10-14");
    // the all-results dialog allows more than five
    assert!(c.get_json("/api/search?q=e&all=1").await.ok().json().is_object());

    // history: record, list, forget one, clear
    assert_eq!(c.get_json("/api/search/history").await.ok().json(), json!([]));
    assert_eq!(c.json(Method::POST, "/api/search/history", json!({ "q": "Zahnarzt", "target": { "key": "x" } })).await.ok().json(), json!(true));
    c.json(Method::POST, "/api/search/history", json!({ "q": "zahnarzt" })).await.ok();
    c.json(Method::POST, "/api/search/history", json!({ "q": "Steuer" })).await.ok();
    let h = c.get_json("/api/search/history").await.json();
    let qs: Vec<&str> = h.as_array().unwrap().iter().map(|e| e["q"].as_str().unwrap()).collect();
    assert_eq!(qs.len(), 2, "the same folded query is one entry: {h}");
    let zahn = h.as_array().unwrap().iter().find(|e| e["q"].as_str().unwrap().eq_ignore_ascii_case("zahnarzt")).unwrap();
    assert_eq!(zahn["uses"], 2);
    let id = zahn["id"].as_i64().unwrap();
    c.delete(&format!("/api/search/history/{id}")).await.ok();
    assert_eq!(c.get_json("/api/search/history").await.json().as_array().unwrap().len(), 1);
    c.delete("/api/search/history").await.ok();
    assert_eq!(c.get_json("/api/search/history").await.json(), json!([]));
    let r = c.json(Method::POST, "/api/search/history", json!({ "q": "x".repeat(201) })).await;
    assert_eq!((r.status, r.body.as_str()), (StatusCode::BAD_REQUEST, "Zu lang."));
    // with the history switched off nothing is kept and the answer says so
    c.post_form("/settings/prefs", &[("search_history_days", "0")]).await;
    assert_eq!(c.json(Method::POST, "/api/search/history", json!({ "q": "a" })).await.json(), json!(false));
    assert_eq!(c.get_json("/api/search/history").await.json(), json!([]));
}

#[tokio::test]
async fn push_without_vapid() {
    let app = TestApp::new().await;
    app.user("alice", "Alice").await;
    let mut c = app.login("alice").await;
    assert_eq!(c.get_json("/api/push/key").await.ok().json(), json!({ "key": null }));
    let r = c.json(Method::POST, "/api/push/subscriptions", json!({ "endpoint": "https://push.example.org/x", "keys": { "p256dh": "a", "auth": "b" } })).await;
    assert_eq!((r.status, r.body.as_str()), (StatusCode::BAD_REQUEST, "Benachrichtigungen sind auf diesem Server ausgeschaltet."));
    assert_eq!(c.get_json("/api/push/subscriptions").await.ok().json(), json!([]));
    assert_eq!(c.send(Method::DELETE, "/api/push/subscriptions", &[("content-type", "application/json"), ("sec-fetch-site", "same-origin"), ("x-csrf-token", &c.csrf.clone())], axum::body::Body::from(r#"{"endpoint":"https://push.example.org/x"}"#)).await.json(), json!({ "id": "" }));
}

#[tokio::test]
async fn push_with_vapid() {
    let kp = rustical_dav_push::VapidKeypair::generate_p256();
    let pem = kp.to_pem().unwrap();
    let app = TestApp::new().await;
    // the keypair lives in the state, not in the config: rebuild the state around it
    let state = std::sync::Arc::new(crate::state::AppState {
        config: app.state.config.clone(),
        db: app.state.db.clone(),
        principals: app.state.principals.clone(),
        calendars: app.state.calendars.clone(),
        addressbooks: app.state.addressbooks.clone(),
        i18n: app.state.i18n.clone(),
        changes: app.state.changes.clone(),
        dev_reload: app.state.dev_reload.clone(),
        http: app.state.http.clone(),
        geo: Default::default(),
        search_index: Default::default(),
        task_cache: Default::default(),
        caches: Default::default(),
        history: Default::default(),
        vapid: Some(rustical_dav_push::VapidKeypair::from_pem(&pem).unwrap()),
        started_at: app.state.started_at,
        guards: Default::default(),
    });
    let router = crate::build_router(&state).unwrap();
    app.user("alice", "Alice").await;
    let mut c = crate::testkit::Client::for_router(router);
    c.login_as("alice").await;
    let key = c.get_json("/api/push/key").await.ok().json();
    assert_eq!(key["key"].as_str().unwrap(), kp.public().encode_b64().0);
    let r = c.json(Method::POST, "/api/push/subscriptions", json!({ "endpoint": "http://push.example.org/x", "keys": { "p256dh": "a", "auth": "b" } })).await;
    assert_eq!((r.status, r.body.as_str()), (StatusCode::BAD_REQUEST, "Der Endpunkt muss https sein."));
    // the server POSTs to the endpoint — only a browser push service, with keys
    // the encryption can use (the endpoint cut mid-character used to panic every reminder round)
    let (p256dh, auth) = (crate::domain::push::guard::P256DH, crate::domain::push::guard::AUTH);
    for e in ["https://push.example.org/x", "https://127.0.0.1:4000/", "https://127.0.0.1:4598/xxxxxxxxxxxxxxxxä", "https://plan.xarif.de/api/events", "https://fcm.googleapis.com.evil.example/x"] {
        let r = c.json(Method::POST, "/api/push/subscriptions", json!({ "endpoint": e, "keys": { "p256dh": p256dh, "auth": auth } })).await;
        assert_eq!((r.status, r.body.as_str()), (StatusCode::BAD_REQUEST, "Das ist keine Push-Adresse eines Browsers."), "{e}");
    }
    let r = c.json(Method::POST, "/api/push/subscriptions", json!({ "endpoint": "https://fcm.googleapis.com/fcm/send/x", "keys": { "p256dh": "a", "auth": "b" } })).await;
    assert_eq!((r.status, r.body.as_str()), (StatusCode::BAD_REQUEST, "Die Schlüssel der Push-Anmeldung sind ungültig."));
    assert_eq!(c.get_json("/api/push/subscriptions").await.json(), json!([]));
    let r = c.json(Method::POST, "/api/push/subscriptions", json!({ "endpoint": "https://fcm.googleapis.com/fcm/send/x", "keys": { "p256dh": p256dh, "auth": auth }, "user_agent": "Test" })).await;
    assert_eq!(r.ok().json()["id"].as_str().unwrap().len(), 36);
    let list = c.get_json("/api/push/subscriptions").await.json();
    assert_eq!((list[0]["endpoint"].as_str(), list[0]["user_agent"].as_str()), (Some("https://fcm.googleapis.com/fcm/send/x"), Some("Test")));
    c.send(Method::DELETE, "/api/push/subscriptions", &[("content-type", "application/json"), ("sec-fetch-site", "same-origin"), ("x-csrf-token", &c.csrf.clone())], axum::body::Body::from(r#"{"endpoint":"https://fcm.googleapis.com/fcm/send/x"}"#)).await.ok();
    assert_eq!(c.get_json("/api/push/subscriptions").await.json(), json!([]));
    // a bounded list per user: the eleventh device drops the oldest; a renewed one keeps its place
    for i in 0..11 {
        let e = format!("https://updates.push.services.mozilla.com/wpush/v2/{i}");
        c.json(Method::POST, "/api/push/subscriptions", json!({ "endpoint": e, "keys": { "p256dh": p256dh, "auth": auth }, "user_agent": "u".repeat(400) })).await.ok();
        if i == 10 {
            c.json(Method::POST, "/api/push/subscriptions", json!({ "endpoint": "https://updates.push.services.mozilla.com/wpush/v2/1", "keys": { "p256dh": p256dh, "auth": auth } })).await.ok();
        }
    }
    let list = c.get_json("/api/push/subscriptions").await.json();
    let ends: Vec<&str> = list.as_array().unwrap().iter().map(|s| s["endpoint"].as_str().unwrap().rsplit('/').next().unwrap()).collect();
    assert_eq!((ends.len(), ends.contains(&"0"), ends.contains(&"1")), (10, false, true), "{ends:?}");
    let two = list.as_array().unwrap().iter().find(|s| s["endpoint"].as_str().unwrap().ends_with("/2")).unwrap();
    assert_eq!(two["user_agent"].as_str().unwrap().len(), 300);
    // the page offers push now
    c.get("/settings").await.ok();
    // logout drops what this session subscribed, not alice's other devices
    sqlx::query("INSERT INTO practical_push_subscriptions (id, principal, endpoint, p256dh, auth) VALUES ('o', 'alice', 'https://web.push.apple.com/other-device', ?, ?)").bind(p256dh).bind(auth).execute(&app.state.db).await.unwrap();
    let csrf = c.csrf.clone();
    assert_eq!(c.post_form_raw("/logout", &[("csrf", &csrf)]).await.location().as_deref(), Some("/login?manual=1"));
    let left: Vec<String> = sqlx::query_scalar("SELECT endpoint FROM practical_push_subscriptions").fetch_all(&app.state.db).await.unwrap();
    assert_eq!(left, ["https://web.push.apple.com/other-device"]);
}

#[tokio::test]
async fn geocoder_without_network() {
    let app = TestApp::new().await;
    app.user("alice", "Alice").await;
    let mut c = app.login("alice").await;
    // too short: answered without asking anyone
    assert_eq!(c.get_json("/api/geo/search?q=ab").await.ok().json(), json!([]));
    let r = c.get_json("/api/geo/reverse?lat=91&lon=0").await;
    assert_eq!((r.status, r.body.as_str()), (StatusCode::BAD_REQUEST, "Breite/Länge außerhalb des gültigen Bereichs."));
    // the provider cannot be reached: a 500, quickly; the session reads the translated message (the
    // geocoder's own error goes to the log)
    let t = std::time::Instant::now();
    let r = c.get_json("/api/geo/search?q=Dresden").await;
    assert_eq!(r.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(r.body, "Das hat nicht geklappt – die Einzelheiten stehen im Server-Log.");
    assert!(t.elapsed() < std::time::Duration::from_secs(5));
}

#[tokio::test]
async fn live_update_stream_is_sse() {
    let app = TestApp::new().await;
    app.user("alice", "Alice").await;
    let mut c = app.login("alice").await;
    let (status, headers) = c.open("/api/changes").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers.get("content-type").unwrap(), "text/event-stream");
    let (status, _) = app.anonymous().open("/api/changes").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // the browser's EventSource: the session's end comes as an event (a 401 never reaches the page), then the stream ends
    let r = app.anonymous().send(Method::GET, "/api/changes", &[("accept", "text/event-stream")], axum::body::Body::empty()).await;
    assert_eq!((r.status, r.header("content-type").as_deref(), r.body.as_str()), (StatusCode::OK, Some("text/event-stream"), "event: auth\ndata: login\n\n"));
    assert_eq!(r.header("set-cookie"), None);
}

#[tokio::test]
async fn holiday_regions() {
    let app = TestApp::new().await;
    app.user("alice", "Alice").await;
    let mut c = app.login("alice").await;
    let countries = c.get_json("/api/holidays/countries").await.ok().json();
    let de = countries.as_array().unwrap().iter().find(|r| r["code"] == "DE").unwrap();
    assert_eq!(de, &json!({ "code": "DE", "name": "Deutschland", "parent": "", "depth": 0, "category": "", "school": true }));
    // sorted by name without accents: Ägypten among the A's
    assert_eq!(countries[0]["code"], "EG");
    // the cached subdivision list (seeded for de/en); French is not cached and the source is
    // unreachable: the built-in CLDR tree answers
    assert_eq!(c.get_json("/api/holidays/subdivisions?country=de").await.ok().json(), json!([{ "code": "DE-SN", "name": "Sachsen", "parent": "DE", "depth": 0, "category": "", "school": false }]));
    c.post_form("/settings/prefs", &[("lang", "fr")]).await;
    let fr = c.get_json("/api/holidays/subdivisions?country=DE").await.ok().json();
    let codes: Vec<&str> = fr.as_array().unwrap().iter().map(|r| r["code"].as_str().unwrap()).collect();
    assert_eq!(codes.len(), 16, "{codes:?}");
    assert!(codes.contains(&"DE-SN"));
    // a code no source covers: nothing
    assert_eq!(c.get_json("/api/holidays/subdivisions?country=XX").await.ok().json(), json!([]));
}

#[tokio::test]
async fn a_card_with_bare_parameters_shows_everywhere() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    // the contact cache is warm: the card below arrives through the change log
    c.get("/contacts").await.ok().has("Anna Schmidt");
    // a vCard 2.1-style card as stored before RustiCal parsed what it stores: caldata rejects it, the
    // store hides it (skip_broken) and fails a change list holding it
    let v21 = "BEGIN:VCARD\r\nVERSION:2.1\r\nUID:otto\r\nN:Alt;Otto;;;\r\nFN:Otto Alt\r\nTEL;CELL;VOICE:0151 222\r\nEMAIL;INTERNET:otto@example.org\r\nBDAY:1972-02-29\r\nEND:VCARD\r\n";
    s.app.put_vcf("alice", &s.freunde, "otto", "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:otto\r\nFN:Otto\r\nEND:VCARD\r\n").await;
    sqlx::query("UPDATE addressobjects SET vcf = ? WHERE id = 'otto'").bind(v21).execute(&s.app.state.db).await.unwrap();
    assert!(rustical_store::AddressbookReadStore::get_objects(&*s.app.state.addressbooks, "alice", &s.freunde).await.unwrap().iter().all(|(id, _)| id != "otto"), "the store alone leaves it out");
    // list, table, search, the birthdays of the year views, the detail, the download
    c.get("/contacts").await.ok().has("Otto Alt").has("29.02.1972");
    c.get("/contacts?view=table").await.ok().has("0151 222").has("otto@example.org");
    let hits = c.get_json("/api/search?q=Otto").await.ok().json();
    assert_eq!(hits["contacts"][0]["name"], "Otto Alt");
    let abs = crate::domain::collections::addressbooks_for(&s.app.state, &s.app.principal("alice").await, false).await.unwrap();
    assert!(crate::domain::contacts::contacts_for(&s.app.state, &abs).await.unwrap().iter().any(|x| x.full_name == "Otto Alt" && x.birthday == "1972-02-29"));
    let path = format!("/api/contacts/alice/{}/otto", s.freunde);
    let d = c.get_json(&path).await.ok().json();
    assert_eq!((d["phones"].clone(), d["emails"][0]["value"].clone()), (json!([{ "kind": "cell", "value": "0151 222", "pref": false }]), json!("otto@example.org")));
    assert_eq!(c.get_json(&format!("{path}/vcf")).await.ok().body, v21, "the stored card, as stored");
    // reading never rewrites it; a save from here does (as vCard 3.0, which caldata reads)
    let stored = || async { sqlx::Row::get::<String, _>(&sqlx::query("SELECT vcf FROM addressobjects WHERE id = 'otto'").fetch_one(&s.app.state.db).await.unwrap(), "vcf") };
    assert_eq!(stored().await, v21);
    let mut upd = d.clone();
    upd["nickname"] = json!("Ottilein");
    c.json(Method::PUT, &path, upd).await.ok();
    let saved = stored().await;
    assert!(saved.contains("NICKNAME:Ottilein") && saved.contains("TEL;TYPE=CELL:0151 222") && rustical_ical::AddressObject::from_vcf(saved.clone()).is_ok(), "{saved}");
}

#[tokio::test]
async fn own_error_texts_speak_the_session_language_api_clients_read_english() {
    let s = seed().await;
    let body = json!({ "principal": "alice", "ab_id": s.freunde });
    // an app token (Basic auth): the English text, as before
    let mut b = s.app.basic("alice").await;
    let r = b.json(Method::POST, "/api/contacts", body.clone()).await;
    assert_eq!((r.status, r.body.as_str()), (StatusCode::BAD_REQUEST, "bad request: name required"));
    // a browser session: its page language
    let mut c = s.app.login("alice").await;
    assert_eq!(c.json(Method::POST, "/api/contacts", body.clone()).await.body, "Bitte einen Namen angeben.");
    for (lang, text) in [("en", "name required"), ("fr", "Indiquez un nom."), ("es", "Indica un nombre.")] {
        c.post_form("/settings/prefs", &[("lang", lang)]).await;
        let r = c.json(Method::POST, "/api/contacts", body.clone()).await;
        assert_eq!((r.status, r.body.as_str()), (StatusCode::BAD_REQUEST, text), "{lang}");
    }
}

#[tokio::test]
async fn download_names_outside_ascii_are_sent_as_rfc6266_filename_star() {
    let s = seed().await;
    let mut c = s.app.login("alice").await;
    // a contact whose object id (the .vcf file name) is not ASCII, as another DAV client may name it
    s.app.put_vcf("alice", &s.freunde, "jörg \"ö\".vcf", "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:joerg\r\nFN:Jörg\r\nN:;Jörg;;;\r\nEND:VCARD\r\n").await;
    let r = c.get_json(&format!("/api/contacts/alice/{}/j%C3%B6rg%20%22%C3%B6%22.vcf/vcf", s.freunde)).await;
    r.ok().has("FN:Jörg");
    assert_eq!(r.header("content-disposition").as_deref(), Some("attachment; filename=\"j_rg ___.vcf.vcf\"; filename*=UTF-8''j%C3%B6rg%20%22%C3%B6%22.vcf.vcf"));
    // an address book named outside ASCII: its export
    let ab = s.app.addressbook("alice", "Grüße", "#000000").await;
    let v = c.get(&format!("/settings/export/addressbooks/alice/{ab}")).await;
    v.ok();
    assert_eq!(v.header("content-disposition").as_deref(), Some("attachment; filename=\"Gr__e.vcf\"; filename*=UTF-8''Gr%C3%BC%C3%9Fe.vcf"));
}
