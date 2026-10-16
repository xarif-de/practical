//! Connecting a device (routes::connect): Login Flow v2 as DAVx⁵ drives it — no browser metadata on the app's
//! requests, the confirmation in the user's session, the app token made when the app fetches it — and the
//! wizard's pairing.

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
    alice.post_form(&path_of(&login), &[("name", "Pixel 9")]).await.ok().has("Verbunden").has("Gruppen sind Kategorien der Kontakte").has("Abschließen").has("alice");
    // a second tap or a reload shows the same; another user gets nothing
    alice.post_form(&path_of(&login), &[("name", "Zweitname")]).await.ok().has("Verbunden");
    alice.get(&path_of(&login)).await.ok().has("Verbunden");
    s.app.login("bob").await.get(&path_of(&login)).await.ok().has("Dieser Link ist abgelaufen");
    // no app token before the app fetched it: a confirmation nobody fetches leaves nothing behind
    assert!(!alice.get("/settings").await.body.contains("Pixel 9"));
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
    assert!(!alice.get("/settings").await.body.contains("Zweitname"), "the first name counts");
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
async fn the_wizard_learns_which_device_got_connected() {
    let s = seed().await;
    let mut alice = s.app.login("alice").await;
    let csrf = alice.csrf.clone();
    let hdr = [("sec-fetch-site", "same-origin"), ("x-csrf-token", csrf.as_str())];
    let r = alice.send(Method::POST, "/settings/connect/pair", &hdr, Body::empty()).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let p = r.json();
    let pair = p["pair"].as_str().unwrap().to_string();
    assert!(p["connect"].as_str().unwrap().starts_with("<svg"));
    assert!(p["stores"]["fd"].as_str().unwrap().starts_with("<svg"));
    // the code's link answers with the app link, no login needed (a camera app opens it in a fresh tab)
    let r = s.app.anonymous().get("/connect/davx5").await;
    assert!(r.status.is_redirection(), "{}", r.status);
    assert!(r.location().unwrap().starts_with("intent://localhost:4001#Intent;scheme=davx5;package=at.bitfire.davdroid;S.loginFlow=1;"));
    // without the CSRF header no pairing is made
    let r = alice.send(Method::POST, "/settings/connect/pair", &hdr[..1], Body::empty()).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let st = format!("/settings/connect/status?pair={pair}");
    assert_eq!(alice.get_json(&st).await.json(), serde_json::json!({ "device": null }));
    // another user cannot read it
    assert_eq!(s.app.login("bob").await.get_json(&st).await.status, StatusCode::NOT_FOUND);
    // a confirmed flow names the device in every open pairing of the user once the app fetched its password
    let mut phone = s.app.login("alice").await;
    let start = s.app.anonymous().send(Method::POST, "/index.php/login/v2", &[("user-agent", UA)], Body::empty()).await.json();
    phone.post_form(&path_of(start["login"].as_str().unwrap()), &[("name", "Pixel 9")]).await.ok();
    assert_eq!(alice.get_json(&st).await.json(), serde_json::json!({ "device": null }), "not before the app fetched it");
    let body = format!("token={}", start["poll"]["token"].as_str().unwrap());
    let r = s.app.anonymous().send(Method::POST, &path_of(start["poll"]["endpoint"].as_str().unwrap()), &[("content-type", "application/x-www-form-urlencoded")], Body::from(body)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(alice.get_json(&st).await.json(), serde_json::json!({ "device": "Pixel 9" }));
}

#[tokio::test]
async fn the_account_page_offers_the_wizard_and_the_manual_way() {
    let s = seed().await;
    let mut alice = s.app.login("alice").await;
    let page = alice.get("/settings").await;
    page.ok()
        .has("id=\"add\"").has("id=\"add-android\"").has("id=\"add-android-install\"").has("id=\"add-android-connect\"").has("id=\"add-done\"").has("id=\"manual\"")
        // DAVx⁵ opens in its Nextcloud login on this server; without the app Chrome comes back to the install step
        .has("intent://localhost:4001#Intent;scheme=davx5;package=at.bitfire.davdroid;S.loginFlow=1;S.browser_fallback_url=http%3A%2F%2Flocalhost%3A4001%2Fsettings%3Fdavx5%3Dmissing%23add-android-install;end")
        .has("https://f-droid.org/packages/at.bitfire.davdroid/")
        .has("https://dav.example.org/caldav/principal/alice");
    // a new password shows once, on the manual page
    let r = alice.post_form("/settings/tokens", &[("name", "Thunderbird")]).await;
    assert_eq!(r.location().as_deref(), Some("/settings#manual"));
}

/// Safari on an iPhone (iOS 26).
const UA_IPHONE: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 26_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Mobile/15E148 Safari/604.1";

#[tokio::test]
async fn an_iphone_gets_a_profile_through_a_link_once() {
    let s = seed().await;
    let mut alice = s.app.login("alice").await;
    let csrf = alice.csrf.clone();
    let hdr = [("sec-fetch-site", "same-origin"), ("x-csrf-token", csrf.as_str())];
    assert_eq!(alice.send(Method::POST, "/settings/connect/apple", &hdr[..1], Body::empty()).await.status, StatusCode::FORBIDDEN, "no link without the CSRF header");
    let pair = alice.send(Method::POST, "/settings/connect/pair", &hdr, Body::empty()).await.json()["pair"].as_str().unwrap().to_string();
    let r = alice.send(Method::POST, "/settings/connect/apple", &hdr, Body::empty()).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let l = r.json();
    let (page, file) = (path_of(l["url"].as_str().unwrap()), path_of(l["file"].as_str().unwrap()));
    assert!(l["qr"].as_str().unwrap().starts_with("<svg"));
    assert_eq!(file, format!("{page}/PractiCal.mobileconfig"));
    assert_eq!(l["other"], false, "the asking browser installs profiles");
    let fx = [("sec-fetch-site", "same-origin"), ("x-csrf-token", csrf.as_str()), ("user-agent", "Mozilla/5.0 (iPhone; CPU iPhone OS 26_0 like Mac OS X) FxiOS/140.0 Mobile/15E148")];
    assert_eq!(alice.send(Method::POST, "/settings/connect/apple", &fx, Body::empty()).await.json()["other"], true, "Firefox only saves it");
    // the code opens the link's page in a browser without a session; Firefox is told to use Safari
    let iphone = [("user-agent", UA_IPHONE)];
    let mut phone = s.app.anonymous();
    phone.send(Method::GET, &page, &iphone, Body::empty()).await.ok().has("PractiCal.mobileconfig").has("alice");
    assert!(!phone.send(Method::GET, &page, &iphone, Body::empty()).await.body.contains("class=\"warn\""));
    phone.send(Method::GET, &page, &[("user-agent", "Mozilla/5.0 (iPhone; CPU iPhone OS 26_0 like Mac OS X) FxiOS/140.0 Mobile/15E148")], Body::empty()).await.ok().has("class=\"warn\"").has(&page);
    let st = format!("/settings/connect/status?pair={pair}");
    assert_eq!(alice.get_json(&st).await.json(), serde_json::json!({ "device": null }), "a page view connects nothing");
    // the download: the profile with a working app token; the wizard learns the device
    let r = phone.send(Method::GET, &file, &iphone, Body::empty()).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.header("content-type").as_deref(), Some("application/x-apple-aspen-config"));
    assert_eq!(r.header("cache-control").as_deref(), Some("no-store"));
    let pw = r.body.split("<key>CalDAVPassword</key><string>").nth(1).and_then(|x| x.split('<').next()).unwrap().to_string();
    assert!(r.body.contains("<string>https://dav.example.org/caldav-compat/principal/alice</string>"), "{}", r.body);
    use base64ct::Encoding;
    let h = format!("Basic {}", base64ct::Base64::encode_string(format!("alice:{pw}").as_bytes()));
    assert_eq!(s.app.anonymous().send(Method::GET, "/api/colors", &[("authorization", &h)], Body::empty()).await.status, StatusCode::OK);
    assert_eq!(alice.get_json(&st).await.json(), serde_json::json!({ "device": "iPhone" }));
    // a second download (Safari's retry) gets the same profile, no second token; another device nothing
    let again = phone.send(Method::GET, &file, &iphone, Body::empty()).await;
    assert_eq!(again.body, r.body);
    assert_eq!(alice.get("/settings").await.body.matches(">iPhone<span class=\"sub\">").count(), 1, "one device");
    let ipad = [("user-agent", "Mozilla/5.0 (iPad; CPU OS 26_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Mobile/15E148 Safari/604.1")];
    assert_eq!(phone.send(Method::GET, &file, &ipad, Body::empty()).await.status, StatusCode::GONE, "one link, one device");
    // a second iPhone (or the same one again): a name of its own, so the two rows can be told apart
    let file2 = path_of(alice.send(Method::POST, "/settings/connect/apple", &hdr, Body::empty()).await.json()["file"].as_str().unwrap());
    let second = phone.send(Method::GET, &file2, &iphone, Body::empty()).await;
    assert_eq!(second.status, StatusCode::OK);
    // a device is listed once it connected, not when its profile was fetched
    assert!(!alice.get("/settings").await.body.contains(">iPhone (2)<span"), "not connected yet");
    first_connection(&s, &second.body).await;
    alice.get("/settings").await.ok().has(">iPhone (2)<span class=\"sub\">");
    // an unknown link, a wrong file name: gone
    let r = phone.get("/connect/apple/nope").await;
    assert_eq!(r.status, StatusCode::GONE);
    r.has("Dieser Link ist abgelaufen");
    assert_eq!(phone.get(&format!("{page}/other.mobileconfig")).await.status, StatusCode::GONE);
}

