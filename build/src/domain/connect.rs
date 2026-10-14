//! Connecting a device: the server side of Nextcloud Login Flow v2 (which DAVx⁵ speaks), the pairing
//! that lets the "add a device" wizard continue on another device, and the QR codes and store links
//! the wizard shows.
//!
//! Login Flow v2, as DAVx⁵ drives it (davx5-ose `NextcloudLoginFlow.kt`, `NextcloudLogin.kt`, 2026-10-14):
//! 1. `POST <base>/index.php/login/v2` → `{poll:{token,endpoint},login}`; the app opens `login` in a
//!    Custom Tab (the default browser's cookies, so a signed-in user needs no second login).
//! 2. The user confirms on that page ([`Flows::grant`] creates an app token).
//! 3. Back in the app, ONE `POST <endpoint>` with `token=…` → `{server, loginName, appPassword}`. DAVx⁵
//!    appends `remote.php/dav` to `server` and, finding no principal there, falls back to
//!    `/.well-known/caldav` on that host (`DavResourceFinder.kt` `findInitialConfiguration`); RustiCal
//!    answers that with a redirect (measured on dav.xarif.de: 308 → /caldav, 308 → /carddav).
//!
//! Both stores live in memory only: a flow holds a fresh app password until the app fetched it, and
//! neither is worth keeping across a restart (the user starts over).

use std::collections::HashMap;
use std::sync::Mutex;

/// The life of a pending flow: RustiCal's own flows last 10 minutes (`nextcloud_login/routes.rs`, v0.16.6).
const FLOW_SECS: i64 = 10 * 60;
/// Pending flows at most. `POST …/login/v2` needs no login, so the map must not grow with requests: a full
/// map drops its oldest flow.
const FLOW_MAX: usize = 256;
/// The life of a pairing: long enough to install an app on the other device in between.
const PAIR_SECS: i64 = 30 * 60;
/// Pairings per user at most (a desktop, a phone, a reload of each).
const PAIR_MAX_PER_USER: usize = 6;
/// A device name as the store keeps it (the token form's `maxlength`).
pub const NAME_MAX: usize = 60;

/// What the app fetches once: the DAV server, the principal, the app password.
#[derive(Clone, serde::Serialize)]
pub struct Grant {
    pub server: String,
    #[serde(rename = "loginName")]
    pub login_name: String,
    #[serde(rename = "appPassword")]
    pub app_password: String,
}

struct Flow {
    poll_token: String,
    /// the app's User-Agent: shown on the confirmation page, so the user sees which app asks
    app: String,
    at: chrono::DateTime<chrono::Utc>,
    grant: Option<Grant>,
}

#[derive(Default)]
pub struct Flows {
    by_id: Mutex<HashMap<String, Flow>>,
}

fn random_token() -> String {
    use rand::RngExt;
    use rand::distr::Alphanumeric;
    rand::rng().sample_iter(Alphanumeric).map(char::from).take(48).collect()
}

impl Flows {
    /// Starts a flow for the app `app` (its User-Agent) and returns (id, poll token).
    pub fn start(&self, app: &str, now: chrono::DateTime<chrono::Utc>) -> (String, String) {
        let id = uuid::Uuid::new_v4().to_string();
        let token = random_token();
        let mut m = self.by_id.lock().unwrap();
        m.retain(|_, f| now.signed_duration_since(f.at).num_seconds() < FLOW_SECS);
        while m.len() >= FLOW_MAX {
            let Some(oldest) = m.iter().min_by_key(|(_, f)| f.at).map(|(k, _)| k.clone()) else { break };
            m.remove(&oldest);
        }
        let app: String = app.chars().filter(|c| !c.is_control()).take(80).collect();
        m.insert(id.clone(), Flow { poll_token: token.clone(), app, at: now, grant: None });
        (id, token)
    }

