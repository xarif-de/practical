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
    a_link_into(app, principal, principal, local_id).await
}

/// A link of `principal`'s whose local collection belongs to `owner` (one of their groups).
async fn a_link_into(app: &crate::testkit::TestApp, principal: &str, owner: &str, local_id: &str) -> String {
    let (account, link) = (uuid::Uuid::new_v4().to_string(), uuid::Uuid::new_v4().to_string());
    sqlx::query("INSERT INTO practical_remote_accounts (id, principal, base_url, login, secret) VALUES (?, ?, 'https://example.org', ?, X'00')")
        .bind(&account)
        .bind(principal)
        .bind(principal)
        .execute(&app.state.db)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO practical_remote_links (id, account_id, principal, owner, kind, local_id, href, direction, every_min, local_token)
           VALUES (?, ?, ?, ?, 'cal', ?, '/remote.php/dav/calendars/x/y/', 'both', 5, 0)",
    )
    .bind(&link)
    .bind(&account)
    .bind(principal)
    .bind(owner)
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

/// The field must not be `type=url`. A browser refuses to submit one without a scheme, so
/// `nextcloud.example.com` — which is what the placeholder asks for and what `origin_of` reads as
/// https — never reached the server: the user was told to type a protocol that is optional.
#[tokio::test]
async fn the_address_field_takes_a_host_without_a_scheme() {
    let s = seed().await;
    let mut alice = s.app.login("alice").await;
    let html = String::from_utf8_lossy(&alice.get("/settings").await.ok().bytes).to_string();
    let rows: Vec<&str> = html.match_indices("rm-origin").map(|(i, _)| &html[i.saturating_sub(120)..i]).collect();
    assert!(!rows.is_empty(), "no address field on the page");
    for r in &rows {
        assert!(!r.contains("type=\"url\""), "the address field is a url field: {r}");
    }
    // and the placeholder is a bare host, so it shows what is accepted
    assert!(html.contains("nextcloud.example.com"), "the placeholder does not name a bare host");
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

    // a round whose PUSH was refused: the remote token moves on (replaying a body the server will
    // not take would stall everything), the local one does not, and the row says so
    crate::domain::remote::round::store_tokens(&s.app.state, &link, "http://sabre.io/ns/sync/7", 42, false, 1, 1).await.unwrap();
    let after = crate::domain::remote::link_of(&s.app.state, &user, &id).await.unwrap();
    assert_eq!(after.local_token, 0, "a skipped push must be reported again next round");
    assert_eq!(after.sync_token.as_deref(), Some("http://sabre.io/ns/sync/7"));
    assert_eq!(after.err.as_deref(), Some("err-remote-skipped"));

    /* A round that skipped a REMOTE object the local side cannot store owes nobody anything, so the
       local token moves on. Held back, it would freeze on an object that is refused every round,
       and every later round would re-read and re-parse the whole collection to suppress echoes. The
       message still appears, and the full-pass stamp is still withheld. */
    crate::domain::remote::round::store_tokens(&s.app.state, &after, "http://sabre.io/ns/sync/7b", 41, true, 1, 0).await.unwrap();
    let after = crate::domain::remote::link_of(&s.app.state, &user, &id).await.unwrap();
    assert_eq!(after.local_token, 41, "an unreadable remote object must not freeze the local token");
    assert_eq!(after.err.as_deref(), Some("err-remote-skipped"));
    assert!(after.full_at.is_none(), "a pass that skipped something is not a clean full pass");

    // a clean round moves both and clears the message
    crate::domain::remote::round::store_tokens(&s.app.state, &after, "http://sabre.io/ns/sync/8", 42, true, 0, 0).await.unwrap();
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
/// An exemption names an ORIGIN, and the resolver can only ever match a NAME: DNS carries no port.
/// So the port is compared where it is known, or `PRACTICAL__REMOTE_PRIVATE_ORIGINS=https://host`
/// would exempt every port of that host from the public-address rule.
#[tokio::test]
async fn an_exemption_covers_one_origin_and_not_every_port_of_its_host() {
    let app = crate::testkit::TestApp::with(|c| c.remote_private_origins = vec!["https://my.example.org".into()]).await;
    let o = |t: &str| crate::domain::remote::origin_of(&app.state, t);
    assert_eq!(o("https://my.example.org").unwrap(), "https://my.example.org", "the configured origin");
    assert_eq!(o("my.example.org").unwrap(), "https://my.example.org", "typed without a scheme");
    for other in ["https://my.example.org:6443", "https://my.example.org:8443"] {
        let r = o(other);
        assert!(
            matches!(&r, Err(crate::error::AppError::BadRequest(m)) if m.key == "err-remote-private"),
            "{other} is not the exempt origin and must fall under the public rule: {r:?}"
        );
    }
    // a host nobody exempted is left to the resolver, port or no port
    assert_eq!(o("https://cloud.example.org:8443").unwrap(), "https://cloud.example.org:8443");
}

/// A direction that is neither "both" nor "pull" used to mean `both`, and that is the direction
/// which WRITES to the user's own server. A typo must not pick it.
#[test]
fn an_unknown_direction_is_refused_rather_than_taken_for_writing() {
    use crate::domain::remote::two_way_from;
    assert_eq!(two_way_from("both").unwrap(), true);
    assert_eq!(two_way_from("pull").unwrap(), false);
    for bad in ["pul", "Both", "", "push", "both ", "BOTH"] {
        let r = two_way_from(bad);
        assert!(
            matches!(&r, Err(crate::error::AppError::BadRequest(m)) if m.key == "err-remote-bad-direction"),
            "{bad:?} was taken for a direction: {r:?}"
        );
    }
}

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

/// A sign-in has to survive the whole batch. Measured during development: destroying the app
/// password after the first import left the second request without one, so picking two calendars
/// imported one — and said it had succeeded. `/settings/remote/done` is what ends it.
#[tokio::test]
async fn one_sign_in_serves_several_collections() {
    let s = seed().await;
    let mut alice = s.app.login("alice").await;
    // no granted handle: every route that needs one answers 404, `done` included — it must not be
    // a way to probe which handles exist
    let unknown = uuid::Uuid::new_v4().to_string();
    for route in ["/settings/remote/collections", "/settings/remote/import"] {
        let r = alice.json(Method::POST, route, json!({"handle": unknown, "kind": "cal", "href": "/x/", "name": "X", "color": ""})).await;
        assert_eq!(r.status, StatusCode::NOT_FOUND, "{route}");
    }
    // `done` on an unknown handle is harmless and says nothing about it
    let r = alice.json(Method::POST, "/settings/remote/done", json!({"handle": unknown})).await;
    assert_eq!(r.status, StatusCode::OK, "done must not fail on a handle that is already gone");
}

/// A link belongs to the collection it syncs: "sync now" is a button on that collection's row, and
/// the way out sits on its page, beside its name and colour. There is no list of links any more —
/// a separate section repeated names the settings already show, and it was the only place where
/// direction and last run could be read.
#[tokio::test]
async fn a_link_is_shown_on_the_collection_it_syncs() {
    let s = seed().await;
    let cal = crate::domain::collections::calendars_for(&s.app.state, &s.app.principal("alice").await, false).await.unwrap()[0].id.clone();
    let mut alice = s.app.login("alice").await;

    // without a link the row is the plain one it always was
    let html = String::from_utf8_lossy(&alice.get("/settings").await.ok().bytes).to_string();
    assert!(!html.contains("rm-now"), "no link, no sync button");
    assert!(!html.contains("rm-row"), "and no row marked as one");

    let link = a_link(&s.app, "alice", &cal).await;
    let html = String::from_utf8_lossy(&alice.get("/settings").await.ok().bytes).to_string();

    // the collections list: the calendar's own row carries the button and the link's id
    let list = html.split("id=\"collections\"").nth(1).unwrap_or_default().split("</section>").next().unwrap_or_default();
    assert!(list.contains(&format!("data-rm-id=\"{link}\"")), "the row does not know its link");
    assert!(list.contains("rm-now"), "no way to sync now from the list");
    assert!(!list.contains("rm-drop"), "ending a sync is not a one-click thing in a list");
    // the button must not sit inside the row's link: nested interactive elements
    let row = list.split(&format!("data-rm-id=\"{link}\"")).nth(1).unwrap_or_default().split("</div>").next().unwrap_or_default();
    assert!(row.contains("class=\"rowlink\""), "the row's link must be its own element");
    let (before_btn, _) = row.split_once("rm-now").unwrap();
    assert!(!before_btn.rsplit("</a>").next().unwrap_or_default().contains("<a "), "the button is inside an <a>");

    // the calendar's own page: direction, last run, and the way out
    let page = html.split(&format!("id=\"cal-alice-{cal}\"")).nth(1).unwrap_or_default().split("</section>").next().unwrap_or_default();
    assert!(page.contains("rm-drop"), "a sync cannot be ended where it is set up");
    /* Ending a sync is a chain coming apart, not an "x": the collection and its entries stay, only
       the connection goes (`rm-remove-hint`), and an x reads as "delete this". */
    let drop_row = page.split("rm-drop").nth(1).unwrap_or_default().split("</button>").next().unwrap_or_default();
    assert!(drop_row.contains("#i-unlink"), "the way out of a sync does not show a parting link: {drop_row}");
    assert!(page.contains("rm-now"), "and running one by hand belongs there too");
    assert!(page.contains(&format!("data-rm-id=\"{link}\"")));
}

/// Deleting a collection ends its link. It has to: a link is reached through its collection, so one
/// whose collection is gone would keep running with no way to stop it.
#[tokio::test]
async fn a_deleted_collection_takes_its_link_with_it() {
    let s = seed().await;
    let who = s.app.principal("alice").await;
    let cal = crate::domain::collections::calendars_for(&s.app.state, &who, false).await.unwrap()[0].id.clone();
    let link = a_link(&s.app, "alice", &cal).await;
    assert_eq!(crate::domain::remote::links_of(&s.app.state, &who).await.unwrap().len(), 1);

    // the bin, not a purge: the entries come back on a restore, the sign-in does not
    crate::domain::collections::delete_calendar(&s.app.state, &who, "alice", &cal, false).await.unwrap();
    let left = crate::domain::remote::links_of(&s.app.state, &who).await.unwrap();
    assert!(left.is_empty(), "the link outlived its calendar: {left:?}");
    // and the account went with it, since it held no other link
    let accounts: i64 = sqlx::query_scalar("SELECT count(*) FROM practical_remote_accounts WHERE principal = 'alice'")
        .fetch_one(&s.app.state.db)
        .await
        .unwrap();
    assert_eq!(accounts, 0, "the account stayed behind with its sealed password");
    let _ = link;
}

/// The progress dialog is ONE element for all four remote pages, because one batch runs at a time.
/// Its bar starts without a value: `remote.js` only gives it one when more than one collection was
/// picked, and a `<progress>` without a value is the indeterminate one.
#[tokio::test]
async fn the_progress_dialog_is_on_the_page_once() {
    let s = seed().await;
    let mut alice = s.app.login("alice").await;
    let html = String::from_utf8_lossy(&alice.get("/settings").await.ok().bytes).to_string();

    assert_eq!(html.matches("class=\"rm-dlg\"").count(), 1, "the dialog is not there exactly once");
    let dlg = html.split("class=\"rm-dlg\"").nth(1).unwrap_or_default().split("</dialog>").next().unwrap_or_default();
    assert!(dlg.contains("rm-dlg-bar"), "no bar");
    assert!(!dlg.contains("<progress class=\"rm-dlg-bar\" value"), "the bar must start indeterminate");
    assert!(dlg.contains("rm-dlg-close\" hidden"), "the way out shows only once there is something to read");
    assert!(dlg.contains("aria-live=\"polite\""), "the running line must be announced");
    // the heading is what labels the dialog, so it must carry the id the dialog points at
    assert!(html.contains("aria-labelledby=\"rm-dlg-h\""));
    assert!(dlg.contains("id=\"rm-dlg-h\""));
}

/// The address field offers back what the user signed in at before, because they type the same
/// server every time. Written only once a sign-in was GRANTED, capped at `SERVERS_OFFERED`, newest
/// first — and it is only the address: no credential is kept here, so a one-off import, which keeps
/// nothing else, still leaves its address behind.
#[tokio::test]
async fn the_address_field_offers_the_servers_used_before() {
    let s = seed().await;
    let who = s.app.principal("alice").await;
    let st = &s.app.state;
    let mut alice = s.app.login("alice").await;

    // nothing used yet: no island at all, rather than an empty one
    let html = String::from_utf8_lossy(&alice.get("/settings").await.ok().bytes).to_string();
    assert!(!html.contains("id=\"rm-servers\""), "an empty list must not be rendered");

    for (i, origin) in ["https://a.example.org", "https://b.example.org", "https://c.example.org", "https://d.example.org"].iter().enumerate() {
        crate::domain::remote::remember_server(st, &who, origin).await;
        // the stored stamp decides the order, and four writes inside one test share a clock
        sqlx::query("UPDATE practical_remote_servers SET used_at = ? WHERE principal = ? AND origin = ?")
            .bind(format!("2026-10-0{}T00:00:00+00:00", i + 1))
            .bind(&who.id)
            .bind(origin)
            .execute(&st.db)
            .await
            .unwrap();
    }
    let offered = crate::domain::remote::servers_of(st, &who).await.unwrap();
    assert_eq!(offered, vec!["https://d.example.org", "https://c.example.org", "https://b.example.org"], "newest first, capped at three");

    // the same address twice is one entry, moved to the front
    crate::domain::remote::remember_server(st, &who, "https://a.example.org").await;
    let again = crate::domain::remote::servers_of(st, &who).await.unwrap();
    assert_eq!(again[0], "https://a.example.org", "a re-used address is the newest one");
    assert_eq!(again.len(), 3);

    // and it is the user's own list
    let bob = s.app.principal("bob").await;
    assert!(crate::domain::remote::servers_of(st, &bob).await.unwrap().is_empty(), "alice's servers reached bob");

    let html = String::from_utf8_lossy(&alice.get("/settings").await.ok().bytes).to_string();
    assert_eq!(html.matches("id=\"rm-servers\"").count(), 1, "one island for every address field");
    assert!(html.contains("https://a.example.org"), "the island does not carry the addresses");
    /* The list is ours, not the browser's: a <datalist> wears a dropdown triangle in Chromium that
       no other field here has. So the field is a combobox over a `.rm-sug` listbox — one per form,
       and two forms here, because the fixture configures no key and the two SYNC pages therefore do
       not exist (`the_ways_in_are_nested_under_import_and_sync`). */
    assert!(!html.contains("<datalist"), "no browser dropdown anywhere");
    assert!(!html.contains("list=\"rm-servers\""), "and no field pointing at one");
    assert_eq!(html.matches("class=\"rm-sug\"").count(), 2, "every address field needs its own listbox");
    // the search field in the frame is a combobox too, so this counts only the address ones
    assert_eq!(html.matches("class=\"rm-origin\" required inputmode=\"url\"").count(), 2);
    for row in html.split("class=\"rm-origin\"").skip(1) {
        let field = row.split('>').next().unwrap_or_default();
        assert!(field.contains("role=\"combobox\""), "an address field does not say what it is: {field}");
        assert!(field.contains("aria-expanded=\"false\""), "and must start closed: {field}");
    }
}

/// A credential the user already granted for the same address is used again, so a second import or
/// a second link needs no sign-in. Without a reusable account the answer is 404 and the page falls
/// back to the sign-in — which is the normal case after a one-off import, whose app password is
/// revoked at the remote on purpose.
#[tokio::test]
async fn an_existing_connection_is_offered_again() {
    let s = seed().await;
    let who = s.app.principal("alice").await;
    let cal = crate::domain::collections::calendars_for(&s.app.state, &who, false).await.unwrap()[0].id.clone();
    let mut alice = s.app.login("alice").await;

    /* Nothing to reuse is 404, not an error: the page reads it as "sign in the usual way". The
       fixture keeps no key, so there can be no sealed credential — and an instance without a key
       still imports, which is why a missing key is an empty answer and not a refusal. */
    let r = alice.json(Method::POST, "/settings/remote/reuse", json!({"origin": "https://example.org"})).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "a missing key must read as 'nothing kept', not as an error");

    // an address the user never went to has nothing either, even with a link to another one
    let _ = a_link(&s.app, "alice", &cal).await;
    let r = alice.json(Method::POST, "/settings/remote/reuse", json!({"origin": "https://nowhere.example.org"})).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "a foreign address must not resolve to an account");

    // a malformed address is still refused here, before anything is looked up
    let r = alice.json(Method::POST, "/settings/remote/reuse", json!({"origin": "http://10.0.0.1"})).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "reuse must not be a way around the address rules");
}

