//! Calendar events: window expansion for the views (caldata's rrule engine — checked against python-dateutil
//! on 60 real series, tests.rs `recurrence_matches_dateutil_oracle`), plus create/update/delete
//! through RustiCal's store. Every view works on [`Instance`]s in the user's zone.
//!
//! - [`instances`] expands a window; [`InstanceCache`] keeps exact windows, [`DensityCache`] the per-month
//!   counts of a year, [`ObjMetaCache`] (detail.rs) per-object metadata for the pickers.
//! - caldata builds generated instances without VALARMs, so a series' instances get the master's.
//! - Saving keeps what the editor does not model (ics.rs): only the properties in `MANAGED` are rewritten;
//!   every other line and component is copied verbatim, a VALARM keeps its position, RECURRENCE-ID overrides
//!   stay untouched (test `unchanged_save_keeps_every_property`). A changed DESCRIPTION drops Outlook's
//!   `X-ALT-DESC` and `STYLED-DESCRIPTION`, which would be stale.
//! - Links and attachments (props.rs): URL, ATTACH and CONFERENCE as one list; FMTTYPE, FILENAME, SIZE and
//!   FEATURE are modelled, other parameters ride along. Embedded files: [`EMBED_FILE_MAX`] (2 MiB) per file,
//!   [`EMBED_TOTAL_MAX`] per save. RELATED-TO is resolved to the other event's or task's title and date
//!   (detail.rs).
//! - ORGANIZER and ATTENDEE are display only: RustiCal does no scheduling (RFC 6638), and every parameter is
//!   saved back unchanged.
//! - Reminders (alarms.rs): presets per user (`alarm_presets_json`, `/api/alarm-presets`; the built-in list
//!   until the user changes it); PROXIMITY and ACKNOWLEDGED are kept as written.
//! - Recurrence: caldata is authoritative; the editor's preview is the client's. A rule the editor's strip
//!   cannot paint (BYSETPOS, several ordinals, BYHOUR …) is saved as typed.

use crate::domain::collections::{self, CalInfo};
use crate::domain::ical;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use caldata::component::{CalendarInnerData, Component, IcalEvent};
use chrono::{Duration, NaiveDate, NaiveDateTime, TimeZone};
use rustical_ical::CalendarObject;
use rustical_store::auth::Principal;
use rustical_store::{CalendarQuery, CalendarReadStore, CalendarWriteStore};
use std::collections::HashSet;

mod detail;
mod props;
mod alarms;
mod ics;
mod shift;
mod write;
pub use detail::*;
pub use props::*;
pub use alarms::*;
pub use ics::*;
pub use shift::*;
pub use write::*;

/// Text an instance shares with every other instance of its series: one allocation per expanded object, not one
/// per occurrence (a minutely series with a 20 KB description added 60 MiB per week view).
pub type Text = std::sync::Arc<str>;

#[derive(Debug, Clone, serde::Serialize)]
pub struct Instance {
    pub principal: String,
    pub cal_id: String,
    pub cal_name: String,
    pub color: String,
    pub object_id: String,
    pub uid: String,
    pub summary: Text,
    pub location: Text,
    pub description: Text,
    pub start: NaiveDateTime,
    /// exclusive end
    pub end: NaiveDateTime,
    pub all_day: bool,
    pub recurring: bool,
    pub categories: Vec<String>,
    /// minutes before start of the first relative alarm, if any (views, push)
    pub alarm_minutes: Option<i64>,
    /// every relative alarm as minutes before the start (RELATED=END ones shifted by the duration)
    pub alarms: Vec<i64>,
    pub is_group: bool,
    /// STATUS:CANCELLED — the views strike it through
    pub cancelled: bool,
}

