//! PractiCalUploader: a private companion tool that mirrors calendars PractiCal cannot reach itself (an Outlook
//! store on a work laptop) into read-only calendars here. Only with `PRACTICAL__UPLOADER=true`.
//!
//! - A **device** ([`setup`]) is a row of `practical_uploader_devices`: who set it up (`principal`, the Basic user
//!   of its uploads), who owns what it uploads (`owner`: that user or one of their groups, fixed at setup) and the
//!   hash of its own secret. That secret is no RustiCal app token: it opens `/api/uploader` and nothing else
//!   ([`authenticate`]; CalDAV/CardDAV and the rest of the API do not know it). No calendar is made at setup.
//! - [`upload`]: `PUT /api/uploader` — the device's COMPLETE selection, one snapshot per calendar. Each entry is
//!   found by the uploader's opaque `key` among the owner's calendars (`practical_readonly_calendars.source_key`):
//!   created when new, taken over when its device is gone, restored when in the trash, refused (409) when another
//!   live device manages it. A calendar of this device that is no longer in the list goes to the trash
//!   (deselected). Everything is checked before the first write; one change in the history (source "uploader").
//! - A managed calendar ([`managed`]) cannot be deleted in PractiCal; [`delete_device`] lets the user pick which
//!   of its calendars go to the trash, the rest stay read-only without a device (and can then be deleted).
//! - Name and colour belong to PractiCal: the uploader's `name` only names a calendar when it is created.

use crate::domain::collections::{self, NewCalendar};
use crate::domain::history::{self, Actor};
use crate::domain::history_text::Store;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use chrono::{DateTime, Utc};
use rustical_ical::CalendarObject;
use rustical_store::auth::{AuthenticationProvider, Principal};
use rustical_store::{CalendarReadStore, CalendarWriteStore};
use sqlx::Row;
use std::collections::{HashMap, HashSet};

/// `practical_readonly_calendars.source` of an uploader's calendar.
pub const SOURCE: &str = "uploader";
/// Events of one calendar at most (a work calendar of 1.5 years holds a few hundred, measured 2026-10-17: 235).
pub const MAX_EVENTS: usize = 20_000;
/// Calendars of one upload at most (one Outlook profile listed 30 calendars, 2026-10-17).
pub const MAX_CALENDARS: usize = 64;
/// Length of a calendar key at most (the uploader sends account address + Exchange folder id).
pub const MAX_KEY: usize = 200;
/// The body limit of the upload route.
pub const MAX_BODY: usize = 32 * 1024 * 1024;

#[derive(serde::Deserialize)]
pub struct Window {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

#[derive(serde::Deserialize)]
pub struct UploadEvent {
    pub uid: String,
    pub ics: String,
}

#[derive(serde::Deserialize)]
pub struct UploadCalendar {
    pub key: String,
    pub name: String,
    pub window: Window,
    pub events: Vec<UploadEvent>,
}

#[derive(serde::Deserialize)]
pub struct Upload {
    pub calendars: Vec<UploadCalendar>,
}

#[derive(serde::Serialize, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub created: usize,
    pub updated: usize,
    pub deleted: usize,
    pub unchanged: usize,
}

#[derive(serde::Serialize, Debug)]
pub struct CalendarResult {
    pub key: String,
    pub calendar: String,
    #[serde(flatten)]
    pub counts: Counts,
}

#[derive(serde::Serialize, Debug)]
pub struct Uploaded {
    pub calendars: Vec<CalendarResult>,
    pub trashed: Vec<String>,
}

/// A row of `practical_uploader_devices`.
#[derive(Debug, Clone)]
pub struct Device {
    pub id: String,
    pub principal: String,
    pub owner: String,
    pub name: String,
}

/// A device of a row with the columns `id, principal, owner, name`.
fn device_of(r: sqlx::sqlite::SqliteRow) -> Device {
    Device { id: r.get("id"), principal: r.get("principal"), owner: r.get("owner"), name: r.get("name") }
}

/// Length of the device-id prefix in front of a token's secret (`<prefix>_<secret>`, as an app token shows it):
/// [`authenticate`] verifies only the hashes of the user's devices with that prefix (its SQL has the 4 written out).
const PREFIX: usize = 4;