    /// The app name of a flow that can still be confirmed (pending, not expired, not confirmed).
    pub fn pending(&self, id: &str, now: chrono::DateTime<chrono::Utc>) -> Option<String> {
        let m = self.by_id.lock().unwrap();
        m.get(id).filter(|f| f.grant.is_none() && now.signed_duration_since(f.at).num_seconds() < FLOW_SECS).map(|f| f.app.clone())
    }

    /// Stores the grant of a pending flow; false when the flow is gone, expired or already confirmed.
    pub fn grant(&self, id: &str, grant: Grant, now: chrono::DateTime<chrono::Utc>) -> bool {
        let mut m = self.by_id.lock().unwrap();
        match m.get_mut(id) {
            Some(f) if f.grant.is_none() && now.signed_duration_since(f.at).num_seconds() < FLOW_SECS => {
                f.grant = Some(grant);
                true
            }
            _ => false,
        }
    }

    /// The app's poll: the grant once (then the flow is gone), `None` while pending, for a wrong token
    /// and for an unknown or expired flow — the app cannot tell these apart, as with Nextcloud.
    pub fn poll(&self, id: &str, token: &str, now: chrono::DateTime<chrono::Utc>) -> Option<Grant> {
        let mut m = self.by_id.lock().unwrap();
        let f = m.get(id)?;
        if !crate::csrf::same(f.poll_token.as_bytes(), token.as_bytes()) || now.signed_duration_since(f.at).num_seconds() >= FLOW_SECS {
            return None;
        }
        f.grant.as_ref()?;
        m.remove(id).and_then(|f| f.grant)
    }
}

/// One run of the wizard: the other device opens it ([`Pairs::open`]), a confirmed flow of the same user
/// names the new device ([`Pairs::connected`]); the wizard reads both through [`Pairs::status`].
struct Pair {
    user: String,
    at: chrono::DateTime<chrono::Utc>,
    opened: bool,
    device: Option<String>,
}

#[derive(Default)]
pub struct Pairs {
    by_id: Mutex<HashMap<String, Pair>>,
}

#[derive(serde::Serialize, Debug, PartialEq)]
pub struct PairStatus {
    pub opened: bool,
    pub device: Option<String>,
}

impl Pairs {
    pub fn create(&self, user: &str, now: chrono::DateTime<chrono::Utc>) -> String {
        let id = random_token();
        let mut m = self.by_id.lock().unwrap();
        m.retain(|_, p| now.signed_duration_since(p.at).num_seconds() < PAIR_SECS);
        while m.values().filter(|p| p.user == user).count() >= PAIR_MAX_PER_USER {
            let Some(oldest) = m.iter().filter(|(_, p)| p.user == user).min_by_key(|(_, p)| p.at).map(|(k, _)| k.clone()) else { break };
            m.remove(&oldest);
        }
        m.insert(id.clone(), Pair { user: user.to_string(), at: now, opened: false, device: None });
        id
    }

    /// The wizard was opened on another device through the pairing's link (only by its own user).
    pub fn open(&self, id: &str, user: &str) {
        if let Some(p) = self.by_id.lock().unwrap().get_mut(id).filter(|p| p.user == user) {
            p.opened = true;
        }
    }

    /// A device of `user` was just connected: every open pairing of theirs learns its name.
    pub fn connected(&self, user: &str, device: &str) {
        for p in self.by_id.lock().unwrap().values_mut().filter(|p| p.user == user) {
            p.device = Some(device.to_string());
        }
    }

    pub fn status(&self, id: &str, user: &str, now: chrono::DateTime<chrono::Utc>) -> Option<PairStatus> {
        let m = self.by_id.lock().unwrap();
        m.get(id).filter(|p| p.user == user && now.signed_duration_since(p.at).num_seconds() < PAIR_SECS).map(|p| PairStatus { opened: p.opened, device: p.device.clone() })
    }
}

/// The in-process state of both, held in [`crate::state::AppState`].
#[derive(Default)]
pub struct Connect {
    pub flows: Flows,
    pub pairs: Pairs,
}

