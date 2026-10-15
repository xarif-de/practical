//! Connecting a device: the server side of Nextcloud Login Flow v2 (which DAVx⁵ speaks), the pairing
//! through which the "add a device" wizard learns that a device got connected, and the QR codes and store
//! links the wizard shows.
//!
//! Login Flow v2, as DAVx⁵ drives it (davx5-ose `NextcloudLoginFlow.kt`, `NextcloudLogin.kt`, 2026-10-14):
//! 1. `POST <base>/index.php/login/v2` → `{poll:{token,endpoint},login}`; the app opens `login` in a
//!    Custom Tab (the default browser's cookies, so a signed-in user needs no second login).
//! 2. The user confirms on that page ([`Flows::grant`]: who, and the device's name).
//! 3. Back in the app, ONE `POST <endpoint>` with `token=…` → `{server, loginName, appPassword}`; the app token
//!    is made here, when the app takes it ([`Flows::poll`]): a confirmation the app never fetches leaves no
//!    token behind. DAVx⁵
//!    appends `remote.php/dav` to `server` and, finding no principal there, falls back to
//!    `/.well-known/caldav` on that host (`DavResourceFinder.kt` `findInitialConfiguration`); RustiCal
//!    answers that with a redirect (measured on dav.xarif.de: 308 → /caldav, 308 → /carddav).
//!
//! Both stores live in memory only: neither is worth keeping across a restart (the user starts over).

use rustical_store::auth::Principal;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// The life of a flow, from its start and again from its confirmation: RustiCal's own flows last 10 minutes
/// (`nextcloud_login/routes.rs`, v0.16.6).
const FLOW_SECS: i64 = 10 * 60;
/// Flows at most. `POST …/login/v2` needs no login, so the map must not grow with requests: a full map drops
/// its oldest unconfirmed flow first (a flood must not take a confirmed one from under its app).
const FLOW_MAX: usize = 1024;
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
    /// confirmed: by whom, for which device name
    grant: Option<(Principal, String)>,
}

impl Flow {
    fn live(&self, now: chrono::DateTime<chrono::Utc>) -> bool {
        now.signed_duration_since(self.at).num_seconds() < FLOW_SECS
    }

    fn confirmed_by(&self, user: &str) -> Option<(String, String)> {
        self.grant.as_ref().filter(|(u, _)| u.id == user).map(|(_, n)| (self.app.clone(), n.clone()))
    }
}

#[derive(Default)]
pub struct Flows {
    by_id: Mutex<HashMap<String, Flow>>,
}

impl Flows {
    /// Starts a flow for the app `app` (its User-Agent) and returns (id, poll token).
    pub fn start(&self, app: &str, now: chrono::DateTime<chrono::Utc>) -> (String, String) {
        let id = uuid::Uuid::new_v4().to_string();
        let token = super::tokens::generate_secret();
        let mut m = self.by_id.lock().unwrap();
        m.retain(|_, f| f.live(now));
        while m.len() >= FLOW_MAX {
            let Some(oldest) = m.iter().min_by_key(|(_, f)| (f.grant.is_some(), f.at)).map(|(k, _)| k.clone()) else { break };
            m.remove(&oldest);
        }
        let app: String = app.chars().filter(|c| !c.is_control()).take(80).collect();
        m.insert(id.clone(), Flow { poll_token: token.clone(), app, at: now, grant: None });
        (id, token)
    }

    /// The app name of a flow that can still be confirmed (pending, not expired, not confirmed).
    pub fn pending(&self, id: &str, now: chrono::DateTime<chrono::Utc>) -> Option<String> {
        let m = self.by_id.lock().unwrap();
        m.get(id).filter(|f| f.grant.is_none() && f.live(now)).map(|f| f.app.clone())
    }