/// What the device page shows once after [`setup`].
#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct Made {
    pub url: String,
    pub user: String,
    pub token: String,
    /// the device's id: the wizard's wait asks for it ([`connected`])
    pub device: String,
    pub name: String,
}

impl Made {
    /// The uploader's `[connection]` block (its README: `practicaluploader set connection`), ready to paste.
    pub fn toml(&self) -> String {
        format!("[connection]\nurl = \"{}\"\nuser = \"{}\"\ntoken = \"{}\"\n", self.url, self.user, self.token)
    }
}

/// A new uploader device of `user`, uploading into calendars of `owner` (the user or one of their groups).
pub async fn setup(state: &AppState, user: &Principal, name: &str, owner: &str) -> AppResult<Made> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::bad("err-name-required"));
    }
    if name.chars().count() > crate::domain::connect::NAME_MAX {
        return Err(AppError::bad("err-name-too-long"));
    }
    collections::ensure_owner(user, owner)?;
    let name = crate::domain::tokens::unique_name(state, user, name).await?;
    let id = uuid::Uuid::new_v4().to_string();
    let secret = crate::domain::tokens::generate_secret();
    let hash = { let s = secret.clone(); tokio::task::spawn_blocking(move || password_auth::generate_hash(s)).await.map_err(|e| AppError::Internal(e.to_string()))? };
    sqlx::query("INSERT INTO practical_uploader_devices (id, principal, owner, name, secret_hash) VALUES (?, ?, ?, ?, ?)")
        .bind(&id)
        .bind(&user.id)
        .bind(owner)
        .bind(&name)
        .bind(&hash)
        .execute(&state.db)
        .await?;
    let token = format!("{}_{secret}", &id[..PREFIX]);
    Ok(Made { url: state.config.public_url.clone(), user: user.id.clone(), token, device: id, name })
}

/// The device reached `/api/uploader`: the first time is when it got connected (the wizard's wait), every time
/// is its last use (the device list).
pub async fn seen(state: &AppState, device: &Device) -> AppResult<()> {
    sqlx::query("UPDATE practical_uploader_devices SET connected_at = COALESCE(connected_at, CURRENT_TIMESTAMP), last_seen = CURRENT_TIMESTAMP WHERE id = ?").bind(&device.id).execute(&state.db).await?;
    Ok(())
}

/// Whether `user`'s device `id` has connected yet (None: no such device of theirs) and its name.
pub async fn connected(state: &AppState, user: &Principal, id: &str) -> AppResult<Option<(bool, String)>> {
    let row = sqlx::query("SELECT connected_at, name FROM practical_uploader_devices WHERE id = ? AND principal = ?").bind(id).bind(&user.id).fetch_optional(&state.db).await?;
    Ok(row.map(|r| (r.get::<Option<String>, _>("connected_at").is_some(), r.get("name"))))
}

/// The live devices, by id: those whose user still exists and may own the device's `owner` (a user who left the
/// group: dead). A dead device's calendars count as orphaned, so the group's other members can delete them or
/// their own device takes them over; its row stays for its user to delete (or for the user's return to the group).
/// One principal lookup per device user.
pub async fn live_devices(state: &AppState) -> AppResult<HashMap<String, Device>> {
    let rows: Vec<Device> = sqlx::query("SELECT id, principal, owner, name FROM practical_uploader_devices").fetch_all(&state.db).await?.into_iter().map(device_of).collect();
    let mut users: HashMap<String, Option<Principal>> = HashMap::new();
    let mut live = HashMap::new();
    for d in rows {
        if !users.contains_key(&d.principal) {
            users.insert(d.principal.clone(), state.principals.get_principal(&d.principal).await?);
        }
        if users[&d.principal].as_ref().is_some_and(|p| collections::ensure_owner(p, &d.owner).is_ok()) {
            live.insert(d.id.clone(), d);
        }
    }
    Ok(live)
}

