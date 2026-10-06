//! Calendar events: window expansion for the views (caldata's rrule engine —
//! validated against the real calendar, plan D11), plus create/update/delete
//! through RustiCal's store. Every view works on `Instance`s in the user's zone.

use crate::domain::collections::{self, CalInfo};
use crate::domain::ical;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use caldata::component::{CalendarInnerData, Component, IcalEvent};
use caldata::types::parse_duration;
use chrono::{Duration, NaiveDate, NaiveDateTime, TimeZone};
use rustical_ical::CalendarObject;
use rustical_store::auth::Principal;
use rustical_store::{CalendarQuery, CalendarReadStore, CalendarWriteStore};
use std::collections::HashSet;

#[derive(Debug, Clone, serde::Serialize)]
pub struct Instance {
    pub principal: String,
    pub cal_id: String,
    pub cal_name: String,
    pub color: String,
    pub object_id: String,
    pub uid: String,
    pub summary: String,
    pub location: String,
    pub description: String,
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
/// left out: they do not follow the instance.
pub fn alarm_offsets(ev: &IcalEvent) -> Vec<i64> {
    let dur = ev.get_duration().map(|d| d.num_minutes()).unwrap_or(0);
    let mut out = vec![];
    for a in ev.get_alarms() {
        let Some(t) = a.get_property("TRIGGER") else { continue };
        if t.params.get_param("VALUE").is_some_and(|v| v.eq_ignore_ascii_case("DATE-TIME")) {
            continue;
        }
        if let Ok(d) = parse_duration(&t.value) {
            let related_end = t.params.get_param("RELATED").is_some_and(|v| v.eq_ignore_ascii_case("END"));
            out.push(-d.num_minutes() - if related_end { dur } else { 0 });
        }
    }
    out
}

fn instance_from(ev: &IcalEvent, cal: &CalInfo, object_id: &str, tz: chrono_tz::Tz, recurring: bool, alarms: Vec<i64>) -> Instance {
    let (start, all_day) = ical::local(&ev.dtstart.0, tz);
    let end = if all_day {
        // Whole days: DTEND is an exclusive DATE. Go through the duration, not the converted
        // DTEND — caldata expands an all-day series to `recurrence-id + duration`, which is a
        // UTC DateTime (caldata event/mod.rs:259) that ical::local would shift into the zone
        // (+2 h in summer → the instance spilled into the next day).
        let days = ev.get_duration().map(|d| d.num_days()).filter(|d| *d > 0).unwrap_or(1);
        start + Duration::days(days)
    } else {
        match (&ev.dtend, ev.get_duration()) {
            (Some(e), _) => ical::local(&e.0, tz).0,
            (None, Some(d)) => start + d,
            (None, None) => start,
        }
        .max(start)
    };
    Instance {
        principal: cal.principal.clone(),
        cal_id: cal.id.clone(),
        cal_name: cal.displayname.clone(),
        // RFC 7986 COLOR on the event wins over the calendar colour
        color: ical::text(ev, "COLOR").map(|c| c.trim().to_string()).filter(|c| valid_color(c)).unwrap_or_else(|| cal.color.clone()),
        object_id: object_id.to_string(),
        uid: ev.get_uid().to_string(),
        summary: ical::text(ev, "SUMMARY").unwrap_or_else(|| "(ohne Titel)".into()),
        location: ical::text(ev, "LOCATION").unwrap_or_default(),
        description: ical::text(ev, "DESCRIPTION").unwrap_or_default(),
        start,
        end,
        all_day,
        recurring,
        categories: ical::list(ev, "CATEGORIES"),
        alarm_minutes: alarms.first().copied(),
        alarms,
        is_group: cal.is_group,
        cancelled: ical::text(ev, "STATUS").is_some_and(|s| s.eq_ignore_ascii_case("CANCELLED")),
    }
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

fn sort_instances(v: &mut [Instance]) {
    v.sort_by(|a, b| a.start.cmp(&b.start).then(b.all_day.cmp(&a.all_day)).then(a.summary.cmp(&b.summary)));
}

/// One calendar's instances touching [start, end), unsorted (in the store's object order), and with
/// `extras` the `SearchExtra` of every object read.
async fn expand(state: &AppState, cal: &CalInfo, start: NaiveDate, end: NaiveDate, tz: chrono_tz::Tz, mut extras: Option<&mut std::collections::HashMap<String, SearchExtra>>) -> AppResult<Vec<Instance>> {
    let ws = ical::to_utc(start.and_hms_opt(0, 0, 0).unwrap(), tz) - Duration::days(1);
    let we = ical::to_utc(end.and_hms_opt(0, 0, 0).unwrap(), tz) + Duration::days(1);
    let win_start = start.and_hms_opt(0, 0, 0).unwrap();
    let win_end = end.and_hms_opt(0, 0, 0).unwrap();
    let mut out = vec![];
    let objs = state
        .calendars
        .calendar_query(&cal.principal, &cal.id, CalendarQuery { time_start: Some(start), time_end: Some(end) })
        .await?;
    for (id, obj) in objs {
        if let Some(x) = extras.as_deref_mut() {
            x.insert(format!("{}/{}/{id}", cal.principal, cal.id), search_extra(&obj));
        }
        let CalendarInnerData::Event(main, overrides) = obj.get_inner().get_inner() else { continue };
        let recurring = main.has_rruleset();
        // caldata builds generated instances without VALARMs (event/mod.rs:262), so a series
        // would never remind; an instance without own alarms takes the master's
        let master_alarms = alarm_offsets(main);
        let expanded = main.expand_recurrence(Some(ws), Some(we), overrides);
        for ev in &expanded {
            let alarms = if ev.get_alarms().is_empty() { master_alarms.clone() } else { alarm_offsets(ev) };
            let inst = instance_from(ev, cal, &id, tz, recurring, alarms);
            if inst.start < win_end && inst.end > win_start || (inst.start == inst.end && inst.start >= win_start && inst.start < win_end) {
                out.push(inst);
            }
        }
    }
    Ok(out)
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
    v.iter().map(|i| std::mem::size_of::<Instance>() + [&i.principal, &i.cal_id, &i.object_id, &i.uid, &i.summary, &i.location, &i.description, &i.color].iter().map(|s| a(s.len())).sum::<usize>() + a(i.categories.len() * 24) + i.categories.iter().map(|c| a(c.len())).sum::<usize>() + a(i.alarms.len() * 8)).sum::<usize>() + 64
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

fn search_extra(obj: &CalendarObject) -> SearchExtra {
    let (raw, _) = split_ics(&obj.get_ics());
    let p = &raw.props;
    let get = |n: &str| p.iter().find(|l| prop_name(l) == n).map(|l| ical::split_line(l).2.trim().to_ascii_uppercase()).unwrap_or_default();
    let mut people = persons_of(p, "ORGANIZER");
    people.extend(persons_of(p, "ATTENDEE"));
    let geo = p.iter().find(|l| prop_name(l) == "GEO").and_then(|l| { let v = ical::split_line(l).2; let (a, b) = v.split_once(';')?; Some((a.trim().parse().ok()?, b.trim().parse().ok()?)) });
    SearchExtra { people, links: links_of(p), status: get("STATUS"), transp: get("TRANSP"), class: get("CLASS"), priority: get("PRIORITY").parse().ok(), geo, related: related_of(p).into_iter().map(|r| r.uid).collect() }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct EventDetail {
    pub principal: String,
    pub cal_id: String,
    pub object_id: String,
    pub uid: String,
    pub summary: String,
    pub location: String,
    pub description: String,
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
    pub all_day: bool,
    pub rrule: Option<String>,
    /// UNTIL of the RRULE as a local date (the rule itself carries it in UTC)
    pub rrule_until: Option<NaiveDate>,
    pub exdates: Vec<String>,
    pub rdates: Vec<String>,
    pub organizer: Option<Person>,
    pub attendees: Vec<Person>,
    pub alarms: Vec<Alarm>,
    pub categories: Vec<String>,
    /// CONFIRMED | TENTATIVE | CANCELLED | ""
    pub status: String,
    /// OPAQUE | TRANSPARENT | ""
    pub transp: String,
    /// PUBLIC | PRIVATE | CONFIDENTIAL | ""
    pub class: String,
    pub priority: Option<i64>,
    /// RFC 7986 COLOR (CSS colour name or #rrggbb)
    pub color: String,
    /// GEO as (lat, lon)
    pub geo: Option<(f64, f64)>,
    /// URL, ATTACH (URIs) and CONFERENCE
    pub links: Vec<Link>,
    /// RELATED-TO, resolved to events/tasks where the UID is known
    pub related: Vec<Related>,
    pub etag: String,
    pub has_overrides: bool,
    pub timezone: String,
}

pub async fn get_event(state: &AppState, user: &Principal, principal: &str, cal_id: &str, object_id: &str, tz: chrono_tz::Tz) -> AppResult<EventDetail> {
    collections::ensure_owner(user, principal)?;
    let obj = state.calendars.get_object(principal, cal_id, object_id, false).await?;
    let CalendarInnerData::Event(main, overrides) = obj.get_inner().get_inner() else {
        return Err(AppError::NotFound);
    };
    let cal = CalInfo { principal: principal.into(), owner_label: String::new(), is_group: false, id: cal_id.into(), displayname: String::new(), color: String::new(), description: String::new(), components: vec![], timezone_id: String::new(), subscription_url: None, deleted_at: None, synctoken: 0 };
    let i = instance_from(main, &cal, object_id, tz, main.has_rruleset(), alarm_offsets(main));
    let (raw, _) = split_ics(&obj.get_ics());
    let rrule = main.get_property("RRULE").map(|p| ical::rrule_canon(&p.value));
    let timezone = match &main.dtstart.0 {
        caldata::types::CalDateOrDateTime::DateTime(d) => ical::tz_of(&d.timezone()).map(|t| t.name().to_string()).unwrap_or_else(|| tz.name().to_string()),
        _ => tz.name().to_string(),
    };
    Ok(EventDetail {
        principal: principal.into(),
        cal_id: cal_id.into(),
        object_id: object_id.into(),
        uid: i.uid,
        summary: i.summary,
        location: i.location,
        description: i.description,
        start: i.start,
        end: i.end,
        all_day: i.all_day,
        rrule_until: rrule.as_deref().and_then(|r| rrule_until_local(r, tz)),
        rrule,
        exdates: dates_of(&raw.props, "EXDATE", tz),
        rdates: dates_of(&raw.props, "RDATE", tz),
        organizer: persons_of(&raw.props, "ORGANIZER").into_iter().next(),
        attendees: persons_of(&raw.props, "ATTENDEE"),
        alarms: raw.alarms.iter().enumerate().map(|(n, a)| parse_alarm(n, a, tz)).collect(),
        categories: i.categories,
        status: ical::text(main, "STATUS").unwrap_or_default().to_ascii_uppercase(),
        transp: ical::text(main, "TRANSP").unwrap_or_default().to_ascii_uppercase(),
        class: ical::text(main, "CLASS").unwrap_or_default().to_ascii_uppercase(),
        priority: ical::text(main, "PRIORITY").and_then(|p| p.trim().parse().ok()),
        color: ical::text(main, "COLOR").map(|c| c.trim().to_string()).filter(|c| valid_color(c)).unwrap_or_default(),
        geo: ical::text(main, "GEO").and_then(|g| { let (a, b) = g.split_once(';')?; Some((a.trim().parse().ok()?, b.trim().parse().ok()?)) }),
        links: links_of(&raw.props),
        related: related_of(&raw.props),
        etag: obj.get_etag(),
        has_overrides: !overrides.is_empty(),
        timezone,
    })
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct EventInput {
    pub principal: String,
    pub cal_id: String,
    pub summary: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub description: String,
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
    #[serde(default)]
    pub all_day: bool,
    /// RRULE value without the property name, e.g. FREQ=WEEKLY
    #[serde(default)]
    pub rrule: Option<String>,
    #[serde(default)]
    pub alarms: Vec<Alarm>,
    /// local "YYYY-MM-DD" (all-day) or "YYYY-MM-DDTHH:MM:SS" in the event's zone
    #[serde(default)]
    pub exdates: Vec<String>,
    #[serde(default)]
    pub rdates: Vec<String>,
    #[serde(default)]
    pub organizer: Option<Person>,
    #[serde(default)]
    pub attendees: Vec<Person>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub transp: String,
    #[serde(default)]
    pub class: String,
    #[serde(default)]
    pub priority: Option<i64>,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub geo: Option<(f64, f64)>,
    #[serde(default)]
    pub links: Vec<Link>,
    #[serde(default)]
    pub related: Vec<Related>,
    #[serde(default)]
    pub timezone: Option<String>,
}

/// URL / ATTACH / CONFERENCE as one list. FMTTYPE, FILENAME, SIZE and FEATURE are fields, every
/// other parameter (X-NC-…, X-…) rides along in `extra`. An embedded (BASE64) ATTACH is read as
/// `embedded` with its `bin_index` (position among the binary lines of the master); the client
/// sends it back with `keep=true` to retain it, and a new file as `data` (base64, input only).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Link {
    /// url | attach | conference
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub uri: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub embedded: bool,
    #[serde(default)]
    pub filename: String,
    /// FMTTYPE (media type)
    #[serde(default)]
    pub fmt: String,
    /// bytes: SIZE parameter, or the decoded length of an embedded file
    #[serde(default)]
    pub size: Option<u64>,
    /// CONFERENCE FEATURE values (AUDIO, VIDEO, CHAT, SCREEN, PHONE, FEED, MODERATOR)
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub bin_index: Option<usize>,
    #[serde(default)]
    pub keep: bool,
    #[serde(default, skip_serializing)]
    pub data: Option<String>,
    #[serde(default)]
    pub extra: Vec<(String, String)>,
}

/// RELATED-TO: a link to another event or task by UID; `target` is resolved for the editor
/// (`resolve_related`), never read back from the client.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Related {
    #[serde(default)]
    pub uid: String,
    /// RELTYPE (PARENT | CHILD | SIBLING), empty = no parameter
    #[serde(default)]
    pub reltype: String,
    #[serde(default)]
    pub extra: Vec<(String, String)>,
    #[serde(default, skip_deserializing, skip_serializing_if = "Option::is_none")]
    pub target: Option<RelatedTarget>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RelatedTarget {
    /// event | task
    pub kind: String,
    pub key: String,
    pub summary: String,
    /// start (event) or due (task) as a local date
    pub date: Option<NaiveDate>,
    pub done: bool,
}

pub(crate) fn related_of(props: &[String]) -> Vec<Related> {
    props.iter().filter(|l| prop_name(l) == "RELATED-TO").map(|l| {
        let (_, params, value) = ical::split_line(l);
        let mut r = Related { uid: value.trim().to_string(), ..Default::default() };
        for (k, v) in params {
            if k == "RELTYPE" { r.reltype = v.to_ascii_uppercase() } else { r.extra.push((k, v)) }
        }
        r
    }).collect()
}

pub(crate) fn related_lines(list: &[Related]) -> Vec<String> {
    list.iter().filter(|r| !r.uid.trim().is_empty() && !r.uid.contains(['\n', '\r', ';', ':'])).map(|r| {
        let mut line = "RELATED-TO".to_string();
        if let Some(t) = one_of(&r.reltype, &["PARENT", "CHILD", "SIBLING"]) {
            line.push_str(&format!(";RELTYPE={t}"));
        }
        for (k, v) in &r.extra {
            line.push_str(&format!(";{k}={v}"));
        }
        format!("{line}:{}", r.uid.trim())
    }).collect()
}

/// CATEGORIES across every visible calendar (events and tasks), most used first. Values a
/// client stamps on every object ("Come From :com.appgenix.bizcal") are not categories and are hidden.
/// `scope` picks the component the counts come from: "task" counts VTODOs, anything else VEVENTs.
/// A category rule belongs to one of the two, so its category list must too. Tasks come from the
/// task cache (`tasks::tasks_for`, hence `tz`); events from the object summaries (`ObjMetaCache`).
pub async fn category_counts(state: &AppState, user: &Principal, scope: &str, tz: chrono_tz::Tz) -> AppResult<Vec<(String, usize)>> {
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut count = |cats: &[String]| {
        for c in cats.iter().map(|c| c.trim()).filter(|c| !c.is_empty() && !c.starts_with("Come From :")) {
            match counts.get_mut(c) {
                Some(n) => *n += 1,
                None => {
                    counts.insert(c.to_string(), 1);
                }
            }
        }
    };
    let cals = visible_calendars(state, user, None).await?;
    if scope == "task" {
        for t in crate::domain::tasks::tasks_for(state, &cals, tz).await? {
            count(&t.categories);
        }
    } else {
        for cal in &cals {
            for o in object_meta(state, cal).await?.objs.iter().filter(|o| o.event) {
                count(&o.categories);
            }
        }
    }
    let mut out: Vec<(String, usize)> = counts.into_iter().collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.to_lowercase().cmp(&b.0.to_lowercase())));
    Ok(out)
}

/// Every event title with its number of events, most used first (the vacation rule's title test in
/// the settings runs over it). Same objects as `category_counts`.
pub async fn title_counts(state: &AppState, user: &Principal) -> AppResult<Vec<(String, usize)>> {
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for cal in visible_calendars(state, user, None).await? {
        for o in object_meta(state, &cal).await?.objs.iter().filter(|o| o.event) {
            let t = o.summary.as_deref().unwrap_or_default().trim();
            if !t.is_empty() {
                match counts.get_mut(t) {
                    Some(n) => *n += 1,
                    None => {
                        counts.insert(t.to_string(), 1);
                    }
                }
            }
        }
    }
    let mut out: Vec<(String, usize)> = counts.into_iter().collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.to_lowercase().cmp(&b.0.to_lowercase())));
    Ok(out)
}

