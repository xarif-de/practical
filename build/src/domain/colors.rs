//! Every colour the user's data carries. The colour picker offers exactly these (a circle per colour,
//! named by its sources); the settings page "Farben" lists them per element and changes them in place.

use crate::domain::collections::{self, CalInfo};
use crate::domain::{catrules, events, ical, prefs};
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use caldata::component::CalendarInnerData;
use rustical_ical::CalendarObject;
use rustical_store::auth::Principal;
use rustical_store::{CalendarReadStore, CalendarWriteStore};

#[derive(Debug, Clone, serde::Serialize)]
pub struct Source {
    /// calendar | addressbook | event | task | category | category-task | holiday | school
    pub kind: &'static str,
    pub name: String,
    /// as stored: #rrggbb, or a CSS colour name on events and tasks (RFC 7986 COLOR);
    /// "" = the theme's colour (holiday, school)
    pub color: String,
    /// what a change addresses: principal/id (collections), principal/calendar/object (events,
    /// tasks), the category name (rules)
    pub key: String,
    /// events and tasks: their calendar, which they fall back to without a colour of their own
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cal: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cal_color: Option<String>,
    /// false for objects of a subscribed calendar: the next refresh would overwrite a change
    pub editable: bool,
    /// events and tasks: their CATEGORIES (the colour optimiser puts them with their category)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cats: Vec<String>,
}

fn src(kind: &'static str, name: &str, color: &str, key: String) -> Source {
    Source { kind, name: name.to_string(), color: color.to_string(), key, cal: None, cal_color: None, editable: true, cats: vec![] }
}

/// Collections, holiday colours, category rules with a colour, then every event and task with a
/// COLOR of its own (one scan of all objects, like `events::category_counts`).
pub async fn sources(state: &AppState, user: &Principal) -> AppResult<Vec<Source>> {
    let mut out = vec![];
    let cals = collections::calendars_for(state, user, false).await?;
    out.extend(cals.iter().map(|c| src("calendar", &c.displayname, &c.color, format!("{}/{}", c.principal, c.id))));
    out.extend(collections::addressbooks_for(state, user, false).await?.iter().map(|a| src("addressbook", &a.displayname, &a.color, format!("{}/{}", a.principal, a.id))));
    let p = prefs::get(&state.db, &user.id).await?;
    out.push(src("holiday", "", p.holiday_color.as_deref().filter(|c| prefs::is_hex_color(c)).unwrap_or(""), String::new()));
    out.push(src("school", "", p.school_color.as_deref().filter(|c| prefs::is_hex_color(c)).unwrap_or(""), String::new()));
    for (scope, kind) in [("event", "category"), ("task", "category-task")] {
        for (name, rule) in catrules::all(&state.db, &user.id, scope).await? {
            if let Some(c) = rule.when.color.filter(|c| events::valid_color(&c.value)) {
                out.push(src(kind, &name, &c.value, name.clone()));
            }
        }
    }
    for cal in &cals {
        out.extend(object_sources(state, cal).await?);
    }
    Ok(out)
}

async fn object_sources(state: &AppState, cal: &CalInfo) -> AppResult<Vec<Source>> {
    let mut out = vec![];
    for (id, obj) in state.calendars.get_objects(&cal.principal, &cal.id).await? {
        let (kind, color, name, cats) = match obj.get_inner().get_inner() {
            CalendarInnerData::Event(ev, _) => ("event", ical::text(ev, "COLOR"), ical::text(ev, "SUMMARY"), ical::list(ev, "CATEGORIES")),
            CalendarInnerData::Todo(t, _) => ("task", ical::text(t, "COLOR"), ical::text(t, "SUMMARY"), ical::list(t, "CATEGORIES")),
            _ => continue,
        };
        let Some(color) = color.map(|c| c.trim().to_string()).filter(|c| events::valid_color(c)) else { continue };
        out.push(Source {
            kind,
            name: name.unwrap_or_default(),
            color,
            key: format!("{}/{}/{id}", cal.principal, cal.id),
            cal: Some(cal.displayname.clone()),
            cal_color: Some(cal.color.clone()),
            editable: cal.subscription_url.is_none(),
            cats: cats.into_iter().map(|c| c.trim().to_string()).filter(|c| !c.is_empty()).collect(),
        });
    }
    Ok(out)
}

