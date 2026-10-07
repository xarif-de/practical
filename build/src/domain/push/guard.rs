//! What the server may POST to and a round that keeps going when one subscription or one calendar fails.

use super::*;
use crate::testkit::{TestApp, vevent};
use std::sync::{Arc, Mutex};

/// A valid subscription's keys (web-push's own example): an uncompressed P-256 point, a 16-byte secret.
pub const P256DH: &str = "BLMbF9ffKBiWQLCKvTHb6LO8Nb6dcUh6TItC455vu2kElga6PQvUmaFyCdykxY2nOSSL3yKgfbmFLRTUaGv4yV8";
pub const AUTH: &str = "xS03Fi5ErfTNH_l9WHE9Ig";

/// An endpoint whose 40-byte log cut lands inside the `ä`.
const CUT_MID_CHAR: &str = "https://127.0.0.1:4598/xxxxxxxxxxxxxxxxä";

#[test]
fn only_the_browsers_push_services_are_allowed() {
    for ok in [
        "https://fcm.googleapis.com/fcm/send/eKClHsXFm9E:APA91bH2x3gNOMv4dF1lQfCgIfOet8EngqKCAUS5DncLOd5hzfSUxcjigIjw9ws-bqa",
        "https://fcm.googleapis.com/wp/dHIoDxE7Hdg:APA91bH1Zj0kNa",
        "https://jmt17.google.com/fcm/send/abc:def",
        "https://updates.push.services.mozilla.com/wpush/v2/gAAAAABk-x_y",
        "https://web.push.apple.com/QGuQyavXutnMHTSzQ4z3fZrS",
        "https://api.push.apple.com/x",
        "https://wns2-par02p.notify.windows.com/w/?token=BQYAAAB%2bAbc%3d",
    ] {
        assert_eq!(check_subscription(ok, P256DH, AUTH), Ok(()), "{ok}");
    }
    let long = format!("https://fcm.googleapis.com/fcm/send/{}", "a".repeat(MAX_ENDPOINT));
    for (bad, why) in [
        ("http://fcm.googleapis.com/fcm/send/x", "err-push-https"),
        ("ftp://fcm.googleapis.com/x", "err-push-https"),
        (CUT_MID_CHAR, "err-push-endpoint"),
        ("https://127.0.0.1/x", "err-push-endpoint"),
        ("https://2130706433/x", "err-push-endpoint"), // 127.0.0.1 as a number
        ("https://0x7f.1/x", "err-push-endpoint"),
        ("https://[::1]/x", "err-push-endpoint"),
        ("https://[::ffff:127.0.0.1]/x", "err-push-endpoint"),
        ("https://localhost/x", "err-push-endpoint"),
        ("https://plan.xarif.de/api/x", "err-push-endpoint"),
        ("https://push.apple.com/x", "err-push-endpoint"),             // the suffix needs a label in front
        ("https://notify.windows.com/x", "err-push-endpoint"),
        ("https://evilpush.apple.com.attacker.example/x", "err-push-endpoint"),
        ("https://fcm.googleapis.com.attacker.example/x", "err-push-endpoint"),
        ("https://attacker.example/fcm.googleapis.com", "err-push-endpoint"),
        ("https://attacker.example/?fcm.googleapis.com", "err-push-endpoint"),
        ("https://fcm.googleapis.com.:443/x", "err-push-endpoint"),
        ("https://fcm.googleapis.com./x", "err-push-endpoint"),
        ("https://fcm.googleapis.com:8443/x", "err-push-endpoint"),
        ("https://fcm.googleapis.com:443/x", "err-push-endpoint"),      // not the normal form
        ("https://user@fcm.googleapis.com/x", "err-push-endpoint"),
        ("https://127.0.0.1@fcm.googleapis.com/x", "err-push-endpoint"),
        ("https://fcm.googleapis.com@127.0.0.1/x", "err-push-endpoint"),
        ("https://fcm.googleapis.com\\@127.0.0.1/x", "err-push-endpoint"),
        ("https://FCM.googleapis.com/x", "err-push-endpoint"),
        ("https://fcm.googleapis.com/x#frag", "err-push-endpoint"),
        ("https://fcm.googleapis.com/a b", "err-push-endpoint"),
        ("https://fcm.googleapis.com", "err-push-endpoint"),
        ("https://", "err-push-endpoint"),
        (long.as_str(), "err-push-endpoint"),
    ] {
        assert_eq!(check_subscription(bad, P256DH, AUTH), Err(why), "{bad}");
    }
    let e = "https://fcm.googleapis.com/fcm/send/x";
    for (p, a) in [
        ("", AUTH),
        (&P256DH[1..], AUTH),
        (&format!("{P256DH}="), AUTH),                                  // padded
        (&format!("A{}", &P256DH[1..]), AUTH),                          // not an uncompressed point (0x00…)
        (&P256DH.replacen('K', "+", 1), AUTH),                           // not the url alphabet
        (P256DH, "xS03Fi5ErfTNH_l9WHE9"),
        (P256DH, "xS03Fi5ErfTNH_l9WHE9Ig=="),
        (P256DH, ""),
    ] {
        assert_eq!(check_subscription(e, p, a), Err("err-push-keys"), "{p} {a}");
    }
}