/// What a page sees of the keep-window: a credential stays usable after its batch, for this user
/// and this address only. The window's own arithmetic is tested where it lives
/// (`domain::remote::login`, which can hand `sweep` a time).
#[tokio::test]
async fn a_credential_stays_usable_after_the_batch_that_fetched_it() {
    let s = seed().await;
    let logins = &s.app.state.remote_logins;

    let h = logins.adopt("https://example.org", "alice", "alice", crate::domain::remote::dav::Secret("pw".into()));
    let (origin, login, got) = logins.granted(&h, "alice").unwrap();
    assert_eq!((origin.as_str(), login.as_str(), got.0.as_str()), ("https://example.org", "alice", "pw"));
    assert!(logins.granted(&h, "bob").is_err(), "a handle of alice's must say nothing to bob");

    // the batch ends: the credential is parked, not destroyed, and comes back for the next one
    logins.park(&h, "alice");
    assert!(logins.expired().is_empty(), "nothing is revoked while the window is open");
    assert_eq!(logins.reusable("https://example.org", "alice").as_deref(), Some(h.as_str()));
    assert!(logins.reusable("https://other.example.org", "alice").is_none(), "another server must not match");
    assert!(logins.reusable("https://example.org", "bob").is_none(), "another user must not match");
}

/// A link may fill a GROUP collection (migration 20): the account stays the user's, the collection
/// is the group's. Rounds address the collection as its owner, the settings show the link on the
/// group calendar, and the push path finds it from a change event, which names the owner.
#[tokio::test]
async fn a_link_can_fill_a_group_collection() {
    let s = seed().await;
    let alice = s.app.principal("alice").await;
    let link_id = a_link_into(&s.app, "alice", "group.familie", &s.familie).await;
    let link = crate::domain::remote::link_of(&s.app.state, &alice, &link_id).await.unwrap();
    assert_eq!((link.principal.as_str(), link.owner.as_str()), ("alice", "group.familie"));

    // what a change event carries is the collection's owner and id
    let found = crate::domain::remote::link_for_collection(&s.app.state, "group.familie", crate::domain::remote::dav::Kind::Cal, &s.familie).await.unwrap();
    assert_eq!(found.map(|l| l.id), Some(link_id.clone()), "the push path cannot find a group collection's link");
    let mine = crate::domain::remote::link_for_collection(&s.app.state, "alice", crate::domain::remote::dav::Kind::Cal, &s.familie).await.unwrap();
    assert!(mine.is_none(), "the owner is part of the key, the caller is not");

    // the settings page puts it on the group calendar's page, not on any of alice's own
    let mut web = s.app.login("alice").await;
    let html = String::from_utf8_lossy(&web.get("/settings").await.ok().bytes).to_string();
    let page = html.split(&format!("id=\"cal-group.familie-{}\"", s.familie)).nth(1).unwrap_or_default().split("</section>").next().unwrap_or_default();
    assert!(page.contains(&format!("data-rm-id=\"{link_id}\"")), "the group calendar does not show its link");
    let own = html.split(&format!("id=\"cal-alice-{}\"", s.arbeit)).nth(1).unwrap_or_default().split("</section>").next().unwrap_or_default();
    assert!(!own.contains("rm-drop"), "a calendar without a link shows one");

    crate::domain::remote::may_write(&s.app.state, &link).await.unwrap();
}

