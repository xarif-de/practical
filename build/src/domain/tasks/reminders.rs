//! Reminders of open tasks for the push tick: when each fires, and which fire inside a window.

use super::*;

/// When a task's reminder fires. A task has no duration, so the two anchors RFC 5545 offers
/// collapse: RELATED=END counts from DUE, RELATED=START from DTSTART, and each falls back to
/// the other when only one of the two is set. Without any date the reminder can never fire —
/// there is nothing to count from — unless it carries an absolute TRIGGER.
pub fn reminder_at(a: &Alarm, start: Option<NaiveDateTime>, due: Option<NaiveDateTime>) -> Option<NaiveDateTime> {
    if let Some(abs) = a.at {
        return Some(abs);
    }
    let anchor = if a.related_end { due.or(start) } else { start.or(due) }?;
    anchor.checked_sub_signed(chrono::Duration::try_minutes(a.minutes)?)
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
    pub(super) summary: String,
    ats: Vec<NaiveDateTime>,
    pub(super) due: Option<NaiveDateTime>,
    pub(super) due_all_day: bool,
}

pub(super) fn seed_of(raw: &events::RawMaster, tz: chrono_tz::Tz) -> Option<Seed> {
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
pub(super) fn window(seed: &Seed, cal: &CalInfo, object_id: &str, from: NaiveDate, to: NaiveDate) -> impl Iterator<Item = TaskReminder> {
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
pub(super) fn reminders_of(ics: &str, cal: &CalInfo, object_id: &str, tz: chrono_tz::Tz, from: NaiveDate, to: NaiveDate) -> Vec<TaskReminder> {
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