#[test]
fn only_public_addresses_are_dialled() {
    for ip in ["127.0.0.1", "10.43.0.10", "172.16.0.1", "192.168.1.1", "169.254.169.254", "100.64.0.1", "0.0.0.0", "255.255.255.255", "224.0.0.1", "192.0.0.8", "198.18.0.1", "240.0.0.1", "::1", "::", "::ffff:127.0.0.1", "::ffff:10.0.0.1", "64:ff9b::7f00:1", "2002:7f00:1::", "fe80::1", "fc00::1", "fd12::1", "2001:db8::1", "2001::1", "::127.0.0.1", "ff02::1"] {
        assert!(!public_ip(ip.parse().unwrap()), "{ip}");
    }
    for ip in ["142.250.185.74", "17.253.144.10", "34.107.221.82", "2a00:1450:4001:80b::200a", "::ffff:142.250.185.74", "64:ff9b::8efa:b94a"] {
        assert!(public_ip(ip.parse().unwrap()), "{ip}");
    }
}

#[tokio::test]
async fn a_name_that_resolves_to_loopback_is_not_dialled() {
    use reqwest::dns::Resolve;
    let r = PublicOnly.resolve("localhost".parse().unwrap()).await;
    assert!(r.is_err_and(|e| e.to_string().contains("no public address")));
}

/// Resolves every name to 127.0.0.1 — a stand-in for DNS pointing a push host at a local server.
struct ToLoopback;

impl reqwest::dns::Resolve for ToLoopback {
    fn resolve(&self, _: reqwest::dns::Name) -> reqwest::dns::Resolving {
        Box::pin(async { Ok(Box::new(std::iter::once(std::net::SocketAddr::from(([127, 0, 0, 1], 0)))) as reqwest::dns::Addrs) })
    }
}

