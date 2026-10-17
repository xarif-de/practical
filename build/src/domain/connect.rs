//! Connecting a device: the server side of Nextcloud Login Flow v2 (which DAVx⁵ speaks), the profile links
//! and configuration profiles of iPhones and iPads (no app: iOS sets up its own accounts, [`apple_profile`]),
//! the pairing through which the "add a device" wizard learns that a device got connected, and the QR codes
//! and store links the wizard shows.
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
//! All three stores live in memory only: none is worth keeping across a restart (the user starts over).

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
    manual: bool,
}

#[derive(Default)]
pub struct Pairs {
    by_id: Mutex<HashMap<String, Pair>>,
}

#[derive(serde::Serialize, Debug, PartialEq)]
pub struct PairStatus {
    pub device: Option<String>,
    /// it fetched the manual setup's password, not a profile: an iPhone that goes on by hand
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub manual: bool,
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
        m.insert(id.clone(), Pair { user: user.to_string(), at: now, device: None, manual: false });
        id
    }

    /// A device of `user` was just connected (`manual`: by hand): every open pairing of theirs learns its name.
    pub fn connected(&self, user: &str, device: &str, manual: bool) {
        for p in self.by_id.lock().unwrap().values_mut().filter(|p| p.user == user) {
            p.device = Some(device.to_string());
            p.manual = manual;
        }
    }

    pub fn status(&self, id: &str, user: &str, now: chrono::DateTime<chrono::Utc>) -> Option<PairStatus> {
        let m = self.by_id.lock().unwrap();
        m.get(id).filter(|p| p.user == user && now.signed_duration_since(p.at).num_seconds() < PAIR_SECS).map(|p| PairStatus { device: p.device.clone(), manual: p.manual })
    }
}

/// The life of a profile link, as long as a Login Flow v2 flow.
const LINK_SECS: i64 = FLOW_SECS;
/// After its download a link answers the same profile for this long more, to the same device only: a second
/// download (Safari may fetch twice, a user who tapped "Ignore" taps again) must not meet a used link. iOS
/// drops a downloaded profile that is not installed within 8 minutes (support.apple.com/102400, 2026-04-14).
const LINK_AGAIN_SECS: i64 = 8 * 60;
/// A link whose manual guide the phone opened (`/manual`, also after a refused profile), and a password it fetched,
/// live this long: the setup by hand (seven pictures, CalDAV, CardDAV, an account per group, app switches)
/// outlasts both windows above, and a guide that expired half-way left a copy button doing nothing (reviews
/// 2026-10-16, 2026-10-17).
const MANUAL_SECS: i64 = 30 * 60;
/// Profile links per user at most (the wizard renews its link before it expires).
const LINK_MAX_PER_USER: usize = 4;

/// One profile link: made in the wizard for its user, opened once by the device that installs the profile.
struct Link {
    user: String,
    at: chrono::DateTime<chrono::Utc>,
    state: LinkState,
    /// when its manual guide was opened ([`Links::open_manual`]): the link lives [`MANUAL_SECS`] from then
    manual: Option<chrono::DateTime<chrono::Utc>>,
    /// a downloaded profile was traded once for the manual password ([`Links::open`])
    switched: bool,
}

/// `device`: who downloads (client address and User-Agent, [`Links::open`]).
enum LinkState {
    Fresh,
    /// its profile is being made for `device`
    Taken(String),
    /// downloaded at the given time by `device`: the profile, answered again for [`LINK_AGAIN_SECS`]
    Done(String, chrono::DateTime<chrono::Utc>, String),
}

impl Link {
    fn live(&self, now: chrono::DateTime<chrono::Utc>) -> bool {
        let within = |at: chrono::DateTime<chrono::Utc>, secs| now.signed_duration_since(at).num_seconds() < secs;
        let guide = self.manual.is_some_and(|m| within(m, MANUAL_SECS));
        match &self.state {
            LinkState::Done(_, at, d) if d.starts_with("password ") => within(*at, MANUAL_SECS),
            // a refused profile: the guide opened on the phone keeps the link for its password
            LinkState::Done(_, at, _) => within(*at, LINK_AGAIN_SECS) || guide,
            _ => guide || (self.manual.is_none() && within(self.at, LINK_SECS)),
        }
    }
}

