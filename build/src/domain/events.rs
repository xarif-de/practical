//! Calendar events: window expansion for the views (caldata's rrule engine —
//! validated against the real calendar, plan D11), plus create/update/delete
//! through RustiCal's store. Every view works on `Instance`s in the user's zone.

use crate::domain::collections::{self, CalInfo};
use crate::domain::ical;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use caldata::component::{CalendarInnerData, Component, IcalEvent};
use caldata::types::parse_duration;
use chrono::{Duration, NaiveDate, NaiveDateTime};
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
    /// minutes before start of the first DISPLAY alarm, if any
    pub alarm_minutes: Option<i64>,
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

pub fn alarm_minutes(ev: &IcalEvent) -> Option<i64> {
    for a in ev.get_alarms() {
        let Some(t) = a.get_property("TRIGGER") else { continue };
        if t.params.get_param("VALUE").is_some_and(|v| v.eq_ignore_ascii_case("DATE-TIME")) {
            continue;
        }
        if let Ok(d) = parse_duration(&t.value) {
            return Some(-d.num_minutes());
        }
    }
    None
}

fn instance_from(ev: &IcalEvent, cal: &CalInfo, object_id: &str, tz: chrono_tz::Tz, recurring: bool) -> Instance {
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
        alarm_minutes: alarm_minutes(ev),
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
            let expanded = main.expand_recurrence(Some(ws), Some(we), overrides);
            for ev in &expanded {
                let inst = instance_from(ev, cal, &id, tz, recurring);
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
    pub alarm_minutes: Option<i64>,
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
    let i = instance_from(main, &cal, object_id, tz, main.has_rruleset());
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
        alarm_minutes: i.alarm_minutes,
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
    pub alarm_minutes: Option<i64>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub timezone: Option<String>,
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

/// Builds the VEVENT lines (without BEGIN/END) for the master event.
fn build_vevent(uid: &str, sequence: i64, input: &EventInput, tz: chrono_tz::Tz) -> AppResult<Vec<String>> {
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
    if let Some(m) = input.alarm_minutes
        && m >= 0
    {
        l.push("BEGIN:VALARM".into());
        l.push("ACTION:DISPLAY".into());
        l.push(format!("DESCRIPTION:{}", ical::escape(input.summary.trim())));
        l.push(if m == 0 { "TRIGGER:PT0S".into() } else if m % 1440 == 0 { format!("TRIGGER:-P{}D", m / 1440) } else if m % 60 == 0 { format!("TRIGGER:-PT{}H", m / 60) } else { format!("TRIGGER:-PT{m}M") });
        l.push("END:VALARM".into());
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
    let lines = build_vevent(&uid, 0, &input, tz)?;
    let ics = assemble(if input.all_day { None } else { ical::vtimezone(tz.name()) }, vec![lines], &[]);
    let obj = CalendarObject::import(&ics, None)?;
    let object_id = uid.clone();
    state.calendars.put_object(&input.principal, &input.cal_id, &object_id, obj, false).await?;
    Ok(object_id)
}

/// Rewrites the master VEVENT; RECURRENCE-ID overrides written by other
/// clients are carried over untouched (raw blocks), so "edit the series"
/// never silently drops a moved single occurrence.
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
    let lines = build_vevent(&uid, sequence, &input, tz)?;
    // carry over override blocks + foreign VTIMEZONEs verbatim
    let raw = existing.get_ics().replace("\r\n", "\n");
    let unfolded = unfold(&raw);
    let mut extra = vec![];
    let mut cur: Vec<String> = vec![];
    let mut depth = 0;
    let mut is_override = false;
    for line in &unfolded {
        if line.starts_with("BEGIN:VEVENT") && depth == 0 {
            depth = 1;
            cur.clear();
            is_override = false;
        }
        if depth > 0 {
            cur.push(line.clone());
            if line.starts_with("RECURRENCE-ID") {
                is_override = true;
            }
            if line.starts_with("BEGIN:") && !line.starts_with("BEGIN:VEVENT") {
                depth += 1;
            } else if line.starts_with("END:VEVENT") {
                depth -= 1;
                if depth == 0 && is_override {
                    extra.push(cur.iter().map(|l| ical::fold(l)).collect::<Vec<_>>().join("\r\n"));
                }
            } else if line.starts_with("END:") {
                depth -= 1;
            }
        }
    }
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
        let inst: Vec<Instance> = expanded.iter().map(|ev| instance_from(ev, &cal, "bday", tz, true)).collect();
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
            alarm_minutes: Some(15),
            categories: vec!["Arzt".into()],
            timezone: None,
        };
        let lines = build_vevent("uid-1", 0, &input, tz).unwrap();
        let ics = assemble(ical::vtimezone("Europe/Berlin"), vec![lines], &[]);
        let obj = CalendarObject::import(&ics, None).unwrap();
        let CalendarInnerData::Event(main, overrides) = obj.get_inner().get_inner() else { panic!() };
        assert!(main.has_rruleset());
        assert_eq!(alarm_minutes(main), Some(15));
        let ex = main.expand_recurrence(None, None, overrides);
        assert_eq!(ex.len(), 3);
        let cal = CalInfo { principal: "p".into(), owner_label: String::new(), is_group: false, id: "c".into(), displayname: "Test".into(), color: "#000".into(), description: String::new(), components: vec![], timezone_id: String::new(), subscription_url: None, deleted_at: None, synctoken: 0 };
        let i = instance_from(&ex[1], &cal, "uid-1", tz, true);
        assert_eq!(i.start.date().day(), 10);
        assert_eq!(i.summary, "Zahnarzt, Kontrolle");
        assert_eq!(i.description, "Karte\nmitnehmen");
        assert_eq!((i.end - i.start).num_minutes(), 45);
        assert!(validate_rrule("DAILY").is_err());
    }
}