/// What the whole-calendar scans (`category_counts`, `title_counts`, `colors::sources`) read of one
/// event or task, so they do not parse every object on every call.
#[derive(Debug, Clone)]
pub struct ObjMeta {
    pub id: String,
    pub uid: String,
    /// VEVENT; else VTODO (other components are not kept)
    pub event: bool,
    /// `ical::text` of SUMMARY, COLOR; `ical::list` of CATEGORIES
    pub summary: Option<String>,
    pub color: Option<String>,
    pub categories: Vec<String>,
}

fn meta_of(id: String, obj: &CalendarObject) -> Option<ObjMeta> {
    let uid = obj.get_uid().to_string();
    Some(match obj.get_inner().get_inner() {
        CalendarInnerData::Event(ev, _) => ObjMeta { id, uid, event: true, summary: ical::text(ev, "SUMMARY"), color: ical::text(ev, "COLOR"), categories: ical::list(ev, "CATEGORIES") },
        CalendarInnerData::Todo(t, _) => ObjMeta { id, uid, event: false, summary: ical::text(t, "SUMMARY"), color: ical::text(t, "COLOR"), categories: ical::list(t, "CATEGORIES") },
        _ => return None,
    })
}

/// The objects of one calendar at a synctoken, in the store's read order (`get_objects` walks the
/// (principal, cal_id, uid) index: by UID — `EXPLAIN QUERY PLAN` on RustiCal 0.16.4's schema).
pub struct CalMeta {
    token: i64,
    pub objs: Vec<ObjMeta>,
}

/// `CalMeta` per calendar. A moved token is caught up through the store's change log
/// (`sync_changes`, as `tasks::TaskCache` does), so an edit re-reads only what changed. Size: one
/// small record per object of the calendars someone looked at.
#[derive(Default)]
pub struct ObjMetaCache(std::sync::Mutex<std::collections::HashMap<(String, String), std::sync::Arc<CalMeta>>>);

pub async fn object_meta(state: &AppState, cal: &CalInfo) -> AppResult<std::sync::Arc<CalMeta>> {
    let k = (cal.principal.clone(), cal.id.clone());
    let cached = state.caches.obj_meta.0.lock().unwrap().get(&k).cloned();
    if let Some(c) = &cached
        && c.token >= cal.synctoken
    {
        // equal, or newer than the caller's calendar row (read before a write): at least as fresh
        return Ok(c.clone());
    }
    // `cal.synctoken` was read before the objects: a write racing this read is applied again next time
    let fresh = match cached {
        Some(c) => {
            let (changed, deleted, _) = state.calendars.sync_changes(&cal.principal, &cal.id, c.token).await?;
            let gone: HashSet<&str> = deleted.iter().map(String::as_str).chain(changed.iter().map(|(id, _)| id.as_str())).collect();
            let mut objs: Vec<ObjMeta> = c.objs.iter().filter(|o| !gone.contains(o.id.as_str())).cloned().collect();
            for (id, obj) in changed {
                if let Some(m) = meta_of(id, &obj) {
                    let at = objs.partition_point(|o| o.uid < m.uid);
                    objs.insert(at, m);
                }
            }
            CalMeta { token: cal.synctoken, objs }
        }
        None => {
            let mut objs: Vec<ObjMeta> = state.calendars.get_objects(&cal.principal, &cal.id).await?.into_iter().filter_map(|(id, obj)| meta_of(id, &obj)).collect();
            // collected in place from the parsed objects' buffer: release what the records do not need
            objs.shrink_to_fit();
            CalMeta { token: cal.synctoken, objs }
        }
    };
    let fresh = std::sync::Arc::new(fresh);
    let mut m = state.caches.obj_meta.0.lock().unwrap();
    if m.get(&k).is_none_or(|c| c.token <= fresh.token) {
        m.insert(k, fresh.clone());
    }
    Ok(fresh)
}