/// What opening a link finds: a profile to make for its user, the one made before, or one being made for this
/// device (wait for [`Links::changed`]).
pub enum Opened {
    Make(String),
    Again(String),
    Busy,
}

#[derive(Default)]
pub struct Links {
    by_id: Mutex<HashMap<String, Link>>,
    /// a profile was made (or could not be): a download that found its link busy looks again
    pub changed: tokio::sync::Notify,
}

impl Links {
    /// A new link of `user`: 32 random characters (190 bits), half of a token's, so that its QR code stays
    /// coarse enough for a camera at a glance.
    pub fn create(&self, user: &str, now: chrono::DateTime<chrono::Utc>) -> String {
        let id = super::tokens::generate_secret()[..32].to_string();
        let mut m = self.by_id.lock().unwrap();
        m.retain(|_, l| l.live(now));
        // full: the oldest unopened link goes first, a downloaded one next, one being made never
        while m.values().filter(|l| l.user == user).count() >= LINK_MAX_PER_USER {
            let Some(oldest) = m.iter().filter(|(_, l)| l.user == user && !matches!(l.state, LinkState::Taken(_))).min_by_key(|(_, l)| (matches!(l.state, LinkState::Done(..)) || l.manual.is_some(), l.manual.unwrap_or(l.at))).map(|(k, _)| k.clone()) else { break };
            m.remove(&oldest);
        }
        m.insert(id.clone(), Link { user: user.to_string(), at: now, state: LinkState::Fresh, manual: None, switched: false });
        id
    }

    /// The download by `device`: a fresh link is taken for it; a link taken or downloaded by it answers
    /// `Busy` / its profile again (for [`LINK_AGAIN_SECS`]); `None` for an unknown or expired link and for
    /// any other device — one link, one device: a photo of the code taken after the download gets nothing.
    pub fn open(&self, id: &str, device: &str, now: chrono::DateTime<chrono::Utc>) -> Option<Opened> {
        let mut m = self.by_id.lock().unwrap();
        let l = m.get_mut(id).filter(|l| l.live(now))?;
        match &l.state {
            LinkState::Fresh => {
                l.state = LinkState::Taken(device.to_string());
                Some(Opened::Make(l.user.clone()))
            }
            LinkState::Taken(d) => (d == device).then_some(Opened::Busy),
            // the profile again only within its own window: a guide opened on the phone keeps the link for the password
            LinkState::Done(p, at, d) if d == device && (d.starts_with("password ") || now.signed_duration_since(*at).num_seconds() < LINK_AGAIN_SECS) => Some(Opened::Again(p.clone())),
            // the same phone after its profile, now asking for the manual password: an iPhone whose company
            // policy refuses profiles ("Unable to Install Profile") goes on by hand — once; the profile's token is
            // never used and goes with the next sweep (tokens::sweep)
            LinkState::Done(_, _, d) if !l.switched && d.strip_prefix("profile ") == device.strip_prefix("password ") && d.starts_with("profile ") => {
                l.switched = true;
                l.state = LinkState::Taken(device.to_string());
                Some(Opened::Make(l.user.clone()))
            }
            LinkState::Done(..) => None,
        }
    }

    /// The manual guide's page was opened on the phone: the link lives [`MANUAL_SECS`] from now (a fresh one, or one
    /// whose profile the phone refused) and is the last one a renewing wizard pushes out. The link's user, while it lives.
    pub fn open_manual(&self, id: &str, now: chrono::DateTime<chrono::Utc>) -> Option<String> {
        let mut m = self.by_id.lock().unwrap();
        let l = m.get_mut(id).filter(|l| l.live(now))?;
        let open = match &l.state {
            LinkState::Fresh => true,
            LinkState::Done(_, _, d) => !l.switched && d.starts_with("profile "),
            LinkState::Taken(_) => false,
        };
        if open && l.manual.is_none() {
            l.manual = Some(now);
        }
        Some(l.user.clone())
    }