impl Instance {
    pub fn key(&self) -> String {
        format!("{}/{}/{}", self.principal, self.cal_id, self.object_id)
    }
    pub fn start_date(&self) -> NaiveDate {
        self.start.date()
    }
    /// last calendar day the instance touches (inclusive)
    pub fn last_date(&self) -> NaiveDate {
        let e = if self.end > self.start { self.end - Duration::seconds(1) } else { self.start };
        e.date()
    }
    pub fn multi_day(&self) -> bool {
        self.last_date() > self.start_date()
    }
    pub fn time_label(&self) -> String {
        if self.all_day { String::new() } else { self.start.format("%H:%M").to_string() }
    }
    pub fn end_label(&self) -> String {
        if self.all_day { String::new() } else { self.end.format("%H:%M").to_string() }
    }
}

/// Relative alarms of an event as minutes before its start (a trigger after the start or
/// relative to the end comes out negative/shifted). Absolute (VALUE=DATE-TIME) triggers are
/// left out: they do not follow the instance. A TRIGGER `ical::parse_duration` refuses (beyond
/// ±400 years, or no duration at all) is skipped.
pub fn alarm_offsets(ev: &IcalEvent) -> Vec<i64> {
    let dur = ev.get_duration().map(|d| d.num_minutes()).unwrap_or(0);
    let mut out = vec![];
    for a in ev.get_alarms() {
        let Some(t) = a.get_property("TRIGGER") else { continue };
        if t.params.get_param("VALUE").is_some_and(|v| v.eq_ignore_ascii_case("DATE-TIME")) {
            continue;
        }
        if let Some(d) = ical::parse_duration(&t.value) {
            let related_end = t.params.get_param("RELATED").is_some_and(|v| v.eq_ignore_ascii_case("END"));
            out.push((-d.num_minutes()).saturating_sub(if related_end { dur } else { 0 }));
        }
    }
    out
}

/// What `instance_from` reads from an event's own properties, read once from the master: the
/// instances a light master (`light_master`) generates carry none of it.
struct MasterText {
    summary: Text,
    location: Text,
    description: Text,
    color: Option<String>,
    categories: Vec<String>,
    cancelled: bool,
}

/// The longest note a view instance carries (characters; longer ones end in "…"): the views and the window JSON
/// repeat it per occurrence, the editor reads the whole text (`get_event`).
pub const VIEW_NOTE_MAX: usize = 10_000;

/// `s` cut to `max` characters with an ellipsis; unchanged (no copy) when it is shorter.
fn cut(s: String, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((at, _)) if s.len() > max => format!("{}…", &s[..at]),
        _ => s,
    }
}

impl MasterText {
    /// Titles and places longer than an editor writes (`MAX_TITLE`, only another client stores them) are cut too:
    /// a week view prints each title twice per occurrence.
    fn of(ev: &IcalEvent) -> Self {
        MasterText {
            summary: cut(ical::text(ev, "SUMMARY").unwrap_or_else(|| "(ohne Titel)".into()), MAX_TITLE).into(),
            location: cut(ical::text(ev, "LOCATION").unwrap_or_default(), MAX_TITLE).into(),
            description: cut(ical::text(ev, "DESCRIPTION").unwrap_or_default(), VIEW_NOTE_MAX).into(),
            // RFC 7986 COLOR on the event wins over the calendar colour
            color: ical::text(ev, "COLOR").map(|c| c.trim().to_string()).filter(|c| valid_color(c)),
            categories: ical::list(ev, "CATEGORIES"),
            cancelled: ical::text(ev, "STATUS").is_some_and(|s| s.eq_ignore_ascii_case("CANCELLED")),
        }
    }
}

fn instance_from(ev: &IcalEvent, cal: &CalInfo, object_id: &str, tz: chrono_tz::Tz, recurring: bool, alarms: Vec<i64>) -> Instance {
    instance_with(ev, &MasterText::of(ev), cal, object_id, tz, recurring, alarms)
}