/// Deletes every device that never connected within [`crate::domain::tokens::CONNECT_GRACE`] (set up half-way, its
/// block never pasted), as [`crate::domain::tokens::sweep`] does with app tokens. Returns how many.
pub async fn sweep(state: &AppState) -> AppResult<u64> {
    let cutoff = (chrono::Utc::now() - crate::domain::tokens::CONNECT_GRACE).format("%Y-%m-%d %H:%M:%S").to_string();
    // one statement: a first request that lands meanwhile sets connected_at and keeps its device
    let n = sqlx::query("DELETE FROM practical_uploader_devices WHERE connected_at IS NULL AND created_at < ?").bind(&cutoff).execute(&state.db).await?.rows_affected();
    // a never-connected device uploaded nothing, so no calendar names it; unlink anyway rather than leave a dangling id
    sqlx::query("UPDATE practical_readonly_calendars SET device_id = NULL WHERE device_id IS NOT NULL AND device_id NOT IN (SELECT id FROM practical_uploader_devices)").execute(&state.db).await?;
    Ok(n)
}

/// Drops a device row and unlinks its calendars (they stay read-only, orphaned).
async fn forget_device(state: &AppState, id: &str) -> AppResult<()> {
    sqlx::query("UPDATE practical_readonly_calendars SET device_id = NULL WHERE device_id = ?").bind(id).execute(&state.db).await?;
    sqlx::query("DELETE FROM practical_uploader_devices WHERE id = ?").bind(id).execute(&state.db).await?;
    Ok(())
}

/// The (owner, calendar id) pairs a live device manages: PractiCal refuses to delete them.
pub async fn managed(state: &AppState) -> AppResult<HashSet<(String, String)>> {
    let live = live_devices(state).await?;
    Ok(sqlx::query("SELECT principal, calendar_id, device_id FROM practical_readonly_calendars WHERE device_id IS NOT NULL")
        .fetch_all(&state.db)
        .await?
        .into_iter()
        .filter(|r| live.contains_key(&r.get::<String, _>("device_id")))
        .map(|r| (r.get("principal"), r.get("calendar_id")))
        .collect())
}

/// The device and its user for HTTP Basic `user` + `token` (`<prefix>_<secret>`, the device's own secret); None for
/// any other credentials (an app token, a password, a wrong secret, a deleted user).
pub async fn authenticate(state: &AppState, user: &str, token: &str) -> AppResult<Option<(Principal, Device)>> {
    let Some((prefix, secret)) = token.split_once('_').filter(|(p, _)| p.len() == PREFIX) else { return Ok(None) };
    let rows = sqlx::query("SELECT id, principal, owner, name, secret_hash FROM practical_uploader_devices WHERE principal = ? AND substr(id, 1, 4) = ?").bind(user).bind(prefix).fetch_all(&state.db).await?;
    for r in rows {
        let hash: String = r.get("secret_hash");
        let s = secret.to_string();
        // a hash check costs CPU time (argon2): off the async workers, as the password login does (auth.rs)
        let ok = tokio::task::spawn_blocking(move || password_auth::verify_password(s, &hash).is_ok()).await.map_err(|e| AppError::Internal(e.to_string()))?;
        if ok {
            let Some(p) = state.principals.get_principal(user).await? else { return Ok(None) };
            return Ok(Some((p, device_of(r))));
        }
    }
    Ok(None)
}

/// The object id of a UID: the UID itself when it is a plain path segment, else a digest of it (so one UID
/// always lands on one object).
pub fn object_id(uid: &str) -> String {
    let plain = !uid.is_empty() && uid.len() <= 200 && uid.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'@')) && !uid.starts_with('.');
    if plain {
        uid.to_string()
    } else {
        use sha2::Digest;
        format!("u-{}", sha2::Sha256::digest(uid.as_bytes()).iter().map(|b| format!("{b:02x}")).collect::<String>())
    }
}

/// Checks one event of a snapshot: a parseable VCALENDAR whose one UID is `uid`, an event.
fn check(key: &str, e: &UploadEvent) -> AppResult<CalendarObject> {
    let bad = |why: &str| AppError::bad1("err-uploader-ics", "uid", format!("{} / {}: {why}", key.chars().take(60).collect::<String>(), e.uid.chars().take(80).collect::<String>()));
    if e.uid.is_empty() || e.uid.chars().count() > 255 {
        return Err(bad("uid length"));
    }
    let obj = CalendarObject::import(&e.ics, None).map_err(|x| bad(&x.to_string()))?;
    if obj.get_uid() != e.uid {
        return Err(bad("UID differs"));
    }
    if obj.get_object_type() != rustical_ical::CalendarObjectType::Event {
        return Err(bad("not a VEVENT"));
    }
    Ok(obj)
}

