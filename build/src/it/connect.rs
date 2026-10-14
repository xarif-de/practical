//! Connecting a device (routes::connect): Login Flow v2 as DAVx⁵ drives it — no browser metadata on the app's
//! requests, the confirmation in the user's session — and the wizard's pairing.

use super::seed::seed;
use crate::testkit::PUBLIC_URL;
use axum::body::Body;
use axum::http::{Method, StatusCode};

/// What DAVx⁵ sends (davx5-ose `ProductIds.kt` User-Agent; ktor sets no Origin, no Sec-Fetch-*).
const UA: &str = "DAVx5/4.5.20-ose (at.bitfire.davdroid)";

fn path_of(url: &str) -> String {
    url.strip_prefix(PUBLIC_URL).unwrap_or_else(|| panic!("{url} is not on {PUBLIC_URL}")).to_string()
}

#[tokio::test]
async fn davx5_gets_an_app_password_through_the_users_confirmation() {
    let s = seed().await;
    let mut app = s.app.anonymous();
    // 1. the app starts a flow: no session, no browser metadata — the origin guard lets exactly this through
    let r = app.send(Method::POST, "/index.php/login/v2", &[("user-agent", UA)], Body::empty()).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let j = r.json();
    let (login, endpoint, token) = (j["login"].as_str().unwrap().to_string(), j["poll"]["endpoint"].as_str().unwrap().to_string(), j["poll"]["token"].as_str().unwrap().to_string());
    let poll = |t: &str| format!("token={t}");
    let form = [("content-type", "application/x-www-form-urlencoded")];
    // 2. nothing before the confirmation
    let r = app.send(Method::POST, &path_of(&endpoint), &form, Body::from(poll(&token))).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    // 3. the browser: a stranger is sent to the login and comes back to this page
    let r = s.app.anonymous().get(&path_of(&login)).await;
    assert_eq!(r.location().unwrap(), format!("/login?redirect_uri={}", crate::auth::urlenc(&path_of(&login))));
    let mut alice = s.app.login("alice").await;
    alice.get(&path_of(&login)).await.ok().has("DAVx⁵ verbinden?").has("Android-Gerät (DAVx⁵)").has("name=\"csrf\"");
    // the confirmation is a browser form: without the session's token it is refused
    let r = alice.post_form_raw(&path_of(&login), &[("name", "Pixel 9")]).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    // after the confirmation the page shows what DAVx⁵ asks next, with this user's account and collections
    alice.post_form(&path_of(&login), &[("name", "Pixel 9"), ("app", "DAVx⁵")]).await.ok().has("Verbunden").has("Gruppen sind Kategorien der Kontakte").has("Abschließen").has("alice");
    // a confirmed flow cannot be confirmed again
    alice.get(&path_of(&login)).await.ok().has("Dieser Link ist abgelaufen");
    // 4. the app polls: a wrong token gets nothing, the right one the credentials once
    let r = app.send(Method::POST, &path_of(&endpoint), &form, Body::from(poll("wrong"))).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    let r = app.send(Method::POST, &path_of(&endpoint), &form, Body::from(poll(&token))).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let g = r.json();
    assert_eq!((g["server"].as_str(), g["loginName"].as_str()), (Some("https://dav.example.org"), Some("alice")));
    let r = app.send(Method::POST, &path_of(&endpoint), &form, Body::from(poll(&token))).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "the credentials leave with their first answer");
    // 5. the password is a working app token, named as the user typed
    use base64ct::Encoding;
    let h = format!("Basic {}", base64ct::Base64::encode_string(format!("alice:{}", g["appPassword"].as_str().unwrap()).as_bytes()));
    let r = s.app.anonymous().send(Method::GET, "/api/colors", &[("authorization", &h)], Body::empty()).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    alice.get("/settings").await.ok().has("Pixel 9").has("Meine verbundenen Geräte");
}

#[tokio::test]
async fn only_the_app_endpoints_skip_the_origin_check() {
    let s = seed().await;
    let mut a = s.app.anonymous();
    // the confirmation from a foreign page is refused before anything else
    let r = a.send(Method::POST, "/index.php/login/v2/flow/x", &[("content-type", "application/x-www-form-urlencoded"), ("origin", "http://evil.example.org")], Body::from("name=x")).await;
    assert_eq!((r.status, r.body.as_str()), (StatusCode::FORBIDDEN, "cross-site request rejected"));
    // an unknown flow answers like a pending one
    let r = a.send(Method::POST, "/index.php/login/v2/poll/nope", &[("content-type", "application/x-www-form-urlencoded")], Body::from("token=x")).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    // an expired or unknown flow: the page says so instead of asking
    s.app.login("alice").await.get("/index.php/login/v2/flow/nope").await.ok().has("Dieser Link ist abgelaufen");
}