fn instance_with(ev: &IcalEvent, text: &MasterText, cal: &CalInfo, object_id: &str, tz: chrono_tz::Tz, recurring: bool, alarms: Vec<i64>) -> Instance {
    let (start, all_day) = ical::local(&ev.dtstart.0, tz);
    let end = if all_day {
        // Whole days: DTEND is an exclusive DATE. Go through the duration, not the converted
        // DTEND — caldata expands an all-day series to `recurrence-id + duration`, which is a
        // UTC DateTime (caldata event/mod.rs:259) that ical::local would shift into the zone
        // (+2 h in summer → the instance spilled into the next day).
        let days = ev.get_duration().map(|d| d.num_days()).filter(|d| *d > 0).unwrap_or(1);
        start.checked_add_signed(Duration::days(days)).unwrap_or(start)
    } else {
        match (&ev.dtend, ev.get_duration()) {
            (Some(e), _) => ical::local(&e.0, tz).0,
            (None, Some(d)) => start.checked_add_signed(d).unwrap_or(start),
            (None, None) => start,
        }
        .max(start)
    };
    Instance {
        principal: cal.principal.clone(),
        cal_id: cal.id.clone(),
        cal_name: cal.displayname.clone(),
        color: text.color.clone().unwrap_or_else(|| cal.color.clone()),
        object_id: object_id.to_string(),
        uid: ev.get_uid().to_string(),
        summary: text.summary.clone(),
        location: text.location.clone(),
        description: text.description.clone(),
        start,
        end,
        all_day,
        recurring,
        categories: text.categories.clone(),
        alarm_minutes: alarms.first().copied(),
        alarms,
        is_group: cal.is_group,
        cancelled: text.cancelled,
    }
}

/// The view window clamped to `ical::YEARS` (no stored date lies outside, and ±days on its edges stay
/// representable); `None` when nothing of it is left.
fn clamp_window(start: NaiveDate, end: NaiveDate) -> Option<(NaiveDate, NaiveDate)> {
    let (lo, hi) = (NaiveDate::from_ymd_opt(*ical::YEARS.start(), 1, 1)?, NaiveDate::from_ymd_opt(*ical::YEARS.end(), 12, 31)?);
    let (s, e) = (start.clamp(lo, hi), end.clamp(lo, hi));
    (s < e).then_some((s, e))
}

/// Calendars the user may see, optionally narrowed to a filter set of "principal/id".
pub async fn visible_calendars(state: &AppState, user: &Principal, filter: Option<&HashSet<String>>) -> AppResult<Vec<CalInfo>> {
    let all = collections::calendars_for(state, user, false).await?;
    Ok(match filter {
        Some(f) => all.into_iter().filter(|c| f.contains(&format!("{}/{}", c.principal, c.id))).collect(),
        None => all,
    })
}

/// All instances touching [start, end) in the user's zone, sorted by start. Each calendar's share
/// comes from `InstanceCache` when that calendar was expanded for the same window, zone and synctoken.
pub async fn instances(state: &AppState, cals: &[CalInfo], start: NaiveDate, end: NaiveDate, tz: chrono_tz::Tz) -> AppResult<Vec<Instance>> {
    let mut out = vec![];
    let Some((start, end)) = clamp_window(start, end) else { return Ok(out) };
    for cal in cals {
        if !cal.components.iter().any(|c| c == "VEVENT") {
            continue;
        }
        // views' windows only: a year or more has its own caches (density, mentions, search corpus)
        let cacheable = (end - start).num_days() <= INSTANCE_CACHE_MAX_DAYS;
        let key: WinKey = (cal.principal.clone(), cal.id.clone(), cal.synctoken, tz, start, end);
        let part = match state.caches.instances.get(&key).filter(|_| cacheable) {
            Some(p) => p,
            None => {
                // without the calendar's name and colour: those change without the synctoken (filled in below)
                let bare = CalInfo { displayname: String::new(), color: String::new(), is_group: false, ..cal.clone() };
                // sorted alone: the merged list below then sorts as runs, with the same (stable) result
                let mut p = expand(state, &bare, start, end, tz, None).await?;
                sort_instances(&mut p);
                p.shrink_to_fit();
                let p = std::sync::Arc::new(p);
                if cacheable {
                    state.caches.instances.put(key, p.clone());
                }
                p
            }
        };
        out.extend(part.iter().map(|i| {
            let mut i = i.clone();
            i.cal_name.clone_from(&cal.displayname);
            i.is_group = cal.is_group;
            // `instance_from` takes the event's own COLOR when valid (never empty), else the calendar's
            if i.color.is_empty() {
                i.color.clone_from(&cal.color);
            }
            i
        }));
    }
    sort_instances(&mut out);
    Ok(out)
}

