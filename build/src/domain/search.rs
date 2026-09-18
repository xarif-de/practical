//! Universal search (top bar): events by title/location, contacts by name,
//! and a "jump to" suggestion when the query looks like a date.

use crate::domain::collections;
use crate::domain::contacts;
use crate::domain::events::{self, Instance};
use crate::domain::tasks;
use crate::error::AppResult;
use crate::state::AppState;
use chrono::{Datelike, Duration, NaiveDate};
use rustical_store::auth::Principal;

#[derive(Debug, serde::Serialize)]
pub struct SearchResult {
    pub jump: Option<JumpTo>,
    pub events: Vec<EventHit>,
    pub contacts: Vec<ContactHit>,
    /// open tasks first; used by the event editor's "link a task" search
    pub tasks: Vec<TaskHit>,
}

#[derive(Debug, serde::Serialize)]
pub struct TaskHit {
    pub key: String,
    pub uid: String,
    pub summary: String,
    pub due: Option<NaiveDate>,
    pub completed: bool,
    pub cal_name: String,
}

#[derive(Debug, serde::Serialize)]
pub struct JumpTo {
    pub date: NaiveDate,
    pub label: String,
    pub view: &'static str,
}

#[derive(Debug, serde::Serialize)]
pub struct EventHit {
    pub key: String,
    pub uid: String,
    pub summary: String,
    pub date: NaiveDate,
    pub time: String,
    pub color: String,
    pub cal_name: String,
}

pub use crate::domain::contacts::ContactHit;

const MONTHS_DE: [&str; 12] = ["januar", "februar", "märz", "april", "mai", "juni", "juli", "august", "september", "oktober", "november", "dezember"];
const MONTHS_EN: [&str; 12] = ["january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november", "december"];

/// `2027`, `märz`, `3. märz 2027`, `15.09.`, `15.9.2026`, `2026-09-15`, `sep 2027`.
pub fn parse_date_query(q: &str, today: NaiveDate) -> Option<JumpTo> {
    let t = q.trim().to_lowercase();
    if t.is_empty() {
        return None;
    }
    if let Ok(d) = NaiveDate::parse_from_str(&t, "%Y-%m-%d") {
        return Some(JumpTo { date: d, label: d.format("%d.%m.%Y").to_string(), view: "day" });
    }
    let mut year: Option<i32> = None;
    let mut month: Option<u32> = None;
    let mut day: Option<u32> = None;
    // dd.mm.[yyyy]
    let parts: Vec<&str> = t.split('.').map(str::trim).collect();
    if parts.len() >= 2 && parts.iter().take(2).all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())) {
        day = parts[0].parse().ok();
        month = parts[1].parse().ok();
        if let Some(y) = parts.get(2).filter(|p| !p.is_empty()) {
            year = y.parse::<i32>().ok().map(|y| if y < 100 { 2000 + y } else { y });
        }
    } else {
        for tok in t.split(|c: char| c == ' ' || c == '.').filter(|s| !s.is_empty()) {
            if let Ok(n) = tok.parse::<u32>() {
                if n >= 1900 && n <= 2200 {
                    year = Some(n as i32);
                } else if (1..=31).contains(&n) && day.is_none() {
                    day = Some(n);
                }
            } else if let Some(i) = MONTHS_DE.iter().chain(MONTHS_EN.iter()).position(|m| m.starts_with(tok) && tok.len() >= 3) {
                month = Some((i % 12) as u32 + 1);
            } else {
                return None;
            }
        }
    }
    if year.is_none() && month.is_none() && day.is_none() {
        return None;
    }
    let (y, m, d) = (year.unwrap_or(today.year()), month.unwrap_or(if day.is_some() { today.month() } else { 1 }), day.unwrap_or(1));
    let date = NaiveDate::from_ymd_opt(y, m, d)?;
    let view = if day.is_some() { "day" } else if month.is_some() { "month" } else { "planner" };
    let label = match (day, month, year) {
        (Some(_), _, _) => date.format("%d.%m.%Y").to_string(),
        (None, Some(_), _) => format!("{} {}", MONTHS_DE[(m - 1) as usize].to_uppercase(), y),
        _ => y.to_string(),
    };
    Some(JumpTo { date, label: capitalize(&label), view })
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase(),
        None => String::new(),
    }
}

