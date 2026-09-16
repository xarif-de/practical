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
        color: cal.color.clone(),
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

/// All instances touching [start, end) in the user's zone, sorted by start.
pub async fn instances(state: &AppState, cals: &[CalInfo], start: NaiveDate, end: NaiveDate, tz: chrono_tz::Tz) -> AppResult<Vec<Instance>> {
    let ws = ical::to_utc(start.and_hms_opt(0, 0, 0).unwrap(), tz) - Duration::days(1);
    let we = ical::to_utc(end.and_hms_opt(0, 0, 0).unwrap(), tz) + Duration::days(1);
    let win_start = start.and_hms_opt(0, 0, 0).unwrap();
    let win_end = end.and_hms_opt(0, 0, 0).unwrap();
    let mut out = vec![];
    for cal in cals {
        if !cal.components.iter().any(|c| c == "VEVENT") {
            continue;
        }
        let objs = state
            .calendars
            .calendar_query(&cal.principal, &cal.id, CalendarQuery { time_start: Some(start), time_end: Some(end) })
            .await?;
        for (id, obj) in objs {
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
    }
    out.sort_by(|a, b| a.start.cmp(&b.start).then(b.all_day.cmp(&a.all_day)).then(a.summary.cmp(&b.summary)));
    Ok(out)
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
    pub alarms: Vec<Alarm>,
    pub categories: Vec<String>,
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
    let rrule = main.get_property("RRULE").map(|p| p.value.clone());
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
        rrule,
        alarms: split_ics(&obj.get_ics()).0.alarms.iter().enumerate().map(|(n, a)| parse_alarm(n, a, tz)).collect(),
        categories: i.categories,
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
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub timezone: Option<String>,
}

/// One VALARM as the editor sees it. `idx` is the alarm's position in the stored object: on
/// save the lines the editor does not model (UID, ACKNOWLEDGED, PROXIMITY, ATTACH, ATTENDEE,
/// X-…) are copied from that alarm; a new alarm has none.
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
    #[serde(default)]
    pub description: String,
    /// DURATION/REPEAT pair: repeat the alarm `repeat` more times every `duration_minutes`
    #[serde(default)]
    pub repeat: i64,
    #[serde(default)]
    pub duration_minutes: i64,
}

// ---- raw access to the stored object: whatever the editor does not model survives a save ----

/// The master VEVENT's lines outside its VALARMs (unfolded, in order) and its VALARM blocks.
#[derive(Debug, Default)]
pub struct RawMaster {
    pub props: Vec<String>,
    pub alarms: Vec<Vec<String>>,
}

/// Properties `build_vevent` writes itself; every other master line is carried over verbatim.
const MANAGED: &[&str] = &["UID", "DTSTAMP", "SEQUENCE", "LAST-MODIFIED", "SUMMARY", "DTSTART", "DTEND", "DURATION", "LOCATION", "DESCRIPTION", "CATEGORIES", "RRULE"];
const ALARM_MANAGED: &[&str] = &["ACTION", "TRIGGER", "DESCRIPTION", "SUMMARY", "DURATION", "REPEAT"];

fn prop_name(line: &str) -> String {
    line.split([';', ':']).next().unwrap_or("").to_ascii_uppercase()
}