/// Note text one response repeats at most (bytes): past it the further instances go without their note. A day
/// with a minutely series and a 10 000-character note would otherwise be 1 440 copies of it in one page or JSON.
pub const NOTES_PER_RESPONSE: usize = 1 << 20;

/// Empties the notes of `v` past `NOTES_PER_RESPONSE` (in order).
pub fn limit_notes(v: &mut [Instance]) {
    let mut left = NOTES_PER_RESPONSE;
    for i in v.iter_mut() {
        match left.checked_sub(i.description.len()) {
            Some(l) => left = l,
            None => i.description = Text::from(""),
        }
    }
}

fn sort_instances(v: &mut [Instance]) {
    v.sort_by(|a, b| a.start.cmp(&b.start).then(b.all_day.cmp(&a.all_day)).then(a.summary.cmp(&b.summary)));
}

/// Instances one `expand` call keeps at most: caldata stops a series at 2 048 occurrences per window
/// (event/mod.rs:210), so only many such series together reach it — then the rest of the calendar is
/// left out of that window (logged) rather than the process running out of memory (192 MiB in prod).
pub(crate) const EXPAND_MAX: usize = 50_000;

/// A master whose properties beyond what the expansion needs (`LIGHT_KEEP`) exceed this many bytes is
/// expanded from a light copy (`light_master`): caldata clones every property of the master into every
/// generated occurrence (event/mod.rs:230), a 20 KB DESCRIPTION 2 048 times = 40 MB per view.
const LIGHT_MIN_BYTES: usize = 2048;

/// What the expansion and `instance_with` read of a generated occurrence: dates, rule parts, UID, DTSTAMP.
const LIGHT_KEEP: &[&str] = &["UID", "DTSTAMP", "DTSTART", "DTEND", "DURATION", "RRULE", "RDATE", "EXDATE", "EXRULE"];

/// Marks the light master (`light_master`); every occurrence generated from it carries the line.
const LIGHT_MARK: &str = "X-PRACTICAL-LIGHT";

/// The object again with its master VEVENT cut down to `LIGHT_KEEP` (no VALARMs) plus `LIGHT_MARK`;
/// overrides and VTIMEZONEs stay as stored, so the expansion — time zones, a THISANDFUTURE override
/// that becomes the template — is caldata's own. `None` when the copy does not parse (the full master
/// is expanded then).
fn light_master(ics: &str) -> Option<CalendarObject> {
    let mut out = String::with_capacity(ics.len().min(64 * 1024));
    let mut block: Option<Vec<String>> = None;
    for l in unfold(&ics.replace("\r\n", "\n")) {
        if let Some(b) = block.as_mut() {
            b.push(l);
            if b.last().is_some_and(|l| l.starts_with("END:VEVENT")) {
                let b = block.take().unwrap_or_default();
                let master = !b.iter().any(|l| prop_name(l) == "RECURRENCE-ID");
                let mut depth = 0;
                for l in &b {
                    let top = depth == 1;
                    if l.starts_with("BEGIN:") {
                        depth += 1;
                    }
                    let keep = !master || (depth == 1 && (l.starts_with("BEGIN:VEVENT") || (top && LIGHT_KEEP.contains(&prop_name(l).as_str())))) || l.starts_with("END:VEVENT");
                    if l.starts_with("END:") {
                        depth -= 1;
                    }
                    if master && l.starts_with("END:VEVENT") {
                        out.push_str(LIGHT_MARK);
                        out.push_str(":1\r\n");
                    }
                    if keep {
                        out.push_str(&ical::fold(l));
                        out.push_str("\r\n");
                    }
                }
            }
        } else if l.starts_with("BEGIN:VEVENT") {
            block = Some(vec![l]);
        } else if !l.is_empty() {
            out.push_str(&ical::fold(&l));
            out.push_str("\r\n");
        }
    }
    CalendarObject::from_ics(out).ok()
}