/// Where DAVx⁵ comes from (www.davx5.com/download, 2026-10-14): key, name, icon, link, the link as the
/// wizard prints it under the QR code, free of charge.
pub struct Store {
    pub key: &'static str,
    pub name: &'static str,
    pub icon: &'static str,
    pub url: &'static str,
    pub short: &'static str,
    pub free: bool,
}

pub const STORES: [Store; 5] = [
    Store { key: "gp", name: "Google Play", icon: "googleplay", url: "https://play.google.com/store/apps/details?id=at.bitfire.davdroid", short: "play.google.com/…/at.bitfire.davdroid", free: false },
    Store { key: "fd", name: "F-Droid", icon: "fdroid", url: "https://f-droid.org/packages/at.bitfire.davdroid/", short: "f-droid.org/packages/at.bitfire.davdroid", free: true },
    Store { key: "gs", name: "Galaxy Store", icon: "galaxystore", url: "https://galaxystore.samsung.com/detail/at.bitfire.davdroid", short: "galaxystore.samsung.com/…/at.bitfire.davdroid", free: false },
    Store { key: "ag", name: "AppGallery", icon: "appgallery", url: "https://appgallery.huawei.com/app/C101567605", short: "appgallery.huawei.com/app/C101567605", free: false },
    Store { key: "gh", name: "GitHub", icon: "github", url: "https://github.com/bitfireAT/davx5-ose/releases", short: "github.com/bitfireAT/davx5-ose/releases", free: true },
];

/// The link that opens DAVx⁵ in its Nextcloud login with this server filled in: `davx5://<host>` is read as
/// `https://<host>` and the `loginFlow` extra picks the Nextcloud login (davx5-ose `LoginActivity.kt`
/// `loginInfoFromIntent`, `StandardLoginTypesProvider.kt`). The `intent:` form is Chrome's (and needs a
/// tap: developer.chrome.com/docs/android/intents); without the app Chrome opens `fallback` instead.
pub fn davx5_intent(public_url: &str, fallback: &str) -> String {
    let host = url::Url::parse(public_url).ok().and_then(|u| u.host_str().map(|h| match u.port() {
        Some(p) => format!("{h}:{p}"),
        None => h.to_string(),
    })).unwrap_or_default();
    let fb: String = url::form_urlencoded::byte_serialize(fallback.as_bytes()).collect();
    format!("intent://{host}#Intent;scheme=davx5;package=at.bitfire.davdroid;S.loginFlow=1;S.browser_fallback_url={fb};end")
}