    /// `user` confirms a flow for the device `name` → (the app's User-Agent, the device name in force), also when
    /// the same user confirmed it already (a second tap shows the same result); `None` when the flow is gone,
    /// expired or another user's. The app has [`FLOW_SECS`] from now to fetch it.
    pub fn grant(&self, id: &str, user: &Principal, name: &str, now: chrono::DateTime<chrono::Utc>) -> Option<(String, String)> {
        let mut m = self.by_id.lock().unwrap();
        let f = m.get_mut(id).filter(|f| f.live(now))?;
        if f.grant.is_none() {
            f.grant = Some((user.clone(), name.to_string()));
            f.at = now;
        }
        f.confirmed_by(&user.id)
    }

    /// A flow `user` confirmed and its app has not fetched yet → (User-Agent, device name): a reload of the
    /// confirmation page shows the result again.
    pub fn confirmed(&self, id: &str, user: &str, now: chrono::DateTime<chrono::Utc>) -> Option<(String, String)> {
        self.by_id.lock().unwrap().get(id).filter(|f| f.live(now)).and_then(|f| f.confirmed_by(user))
    }

    /// The app's poll: the confirmed flow, taken out (a second poll finds nothing), `None` while pending, for a
    /// wrong token and for an unknown or expired flow — the app cannot tell these apart, as with Nextcloud.
    pub fn poll(&self, id: &str, token: &str, now: chrono::DateTime<chrono::Utc>) -> Option<Taken> {
        let mut m = self.by_id.lock().unwrap();
        let f = m.get(id)?;
        if !crate::csrf::same(f.poll_token.as_bytes(), token.as_bytes()) || !f.live(now) || f.grant.is_none() {
            return None;
        }
        let mut flow = m.remove(id)?;
        let (user, name) = flow.grant.take()?;
        Some(Taken { id: id.to_string(), flow, user, name })
    }

    /// Puts a taken flow back: its app token could not be made, the app may ask again.
    pub fn restore(&self, t: Taken) {
        let Taken { id, mut flow, user, name } = t;
        flow.grant = Some((user, name));
        self.by_id.lock().unwrap().insert(id, flow);
    }
}

/// A confirmed flow its app is fetching ([`Flows::poll`]): who confirmed it, for which device name.
pub struct Taken {
    id: String,
    flow: Flow,
    pub user: Principal,
    pub name: String,
}

/// One run of the wizard: a device of the same user that fetches its app password meanwhile names it
/// ([`Pairs::connected`]); the wizard reads that through [`Pairs::status`].
struct Pair {
    user: String,
    at: chrono::DateTime<chrono::Utc>,
    device: Option<String>,
}

#[derive(Default)]
pub struct Pairs {
    by_id: Mutex<HashMap<String, Pair>>,
}

#[derive(serde::Serialize, Debug, PartialEq)]
pub struct PairStatus {
    pub device: Option<String>,
}

impl Pairs {
    pub fn create(&self, user: &str, now: chrono::DateTime<chrono::Utc>) -> String {
        let id = super::tokens::generate_secret();
        let mut m = self.by_id.lock().unwrap();
        m.retain(|_, p| now.signed_duration_since(p.at).num_seconds() < PAIR_SECS);
        while m.values().filter(|p| p.user == user).count() >= PAIR_MAX_PER_USER {
            let Some(oldest) = m.iter().filter(|(_, p)| p.user == user).min_by_key(|(_, p)| p.at).map(|(k, _)| k.clone()) else { break };
            m.remove(&oldest);
        }
        m.insert(id.clone(), Pair { user: user.to_string(), at: now, device: None });
        id
    }

    /// A device of `user` was just connected: every open pairing of theirs learns its name.
    pub fn connected(&self, user: &str, device: &str) {
        for p in self.by_id.lock().unwrap().values_mut().filter(|p| p.user == user) {
            p.device = Some(device.to_string());
        }
    }

    pub fn status(&self, id: &str, user: &str, now: chrono::DateTime<chrono::Utc>) -> Option<PairStatus> {
        let m = self.by_id.lock().unwrap();
        m.get(id).filter(|p| p.user == user && now.signed_duration_since(p.at).num_seconds() < PAIR_SECS).map(|p| PairStatus { device: p.device.clone() })
    }
}

