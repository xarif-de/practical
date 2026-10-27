//! The group trash with shares and claims: what a trash, a restore and a purge do to the shares of and to a
//! group, who may act on an entry, and how two runs on one entry keep out of each other's way.

use super::*;
use crate::domain::collections;
use crate::state::{Access, Need};
use crate::testkit::TestApp;

/// alice and bob in `group.f`, which owns the calendar "G" shared with carol; alice's "A" is shared with the group
struct World {
    app: TestApp,
    gid: String,
    g: String,
    a: String,
}

async fn world() -> World {
    let app = TestApp::new().await;
    app.user("alice", "Alice").await;
    app.user("bob", "Bob").await;
    app.user("carol", "Carol").await;
    let alice = app.principal("alice").await;
    let gid = create(&app.state, &alice, "F").await.unwrap();
    let alice = app.principal("alice").await;
    add_member(&app.state, &alice, &gid, "bob").await.unwrap();
    let alice = app.principal("alice").await;
    let g = app.calendar(&gid, "G", "#123456", &["VEVENT"]).await;
    let a = app.calendar("alice", "A", "#654321", &["VEVENT"]).await;
    let ga = collections::ensure_calendar(&app.state, &alice, &gid, &g, Need::Own).await.unwrap();
    app.state.collections().share_calendar(&ga, "carol", Access::Read, &alice).await.unwrap();
    let aa = collections::ensure_calendar(&app.state, &alice, "alice", &a, Need::Own).await.unwrap();
    app.state.collections().share_calendar(&aa, &gid, Access::Write, &alice).await.unwrap();
    World { app, gid, g, a }
}

async fn row(app: &TestApp, gid: &str) -> Option<(String, Option<String>, Option<String>)> {
    sqlx::query("SELECT members_json, claim, claimed_at FROM practical_trashed_groups WHERE id = ?")
        .bind(gid)
        .fetch_optional(&app.state.db)
        .await
        .unwrap()
        .map(|r| (r.get("members_json"), r.get("claim"), r.get("claimed_at")))
}

/// Whether `user` lists a calendar of `owner` with the id `id` (own or through a share)
async fn lists(app: &TestApp, user: &str, owner: &str, id: &str) -> bool {
    let u = app.principal(user).await;
    collections::calendars_for(&app.state, &u, false).await.unwrap().iter().any(|c| c.owner == owner && c.auth.id() == id)
}

async fn past_lease(app: &TestApp, gid: &str) {
    let t = (crate::clock::now() - CLAIM_LEASE - chrono::Duration::minutes(1)).to_rfc3339();
    sqlx::query("UPDATE practical_trashed_groups SET claimed_at = ? WHERE id = ?").bind(t).bind(gid).execute(&app.state.db).await.unwrap();
}

#[tokio::test]
async fn shares_of_and_to_a_group_rest_and_return_with_restore() {
    let w = world().await;
    let alice = w.app.principal("alice").await;
    assert!(lists(&w.app, "carol", &w.gid, &w.g).await && lists(&w.app, "bob", "alice", &w.a).await);
    trash(&w.app.state, &alice, &w.gid, None).await.unwrap();
    // in the bin: carol's copy of G is gone, bob reaches A no more (no memberships)
    assert!(!lists(&w.app, "carol", &w.gid, &w.g).await);
    assert!(!lists(&w.app, "bob", "alice", &w.a).await);
    // the shares themselves are kept
    let aa = collections::ensure_calendar(&w.app.state, &alice, "alice", &w.a, Need::Own).await.unwrap();
    assert_eq!(w.app.state.collections().calendar_shares(&aa).await.unwrap().iter().map(|s| s.sharee.as_str()).collect::<Vec<_>>(), [w.gid.as_str()]);
    let alice = w.app.principal("alice").await;
    restore(&w.app.state, &alice, &w.gid).await.unwrap();
    assert!(lists(&w.app, "carol", &w.gid, &w.g).await, "the group's share is back");
    assert!(lists(&w.app, "bob", "alice", &w.a).await, "the share to the group is usable again");
    assert!(row(&w.app, &w.gid).await.is_none());
}

#[tokio::test]
async fn last_member_leaving_keeps_the_share_to_the_group() {
    let w = world().await;
    let alice = w.app.principal("alice").await;
    remove_member(&w.app.state, &alice, &w.gid, "bob").await.unwrap();
    let alice = w.app.principal("alice").await;
    assert!(matches!(remove_member(&w.app.state, &alice, &w.gid, "alice").await, Ok(RemoveOutcome::GroupTrashed)));
    let alice = w.app.principal("alice").await;
    let aa = collections::ensure_calendar(&w.app.state, &alice, "alice", &w.a, Need::Own).await.unwrap();
    assert_eq!(w.app.state.collections().calendar_shares(&aa).await.unwrap().len(), 1);
}

