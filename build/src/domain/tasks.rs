//! Tasks (VTODO). The list rows carry everything the views draw; `TaskDetail` carries
//! everything the editor shows and is deliberately shaped like `events::EventDetail`, so the
//! shared editor cards can be fed from either. Reading and writing reuse the event helpers
//! (`events::links_of`, `parse_alarm`, `build_valarm`, …) — one implementation, two components.
//!
//! What this module does NOT model (recurrence, attendees, X-properties) survives a save
//! verbatim; see `MANAGED` and `update_task`.

use crate::domain::collections::{self, CalInfo};
use crate::domain::events::{self, Alarm, Link, Related};
use crate::domain::ical;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use caldata::component::{CalendarInnerData, Component};
use chrono::{NaiveDate, NaiveDateTime};
use rustical_ical::CalendarObject;
use rustical_store::auth::Principal;
use rustical_store::{CalendarReadStore, CalendarWriteStore};

/// RFC 5545 §3.8.1.11 for VTODO. An empty string means the property is absent.
pub const STATUSES: &[&str] = &["NEEDS-ACTION", "IN-PROCESS", "COMPLETED", "CANCELLED"];

#[derive(Debug, Clone, serde::Serialize)]
pub struct Task {
    pub principal: String,
    pub cal_id: String,
    pub cal_name: String,
    /// The task's own COLOR when it has one, else the calendar's
    pub color: String,
    pub object_id: String,
    pub uid: String,
    pub summary: String,
    pub description: String,
    pub location: String,
    pub start: Option<NaiveDateTime>,
    pub start_all_day: bool,
    pub due: Option<NaiveDateTime>,
    pub due_all_day: bool,
    /// "" when the task carries no STATUS at all
    pub status: String,
    /// PERCENT-COMPLETE; `None` when absent
    pub percent: Option<i64>,
    /// COMPLETED, in the user's zone
    pub completed_at: Option<NaiveDateTime>,
    pub priority: u8,
    pub class: String,
    pub categories: Vec<String>,
    pub alarms: usize,
    pub links: usize,
    pub created: Option<NaiveDateTime>,
    /// what only the search needs (domain::search); not sent to the page
    #[serde(skip)]
    pub extra: TaskExtra,
}

#[derive(Debug, Clone, Default)]
pub struct TaskExtra {
    pub links: Vec<events::Link>,
    pub geo: Option<(f64, f64)>,
    /// RELATED-TO UIDs
    pub related: Vec<String>,
}

impl Task {
    /// True for STATUS:COMPLETED and for the older shape that only carries a COMPLETED stamp.
    pub fn completed(&self) -> bool {
        self.status == "COMPLETED" || (self.status.is_empty() && self.completed_at.is_some())
    }
    pub fn open(&self) -> bool {
        !self.completed() && self.status != "CANCELLED"
    }
    pub fn due_label(&self) -> String {
        match self.due {
            Some(d) if self.due_all_day => d.format("%d.%m.%Y").to_string(),
            Some(d) => d.format("%d.%m.%Y %H:%M").to_string(),
            None => String::new(),
        }
    }
    pub fn overdue(&self, today: NaiveDate) -> bool {
        self.open() && self.due.is_some_and(|d| d.date() < today)
    }
    pub fn key(&self) -> String {
        format!("{}/{}/{}", self.principal, self.cal_id, self.object_id)
    }
}

/// Everything the editor shows for one task.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TaskDetail {
    pub principal: String,
    pub cal_id: String,
    pub object_id: String,
    pub uid: String,
    pub summary: String,
    pub description: String,
    pub location: String,
    pub start: Option<NaiveDateTime>,
    pub start_all_day: bool,
    pub due: Option<NaiveDateTime>,
    pub due_all_day: bool,
    pub status: String,
    pub percent: Option<i64>,
    pub completed_at: Option<NaiveDateTime>,
    pub class: String,
    pub priority: Option<i64>,
    pub categories: Vec<String>,
    /// RFC 7986 COLOR (CSS colour name or #rrggbb)
    pub color: String,
    pub geo: Option<(f64, f64)>,
    pub alarms: Vec<Alarm>,
    pub links: Vec<Link>,
    pub related: Vec<Related>,
    pub etag: String,
    pub timezone: String,
    /// The task carries a recurrence rule this editor does not model — shown as a hint, kept on save
    pub has_rrule: bool,
}

fn first<'a>(props: &'a [String], name: &str) -> Option<&'a String> {
    props.iter().find(|l| events::prop_name(l) == name)
}

/// A property's unescaped TEXT value.
fn text_of(props: &[String], name: &str) -> String {
    first(props, name).map(|l| ical::unescape(ical::split_line(l).2.trim())).unwrap_or_default()
}

fn num_of(props: &[String], name: &str) -> Option<i64> {
    first(props, name).and_then(|l| ical::split_line(l).2.trim().parse().ok())
}

/// COMPLETED is a UTC date-time by definition (RFC 5545 §3.8.2.1) — read it as such.
fn completed_of(props: &[String], tz: chrono_tz::Tz) -> Option<NaiveDateTime> {
    let v = first(props, "COMPLETED")?;
    let raw = ical::split_line(v).2.trim().trim_end_matches('Z').to_string();
    let naive = NaiveDateTime::parse_from_str(&raw, "%Y%m%dT%H%M%S").ok()?;
    Some(chrono::TimeZone::from_utc_datetime(&chrono::Utc, &naive).with_timezone(&tz).naive_local())
}