/// Fills `target` of every RELATED-TO whose UID is an event or task in a visible calendar. One
/// scan of all objects (≈ 320 ms in a debug build on 3 800 objects), only called when the
/// event has RELATED-TO lines.
pub async fn resolve_related(state: &AppState, user: &Principal, related: &mut [Related], tz: chrono_tz::Tz) -> AppResult<()> {
    let wanted: HashSet<String> = related.iter().map(|r| r.uid.trim().to_string()).filter(|u| !u.is_empty()).collect();
    if wanted.is_empty() {
        return Ok(());
    }
    let mut found: std::collections::HashMap<String, RelatedTarget> = std::collections::HashMap::new();
    for cal in visible_calendars(state, user, None).await? {
        for (id, obj) in state.calendars.get_objects(&cal.principal, &cal.id).await? {
            let uid = obj.get_uid().to_string();
            if !wanted.contains(&uid) || found.contains_key(&uid) {
                continue;
            }
            let key = format!("{}/{}/{}", cal.principal, cal.id, id);
            match obj.get_inner().get_inner() {
                CalendarInnerData::Event(ev, _) => {
                    found.insert(uid, RelatedTarget { kind: "event".into(), key, summary: ical::text(ev, "SUMMARY").unwrap_or_default(), date: Some(ical::local(&ev.dtstart.0, tz).0.date()), done: ev.get_property("STATUS").is_some_and(|s| s.value.eq_ignore_ascii_case("CANCELLED")) });
                }
                CalendarInnerData::Todo(t, _) => {
                    found.insert(uid, RelatedTarget { kind: "task".into(), key, summary: ical::text(t, "SUMMARY").unwrap_or_default(), date: t.due.as_ref().map(|d| ical::local(&d.0, tz).0.date()), done: t.get_property("STATUS").is_some_and(|s| s.value.eq_ignore_ascii_case("COMPLETED")) || t.get_property("COMPLETED").is_some() });
                }
                _ => {}
            }
            if found.len() == wanted.len() {
                break;
            }
        }
    }
    for r in related.iter_mut() {
        r.target = found.get(r.uid.trim()).cloned();
    }
    Ok(())
}

/// 2 MiB per embedded file, 6 MiB of new files per save (the JSON body carries them as base64).
pub const EMBED_FILE_MAX: usize = 2 * 1024 * 1024;
pub const EMBED_TOTAL_MAX: usize = 6 * 1024 * 1024;

/// Decoded byte length of a base64 text (ignoring whitespace and `=` padding, which carries no data),
/// without decoding it: every 4 characters are 3 bytes, a tail of 2 or 3 characters 1 or 2 bytes (a
/// lone tail character holds no whole byte).
fn base64_len(b64: &str) -> usize {
    let n = b64.bytes().filter(|b| !b.is_ascii_whitespace() && *b != b'=').count();
    n / 4 * 3 + [0, 0, 1, 2][n % 4]
}

pub(crate) fn is_binary_attach(params: &[(String, String)]) -> bool {
    params.iter().any(|(k, v)| (k == "VALUE" && v.eq_ignore_ascii_case("BINARY")) || k == "ENCODING")
}

pub(crate) fn links_of(props: &[String]) -> Vec<Link> {
    let mut bin = 0usize;
    props.iter().filter_map(|l| {
        let (name, params, value) = ical::split_line(l);
        let kind = match name.as_str() { "URL" => "url", "ATTACH" => "attach", "CONFERENCE" => "conference", _ => return None };
        let mut link = Link { kind: kind.into(), uri: value.trim().to_string(), ..Default::default() };
        let binary = kind == "attach" && is_binary_attach(&params);
        if binary {
            link.embedded = true;
            link.keep = true;
            link.uri = String::new();
            link.size = Some(base64_len(value.trim()) as u64);
            link.bin_index = Some(bin);
            bin += 1;
        }
        for (k, v) in params {
            match k.as_str() {
                "LABEL" => link.label = v.trim_matches('"').to_string(),
                "FMTTYPE" => link.fmt = v.trim_matches('"').to_string(),
                "FILENAME" | "X-FILENAME" => link.filename = v.trim_matches('"').trim_start_matches('/').to_string(),
                "SIZE" => link.size = v.trim().parse().ok().or(link.size),
                "FEATURE" => link.features = v.split(',').map(|f| f.trim().to_ascii_uppercase()).filter(|f| !f.is_empty()).collect(),
                "VALUE" | "ENCODING" => {}
                _ => link.extra.push((k, v)),
            }
        }
        Some(link)
    }).collect()
}

/// The n-th embedded ATTACH of a stored master: (media type, file name, bytes).
pub fn embedded_attachment(props: &[String], n: usize) -> Option<(String, String, Vec<u8>)> {
    use base64ct::Encoding;
    let line = props.iter().filter(|l| prop_name(l) == "ATTACH" && is_binary_attach(&ical::split_line(l).1)).nth(n)?;
    let (_, params, value) = ical::split_line(line);
    let get = |k: &str| params.iter().find(|(pk, _)| pk == k).map(|(_, v)| v.trim_matches('"').to_string());
    let fmt = get("FMTTYPE").filter(|f| f.contains('/')).unwrap_or_else(|| "application/octet-stream".into());
    let name = get("FILENAME").or_else(|| get("X-FILENAME")).map(|n| n.trim_start_matches('/').to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| format!("anhang-{}", n + 1));
    let clean: String = value.chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = base64ct::Base64::decode_vec(&clean).ok()?;
    Some((fmt, name, bytes))
}

pub(crate) fn link_lines(links: &[Link]) -> Vec<String> {
    let mut out = vec![];
    let mut url_written = false;
    for l in links {
        let uri = l.uri.trim();
        if l.embedded || uri.is_empty() || uri.contains(['\n', '\r']) {
            continue;
        }
        let mut name = match l.kind.as_str() { "url" => "URL", "attach" => "ATTACH", "conference" => "CONFERENCE", _ => continue };
        if name == "URL" {
            if url_written {
                name = "ATTACH"; // URL must not occur more than once: further websites are kept as ATTACH links
            }
            url_written = true;
        }
        let mut line = name.to_string();
        if name == "CONFERENCE" {
            line.push_str(";VALUE=URI");
        }
        line.push_str(&link_params(l, name == "CONFERENCE"));
        let label = l.label.trim().replace('"', "");
        if !label.is_empty() {
            line.push_str(&format!(";LABEL=\"{label}\"")); // LABEL is registered for CONFERENCE (RFC 7986); on URL/ATTACH it is an other-param every parser ignores
        }
        out.push(format!("{line}:{uri}"));
    }
    out
}

/// FMTTYPE / FILENAME / SIZE / FEATURE / extra as parameter text (leading `;`).
fn link_params(l: &Link, conference: bool) -> String {
    let mut p = String::new();
    let clean = |v: &str| v.trim().replace(['"', ';', ':', '\n', '\r'], "");
    if !l.fmt.trim().is_empty() {
        p.push_str(&format!(";FMTTYPE={}", clean(&l.fmt)));
    }
    if !l.filename.trim().is_empty() {
        let n = clean(&l.filename);
        p.push_str(&if n.contains([',', ' ']) { format!(";FILENAME=\"{n}\"") } else { format!(";FILENAME={n}") });
    }
    if let Some(sz) = l.size.filter(|_| !l.embedded) {
        p.push_str(&format!(";SIZE={sz}"));
    }
    if conference && !l.features.is_empty() {
        p.push_str(&format!(";FEATURE={}", l.features.iter().map(|f| clean(f).to_ascii_uppercase()).collect::<Vec<_>>().join(",")));
    }
    for (k, v) in &l.extra {
        p.push_str(&format!(";{k}={v}"));
    }
    p
}

/// New embedded files as `ATTACH;…;ENCODING=BASE64;VALUE=BINARY:` lines, size-checked.
pub(crate) fn embedded_lines(links: &[Link]) -> AppResult<Vec<String>> {
    let mut out = vec![];
    let mut total = 0usize;
    for l in links.iter().filter(|l| l.embedded) {
        let Some(data) = l.data.as_deref() else { continue };
        let clean: String = data.chars().filter(|c| !c.is_whitespace()).collect();
        if clean.is_empty() || !clean.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'=') {
            return Err(AppError::BadRequest("attachment is not base64".into()));
        }
        let n = base64_len(&clean);
        total += n;
        if n > EMBED_FILE_MAX || total > EMBED_TOTAL_MAX {
            return Err(AppError::BadRequest(format!("attachment too large: {} bytes (limit {} per file, {} per save)", n, EMBED_FILE_MAX, EMBED_TOTAL_MAX)));
        }
        let mut line = "ATTACH".to_string();
        line.push_str(&link_params(l, false));
        line.push_str(";ENCODING=BASE64;VALUE=BINARY:");
        line.push_str(&clean);
        out.push(line);
    }
    Ok(out)
}

pub fn valid_color(c: &str) -> bool {
    !c.is_empty() && c.len() <= 32 && c.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '#')
}

pub(crate) fn one_of<'a>(v: &str, allowed: &[&'a str]) -> Option<&'a str> {
    let u = v.trim().to_ascii_uppercase();
    allowed.iter().copied().find(|a| *a == u)
}

/// ORGANIZER / ATTENDEE: the modelled parameters plus every other parameter verbatim (`extra`),
/// so RSVP, DELEGATED-FROM, X-… survive a save.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Person {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub email: String,
    /// REQ-PARTICIPANT | OPT-PARTICIPANT | CHAIR | NON-PARTICIPANT
    #[serde(default)]
    pub role: String,
    /// NEEDS-ACTION | ACCEPTED | DECLINED | TENTATIVE | DELEGATED
    #[serde(default)]
    pub status: String,
    /// INDIVIDUAL | GROUP | RESOURCE | ROOM
    #[serde(default)]
    pub cutype: String,
    #[serde(default)]
    pub extra: Vec<(String, String)>,
    /// address-book entries that look like this person (`contacts::matches`); output only
    #[serde(default, skip_deserializing, skip_serializing_if = "Vec::is_empty")]
    pub contacts: Vec<crate::domain::contacts::ContactHit>,
}

fn persons_of(props: &[String], name: &str) -> Vec<Person> {
    props.iter().filter(|l| prop_name(l) == name).map(|l| {
        let (_, params, value) = ical::split_line(l);
        let mut p = Person { email: ical::strip_mailto(value.trim()).to_string(), ..Default::default() };
        for (k, v) in params {
            match k.as_str() {
                "CN" => p.name = v.trim_matches('"').to_string(),
                "ROLE" => p.role = v.to_ascii_uppercase(),
                "PARTSTAT" => p.status = v.to_ascii_uppercase(),
                "CUTYPE" => p.cutype = v.to_ascii_uppercase(),
                _ => p.extra.push((k, v)),
            }
        }
        p
    }).collect()
}

fn person_line(name: &str, p: &Person) -> Option<String> {
    let email = p.email.trim();
    if email.is_empty() {
        return None;
    }
    let mut l = name.to_string();
    if !p.name.trim().is_empty() {
        let n = p.name.trim().replace('"', "");
        if n.contains([';', ':', ',']) { l.push_str(&format!(";CN=\"{n}\"")) } else { l.push_str(&format!(";CN={n}")) }
    }
    for (k, v) in [("ROLE", &p.role), ("PARTSTAT", &p.status), ("CUTYPE", &p.cutype)] {
        if !v.trim().is_empty() {
            l.push_str(&format!(";{k}={}", v.trim().to_ascii_uppercase()));
        }
    }
    for (k, v) in &p.extra {
        l.push_str(&format!(";{k}={v}"));
    }
    Some(format!("{l}:mailto:{email}"))
}

