//! The `/settings/remote/*` routes: who may reach a link, and what may be dialled.
//!
//! The protocol itself is tested in `domain::remote::tests` (against answers measured from a real
//! nextcloud) and by hand against `e2e/nextcloud-dev.sh`. What matters here is the surface: a link
//! id must be useless to anyone but its owner, because "sync now" on a foreign link would make the
//! server dial that user's Nextcloud with that user's credential.

use super::seed::seed;
use axum::http::{Method, StatusCode};
use serde_json::json;

/// A link of alice's, written straight into the tables: the routes that create one need a granted
/// login, which a test cannot have without a browser.
async fn a_link(app: &crate::testkit::TestApp, principal: &str, local_id: &str) -> String {
    let (account, link) = (uuid::Uuid::new_v4().to_string(), uuid::Uuid::new_v4().to_string());
    sqlx::query("INSERT INTO practical_remote_accounts (id, principal, base_url, login, secret) VALUES (?, ?, 'https://example.org', ?, X'00')")
        .bind(&account)
        .bind(principal)
        .bind(principal)
        .execute(&app.state.db)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO practical_remote_links (id, account_id, principal, kind, local_id, href, direction, every_min, local_token)
           VALUES (?, ?, ?, 'cal', ?, '/remote.php/dav/calendars/x/y/', 'both', 5, 0)",
    )
    .bind(&link)
    .bind(&account)
    .bind(principal)
    .bind(local_id)
    .execute(&app.state.db)
    .await
    .unwrap();
    link
}

#[tokio::test]
async fn a_link_id_is_useless_to_anyone_but_its_owner() {
    let s = seed().await;
    let cal = crate::domain::collections::calendars_for(&s.app.state, &s.app.principal("alice").await, false).await.unwrap()[0].id.clone();
    let link = a_link(&s.app, "alice", &cal).await;
    let mut bob = s.app.login("bob").await;

    // 404, not 403: a foreign id must not be distinguishable from one that does not exist
    for body in [json!({"action": "now"}), json!({"action": "remove"}), json!({"direction": "pull"})] {
        let r = bob.json(Method::POST, &format!("/settings/remote/link/{link}"), body).await;
        assert_eq!(r.status, StatusCode::NOT_FOUND, "bob reached alice's link");
    }
    // and it is still there, untouched
    let mut alice = s.app.login("alice").await;
    let rows = crate::domain::remote::links_of(&s.app.state, &s.app.principal("alice").await).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].two_way, "bob's 'pull' did not land");
    // the owner may change it
    alice.json(Method::POST, &format!("/settings/remote/link/{link}"), json!({"direction": "pull"})).await.ok();
    let rows = crate::domain::remote::links_of(&s.app.state, &s.app.principal("alice").await).await.unwrap();
    assert!(!rows[0].two_way);
}

#[tokio::test]
async fn only_a_configured_origin_may_be_dialled() {
    let s = seed().await;
    let mut alice = s.app.login("alice").await;
    // the fixture configures no origin at all, so nothing is allowed — including a plausible one
    for origin in ["https://my.xarif.de", "http://localhost:8099", "https://evil.example", "", "not a url"] {
        let r = alice.json(Method::POST, "/settings/remote/login", json!({"origin": origin})).await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{origin} was accepted");
    }
}