#[tokio::test]
async fn only_former_members_restore_or_purge() {
    let w = world().await;
    let alice = w.app.principal("alice").await;
    trash(&w.app.state, &alice, &w.gid, None).await.unwrap();
    let carol = w.app.principal("carol").await;
    assert!(matches!(restore(&w.app.state, &carol, &w.gid).await, Err(AppError::Forbidden)));
    assert!(matches!(purge_by_member(&w.app.state, &carol, &w.gid).await, Err(AppError::Forbidden)));
    assert!(trashed_for_user(&w.app.state, &carol).await.unwrap().is_empty());
    // the refused run gave its claim up: the member acts at once
    let bob = w.app.principal("bob").await;
    restore(&w.app.state, &bob, &w.gid).await.unwrap();
}

#[tokio::test]
async fn a_member_removed_from_the_entry_has_no_right_to_it() {
    // a member removed while the trash ran: the entry records only those it detached (`clear_memberships`)
    let w = world().await;
    let alice = w.app.principal("alice").await;
    trash(&w.app.state, &alice, &w.gid, None).await.unwrap();
    sqlx::query("UPDATE practical_trashed_groups SET members_json = '[\"alice\"]' WHERE id = ?").bind(&w.gid).execute(&w.app.state.db).await.unwrap();
    let bob = w.app.principal("bob").await;
    assert!(trashed_for_user(&w.app.state, &bob).await.unwrap().is_empty());
    assert!(matches!(restore(&w.app.state, &bob, &w.gid).await, Err(AppError::Forbidden)));
    assert!(matches!(purge_by_member(&w.app.state, &bob, &w.gid).await, Err(AppError::Forbidden)));
    let alice = w.app.principal("alice").await;
    restore(&w.app.state, &alice, &w.gid).await.unwrap();
    assert_eq!(w.app.state.principals().list_members(&w.gid).await.unwrap(), ["alice"], "the removed member does not come back");
}

#[tokio::test]
async fn restore_brings_back_only_what_the_trash_put_away() {
    let w = world().await;
    let alice = w.app.principal("alice").await;
    let old = w.app.calendar(&w.gid, "Alt", "#000000", &["VEVENT"]).await;
    collections::delete_calendar(&w.app.state, &alice, &w.gid, &old, false).await.unwrap();
    trash(&w.app.state, &alice, &w.gid, None).await.unwrap();
    let alice = w.app.principal("alice").await;
    restore(&w.app.state, &alice, &w.gid).await.unwrap();
    assert!(lists(&w.app, "alice", &w.gid, &w.g).await);
    assert!(!lists(&w.app, "alice", &w.gid, &old).await, "deleted by a member before: stays in the bin");
}

#[tokio::test]
async fn restore_with_an_account_deleted_meanwhile() {
    let w = world().await;
    let alice = w.app.principal("alice").await;
    trash(&w.app.state, &alice, &w.gid, None).await.unwrap();
    w.app.state.principals().remove_principal("bob").await.unwrap();
    restore(&w.app.state, &alice, &w.gid).await.unwrap();
    assert_eq!(w.app.state.principals().list_members(&w.gid).await.unwrap(), ["alice"]);
}

#[tokio::test]
async fn a_live_groups_stale_row_is_not_purged() {
    let w = world().await;
    let alice = w.app.principal("alice").await;
    trash(&w.app.state, &alice, &w.gid, None).await.unwrap();
    // an interrupted restore: memberships back, the row still there, its claim run out
    w.app.state.principals().set_memberships(&w.gid, &["alice".to_string(), "bob".to_string()]).await.unwrap();
    sqlx::query("UPDATE practical_trashed_groups SET deleted_at = '2020-01-01T00:00:00+00:00', claim = 'restore:x' WHERE id = ?").bind(&w.gid).execute(&w.app.state.db).await.unwrap();
    past_lease(&w.app, &w.gid).await;
    assert_eq!(purge_expired(&w.app.state).await.unwrap(), 1);
    assert!(row(&w.app, &w.gid).await.is_none(), "only the stale row went");
    assert!(w.app.state.principals().get_principal(&w.gid).await.unwrap().is_some());
    assert_eq!(w.app.state.principals().list_members(&w.gid).await.unwrap().len(), 2);
}