fn status_of(props: &[String]) -> String {
    let v = text_of(props, "STATUS").to_ascii_uppercase();
    if STATUSES.contains(&v.as_str()) { v } else { String::new() }
}

fn class_of(props: &[String]) -> String {
    let v = text_of(props, "CLASS").to_ascii_uppercase();
    if ["PUBLIC", "PRIVATE", "CONFIDENTIAL"].contains(&v.as_str()) { v } else { String::new() }
}

fn geo_of(props: &[String]) -> Option<(f64, f64)> {
    let v = ical::split_line(first(props, "GEO")?).2;
    let (a, b) = v.split_once(';')?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

fn cats_of(props: &[String]) -> Vec<String> {
    props.iter().filter(|l| events::prop_name(l) == "CATEGORIES").flat_map(|l| ical::split_list(&ical::split_line(l).2)).collect()
}

/// Local wall-clock of a DTSTART/DUE line, plus whether it is a plain date.
fn stamp_of(props: &[String], name: &str, tz: chrono_tz::Tz) -> (Option<NaiveDateTime>, bool) {
    let Some(line) = first(props, name) else { return (None, false) };
    let (_, params, value) = ical::split_line(line);
    let value = value.trim().to_string();
    let param = |k: &str| params.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    if param("VALUE") == Some("DATE") || value.len() == 8 {
        return (NaiveDate::parse_from_str(&value, "%Y%m%d").ok().map(|d| d.and_hms_opt(0, 0, 0).unwrap()), true);
    }
    if let Some(utc) = value.strip_suffix('Z') {
        let naive = NaiveDateTime::parse_from_str(utc, "%Y%m%dT%H%M%S").ok();
        return (naive.map(|n| chrono::TimeZone::from_utc_datetime(&chrono::Utc, &n).with_timezone(&tz).naive_local()), false);
    }
    // TZID or floating: a stored wall-clock is converted from its own zone into the user's
    let naive = NaiveDateTime::parse_from_str(&value, "%Y%m%dT%H%M%S").ok();
    let from = param("TZID").and_then(ical::parse_tz);
    match (naive, from) {
        (Some(n), Some(z)) if z != tz => (Some(ical::to_utc(n, z).with_timezone(&tz).naive_local()), false),
        (n, _) => (n, false),
    }
}

/// One VTODO object as a list row, plus its reminders (`Seed`). The calendar's name and colour are
/// filled in by `tasks_for` (they change without the synctoken moving); `color` here is the task's
/// own COLOR, maybe empty.
fn task_of(principal: &str, cal_id: &str, id: String, obj: &CalendarObject, tz: chrono_tz::Tz) -> Option<(Task, Option<Seed>)> {
    let CalendarInnerData::Todo(t, _) = obj.get_inner().get_inner() else { return None };
    let (raw, _) = events::split_component(&obj.get_ics(), "VTODO");
    let seed = seed_of(&raw, tz);
    let p = &raw.props;
    let (start, start_all_day) = stamp_of(p, "DTSTART", tz);
    let (due, due_all_day) = stamp_of(p, "DUE", tz);
    let task = Task {
        principal: principal.to_string(),
        cal_id: cal_id.to_string(),
        cal_name: String::new(),
        color: text_of(p, "COLOR"),
        uid: obj.get_uid().to_string(),
        object_id: id,
        summary: ical::text(t, "SUMMARY").unwrap_or_else(|| "(ohne Titel)".into()),
        description: text_of(p, "DESCRIPTION"),
        location: text_of(p, "LOCATION"),
        start,
        start_all_day,
        due,
        due_all_day,
        status: status_of(p),
        percent: num_of(p, "PERCENT-COMPLETE"),
        completed_at: completed_of(p, tz),
        priority: num_of(p, "PRIORITY").unwrap_or(0).clamp(0, 9) as u8,
        class: class_of(p),
        categories: cats_of(p),
        alarms: raw.alarms.len(),
        links: events::links_of(p).len(),
        created: stamp_of(p, "CREATED", tz).0,
        extra: TaskExtra { links: events::links_of(p), geo: geo_of(p), related: events::related_of(p).into_iter().map(|r| r.uid).collect() },
    };
    Some((task, seed))
}

/// The parsed tasks of each calendar at a synctoken, so a page load does not parse every object
/// again: a task calendar usually holds the events too (dev copy: 3 811 objects for 946 tasks,
/// ≈ 0.9 s per /tasks in a debug build). A moved token is caught up through the store's change
/// log (`sync_changes`), so an edit re-parses only what changed.
#[derive(Default)]
pub struct TaskCache(std::sync::Mutex<std::collections::HashMap<(String, String), std::sync::Arc<CalTasks>>>);

#[cfg(test)]
impl TaskCache {
    pub fn clear(&self) {
        self.0.lock().unwrap().clear();
    }
}

pub struct CalTasks {
    token: i64,
    tz: chrono_tz::Tz,
    /// object id → task
    tasks: std::collections::HashMap<String, Task>,
    /// object id → its reminders, for the open tasks that have any (`reminders`, the push loop)
    seeds: std::collections::HashMap<String, Seed>,
}

async fn cal_tasks(state: &AppState, cal: &CalInfo, tz: chrono_tz::Tz) -> AppResult<std::sync::Arc<CalTasks>> {
    let k = (cal.principal.clone(), cal.id.clone());
    let cached = state.task_cache.0.lock().unwrap().get(&k).cloned().filter(|c| c.tz == tz);
    if let Some(c) = &cached
        && c.token == cal.synctoken
    {
        return Ok(c.clone());
    }
    let fresh = match cached {
        Some(c) => {
            let (changed, deleted, token) = state.calendars.sync_changes(&cal.principal, &cal.id, c.token).await?;
            let (mut tasks, mut seeds) = (c.tasks.clone(), c.seeds.clone());
            for id in deleted {
                tasks.remove(&id);
                seeds.remove(&id);
            }
            for (id, obj) in changed {
                tasks.remove(&id);
                seeds.remove(&id);
                if let Some((t, seed)) = task_of(&cal.principal, &cal.id, id.clone(), &obj, tz) {
                    if let Some(seed) = seed {
                        seeds.insert(id.clone(), seed);
                    }
                    tasks.insert(id, t);
                }
            }
            CalTasks { token, tz, tasks, seeds }
        }
        None => {
            // the token first: a write racing the read is then caught up next time, never lost
            let token = state.calendars.get_calendar(&cal.principal, &cal.id, false).await?.synctoken;
            let objects = state.calendars.get_objects(&cal.principal, &cal.id).await?;
            let (mut tasks, mut seeds) = (std::collections::HashMap::new(), std::collections::HashMap::new());
            for (id, obj) in objects {
                if let Some((t, seed)) = task_of(&cal.principal, &cal.id, id.clone(), &obj, tz) {
                    if let Some(seed) = seed {
                        seeds.insert(id.clone(), seed);
                    }
                    tasks.insert(id, t);
                }
            }
            CalTasks { token, tz, tasks, seeds }
        }
    };
    let fresh = std::sync::Arc::new(fresh);
    let mut m = state.task_cache.0.lock().unwrap();
    if m.get(&k).is_none_or(|c| c.tz != tz || c.token <= fresh.token) {
        m.insert(k, fresh.clone());
    }
    Ok(fresh)
}

pub async fn tasks_for(state: &AppState, cals: &[CalInfo], tz: chrono_tz::Tz) -> AppResult<Vec<Task>> {
    let mut out = vec![];
    for cal in cals {
        if !cal.components.iter().any(|c| c == "VTODO") {
            continue;
        }
        for t in cal_tasks(state, cal, tz).await?.tasks.values() {
            let mut t = t.clone();
            t.cal_name = cal.displayname.clone();
            if !events::valid_color(&t.color) {
                t.color = cal.color.clone();
            }
            out.push(t);
        }
    }
    out.sort_by(|a, b| a.completed().cmp(&b.completed()).then(a.due.is_none().cmp(&b.due.is_none())).then(a.due.cmp(&b.due)).then(a.summary.cmp(&b.summary)).then(a.object_id.cmp(&b.object_id)));
    Ok(out)
}

/// One task with everything the editor needs. Fails for a calendar the user does not own.
pub async fn get_task(state: &AppState, user: &Principal, principal: &str, cal_id: &str, object_id: &str, tz: chrono_tz::Tz) -> AppResult<TaskDetail> {
    collections::ensure_owner(user, principal)?;
    let obj = state.calendars.get_object(principal, cal_id, object_id, false).await?;
    let CalendarInnerData::Todo(t, _) = obj.get_inner().get_inner() else {
        return Err(AppError::NotFound);
    };
    let (raw, _) = events::split_component(&obj.get_ics(), "VTODO");
    let p = &raw.props;
    let (start, start_all_day) = stamp_of(p, "DTSTART", tz);
    let (due, due_all_day) = stamp_of(p, "DUE", tz);
    Ok(TaskDetail {
        principal: principal.into(),
        cal_id: cal_id.into(),
        object_id: object_id.into(),
        uid: t.get_uid().to_string(),
        summary: text_of(p, "SUMMARY"),
        description: text_of(p, "DESCRIPTION"),
        location: text_of(p, "LOCATION"),
        start,
        start_all_day,
        due,
        due_all_day,
        status: status_of(p),
        percent: num_of(p, "PERCENT-COMPLETE"),
        completed_at: completed_of(p, tz),
        class: class_of(p),
        priority: num_of(p, "PRIORITY").filter(|v| (1..=9).contains(v)),
        categories: cats_of(p),
        color: text_of(p, "COLOR"),
        geo: geo_of(p),
        alarms: raw.alarms.iter().enumerate().map(|(n, a)| events::parse_alarm(n, a, tz)).collect(),
        links: events::links_of(p),
        related: events::related_of(p),
        etag: obj.get_etag().to_string(),
        timezone: tz.name().to_string(),
        has_rrule: first(p, "RRULE").is_some(),
    })
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct TaskInput {
    pub principal: String,
    pub cal_id: String,
    pub summary: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub location: String,
    #[serde(default, deserialize_with = "date_or_datetime")]
    pub start: Option<NaiveDateTime>,
    #[serde(default)]
    pub start_all_day: bool,
    #[serde(default, deserialize_with = "date_or_datetime")]
    pub due: Option<NaiveDateTime>,
    /// A due date without a time — the common case, and what every older client wrote
    #[serde(default = "yes")]
    pub due_all_day: bool,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub percent: Option<i64>,
    #[serde(default, deserialize_with = "date_or_datetime")]
    pub completed_at: Option<NaiveDateTime>,
    #[serde(default)]
    pub priority: Option<i64>,
    #[serde(default)]
    pub class: String,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub geo: Option<(f64, f64)>,
    #[serde(default)]
    pub alarms: Vec<Alarm>,
    #[serde(default)]
    pub links: Vec<Link>,
    #[serde(default)]
    pub related: Vec<Related>,
}
fn yes() -> bool {
    true
}

/// `2026-09-25` or `2026-09-25T17:30` — the date form is what a plain date input sends and what
/// every task without a due *time* carries; accepting both keeps one field for both shapes.
fn date_or_datetime<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<NaiveDateTime>, D::Error> {
    let Some(s): Option<String> = serde::Deserialize::deserialize(d)? else { return Ok(None) };
    let s = s.trim();
    if s.is_empty() {
        return Ok(None);
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return Ok(Some(dt));
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M") {
        return Ok(Some(dt));
    }
    NaiveDate::parse_from_str(s, "%Y-%m-%d").map(|d| Some(d.and_hms_opt(0, 0, 0).unwrap())).map_err(serde::de::Error::custom)
}

/// The three progress fields are independent properties in RFC 5545 — nothing in the format ties
/// them together, so the invariant is ours to hold (the editor mirrors it client-side):
/// COMPLETED ⇔ percent 100 ⇔ a completion stamp, and progress above 0 means "in process".
pub fn normalize(i: &mut TaskInput, now: NaiveDateTime) {
    if !STATUSES.contains(&i.status.as_str()) {
        i.status = String::new();
    }
    if i.status == "COMPLETED" {
        i.percent = Some(100);
        i.completed_at.get_or_insert(now);
    } else {
        i.completed_at = None;
        if i.percent == Some(100) {
            i.percent = Some(0);
        }
        if i.percent.is_some_and(|p| p > 0) && (i.status == "NEEDS-ACTION" || i.status.is_empty()) {
            i.status = "IN-PROCESS".into();
        }
        i.percent = i.percent.filter(|p| *p > 0);
    }
    i.percent = i.percent.map(|p| p.clamp(0, 100));
    if i.due.is_none() {
        i.due_all_day = true;
    }
}

/// Properties `build_vtodo` writes itself; every other stored line is carried over verbatim,
/// so a save from this UI never drops what another client wrote (RRULE, ATTENDEE, X-*, …).
/// Mirrors `events::MANAGED`.
const MANAGED: &[&str] = &[
    "UID",
    "DTSTAMP",
    "SEQUENCE",
    "LAST-MODIFIED",
    "SUMMARY",
    "DESCRIPTION",
    "LOCATION",
    "DTSTART",
    "DUE",
    "DURATION",
    "STATUS",
    "COMPLETED",
    "PERCENT-COMPLETE",
    "PRIORITY",
    "CLASS",
    "CATEGORIES",
    "COLOR",
    "GEO",
    "URL",
    "CONFERENCE",
    "RELATED-TO",
];

/// A line `build_vtodo` rewrites; binary ATTACHs are the exception and stay verbatim.
fn is_managed(line: &str) -> bool {
    let name = events::prop_name(line);
    if name == "ATTACH" {
        return !events::is_binary_attach(&ical::split_line(line).1);
    }
    MANAGED.contains(&name.as_str())
}

fn stamp_line(name: &str, dt: NaiveDateTime, all_day: bool, tz: chrono_tz::Tz) -> String {
    if all_day { format!("{name};VALUE=DATE:{}", ical::fmt_date(dt.date())) } else { format!("{name};TZID={}:{}", tz.name(), ical::fmt_dt(dt)) }
}

fn build_vtodo(uid: &str, sequence: i64, i: &TaskInput, tz: chrono_tz::Tz, raw: Option<&events::RawMaster>) -> AppResult<Vec<String>> {
    if i.summary.trim().is_empty() {
        return Err(AppError::BadRequest("summary required".into()));
    }
    if let (Some(s), Some(d)) = (i.start, i.due)
        && d < s
    {
        return Err(AppError::BadRequest("due before start".into()));
    }
    let mut l = vec![
        format!("UID:{uid}"),
        format!("DTSTAMP:{}", ical::fmt_utc_now()),
        format!("SEQUENCE:{sequence}"),
        format!("SUMMARY:{}", ical::escape(i.summary.trim())),
    ];
    if let Some(s) = i.start {
        l.push(stamp_line("DTSTART", s, i.start_all_day, tz));
    }
    if let Some(d) = i.due {
        l.push(stamp_line("DUE", d, i.due_all_day, tz));
    }
    if !i.description.trim().is_empty() {
        l.push(format!("DESCRIPTION:{}", ical::escape(i.description.trim())));
    }
    if !i.location.trim().is_empty() {
        l.push(format!("LOCATION:{}", ical::escape(i.location.trim())));
    }
    let cats: Vec<String> = i.categories.iter().map(|c| c.trim()).filter(|c| !c.is_empty()).map(ical::escape).collect();
    if !cats.is_empty() {
        l.push(format!("CATEGORIES:{}", cats.join(",")));
    }
    if let Some(v) = events::one_of(&i.status, STATUSES) {
        l.push(format!("STATUS:{v}"));
    }
    if let Some(p) = i.percent {
        l.push(format!("PERCENT-COMPLETE:{p}"));
    }
    if let Some(c) = i.completed_at {
        l.push(format!("COMPLETED:{}", ical::to_utc(c, tz).format("%Y%m%dT%H%M%SZ")));
    }
    if let Some(v) = events::one_of(&i.class, &["PUBLIC", "PRIVATE", "CONFIDENTIAL"]) {
        l.push(format!("CLASS:{v}"));
    }
    if let Some(p) = i.priority.filter(|p| (1..=9).contains(p)) {
        l.push(format!("PRIORITY:{p}"));
    }
    let color = i.color.trim();
    if events::valid_color(color) {
        l.push(format!("COLOR:{color}"));
    }
    if let Some((lat, lon)) = i.geo.filter(|(a, b)| (-90.0..=90.0).contains(a) && (-180.0..=180.0).contains(b)) {
        l.push(format!("GEO:{lat:.6};{lon:.6}"));
    }
    l.extend(events::link_lines(&i.links));
    l.extend(events::embedded_lines(&i.links)?);
    l.extend(events::related_lines(&i.related));
    l.push(format!("LAST-MODIFIED:{}", ical::fmt_utc_now()));
    if let Some(r) = raw {
        // binary ATTACHs are kept only when the client sent them back with `keep` (else: removed)
        let keep: std::collections::HashSet<usize> = i.links.iter().filter(|x| x.embedded && x.keep && x.data.is_none()).filter_map(|x| x.bin_index).collect();
        let mut bin = 0usize;
        for x in r.props.iter().filter(|x| !is_managed(x)) {
            if events::prop_name(x) == "ATTACH" {
                let n = bin;
                bin += 1;
                if !keep.contains(&n) {
                    continue;
                }
            }
            l.push(x.clone());
        }
    }
    for a in &i.alarms {
        l.extend(events::build_valarm(a, i.summary.trim(), raw, tz));
    }
    Ok(l)
}

fn assemble(vtimezone: Option<String>, lines: Vec<String>, extra_blocks: &[String]) -> String {
    let mut out = String::from("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//xarif.de//PractiCal//DE\r\nCALSCALE:GREGORIAN\r\n");
    if let Some(v) = vtimezone {
        out.push_str(v.trim_end());
        out.push_str("\r\n");
    }
    out.push_str("BEGIN:VTODO\r\n");
    for line in lines {
        out.push_str(&ical::fold(&line));
        out.push_str("\r\n");
    }
    out.push_str("END:VTODO\r\n");
    for b in extra_blocks {
        out.push_str(b.trim_end());
        out.push_str("\r\n");
    }
    out.push_str("END:VCALENDAR\r\n");
    out
}

pub async fn create_task(state: &AppState, user: &Principal, mut i: TaskInput, tz: chrono_tz::Tz) -> AppResult<String> {
    collections::ensure_owner(user, &i.principal)?;
    normalize(&mut i, crate::clock::now().with_timezone(&tz).naive_local());
    let uid = uuid::Uuid::new_v4().to_string();
    let lines = build_vtodo(&uid, 0, &i, tz, None)?;
    let vtz = lines.iter().any(|l| l.contains("TZID=")).then(|| ical::vtimezone(tz.name())).flatten();
    let obj = CalendarObject::import(&assemble(vtz, lines, &[]), None)?;
    state.calendars.put_object(&i.principal, &i.cal_id, &uid, obj, false).await?;
    Ok(uid)
}

/// Rewrites the task from the input. Everything this editor does not model — recurrence,
/// attendees, foreign X-properties — is carried over from the stored object.
pub async fn update_task(state: &AppState, user: &Principal, object_id: &str, mut i: TaskInput, expected_etag: Option<&str>, tz: chrono_tz::Tz) -> AppResult<()> {
    collections::ensure_owner(user, &i.principal)?;
    let existing = state.calendars.get_object(&i.principal, &i.cal_id, object_id, false).await?;
    if let Some(e) = expected_etag
        && !e.is_empty()
        && e != existing.get_etag()
    {
        return Err(AppError::Conflict("the task was changed elsewhere; reload".into()));
    }
    let CalendarInnerData::Todo(t, _) = existing.get_inner().get_inner() else {
        return Err(AppError::NotFound);
    };
    normalize(&mut i, crate::clock::now().with_timezone(&tz).naive_local());
    let uid = t.get_uid().to_string();
    let stored = existing.get_ics();
    let sequence = t.get_property("SEQUENCE").and_then(|p| p.value.parse::<i64>().ok()).unwrap_or(0) + 1;
    let (raw, extra) = events::split_component(&stored, "VTODO");
    let lines = build_vtodo(&uid, sequence, &i, tz, Some(&raw))?;
    // a carried-over TZID must keep its definition; prefer the stored blocks over a generated one
    let vtz = if !lines.iter().any(|l| l.contains("TZID=")) {
        None
    } else {
        let blocks = events::vtimezone_blocks(&stored);
        if blocks.is_empty() { ical::vtimezone(tz.name()) } else { Some(blocks.join("\r\n")) }
    };
    let obj = CalendarObject::import(&assemble(vtz, lines, &extra), None)?;
    state.calendars.put_object(&i.principal, &i.cal_id, object_id, obj, true).await?;
    Ok(())
}

/// Ticking a task off (or on again) without touching anything else.
pub async fn set_completed(state: &AppState, user: &Principal, principal: &str, cal_id: &str, object_id: &str, done: bool, tz: chrono_tz::Tz) -> AppResult<()> {
    let d = get_task(state, user, principal, cal_id, object_id, tz).await?;
    let etag = d.etag.clone();
    let i = TaskInput {
        principal: d.principal,
        cal_id: d.cal_id,
        summary: d.summary,
        description: d.description,
        location: d.location,
        start: d.start,
        start_all_day: d.start_all_day,
        due: d.due,
        due_all_day: d.due_all_day,
        status: if done { "COMPLETED".into() } else { "NEEDS-ACTION".into() },
        percent: if done { Some(100) } else { Some(0) },
        completed_at: if done { d.completed_at } else { None },
        priority: d.priority,
        class: d.class,
        categories: d.categories,
        color: d.color,
        geo: d.geo,
        alarms: d.alarms,
        links: d.links,
        related: d.related,
    };
    update_task(state, user, object_id, i, Some(&etag), tz).await
}

pub async fn delete_task(state: &AppState, user: &Principal, principal: &str, cal_id: &str, object_id: &str) -> AppResult<()> {
    collections::ensure_owner(user, principal)?;
    state.calendars.delete_object(principal, cal_id, object_id, true).await?;
    Ok(())
}

#[cfg(test)]
mod pinned;

#[cfg(test)]
mod tests {
    use super::*;

    const STORED: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VTODO\r\nUID:u1\r\nDTSTAMP:20260901T100000Z\r\nCREATED:20260801T080000Z\r\nSEQUENCE:4\r\nSUMMARY:Alt\r\nDESCRIPTION:Beschreibung\r\nDUE;VALUE=DATE:20260918\r\nCATEGORIES:Arbeit,Familie\r\nLOCATION:Stadtbücherei\r\nURL:https://example.org/t\r\nCLASS:PRIVATE\r\nRELATED-TO;RELTYPE=PARENT:parent-uid\r\nX-APPLE-SORT-ORDER:12\r\nRRULE:FREQ=WEEKLY\r\nATTENDEE;CN=Max:mailto:max@example.org\r\nSTATUS:NEEDS-ACTION\r\nBEGIN:VALARM\r\nACTION:DISPLAY\r\nTRIGGER:-PT2H\r\nDESCRIPTION:Erinnerung\r\nEND:VALARM\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";

    fn input() -> TaskInput {
        TaskInput { principal: "p".into(), cal_id: "c".into(), summary: "Neu".into(), due_all_day: true, ..Default::default() }
    }
    fn tz() -> chrono_tz::Tz {
        chrono_tz::Europe::Berlin
    }

    /// The regression this guards: a save used to rebuild the VTODO from six fields,
    /// so every property another client had written was gone after one click.
    #[test]
    fn save_keeps_every_unmanaged_property() {
        let (raw, extra) = events::split_component(STORED, "VTODO");
        assert!(extra.is_empty());
        let mut i = input();
        // alarms are modelled, so they travel through the input — as they do when the editor saves
        i.alarms = raw.alarms.iter().enumerate().map(|(n, a)| events::parse_alarm(n, a, tz())).collect();
        let out = build_vtodo("u1", 5, &i, tz(), Some(&raw)).unwrap().join("\n");
        for line in ["CREATED:20260801T080000Z", "X-APPLE-SORT-ORDER:12", "RRULE:FREQ=WEEKLY", "ATTENDEE;CN=Max:mailto:max@example.org"] {
            assert!(out.contains(line), "lost: {line}\n{out}");
        }
        assert!(out.contains("TRIGGER:-PT2H"), "alarm lost\n{out}");
        // managed properties come from the input, not from the stored copy
        assert!(out.contains("SUMMARY:Neu") && !out.contains("SUMMARY:Alt"));
        assert!(!out.contains("CATEGORIES") && !out.contains("LOCATION") && !out.contains("CLASS:PRIVATE"));
        assert_eq!(out.matches("SEQUENCE:5").count(), 1);
        assert_eq!(out.matches("LAST-MODIFIED:").count(), 1);
    }

    #[test]
    fn without_raw_nothing_extra_is_written() {
        let out = build_vtodo("u1", 0, &input(), tz(), None).unwrap().join("\n");
        assert!(!out.contains("CATEGORIES") && !out.contains("VALARM"));
        assert!(out.contains("SUMMARY:Neu"));
    }

    #[test]
    fn full_input_round_trips_through_the_reader() {
        let mut i = input();
        i.summary = "Steuer".into();
        i.description = "Belege".into();
        i.location = "Büro".into();
        i.start = Some(NaiveDate::from_ymd_opt(2026, 9, 21).unwrap().and_hms_opt(9, 0, 0).unwrap());
        i.start_all_day = false;
        i.due = Some(NaiveDate::from_ymd_opt(2026, 9, 24).unwrap().and_hms_opt(17, 30, 0).unwrap());
        i.due_all_day = false;
        i.status = "IN-PROCESS".into();
        i.percent = Some(60);
        i.priority = Some(2);
        i.class = "PRIVATE".into();
        i.categories = vec!["Arbeit".into(), "Steuer".into()];
        i.color = "tomato".into();
        i.geo = Some((49.45, 11.08));
        normalize(&mut i, NaiveDate::from_ymd_opt(2026, 9, 22).unwrap().and_hms_opt(12, 0, 0).unwrap());
        let p = build_vtodo("u1", 1, &i, tz(), None).unwrap();
        assert_eq!(stamp_of(&p, "DUE", tz()), (i.due, false));
        assert_eq!(stamp_of(&p, "DTSTART", tz()), (i.start, false));
        assert_eq!(status_of(&p), "IN-PROCESS");
        assert_eq!(num_of(&p, "PERCENT-COMPLETE"), Some(60));
        assert_eq!(num_of(&p, "PRIORITY"), Some(2));
        assert_eq!(class_of(&p), "PRIVATE");
        assert_eq!(cats_of(&p), vec!["Arbeit".to_string(), "Steuer".into()]);
        assert_eq!(text_of(&p, "COLOR"), "tomato");
        assert_eq!(geo_of(&p), Some((49.45, 11.08)));
        assert_eq!(text_of(&p, "LOCATION"), "Büro");
    }

    /// The consistency rule the format does not give us.
    #[test]
    fn progress_fields_stay_consistent() {
        let now = NaiveDate::from_ymd_opt(2026, 9, 22).unwrap().and_hms_opt(12, 0, 0).unwrap();
        let mut done = input();
        done.status = "COMPLETED".into();
        normalize(&mut done, now);
        assert_eq!((done.percent, done.completed_at), (Some(100), Some(now)));

        let mut reopened = input();
        reopened.status = "NEEDS-ACTION".into();
        reopened.percent = Some(100);
        reopened.completed_at = Some(now);
        normalize(&mut reopened, now);
        assert_eq!((reopened.percent, reopened.completed_at), (None, None));

        // progress on an "open" task means it is being worked on
        let mut started = input();
        started.status = "NEEDS-ACTION".into();
        started.percent = Some(40);
        normalize(&mut started, now);
        assert_eq!((started.status.as_str(), started.percent), ("IN-PROCESS", Some(40)));

        // a bogus status never reaches the file
        let mut junk = input();
        junk.status = "DONE-ISH".into();
        normalize(&mut junk, now);
        assert_eq!(junk.status, "");
    }

    #[test]
    fn completed_stamp_is_read_as_utc() {
        let p = vec!["COMPLETED:20260920T143200Z".to_string()];
        let local = completed_of(&p, tz()).unwrap();
        assert_eq!(local.format("%Y-%m-%d %H:%M").to_string(), "2026-09-20 16:32"); // CEST = UTC+2
    }

    fn cal() -> CalInfo {
        CalInfo {
            principal: "p".into(),
            owner_label: String::new(),
            is_group: false,
            id: "c".into(),
            displayname: "Persönlicher Kalender".into(),
            color: "#3478f6".into(),
            description: String::new(),
            components: vec!["VTODO".into()],
            timezone_id: String::new(),
            subscription_url: None,
            deleted_at: None,
            synctoken: 0,
        }
    }
    fn with(extra: &str) -> String {
        format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VTODO\r\nUID:u1\r\nDTSTAMP:20260901T100000Z\r\nSUMMARY:Zahnarzt anrufen\r\nDUE;TZID=Europe/Berlin:20260924T170000\r\n{extra}BEGIN:VALARM\r\nACTION:DISPLAY\r\nTRIGGER;RELATED=END:-PT2H\r\nDESCRIPTION:x\r\nEND:VALARM\r\nEND:VTODO\r\nEND:VCALENDAR\r\n")
    }

    /// Reminders on tasks were stored and shown but never fired; this is the path that fires them.
    #[test]
    fn only_an_open_task_in_the_window_reminds() {
        let win = (NaiveDate::from_ymd_opt(2026, 9, 21).unwrap(), NaiveDate::from_ymd_opt(2026, 9, 26).unwrap());
        let r = reminders_of(&with(""), &cal(), "o1", tz(), win.0, win.1);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].at.format("%Y-%m-%d %H:%M").to_string(), "2026-09-24 15:00");
        assert_eq!((r[0].summary.as_str(), r[0].object_id.as_str(), r[0].cal_name.as_str()), ("Zahnarzt anrufen", "o1", "Persönlicher Kalender"));

        // finished or cancelled: nothing to remind about
        assert!(reminders_of(&with("STATUS:COMPLETED\r\n"), &cal(), "o1", tz(), win.0, win.1).is_empty());
        assert!(reminders_of(&with("COMPLETED:20260923T080000Z\r\n"), &cal(), "o1", tz(), win.0, win.1).is_empty());
        assert!(reminders_of(&with("STATUS:CANCELLED\r\n"), &cal(), "o1", tz(), win.0, win.1).is_empty());
        // outside the window
        assert!(reminders_of(&with(""), &cal(), "o1", tz(), NaiveDate::from_ymd_opt(2026, 9, 25).unwrap(), win.1).is_empty());
        // a task without any alarm contributes nothing at all
        let bare = "BEGIN:VCALENDAR\r\nBEGIN:VTODO\r\nUID:u2\r\nSUMMARY:x\r\nDUE;VALUE=DATE:20260924\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";
        assert!(reminders_of(bare, &cal(), "o2", tz(), win.0, win.1).is_empty());
    }

    /// The anchor rule for a component that has no duration.
    #[test]
    fn a_reminder_anchors_on_what_the_task_has() {
        let d = |h: u32| NaiveDate::from_ymd_opt(2026, 9, 24).unwrap().and_hms_opt(h, 0, 0).unwrap();
        let al = |minutes: i64, related_end: bool| Alarm { minutes, related_end, ..Default::default() };

        // both dates: each side counts from its own anchor
        assert_eq!(reminder_at(&al(120, false), Some(d(9)), Some(d(17))), Some(d(7)));
        assert_eq!(reminder_at(&al(120, true), Some(d(9)), Some(d(17))), Some(d(15)));
        // only a due date: both sides fall back to it
        assert_eq!(reminder_at(&al(60, false), None, Some(d(17))), Some(d(16)));
        assert_eq!(reminder_at(&al(60, true), None, Some(d(17))), Some(d(16)));
        // only a start: likewise
        assert_eq!(reminder_at(&al(60, true), Some(d(9)), None), Some(d(8)));
        // a negative offset is "after"
        assert_eq!(reminder_at(&al(-60, true), None, Some(d(17))), Some(d(18)));
        // no date at all: nothing to count from
        assert_eq!(reminder_at(&al(60, false), None, None), None);
        // an absolute trigger wins over every anchor
        let abs = Alarm { at: Some(d(6)), minutes: 999, ..Default::default() };
        assert_eq!(reminder_at(&abs, Some(d(9)), Some(d(17))), Some(d(6)));
    }

    /// A DUE with a foreign TZID must arrive as the right wall-clock in the user's zone.
    #[test]
    fn due_with_a_foreign_tzid_is_converted() {
        let p = vec!["DUE;TZID=America/New_York:20260924T090000".to_string()];
        let (dt, all_day) = stamp_of(&p, "DUE", tz());
        assert!(!all_day);
        assert_eq!(dt.unwrap().format("%Y-%m-%d %H:%M").to_string(), "2026-09-24 15:00");
    }
}