/// `text` as an SVG QR code (error correction M, 2 modules of quiet zone; the page frames it white).
pub fn qr_svg(text: &str) -> String {
    use qrcodegen::{QrCode, QrCodeEcc};
    let Ok(q) = QrCode::encode_text(text, QrCodeEcc::Medium) else { return String::new() };
    let (n, b) = (q.size(), 2);
    let mut d = String::new();
    for y in 0..n {
        let mut x = 0;
        while x < n {
            if q.get_module(x, y) {
                let start = x;
                while x < n && q.get_module(x, y) {
                    x += 1;
                }
                d.push_str(&format!("M{} {}h{}v1h-{}z", start + b, y + b, x - start, x - start));
            } else {
                x += 1;
            }
        }
    }
    let w = n + 2 * b;
    format!(r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {w}" shape-rendering="crispEdges" aria-hidden="true"><rect width="{w}" height="{w}" fill="#fff"/><path fill="#000" d="{d}"/></svg>"##)
}

/// The symbol of a device row, guessed from its name (a token carries nothing else).
pub fn device_icon(name: &str) -> &'static str {
    let n = name.to_lowercase();
    if ["ipad", "tablet", "tab "].iter().any(|k| n.contains(k)) {
        "tablet"
    } else if ["davx", "android", "iphone", "phone", "handy", "pixel", "galaxy", "smartphone", "móvil", "portable", "téléphone"].iter().any(|k| n.contains(k)) {
        "smartphone"
    } else {
        "laptop"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: i64) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::from_timestamp(1_800_000_000 + s, 0).unwrap()
    }

    fn grant() -> Grant {
        Grant { server: "https://dav.example.org".into(), login_name: "alice".into(), app_password: "x_secret".into() }
    }

    #[test]
    fn a_flow_answers_once_and_only_with_its_token() {
        let f = Flows::default();
        let (id, tok) = f.start("DAVx5/4.5 (at.bitfire.davdroid)", t(0));
        assert_eq!(f.pending(&id, t(1)).as_deref(), Some("DAVx5/4.5 (at.bitfire.davdroid)"));
        assert!(f.poll(&id, &tok, t(1)).is_none(), "nothing before the grant");
        assert!(f.grant(&id, grant(), t(2)));
        assert!(!f.grant(&id, grant(), t(3)), "a flow is confirmed once");
        assert!(f.pending(&id, t(3)).is_none(), "a confirmed flow asks no more");
        assert!(f.poll(&id, "wrong", t(3)).is_none(), "a wrong token gets nothing");
        assert_eq!(f.poll(&id, &tok, t(4)).map(|g| g.app_password).as_deref(), Some("x_secret"));
        assert!(f.poll(&id, &tok, t(5)).is_none(), "the grant leaves with its first answer");
    }

    #[test]
    fn a_flow_expires_and_the_map_stays_bounded() {
        let f = Flows::default();
        let (id, tok) = f.start("a", t(0));
        assert!(!f.grant(&id, grant(), t(FLOW_SECS)), "expired");
        assert!(f.poll(&id, &tok, t(FLOW_SECS)).is_none());
        for i in 0..(FLOW_MAX + 50) {
            f.start("b", t(i as i64));
        }
        assert!(f.by_id.lock().unwrap().len() <= FLOW_MAX, "the oldest flows make room");
    }

    #[test]
    fn a_pair_is_its_users_only() {
        let p = Pairs::default();
        let id = p.create("alice", t(0));
        p.open(&id, "mallory");
        assert_eq!(p.status(&id, "alice", t(1)), Some(PairStatus { opened: false, device: None }), "another user cannot open it");
        assert!(p.status(&id, "mallory", t(1)).is_none(), "nor read it");
        p.open(&id, "alice");
        p.connected("bob", "Bob's phone");
        p.connected("alice", "Pixel 9");
        assert_eq!(p.status(&id, "alice", t(2)), Some(PairStatus { opened: true, device: Some("Pixel 9".into()) }));
        assert!(p.status(&id, "alice", t(PAIR_SECS)).is_none(), "expired");
        for i in 0..20 {
            p.create("carol", t(i));
        }
        assert!(p.by_id.lock().unwrap().values().filter(|x| x.user == "carol").count() <= PAIR_MAX_PER_USER);
    }

    #[test]
    fn the_intent_names_host_package_and_fallback() {
        let i = davx5_intent("https://plan.example.org", "https://plan.example.org/settings#add-android-install");
        assert_eq!(i, "intent://plan.example.org#Intent;scheme=davx5;package=at.bitfire.davdroid;S.loginFlow=1;S.browser_fallback_url=https%3A%2F%2Fplan.example.org%2Fsettings%23add-android-install;end");
        assert!(davx5_intent("http://localhost:4001", "x").starts_with("intent://localhost:4001#"));
    }

    #[test]
    fn a_qr_code_is_a_square_svg() {
        let s = qr_svg("https://plan.example.org/settings?pair=abc#add-android-install");
        assert!(s.starts_with("<svg") && s.contains("viewBox=\"0 0 ") && s.contains("<path fill=\"#000\" d=\"M"), "{s}");
    }

    #[test]
    fn device_icons_follow_the_name() {
        assert_eq!(device_icon("Pixel 9 (DAVx⁵)"), "smartphone");
        assert_eq!(device_icon("iPad"), "tablet");
        assert_eq!(device_icon("MacBook (Thunderbird)"), "laptop");
    }
}
