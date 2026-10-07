//! Characterisation of category rules: validation branches and the stored map.

use super::*;
use crate::testkit::TestApp;

fn rule(j: serde_json::Value) -> Rule {
    serde_json::from_value(j).unwrap()
}

#[test]
fn validation() {
    let ok = |j| validate(&rule(j)).is_ok();
    assert!(ok(serde_json::json!({})));
    assert!(ok(serde_json::json!({ "title": { "mode": "lock", "value": "Training" } })));
    assert!(!ok(serde_json::json!({ "title": { "mode": "lock", "value": " " } })));
    assert!(!ok(serde_json::json!({ "title": { "mode": "lock", "value": "x".repeat(201) } })));
    assert!(!ok(serde_json::json!({ "when": { "color": { "mode": "default", "value": "not a colour" } } })));
    assert!(ok(serde_json::json!({ "when": { "calendar": { "mode": "default", "value": "p/c" }, "all_day": { "mode": "lock", "value": false } } })));
    assert!(ok(serde_json::json!({ "where": { "mode": "default", "location": "Halle", "geo": [51.0, 13.0] } })));
    assert!(ok(serde_json::json!({ "repeat": { "mode": "default", "rrule": "FREQ=WEEKLY" } })));
    assert!(ok(serde_json::json!({ "repeat": { "mode": "lock", "rrule": "" } })), "an empty rule is not parsed");
    assert!(!ok(serde_json::json!({ "status": { "priority": { "mode": "fixed", "value": 1 } } })));
    assert!(!ok(serde_json::json!({ "alarm": { "alarms": (0..21).map(|_| serde_json::json!({ "mode": "default", "minutes": 5 })).collect::<Vec<_>>() } })));
    assert!(!ok(serde_json::json!({ "links": { "links": [{ "mode": "default", "kind": "attach", "embedded": true }] } })));
    assert!(!ok(serde_json::json!({ "links": { "links": [{ "mode": "default", "kind": "attach", "data": "aGk=" }] } })));
    assert!(valid_scope("event") && valid_scope("task") && !valid_scope("journal"));
}

#[tokio::test]
async fn stored_per_user_and_scope() {
    let app = TestApp::new().await;
    let db = &app.state.db;
    let r = rule(serde_json::json!({ "title": { "mode": "default", "value": "T" } }));
    put(db, "u", "event", " Sport ", &r).await.unwrap();
    put(db, "u", "task", "Sport", &Rule::default()).await.unwrap();
    put(db, "v", "event", "Sport", &Rule::default()).await.unwrap();
    for bad in ["", "a,b", "x\ny", &"x".repeat(81)] {
        assert!(matches!(put(db, "u", "event", bad, &r).await, Err(AppError::BadRequest(m)) if m == "invalid category name"), "{bad:?}");
    }
    assert!(matches!(put(db, "u", "journal", "X", &r).await, Err(AppError::BadRequest(m)) if m == "unknown rule scope"));
    let all_u = all(db, "u", "event").await.unwrap();
    assert_eq!(all_u.keys().collect::<Vec<_>>(), ["Sport"], "trimmed");
    assert_eq!(all_u["Sport"].title.as_ref().unwrap().value, "T");
    // an update replaces, a stored row that no longer parses is skipped
    put(db, "u", "event", "Sport", &Rule::default()).await.unwrap();
    assert!(all(db, "u", "event").await.unwrap()["Sport"].title.is_none());
    sqlx::query("INSERT INTO practical_category_rules (principal, scope, category, rule_json) VALUES ('u', 'event', 'Kaputt', 'not json')").execute(db).await.unwrap();
    assert_eq!(all(db, "u", "event").await.unwrap().len(), 1);
    delete(db, "u", "event", " Sport").await.unwrap();
    assert!(!all(db, "u", "event").await.unwrap().contains_key("Sport"));
    assert_eq!(all(db, "u", "task").await.unwrap().len(), 1);
}