/// Bytes of the master's properties a light copy leaves out (VALARMs do not count: caldata gives a generated
/// occurrence none).
fn heavy_bytes(main: &IcalEvent) -> usize {
    main.get_properties().iter().filter(|p| !LIGHT_KEEP.contains(&p.name.as_str())).map(|p| p.value.len() + p.name.len()).sum()
}

/// What a read already logged as skipped (an object key, a calendar over `EXPAND_MAX`): once per process.
static LOGGED: std::sync::Mutex<Option<HashSet<String>>> = std::sync::Mutex::new(None);

fn first_time(what: String) -> bool {
    LOGGED.lock().unwrap_or_else(|p| p.into_inner()).get_or_insert_with(HashSet::new).insert(what)
}

/// Runs `f` for one stored object; a panic in it (an object no parser here was written for) skips the object
/// and is logged once per key, so one broken object never takes the view of a whole calendar down with it.
pub(crate) fn per_object<T>(key: &str, f: impl FnOnce() -> T) -> Option<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(v) => Some(v),
        Err(e) => {
            if first_time(key.to_string()) {
                let msg = e.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| e.downcast_ref::<String>().cloned()).unwrap_or_default();
                tracing::warn!("object {key} skipped: reading it panicked: {msg}");
            }
            None
        }
    }
}

/// One calendar's instances touching [start, end), unsorted (in the store's object order), and with
/// `extras` the `SearchExtra` of every object read.
async fn expand(state: &AppState, cal: &CalInfo, start: NaiveDate, end: NaiveDate, tz: chrono_tz::Tz, mut extras: Option<&mut std::collections::HashMap<String, SearchExtra>>) -> AppResult<Vec<Instance>> {
    let Some((start, end)) = clamp_window(start, end) else { return Ok(vec![]) };
    let ws = ical::to_utc(start.and_time(chrono::NaiveTime::MIN), tz) - Duration::days(1);
    let we = ical::to_utc(end.and_time(chrono::NaiveTime::MIN), tz) + Duration::days(1);
    let win_start = start.and_time(chrono::NaiveTime::MIN);
    let win_end = end.and_time(chrono::NaiveTime::MIN);
    let mut out = vec![];
    let objs = state
        .calendars
        .calendar_query(&cal.principal, &cal.id, CalendarQuery { time_start: Some(start), time_end: Some(end) })
        .await?;
    for (id, obj) in objs {
        let key = format!("{}/{}/{id}", cal.principal, cal.id);
        if let Some(x) = extras.as_deref_mut() {
            x.insert(key.clone(), per_object(&key, || search_extra(&obj)).unwrap_or_default());
        }
        if out.len() >= EXPAND_MAX {
            if first_time(format!("{}/{} over the instance limit", cal.principal, cal.id)) {
                tracing::warn!("calendar {}/{}: more than {EXPAND_MAX} instances in {start}..{end}, the rest is left out", cal.principal, cal.id);
            }
            break;
        }
        let before = out.len();
        if per_object(&key, || expand_object(&obj, cal, &id, tz, (ws, we), (win_start, win_end), LIGHT_MIN_BYTES, &mut out)).is_none() {
            out.truncate(before);
        }
    }
    out.truncate(EXPAND_MAX);
    Ok(out)
}