    /// The user of a link that can still be downloaded: its page offers the download.
    pub fn user(&self, id: &str, now: chrono::DateTime<chrono::Utc>) -> Option<String> {
        self.by_id.lock().unwrap().get(id).filter(|l| l.live(now)).map(|l| l.user.clone())
    }

    /// The profile of a taken link was made (`Some`), or could not be (`None`: the link is fresh again).
    pub fn made(&self, id: &str, profile: Option<&str>, now: chrono::DateTime<chrono::Utc>) {
        let mut m = self.by_id.lock().unwrap();
        if let Some(l) = m.get_mut(id)
            && let LinkState::Taken(d) = &l.state
        {
            match profile {
                Some(p) => l.state = LinkState::Done(p.to_string(), now, d.clone()),
                // a switch to the password that failed: gone (fresh, any device could take it while the guide lives)
                None if l.switched => drop(m.remove(id)),
                None => l.state = LinkState::Fresh,
            }
        }
        drop(m);
        self.changed.notify_waiters();
    }
}

/// The device's name from the downloading browser's User-Agent. An iPad's Safari asks for desktop sites and
/// says "Macintosh" (iPadOS 13+); a Mac rarely opens a code that a phone camera scans.
pub fn apple_device_name(ua: &str) -> &'static str {
    if ua.contains("iPad") || ua.contains("Macintosh") { "iPad" } else { "iPhone" }
}

/// Whether the browser of this User-Agent installs a configuration profile it downloads: Safari (Apple
/// names it, support.apple.com/guide/profile-manager/pmdbd71ebc9) and Chrome (since ~v95, brave-ios #4358) do;
/// Firefox (firefox-ios #25476) and DuckDuckGo (duckduckgo/iOS #830) only save it; Edge, Opera and the web views
/// inside apps (Google, Instagram, Facebook, Line, WeChat) are not known to install it.
pub fn installs_profiles(ua: &str) -> bool {
    !["FxiOS", "DuckDuckGo", "EdgiOS", "OPiOS", "OPT/", "GSA/", "Instagram", "FBAN", "FBAV", "Line/", "MicroMessenger"].iter().any(|k| ua.contains(k))
}

/// An account of a profile: CalDAV or CardDAV, for the user or one of their groups.
pub struct AppleAccount {
    pub carddav: bool,
    /// the principal the account opens; a group's is reached by logging in as `user$group` (RustiCal
    /// `auth/middleware.rs`, v0.16.6)
    pub principal: String,
    pub description: String,
}

/// What a profile holds besides its accounts.
pub struct AppleProfile<'a> {
    pub dav_url: &'a str,
    pub user: &'a str,
    pub password: &'a str,
    /// this server's host: the profile's organization and part of its identifier
    pub host: &'a str,
    pub name: &'a str,
    pub description: &'a str,
}