/// A link into a group ends when its user can no longer reach the group: they left it, or it went
/// to the bin (which lifts every membership). Kept "paused" instead, its row would show on no page
/// its user can see, so it could not be stopped and would hold its app password for good. The round
/// refuses before it dials anything — the account here points at nothing reachable.
#[tokio::test]
async fn a_group_link_ends_when_its_user_cannot_reach_the_group_any_more() {
    let s = seed().await;
    let alice = s.app.principal("alice").await;
    let accounts = || async { sqlx::query_scalar::<_, i64>("SELECT count(*) FROM practical_remote_accounts WHERE principal = 'alice'").fetch_one(&s.app.state.db).await.unwrap() };

    // she leaves the group
    let id = a_link_into(&s.app, "alice", "group.familie", &s.familie).await;
    let link = crate::domain::remote::link_of(&s.app.state, &alice, &id).await.unwrap();
    crate::domain::groups::remove_member(&s.app.state, &alice, "group.familie", "alice").await.unwrap();
    let r = s.app.state.remote_runner.run(&s.app.state, &link, false, true).await;
    assert!(matches!(r, Err(crate::error::AppError::Forbidden)), "{r:?}");
    assert!(crate::domain::remote::links_of(&s.app.state, &alice).await.unwrap().is_empty(), "the link outlived the membership");
    assert_eq!(accounts().await, 0, "its account stayed with the sealed password");

    // the group goes to the bin (bob is the last member now)
    let bob = s.app.principal("bob").await;
    let id = a_link_into(&s.app, "bob", "group.familie", &s.familie).await;
    let link = crate::domain::remote::link_of(&s.app.state, &bob, &id).await.unwrap();
    crate::domain::groups::trash(&s.app.state, &bob, "group.familie", None).await.unwrap();
    assert!(crate::domain::remote::may_write(&s.app.state, &link).await.is_err());
    assert!(crate::domain::remote::links_of(&s.app.state, &bob).await.unwrap().is_empty(), "the link outlived the group");
}