/// Events (scope "event") or tasks ("task") carrying category `name` and the colour `from` — the ones a
/// changed category colour should follow; an object someone gave another colour is left alone. Objects of
/// subscribed calendars are skipped (read-only). Returns principal/calendar/object keys.
pub async fn category_objects(state: &AppState, user: &Principal, scope: &str, name: &str, from: &str) -> AppResult<Vec<String>> {
    let (name, from) = (name.trim(), from.trim().to_lowercase());
    let mut out = vec![];
    for cal in collections::calendars_for(state, user, false).await?.iter().filter(|c| c.subscription_url.is_none()) {
        for (id, obj) in state.calendars.get_objects(&cal.principal, &cal.id).await? {
            let (cats, color) = match (obj.get_inner().get_inner(), scope) {
                (CalendarInnerData::Event(ev, _), "event") => (ical::list(ev, "CATEGORIES"), ical::text(ev, "COLOR")),
                (CalendarInnerData::Todo(t, _), "task") => (ical::list(t, "CATEGORIES"), ical::text(t, "COLOR")),
                _ => continue,
            };
            if color.is_some_and(|c| c.trim().to_lowercase() == from) && cats.iter().any(|c| c.trim() == name) {
                out.push(format!("{}/{}/{id}", cal.principal, cal.id));
            }
        }
    }
    Ok(out)
}

/// One change of the colour optimiser. `key` as in `Source`; `color` "#rrggbb", or "" where that means
/// something: an event or task follows its calendar again, holidays/school holidays take the theme's.
#[derive(Debug, serde::Deserialize)]
pub struct Change {
    pub kind: String,
    pub key: String,
    pub color: String,
}

/// Applies `changes` one by one and returns (done, failed): a failing one (gone, read-only, invalid) does
/// not stop the others. Category rules and preferences are read once and written once.
pub async fn apply(state: &AppState, user: &Principal, changes: &[Change]) -> AppResult<(usize, usize)> {
    let (mut done, mut failed) = (0, 0);
    let mut rules: std::collections::BTreeMap<&str, std::collections::BTreeMap<String, catrules::Rule>> = Default::default();
    let mut p = None;
    for ch in changes {
        let color = ch.color.trim().to_lowercase();
        let parts: Vec<&str> = ch.key.split('/').collect();
        let ok = match (ch.kind.as_str(), parts.as_slice()) {
            ("calendar", [o, id]) => set_calendar_color(state, user, o, id, &color).await.is_ok(),
            ("event" | "task", [o, cal, id]) => set_object_color(state, user, o, cal, id, &color).await.is_ok(),
            ("category" | "category-task", _) if prefs::is_hex_color(&color) => {
                let scope = if ch.kind == "category" { "event" } else { "task" };
                if !rules.contains_key(scope) {
                    rules.insert(scope, catrules::all(&state.db, &user.id, scope).await?);
                }
                match rules.get_mut(scope).and_then(|r| r.get_mut(&ch.key)).and_then(|r| r.when.color.as_mut()) {
                    Some(c) => { c.value = color; true }
                    None => false,
                }
            }
            ("holiday" | "school", _) if color.is_empty() || prefs::is_hex_color(&color) => {
                if p.is_none() {
                    p = Some(prefs::get(&state.db, &user.id).await?);
                }
                let v = Some(color).filter(|c| !c.is_empty());
                let p = p.as_mut().unwrap();
                if ch.kind == "holiday" { p.holiday_color = v } else { p.school_color = v }
                true
            }
            _ => false,
        };
        if ok { done += 1 } else { failed += 1 }
    }
    for (scope, all) in &rules {
        for (name, rule) in all {
            if changes.iter().any(|c| &c.key == name && c.kind == if *scope == "event" { "category" } else { "category-task" }) {
                catrules::put(&state.db, &user.id, scope, name, rule).await?;
            }
        }
    }
    if let Some(p) = p {
        prefs::set(&state.db, &user.id, &p).await?;
    }
    Ok((done, failed))
}

pub async fn set_calendar_color(state: &AppState, user: &Principal, owner: &str, id: &str, color: &str) -> AppResult<()> {
    collections::ensure_owner(user, owner)?;
    if !prefs::is_hex_color(color) {
        return Err(AppError::BadRequest("color must be #rrggbb".into()));
    }
    let mut c = state.calendars.get_calendar(owner, id, false).await?;
    c.meta.color = Some(color.to_lowercase());
    state.calendars.update_calendar(owner, id, c).await?;
    Ok(())
}

/// An event's or task's own colour: `color` "" removes it (the calendar's shows again). Only the
/// COLOR line of the master component changes; every other line keeps its bytes.
pub async fn set_object_color(state: &AppState, user: &Principal, principal: &str, cal_id: &str, object_id: &str, color: &str) -> AppResult<()> {
    collections::ensure_owner(user, principal)?;
    if !color.is_empty() && !prefs::is_hex_color(color) {
        return Err(AppError::BadRequest("color must be #rrggbb or empty".into()));
    }
    let cal = state.calendars.get_calendar(principal, cal_id, false).await?;
    if cal.subscription_url.is_some() {
        return Err(AppError::BadRequest("a subscribed calendar is read-only".into()));
    }
    let existing = state.calendars.get_object(principal, cal_id, object_id, false).await?;
    let Some(ics) = with_color(&existing.get_ics(), &color.to_lowercase()) else {
        return Err(AppError::BadRequest("no event or task in this object".into()));
    };
    let obj = CalendarObject::import(&ics, None)?;
    state.calendars.put_object(principal, cal_id, object_id, obj, true).await?;
    Ok(())
}

