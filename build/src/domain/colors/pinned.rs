//! Characterisation of the colour sources and the optimiser's batch on the real store.

use super::*;
use crate::testkit::{TestApp, vevent, vtodo};

#[tokio::test]
async fn sources_and_batch() {
    let app = TestApp::new().await;
    let alice = app.user("alice", "Alice").await;
    let cal = app.calendar("alice", "K", "#0a84ff", &["VEVENT", "VTODO"]).await;
    app.put_ics("alice", &cal, "e", &vevent("e", "E", "DTSTART:20261001T100000Z\nCOLOR:Tomato\nCATEGORIES:Sport")).await;
    app.put_ics("alice", &cal, "t", &vtodo("t", "T", "COLOR:#00FF00\nCATEGORIES:Haus")).await;
    app.put_ics("alice", &cal, "n", &vevent("n", "N", "DTSTART:20261001T100000Z\nCOLOR:no good!")).await;
    crate::domain::catrules::put(&app.state.db, "alice", "event", "Sport", &serde_json::from_value(serde_json::json!({ "when": { "color": { "mode": "default", "value": "#ff0000" } } })).unwrap()).await.unwrap();
    crate::domain::catrules::put(&app.state.db, "alice", "task", "Haus", &serde_json::from_value(serde_json::json!({ "when": { "color": { "mode": "lock", "value": "#0000ff" } } })).unwrap()).await.unwrap();
    let s = sources(&app.state, &alice).await.unwrap();
    let mut v = serde_json::to_value(&s).unwrap();
    for x in v.as_array_mut().unwrap() {
        if let Some(k) = x["key"].as_str() {
            x["key"] = serde_json::json!(k.replace(&cal, "<cal>"));
        }
    }
    let want = serde_json::json!([
        { "kind": "calendar", "name": "K", "color": "#0a84ff", "key": "alice/<cal>", "editable": true },
        { "kind": "holiday", "name": "", "color": "", "key": "", "editable": true },
        { "kind": "school", "name": "", "color": "", "key": "", "editable": true },
        { "kind": "category", "name": "Sport", "color": "#ff0000", "key": "Sport", "editable": true },
        { "kind": "category-task", "name": "Haus", "color": "#0000ff", "key": "Haus", "editable": true },
        { "kind": "event", "name": "E", "color": "Tomato", "key": "alice/<cal>/e", "cal": "K", "cal_color": "#0a84ff", "editable": true, "cats": ["Sport"] },
        { "kind": "task", "name": "T", "color": "#00FF00", "key": "alice/<cal>/t", "cal": "K", "cal_color": "#0a84ff", "editable": true, "cats": ["Haus"] },
    ]);
    // collections, holiday colours and rules in this order; the objects after them in store order
    assert_eq!(v.as_array().unwrap()[..5], want.as_array().unwrap()[..5]);
    let mut got: Vec<String> = v.as_array().unwrap()[5..].iter().map(|x| x.to_string()).collect();
    got.sort();
    let mut want_objs: Vec<String> = want.as_array().unwrap()[5..].iter().map(|x| x.to_string()).collect();
    want_objs.sort();
    assert_eq!(got, want_objs, "an invalid COLOR is no source");
    assert_eq!(category_objects(&app.state, &alice, "event", " Sport ", "TOMATO").await.unwrap(), [format!("alice/{cal}/e")]);
    assert!(category_objects(&app.state, &alice, "task", "Sport", "tomato").await.unwrap().is_empty());
    let ch = |kind: &str, key: &str, color: &str| Change { kind: kind.into(), key: key.into(), color: color.into() };
    let (done, failed) = apply(&app.state, &alice, &[
        ch("calendar", &format!("alice/{cal}"), "#ABCDEF"),
        ch("event", &format!("alice/{cal}/e"), ""),
        ch("task", &format!("alice/{cal}/t"), "#111111"),
        ch("category", "Sport", "#222222"),
        ch("category-task", "Haus", "bad"),
        ch("category", "Unbekannt", "#333333"),
        ch("holiday", "", "#444444"),
        ch("school", "", ""),
        ch("calendar", "too/many/parts", "#000000"),
        ch("weather", "", "#000000"),
    ]).await.unwrap();
    assert_eq!((done, failed), (6, 4));
    let s = sources(&app.state, &alice).await.unwrap();
    let get = |kind: &str, name: &str| s.iter().find(|x| x.kind == kind && x.name == name).map(|x| x.color.clone());
    assert_eq!(get("calendar", "K").as_deref(), Some("#abcdef"));
    assert_eq!(get("event", "E"), None, "colour removed: no longer a source");
    assert_eq!(get("task", "T").as_deref(), Some("#111111"));
    assert_eq!(get("category", "Sport").as_deref(), Some("#222222"));
    assert_eq!(get("category-task", "Haus").as_deref(), Some("#0000ff"));
    assert_eq!(get("holiday", "").as_deref(), Some("#444444"));
    // a subscribed calendar is read-only
    // (the store's update_calendar does not write subscription_url: set it as a subscription would)
    sqlx::query("UPDATE calendars SET subscription_url = 'https://example.org/feed.ics' WHERE id = ?").bind(&cal).execute(&app.state.db).await.unwrap();
    assert!(matches!(set_object_color(&app.state, &alice, "alice", &cal, "t", "#000000").await, Err(AppError::BadRequest(m)) if m == "a subscribed calendar is read-only"));
    assert!(matches!(set_object_color(&app.state, &alice, "alice", &cal, "t", "red").await, Err(AppError::BadRequest(m)) if m == "color must be #rrggbb or empty"));
    assert!(category_objects(&app.state, &alice, "task", "Haus", "#111111").await.unwrap().is_empty(), "subscribed calendars are skipped");
}
