//! Characterisation of the change log below the routes: moves between collections, a technical
//! change, contacts, retention.

use super::*;
use crate::testkit::{TestApp, vevent};

#[tokio::test]
async fn moves_contacts_tech_and_retention() {
    let app = TestApp::new().await;
    let alice = app.user("alice", "Alice").await;
    let a = app.calendar("alice", "A", "#000000", &["VEVENT"]).await;
    let b = app.calendar("alice", "B", "#000000", &["VEVENT"]).await;
    let ab = app.addressbook("alice", "Adr", "#000000").await;
    poll(&app.state).await;
    // a DAV MOVE: delete here, create there with the same UID, read in one poll → one move
    app.put_ics("alice", &a, "m", &vevent("m", "Umzug", "DTSTART:20261001T100000Z")).await;
    poll(&app.state).await;
    crate::domain::events::delete_event(&app.state, &alice, "alice", &a, "m").await.unwrap();
    app.put_ics("alice", &b, "m", &vevent("m", "Umzug", "DTSTART:20261001T100000Z")).await;
    poll(&app.state).await;
    // a dismissed reminder only: technical
    let with_alarm = |ack: &str| vevent("t", "Tech", &format!("DTSTART:20261001T100000Z\nBEGIN:VALARM\nACTION:DISPLAY\nTRIGGER:-PT5M\n{ack}END:VALARM"));
    app.put_ics("alice", &a, "t", &with_alarm("")).await;
    poll(&app.state).await;
    app.put_ics("alice", &a, "t", &with_alarm("ACKNOWLEDGED:20261001T095800Z\n")).await;
    poll(&app.state).await;
    // a contact written by the UI
    let mut g = begin(&app.state, Actor::ui(&alice)).await;
    let id = crate::domain::contacts::create_contact(&app.state, &alice, crate::domain::contacts::ContactInput { principal: "alice".into(), ab_id: ab.clone(), given: "Kim".into(), ..Default::default() }).await.unwrap();
    g.object(Store::Ab, "alice", &ab, &id);
    let n = g.commit().await;
    assert!(n.is_some());
    // a commit that touched nothing records nothing
    assert_eq!(begin(&app.state, Actor::ui(&alice)).await.commit().await, None);
    let all = visible(&app.state, &alice).await.unwrap();
    let rows: Vec<(String, String, String, String, bool)> = all.iter().map(|c| (c.src.clone(), c.items[0].act.clone(), c.items[0].kind.clone(), c.items[0].title.clone(), c.tech)).collect();
    assert_eq!(rows, [
        ("dav".to_string(), "create".to_string(), "event".to_string(), "Umzug".to_string(), false),
        ("dav".into(), "move".into(), "event".into(), "Umzug".into(), false),
        ("dav".into(), "create".into(), "event".into(), "Tech".into(), false),
        ("dav".into(), "update".into(), "event".into(), "Tech".into(), true),
        ("ui".into(), "create".into(), "contact".into(), "Kim".into(), false),
    ]);
    let mv = &all[1].items[0];
    assert_eq!((mv.from.as_ref().map(|f| f.1.clone()), mv.coll.clone()), (Some(a.clone()), b.clone()));
    assert_eq!(mv.attrs[0], (crate::domain::history_text::COLL_KEY.to_string(), format!("alice/{a}"), format!("alice/{b}")));
    assert_eq!(all[1].items[0].el(), "cal:m");
    assert!(alive(&app.state).await.unwrap().contains(&("ab".to_string(), id.clone())));
    // another user sees none of it
    let bob = app.user("bob", "Bob").await;
    assert!(visible(&app.state, &bob).await.unwrap().is_empty());
    // retention: nothing is old yet; a change dated far back goes, with the versions only it kept
    assert_eq!(purge(&app.state).await.unwrap(), 0);
    sqlx::query("UPDATE practical_history_changes SET at = '2000-01-01 00:00:00' WHERE id = ?").bind(all[0].id).execute(&app.state.db).await.unwrap();
    assert_eq!(purge(&app.state).await.unwrap(), 1);
    assert_eq!(visible(&app.state, &alice).await.unwrap().len(), 4);
}