/// An Apple configuration profile that sets up the user's accounts (developer.apple.com/documentation/
/// devicemanagement/caldav and …/carddav). CalDAV goes to `/caldav-compat`: Apple's Calendar follows one
/// calendar-home-set only, and that tree names the principal itself as its home (RustiCal `app.rs`, issue
/// #180); Apple's Calendar sees one principal per account, so each group gets accounts of its own — both as
/// in RustiCal's own profile (`apple_configuration/template.xml`, v0.16.6). `TargetDeviceType` 1 (iPhone/iPad)
/// spares the user with a paired Apple Watch the question which device the profile is for
/// (developer.apple.com/documentation/devicemanagement/toplevel). The identifier is the same for
/// every profile of this user on this server: installing one again replaces the last one on that device
/// instead of adding a second set of accounts.
pub fn apple_profile(p: &AppleProfile, accounts: &[AppleAccount]) -> String {
    use crate::domain::remote::xml::esc;
    use std::fmt::Write;
    let url = url::Url::parse(p.dav_url).unwrap_or_else(|_| url::Url::parse("https://invalid/").unwrap());
    let (host, ssl) = (url.host_str().unwrap_or_default(), if url.scheme() == "https" { "true" } else { "false" });
    // reverse-DNS characters only: "localhost:4001" and "a@b" are no identifier parts
    let ident: String = format!("{}.practical.{}", p.host, p.user).chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '-' }).collect();
    let uuid = || uuid::Uuid::new_v4().to_string().to_uppercase();
    let mut items = String::new();
    for (i, a) in accounts.iter().enumerate() {
        let (k, kind, tree) = if a.carddav { ("CardDAV", "carddav", "carddav") } else { ("CalDAV", "caldav", "caldav-compat") };
        let mut principal = url.clone();
        if let Ok(mut s) = principal.path_segments_mut() {
            s.pop_if_empty().extend([tree, "principal", a.principal.as_str()]);
        }
        let login = if a.principal == p.user { p.user.to_string() } else { format!("{}${}", p.user, a.principal) };
        let port = url.port().map(|n| format!("<key>{k}Port</key><integer>{n}</integer>")).unwrap_or_default();
        let _ = write!(
            items,
            "\n<dict><key>PayloadType</key><string>com.apple.{kind}.account</string><key>PayloadIdentifier</key><string>{ident}.{kind}.{i}</string><key>PayloadUUID</key><string>{}</string><key>PayloadVersion</key><integer>1</integer><key>PayloadDisplayName</key><string>{d}</string><key>{k}AccountDescription</key><string>{d}</string><key>{k}HostName</key><string>{}</string>{port}<key>{k}UseSSL</key><{ssl}/><key>{k}PrincipalURL</key><string>{}</string><key>{k}Username</key><string>{}</string><key>{k}Password</key><string>{}</string></dict>",
            uuid(),
            esc(host),
            esc(principal.as_str()),
            esc(&login),
            esc(p.password),
            d = esc(&a.description),
        );
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>PayloadType</key><string>Configuration</string><key>TargetDeviceType</key><integer>1</integer><key>PayloadIdentifier</key><string>{ident}</string><key>PayloadUUID</key><string>{}</string><key>PayloadVersion</key><integer>1</integer><key>PayloadDisplayName</key><string>{}</string><key>PayloadDescription</key><string>{}</string><key>PayloadOrganization</key><string>{}</string><key>PayloadRemovalDisallowed</key><false/><key>PayloadContent</key><array>{items}\n</array></dict></plist>\n",
        uuid(),
        esc(p.name),
        esc(p.description),
        esc(p.host),
    )
}

/// The in-process state of all three, held in [`crate::state::AppState`], and the wizard's QR codes.
#[derive(Default)]
pub struct Connect {
    pub flows: Flows,
    pub pairs: Pairs,
    pub links: Links,
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
    if n.contains("uploader") {
        "briefcase" // a PractiCalUploader (domain::uploader): fills a work calendar
    } else if ["ipad", "tablet", "tab "].iter().any(|k| n.contains(k)) {
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
        assert_eq!(p.status(&id, "alice", t(1)), Some(PairStatus { device: None, manual: false }));
        assert!(p.status(&id, "mallory", t(1)).is_none(), "another user cannot read it");
        p.connected("bob", "Bob's phone", false);
        p.connected("alice", "Pixel 9", false);
        assert_eq!(p.status(&id, "alice", t(2)), Some(PairStatus { device: Some("Pixel 9".into()), manual: false }));
        assert!(p.status(&id, "alice", t(PAIR_SECS)).is_none(), "expired");
        for i in 0..20 {
            p.create("carol", t(i));
        }
        assert!(p.by_id.lock().unwrap().values().filter(|x| x.user == "carol").count() <= PAIR_MAX_PER_USER);
    }