#[tokio::test]
async fn an_ipad_is_named_by_its_browser() {
    let s = seed().await;
    let mut alice = s.app.login("alice").await;
    let csrf = alice.csrf.clone();
    let hdr = [("sec-fetch-site", "same-origin"), ("x-csrf-token", csrf.as_str())];
    let file = path_of(alice.send(Method::POST, "/settings/connect/apple", &hdr, Body::empty()).await.json()["file"].as_str().unwrap());
    // iPadOS Safari asks for desktop sites: it says "Macintosh"
    let ipad = [("user-agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15")];
    let r = s.app.anonymous().send(Method::GET, &file, &ipad, Body::empty()).await;
    assert_eq!(r.status, StatusCode::OK);
    first_connection(&s, &r.body).await;
    alice.get("/settings").await.ok().has(">iPad<span class=\"sub\">");
}

/// The device's first request with the profile's password: RustiCal records the use, and only then the
/// settings list the device (domain::tokens::devices).
async fn first_connection(s: &super::seed::Seed, profile: &str) {
    use base64ct::Encoding;
    let user = profile.split("<key>CalDAVUsername</key><string>").nth(1).and_then(|x| x.split('<').next()).unwrap();
    let pw = profile.split("<key>CalDAVPassword</key><string>").nth(1).and_then(|x| x.split('<').next()).unwrap();
    let h = format!("Basic {}", base64ct::Base64::encode_string(format!("{user}:{pw}").as_bytes()));
    assert_eq!(s.app.anonymous().send(Method::GET, "/api/colors", &[("authorization", &h)], Body::empty()).await.status, StatusCode::OK);
}