/// EXDATE/RDATE values as local strings in the user's zone ("YYYY-MM-DD" for DATE values).
/// PERIOD-valued RDATEs are not modelled and get dropped on save.
fn dates_of(props: &[String], name: &str, tz: chrono_tz::Tz) -> Vec<String> {
    let mut out = vec![];
    for l in props.iter().filter(|l| prop_name(l) == name) {
        let (_, params, value) = ical::split_line(l);
        let param = |k: &str| params.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        let is_date = param("VALUE").is_some_and(|v| v.eq_ignore_ascii_case("DATE"));
        let src_tz = param("TZID").and_then(|t| ical::parse_tz(&t));
        for v in value.split(',') {
            let v = v.trim();
            if is_date || v.len() == 8 {
                if let Ok(d) = NaiveDate::parse_from_str(v, "%Y%m%d") {
                    out.push(d.format("%Y-%m-%d").to_string());
                }
            } else if let Ok(dt) = NaiveDateTime::parse_from_str(v.trim_end_matches('Z'), "%Y%m%dT%H%M%S") {
                let local = if v.ends_with('Z') {
                    chrono::Utc.from_utc_datetime(&dt).with_timezone(&tz).naive_local()
                } else if let Some(stz) = src_tz {
                    stz.from_local_datetime(&dt).single().map(|d| d.with_timezone(&tz).naive_local()).unwrap_or(dt)
                } else {
                    dt
                };
                out.push(local.format("%Y-%m-%dT%H:%M:%S").to_string());
            }
        }
    }
    out
}

fn date_lines(name: &str, values: &[String], all_day: bool, tz: chrono_tz::Tz) -> Vec<String> {
    let mut out = vec![];
    for v in values {
        let v = v.trim();
        if all_day || v.len() == 10 {
            if let Ok(d) = NaiveDate::parse_from_str(&v[..v.len().min(10)], "%Y-%m-%d") {
                out.push(format!("{name};VALUE=DATE:{}", ical::fmt_date(d)));
            }
        } else if let Ok(dt) = NaiveDateTime::parse_from_str(v, "%Y-%m-%dT%H:%M:%S").or_else(|_| NaiveDateTime::parse_from_str(v, "%Y-%m-%dT%H:%M")) {
            out.push(format!("{name};TZID={}:{}", tz.name(), ical::fmt_dt(dt)));
        }
    }
    out
}

/// The RRULE's UNTIL as a local date, whatever form it was written in.
fn rrule_until_local(rrule: &str, tz: chrono_tz::Tz) -> Option<NaiveDate> {
    let rrule = ical::rrule_canon(rrule);
    let v = rrule.split(';').find_map(|p| p.strip_prefix("UNTIL="))?;
    if let Ok(dt) = NaiveDateTime::parse_from_str(v.trim_end_matches('Z'), "%Y%m%dT%H%M%S") {
        return Some(if v.ends_with('Z') { chrono::Utc.from_utc_datetime(&dt).with_timezone(&tz).date_naive() } else { dt.date() });
    }
    NaiveDate::parse_from_str(&v[..v.len().min(8)], "%Y%m%d").ok()
}

/// A bare `UNTIL=YYYYMMDD` on a timed event becomes the end of that local day in UTC (RFC 5545
/// wants UNTIL in the DTSTART value type); on an all-day event a date-time UNTIL becomes a date.
fn normalize_until(rrule: &str, all_day: bool, tz: chrono_tz::Tz) -> String {
    ical::rrule_canon(rrule).split(';').map(|p| {
        let Some(v) = p.strip_prefix("UNTIL=") else { return p.to_string() };
        let date = rrule_until_local(&format!("UNTIL={v}"), tz);
        match date {
            Some(d) if all_day => format!("UNTIL={}", ical::fmt_date(d)),
            Some(d) if v.len() <= 8 => format!("UNTIL={}", ical::to_utc(d.and_hms_opt(23, 59, 59).unwrap(), tz).format("%Y%m%dT%H%M%SZ")),
            _ => p.to_string(),
        }
    }).collect::<Vec<_>>().join(";")
}

/// One VALARM as the editor sees it. `idx` is the alarm's position in the stored object: on
/// save the lines the editor does not model (UID, RELATED-TO, binary ATTACH, an EMAIL's
/// attachments, VLOCATION, X-…) are copied from that alarm; a new alarm has none.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Alarm {
    #[serde(default)]
    pub idx: Option<usize>,
    /// DISPLAY (default) | AUDIO | EMAIL
    #[serde(default)]
    pub action: String,
    /// minutes before the anchor (negative = after)
    #[serde(default)]
    pub minutes: i64,
    /// anchor is the end (TRIGGER;RELATED=END) instead of the start
    #[serde(default)]
    pub related_end: bool,
    /// absolute trigger in the user's zone (TRIGGER;VALUE=DATE-TIME); wins over `minutes`
    #[serde(default)]
    pub at: Option<NaiveDateTime>,
    /// DESCRIPTION: the text shown (DISPLAY) or the mail body (EMAIL); empty = the event's title
    #[serde(default)]
    pub description: String,
    /// SUMMARY: the mail subject (EMAIL only); empty = the event's title
    #[serde(default)]
    pub summary: String,
    /// DURATION/REPEAT pair: repeat the alarm `repeat` more times every `duration_minutes`
    #[serde(default)]
    pub repeat: i64,
    #[serde(default)]
    pub duration_minutes: i64,
    /// ATTENDEE mail addresses (EMAIL only)
    #[serde(default)]
    pub attendees: Vec<String>,
    /// ATTACH: the sound's URI (AUDIO only); binary and EMAIL attachments are preserved, not edited
    #[serde(default)]
    pub attach: String,
    /// PROXIMITY (RFC 9074): ARRIVE | DEPART | CONNECT | DISCONNECT, empty = none
    #[serde(default)]
    pub proximity: String,
    /// ACKNOWLEDGED (RFC 9074) in the user's zone: the alarm was dismissed at that time
    #[serde(default)]
    pub acknowledged: Option<NaiveDateTime>,
}

/// Preset of a reminder setting, kept per user (`practical_user_settings.alarm_presets_json`): the
/// same shape as `Alarm` without idx, text, subject and acknowledgement — those belong to one event.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct AlarmPreset {
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub minutes: i64,
    #[serde(default)]
    pub related_end: bool,
    #[serde(default)]
    pub repeat: i64,
    #[serde(default)]
    pub duration_minutes: i64,
    #[serde(default)]
    pub attendees: Vec<String>,
    #[serde(default)]
    pub attach: String,
    #[serde(default)]
    pub proximity: String,
}

pub const PROXIMITY: &[&str] = &["ARRIVE", "DEPART", "CONNECT", "DISCONNECT"];

/// Snap points of the reminder strip in minutes: before the start and before the end.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AlarmSnaps {
    pub start: Vec<i64>,
    pub end: Vec<i64>,
}

impl Default for AlarmSnaps {
    fn default() -> Self {
        Self { start: vec![2, 5, 15, 30, 60, 180, 480, 1440, 2880, 4320, 10080, 20160], end: vec![5, 10, 15, 30, 60] }
    }
}

/// The snap points from a user's setting or the built-in list.
pub fn alarm_snaps(json: Option<&str>) -> AlarmSnaps {
    json.and_then(|j| serde_json::from_str::<AlarmSnaps>(j).ok()).filter(|s| !s.start.is_empty()).unwrap_or_default()
}

/// A comma-separated list of offsets as typed in the settings ("2 min, 1 h, 1 d, 2 w"; German
/// unit words work too) → ascending, distinct minutes. Errors name the token that did not parse.
pub fn parse_offsets(text: &str) -> Result<Vec<i64>, String> {
    let mut out = vec![];
    for tok in text.split([',', ';', '\n']).map(str::trim).filter(|t| !t.is_empty()) {
        let t = tok.to_lowercase(); // the comma separates entries, so a fraction uses a dot ("1.5 h")
        let split = t.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(t.len());
        let (num, unit) = (t[..split].trim(), t[split..].trim().trim_end_matches('.'));
        let n: f64 = num.parse().map_err(|_| tok.to_string())?;
        let factor = match unit.chars().next() {
            None | Some('m') => 1.0,
            Some('h') | Some('s') => 60.0,
            Some('d') | Some('t') => 1440.0,
            Some('w') => 10080.0,
            _ => return Err(tok.to_string()),
        };
        let minutes = (n * factor).round() as i64;
        if minutes <= 0 || minutes > 366 * 1440 {
            return Err(tok.to_string());
        }
        out.push(minutes);
    }
    out.sort_unstable();
    out.dedup();
    if out.len() > 30 {
        return Err("too many".into());
    }
    Ok(out)
}

/// The inverse of `parse_offsets` for the settings form: "2 min, 1 h, 1 d, 2 w".
pub fn fmt_offsets(list: &[i64]) -> String {
    list.iter().map(|&m| if m % 10080 == 0 { format!("{} w", m / 10080) } else if m % 1440 == 0 { format!("{} d", m / 1440) } else if m % 60 == 0 { format!("{} h", m / 60) } else { format!("{m} min") }).collect::<Vec<_>>().join(", ")
}

/// Rejects a preset list that could not have come from the editor (bounds the stored JSON, too).
pub fn validate_presets(list: &[AlarmPreset]) -> AppResult<()> {
    if list.len() > 50 {
        return Err(AppError::BadRequest("too many presets".into()));
    }
    for p in list {
        if one_of(&p.action, &["DISPLAY", "AUDIO", "EMAIL"]).is_none() || (!p.proximity.is_empty() && one_of(&p.proximity, PROXIMITY).is_none()) {
            return Err(AppError::BadRequest("invalid preset".into()));
        }
        if p.minutes.abs() > 366 * 1440 || p.repeat < 0 || p.repeat > 100 || p.duration_minutes < 0 || p.attach.len() > 1000 || p.attendees.len() > 20 {
            return Err(AppError::BadRequest("invalid preset".into()));
        }
    }
    Ok(())
}

// ---- raw access to the stored object: whatever the editor does not model survives a save ----

/// The master VEVENT's lines outside its VALARMs (unfolded, in order) and its VALARM blocks.
#[derive(Debug, Default)]
pub struct RawMaster {
    pub props: Vec<String>,
    pub alarms: Vec<Vec<String>>,
}

/// Properties `build_vevent` writes itself; every other master line is carried over verbatim.
const MANAGED: &[&str] = &["UID", "DTSTAMP", "SEQUENCE", "LAST-MODIFIED", "SUMMARY", "DTSTART", "DTEND", "DURATION", "LOCATION", "DESCRIPTION", "CATEGORIES", "RRULE", "EXDATE", "RDATE", "ORGANIZER", "ATTENDEE", "STATUS", "TRANSP", "CLASS", "PRIORITY", "URL", "COLOR", "GEO", "ATTACH", "CONFERENCE", "RELATED-TO"];

/// A line `build_vevent` rewrites; binary ATTACHs are the exception and stay verbatim.
fn is_managed(line: &str) -> bool {
    let name = prop_name(line);
    if name == "ATTACH" {
        let (_, params, _) = ical::split_line(line);
        return !is_binary_attach(&params);
    }
    MANAGED.contains(&name.as_str())
}
const ALARM_MANAGED: &[&str] = &["ACTION", "TRIGGER", "DESCRIPTION", "SUMMARY", "DURATION", "REPEAT", "PROXIMITY", "ACKNOWLEDGED"];

pub fn prop_name(line: &str) -> String {
    line.split([';', ':']).next().unwrap_or("").to_ascii_uppercase()
}

