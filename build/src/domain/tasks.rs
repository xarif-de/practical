//! Tasks (VTODO) — the minimal view of plan D15: list per calendar, open/done,
//! title · due · priority, create, toggle, delete.

use crate::domain::collections::{self, CalInfo};
use crate::domain::ical;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use caldata::component::{CalendarInnerData, Component};
use chrono::{NaiveDate, NaiveDateTime};
use rustical_ical::CalendarObject;
use rustical_store::auth::Principal;
use rustical_store::{CalendarReadStore, CalendarWriteStore};

#[derive(Debug, Clone, serde::Serialize)]
pub struct Task {
    pub principal: String,
    pub cal_id: String,
    pub cal_name: String,
    pub color: String,
    pub object_id: String,
    pub summary: String,
    pub description: String,
    pub due: Option<NaiveDateTime>,
    pub due_all_day: bool,
    pub completed: bool,
    pub priority: u8,
}

impl Task {
    pub fn due_label(&self) -> String {
        match self.due {
            Some(d) if self.due_all_day => d.format("%d.%m.%Y").to_string(),
            Some(d) => d.format("%d.%m.%Y %H:%M").to_string(),
            None => String::new(),
        }
    }
    pub fn overdue(&self, today: NaiveDate) -> bool {
        !self.completed && self.due.is_some_and(|d| d.date() < today)
    }
    pub fn key(&self) -> String {
        format!("{}/{}/{}", self.principal, self.cal_id, self.object_id)
    }
}

pub async fn tasks_for(state: &AppState, cals: &[CalInfo], tz: chrono_tz::Tz) -> AppResult<Vec<Task>> {
    let mut out = vec![];
    for cal in cals {
        if !cal.components.iter().any(|c| c == "VTODO") {
            continue;
        }
        for (id, obj) in state.calendars.get_objects(&cal.principal, &cal.id).await? {
            let CalendarInnerData::Todo(t, _) = obj.get_inner().get_inner() else { continue };
            let (due, due_all_day) = match &t.due {
                Some(d) => {
                    let (l, ad) = ical::local(&d.0, tz);
                    (Some(l), ad)
                }
                None => (None, false),
            };
            out.push(Task {
                principal: cal.principal.clone(),
                cal_id: cal.id.clone(),
                cal_name: cal.displayname.clone(),
                color: cal.color.clone(),
                object_id: id,
                summary: ical::text(t, "SUMMARY").unwrap_or_else(|| "(ohne Titel)".into()),
                description: ical::text(t, "DESCRIPTION").unwrap_or_default(),
                due,
                due_all_day,
                completed: t.get_property("STATUS").is_some_and(|s| s.value.eq_ignore_ascii_case("COMPLETED")) || t.get_property("COMPLETED").is_some(),
                priority: t.get_property("PRIORITY").and_then(|p| p.value.parse().ok()).unwrap_or(0),
            });
        }
    }
    out.sort_by(|a, b| a.completed.cmp(&b.completed).then(a.due.is_none().cmp(&b.due.is_none())).then(a.due.cmp(&b.due)).then(a.summary.cmp(&b.summary)));
    Ok(out)
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct TaskInput {
    pub principal: String,
    pub cal_id: String,
    pub summary: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub due: Option<NaiveDate>,
    #[serde(default)]
    pub priority: u8,
}

fn build_vtodo(uid: &str, i: &TaskInput, completed: bool, existing_desc: Option<&str>) -> AppResult<String> {
    if i.summary.trim().is_empty() {
        return Err(AppError::BadRequest("summary required".into()));
    }
    let mut l = vec![
        "BEGIN:VCALENDAR".to_string(),
        "VERSION:2.0".to_string(),
        "PRODID:-//xarif.de//rustical-ui//DE".to_string(),
        "BEGIN:VTODO".to_string(),
        format!("UID:{uid}"),
        format!("DTSTAMP:{}", ical::fmt_utc_now()),
        format!("SUMMARY:{}", ical::escape(i.summary.trim())),
    ];
    let desc = if i.description.trim().is_empty() { existing_desc.unwrap_or("") } else { i.description.trim() };
    if !desc.is_empty() {
        l.push(format!("DESCRIPTION:{}", ical::escape(desc)));
    }
    if let Some(d) = i.due {
        l.push(format!("DUE;VALUE=DATE:{}", ical::fmt_date(d)));
    }
    if i.priority > 0 {
        l.push(format!("PRIORITY:{}", i.priority.min(9)));
    }
    if completed {
        l.push("STATUS:COMPLETED".into());
        l.push(format!("COMPLETED:{}", ical::fmt_utc_now()));
        l.push("PERCENT-COMPLETE:100".into());
    } else {
        l.push("STATUS:NEEDS-ACTION".into());
    }
    l.push("END:VTODO".into());
    l.push("END:VCALENDAR".into());
    Ok(l.iter().map(|x| ical::fold(x)).collect::<Vec<_>>().join("\r\n") + "\r\n")
}

pub async fn create_task(state: &AppState, user: &Principal, i: TaskInput) -> AppResult<String> {
    collections::ensure_owner(user, &i.principal)?;
    let uid = uuid::Uuid::new_v4().to_string();
    let obj = CalendarObject::import(&build_vtodo(&uid, &i, false, None)?, None)?;
    state.calendars.put_object(&i.principal, &i.cal_id, &uid, obj, false).await?;
    Ok(uid)
}

/// Rewrites the task from the input; `completed` toggles the status.
pub async fn update_task(state: &AppState, user: &Principal, object_id: &str, i: TaskInput, completed: bool) -> AppResult<()> {
    collections::ensure_owner(user, &i.principal)?;
    let existing = state.calendars.get_object(&i.principal, &i.cal_id, object_id, false).await?;
    let CalendarInnerData::Todo(t, _) = existing.get_inner().get_inner() else { return Err(AppError::NotFound) };
    let uid = t.get_uid().to_string();
    let desc = ical::text(t, "DESCRIPTION");
    let obj = CalendarObject::import(&build_vtodo(&uid, &i, completed, desc.as_deref())?, None)?;
    state.calendars.put_object(&i.principal, &i.cal_id, object_id, obj, true).await?;
    Ok(())
}

pub async fn delete_task(state: &AppState, user: &Principal, principal: &str, cal_id: &str, object_id: &str) -> AppResult<()> {
    collections::ensure_owner(user, principal)?;
    state.calendars.delete_object(principal, cal_id, object_id, true).await?;
    Ok(())
}