/// The object text with the master VEVENT/VTODO's COLOR set to `color` ("" = removed). The master is
/// the first such component without RECURRENCE-ID; a nested VALARM is never touched. Works on
/// physical lines so every other line (folding included) stays as it was. None: no VEVENT/VTODO.
pub fn with_color(ics: &str, color: &str) -> Option<String> {
    // physical lines grouped into content lines: a line plus its folded continuations
    let mut groups: Vec<Vec<&str>> = vec![];
    for l in ics.split('\n').map(|l| l.trim_end_matches('\r')) {
        if (l.starts_with(' ') || l.starts_with('\t')) && !groups.is_empty() {
            groups.last_mut().unwrap().push(l);
        } else {
            groups.push(vec![l]);
        }
    }
    let name = |g: &Vec<&str>| events::prop_name(g[0]);
    // top-level components (depth 2, inside VCALENDAR): (begin, end, has RECURRENCE-ID)
    let (mut depth, mut comps, mut open): (usize, Vec<(usize, usize, bool)>, Option<(usize, bool)>) = (0, vec![], None);
    for (i, g) in groups.iter().enumerate() {
        let line = g[0].to_ascii_uppercase();
        if line.starts_with("BEGIN:") {
            depth += 1;
            if depth == 2 && (line == "BEGIN:VEVENT" || line == "BEGIN:VTODO") {
                open = Some((i, false));
            }
        } else if line.starts_with("END:") {
            if depth == 2
                && let Some((b, rid)) = open.take()
            {
                comps.push((b, i, rid));
            }
            depth = depth.saturating_sub(1);
        } else if depth == 2
            && let Some((_, rid)) = open.as_mut()
            && name(g) == "RECURRENCE-ID"
        {
            *rid = true;
        }
    }
    let (b, e, _) = *comps.iter().find(|c| !c.2).or(comps.first())?;
    // COLOR lines directly in the master (not in its VALARMs)
    let mut inner = 0usize;
    let mut drop: Vec<usize> = vec![];
    for (i, g) in groups.iter().enumerate().take(e).skip(b + 1) {
        let line = g[0].to_ascii_uppercase();
        if line.starts_with("BEGIN:") {
            inner += 1;
        } else if line.starts_with("END:") {
            inner = inner.saturating_sub(1);
        } else if inner == 0 && name(g) == "COLOR" {
            drop.push(i);
        }
    }
    let new_line = format!("COLOR:{color}");
    let mut out: Vec<&str> = vec![];
    for (i, g) in groups.iter().enumerate() {
        if i == e && !color.is_empty() {
            out.push(&new_line);
        }
        if !drop.contains(&i) {
            out.extend(g.iter().copied());
        }
    }
    // the input's trailing CRLF made an empty last group, which is kept, so the ending survives
    Some(out.join("\r\n"))
}

#[cfg(test)]
mod pinned;

#[cfg(test)]
mod tests {
    use super::*;

    const ICS: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:a\r\nSUMMARY:Lang\r\n  gefaltet\r\nCOLOR:tomato\r\nBEGIN:VALARM\r\nACTION:DISPLAY\r\nCOLOR:red\r\nEND:VALARM\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:a\r\nRECURRENCE-ID:20260101T100000Z\r\nCOLOR:blue\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    #[test]
    fn colour_changes_only_on_the_master() {
        let out = with_color(ICS, "#30b46c").unwrap();
        assert_eq!(out, ICS.replace("COLOR:tomato\r\nBEGIN:VALARM", "BEGIN:VALARM").replace("END:VALARM\r\nEND:VEVENT\r\nBEGIN:VEVENT", "END:VALARM\r\nCOLOR:#30b46c\r\nEND:VEVENT\r\nBEGIN:VEVENT"));
        assert!(out.contains("COLOR:red") && out.contains("COLOR:blue") && out.contains("SUMMARY:Lang\r\n  gefaltet"));
    }

    #[test]
    fn empty_colour_removes_it_and_a_task_works_too() {
        assert_eq!(with_color(ICS, "").unwrap(), ICS.replace("COLOR:tomato\r\n", ""));
        let todo = "BEGIN:VCALENDAR\nBEGIN:VTODO\nUID:t\nEND:VTODO\nEND:VCALENDAR\n";
        assert_eq!(with_color(todo, "#0a84ff").unwrap(), "BEGIN:VCALENDAR\r\nBEGIN:VTODO\r\nUID:t\r\nCOLOR:#0a84ff\r\nEND:VTODO\r\nEND:VCALENDAR\r\n");
        assert_eq!(with_color("BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n", "#0a84ff"), None);
    }
}