#[tokio::test]
async fn the_wizard_learns_when_the_other_device_opened_it_and_what_got_connected() {
    let s = seed().await;
    let mut alice = s.app.login("alice").await;
    let csrf = alice.csrf.clone();
    let hdr = [("content-type", "application/x-www-form-urlencoded"), ("sec-fetch-site", "same-origin"), ("x-csrf-token", csrf.as_str())];
    let r = alice.send(Method::POST, "/settings/connect/pair", &hdr, Body::from("pair=")).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let p = r.json();
    let pair = p["pair"].as_str().unwrap().to_string();
    assert_eq!(p["connect"]["url"].as_str().unwrap(), format!("{PUBLIC_URL}/connect/davx5"), "the code opens DAVx⁵ directly");
    assert!(p["connect"]["svg"].as_str().unwrap().starts_with("<svg"));
    // that link answers with the app link, no login needed (a camera app opens it in a fresh tab)
    let r = s.app.anonymous().get("/connect/davx5").await;
    assert!(r.status.is_redirection(), "{}", r.status);
    assert!(r.location().unwrap().starts_with("intent://localhost:4001#Intent;scheme=davx5;package=at.bitfire.davdroid;S.loginFlow=1;"));
    assert_eq!(p["stores"].as_array().unwrap().len(), 5);
    // without the CSRF header no pairing is made
    let r = alice.send(Method::POST, "/settings/connect/pair", &hdr[..2], Body::from("pair=")).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let st = format!("/settings/connect/status?pair={pair}");
    assert_eq!(alice.get_json(&st).await.json(), serde_json::json!({ "opened": false, "device": null }));
    // another user can neither open nor read it
    let mut bob = s.app.login("bob").await;
    bob.get(&format!("/settings?pair={pair}")).await.ok();
    assert_eq!(bob.get_json(&st).await.status, StatusCode::NOT_FOUND);
    assert_eq!(alice.get_json(&st).await.json()["opened"], false);
    // the phone opens the code's link (same user, its own session)
    let mut phone = s.app.login("alice").await;
    phone.get(&format!("/settings?pair={pair}")).await.ok();
    assert_eq!(alice.get_json(&st).await.json()["opened"], true);
    // the page reuses a pairing it was opened through
    let r = phone.send(Method::POST, "/settings/connect/pair", &[("content-type", "application/x-www-form-urlencoded"), ("sec-fetch-site", "same-origin"), ("x-csrf-token", &phone.csrf.clone())], Body::from(format!("pair={pair}"))).await;
    assert_eq!(r.json()["pair"].as_str(), Some(pair.as_str()));
    // a confirmed flow names the device in every open pairing of the user
    let start = s.app.anonymous().send(Method::POST, "/index.php/login/v2", &[("user-agent", UA)], Body::empty()).await.json();
    phone.post_form(&path_of(start["login"].as_str().unwrap()), &[("name", "Pixel 9")]).await.ok();
    assert_eq!(alice.get_json(&st).await.json(), serde_json::json!({ "opened": true, "device": "Pixel 9" }));
}

#[tokio::test]
async fn the_account_page_offers_the_wizard_and_the_manual_way() {
    let s = seed().await;
    let mut alice = s.app.login("alice").await;
    let page = alice.get("/settings").await;
    page.ok()
        .has("id=\"add\"").has("id=\"add-android\"").has("id=\"add-android-install\"").has("id=\"add-android-connect\"").has("id=\"add-done\"").has("id=\"manual\"")
        // DAVx⁵ opens in its Nextcloud login on this server; without the app Chrome comes back to the install step
        .has("intent://localhost:4001#Intent;scheme=davx5;package=at.bitfire.davdroid;S.loginFlow=1;S.browser_fallback_url=http%3A%2F%2Flocalhost%3A4001%2Fsettings%23add-android-install;end")
        .has("https://f-droid.org/packages/at.bitfire.davdroid/")
        .has("https://dav.example.org/caldav/principal/alice");
    // a new password shows once, on the manual page
    let r = alice.post_form("/settings/tokens", &[("name", "Thunderbird")]).await;
    assert_eq!(r.location().as_deref(), Some("/settings#manual"));
}