/// `light_min`: `LIGHT_MIN_BYTES` (tests force both paths with 0 and `usize::MAX`).
#[allow(clippy::too_many_arguments)]
fn expand_object(obj: &CalendarObject, cal: &CalInfo, id: &str, tz: chrono_tz::Tz, (ws, we): (chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>), (win_start, win_end): (NaiveDateTime, NaiveDateTime), light_min: usize, out: &mut Vec<Instance>) {
    let CalendarInnerData::Event(main, overrides) = obj.get_inner().get_inner() else { return };
    let recurring = main.has_rruleset();
    // caldata builds generated instances without VALARMs (event/mod.rs:262), so a series
    // would never remind; an instance without own alarms takes the master's
    let master_alarms = alarm_offsets(main);
    let light = if recurring && heavy_bytes(main) > light_min { light_master(&obj.get_ics()) } else { None };
    let expanded = match light.as_ref().map(|l| l.get_inner().get_inner()) {
        Some(CalendarInnerData::Event(lm, lov)) => lm.expand_recurrence(Some(ws), Some(we), lov),
        _ => main.expand_recurrence(Some(ws), Some(we), overrides),
    };
    let master_text = light.is_some().then(|| MasterText::of(main));
    for ev in &expanded {
        let alarms = if ev.get_alarms().is_empty() { master_alarms.clone() } else { alarm_offsets(ev) };
        let inst = match &master_text {
            Some(t) if ev.get_property(LIGHT_MARK).is_some() => instance_with(ev, t, cal, id, tz, recurring, alarms),
            _ => instance_from(ev, cal, id, tz, recurring, alarms),
        };
        if inst.start < win_end && inst.end > win_start || (inst.start == inst.end && inst.start >= win_start && inst.start < win_end) {
            out.push(inst);
            if out.len() >= EXPAND_MAX {
                return;
            }
        }
    }
}

/// One object's instances touching [start, end), read as `expand` reads them (window clamped, a panic of the
/// object caught, at most `EXPAND_MAX`): the search corpus re-reads single objects (`search::cal_part_update`).
pub(crate) fn object_instances(obj: &CalendarObject, cal: &CalInfo, id: &str, start: NaiveDate, end: NaiveDate, tz: chrono_tz::Tz) -> Vec<Instance> {
    let Some((start, end)) = clamp_window(start, end) else { return vec![] };
    let ws = ical::to_utc(start.and_time(chrono::NaiveTime::MIN), tz) - Duration::days(1);
    let we = ical::to_utc(end.and_time(chrono::NaiveTime::MIN), tz) + Duration::days(1);
    let (win_start, win_end) = (start.and_time(chrono::NaiveTime::MIN), end.and_time(chrono::NaiveTime::MIN));
    let mut out = vec![];
    let key = format!("{}/{}/{id}", cal.principal, cal.id);
    if per_object(&key, || expand_object(obj, cal, id, tz, (ws, we), (win_start, win_end), LIGHT_MIN_BYTES, &mut out)).is_none() {
        out.clear();
    }
    out
}

/// `search_extra` of one object, a panic caught as `expand` catches it (none: the default).
pub(crate) fn object_extra(obj: &CalendarObject, cal: &CalInfo, id: &str) -> SearchExtra {
    per_object(&format!("{}/{}/{id}", cal.principal, cal.id), || search_extra(obj)).unwrap_or_default()
}

type WinKey = (String, String, i64, chrono_tz::Tz, NaiveDate, NaiveDate);

/// Expanded instances per (calendar, synctoken, zone, window): the calendar views, the endless lists
/// and the reminder loop ask for the same windows again until something in the calendar changes —
/// RustiCal moves the synctoken on every object write. Exact windows only: caldata's expansion depends
/// on the window (a THISANDFUTURE override before it, the 2048-occurrence cap), so a wider entry is
/// never cut down to a narrower one. Least recently used entries go first above `INSTANCE_CACHE_BYTES`
/// (estimated heap); an entry above a quarter of it (a lifetime of history) is not kept.
#[derive(Default)]
pub struct InstanceCache(std::sync::Mutex<InstEntries>);

#[derive(Default)]
pub struct InstEntries {
    map: std::collections::HashMap<WinKey, (std::sync::Arc<Vec<Instance>>, usize, u64)>,
    bytes: usize,
    tick: u64,
}

const INSTANCE_CACHE_BYTES: usize = 4 << 20;
/// the longest window kept: the views ask for ≤ 84 days (week stream + month grid), the endless lists ≤ 120
const INSTANCE_CACHE_MAX_DAYS: i64 = 120;