/// Whether a stored object lies wholly before `start` (its last occurrence ends before that day): kept,
/// the source's window no longer reaches it. A series without an end never does.
fn before(obj: &CalendarObject, start: DateTime<Utc>) -> bool {
    obj.get_inner().get_inner().get_last_occurence().map(|l| l.date_ceil()).is_some_and(|d| d <= start.date_naive())
}

/// How an entry of the upload reaches its calendar.
enum Target {
    /// a calendar with this key: (id, in the trash, purged from the store since, managed by this device)
    Known { cal: String, trashed: bool, gone: bool, mine: bool },
    New,
}

/// (in the trash, gone from the store) of a calendar of `owner`.
async fn whereabouts(state: &AppState, owner: &str, cal: &str) -> AppResult<(bool, bool)> {
    match state.calendars.get_calendar(owner, cal, true).await {
        Ok(c) => Ok((c.deleted_at.is_some(), false)),
        Err(rustical_store::Error::NotFound) => Ok((false, true)),
        Err(e) => Err(e.into()),
    }
}

pub async fn upload(state: &AppState, user: &Principal, device: &Device, u: Upload) -> AppResult<Uploaded> {
    collections::ensure_owner(user, &device.owner)?;
    let owner = device.owner.as_str();
    if u.calendars.len() > MAX_CALENDARS {
        return Err(AppError::bad("err-uploader-too-many"));
    }
    // everything checked before the first write: a bad request changes nothing
    let mut keys = HashSet::new();
    let mut wants: Vec<HashMap<String, CalendarObject>> = vec![];
    for c in &u.calendars {
        if c.key.is_empty() || c.key.chars().count() > MAX_KEY || !keys.insert(c.key.as_str()) {
            return Err(AppError::bad1("err-uploader-ics", "uid", format!("{}: calendar key empty, too long or twice", c.key.chars().take(60).collect::<String>())));
        }
        if c.events.len() > MAX_EVENTS {
            return Err(AppError::bad("err-uploader-too-many"));
        }
        if c.window.end <= c.window.start {
            return Err(AppError::bad("err-date-range"));
        }
        let mut want = HashMap::new();
        for e in &c.events {
            let obj = check(&c.key, e)?;
            if want.insert(object_id(&e.uid), obj).is_some() {
                return Err(AppError::bad1("err-uploader-ics", "uid", format!("{} / {}: duplicate", c.key.chars().take(60).collect::<String>(), e.uid.chars().take(80).collect::<String>())));
            }
        }
        wants.push(want);
    }
    let live = live_devices(state).await?;
    let rows: Vec<(String, String, Option<String>)> = sqlx::query("SELECT calendar_id, source_key, device_id FROM practical_readonly_calendars WHERE principal = ? AND source = ? AND source_key IS NOT NULL")
        .bind(owner)
        .bind(SOURCE)
        .fetch_all(&state.db)
        .await?
        .into_iter()
        .map(|r| (r.get("calendar_id"), r.get("source_key"), r.get("device_id")))
        .collect();
    let mut targets = vec![];
    for c in &u.calendars {
        let t = match rows.iter().find(|(_, k, _)| *k == c.key) {
            None => Target::New,
            Some((_, _, Some(d))) if *d != device.id && live.contains_key(d) => {
                return Err(AppError::Conflict(crate::error::Msg { key: "err-uploader-other-device", args: vec![("key", c.key.chars().take(80).collect())] }));
            }
            Some((cal, _, d)) => {
                let (trashed, gone) = whereabouts(state, owner, cal).await?;
                Target::Known { cal: cal.clone(), trashed, gone, mine: d.as_deref() == Some(device.id.as_str()) }
            }
        };
        targets.push(t);
    }

    let mut g = history::begin(state, Actor { src: SOURCE, who: user.id.clone(), label: None, reverts: None }).await;
    let mut out = Uploaded { calendars: vec![], trashed: vec![] };
    for ((c, want), t) in u.calendars.iter().zip(wants).zip(targets) {
        let cal = match t {
            Target::Known { cal, gone: true, .. } => {
                // purged from the store since: the row only holds the key — start over
                sqlx::query("DELETE FROM practical_readonly_calendars WHERE principal = ? AND calendar_id = ?").bind(owner).bind(&cal).execute(&state.db).await?;
                create(state, user, owner, device, c).await?
            }
            Target::Known { cal, trashed, mine, .. } => {
                if trashed {
                    state.calendars.restore_calendar(owner, &cal).await?;
                }
                if !mine {
                    sqlx::query("UPDATE practical_readonly_calendars SET device_id = ? WHERE principal = ? AND calendar_id = ?").bind(&device.id).bind(owner).bind(&cal).execute(&state.db).await?;
                }
                cal
            }
            Target::New => create(state, user, owner, device, c).await?,
        };
        let counts = apply(state, owner, &cal, &c.window, want, &mut g).await?;
        out.calendars.push(CalendarResult { key: c.key.clone(), calendar: cal, counts });
    }
    // calendars of this device that are no longer selected go to the trash (restored when selected again)
    for (cal, key, d) in &rows {
        if d.as_deref() == Some(device.id.as_str()) && !keys.contains(key.as_str()) && whereabouts(state, owner, cal).await? == (false, false) {
            state.calendars.delete_calendar(owner, cal, true).await?;
            out.trashed.push(cal.clone());
        }
    }
    g.commit().await;
    Ok(out)
}