#[tokio::test]
async fn trash_over_an_existing_row() {
    let w = world().await;
    let alice = w.app.principal("alice").await;
    trash(&w.app.state, &alice, &w.gid, None).await.unwrap();
    let alice = w.app.principal("alice").await;
    restore(&w.app.state, &alice, &w.gid).await.unwrap();
    // a row held by a running restore of the live group: a trash stops without a change
    sqlx::query("INSERT INTO practical_trashed_groups (id, displayname, members_json, calendars_json, addressbooks_json, deleted_by, deleted_at, claim, claimed_at) VALUES (?, 'F', '[\"alice\"]', '[\"x\"]', '[]', 'alice', ?, 'restore:held', ?)")
        .bind(&w.gid)
        .bind(crate::clock::now().to_rfc3339())
        .bind(crate::clock::now().to_rfc3339())
        .execute(&w.app.state.db)
        .await
        .unwrap();
    let alice = w.app.principal("alice").await;
    assert!(matches!(trash(&w.app.state, &alice, &w.gid, None).await, Err(AppError::Conflict(_))));
    assert_eq!(w.app.state.principals().list_members(&w.gid).await.unwrap().len(), 2);
    assert!(lists(&w.app, "alice", &w.gid, &w.g).await);
    // stale (the claim ran out): replaced; the lists merged, the members the current ones
    past_lease(&w.app, &w.gid).await;
    trash(&w.app.state, &alice, &w.gid, None).await.unwrap();
    let r = sqlx::query("SELECT members_json, calendars_json, claim FROM practical_trashed_groups WHERE id = ?").bind(&w.gid).fetch_one(&w.app.state.db).await.unwrap();
    let j = |k: &str| serde_json::from_str::<Vec<String>>(&r.get::<String, _>(k)).unwrap();
    let mut members = j("members_json");
    members.sort();
    assert_eq!(members, ["alice", "bob"]);
    let cals = j("calendars_json");
    assert!(cals.contains(&"x".to_string()) && cals.contains(&w.g), "{cals:?}");
    assert!(r.get::<Option<String>, _>("claim").is_none(), "done: nobody holds it");
}

#[tokio::test]
async fn an_abandoned_claim_is_taken_over_after_the_lease() {
    let w = world().await;
    let alice = w.app.principal("alice").await;
    trash(&w.app.state, &alice, &w.gid, None).await.unwrap();
    let held = claim(&w.app.state, &w.gid, "restore", None).await.unwrap().expect("free");
    assert!(claim(&w.app.state, &w.gid, "purge", None).await.unwrap().is_none(), "held");
    assert!(matches!(restore(&w.app.state, &alice, &w.gid).await, Err(AppError::Conflict(_))));
    past_lease(&w.app, &w.gid).await;
    let taken = claim(&w.app.state, &w.gid, "restore", None).await.unwrap().expect("ran out");
    // the old run notices before its next step and stops
    assert!(renew(&w.app.state, &w.gid, &held.claim).await.is_err());
    assert!(renew(&w.app.state, &w.gid, &taken.claim).await.is_ok());
    // and removes nothing: deleting the row needs the claim
    assert!(delete_claimed(&w.app.state, &w.gid, &held.claim).await.is_err());
    assert!(row(&w.app, &w.gid).await.is_some());
}

#[tokio::test]
async fn purge_with_a_trashed_collection_and_per_group_failures() {
    let w = world().await;
    let alice = w.app.principal("alice").await;
    let old = w.app.calendar(&w.gid, "Alt", "#000000", &["VEVENT"]).await;
    collections::delete_calendar(&w.app.state, &alice, &w.gid, &old, false).await.unwrap();
    let other = create(&w.app.state, &alice, "Zwei").await.unwrap();
    let alice = w.app.principal("alice").await;
    trash(&w.app.state, &alice, &w.gid, None).await.unwrap();
    trash(&w.app.state, &alice, &other, None).await.unwrap();
    sqlx::query("UPDATE practical_trashed_groups SET deleted_at = '2020-01-01T00:00:00+00:00'").execute(&w.app.state.db).await.unwrap();
    // one entry held by a running restore: it fails this round, the other one is purged
    sqlx::query("UPDATE practical_trashed_groups SET claim = 'restore:held', claimed_at = ? WHERE id = ?").bind(crate::clock::now().to_rfc3339()).bind(&other).execute(&w.app.state.db).await.unwrap();
    assert_eq!(purge_expired(&w.app.state).await.unwrap(), 1);
    assert!(w.app.state.principals().get_principal(&w.gid).await.unwrap().is_none());
    assert!(w.app.state.principals().get_principal(&other).await.unwrap().is_some());
    // carol's copy and the share to the group went with the group (CASCADE)
    assert!(!lists(&w.app, "carol", &w.gid, &w.g).await);
    let alice = w.app.principal("alice").await;
    let aa = collections::ensure_calendar(&w.app.state, &alice, "alice", &w.a, Need::Own).await.unwrap();
    assert!(w.app.state.collections().calendar_shares(&aa).await.unwrap().is_empty());
}

#[test]
fn the_purger_leaves_groups_alone_right_after_the_start() {
    let t0 = tokio::time::Instant::now();
    assert!(!crate::tasks::group_purge_due(t0, t0));
    assert!(!crate::tasks::group_purge_due(t0, t0 + std::time::Duration::from_secs(14 * 60)));
    assert!(crate::tasks::group_purge_due(t0, t0 + crate::tasks::GROUP_PURGE_DELAY));
}
