//! Characterisation of groups, collections and app tokens on the real store.

use super::*;
use crate::domain::collections;
use crate::testkit::TestApp;

#[test]
fn ids_and_colours() {
    for (name, slug) in [("Familie & Freunde", "familie-freunde"), ("  Größe Übung  ", "groesse-uebung"), ("!!!", ""), ("a--b", "a-b"), ("Ünïcødé", "uen-c-d"), (&"x".repeat(50), &"x".repeat(40))] {
        assert_eq!(slugify(name), slug, "{name}");
    }
    assert!(is_group_id("group.x") && !is_group_id("groupx"));
    assert_eq!(collections::default_color("abc"), collections::default_color("abc"));
    assert!(collections::PALETTE.contains(&collections::default_color("any-id").as_str()));
    assert_eq!(collections::parse_components(&[]), [rustical_ical::CalendarObjectType::Event]);
    assert_eq!(collections::parse_components(&["VTODO".into(), "VFREEBUSY".into(), "VJOURNAL".into()]), [rustical_ical::CalendarObjectType::Todo, rustical_ical::CalendarObjectType::Journal]);
    assert!(collections::valid_color("#00AAff") && !collections::valid_color("tomato"));
    let t = crate::domain::tokens::generate_secret();
    assert!(t.len() == 64 && t.chars().all(|c| c.is_ascii_alphanumeric()));
    assert_ne!(t, crate::domain::tokens::generate_secret());
}

#[tokio::test]
async fn group_rules() {
    let app = TestApp::new().await;
    let alice = app.user("alice", "Alice").await;
    app.user("bob", "Bob").await;
    let carol = app.user("carol", "Carol").await;
    let gid = create(&app.state, &alice, "Lauftreff").await.unwrap();
    assert_eq!(gid, "group.lauftreff");
    assert_eq!(create(&app.state, &alice, "Lauf-Treff!").await.unwrap(), "group.lauf-treff", "another slug, another group");
    assert!(matches!(create(&app.state, &alice, "Lauftreff").await, Err(AppError::Conflict(m)) if m == "group group.lauftreff exists"));
    assert!(matches!(create(&app.state, &alice, "…").await, Err(AppError::BadRequest(_))));
    let alice = app.principal("alice").await;
    assert!(matches!(add_member(&app.state, &carol, &gid, "carol").await, Err(AppError::Forbidden)));
    assert!(matches!(add_member(&app.state, &alice, &gid, "nobody").await, Err(AppError::NotFound)));
    add_member(&app.state, &alice, &gid, "bob").await.unwrap();
    add_member(&app.state, &alice, &gid, "bob").await.unwrap();
    assert!(matches!(ensure_member(&app.state, &alice, "alice").await, Err(AppError::NotFound)), "not a group id");
    // a group calendar counts on the list
    app.calendar(&gid, "Läufe", "#000000", &["VEVENT"]).await;
    let list = list_for_user(&app.state, &alice).await.unwrap();
    assert_eq!(list.iter().map(|g| (g.id.as_str(), g.members.len(), g.n_calendars, g.n_addressbooks)).collect::<Vec<_>>(), [("group.lauf-treff", 1, 0, 0), ("group.lauftreff", 2, 1, 0)]);
    assert_eq!(individuals(&app.state).await.unwrap().iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["alice", "bob", "carol"]);
    // trash: wrong name, then the id works too; collections go to the trash with it
    assert!(matches!(trash(&app.state, &alice, &gid, Some("falsch")).await, Err(AppError::BadRequest(_))));
    trash(&app.state, &alice, &gid, Some(&gid)).await.unwrap();
    let alice = app.principal("alice").await;
    let t = trashed_for_user(&app.state, &alice).await.unwrap();
    assert_eq!(t.iter().map(|g| (g.id.as_str(), g.displayname.as_str(), g.deleted_by.as_str())).collect::<Vec<_>>(), [("group.lauftreff", "Lauftreff", "alice")]);
    assert_eq!((t[0].expires_at - t[0].deleted_at).num_days(), 90);
    let bob = app.principal("bob").await;
    assert_eq!(trashed_for_user(&app.state, &bob).await.unwrap().len(), 1, "every former member sees it");
    assert!(matches!(create(&app.state, &bob, "Lauftreff").await, Err(AppError::Conflict(_))), "a trashed id is taken");
    restore(&app.state, &bob, &gid).await.unwrap();
    let alice = app.principal("alice").await;
    assert!(alice.memberships.contains(&gid));
    assert_eq!(collections::calendars_for(&app.state, &alice, false).await.unwrap().iter().filter(|c| c.principal == gid).count(), 1, "the calendar is back");
    // the last member leaving trashes it
    assert!(matches!(remove_member(&app.state, &alice, &gid, "bob").await, Ok(RemoveOutcome::Removed)));
    assert!(matches!(remove_member(&app.state, &alice, &gid, "bob").await, Err(AppError::NotFound)));
    assert!(matches!(remove_member(&app.state, &alice, &gid, "alice").await, Ok(RemoveOutcome::GroupTrashed)));
    assert!(matches!(purge_by_member(&app.state, &carol, &gid).await, Err(AppError::Forbidden) | Err(AppError::NotFound)));
    let alice = app.principal("alice").await;
    purge_by_member(&app.state, &alice, &gid).await.unwrap();
    assert!(trashed_for_user(&app.state, &alice).await.unwrap().is_empty());
    assert!(app.state.principals().get_principal(&gid).await.unwrap().is_none());
    // nothing expired yet
    assert_eq!(purge_expired(&app.state).await.unwrap(), 0);
}