/// When a task's reminder fires. A task has no duration, so the two anchors RFC 5545 offers
/// collapse: RELATED=END counts from DUE, RELATED=START from DTSTART, and each falls back to
/// the other when only one of the two is set. Without any date the reminder can never fire —
/// there is nothing to count from — unless it carries an absolute TRIGGER.
pub fn reminder_at(a: &Alarm, start: Option<NaiveDateTime>, due: Option<NaiveDateTime>) -> Option<NaiveDateTime> {
    if let Some(abs) = a.at {
        return Some(abs);
    }
    let anchor = if a.related_end { due.or(start) } else { start.or(due) }?;
    Some(anchor - chrono::Duration::minutes(a.minutes))
}

/// A reminder a task carries, resolved to the moment it fires (wall clock in the user's zone).
#[derive(Debug, Clone)]
pub struct TaskReminder {
    pub principal: String,
    pub cal_id: String,
    pub cal_name: String,
    pub object_id: String,
    pub summary: String,
    pub at: NaiveDateTime,
    pub due: Option<NaiveDateTime>,
    pub due_all_day: bool,
}

impl TaskReminder {
    pub fn key(&self) -> String {
        format!("{}/{}/{}", self.principal, self.cal_id, self.object_id)
    }
}

/// What one stored task's reminders need, read once with the task (`cal_tasks`): when each fires
/// (wall clock in the cache's zone, in VALARM order) and what the notification says. Finished and
/// cancelled tasks have none — a reminder for something already done is noise, and every client
/// stops showing it too. `None` = nothing will ever fire.
#[derive(Debug, Clone)]
pub struct Seed {
    summary: String,
    ats: Vec<NaiveDateTime>,
    due: Option<NaiveDateTime>,
    due_all_day: bool,
}

