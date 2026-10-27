//! Create / update / delete through RustiCal's store, and the input limits every event and task save checks.

use super::*;

/// Text and list limits of what the editors write (characters / entries) — far above what a person types or a
/// client stores, low enough that one object cannot hold hundreds of MiB of views.
pub const MAX_TITLE: usize = 1_000;

pub const MAX_NOTE: usize = 100_000;

pub const MAX_LIST: usize = 1_000;

/// One alarm as the editor may send it: offsets within `ical::MAX_DURATION_SECS` (what the read path accepts,
/// so a stored alarm survives a save), absolute times within `ical::YEARS`.
fn validate_alarm(a: &Alarm) -> AppResult<()> {
    let max = ical::MAX_DURATION_SECS / 60;
    let date_ok = |d: Option<NaiveDateTime>| d.is_none_or(|d| ical::year_ok(d.date()));
    if a.minutes.checked_abs().is_none_or(|m| m > max) || !(0..=max).contains(&a.duration_minutes) || !(0..=1_000_000).contains(&a.repeat) || !date_ok(a.at) || !date_ok(a.acknowledged) {
        return Err(AppError::bad("err-alarm-range"));
    }
    Ok(())
}

/// The checks every event and task save shares: text lengths, list lengths, alarm bounds.
pub(crate) fn validate_common(summary: &str, location: &str, description: &str, categories: &[String], alarms: &[Alarm], links: &[Link], related: &[Related]) -> AppResult<()> {
    let long = |s: &str, max: usize| s.len() > max && s.chars().count() > max;
    if long(summary, MAX_TITLE) || long(location, MAX_TITLE) || long(description, MAX_NOTE) || categories.iter().any(|c| long(c, MAX_TITLE)) {
        return Err(AppError::bad("err-too-long"));
    }
    if categories.len() > MAX_LIST || alarms.len() > MAX_LIST || links.len() > MAX_LIST || related.len() > MAX_LIST {
        return Err(AppError::bad("err-too-long"));
    }
    alarms.iter().try_for_each(validate_alarm)
}

pub fn validate_rrule(r: &str) -> AppResult<String> {
    let r = ical::rrule_canon(r);
    if r.is_empty() {
        return Err(AppError::bad("err-rrule-empty"));
    }
    if !r.starts_with("FREQ=") || r.chars().any(|c| c == '\n' || c == '\r') {
        return Err(AppError::bad("err-rrule-freq"));
    }
    let _: caldata::rrule::RRule<caldata::rrule::Unvalidated> = r.parse().map_err(|e| AppError::bad1("err-rrule", "detail", e))?;
    Ok(r)
}

fn user_tz(input: &EventInput, default: chrono_tz::Tz) -> chrono_tz::Tz {
    input.timezone.as_deref().and_then(ical::parse_tz).unwrap_or(default)
}

/// A sharee who writes an object it would then see shortened or not at all (its CLASS under the owner's
/// policy) has to confirm it first (R2: it may; the editor asks). Own calendars: nothing to ask.
pub(crate) fn confirm_visibility(cal: &crate::state::Authorized, obj: &CalendarObject, confirmed: bool) -> AppResult<()> {
    let level = cal.level(obj);
    if !cal.is_own() && !confirmed && level < crate::state::FULL_LEVEL {
        return Err(AppError::ConfirmVisibility(level));
    }
    Ok(())
}

pub async fn create_event(state: &AppState, user: &Principal, input: EventInput, default_tz: chrono_tz::Tz) -> AppResult<String> {
    // no calendar the user may write into (only read-only ones, or none): say so instead of a bare "not found" /
    // "read-only" for whatever target the request names — the editor's dialog says the same (fill.js openEditor)
    if !collections::calendars_for(state, user, false).await?.iter().any(|c| !c.read_only) {
        return Err(AppError::bad("err-no-writable-calendar"));
    }
    create_event_as(state, user, input, default_tz, None).await
}

/// `keep_uid`: a move to another calendar keeps the event's UID (as a CalDAV MOVE does), so
/// clients and the change history see the same event; a live object with it there gets a new one.
pub async fn create_event_as(state: &AppState, user: &Principal, input: EventInput, default_tz: chrono_tz::Tz, keep_uid: Option<&str>) -> AppResult<String> {
    let cal = collections::ensure_calendar(state, user, &input.principal, &input.cal_id, Need::Read).await?;
    collections::ensure_writable(state, &cal).await?;
    let tz = user_tz(&input, default_tz);
    let keep = match keep_uid {
        Some(u) if state.collections().object(&cal, u).await.is_err() => Some(u.to_string()),
        _ => None,
    };
    let uid = keep.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let lines = build_vevent(&uid, 0, &input, tz, None)?;
    let ics = ical::assemble(if input.all_day { None } else { ical::vtimezone(tz.name()) }, "VEVENT", lines, &[]);
    let obj = CalendarObject::import(&ics, None)?;
    confirm_visibility(&cal, &obj, input.confirm_visibility)?;
    let object_id = uid.clone();
    // overwrite only replaces a trashed copy of a kept UID (a move there and back; never through a share)
    state.collections().put_object(&cal, &object_id, obj, keep.is_some(), None).await?;
    Ok(object_id)
}

/// Rewrites the master VEVENT from the input plus every stored line the editor does not
/// model; RECURRENCE-ID overrides written by other clients are carried over untouched
/// (raw blocks), so "edit the series" never silently drops a moved single occurrence.
pub async fn update_event(state: &AppState, user: &Principal, object_id: &str, input: EventInput, expected_etag: Option<&str>, default_tz: chrono_tz::Tz) -> AppResult<()> {
    let cal = collections::ensure_calendar(state, user, &input.principal, &input.cal_id, Need::Read).await?;
    collections::ensure_writable(state, &cal).await?;
    let tz = user_tz(&input, default_tz);
    let existing = state.collections().object(&cal, object_id).await?;
    if let Some(e) = expected_etag
        && !e.is_empty()
        && e != existing.get_etag()
    {
        return Err(AppError::conflict("err-changed-event"));
    }
    let CalendarInnerData::Event(main, _) = existing.get_inner().get_inner() else {
        return Err(AppError::NotFound);
    };
    let uid = main.get_uid().to_string();
    let sequence: i64 = main.get_property("SEQUENCE").and_then(|p| p.value.parse::<i64>().ok()).unwrap_or(0).saturating_add(1);
    let (raw, extra) = split_ics(&existing.get_ics());
    let lines = build_vevent(&uid, sequence, &input, tz, Some(&raw))?;
    let ics = ical::assemble(if input.all_day { None } else { ical::vtimezone(tz.name()) }, "VEVENT", lines, &extra);
    let obj = CalendarObject::import(&ics, None)?;
    confirm_visibility(&cal, &obj, input.confirm_visibility)?;
    // the version the editor showed: through a share checked again in the write's transaction
    state.collections().put_object(&cal, object_id, obj, true, Some(&existing.get_etag())).await?;
    Ok(())
}