    #[test]
    fn a_profile_link_serves_one_device_briefly() {
        let (l, phone, other) = (Links::default(), "1.2.3.4 iPhone", "5.6.7.8 iPhone");
        let id = l.create("alice", t(0));
        assert_eq!(l.user(&id, t(1)).as_deref(), Some("alice"));
        assert!(matches!(l.open(&id, phone, t(1)), Some(Opened::Make(u)) if u == "alice"));
        assert!(matches!(l.open(&id, phone, t(1)), Some(Opened::Busy)), "being made: the same device waits");
        assert!(l.open(&id, other, t(1)).is_none(), "another device gets nothing");
        l.made(&id, None, t(2));
        assert!(matches!(l.open(&id, other, t(2)), Some(Opened::Make(..))), "not made: fresh again, for anyone");
        l.made(&id, Some("<plist/>"), t(3));
        assert!(matches!(l.open(&id, other, t(3 + LINK_AGAIN_SECS - 1)), Some(Opened::Again(p)) if p == "<plist/>"), "a second download: the same profile");
        assert!(l.open(&id, phone, t(4)).is_none(), "but to that device only");
        assert!(l.open(&id, other, t(3 + LINK_AGAIN_SECS)).is_none(), "then never again");
        assert!(l.user(&l.create("alice", t(0)), t(LINK_SECS)).is_none(), "expired unopened");
        // the manual guide: opened late, it lives MANUAL_SECS from then; its password as long after the fetch
        let m = l.create("alice", t(0));
        assert_eq!(l.open_manual(&m, t(LINK_SECS - 1)).as_deref(), Some("alice"));
        assert!(l.user(&m, t(LINK_SECS - 1 + MANUAL_SECS - 1)).is_some() && l.user(&m, t(LINK_SECS - 1 + MANUAL_SECS)).is_none());
        let pw = "password 1.2.3.4 Safari";
        let m = l.create("alice", t(0));
        assert!(matches!(l.open(&m, pw, t(1)), Some(Opened::Make(..))));
        l.made(&m, Some("secret"), t(2));
        assert!(matches!(l.open(&m, pw, t(2 + MANUAL_SECS - 1)), Some(Opened::Again(p)) if p == "secret"));
        // a profile, then the same phone by hand: one switch, never a second, never another device
        let s = l.create("alice", t(0));
        assert!(matches!(l.open(&s, "profile 1.2.3.4 Safari", t(1)), Some(Opened::Make(..))));
        l.made(&s, Some("<plist/>"), t(2));
        assert!(l.open(&s, "password 9.9.9.9 Safari", t(3)).is_none(), "another device");
        assert!(matches!(l.open(&s, pw, t(3)), Some(Opened::Make(..))), "the same phone, by hand");
        l.made(&s, Some("pw"), t(4));
        assert!(l.open(&s, "profile 1.2.3.4 Safari", t(5)).is_none(), "no way back to a second profile");
        // the guide opened after the profile: its password still comes when the walk through Settings outlasts
        // the profile's window (review 2026-10-17)
        let late = l.create("alice", t(0));
        l.open(&late, "profile 1.2.3.4 Safari", t(1));
        l.made(&late, Some("<plist/>"), t(2));
        assert_eq!(l.open_manual(&late, t(3)).as_deref(), Some("alice"));
        assert!(l.open(&late, "profile 1.2.3.4 Safari", t(2 + LINK_AGAIN_SECS)).is_none(), "the profile itself not again after its window");
        assert!(matches!(l.open(&late, pw, t(2 + LINK_AGAIN_SECS + 60)), Some(Opened::Make(..))), "by hand, after the profile's window");
        l.made(&late, None, t(2 + LINK_AGAIN_SECS + 61));
        assert!(l.user(&late, t(2 + LINK_AGAIN_SECS + 62)).is_none(), "a failed switch is not fresh for any device");
        let gone = l.create("alice", t(0));
        l.open(&gone, "profile 1.2.3.4 Safari", t(1));
        l.made(&gone, Some("<plist/>"), t(2));
        assert!(l.open_manual(&gone, t(2 + LINK_AGAIN_SECS)).is_none(), "the guide opened too late finds nothing");
        let busy = l.create("bob", t(19));
        l.open(&busy, phone, t(19));
        for i in 0..10 {
            l.create("bob", t(20 + i));
        }
        let m = l.by_id.lock().unwrap();
        assert!(m.values().filter(|x| x.user == "bob").count() <= LINK_MAX_PER_USER);
        assert!(m.contains_key(&busy), "a link being made is never evicted");
    }