/// Splits a stored object into its master VEVENT and the RECURRENCE-ID override blocks
/// (verbatim, folded again) — those come from other clients and are never rewritten here.
pub fn split_ics(ics: &str) -> (RawMaster, Vec<String>) {
    let unfolded = unfold(&ics.replace("\r\n", "\n"));
    let mut master = RawMaster::default();
    let mut overrides = vec![];
    let mut block: Vec<String> = vec![];
    let mut depth = 0;
    for line in &unfolded {
        if line.starts_with("BEGIN:VEVENT") && depth == 0 {
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
                    let folded: Vec<String> = std::iter::once("BEGIN:VEVENT".to_string()).chain(block.iter().cloned()).chain(std::iter::once("END:VEVENT".to_string())).map(|l| ical::fold(&l)).collect();
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

fn parse_alarm(idx: usize, lines: &[String], tz: chrono_tz::Tz) -> Alarm {
    let mut a = Alarm { idx: Some(idx), action: "DISPLAY".into(), ..Default::default() };
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
            "SUMMARY" if a.description.is_empty() => a.description = ical::unescape(&value),
            "DURATION" => a.duration_minutes = parse_duration(&value).map(|d| d.num_minutes()).unwrap_or(0),
            "REPEAT" => a.repeat = value.trim().parse().unwrap_or(0),
            _ => {}
        }
    }
    a
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
fn build_valarm(a: &Alarm, summary: &str, raw: Option<&RawMaster>, tz: chrono_tz::Tz) -> Vec<String> {
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
    if action == "EMAIL" {
        l.push(format!("SUMMARY:{}", ical::escape(text)));
    }
    if a.repeat > 0 && a.duration_minutes > 0 {
        l.push(format!("DURATION:{}", fmt_trigger_offset(-a.duration_minutes)));
        l.push(format!("REPEAT:{}", a.repeat));
    }
    if let Some(src) = a.idx.and_then(|i| raw.and_then(|r| r.alarms.get(i))) {
        l.extend(src.iter().filter(|x| !ALARM_MANAGED.contains(&prop_name(x).as_str())).cloned());
    }
    l.push("END:VALARM".into());
    l
}

fn validate_rrule(r: &str) -> AppResult<String> {
    let r = r.trim().trim_start_matches("RRULE:").to_string();
    if r.is_empty() {
        return Err(AppError::BadRequest("empty RRULE".into()));
    }
    if !r.to_uppercase().starts_with("FREQ=") || r.chars().any(|c| c == '\n' || c == '\r') {
        return Err(AppError::BadRequest("RRULE must start with FREQ=".into()));
    }
    let _: caldata::rrule::RRule<caldata::rrule::Unvalidated> = r.parse().map_err(|e| AppError::BadRequest(format!("RRULE: {e}")))?;
    Ok(r.to_uppercase())
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
        l.push(format!("RRULE:{}", validate_rrule(r)?));
    }
    l.push(format!("LAST-MODIFIED:{}", ical::fmt_utc_now()));
    if let Some(r) = raw {
        l.extend(r.props.iter().filter(|x| !MANAGED.contains(&prop_name(x).as_str())).cloned());
    }
    for a in &input.alarms {
        l.extend(build_valarm(a, input.summary.trim(), raw, tz));
    }
    Ok(l)
}

fn assemble(vtimezone: Option<String>, vevents: Vec<Vec<String>>, extra_blocks: &[String]) -> String {
    let mut out = String::new();
    out.push_str("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//xarif.de//rustical-ui//DE\r\nCALSCALE:GREGORIAN\r\n");
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
    collections::ensure_owner(user, &input.principal)?;
    let tz = user_tz(&input, default_tz);
    let uid = uuid::Uuid::new_v4().to_string();
    let lines = build_vevent(&uid, 0, &input, tz, None)?;
    let ics = assemble(if input.all_day { None } else { ical::vtimezone(tz.name()) }, vec![lines], &[]);
    let obj = CalendarObject::import(&ics, None)?;
    let object_id = uid.clone();
    state.calendars.put_object(&input.principal, &input.cal_id, &object_id, obj, false).await?;
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
    out
}

pub async fn delete_event(state: &AppState, user: &Principal, principal: &str, cal_id: &str, object_id: &str) -> AppResult<()> {
    collections::ensure_owner(user, principal)?;
    state.calendars.delete_object(principal, cal_id, object_id, true).await?;
    Ok(())
}

/// Number of instances per month of `year` (picker density) — one pass over the year.
pub async fn month_density(state: &AppState, cals: &[CalInfo], year: i32, tz: chrono_tz::Tz) -> AppResult<[usize; 12]> {
    let start = NaiveDate::from_ymd_opt(year, 1, 1).unwrap();
    let end = NaiveDate::from_ymd_opt(year + 1, 1, 1).unwrap();
    let mut d = [0usize; 12];
    for i in instances(state, cals, start, end, tz).await? {
        use chrono::Datelike;
        if i.start.date().year() == year {
            d[(i.start.date().month() - 1) as usize] += 1;
        }
    }
    Ok(d)
}

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
            categories: vec!["Arzt".into()],
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
            categories: vec!["Familie".into(), "Kultur".into(), "Demo".into()],
            timezone: Some("Europe/Berlin".into()),
        };
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
        assert!(out.contains("ACKNOWLEDGED:20260915T170000Z") && out.contains("PROXIMITY:DEPART") && out.contains("ATTENDEE;CN=Erika Musterfrau"));
    }
}