#[tokio::test]
async fn an_iphone_by_hand_gets_a_password_of_its_own() {
    let s = seed().await;
    let mut alice = s.app.login("alice").await;
    let csrf = alice.csrf.clone();
    let hdr = [("sec-fetch-site", "same-origin"), ("x-csrf-token", csrf.as_str())];
    assert_eq!(alice.send(Method::POST, "/settings/connect/manual?name=iPhone", &hdr[..1], Body::empty()).await.status, StatusCode::FORBIDDEN);
    let resp = alice.send(Method::POST, "/settings/connect/manual?name=iPhone", &hdr, Body::empty()).await;
    assert_eq!(resp.header("cache-control").as_deref(), Some("no-store"), "a password never goes into a cache");
    let r = resp.json();
    assert_eq!(r["name"], "iPhone");
    use base64ct::Encoding;
    let h = format!("Basic {}", base64ct::Base64::encode_string(format!("alice:{}", r["password"].as_str().unwrap()).as_bytes()));
    assert_eq!(s.app.anonymous().send(Method::GET, "/api/colors", &[("authorization", &h)], Body::empty()).await.status, StatusCode::OK);
    assert_eq!(alice.send(Method::POST, "/settings/connect/manual?name=iPhone", &hdr, Body::empty()).await.json()["name"], "iPhone (2)");
    // that one was never used: a new attempt does not count it, and becomes "iPhone (2)" again, not "(3)"
    assert_eq!(alice.send(Method::POST, "/settings/connect/manual?name=iPhone", &hdr, Body::empty()).await.json()["name"], "iPhone (2)");
    // the guide lists what a group's calendars need: an account as user$group
    alice.get("/settings").await.ok().has("id=\"add-ios-manual\"").has("alice$group.familie");
}