    #[test]
    fn a_profile_sets_up_the_users_and_their_groups_accounts() {
        let accounts = [
            AppleAccount { carddav: false, principal: "alice".into(), description: "PractiCal".into() },
            AppleAccount { carddav: true, principal: "alice".into(), description: "PractiCal".into() },
            AppleAccount { carddav: false, principal: "group.k-s".into(), description: "PractiCal · K&S".into() },
        ];
        let p = AppleProfile { dav_url: "https://dav.example.org", user: "alice", password: "abcd_x<y", host: "plan.example.org", name: "PractiCal", description: "für alice" };
        let x = apple_profile(&p, &accounts);
        assert!(x.starts_with("<?xml") && x.contains("<key>TargetDeviceType</key><integer>1</integer>"), "{x}");
        assert!(x.contains("<key>PayloadIdentifier</key><string>plan.example.org.practical.alice</string>"));
        assert!(x.contains("<key>CalDAVHostName</key><string>dav.example.org</string><key>CalDAVUseSSL</key><true/><key>CalDAVPrincipalURL</key><string>https://dav.example.org/caldav-compat/principal/alice</string><key>CalDAVUsername</key><string>alice</string><key>CalDAVPassword</key><string>abcd_x&lt;y</string>"), "{x}");
        assert!(x.contains("<key>CardDAVPrincipalURL</key><string>https://dav.example.org/carddav/principal/alice</string><key>CardDAVUsername</key><string>alice</string>"));
        assert!(x.contains("<string>PractiCal · K&amp;S</string>") && x.contains("<key>CalDAVUsername</key><string>alice$group.k-s</string>"), "a group: its own account, logged in as user$group");
        assert_eq!(x.matches("<key>PayloadUUID</key>").count(), 4, "one per account and the profile");
        let local = apple_profile(&AppleProfile { dav_url: "http://localhost:4000/", ..p }, &accounts[..1]);
        assert!(local.contains("<key>CalDAVHostName</key><string>localhost</string><key>CalDAVPort</key><integer>4000</integer><key>CalDAVUseSSL</key><false/><key>CalDAVPrincipalURL</key><string>http://localhost:4000/caldav-compat/principal/alice</string>"), "{local}");
    }

    #[test]
    fn apple_devices_and_browsers() {
        let safari = "Mozilla/5.0 (iPhone; CPU iPhone OS 26_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Mobile/15E148 Safari/604.1";
        assert_eq!(apple_device_name(safari), "iPhone");
        assert_eq!(apple_device_name("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15"), "iPad");
        assert!(installs_profiles(safari));
        assert!(installs_profiles("Mozilla/5.0 (iPhone; CPU iPhone OS 26_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) CriOS/140.0 Mobile/15E148 Safari/604.1"), "Chrome");
        assert!(!installs_profiles("Mozilla/5.0 (iPhone; CPU iPhone OS 26_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) FxiOS/140.0 Mobile/15E148 Safari/605.1.15"), "Firefox");
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
        assert_eq!(device_icon("PractiCalUploader"), "briefcase");
    }
}