/// The in-process state of both, held in [`crate::state::AppState`], and the wizard's QR codes.
#[derive(Default)]
pub struct Connect {
    pub flows: Flows,
    pub pairs: Pairs,
    codes: OnceLock<Codes>,
}

/// The wizard's QR codes as SVG: the one that opens DAVx⁵ (`/connect/davx5`) and one per store (by
/// [`Store::key`]). They depend on nothing but this server's address: made once per process.
#[derive(serde::Serialize)]
pub struct Codes {
    pub connect: String,
    pub stores: HashMap<&'static str, String>,
}

impl Connect {
    pub fn codes(&self, public_url: &str) -> &Codes {
        self.codes.get_or_init(|| Codes { connect: qr_svg(&format!("{public_url}/connect/davx5")), stores: STORES.iter().map(|s| (s.key, qr_svg(s.url))).collect() })
    }
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
/// tap: developer.chrome.com/docs/android/intents); without the app Chrome may open the fallback, the
/// wizard's install step (on a Samsung phone, 2026-10-14, it opened the Play Store instead).
pub fn davx5_intent(public_url: &str) -> String {
    let fb: String = url::form_urlencoded::byte_serialize(format!("{public_url}/settings?davx5=missing#add-android-install").as_bytes()).collect();
    format!("intent://{}#Intent;scheme=davx5;package=at.bitfire.davdroid;S.loginFlow=1;S.browser_fallback_url={fb};end", host_of(public_url))
}

/// The host (and a non-default port) of `url`, as a browser's address bar shows it: "plan.example.org".
pub fn host_of(url: &str) -> String {
    url::Url::parse(url).ok().and_then(|u| u.host_str().map(|h| match u.port() {
        Some(p) => format!("{h}:{p}"),
        None => h.to_string(),
    })).unwrap_or_default()
}

/// The calendars as DAVx⁵ lists them: the user's calendars, then one birthday calendar per address book
/// (RustiCal's `birthday_calendar.rs`: "`<address book>` birthdays", v0.16.6).
pub fn davx5_calendars(calendars: impl Iterator<Item = String>, addressbooks: &[String]) -> Vec<String> {
    calendars.chain(addressbooks.iter().map(|a| format!("{a} birthdays"))).collect()
}

/// `text` as an SVG QR code (error correction M, 2 modules of quiet zone; the page frames it white).
pub fn qr_svg(text: &str) -> String {
    use qrcodegen::{QrCode, QrCodeEcc};
    let Ok(q) = QrCode::encode_text(text, QrCodeEcc::Medium) else { return String::new() };
    let (n, b) = (q.size(), 2);
    use std::fmt::Write;
    let mut d = String::new();
    for y in 0..n {
        let mut x = 0;
        while x < n {
            if q.get_module(x, y) {
                let start = x;
                while x < n && q.get_module(x, y) {
                    x += 1;
                }
                let _ = write!(d, "M{} {}h{}v1h-{}z", start + b, y + b, x - start, x - start);
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

    fn user(id: &str) -> Principal {
        Principal { id: id.into(), displayname: None, principal_type: Default::default(), password: None, memberships: vec![] }
    }

    #[test]
    fn a_flow_answers_once_and_only_with_its_token() {
        let f = Flows::default();
        let (id, tok) = f.start("DAVx5/4.5 (at.bitfire.davdroid)", t(0));
        assert_eq!(f.pending(&id, t(1)).as_deref(), Some("DAVx5/4.5 (at.bitfire.davdroid)"));
        assert!(f.poll(&id, &tok, t(1)).is_none(), "nothing before the grant");
        assert_eq!(f.grant(&id, &user("alice"), "Pixel 9", t(2)).map(|g| g.1).as_deref(), Some("Pixel 9"));
        assert_eq!(f.grant(&id, &user("alice"), "Other", t(3)).map(|g| g.1).as_deref(), Some("Pixel 9"), "a second tap: the first name");
        assert!(f.grant(&id, &user("bob"), "Bob's", t(3)).is_none(), "another user's");
        assert_eq!(f.confirmed(&id, "alice", t(3)).map(|g| g.1).as_deref(), Some("Pixel 9"), "a reload shows it again");
        assert!(f.confirmed(&id, "bob", t(3)).is_none());
        assert!(f.pending(&id, t(3)).is_none(), "a confirmed flow asks no more");
        assert!(f.poll(&id, "wrong", t(3)).is_none(), "a wrong token gets nothing");
        let taken = f.poll(&id, &tok, t(2 + FLOW_SECS - 1)).expect("the app's time runs from the confirmation");
        assert_eq!((taken.user.id.as_str(), taken.name.as_str()), ("alice", "Pixel 9"));
        assert!(f.poll(&id, &tok, t(5)).is_none(), "taken: a second poll finds nothing");
        f.restore(taken);
        assert!(f.poll(&id, &tok, t(6)).is_some(), "put back when its token could not be made");
    }

    #[test]
    fn a_flow_expires_and_the_map_stays_bounded() {
        let f = Flows::default();
        let (id, tok) = f.start("a", t(0));
        assert!(f.grant(&id, &user("alice"), "x", t(FLOW_SECS)).is_none(), "expired");
        assert!(f.poll(&id, &tok, t(FLOW_SECS)).is_none());
        let (kept, _) = f.start("a", t(1));
        f.grant(&kept, &user("alice"), "x", t(1));
        for _ in 0..(FLOW_MAX + 50) {
            f.start("b", t(2));
        }
        let m = f.by_id.lock().unwrap();
        assert!(m.len() <= FLOW_MAX, "the oldest flows make room");
        assert!(m.contains_key(&kept), "a confirmed flow is not one of them");
    }

    #[test]
    fn a_pair_is_its_users_only() {
        let p = Pairs::default();
        let id = p.create("alice", t(0));
        assert_eq!(p.status(&id, "alice", t(1)), Some(PairStatus { device: None }));
        assert!(p.status(&id, "mallory", t(1)).is_none(), "another user cannot read it");
        p.connected("bob", "Bob's phone");
        p.connected("alice", "Pixel 9");
        assert_eq!(p.status(&id, "alice", t(2)), Some(PairStatus { device: Some("Pixel 9".into()) }));
        assert!(p.status(&id, "alice", t(PAIR_SECS)).is_none(), "expired");
        for i in 0..20 {
            p.create("carol", t(i));
        }
        assert!(p.by_id.lock().unwrap().values().filter(|x| x.user == "carol").count() <= PAIR_MAX_PER_USER);
    }

    #[test]
    fn the_intent_names_host_package_and_fallback() {
        let i = davx5_intent("https://plan.example.org");
        assert_eq!(i, "intent://plan.example.org#Intent;scheme=davx5;package=at.bitfire.davdroid;S.loginFlow=1;S.browser_fallback_url=https%3A%2F%2Fplan.example.org%2Fsettings%3Fdavx5%3Dmissing%23add-android-install;end");
        assert!(davx5_intent("http://localhost:4001").starts_with("intent://localhost:4001#"));
    }

    #[test]
    fn a_qr_code_is_a_square_svg() {
        let s = qr_svg("https://plan.example.org/connect/davx5");
        assert!(s.starts_with("<svg") && s.contains("viewBox=\"0 0 ") && s.contains("<path fill=\"#000\" d=\"M"), "{s}");
    }

    #[test]
    fn device_icons_follow_the_name() {
        assert_eq!(device_icon("Pixel 9 (DAVx⁵)"), "smartphone");
        assert_eq!(device_icon("iPad"), "tablet");
        assert_eq!(device_icon("MacBook (Thunderbird)"), "laptop");
    }
}