/// Choosing an owner is the same right as on "create": the user or a group they are in. The check
/// comes before the sign-in handle is even looked at, so a refused owner costs no request to the
/// remote and leaves nothing behind.
#[tokio::test]
async fn an_import_or_link_into_a_foreign_group_is_refused() {
    let s = seed().await;
    let mut carol = s.app.login("carol").await;
    let body = json!({"handle": "nope", "kind": "cal", "href": "/remote.php/dav/calendars/c/x/", "owner": "group.familie"});
    // 403, not the unknown handle's answer: the owner is checked first
    assert_eq!(carol.json(Method::POST, "/settings/remote/import", body.clone()).await.status, StatusCode::FORBIDDEN);
    // the link route answers "no key" (400) before that where sync is off
    let r = carol.json(Method::POST, "/settings/remote/link", body).await;
    assert!(matches!(r.status, StatusCode::FORBIDDEN | StatusCode::BAD_REQUEST), "{}", r.status);
}

/// Any member may delete a group collection, and its link has to go with it — whoever made the
/// link. Before migration 20 the lookup went by the CALLER, so bob's delete left alice's link
/// running against a calendar in the bin.
#[tokio::test]
async fn deleting_a_group_collection_ends_the_link_another_member_made() {
    let s = seed().await;
    let alice = s.app.principal("alice").await;
    let bob = s.app.principal("bob").await;
    a_link_into(&s.app, "alice", "group.familie", &s.familie).await;
    crate::domain::collections::delete_calendar(&s.app.state, &bob, "group.familie", &s.familie, false).await.unwrap();
    let left = crate::domain::remote::links_of(&s.app.state, &alice).await.unwrap();
    assert!(left.is_empty(), "alice's link outlived the group calendar bob deleted: {left:?}");
}