pub async fn search(state: &AppState, user: &Principal, q: &str, today: NaiveDate, tz: chrono_tz::Tz) -> AppResult<SearchResult> {
    let jump = parse_date_query(q, today);
    let ql = q.trim().to_lowercase();
    let mut ev_hits = vec![];
    let mut ct_hits = vec![];
    let mut task_hits = vec![];
    if ql.len() >= 2 {
        let cals = events::visible_calendars(state, user, None).await?;
        // ±2 years around today: scanning 4 000 objects takes ~40 ms on the real store
        let inst: Vec<Instance> = events::instances(state, &cals, today - Duration::days(730), today + Duration::days(730), tz).await?;
        let mut seen = std::collections::HashSet::new();
        let mut hits: Vec<&Instance> = inst.iter().filter(|i| i.summary.to_lowercase().contains(&ql) || i.location.to_lowercase().contains(&ql)).collect();
        // upcoming first, then the most recent past
        hits.sort_by_key(|i| if i.start.date() >= today { (0, (i.start.date() - today).num_days()) } else { (1, (today - i.start.date()).num_days()) });
        for i in hits {
            if !seen.insert((i.key(), i.start)) {
                continue;
            }
            ev_hits.push(EventHit { key: i.key(), uid: i.uid.clone(), summary: i.summary.clone(), date: i.start.date(), time: i.time_label(), color: i.color.clone(), cal_name: i.cal_name.clone() });
            if ev_hits.len() >= 6 {
                break;
            }
        }
        let abs = collections::addressbooks_for(state, user, false).await?;
        for c in contacts::contacts_for(state, &abs).await? {
            if c.full_name.to_lowercase().contains(&ql) || c.org.to_lowercase().contains(&ql) || c.primary_email().to_lowercase().contains(&ql) {
                let sub = if !c.birthday.is_empty() { format!("🎂 {}", c.birthday_label()) } else if !c.org.is_empty() { c.org.clone() } else { c.category() };
                ct_hits.push(ContactHit::of(&c, sub));
                if ct_hits.len() >= 6 {
                    break;
                }
            }
        }
        for t in tasks::tasks_for(state, &cals, tz).await? {
            if t.summary.to_lowercase().contains(&ql) {
                task_hits.push(TaskHit { key: t.key(), uid: t.uid.clone(), summary: t.summary.clone(), due: t.due.map(|d| d.date()), completed: t.completed, cal_name: t.cal_name.clone() });
                if task_hits.len() >= 6 {
                    break;
                }
            }
        }
    }
    Ok(SearchResult { jump, events: ev_hits, contacts: ct_hits, tasks: task_hits })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_queries() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 15).unwrap();
        assert_eq!(parse_date_query("2027", today).unwrap().date, NaiveDate::from_ymd_opt(2027, 1, 1).unwrap());
        assert_eq!(parse_date_query("März", today).unwrap().date, NaiveDate::from_ymd_opt(2026, 3, 1).unwrap());
        assert_eq!(parse_date_query("3. März 2027", today).unwrap().date, NaiveDate::from_ymd_opt(2027, 3, 3).unwrap());
        assert_eq!(parse_date_query("15.09.", today).unwrap().date, NaiveDate::from_ymd_opt(2026, 9, 15).unwrap());
        assert_eq!(parse_date_query("1.1.27", today).unwrap().date, NaiveDate::from_ymd_opt(2027, 1, 1).unwrap());
        assert!(parse_date_query("Zahnarzt", today).is_none());
        assert_eq!(parse_date_query("sep 2027", today).unwrap().view, "month");
    }
}