/// Heap of a cached part: each non-empty String/Vec is one allocation, rounded up to 16 bytes plus
/// 16 of allocator overhead (macOS `heap` size classes; synthetic DB: 6.0 MB estimated for 5.7 MB live-heap growth).
fn est_bytes(v: &[Instance]) -> usize {
    let a = |n: usize| if n == 0 { 0 } else { n.div_ceil(16) * 16 + 16 };
    v.iter().map(|i| std::mem::size_of::<Instance>() + [i.principal.len(), i.cal_id.len(), i.object_id.len(), i.uid.len(), i.summary.len(), i.location.len(), i.description.len(), i.color.len()].iter().map(|n| a(*n)).sum::<usize>() + a(i.categories.len() * 24) + i.categories.iter().map(|c| a(c.len())).sum::<usize>() + a(i.alarms.len() * 8)).sum::<usize>() + 64
}

impl InstanceCache {
    fn get(&self, k: &WinKey) -> Option<std::sync::Arc<Vec<Instance>>> {
        let mut g = self.0.lock().unwrap();
        g.tick += 1;
        let t = g.tick;
        g.map.get_mut(k).map(|e| {
            e.2 = t;
            e.0.clone()
        })
    }
    fn put(&self, k: WinKey, v: std::sync::Arc<Vec<Instance>>) {
        let size = est_bytes(&v);
        if size > INSTANCE_CACHE_BYTES / 4 {
            return;
        }
        let mut g = self.0.lock().unwrap();
        let g = &mut *g;
        // a request that read the calendar before a write must not push out the newer token's windows
        if g.map.keys().any(|o| o.0 == k.0 && o.1 == k.1 && o.2 > k.2) {
            return;
        }
        // older tokens of this calendar are dead
        let stale: Vec<WinKey> = g.map.keys().filter(|o| o.0 == k.0 && o.1 == k.1 && o.2 < k.2).cloned().collect();
        for o in stale {
            g.bytes -= g.map.remove(&o).unwrap().1;
        }
        while g.bytes + size > INSTANCE_CACHE_BYTES {
            let Some(old) = g.map.iter().min_by_key(|(_, e)| e.2).map(|(o, _)| o.clone()) else { break };
            g.bytes -= g.map.remove(&old).unwrap().1;
        }
        g.tick += 1;
        if let Some(prev) = g.map.insert(k, (v, size, g.tick)) {
            g.bytes -= prev.1;
        }
        g.bytes += size;
    }
}

/// `instances` and the `SearchExtra` of every object read for them (any component, as the store returns
/// them; keyed "principal/cal/object") — one fetch and parse of the window's objects for both. Not
/// cached: the search corpus keeps its own (`search::IndexCache`).
pub async fn instances_with_extras(state: &AppState, cals: &[CalInfo], start: NaiveDate, end: NaiveDate, tz: chrono_tz::Tz) -> AppResult<(Vec<Instance>, std::collections::HashMap<String, SearchExtra>)> {
    let mut out = vec![];
    let mut extras = std::collections::HashMap::new();
    for cal in cals.iter().filter(|c| c.components.iter().any(|c| c == "VEVENT")) {
        out.extend(expand(state, cal, start, end, tz, Some(&mut extras)).await?);
    }
    sort_instances(&mut out);
    Ok((out, extras))
}

/// What the top-bar search indexes beyond an `Instance` (domain::search): people, links, status words,
/// GEO and RELATED-TO of the master, per object key ("principal/cal/object").
#[derive(Debug, Default)]
pub struct SearchExtra {
    /// the organizer first, then the attendees
    pub people: Vec<Person>,
    pub links: Vec<Link>,
    pub status: String,
    pub transp: String,
    pub class: String,
    pub priority: Option<i64>,
    pub geo: Option<(f64, f64)>,
    /// RELATED-TO UIDs
    pub related: Vec<String>,
}