#[tokio::test]
async fn a_computer_hands_the_manual_setup_to_the_iphone_without_a_login() {
    let s = seed().await;
    let mut alice = s.app.login("alice").await;
    let csrf = alice.csrf.clone();
    let hdr = [("sec-fetch-site", "same-origin"), ("x-csrf-token", csrf.as_str())];
    let pair = alice.send(Method::POST, "/settings/connect/pair", &hdr, Body::empty()).await.json()["pair"].as_str().unwrap().to_string();
    let l = alice.send(Method::POST, "/settings/connect/apple", &hdr, Body::empty()).await.json();
    assert!(l["manual_qr"].as_str().unwrap().starts_with("<svg"));
    let page = path_of(l["url"].as_str().unwrap());
    let iphone = [("user-agent", UA_IPHONE)];
    let mut phone = s.app.anonymous();
    // the guide with the values and the group's login; no password before it is asked for
    phone.send(Method::GET, &format!("{page}/manual"), &iphone, Body::empty()).await.ok().has("https://dav.example.org").has("alice$group.familie").has("data-pw-url");
    let r = phone.send(Method::GET, &format!("{page}/password"), &iphone, Body::empty()).await;
    assert_eq!(r.header("cache-control").as_deref(), Some("no-store"), "the phone's password: never cached");
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let pw = r.json()["password"].as_str().unwrap().to_string();
    use base64ct::Encoding;
    let h = format!("Basic {}", base64ct::Base64::encode_string(format!("alice:{pw}").as_bytes()));
    assert_eq!(s.app.anonymous().send(Method::GET, "/api/colors", &[("authorization", &h)], Body::empty()).await.status, StatusCode::OK);
    assert_eq!(phone.send(Method::GET, &format!("{page}/password"), &iphone, Body::empty()).await.json()["password"], pw, "a second copy: the same password");
    // the computer's wizard learns that the phone went on by hand: not the profile's install step
    assert_eq!(alice.get_json(&format!("/settings/connect/status?pair={pair}")).await.json(), serde_json::json!({ "device": "iPhone", "manual": true }));
    assert_eq!(phone.send(Method::GET, &format!("{page}/PractiCal.mobileconfig"), &iphone, Body::empty()).await.status, StatusCode::GONE, "one link, one kind");
}
