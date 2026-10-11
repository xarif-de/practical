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

/// The address is typed in, so the shape check is the first line and the field is required.
/// WHETHER an address may be dialled is not decided here — the resolver does that at connection
/// time, when the name's real addresses are known (`domain::remote::PublicOrAllowed`).
#[tokio::test]
async fn the_server_address_is_required_and_must_be_one() {
    let s = seed().await;
    let mut alice = s.app.login("alice").await;
    for (origin, why) in [
        ("", "the field is required"),
        ("   ", "and whitespace is empty"),
        ("not a url", "a sentence is not an address"),
        ("http://cloud.example.org", "http would carry the password in the clear"),
        ("https://user:pw@cloud.example.org", "userinfo in a server address hides where we connect"),
        ("https://cloud.example.org/nextcloud", "a path is not part of an origin"),
        ("https://cloud.example.org/?x=1", "nor is a query"),
    ] {
        let r = alice.json(Method::POST, "/settings/remote/login", json!({"origin": origin})).await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{origin:?} was accepted ({why})");
    }
    /* A well-formed address gets past the shape check and then fails at the connection, which is
       the honest order: the shape is ours to judge, reachability is not. `invalid.` is reserved by
       RFC 2606 and never resolves, so this does not touch the network. */
    let r = alice.json(Method::POST, "/settings/remote/login", json!({"origin": "https://nextcloud.invalid."})).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "an unreachable server is still an error");
}

/// A bare host is what a user types, and refusing it would be pedantry: https is assumed.
#[tokio::test]
async fn a_bare_host_is_read_as_https() {
    let s = seed().await;
    let app = &s.app;
    let o = |t: &str| crate::domain::remote::origin_of(&app.state, t);
    assert_eq!(o("cloud.example.org").unwrap(), "https://cloud.example.org");
    assert_eq!(o("  cloud.example.org  ").unwrap(), "https://cloud.example.org", "and it is trimmed");
    assert_eq!(o("https://cloud.example.org/").unwrap(), "https://cloud.example.org", "a trailing slash is not a path");
    assert_eq!(o("https://cloud.example.org:8443").unwrap(), "https://cloud.example.org:8443", "a port is kept");
    // http only for localhost, and only where a development login is configured
    assert!(o("http://localhost:8099").is_err(), "the fixture has no dev login");
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
    let list = html.split("id=\"collections\"").nth(1).unwrap_or_default().split("</section>").next().unwrap_or_default();
    assert!(list.contains("href=\"#new-calendar\""));
    assert!(list.contains("href=\"#import-calendar\""));
    assert!(!list.contains("href=\"#sync-calendar\""), "no key: no sync row");
    assert!(!html.contains("id=\"sync-calendar\""), "and no sync page to point at");
    // the remote rows moved one level down, onto the import page, where they need nothing
    // configured — the address is typed in
    assert!(!list.contains("#remote-cal"), "the list must not offer the source directly");
    assert!(html.contains("id=\"remote-cal\""), "the import source page is always there");

    // the import page is where a source is chosen, so its file row is one source among several
    let imp = html.split("id=\"import-calendar\"").nth(1).unwrap_or_default();
    let imp = imp.split("</section>").next().unwrap_or_default();
    assert!(imp.contains("imp-pick"), "the file row stays on the import page");
}

/// An IP literal never reaches the resolver: reqwest connects to an address directly, so
/// `PublicOrAllowed` is not consulted at all. Measured during development — `https://10.43.0.1`
/// answered "timeout", which in a cluster is the difference between "refused" and "connected".
/// The same rule therefore applies where the address is already known.
#[tokio::test]
async fn a_private_address_typed_as_a_number_is_refused_too() {
    let s = seed().await;
    let app = &s.app;
    let o = |t: &str| crate::domain::remote::origin_of(&app.state, t);
    for bad in [
        "https://10.43.0.1",        // a cluster service
        "https://192.168.1.1",      // a home router
        "https://127.0.0.1",        // the pod itself
        "https://169.254.169.254",  // a cloud metadata service
        "https://[::1]",
        "https://[fd00::1]",
        "https://2130706433",       // 127.0.0.1 written as one number
    ] {
        let r = o(bad);
        assert!(matches!(&r, Err(crate::error::AppError::BadRequest(m)) if m.key == "err-remote-private" || m.key == "err-remote-bad-server"), "{bad} was accepted: {r:?}");
    }
    // a public address is fine, because the rule is about where it points, not about being a number
    assert_eq!(o("https://142.250.185.74").unwrap(), "https://142.250.185.74");
    // and a name is left to the resolver, which sees what it really resolves to
    assert_eq!(o("https://cloud.example.org").unwrap(), "https://cloud.example.org");
}