fn seed_of(raw: &events::RawMaster, tz: chrono_tz::Tz) -> Option<Seed> {
    if raw.alarms.is_empty() {
        return None;
    }
    let p = &raw.props;
    let status = status_of(p);
    let done = status == "COMPLETED" || (status.is_empty() && completed_of(p, tz).is_some());
    if done || status == "CANCELLED" {
        return None;
    }
    let (start, _) = stamp_of(p, "DTSTART", tz);
    let (due, due_all_day) = stamp_of(p, "DUE", tz);
    let ats: Vec<NaiveDateTime> = raw.alarms.iter().enumerate().filter_map(|(n, lines)| reminder_at(&events::parse_alarm(n, lines, tz), start, due)).collect();
    (!ats.is_empty()).then(|| Seed { summary: text_of(p, "SUMMARY"), ats, due, due_all_day })
}

/// A task's reminders that fire between `from` and `to` (dates, inclusive).
fn window(seed: &Seed, cal: &CalInfo, object_id: &str, from: NaiveDate, to: NaiveDate) -> impl Iterator<Item = TaskReminder> {
    seed.ats.iter().filter(move |at| at.date() >= from && at.date() <= to).map(move |&at| TaskReminder {
        principal: cal.principal.clone(),
        cal_id: cal.id.clone(),
        cal_name: cal.displayname.clone(),
        object_id: object_id.to_string(),
        summary: seed.summary.clone(),
        at,
        due: seed.due,
        due_all_day: seed.due_all_day,
    })
}