#[tokio::test]
async fn expired_trash_is_purged() {
    let app = TestApp::new().await;
    let alice = app.user("alice", "Alice").await;
    let gid = create(&app.state, &alice, "Alt").await.unwrap();
    let alice = app.principal("alice").await;
    trash(&app.state, &alice, &gid, None).await.unwrap();
    sqlx::query("UPDATE practical_trashed_groups SET deleted_at = '2020-01-01T00:00:00+00:00'").execute(&app.state.db).await.unwrap();
    assert_eq!(purge_expired(&app.state).await.unwrap(), 1);
    assert!(app.state.principals().get_principal(&gid).await.unwrap().is_none());
}

#[tokio::test]
async fn collections_and_tokens() {
    let app = TestApp::new().await;
    let alice = app.user("alice", "Alice").await;
    let bob = app.user("bob", "Bob").await;
    let n = |name: &str, owner: &str| collections::NewCalendar { owner: owner.into(), name: name.into(), color: " ".into(), description: " ".into(), components: vec![], timezone: " ".into() };
    assert!(matches!(collections::create_calendar(&app.state, &alice, n(" ", "alice")).await, Err(AppError::BadRequest(_))));
    assert!(matches!(collections::create_calendar(&app.state, &alice, n("X", "bob")).await, Err(AppError::Forbidden)));
    let id = collections::create_calendar(&app.state, &alice, n(" X ", "alice")).await.unwrap();
    let c = &collections::calendars_for(&app.state, &alice, false).await.unwrap()[0];
    assert_eq!((c.id.as_str(), c.displayname.as_str(), c.description.as_str(), c.timezone_id.as_str(), c.owner_label.as_str(), c.is_group), (id.as_str(), "X", "", "", "Alice", false));
    assert_eq!(c.color, collections::default_color(&id));
    collections::update_calendar(&app.state, &alice, "alice", &id, "", "#123456", "D").await.unwrap();
    let c = &collections::calendars_for(&app.state, &alice, false).await.unwrap()[0];
    assert_eq!((c.displayname.as_str(), c.color.as_str(), c.description.as_str()), ("X", "#123456", "D"), "an empty name keeps the old one");
    collections::update_calendar(&app.state, &alice, "alice", &id, "Y", "tomato", "").await.unwrap();
    assert_eq!(collections::calendars_for(&app.state, &alice, false).await.unwrap()[0].color, collections::default_color(&id), "only #rrggbb is shown");
    assert!(matches!(collections::set_addressbook_color_checked(&app.state, &alice, "alice", "missing", "#000000").await, Err(AppError::Store(_))));
    // owners: the user, then the groups
    app.group("group.g", "G", &["alice"]).await;
    let alice = app.principal("alice").await;
    let o = collections::owners(&app.state, &alice).await.unwrap();
    assert_eq!(o.iter().map(|o| (o.id.as_str(), o.label.as_str(), o.is_group)).collect::<Vec<_>>(), [("alice", "Alice", false), ("group.g", "G", true)]);
    assert!(collections::ensure_owner(&alice, "group.g").is_ok() && collections::ensure_owner(&alice, "bob").is_err());
    // tokens: newest first, shown once as prefix_secret, deletable by the owner only
    let t1 = crate::domain::tokens::create(&app.state, &alice, " Laptop ").await.unwrap();
    assert!(matches!(crate::domain::tokens::create(&app.state, &alice, "").await, Err(AppError::BadRequest(_))));
    let list = crate::domain::tokens::list(&app.state, &alice).await.unwrap();
    assert_eq!(list.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["Laptop"]);
    assert!(list[0].id.starts_with(t1.split('_').next().unwrap()));
    assert!(matches!(crate::domain::tokens::delete(&app.state, &bob, &list[0].id).await, Err(AppError::NotFound)));
    crate::domain::tokens::delete(&app.state, &alice, &list[0].id).await.unwrap();
    assert!(crate::domain::tokens::list(&app.state, &alice).await.unwrap().is_empty());
}