async fn create(state: &AppState, user: &Principal, owner: &str, device: &Device, c: &UploadCalendar) -> AppResult<String> {
    let name: String = c.name.trim().chars().take(120).collect();
    let new = NewCalendar {
        owner: owner.to_string(),
        name: if name.is_empty() { device.name.clone() } else { name },
        color: String::new(),
        description: String::new(),
        components: vec![rustical_ical::CalendarObjectType::Event],
        timezone: state.config.default_timezone.clone(),
    };
    let cal = collections::create_calendar(state, user, new).await?;
    sqlx::query("INSERT INTO practical_readonly_calendars (principal, calendar_id, source, source_key, device_id) VALUES (?, ?, ?, ?, ?)")
        .bind(owner)
        .bind(&cal)
        .bind(SOURCE)
        .bind(&c.key)
        .bind(&device.id)
        .execute(&state.db)
        .await?;
    Ok(cal)
}

/// One calendar's snapshot: put what is new or changed (by bytes), leave identical objects alone, delete what
/// is no longer there unless it ends before the window.
async fn apply(state: &AppState, owner: &str, cal: &str, window: &Window, want: HashMap<String, CalendarObject>, g: &mut history::Guard<'_>) -> AppResult<Counts> {
    let have: HashMap<String, CalendarObject> = state.calendars.get_objects(owner, cal).await?.into_iter().collect();
    let mut n = Counts::default();
    let mut puts = vec![];
    for (id, obj) in want.iter() {
        match have.get(id) {
            Some(old) if old.get_ics() == obj.get_ics() => n.unchanged += 1,
            Some(_) => {
                n.updated += 1;
                puts.push((id.clone(), obj.clone()));
            }
            None => {
                n.created += 1;
                puts.push((id.clone(), obj.clone()));
            }
        }
    }
    let gone: Vec<&String> = have.iter().filter(|(id, o)| !want.contains_key(*id) && !before(o, window.start)).map(|(id, _)| id).collect();
    for (id, _) in &puts {
        g.object(Store::Cal, owner, cal, id);
    }
    for id in &gone {
        g.object(Store::Cal, owner, cal, id);
    }
    for chunk in puts.chunks(crate::domain::transfer::IMPORT_CHUNK) {
        state.calendars.put_objects(owner, cal, chunk.to_vec(), true).await?;
    }
    for id in gone {
        state.calendars.delete_object(owner, cal, id, true).await?;
        n.deleted += 1;
    }
    Ok(n)
}

/// What `GET /api/uploader` tells the device about itself (`practicaluploader status`): its name and owner,
/// and every calendar it manages, by key, with the name it has in PractiCal and whether it is in the trash.
#[derive(serde::Serialize)]
pub struct DeviceInfo {
    pub device: String,
    pub owner: String,
    pub calendars: Vec<ManagedCalendar>,
}

#[derive(serde::Serialize)]
pub struct ManagedCalendar {
    pub key: String,
    pub calendar: String,
    pub name: String,
    pub trashed: bool,
}