/// The reminders one stored task contributes inside the window.
#[cfg(test)]
fn reminders_of(ics: &str, cal: &CalInfo, object_id: &str, tz: chrono_tz::Tz, from: NaiveDate, to: NaiveDate) -> Vec<TaskReminder> {
    let (raw, _) = events::split_component(ics, "VTODO");
    seed_of(&raw, tz).map(|s| window(&s, cal, object_id, from, to).collect()).unwrap_or_default()
}

/// Reminders of the *open* tasks that fire between `from` and `to`; see `reminder_at`. Read from the
/// task cache, so a minute's push round re-parses nothing that did not change.
pub async fn reminders(state: &AppState, cals: &[CalInfo], tz: chrono_tz::Tz, from: NaiveDate, to: NaiveDate) -> AppResult<Vec<TaskReminder>> {
    let mut out = vec![];
    for cal in cals {
        if !cal.components.iter().any(|c| c == "VTODO") {
            continue;
        }
        let ct = cal_tasks(state, cal, tz).await?;
        // object id order: the cache is a map, the store's order was the table's
        let mut ids: Vec<&String> = ct.seeds.keys().collect();
        ids.sort();
        for id in ids {
            out.extend(window(&ct.seeds[id], cal, id, from, to));
        }
    }
    Ok(out)
}