#[tokio::test]
async fn an_unknown_handle_is_not_a_way_in() {
    let s = seed().await;
    let mut alice = s.app.login("alice").await;
    for route in ["/settings/remote/poll", "/settings/remote/collections"] {
        let r = alice.json(Method::POST, route, json!({"handle": uuid::Uuid::new_v4().to_string()})).await;
        assert_eq!(r.status, StatusCode::NOT_FOUND, "{route}");
    }
    // an import needs a granted login too, not just a well-formed body
    let r = alice
        .json(
            Method::POST,
            "/settings/remote/import",
            json!({"handle": uuid::Uuid::new_v4().to_string(), "kind": "cal", "href": "/x/", "name": "X", "color": ""}),
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
}

/// The caps exist so a handful of links cannot turn into a request storm. Checked BEFORE the local
/// collection is created, or a refusal would leave an empty calendar behind.
#[tokio::test]
async fn the_link_caps_refuse_before_anything_is_created() {
    let s = seed().await;
    let user = s.app.principal("alice").await;
    let cals = crate::domain::collections::calendars_for(&s.app.state, &user, false).await.unwrap();
    let account = {
        let link = a_link(&s.app, "alice", &cals[0].id).await;
        crate::domain::remote::link_of(&s.app.state, &user, &link).await.unwrap().account_id
    };
    // the same remote collection a second time: that is what the UNIQUE guards, with a message
    let again = crate::domain::remote::room_for_a_link(&s.app.state, &user, &account, crate::domain::remote::dav::Kind::Cal, "/remote.php/dav/calendars/x/y/").await;
    assert!(matches!(&again, Err(crate::error::AppError::BadRequest(m)) if m.key == "err-remote-already-linked"), "{again:?}");
    // a different one is fine
    crate::domain::remote::room_for_a_link(&s.app.state, &user, &account, crate::domain::remote::dav::Kind::Cal, "/remote.php/dav/calendars/x/other/")
        .await
        .expect("a second collection of the same account");
}

/// The two tokens forget differently, and getting the local one wrong loses a user's change for
/// good: `sync_changes` only ever reports `synctoken > ?`, so an object whose push failed would
/// never appear in any later change list, and no full reconciliation brings it back — that one
/// rebuilds the REMOTE side.
#[tokio::test]
async fn a_failed_push_does_not_advance_the_local_token() {
    let s = seed().await;
    let user = s.app.principal("alice").await;
    let cals = crate::domain::collections::calendars_for(&s.app.state, &user, false).await.unwrap();
    let id = a_link(&s.app, "alice", &cals[0].id).await;
    let link = crate::domain::remote::link_of(&s.app.state, &user, &id).await.unwrap();
    assert_eq!(link.local_token, 0);

    // a round that skipped an object: the remote token moves on (replaying a body the server will
    // not take would stall everything), the local one does not, and the row says so
    crate::domain::remote::round::store_tokens(&s.app.state, &link, "http://sabre.io/ns/sync/7", 42, false, 1).await.unwrap();
    let after = crate::domain::remote::link_of(&s.app.state, &user, &id).await.unwrap();
    assert_eq!(after.local_token, 0, "a skipped push must be reported again next round");
    assert_eq!(after.sync_token.as_deref(), Some("http://sabre.io/ns/sync/7"));
    assert_eq!(after.err.as_deref(), Some("err-remote-skipped"));

    // a clean round moves both and clears the message
    crate::domain::remote::round::store_tokens(&s.app.state, &after, "http://sabre.io/ns/sync/8", 42, true, 0).await.unwrap();
    let after = crate::domain::remote::link_of(&s.app.state, &user, &id).await.unwrap();
    assert_eq!((after.local_token, after.err.as_deref()), (42, None));
    assert!(after.full_at.is_some(), "a clean full pass is recorded, so the next one is a day away");
}

/// The ways in are nested, not flat: the collections list offers three choices, and the remote
/// rows live one level down, beside the other sources of the same kind. That keeps the list short
/// while leaving room for Google and Apple under the same two headings later.
///
/// `remote_sync` off (no key) must hide the sync row AND its page, or the list would point at a
/// section that is not in the document.
#[tokio::test]
async fn the_ways_in_are_nested_under_import_and_sync() {
    let s = seed().await;
    let mut alice = s.app.login("alice").await;
    let page = alice.get("/settings").await;
    let html = String::from_utf8_lossy(&page.ok().bytes).to_string();

    // the list itself: create and import always, sync only with a key — the fixture has none
    assert!(html.contains("href=\"#new-calendar\""));
    assert!(html.contains("href=\"#import-calendar\""));
    assert!(!html.contains("href=\"#sync-calendar\""), "no key: no sync row");
    assert!(!html.contains("id=\"sync-calendar\""), "and no sync page to point at");
    // the remote rows are NOT in the list any more; without an allowed origin they are nowhere
    assert!(!html.contains("href=\"#remote-cal\""), "no origin: no remote source row");
    assert!(!html.contains("id=\"remote-cal\""));

    // the import page is where a source is chosen, so its file row is one source among several
    let imp = html.split("id=\"import-calendar\"").nth(1).unwrap_or_default();
    let imp = imp.split("</section>").next().unwrap_or_default();
    assert!(imp.contains("imp-pick"), "the file row stays on the import page");
}