/// 403 when `user` may no longer own the device's `owner` (left the group).
pub async fn info(state: &AppState, user: &Principal, device: &Device) -> AppResult<DeviceInfo> {
    collections::ensure_owner(user, &device.owner)?;
    let rows: Vec<(String, String)> = sqlx::query("SELECT calendar_id, source_key FROM practical_readonly_calendars WHERE device_id = ? AND source_key IS NOT NULL")
        .bind(&device.id)
        .fetch_all(&state.db)
        .await?
        .into_iter()
        .map(|r| (r.get("calendar_id"), r.get("source_key")))
        .collect();
    let mut calendars = vec![];
    for (calendar, key) in rows {
        // a calendar purged from the store since keeps only its key row: not reported
        if let Ok(c) = state.calendars.get_calendar(&device.owner, &calendar, true).await {
            calendars.push(ManagedCalendar { key, name: c.meta.displayname.clone().unwrap_or_default(), trashed: c.deleted_at.is_some(), calendar });
        }
    }
    calendars.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(DeviceInfo { device: device.name.clone(), owner: device.owner.clone(), calendars })
}

/// A device of `user` with its calendars (for the device list and its delete page).
pub struct DeviceView {
    pub id: String,
    pub name: String,
    /// (calendar id, display name), not in the trash
    pub calendars: Vec<(String, String)>,
    /// its last request (`seen`)
    pub last_seen: Option<DateTime<Utc>>,
    pub created_at: Option<DateTime<Utc>>,
}

/// `YYYY-MM-DD HH:MM:SS` (SQLite's CURRENT_TIMESTAMP, UTC) as a time.
fn utc(s: Option<String>) -> Option<DateTime<Utc>> {
    s.and_then(|s| chrono::NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S").ok()).map(|n| n.and_utc())
}

/// The user's devices that have connected (one that never did is not listed, like an app token: [`sweep`]). A
/// device for a group the user left lists no calendars: they are not the user's to send to the trash.
pub async fn devices_of(state: &AppState, user: &Principal) -> AppResult<Vec<DeviceView>> {
    let mut out = vec![];
    let rows = sqlx::query("SELECT id, principal, owner, name, last_seen, created_at FROM practical_uploader_devices WHERE principal = ? AND connected_at IS NOT NULL").bind(&user.id).fetch_all(&state.db).await?;
    for r in rows {
        let (last_seen, created_at) = (utc(r.get("last_seen")), utc(r.get("created_at")));
        let d = device_of(r);
        let ids: Vec<String> = if collections::ensure_owner(user, &d.owner).is_ok() { sqlx::query_scalar("SELECT calendar_id FROM practical_readonly_calendars WHERE device_id = ?").bind(&d.id).fetch_all(&state.db).await? } else { vec![] };
        let mut calendars = vec![];
        for id in ids {
            if let Ok(c) = state.calendars.get_calendar(&d.owner, &id, false).await {
                calendars.push((id, c.meta.displayname.unwrap_or_default()));
            }
        }
        calendars.sort_by(|a, b| a.1.cmp(&b.1));
        out.push(DeviceView { id: d.id, name: d.name, calendars, last_seen, created_at });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Deletes a device of `user` (its secret goes with the row), the calendars in `trash` go to the trash, the others
/// stay read-only without a device. NotFound for another user's device.
pub async fn delete_device(state: &AppState, user: &Principal, id: &str, trash: &HashSet<String>) -> AppResult<()> {
    let d = sqlx::query("SELECT id, principal, owner, name FROM practical_uploader_devices WHERE id = ? AND principal = ?").bind(id).bind(&user.id).fetch_optional(&state.db).await?.map(device_of).ok_or(AppError::NotFound)?;
    let ids: Vec<String> = sqlx::query_scalar("SELECT calendar_id FROM practical_readonly_calendars WHERE device_id = ?").bind(&d.id).fetch_all(&state.db).await?;
    // only into the trash of an owner the user still is (a group they left: refused, nothing changes)
    if ids.iter().any(|c| trash.contains(c)) {
        collections::ensure_owner(user, &d.owner)?;
    }
    for cal in ids.iter().filter(|c| trash.contains(*c)) {
        match state.calendars.delete_calendar(&d.owner, cal, true).await {
            Ok(()) | Err(rustical_store::Error::NotFound) => {}
            Err(e) => return Err(e.into()),
        }
    }
    forget_device(state, &d.id).await
}