/// Splits a stored object into its master VEVENT and the RECURRENCE-ID override blocks
/// (verbatim, folded again) — those come from other clients and are never rewritten here.
pub fn split_ics(ics: &str) -> (RawMaster, Vec<String>) {
    split_component(ics, "VEVENT")
}

/// The VTIMEZONE blocks of a stored object, verbatim — a carried-over property may still
/// reference one by TZID, and a dangling TZID is an invalid object.
pub fn vtimezone_blocks(ics: &str) -> Vec<String> {
    let mut out = vec![];
    let mut block: Vec<String> = vec![];
    let mut depth = 0;
    for line in unfold(&ics.replace("\r\n", "\n")) {
        if line.starts_with("BEGIN:VTIMEZONE") {
            depth = 1;
            block = vec![line];
            continue;
        }
        if depth == 0 {
            continue;
        }
        if line.starts_with("BEGIN:") {
            depth += 1;
        } else if line.starts_with("END:") {
            depth -= 1;
        }
        block.push(line);
        if depth == 0 {
            out.push(block.iter().map(|l| ical::fold(l)).collect::<Vec<_>>().join("\r\n"));
        }
    }
    out
}

/// Same for any calendar component — `VTODO` uses it to keep whatever the task editor does not model.
pub fn split_component(ics: &str, comp: &str) -> (RawMaster, Vec<String>) {
    let begin = format!("BEGIN:{comp}");
    let end = format!("END:{comp}");
    let unfolded = unfold(&ics.replace("\r\n", "\n"));
    let mut master = RawMaster::default();
    let mut overrides = vec![];
    let mut block: Vec<String> = vec![];
    let mut depth = 0;
    for line in &unfolded {
        if line.starts_with(&begin) && depth == 0 {
            depth = 1;
            block.clear();
            continue;
        }
        if depth == 0 {
            continue;
        }
        if line.starts_with("BEGIN:") {
            depth += 1;
        } else if line.starts_with("END:") {
            depth -= 1;
            if depth == 0 {
                if block.iter().any(|l| l.starts_with("RECURRENCE-ID")) {
                    let folded: Vec<String> = std::iter::once(begin.clone()).chain(block.iter().cloned()).chain(std::iter::once(end.clone())).map(|l| ical::fold(&l)).collect();
                    overrides.push(folded.join("\r\n"));
                } else if master.props.is_empty() && master.alarms.is_empty() {
                    let mut alarm: Option<Vec<String>> = None;
                    for l in &block {
                        if l.starts_with("BEGIN:VALARM") {
                            alarm = Some(vec![]);
                        } else if l.starts_with("END:VALARM") {
                            master.alarms.push(alarm.take().unwrap_or_default());
                        } else if let Some(a) = alarm.as_mut() {
                            a.push(l.clone());
                        } else {
                            master.props.push(l.clone());
                        }
                    }
                }
                continue;
            }
        }
        block.push(line.clone());
    }
    (master, overrides)
}

pub(crate) fn parse_alarm(idx: usize, lines: &[String], tz: chrono_tz::Tz) -> Alarm {
    let mut a = Alarm { idx: Some(idx), action: "DISPLAY".into(), ..Default::default() };
    let mut attach = String::new(); // the first non-binary ATTACH; it is the sound only for AUDIO (ACTION may follow it)
    for l in lines {
        let (name, params, value) = ical::split_line(l);
        let param = |k: &str| params.iter().find(|(n, _)| n == k).map(|(_, v)| v.to_ascii_uppercase());
        match name.as_str() {
            "ACTION" => a.action = value.to_ascii_uppercase(),
            "TRIGGER" => {
                if param("VALUE").as_deref() == Some("DATE-TIME") || (value.ends_with('Z') && !value.starts_with(['P', '-', '+'])) {
                    if let Ok(utc) = NaiveDateTime::parse_from_str(value.trim_end_matches('Z'), "%Y%m%dT%H%M%S") {
                        a.at = Some(chrono::Utc.from_utc_datetime(&utc).with_timezone(&tz).naive_local());
                    }
                } else if let Ok(d) = parse_duration(&value) {
                    a.minutes = -d.num_minutes();
                    a.related_end = param("RELATED").as_deref() == Some("END");
                }
            }
            "DESCRIPTION" => a.description = ical::unescape(&value),
            "SUMMARY" => a.summary = ical::unescape(&value),
            "DURATION" => a.duration_minutes = parse_duration(&value).map(|d| d.num_minutes()).unwrap_or(0),
            "REPEAT" => a.repeat = value.trim().parse().unwrap_or(0),
            "ATTENDEE" => a.attendees.push(ical::strip_mailto(value.trim()).to_string()),
            "ATTACH" if attach.is_empty() && !is_binary_attach(&params) => attach = value.trim().to_string(),
            "PROXIMITY" => a.proximity = one_of(&value, PROXIMITY).unwrap_or("").to_string(),
            "ACKNOWLEDGED" => {
                if let Ok(utc) = NaiveDateTime::parse_from_str(value.trim().trim_end_matches('Z'), "%Y%m%dT%H%M%S") {
                    a.acknowledged = Some(chrono::Utc.from_utc_datetime(&utc).with_timezone(&tz).naive_local());
                }
            }
            _ => {}
        }
    }
    if a.action == "AUDIO" {
        a.attach = attach;
    }
    a
}

/// A stored VALARM line `build_valarm` rewrites for this alarm: the managed set, ATTENDEEs of an
/// EMAIL alarm and the (non-binary) sound of an AUDIO alarm; an EMAIL's attachments stay verbatim.
fn alarm_line_managed(line: &str, action: &str) -> bool {
    let name = prop_name(line);
    match name.as_str() {
        "ATTENDEE" => action == "EMAIL",
        "ATTACH" => {
            let (_, params, _) = ical::split_line(line);
            action == "AUDIO" && !is_binary_attach(&params)
        }
        _ => ALARM_MANAGED.contains(&name.as_str()),
    }
}

fn fmt_trigger_offset(minutes: i64) -> String {
    if minutes == 0 {
        return "PT0S".into();
    }
    let sign = if minutes > 0 { "-" } else { "" };
    let m = minutes.abs();
    if m % 1440 == 0 { format!("{sign}P{}D", m / 1440) } else if m % 60 == 0 { format!("{sign}PT{}H", m / 60) } else { format!("{sign}PT{m}M") }
}

/// VALARM lines for one alarm: the modelled properties from `a`, the rest from the stored alarm.
pub(crate) fn build_valarm(a: &Alarm, summary: &str, raw: Option<&RawMaster>, tz: chrono_tz::Tz) -> Vec<String> {
    let action = match a.action.to_ascii_uppercase().as_str() { "AUDIO" => "AUDIO", "EMAIL" => "EMAIL", _ => "DISPLAY" };
    let mut l = vec!["BEGIN:VALARM".to_string(), format!("ACTION:{action}")];
    l.push(match a.at {
        Some(at) => format!("TRIGGER;VALUE=DATE-TIME:{}", ical::to_utc(at, tz).format("%Y%m%dT%H%M%SZ")),
        None if a.related_end => format!("TRIGGER;RELATED=END:{}", fmt_trigger_offset(a.minutes)),
        None => format!("TRIGGER:{}", fmt_trigger_offset(a.minutes)),
    });
    let text = if a.description.trim().is_empty() { summary } else { a.description.trim() };
    if action != "AUDIO" {
        l.push(format!("DESCRIPTION:{}", ical::escape(text)));
    }
    let src = a.idx.and_then(|i| raw.and_then(|r| r.alarms.get(i)));
    if action == "EMAIL" {
        let subject = if a.summary.trim().is_empty() { summary } else { a.summary.trim() };
        l.push(format!("SUMMARY:{}", ical::escape(subject)));
        for m in a.attendees.iter().map(|m| m.trim()).filter(|m| m.contains('@')) {
            // the stored line keeps its parameters (CN, …) while the address is unchanged
            let kept = src.and_then(|s| s.iter().find(|x| prop_name(x) == "ATTENDEE" && ical::strip_mailto(ical::split_line(x).2.trim()) == m));
            l.push(kept.cloned().unwrap_or_else(|| format!("ATTENDEE:mailto:{m}")));
        }
    }
    if action == "AUDIO" && !a.attach.trim().is_empty() {
        let uri = a.attach.trim();
        let kept = src.and_then(|s| s.iter().find(|x| prop_name(x) == "ATTACH" && ical::split_line(x).2.trim() == uri));
        l.push(kept.cloned().unwrap_or_else(|| format!("ATTACH:{uri}"))); // a kept line carries its FMTTYPE
    }
    if a.repeat > 0 && a.duration_minutes > 0 {
        l.push(format!("DURATION:{}", fmt_trigger_offset(-a.duration_minutes)));
        l.push(format!("REPEAT:{}", a.repeat));
    }
    if let Some(p) = one_of(&a.proximity, PROXIMITY) {
        l.push(format!("PROXIMITY:{p}"));
    }
    if let Some(ack) = a.acknowledged {
        l.push(format!("ACKNOWLEDGED:{}", ical::to_utc(ack, tz).format("%Y%m%dT%H%M%SZ")));
    }
    if let Some(src) = src {
        l.extend(src.iter().filter(|x| !alarm_line_managed(x, action)).cloned());
    }
    l.push("END:VALARM".into());
    l
}

pub fn validate_rrule(r: &str) -> AppResult<String> {
    let r = ical::rrule_canon(r);
    if r.is_empty() {
        return Err(AppError::BadRequest("empty RRULE".into()));
    }
    if !r.starts_with("FREQ=") || r.chars().any(|c| c == '\n' || c == '\r') {
        return Err(AppError::BadRequest("RRULE must start with FREQ=".into()));
    }
    let _: caldata::rrule::RRule<caldata::rrule::Unvalidated> = r.parse().map_err(|e| AppError::BadRequest(format!("RRULE: {e}")))?;
    Ok(r)
}