pub(crate) fn search_extra(obj: &CalendarObject) -> SearchExtra {
    let (raw, _) = split_ics(&obj.get_ics());
    let p = &raw.props;
    let get = |n: &str| p.iter().find(|l| prop_name(l) == n).map(|l| ical::split_line(l).2.trim().to_ascii_uppercase()).unwrap_or_default();
    let mut people = persons_of(p, "ORGANIZER");
    people.extend(persons_of(p, "ATTENDEE"));
    let geo = p.iter().find(|l| prop_name(l) == "GEO").and_then(|l| { let v = ical::split_line(l).2; let (a, b) = v.split_once(';')?; Some((a.trim().parse().ok()?, b.trim().parse().ok()?)) });
    SearchExtra { people, links: links_of(p), status: get("STATUS"), transp: get("TRANSP"), class: get("CLASS"), priority: get("PRIORITY").parse().ok(), geo, related: related_of(p).into_iter().map(|r| r.uid).collect() }
}

/// Number of instances per month of `year` (picker density) — one pass over the year per calendar,
/// kept in `DensityCache`. `instances` expands every calendar on its own, so the set's counts are the
/// sum of its calendars' and a calendar filter reuses them.
pub async fn month_density(state: &AppState, cals: &[CalInfo], year: i32, tz: chrono_tz::Tz) -> AppResult<[usize; 12]> {
    let mut d = [0usize; 12];
    if !ical::YEARS.contains(&year) {
        return Ok(d);
    }
    let (Some(start), Some(end)) = (NaiveDate::from_ymd_opt(year, 1, 1), NaiveDate::from_ymd_opt(year + 1, 1, 1)) else { return Ok(d) };
    for cal in cals.iter().filter(|c| c.components.iter().any(|c| c == "VEVENT")) {
        let counts = match state.caches.density.get(cal, tz, year) {
            Some(c) => c,
            None => {
                let mut c = [0usize; 12];
                for i in instances(state, std::slice::from_ref(cal), start, end, tz).await? {
                    use chrono::Datelike;
                    if i.start.date().year() == year {
                        c[(i.start.date().month() - 1) as usize] += 1;
                    }
                }
                state.caches.density.put(cal, tz, year, c);
                c
            }
        };
        for (a, b) in d.iter_mut().zip(counts) {
            *a += b;
        }
    }
    Ok(d)
}

/// The picker density of one calendar per (synctoken, zone, year). RustiCal moves a calendar's synctoken
/// on every object write (store_sqlite calendar_store/mod.rs `log_object_operation`), and the counts
/// depend on nothing else of the calendar. At most `DENSITY_KEEP` (zone, year) pairs per calendar.
#[derive(Default)]
pub struct DensityCache(std::sync::Mutex<std::collections::HashMap<(String, String), Vec<(i64, chrono_tz::Tz, i32, [usize; 12])>>>);

const DENSITY_KEEP: usize = 6;

impl DensityCache {
    fn get(&self, cal: &CalInfo, tz: chrono_tz::Tz, year: i32) -> Option<[usize; 12]> {
        let m = self.0.lock().unwrap();
        m.get(&(cal.principal.clone(), cal.id.clone()))?.iter().find(|e| e.0 == cal.synctoken && e.1 == tz && e.2 == year).map(|e| e.3)
    }
    fn put(&self, cal: &CalInfo, tz: chrono_tz::Tz, year: i32, counts: [usize; 12]) {
        let mut m = self.0.lock().unwrap();
        let v = m.entry((cal.principal.clone(), cal.id.clone())).or_default();
        // a request that read the calendar before a write must not push out the newer token's counts
        if v.iter().any(|e| e.0 > cal.synctoken) {
            return;
        }
        v.retain(|e| e.0 == cal.synctoken && !(e.1 == tz && e.2 == year));
        if v.len() >= DENSITY_KEEP {
            v.remove(0);
        }
        v.push((cal.synctoken, tz, year, counts));
    }
}

#[cfg(test)]
mod pinned;

#[cfg(test)]
mod tests;
