//! PractiCalUploader: a private companion tool (one per device) that mirrors a calendar PractiCal cannot reach
//! itself (an Outlook store on a work laptop) into a read-only calendar here. Only with `PRACTICAL__UPLOADER=true`.
//!
//! - [`setup`]: the device tile's "connect" — an app token named after the device and a new VEVENT calendar,
//!   marked read-only with source [`SOURCE`] (`collections::set_read_only`), so nothing in PractiCal writes into it.
//! - [`upload`]: `PUT /api/uploader/calendars/{id}` — a full snapshot of the source's window. Everything is
//!   checked before the first write; then changed objects are put, identical ones left alone, and objects no
//!   longer in the snapshot deleted unless they lie wholly before the window (history the source no longer
//!   holds). One change in the history (source "uploader").

use crate::domain::collections::{self, NewCalendar};
use crate::domain::history::{self, Actor};
use crate::domain::history_text::Store;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use chrono::{DateTime, Utc};
use rustical_ical::CalendarObject;
use rustical_store::auth::Principal;
use rustical_store::{CalendarReadStore, CalendarWriteStore};

/// `practical_readonly_calendars.source` of an uploader's calendar.
pub const SOURCE: &str = "uploader";
/// Events of one snapshot at most (a work calendar of 1.5 years holds a few hundred, measured 2026-10-17: 235).
pub const MAX_EVENTS: usize = 20_000;
/// The body limit of the upload route (a snapshot of [`MAX_EVENTS`] small events stays below it).
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
pub struct Upload {
    pub window: Window,
    pub events: Vec<UploadEvent>,
}

#[derive(serde::Serialize, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub created: usize,
    pub updated: usize,
    pub deleted: usize,
    pub unchanged: usize,
}

/// What the device page shows once after [`setup`].
#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct Made {
    pub url: String,
    pub user: String,
    pub token: String,
    pub calendar: String,
    pub calendar_name: String,
}

impl Made {
    /// The uploader's `[practical]` config section (its README), ready to paste.
    pub fn toml(&self) -> String {
        format!("[practical]\nurl = \"{}\"\nuser = \"{}\"\ntoken = \"{}\"\ncalendar = \"{}\"\n", self.url, self.user, self.token, self.calendar)
    }
}

/// A new uploader device: its app token (`device`) and its read-only calendar (`calendar_name`).
pub async fn setup(state: &AppState, user: &Principal, device: &str, calendar_name: &str) -> AppResult<Made> {
    let (device, calendar_name) = (device.trim(), calendar_name.trim());
    if device.is_empty() || calendar_name.is_empty() {
        return Err(AppError::bad("err-name-required"));
    }
    if device.chars().count() > crate::domain::connect::NAME_MAX || calendar_name.chars().count() > crate::domain::connect::NAME_MAX {
        return Err(AppError::bad("err-name-too-long"));
    }
    let new = NewCalendar {
        owner: user.id.clone(),
        name: calendar_name.to_string(),
        color: String::new(),
        description: String::new(),
        components: vec![rustical_ical::CalendarObjectType::Event],
        timezone: state.config.default_timezone.clone(),
    };
    let calendar = collections::create_calendar(state, user, new).await?;
    collections::set_read_only(state, &user.id, &calendar, SOURCE).await?;
    let token = crate::domain::tokens::create(state, user, device).await?;
    Ok(Made { url: state.config.public_url.clone(), user: user.id.clone(), token, calendar, calendar_name: calendar_name.to_string() })
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

/// The calendar `cal_id` is the user's uploader calendar, else NotFound (the route must not tell other
/// calendars apart from missing ones).
async fn ensure_target(state: &AppState, user: &Principal, cal_id: &str) -> AppResult<()> {
    if collections::read_only_source(state, &user.id, cal_id).await?.as_deref() != Some(SOURCE) {
        return Err(AppError::NotFound);
    }
    state.calendars.get_calendar(&user.id, cal_id, false).await.map_err(|_| AppError::NotFound)?;
    Ok(())
}

/// Checks one event of a snapshot: a parseable VCALENDAR whose one UID is `uid`, an event.
fn check(e: &UploadEvent) -> AppResult<CalendarObject> {
    let bad = |why: &str| AppError::bad1("err-uploader-ics", "uid", format!("{}: {why}", e.uid.chars().take(80).collect::<String>()));
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

pub async fn upload(state: &AppState, user: &Principal, cal_id: &str, u: Upload) -> AppResult<Counts> {
    ensure_target(state, user, cal_id).await?;
    if u.events.len() > MAX_EVENTS {
        return Err(AppError::bad("err-uploader-too-many"));
    }
    if u.window.end <= u.window.start {
        return Err(AppError::bad("err-date-range"));
    }
    // everything checked before the first write: a bad snapshot changes nothing
    let mut want = std::collections::HashMap::new();
    for e in &u.events {
        let obj = check(e)?;
        if want.insert(object_id(&e.uid), obj).is_some() {
            return Err(AppError::bad1("err-uploader-ics", "uid", format!("{}: duplicate", e.uid.chars().take(80).collect::<String>())));
        }
    }
    let have: std::collections::HashMap<String, CalendarObject> = state.calendars.get_objects(&user.id, cal_id).await?.into_iter().collect();
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
    let gone: Vec<&String> = have.iter().filter(|(id, o)| !want.contains_key(*id) && !before(o, u.window.start)).map(|(id, _)| id).collect();
    if puts.is_empty() && gone.is_empty() {
        return Ok(n);
    }
    let mut g = history::begin(state, Actor { src: SOURCE, who: user.id.clone(), label: None, reverts: None }).await;
    for (id, _) in &puts {
        g.object(Store::Cal, &user.id, cal_id, id);
    }
    for id in &gone {
        g.object(Store::Cal, &user.id, cal_id, id);
    }
    for chunk in puts.chunks(crate::domain::transfer::IMPORT_CHUNK) {
        state.calendars.put_objects(&user.id, cal_id, chunk.to_vec(), true).await?;
    }
    for id in gone {
        state.calendars.delete_object(&user.id, cal_id, id, true).await?;
        n.deleted += 1;
    }
    g.commit().await;
    Ok(n)
}