/// Builds the VEVENT lines (without BEGIN/END) for the master event. With `raw` (an update)
/// every stored line the editor does not model is carried over verbatim — properties like
/// ATTENDEE, STATUS, ATTACH, COLOR, EXDATE, X-… and the unmodelled parts of each VALARM.
fn build_vevent(uid: &str, sequence: i64, input: &EventInput, tz: chrono_tz::Tz, raw: Option<&RawMaster>) -> AppResult<Vec<String>> {
    if input.summary.trim().is_empty() {
        return Err(AppError::BadRequest("summary required".into()));
    }
    if input.end < input.start {
        return Err(AppError::BadRequest("end before start".into()));
    }
    let mut l = vec![
        format!("UID:{uid}"),
        format!("DTSTAMP:{}", ical::fmt_utc_now()),
        format!("SEQUENCE:{sequence}"),
        format!("SUMMARY:{}", ical::escape(input.summary.trim())),
    ];
    if input.all_day {
        let s = input.start.date();
        let mut e = input.end.date();
        if e <= s {
            e = s + Duration::days(1);
        }
        l.push(format!("DTSTART;VALUE=DATE:{}", ical::fmt_date(s)));
        l.push(format!("DTEND;VALUE=DATE:{}", ical::fmt_date(e)));
    } else {
        let e = if input.end <= input.start { input.start + Duration::hours(1) } else { input.end };
        l.push(format!("DTSTART;TZID={}:{}", tz.name(), ical::fmt_dt(input.start)));
        l.push(format!("DTEND;TZID={}:{}", tz.name(), ical::fmt_dt(e)));
    }
    if !input.location.trim().is_empty() {
        l.push(format!("LOCATION:{}", ical::escape(input.location.trim())));
    }
    if !input.description.trim().is_empty() {
        l.push(format!("DESCRIPTION:{}", ical::escape(input.description.trim())));
    }
    let cats: Vec<String> = input.categories.iter().map(|c| c.trim()).filter(|c| !c.is_empty()).map(ical::escape).collect();
    if !cats.is_empty() {
        l.push(format!("CATEGORIES:{}", cats.join(",")));
    }
    if let Some(r) = &input.rrule
        && !r.trim().is_empty()
    {
        l.push(format!("RRULE:{}", validate_rrule(&normalize_until(r, input.all_day, tz))?));
        l.extend(date_lines("EXDATE", &input.exdates, input.all_day, tz));
        l.extend(date_lines("RDATE", &input.rdates, input.all_day, tz));
    }
    if let Some(o) = &input.organizer
        && let Some(line) = person_line("ORGANIZER", o)
    {
        l.push(line);
    }
    l.extend(input.attendees.iter().filter_map(|a| person_line("ATTENDEE", a)));
    if let Some(v) = one_of(&input.status, &["CONFIRMED", "TENTATIVE", "CANCELLED"]) {
        l.push(format!("STATUS:{v}"));
    }
    if let Some(v) = one_of(&input.transp, &["OPAQUE", "TRANSPARENT"]) {
        l.push(format!("TRANSP:{v}"));
    }
    if let Some(v) = one_of(&input.class, &["PUBLIC", "PRIVATE", "CONFIDENTIAL"]) {
        l.push(format!("CLASS:{v}"));
    }
    if let Some(p) = input.priority.filter(|p| (0..=9).contains(p)) {
        l.push(format!("PRIORITY:{p}"));
    }
    let color = input.color.trim();
    if valid_color(color) {
        l.push(format!("COLOR:{color}"));
    }
    if let Some((lat, lon)) = input.geo.filter(|(a, b)| (-90.0..=90.0).contains(a) && (-180.0..=180.0).contains(b)) {
        l.push(format!("GEO:{lat:.6};{lon:.6}"));
    }
    l.extend(link_lines(&input.links));
    l.extend(embedded_lines(&input.links)?);
    l.extend(related_lines(&input.related));
    l.push(format!("LAST-MODIFIED:{}", ical::fmt_utc_now()));
    if let Some(r) = raw {
        // binary ATTACHs are kept only when the client sent them back with `keep` (else: removed)
        let keep: HashSet<usize> = input.links.iter().filter(|x| x.embedded && x.keep && x.data.is_none()).filter_map(|x| x.bin_index).collect();
        let mut bin = 0usize;
        // a changed DESCRIPTION makes Outlook's X-ALT-DESC (and STYLED-DESCRIPTION) stale: drop them then
        let stored = r.props.iter().find(|x| prop_name(x) == "DESCRIPTION").map(|x| ical::unescape(ical::split_line(x).2.trim())).unwrap_or_default();
        let desc_changed = stored.trim() != input.description.trim();
        for x in r.props.iter().filter(|x| !is_managed(x)) {
            let name = prop_name(x);
            if desc_changed && (name == "X-ALT-DESC" || name == "STYLED-DESCRIPTION") {
                continue;
            }
            if name == "ATTACH" && is_binary_attach(&ical::split_line(x).1) {
                let i = bin;
                bin += 1;
                if !keep.contains(&i) {
                    continue;
                }
            }
            l.push(x.clone());
        }
    }
    for a in &input.alarms {
        l.extend(build_valarm(a, input.summary.trim(), raw, tz));
    }
    Ok(l)
}

fn assemble(vtimezone: Option<String>, vevents: Vec<Vec<String>>, extra_blocks: &[String]) -> String {
    let mut out = String::new();
    out.push_str("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//xarif.de//PractiCal//DE\r\nCALSCALE:GREGORIAN\r\n");
    if let Some(v) = vtimezone {
        out.push_str(v.trim_end());
        out.push_str("\r\n");
    }
    for ev in vevents {
        out.push_str("BEGIN:VEVENT\r\n");
        for line in ev {
            out.push_str(&ical::fold(&line));
            out.push_str("\r\n");
        }
        out.push_str("END:VEVENT\r\n");
    }
    for b in extra_blocks {
        out.push_str(b.trim_end());
        out.push_str("\r\n");
    }
    out.push_str("END:VCALENDAR\r\n");
    out
}

fn user_tz(input: &EventInput, default: chrono_tz::Tz) -> chrono_tz::Tz {
    input.timezone.as_deref().and_then(ical::parse_tz).unwrap_or(default)
}

pub async fn create_event(state: &AppState, user: &Principal, input: EventInput, default_tz: chrono_tz::Tz) -> AppResult<String> {
    create_event_as(state, user, input, default_tz, None).await
}

/// `keep_uid`: a move to another calendar keeps the event's UID (as a CalDAV MOVE does), so
/// clients and the change history see the same event; a live object with it there gets a new one.
pub async fn create_event_as(state: &AppState, user: &Principal, input: EventInput, default_tz: chrono_tz::Tz, keep_uid: Option<&str>) -> AppResult<String> {
    collections::ensure_owner(user, &input.principal)?;
    let tz = user_tz(&input, default_tz);
    let keep = match keep_uid {
        Some(u) if state.calendars.get_object(&input.principal, &input.cal_id, u, false).await.is_err() => Some(u.to_string()),
        _ => None,
    };
    let uid = keep.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let lines = build_vevent(&uid, 0, &input, tz, None)?;
    let ics = assemble(if input.all_day { None } else { ical::vtimezone(tz.name()) }, vec![lines], &[]);
    let obj = CalendarObject::import(&ics, None)?;
    let object_id = uid.clone();
    // overwrite only replaces a trashed copy of a kept UID (a move there and back)
    state.calendars.put_object(&input.principal, &input.cal_id, &object_id, obj, keep.is_some()).await?;
    Ok(object_id)
}

/// Rewrites the master VEVENT from the input plus every stored line the editor does not
/// model; RECURRENCE-ID overrides written by other clients are carried over untouched
/// (raw blocks), so "edit the series" never silently drops a moved single occurrence.
pub async fn update_event(state: &AppState, user: &Principal, object_id: &str, input: EventInput, expected_etag: Option<&str>, default_tz: chrono_tz::Tz) -> AppResult<()> {
    collections::ensure_owner(user, &input.principal)?;
    let tz = user_tz(&input, default_tz);
    let existing = state.calendars.get_object(&input.principal, &input.cal_id, object_id, false).await?;
    if let Some(e) = expected_etag
        && !e.is_empty()
        && e != existing.get_etag()
    {
        return Err(AppError::Conflict("the event was changed elsewhere; reload".into()));
    }
    let CalendarInnerData::Event(main, _) = existing.get_inner().get_inner() else {
        return Err(AppError::NotFound);
    };
    let uid = main.get_uid().to_string();
    let sequence: i64 = main.get_property("SEQUENCE").and_then(|p| p.value.parse().ok()).unwrap_or(0) + 1;
    let (raw, extra) = split_ics(&existing.get_ics());
    let lines = build_vevent(&uid, sequence, &input, tz, Some(&raw))?;
    let ics = assemble(if input.all_day { None } else { ical::vtimezone(tz.name()) }, vec![lines], &extra);
    let obj = CalendarObject::import(&ics, None)?;
    state.calendars.put_object(&input.principal, &input.cal_id, object_id, obj, true).await?;
    Ok(())
}

/// Unfolded content lines, each in its canonical case: property names — and the component name of a
/// BEGIN/END line — are case-insensitive (RFC 5545 §2, RFC 6350 §3.3) and come out upper-case, so every
/// `starts_with("BEGIN:VEVENT")` / `prop_name(l) == …` downstream holds for `begin:vevent` too. Values
/// and parameters stay as written.
pub fn unfold(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for l in raw.split('\n') {
        let l = l.trim_end_matches('\r');
        if (l.starts_with(' ') || l.starts_with('\t')) && !out.is_empty() {
            out.last_mut().unwrap().push_str(&l[1..]);
        } else {
            out.push(l.to_string());
        }
    }
    for l in &mut out {
        canon_name(l);
    }
    out
}

/// Upper-cases the name of one unfolded content line (and the value of BEGIN/END) in place.
fn canon_name(l: &mut String) {
    if l.starts_with([' ', '\t']) {
        return;
    }
    let end = l.find([';', ':']).unwrap_or(l.len());
    if l[..end].bytes().any(|b| b.is_ascii_lowercase()) {
        let up = l[..end].to_ascii_uppercase();
        l.replace_range(..end, &up);
    }
    if (l.starts_with("BEGIN:") || l.starts_with("END:")) && l.bytes().any(|b| b.is_ascii_lowercase()) {
        l.make_ascii_uppercase();
    }
}

pub async fn delete_event(state: &AppState, user: &Principal, principal: &str, cal_id: &str, object_id: &str) -> AppResult<()> {
    collections::ensure_owner(user, principal)?;
    state.calendars.delete_object(principal, cal_id, object_id, true).await?;
    Ok(())
}

