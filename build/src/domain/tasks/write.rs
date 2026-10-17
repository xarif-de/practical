//! Writing: `TaskInput` → VTODO lines (`build_vtodo`, unmodelled lines carried over), create / update /
//! complete / delete through the store.

use super::*;

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
pub(super) const MANAGED: &[&str] = &[
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
pub(super) fn is_managed(line: &str) -> bool {
    let name = events::prop_name(line);
    if name == "ATTACH" {
        return !events::is_binary_attach(&ical::split_line(line).1);
    }
    MANAGED.contains(&name.as_str())
}

pub(super) fn stamp_line(name: &str, dt: NaiveDateTime, all_day: bool, tz: chrono_tz::Tz) -> String {
    if all_day { format!("{name};VALUE=DATE:{}", ical::fmt_date(dt.date())) } else { format!("{name};TZID={}:{}", tz.name(), ical::fmt_dt(dt)) }
}

pub(super) fn build_vtodo(uid: &str, sequence: i64, i: &TaskInput, tz: chrono_tz::Tz, raw: Option<&events::RawMaster>) -> AppResult<Vec<String>> {
    if i.summary.trim().is_empty() {
        return Err(AppError::bad("err-summary-required"));
    }
    if let (Some(s), Some(d)) = (i.start, i.due)
        && d < s
    {
        return Err(AppError::bad("err-due-before-start"));
    }
    if [i.start, i.due, i.completed_at].iter().flatten().any(|d| !ical::write_date_ok(d.date())) {
        return Err(AppError::bad("err-date-range"));
    }
    events::validate_common(&i.summary, &i.location, &i.description, &i.categories, &i.alarms, &i.links, &i.related)?;
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

/// The line references a time zone (a TZID parameter, its name in any case): the object needs that VTIMEZONE.
fn has_tzid(l: &str) -> bool {
    ical::split_line(l).1.iter().any(|(k, _)| k == "TZID")
}

pub async fn create_task(state: &AppState, user: &Principal, mut i: TaskInput, tz: chrono_tz::Tz) -> AppResult<String> {
    collections::ensure_owner(user, &i.principal)?;
    collections::ensure_writable(state, &i.principal, &i.cal_id).await?;
    normalize(&mut i, crate::clock::now().with_timezone(&tz).naive_local());
    let uid = uuid::Uuid::new_v4().to_string();
    let lines = build_vtodo(&uid, 0, &i, tz, None)?;
    let vtz = lines.iter().any(|l| has_tzid(l)).then(|| ical::vtimezone(tz.name())).flatten();
    let obj = CalendarObject::import(&ical::assemble(vtz, "VTODO", lines, &[]), None)?;
    state.calendars.put_object(&i.principal, &i.cal_id, &uid, obj, false).await?;
    Ok(uid)
}

/// Rewrites the task from the input. Everything this editor does not model — recurrence,
/// attendees, foreign X-properties — is carried over from the stored object.
pub async fn update_task(state: &AppState, user: &Principal, object_id: &str, mut i: TaskInput, expected_etag: Option<&str>, tz: chrono_tz::Tz) -> AppResult<()> {
    collections::ensure_owner(user, &i.principal)?;
    collections::ensure_writable(state, &i.principal, &i.cal_id).await?;
    let existing = state.calendars.get_object(&i.principal, &i.cal_id, object_id, false).await?;
    if let Some(e) = expected_etag
        && !e.is_empty()
        && e != existing.get_etag()
    {
        return Err(AppError::conflict("err-changed-task"));
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
    let vtz = if !lines.iter().any(|l| has_tzid(l)) {
        None
    } else {
        let blocks = events::vtimezone_blocks(&stored);
        if blocks.is_empty() { ical::vtimezone(tz.name()) } else { Some(blocks.join("\r\n")) }
    };
    let obj = CalendarObject::import(&ical::assemble(vtz, "VTODO", lines, &extra), None)?;
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