/// A local server that answers every request with a redirect to itself; counts the requests.
async fn redirector() -> (u16, Arc<Mutex<usize>>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    let hits = Arc::new(Mutex::new(0));
    let h = hits.clone();
    tokio::spawn(async move {
        while let Ok((mut c, _)) = l.accept().await {
            let mut buf = [0u8; 4096];
            let _ = c.read(&mut buf).await;
            *h.lock().unwrap() += 1;
            let _ = c.write_all(b"HTTP/1.1 302 Found\r\nLocation: /next\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
        }
    });
    (port, hits)
}

#[tokio::test]
async fn redirects_are_not_followed() {
    let (port, hits) = redirector().await;
    // the delivery client's settings, over http and a resolver that leads to the local server
    let client = client_builder(ToLoopback).build().unwrap();
    let r = client.post(format!("http://push.test:{port}/x")).send().await.unwrap();
    assert_eq!((r.status().as_u16(), *hits.lock().unwrap()), (302, 1));
    // reqwest's default, for contrast: it follows (10 hops)
    let (port, hits) = redirector().await;
    let _ = reqwest::Client::builder().no_proxy().dns_resolver(ToLoopback).build().unwrap().post(format!("http://push.test:{port}/x")).send().await;
    assert_eq!(*hits.lock().unwrap(), 11);
    // and the real one speaks https only
    let e = push_client().post(format!("http://push.test:{port}/x")).send().await.unwrap_err();
    assert!(e.is_builder(), "{e:?}");
}

/// The delivery refuses a stored row the check refuses before it signs or dials anything.
#[tokio::test]
async fn send_refuses_an_endpoint_outside_the_allowlist() {
    let app = TestApp::new().await;
    for e in [CUT_MID_CHAR, "https://127.0.0.1:4000/", "http://fcm.googleapis.com/fcm/send/x"] {
        let r = send(&app.state, &Sub { endpoint: e.into(), p256dh: P256DH.into(), auth: AUTH.into() }, "{}").await.unwrap_err();
        assert!(!r.gone && r.msg.starts_with("err-push-"), "{e}: {r:?}");
    }
    // a valid row gets past the check (and here stops at the missing VAPID key)
    let r = send(&app.state, &Sub { endpoint: "https://fcm.googleapis.com/fcm/send/x".into(), p256dh: P256DH.into(), auth: AUTH.into() }, "{}").await.unwrap_err();
    assert_eq!(r.msg, "push disabled");
}

fn at(s: &str) -> chrono::DateTime<Utc> {
    chrono::DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
}

async fn subscribe(app: &TestApp, id: &str, principal: &str, endpoint: &str) {
    sqlx::query("INSERT INTO practical_push_subscriptions (id, principal, endpoint, p256dh, auth) VALUES (?, ?, ?, ?, ?)").bind(id).bind(principal).bind(endpoint).bind(P256DH).bind(AUTH).execute(&app.state.db).await.unwrap();
}

async fn endpoints(app: &TestApp) -> Vec<String> {
    sqlx::query("SELECT endpoint FROM practical_push_subscriptions ORDER BY endpoint").fetch_all(&app.state.db).await.unwrap().iter().map(|r| r.get("endpoint")).collect()
}

/// alice and bob share a group calendar with a reminder at 08:30Z.
async fn shared_reminder() -> TestApp {
    let app = TestApp::new().await;
    app.user("alice", "Alice").await;
    app.user("bob", "Bob").await;
    app.group("group.fam", "Familie", &["alice", "bob"]).await;
    let fam = app.calendar("group.fam", "Familie", "#af52de", &["VEVENT"]).await;
    app.put_ics("group.fam", &fam, "g", &vevent("g", "Treffen", "DTSTART:20261014T084000Z\nDTEND:20261014T090000Z\nBEGIN:VALARM\nACTION:DISPLAY\nDESCRIPTION:x\nTRIGGER:-PT10M\nEND:VALARM")).await;
    app
}

/// Alice's stored endpoint used to panic the round in its log line, every minute,
/// and bob (sorted after her) never got his reminder. Uses only what the loop had before the fix.
#[tokio::test]
async fn a_malformed_stored_endpoint_does_not_stop_the_round() {
    // the log line was the panic: it only runs with a subscriber listening (as in production)
    let _log = tracing::subscriber::set_default(tracing_subscriber::fmt().with_test_writer().finish());
    let app = shared_reminder().await;
    subscribe(&app, "a", "alice", CUT_MID_CHAR).await;
    subscribe(&app, "b", "bob", "https://fcm.googleapis.com/fcm/send/bob").await;
    let mut plan = Plan::default();
    let mut watch = crate::db::Watch::new(app.state.db.clone());
    let (fired, _) = tick(&app.state, &mut plan, watch.stamp().await, at("2026-10-14T08:30:17Z")).await.unwrap();
    // alice has no deliverable subscription: nothing planned for her; bob's reminder goes out
    assert_eq!(fired.iter().map(|f| f.principal.as_str()).collect::<Vec<_>>(), ["bob"]);
}

/// One subscription that errs, is gone or panics costs only that delivery.
#[tokio::test]
async fn one_failing_subscription_does_not_stop_the_others() {
    let app = shared_reminder().await;
    subscribe(&app, "a0", "alice", CUT_MID_CHAR).await;
    subscribe(&app, "a1", "alice", "https://fcm.googleapis.com/fcm/send/panics").await;
    subscribe(&app, "a2", "alice", "https://fcm.googleapis.com/fcm/send/gone").await;
    subscribe(&app, "a3", "alice", "https://fcm.googleapis.com/fcm/send/fails").await;
    subscribe(&app, "a4", "alice", "https://updates.push.services.mozilla.com/wpush/v2/alice").await;
    subscribe(&app, "b", "bob", "https://web.push.apple.com/bob").await;
    let tried = Arc::new(Mutex::new(vec![]));
    let t = tried.clone();
    let deliver = move |s: Sub, _payload: String| {
        let t = t.clone();
        async move {
            t.lock().unwrap().push(s.endpoint.clone());
            match s.endpoint.rsplit('/').next().unwrap() {
                "panics" => panic!("a delivery panicked"),
                "gone" => Err(Failure { gone: true, msg: "endpoint not valid: code 410".into() }),
                "fails" => Err(Failure::other("server error: code 503")),
                _ => Ok(()),
            }
        }
    };
    let mut plan = Plan::default();
    let mut watch = crate::db::Watch::new(app.state.db.clone());
    let (fired, sent) = tick_with(&app.state, &mut plan, watch.stamp().await, at("2026-10-14T08:30:17Z"), deliver).await.unwrap();
    assert_eq!((fired.len(), sent), (2, 2));
    let mut tried = tried.lock().unwrap().clone();
    tried.sort();
    // the malformed one is never handed to the delivery; every other one is tried once
    assert_eq!(tried, ["https://fcm.googleapis.com/fcm/send/fails", "https://fcm.googleapis.com/fcm/send/gone", "https://fcm.googleapis.com/fcm/send/panics", "https://updates.push.services.mozilla.com/wpush/v2/alice", "https://web.push.apple.com/bob"]);
    // only the one the push service no longer knows is deleted
    assert_eq!(endpoints(&app).await, [CUT_MID_CHAR, "https://fcm.googleapis.com/fcm/send/fails", "https://fcm.googleapis.com/fcm/send/panics", "https://updates.push.services.mozilla.com/wpush/v2/alice", "https://web.push.apple.com/bob"]);
}

/// An alarm offset past chrono's range panicked the build of the whole round,
/// every minute (each time a full build). Now only that calendar's reminders are missing, the others go
/// out, and the failing build is retried with a growing pause (and after every write), not every minute.
#[tokio::test]
async fn a_poison_object_costs_only_its_calendar() {
    let app = shared_reminder().await;
    let own = app.calendar("alice", "Plan", "#3478f6", &["VEVENT"]).await;
    app.put_ics("alice", &own, "p", &vevent("p", "Gift", "DTSTART:20261014T090000Z\nDTEND:20261014T100000Z\nBEGIN:VALARM\nACTION:DISPLAY\nDESCRIPTION:x\nTRIGGER:-PT1000000000000000M\nEND:VALARM")).await;
    app.put_ics("alice", &own, "o", &vevent("o", "Eigenes", "DTSTART:20261014T085000Z\nDTEND:20261014T090000Z\nBEGIN:VALARM\nACTION:DISPLAY\nDESCRIPTION:x\nTRIGGER:-PT5M\nEND:VALARM")).await;
    subscribe(&app, "a", "alice", "https://fcm.googleapis.com/fcm/send/alice").await;
    subscribe(&app, "b", "bob", "https://web.push.apple.com/bob").await;
    let mut plan = Plan::default();
    let mut watch = crate::db::Watch::new(app.state.db.clone());
    let mut fired = vec![];
    let start = at("2026-10-14T08:00:17Z");
    for m in 0..60 {
        let now = start + Duration::minutes(m);
        let (f, _) = tick_with(&app.state, &mut plan, watch.stamp().await, now, |_, _| async { Ok(()) }).await.unwrap();
        fired.extend(f.into_iter().map(|f| format!("{} {} {}", now.format("%H:%M"), f.principal, f.tag.rsplit('/').next().unwrap())));
    }
    let poisoned = plan.retry_at.is_some();
    let mut want = vec!["08:29 alice g", "08:29 bob g"];
    if !poisoned {
        // the poison object made harmless by the parser (W3 package `data`): its calendar's other reminder fires too
        want.push("08:44 alice o");
    }
    assert_eq!(fired, want);
    // builds: the first, the retries after 1, 2, 4, 8, 16, 16 minutes, and one after each sent-log write
    // (08:30, two rows in one round); a full build every minute before
    assert!(plan.builds <= 9, "{} builds", plan.builds);
}

/// The logout hook drops what this session subscribed, of this session's user, and nothing else.
#[tokio::test]
async fn logout_forgets_this_sessions_subscriptions() {
    let app = TestApp::new().await;
    // the production store (SQLite, practical_sessions): the endpoints survive a save and a reload
    let store = Arc::new(crate::sessions::DbStore::new(app.state.db.clone()));
    let session = tower_sessions::Session::new(None, store.clone(), None);
    // no user in the session: nothing happens
    assert_eq!(forget_on_logout(&app.state, &session).await, 0);
    session.insert(crate::ctx::SESSION_USER, "alice").await.unwrap();
    subscribe(&app, "a1", "alice", "https://fcm.googleapis.com/fcm/send/here").await;
    subscribe(&app, "a2", "alice", "https://fcm.googleapis.com/fcm/send/other-device").await;
    subscribe(&app, "b", "bob", "https://fcm.googleapis.com/fcm/send/bob").await;
    remember_in_session(&session, "https://fcm.googleapis.com/fcm/send/here").await;
    remember_in_session(&session, "https://fcm.googleapis.com/fcm/send/bob").await; // not alice's: stays
    remember_in_session(&session, "https://fcm.googleapis.com/fcm/send/here").await;
    session.save().await.unwrap();
    let session = tower_sessions::Session::new(session.id(), store, None);
    assert_eq!(forget_on_logout(&app.state, &session).await, 1);
    assert_eq!(endpoints(&app).await, ["https://fcm.googleapis.com/fcm/send/bob", "https://fcm.googleapis.com/fcm/send/other-device"]);
    // the session keeps the newest MAX_SUBSCRIPTIONS endpoints
    for i in 0..15 {
        remember_in_session(&session, &format!("https://fcm.googleapis.com/fcm/send/{i}")).await;
    }
    let list: Vec<String> = session.get(SESSION_ENDPOINTS).await.unwrap().unwrap();
    assert_eq!((list.len(), list[0].as_str()), (MAX_SUBSCRIPTIONS as usize, "https://fcm.googleapis.com/fcm/send/5"));
}