/// Number of instances per month of `year` (picker density) — one pass over the year per calendar,
/// kept in `DensityCache`. `instances` expands every calendar on its own, so the set's counts are the
/// sum of its calendars' and a calendar filter reuses them.
pub async fn month_density(state: &AppState, cals: &[CalInfo], year: i32, tz: chrono_tz::Tz) -> AppResult<[usize; 12]> {
    let start = NaiveDate::from_ymd_opt(year, 1, 1).unwrap();
    let end = NaiveDate::from_ymd_opt(year + 1, 1, 1).unwrap();
    let mut d = [0usize; 12];
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
mod tests {
    use super::*;
    use chrono::Datelike;

    /// D11: the real calendar's 60 recurring series, anonymised, against frozen
    /// python-dateutil expectations (first 60 instances each, tz-normalised).
    #[test]
    fn recurrence_matches_dateutil_oracle() {
        #[derive(serde::Deserialize)]
        struct Fx {
            name: String,
            ics: String,
            expected: Vec<String>,
            allday: bool,
        }
        let fx: Vec<Fx> = serde_json::from_str(include_str!("../../tests/fixtures/recurrence-real.json")).unwrap();
        assert_eq!(fx.len(), 60);
        let mut checked = 0;
        for f in &fx {
            let obj = CalendarObject::from_ics(f.ics.clone()).unwrap_or_else(|e| panic!("{}: {e}", f.name));
            let CalendarInnerData::Event(main, _) = obj.get_inner().get_inner() else { panic!("{} not an event", f.name) };
            let set = main.get_rruleset().unwrap_or_else(|| panic!("{} has no rrule set", f.name));
            let got: Vec<String> = set
                .all(60)
                .dates
                .iter()
                .map(|d| if f.allday { d.date_naive().format("%Y-%m-%d").to_string() } else { d.with_timezone(&chrono::Utc).format("%Y-%m-%dT%H:%M:%SZ").to_string() })
                .filter(|s| s[..4].parse::<i32>().unwrap() < 2100)
                .collect();
            let exp: Vec<String> = f.expected.iter().filter(|s| s[..4].parse::<i32>().unwrap() < 2100).cloned().collect();
            assert_eq!(got, exp, "series {}", f.name);
            checked += got.len();
        }
        assert!(checked > 2000, "only {checked} instances compared");
    }

    /// An all-day yearly series: caldata expands DATE + duration into a UTC DateTime end; the
    /// instance must still be exactly one day (summer instance → zone offset must not leak).
    #[test]
    fn allday_series_instance_is_one_day() {
        let tz: chrono_tz::Tz = "Europe/Berlin".parse().unwrap();
        let ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//t//EN\r\nBEGIN:VEVENT\r\nUID:bday\r\nDTSTAMP:20240901T000000Z\r\nSUMMARY:Geburtstag\r\nDTSTART;VALUE=DATE:20240911\r\nDTEND;VALUE=DATE:20240912\r\nRRULE:FREQ=YEARLY;WKST=MO\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let obj = CalendarObject::from_ics(ics.to_string()).unwrap();
        let CalendarInnerData::Event(main, overrides) = obj.get_inner().get_inner() else { panic!("not an event") };
        let cal = CalInfo {
            principal: "p".into(),
            owner_label: "p".into(),
            is_group: false,
            id: "c".into(),
            displayname: "c".into(),
            color: "#000".into(),
            description: String::new(),
            components: vec!["VEVENT".into()],
            timezone_id: "Europe/Berlin".into(),
            subscription_url: None,
            deleted_at: None,
            synctoken: 0,
        };
        let ws = ical::to_utc(NaiveDate::from_ymd_opt(2026, 9, 1).unwrap().and_hms_opt(0, 0, 0).unwrap(), tz);
        let we = ical::to_utc(NaiveDate::from_ymd_opt(2026, 9, 30).unwrap().and_hms_opt(0, 0, 0).unwrap(), tz);
        let expanded = main.expand_recurrence(Some(ws), Some(we), overrides);
        let inst: Vec<Instance> = expanded.iter().map(|ev| instance_from(ev, &cal, "bday", tz, true, vec![])).collect();
        assert_eq!(inst.len(), 1);
        let i = &inst[0];
        assert!(i.all_day);
        assert_eq!(i.start, NaiveDate::from_ymd_opt(2026, 9, 11).unwrap().and_hms_opt(0, 0, 0).unwrap());
        assert_eq!(i.end, NaiveDate::from_ymd_opt(2026, 9, 12).unwrap().and_hms_opt(0, 0, 0).unwrap());
        assert_eq!(i.last_date(), NaiveDate::from_ymd_opt(2026, 9, 11).unwrap());
        assert!(!i.multi_day());
    }

    #[test]
    fn build_and_reparse_event() {
        let tz: chrono_tz::Tz = "Europe/Berlin".parse().unwrap();
        let input = EventInput {
            principal: "p".into(),
            cal_id: "c".into(),
            summary: "Zahnarzt, Kontrolle".into(),
            location: "Praxis".into(),
            description: "Karte\nmitnehmen".into(),
            start: NaiveDate::from_ymd_opt(2026, 9, 3).unwrap().and_hms_opt(8, 30, 0).unwrap(),
            end: NaiveDate::from_ymd_opt(2026, 9, 3).unwrap().and_hms_opt(9, 15, 0).unwrap(),
            all_day: false,
            rrule: Some("FREQ=WEEKLY;COUNT=3".into()),
            alarms: vec![Alarm { minutes: 15, ..Default::default() }],
            exdates: vec![],
            rdates: vec![],
            organizer: None,
            attendees: vec![],
            categories: vec!["Arzt".into()],
            status: String::new(),
            transp: String::new(),
            class: String::new(),
            priority: None,
            color: String::new(),
            geo: None,
            links: vec![],
            related: vec![],
            timezone: None,
        };
        let lines = build_vevent("uid-1", 0, &input, tz, None).unwrap();
        let ics = assemble(ical::vtimezone("Europe/Berlin"), vec![lines], &[]);
        let obj = CalendarObject::import(&ics, None).unwrap();
        let CalendarInnerData::Event(main, overrides) = obj.get_inner().get_inner() else { panic!() };
        assert!(main.has_rruleset());
        assert_eq!(alarm_offsets(main), vec![15]);
        let ex = main.expand_recurrence(None, None, overrides);
        assert_eq!(ex.len(), 3);
        let cal = CalInfo { principal: "p".into(), owner_label: String::new(), is_group: false, id: "c".into(), displayname: "Test".into(), color: "#000".into(), description: String::new(), components: vec![], timezone_id: String::new(), subscription_url: None, deleted_at: None, synctoken: 0 };
        let i = instance_from(&ex[1], &cal, "uid-1", tz, true, alarm_offsets(main));
        assert_eq!(i.start.date().day(), 10);
        assert_eq!(i.summary, "Zahnarzt, Kontrolle");
        assert_eq!(i.description, "Karte\nmitnehmen");
        assert_eq!((i.end - i.start).num_minutes(), 45);
        assert!(validate_rrule("DAILY").is_err());
    }

    /// Property histogram of a VEVENT block (nested VALARMs prefixed), CATEGORIES merged.
    fn histogram(ics: &str) -> std::collections::BTreeMap<String, usize> {
        let (m, _) = split_ics(ics);
        let mut h = std::collections::BTreeMap::new();
        for l in &m.props {
            let n = prop_name(l);
            if n != "LAST-MODIFIED" && n != "DTSTAMP" {
                *h.entry(n).or_insert(0) += 1;
            }
        }
        for a in &m.alarms {
            for l in a {
                *h.entry(format!("VALARM:{}", prop_name(l))).or_insert(0) += 1;
            }
        }
        h.entry("CATEGORIES".into()).and_modify(|v| *v = 1);
        h
    }

    /// The demo fixture carries every VEVENT property there is (RFC 5545/7986/9073/9074 + X-);
    /// an unchanged round trip through the editor's model must keep all of them, alarm by alarm.

    #[test]
    fn changed_description_drops_stale_html_description() {
        let tz: chrono_tz::Tz = "Europe/Berlin".parse().unwrap();
        let (raw, _) = split_ics(include_str!("../../tests/fixtures/demo-event-full.ics"));
        let mut input: EventInput = serde_json::from_str(r#"{"principal":"p","cal_id":"c","summary":"s","description":"Zeilen mit Umbruch\nund Sonderzeichen, Komma; Semikolon.\nZweiter Absatz.","start":"2026-09-22T19:00:00","end":"2026-09-22T20:00:00"}"#).unwrap();
        let same = build_vevent("u", 1, &input, tz, Some(&raw)).unwrap().join("\n");
        assert!(same.contains("X-ALT-DESC") && same.contains("STYLED-DESCRIPTION"), "{same}");
        input.description = "neuer Text".into();
        let changed = build_vevent("u", 1, &input, tz, Some(&raw)).unwrap().join("\n");
        assert!(!changed.contains("X-ALT-DESC") && !changed.contains("STYLED-DESCRIPTION") && changed.contains("DESCRIPTION:neuer Text"), "{changed}");
    }

    #[test]
    fn links_related_and_embedded_attachments() {
        let tz: chrono_tz::Tz = "Europe/Berlin".parse().unwrap();
        let raw = RawMaster { props: vec![
            "ATTACH;FMTTYPE=application/pdf;FILENAME=/x.pdf;X-NC-FILE-ID=1:https://cloud.example.org/x.pdf".into(),
            "ATTACH;FMTTYPE=image/jpeg;X-FILENAME=scan.jpg;ENCODING=BASE64;VALUE=BINARY:aGVsbG8=".into(),
            "ATTACH;ENCODING=BASE64;VALUE=BINARY:d29ybGQ=".into(),
            "CONFERENCE;VALUE=URI;FEATURE=audio,phone:tel:+49691234".into(),
            "RELATED-TO:abc".into(),
            "RELATED-TO;RELTYPE=CHILD;X-FOO=1:def".into(),
        ], alarms: vec![] };
        let links = links_of(&raw.props);
        assert_eq!(links.len(), 4);
        assert_eq!((links[0].fmt.as_str(), links[0].filename.as_str(), links[0].extra.len(), links[0].embedded), ("application/pdf", "x.pdf", 1, false));
        assert_eq!((links[1].embedded, links[1].bin_index, links[1].size, links[1].filename.as_str(), links[1].keep), (true, Some(0), Some(5), "scan.jpg", true));
        assert_eq!((links[2].bin_index, links[2].size), (Some(1), Some(5)));
        assert_eq!(links[3].features, vec!["AUDIO", "PHONE"]);
        let (fmt, name, bytes) = embedded_attachment(&raw.props, 1).unwrap();
        assert_eq!((fmt.as_str(), name.as_str(), bytes.as_slice()), ("application/octet-stream", "anhang-2", b"world".as_slice()));
        assert_eq!(embedded_attachment(&raw.props, 0).unwrap().1, "scan.jpg");
        assert!(embedded_attachment(&raw.props, 2).is_none());
        let related = related_of(&raw.props);
        assert_eq!(related.len(), 2);
        assert_eq!((related[1].reltype.as_str(), related[1].extra.len()), ("CHILD", 1));
        // save: drop the first embedded file, keep the second, add a new one, keep the RELATED-TOs
        let mut input: EventInput = serde_json::from_str(r#"{"principal":"p","cal_id":"c","summary":"s","description":"","start":"2026-09-22T19:00:00","end":"2026-09-22T20:00:00"}"#).unwrap();
        input.links = links.clone();
        input.links[1].keep = false;
        input.links.push(Link { kind: "attach".into(), embedded: true, data: Some("aGk=".into()), fmt: "text/plain".into(), filename: "new file.txt".into(), ..Default::default() });
        input.related = related;
        let lines = build_vevent("u", 1, &input, tz, Some(&raw)).unwrap();
        let flat = lines.join("\n");
        assert!(!flat.contains("aGVsbG8="), "{flat}");
        assert!(flat.contains("ATTACH;ENCODING=BASE64;VALUE=BINARY:d29ybGQ="), "{flat}");
        assert!(flat.contains("ATTACH;FMTTYPE=text/plain;FILENAME=\"new file.txt\";ENCODING=BASE64;VALUE=BINARY:aGk="), "{flat}");
        assert!(flat.contains("ATTACH;FMTTYPE=application/pdf;FILENAME=x.pdf;X-NC-FILE-ID=1:https://cloud.example.org/x.pdf"), "{flat}");
        assert!(flat.contains("CONFERENCE;VALUE=URI;FEATURE=AUDIO,PHONE:tel:+49691234"), "{flat}");
        assert!(flat.contains("RELATED-TO:abc") && flat.contains("RELATED-TO;RELTYPE=CHILD;X-FOO=1:def"), "{flat}");
        input.links.push(Link { kind: "url".into(), uri: "https://a.example.org/".into(), ..Default::default() });
        input.links.push(Link { kind: "url".into(), uri: "https://b.example.org/".into(), label: "B".into(), ..Default::default() });
        let flat2 = build_vevent("u", 1, &input, tz, Some(&raw)).unwrap().join("\n");
        assert!(flat2.contains("URL:https://a.example.org/") && flat2.contains("ATTACH;LABEL=\"B\":https://b.example.org/"), "{flat2}");
        input.links.truncate(input.links.len() - 2);
        assert_eq!(flat.matches("RELATED-TO").count(), 2);
        // limits
        let big = "A".repeat(4 * (EMBED_FILE_MAX / 3 + 10));
        input.links.push(Link { kind: "attach".into(), embedded: true, data: Some(big), ..Default::default() });
        assert!(build_vevent("u", 1, &input, tz, Some(&raw)).is_err());
        input.links.pop();
        input.links.push(Link { kind: "attach".into(), embedded: true, data: Some("not base64!".into()), ..Default::default() });
        assert!(build_vevent("u", 1, &input, tz, Some(&raw)).is_err());
    }

    #[test]
    fn unchanged_save_keeps_every_property() {
        let tz: chrono_tz::Tz = "Europe/Berlin".parse().unwrap();
        let ics = include_str!("../../tests/fixtures/demo-event-full.ics");
        let (raw, overrides) = split_ics(ics);
        assert_eq!(raw.alarms.len(), 5);
        assert_eq!(overrides.len(), 1);
        let alarms: Vec<Alarm> = raw.alarms.iter().enumerate().map(|(n, a)| parse_alarm(n, a, tz)).collect();
        assert_eq!(alarms[0].minutes, 15);
        assert_eq!((alarms[1].minutes, alarms[1].repeat, alarms[1].duration_minutes), (1440, 2, 10));
        assert!(alarms[2].related_end && alarms[2].action == "AUDIO");
        assert!(alarms[3].at.is_some() && alarms[3].action == "EMAIL");
        let input = EventInput {
            principal: "p".into(),
            cal_id: "c".into(),
            summary: "Demo-Termin: alle Eigenschaften".into(),
            location: "Kulturpalast, Schloßstraße 2, 01067 Dresden".into(),
            description: "Zeilen mit Umbruch\nund Sonderzeichen, Komma; Semikolon.\nZweiter Absatz.".into(),
            start: NaiveDate::from_ymd_opt(2026, 9, 22).unwrap().and_hms_opt(19, 0, 0).unwrap(),
            end: NaiveDate::from_ymd_opt(2026, 9, 22).unwrap().and_hms_opt(21, 30, 0).unwrap(),
            all_day: false,
            rrule: Some("FREQ=WEEKLY;BYDAY=TU;COUNT=6".into()),
            alarms,
            exdates: dates_of(&raw.props, "EXDATE", tz),
            rdates: dates_of(&raw.props, "RDATE", tz),
            organizer: persons_of(&raw.props, "ORGANIZER").into_iter().next(),
            attendees: persons_of(&raw.props, "ATTENDEE"),
            categories: vec!["Familie".into(), "Kultur".into(), "Demo".into()],
            status: "CONFIRMED".into(),
            transp: "OPAQUE".into(),
            class: "PRIVATE".into(),
            priority: Some(5),
            color: "tomato".into(),
            geo: Some((51.050407, 13.737262)),
            links: links_of(&raw.props),
            related: related_of(&raw.props),
            timezone: Some("Europe/Berlin".into()),
        };
        assert_eq!(input.links.iter().map(|l| (l.kind.as_str(), l.embedded)).collect::<Vec<_>>(), vec![("url", false), ("conference", false), ("attach", false), ("attach", true)]);
        assert_eq!(input.links[1].features, vec!["AUDIO", "VIDEO"]);
        assert_eq!((input.related.len(), input.related[0].reltype.as_str(), input.related[0].uid.as_str()), (1, "PARENT", "parent-event@xarif.de"));
        assert_eq!(input.links[1].label, "Videocall");
        assert_eq!(input.exdates, vec!["2026-10-06T19:00:00"]);
        assert_eq!(input.rdates, vec!["2026-10-10T19:00:00"]);
        assert_eq!(input.attendees.len(), 3);
        assert_eq!(input.attendees[0].extra, vec![("RSVP".to_string(), "TRUE".to_string())]);
        assert_eq!(input.organizer.as_ref().map(|o| o.name.as_str()), Some("Thomas Steinbach"));
        let lines = build_vevent("demo-event-full@xarif.de", 4, &input, tz, Some(&raw)).unwrap();
        let out = assemble(ical::vtimezone("Europe/Berlin"), vec![lines], &overrides);
        let (before, after) = (histogram(ics), histogram(&out));
        let lost: Vec<String> = before.iter().filter(|(k, v)| after.get(*k).copied().unwrap_or(0) < **v).map(|(k, v)| format!("{k} {v}->{}", after.get(k).copied().unwrap_or(0))).collect();
        assert!(lost.is_empty(), "lost on save: {lost:?}");
        // and the result is still a valid object with the same series, override and alarms
        let obj = CalendarObject::import(&out, None).unwrap();
        let CalendarInnerData::Event(main, overrides) = obj.get_inner().get_inner() else { panic!() };
        assert_eq!(overrides.len(), 1);
        assert_eq!(main.get_alarms().len(), 5);
        assert_eq!(alarm_offsets(main), vec![15, 1440, 5 - 150, 60]);
        let flat = unfold(&out.replace("\r\n", "\n")).join("\n"); // long lines are folded in `out`
        assert!(flat.contains("ACKNOWLEDGED:20260915T170000Z") && flat.contains("PROXIMITY:DEPART"), "{flat}");
        assert!(flat.contains("ATTENDEE;CN=Erika Musterfrau;ROLE=REQ-PARTICIPANT;PARTSTAT=ACCEPTED;CUTYPE=INDIVIDUAL;RSVP=TRUE:mailto:erika@example.org"), "{flat}");
        assert!(flat.contains("EXDATE;TZID=Europe/Berlin:20261006T190000") && flat.contains("RDATE;TZID=Europe/Berlin:20261010T190000"), "{flat}");
        assert!(flat.contains("CONFERENCE;VALUE=URI;FEATURE=AUDIO,VIDEO;LABEL=\"Videocall\":https://meet.example.org/demo"), "{flat}");
        assert!(flat.contains("ATTACH;FMTTYPE=text/plain;ENCODING=BASE64;VALUE=BINARY:SGFsbG8gRGVtbyE=") && flat.contains("GEO:51.050407;13.737262") && flat.contains("COLOR:tomato"), "{flat}");
        assert_eq!(flat.matches("\nURL:").count(), 1);
        assert_eq!(main.expand_recurrence(None, None, overrides).len(), 6, "5 rule instances after the exclusion + the RDATE");
    }

    #[test]
    fn alarm_fields_round_trip() {
        let tz: chrono_tz::Tz = "Europe/Berlin".parse().unwrap();
        let ics = include_str!("../../tests/fixtures/demo-event-full.ics");
        let (raw, _) = split_ics(ics);
        let mut alarms: Vec<Alarm> = raw.alarms.iter().enumerate().map(|(n, a)| parse_alarm(n, a, tz)).collect();
        // the fixture's fields land in the model
        assert_eq!(alarms[2].attach, "https://example.org/ping.au");
        assert_eq!((alarms[3].summary.as_str(), alarms[3].attendees.as_slice()), ("Erinnerung: Demo-Termin", &["thomas@example.org".to_string()][..]));
        assert_eq!(alarms[4].proximity, "DEPART");
        assert_eq!(alarms[1].acknowledged, NaiveDate::from_ymd_opt(2026, 9, 15).unwrap().and_hms_opt(19, 0, 0)); // 17:00Z in Berlin
        // edited: a second recipient and a new subject, a new sound, a proximity change, an acknowledgement now
        alarms[3].attendees.push("anna@example.org".into());
        alarms[3].summary = "Neuer Betreff".into();
        alarms[2].attach = "https://example.org/pong.au".into();
        alarms[4].proximity = "ARRIVE".into();
        alarms[0].acknowledged = NaiveDate::from_ymd_opt(2026, 9, 17).unwrap().and_hms_opt(14, 0, 0);
        let out: Vec<String> = alarms.iter().flat_map(|a| build_valarm(a, "Titel", Some(&raw), tz)).collect();
        let flat = out.join("\n");
        assert!(flat.contains("ATTENDEE:mailto:thomas@example.org\nATTENDEE:mailto:anna@example.org"), "{flat}");
        assert!(flat.contains("SUMMARY:Neuer Betreff") && flat.contains("DESCRIPTION:Morgen Abend im Kulturpalast."), "{flat}");
        assert!(flat.contains("ATTACH:https://example.org/pong.au") && !flat.contains("ping.au"), "{flat}");
        assert!(flat.contains("PROXIMITY:ARRIVE") && !flat.contains("DEPART"), "{flat}");
        assert!(flat.contains("ACKNOWLEDGED:20260917T120000Z") && flat.contains("ACKNOWLEDGED:20260915T170000Z"), "{flat}");
        assert_eq!(flat.matches("UID:demo-alarm-").count(), 2, "alarm UIDs ride along: {flat}");
        // unchanged: the sound keeps its FMTTYPE, the edited alarm still parses to the same model
        let kept: Vec<String> = build_valarm(&parse_alarm(2, &raw.alarms[2], tz), "Titel", Some(&raw), tz);
        assert!(kept.iter().any(|l| l == "ATTACH;FMTTYPE=audio/basic:https://example.org/ping.au"), "{kept:?}");
        let again = parse_alarm(0, &out[out.iter().position(|l| l == "BEGIN:VALARM").unwrap() + 1..out.iter().position(|l| l == "END:VALARM").unwrap()], tz);
        assert_eq!((again.minutes, again.acknowledged), (15, alarms[0].acknowledged));
        assert!(validate_presets(&[AlarmPreset { action: "EMAIL".into(), minutes: 30, ..Default::default() }]).is_ok());
        assert!(validate_presets(&[AlarmPreset { action: "PROCEDURE".into(), ..Default::default() }]).is_err());
    }

    #[test]
    fn snap_point_lists() {
        assert_eq!(parse_offsets("2 min, 1 h, 1 Std., 1 Tag, 2 Tage, 1 Wo, 30, 1.5h"), Ok(vec![2, 30, 60, 90, 1440, 2880, 10080]));
        assert_eq!(parse_offsets(""), Ok(vec![]));
        assert_eq!(parse_offsets("5 min, x"), Err("x".into()));
        assert_eq!(parse_offsets("0 min"), Err("0 min".into()));
        assert_eq!(fmt_offsets(&[2, 90, 60, 1440, 20160]), "2 min, 90 min, 1 h, 1 d, 2 w");
        assert_eq!(alarm_snaps(None), AlarmSnaps::default());
        assert_eq!(alarm_snaps(Some("{\"start\":[],\"end\":[]}")), AlarmSnaps::default(), "an empty start list means the default");
        assert_eq!(alarm_snaps(Some("{\"start\":[10],\"end\":[]}")).start, vec![10]);
    }

    #[test]
    fn until_forms() {
        let tz: chrono_tz::Tz = "Europe/Berlin".parse().unwrap();
        assert_eq!(normalize_until("FREQ=DAILY;UNTIL=20260930", false, tz), "FREQ=DAILY;UNTIL=20260930T215959Z");
        assert_eq!(normalize_until("FREQ=DAILY;UNTIL=20260930T215959Z", true, tz), "FREQ=DAILY;UNTIL=20260930");
        assert_eq!(rrule_until_local("FREQ=DAILY;UNTIL=20260930T215959Z", tz), NaiveDate::from_ymd_opt(2026, 9, 30));
    }
}
