//! Tasks (VTODO). The list rows carry everything the views draw; `TaskDetail` carries
//! everything the editor shows and is deliberately shaped like `events::EventDetail`, so the
//! shared editor cards can be fed from either. Reading and writing reuse the event helpers
//! (`events::links_of`, `parse_alarm`, `build_valarm`, …) — one implementation, two components.
//!
//! What this module does NOT model (recurrence, attendees, X-properties) survives a save
//! verbatim; see `MANAGED` and `update_task`.
//!
//! - The model is a minimal VTODO: [`Task`] (list row), [`TaskDetail`] (editor), [`TaskCache`] (parsed tasks
//!   per calendar and synctoken).
//! - Status shown: a task without STATUS is open, or done when it carries a COMPLETED stamp
//!   ([`Task::completed`]); a save writes no STATUS the user did not pick.

use crate::domain::collections::{self, CalInfo};
use crate::domain::events::{self, Alarm, Link, Related};
use crate::domain::ical;
use crate::error::{AppError, AppResult};
use crate::state::{AppState, Need};
use caldata::component::{CalendarInnerData, Component};
use chrono::{NaiveDate, NaiveDateTime};
use rustical_ical::CalendarObject;
use rustical_store::auth::Principal;

mod write;
mod reminders;
pub use write::*;
pub use reminders::*;

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
    /// Never empty: a task without STATUS is listed as open or, with a COMPLETED stamp, as done (`shown_status`)
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
    /// as stored: another client's value too (`events::class_value`), so a save and the tick box keep it
    pub class: String,
    /// the strictest CLASS of the RECURRENCE-ID overrides (`events::overrides_class`)
    pub class_overrides: String,
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
    /// its calendar takes no PractiCal writes (`collections::is_read_only`): the editor stays in view mode
    pub read_only: bool,
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

/// The status a list row is shown, filed, filtered and counted by. A VTODO without STATUS is open
/// (NEEDS-ACTION), or done when it carries a COMPLETED stamp (the older shape `Task::completed` accepts).
/// Only the list rows get it: `TaskDetail` keeps the stored value, so a save never adds a STATUS the user
/// did not pick (`build_vtodo` writes none for "").
fn shown_status(stored: String, stamped: bool) -> String {
    match stored.as_str() {
        "" if stamped => "COMPLETED".into(),
        "" => "NEEDS-ACTION".into(),
        _ => stored,
    }
}

/// The list row's CLASS (`events::shown_class`): an unknown value counts as PRIVATE, as for sharees
fn class_of(props: &[String]) -> String {
    events::shown_class(first(props, "CLASS").map(|_| text_of(props, "CLASS")).as_deref())
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
    let completed_at = completed_of(p, tz);
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
        status: shown_status(status_of(p), completed_at.is_some()),
        percent: num_of(p, "PERCENT-COMPLETE"),
        completed_at,
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
pub struct TaskCache(std::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<CalTasks>>>);

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
    // the calendar, its view and its path (`state::Authorized::cache_key`)
    let k = cal.auth.cache_key();
    let cached = state.task_cache.0.lock().unwrap().get(&k).cloned().filter(|c| c.tz == tz);
    if let Some(c) = &cached
        && c.token == cal.synctoken
    {
        return Ok(c.clone());
    }
    let fresh = match cached {
        Some(c) => {
            let (changed, deleted, token) = state.collections().sync_changes(&cal.auth, c.token).await?;
            let (mut tasks, mut seeds) = (c.tasks.clone(), c.seeds.clone());
            for id in deleted {
                tasks.remove(&id);
                seeds.remove(&id);
            }
            for (id, obj) in changed {
                tasks.remove(&id);
                seeds.remove(&id);
                if let Some((t, seed)) = events::per_object(&format!("{}/{}/{id}", cal.principal, cal.id), || task_of(&cal.principal, &cal.id, id.clone(), &obj, tz)).flatten() {
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
            let token = state.collections().calendar_token(&cal.auth).await?;
            let objects = state.collections().objects(&cal.auth).await?;
            let (mut tasks, mut seeds) = (std::collections::HashMap::new(), std::collections::HashMap::new());
            for (id, obj) in objects {
                if let Some((t, seed)) = events::per_object(&format!("{}/{}/{id}", cal.principal, cal.id), || task_of(&cal.principal, &cal.id, id.clone(), &obj, tz)).flatten() {
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
    let auth = collections::ensure_calendar(state, user, principal, cal_id, Need::Read).await?;
    let obj = state.collections().object(&auth, object_id).await?;
    let CalendarInnerData::Todo(t, _) = obj.get_inner().get_inner() else {
        return Err(AppError::NotFound);
    };
    let (raw, extra) = events::split_component(&obj.get_ics(), "VTODO");
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
        class: events::stored_class(first(p, "CLASS").map(|_| text_of(p, "CLASS")).as_deref()),
        class_overrides: events::overrides_class(&extra),
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
        // through a share also: read access, or less than full visibility
        read_only: collections::is_read_only(state, auth.owner(), auth.id()).await? || !auth.allows(Need::Write) || auth.level(&obj) < crate::state::FULL_LEVEL,
    })
}

#[cfg(test)]
mod pinned;

#[cfg(test)]
mod tests;

