//! Preference saves: the preferences form, holiday display, colour scheme, contact/task columns and list fields.

use super::*;

#[derive(Deserialize)]
pub(super) struct PrefsForm {
    pub(super) csrf: String,
    #[serde(default)]
    pub(super) lang: String,
    #[serde(default)]
    pub(super) scheme: String,
    #[serde(default)]
    pub(super) timezone: String,
    #[serde(default)]
    holiday_color: String,
    #[serde(default)]
    school_color: String,
    /// comma lists of region codes; absent = unchanged
    pub(super) public_regions: Option<String>,
    pub(super) school_regions: Option<String>,
    #[serde(default)]
    vacation_days: String,
    #[serde(default)]
    pub(super) vacation_kind: String,
    #[serde(default)]
    pub(super) vacation_category: String,
    #[serde(default)]
    pub(super) vacation_calendar: String,
    #[serde(default)]
    pub(super) vacation_title: String,
    #[serde(default)]
    pub(super) alarm_snaps_start: String,
    #[serde(default)]
    pub(super) alarm_snaps_end: String,
    /// days the search history is kept (prefs::HISTORY_DAYS); absent = unchanged
    search_history_days: Option<String>,
}

pub(super) async fn prefs_save(State(state): State<SharedState>, user: Principal, session: Session, mut ctx: PageCtx, headers: axum::http::HeaderMap, Form(f): Form<PrefsForm>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        let _lock = state.guards.prefs(&user.id).await; // until prefs::set: no lost update
        let mut p = prefs::get(&state.db, &user.id).await?;
        // what the two holiday lists held before this save, for the one-off copy further down
        let had = (!p.regions(false).is_empty(), !p.regions(true).is_empty());
        p.lang = Some(f.lang.clone()).filter(|l| crate::i18n::supported(l));
        p.scheme = Some(f.scheme.clone()).filter(|s| s == "light" || s == "dark");
        p.timezone = Some(f.timezone.trim().to_string()).filter(|t| !t.is_empty() && t.parse::<chrono_tz::Tz>().is_ok());
        // holiday colours: "" = the theme's; the switches live in the calendar's filter (holiday_display_save)
        p.holiday_color = Some(f.holiday_color.trim().to_lowercase()).filter(|c| prefs::is_hex_color(c));
        p.school_color = Some(f.school_color.trim().to_lowercase()).filter(|c| prefs::is_hex_color(c));
        // region lists: known-shaped codes, each once, in the user's order ("" = none)
        let regions = |v: &str| {
            let mut out: Vec<String> = vec![];
            for c in v.split(',').map(|c| c.trim().to_uppercase()).filter(|c| holidays::valid_code(c)) {
                if !out.contains(&c) && out.len() < 40 {
                    out.push(c);
                }
            }
            out.join(",")
        };
        if let Some(v) = &f.public_regions {
            p.public_regions = Some(regions(v));
        }
        if let Some(v) = &f.school_regions {
            p.school_regions = Some(regions(v));
        }
        mirror_first_region(&state, &user.id, &mut p, had).await?;
        p.vacation_days = f.vacation_days.trim().parse::<i64>().ok().filter(|d| (0..=366).contains(d));
        // vacation rule: the chosen kind with its own field; a title pattern must parse
        let (kind, value) = match f.vacation_kind.as_str() {
            "calendar" => ("calendar", f.vacation_calendar.trim()),
            "title" => ("title", f.vacation_title.trim()),
            _ => ("category", f.vacation_category.trim()),
        };
        if kind == "title" && !value.is_empty() {
            prefs::VacationRule::title_regex(value).map_err(|e| AppError::bad1("err-vacation-title", "detail", e))?;
        }
        p.vacation_rule_json = Some(serde_json::to_string(&prefs::VacationRule { v: 2, kind: kind.into(), value: value.into() }).unwrap());
        // reminder strip snap points: an empty list falls back to the built-in one; a bad token rejects the form
        let start = events::parse_offsets(&f.alarm_snaps_start).map_err(|t| AppError::bad1("err-snap-point", "token", t))?;
        let end = events::parse_offsets(&f.alarm_snaps_end).map_err(|t| AppError::bad1("err-snap-point", "token", t))?;
        let snaps = events::AlarmSnaps { start: if start.is_empty() { events::AlarmSnaps::default().start } else { start }, end };
        p.alarm_snaps_json = if snaps == events::AlarmSnaps::default() { None } else { Some(serde_json::to_string(&snaps).unwrap()) };
        if let Some(d) = f.search_history_days.as_deref().and_then(|d| d.parse::<i64>().ok()).filter(|d| prefs::HISTORY_DAYS.contains(d)) {
            p.search_history_days = Some(d);
        }
        prefs::set(&state.db, &user.id, &p).await?;
        // the answer ("Saved.") speaks the language just chosen — "automatic": the browser's
        let accept = headers.get(axum::http::header::ACCEPT_LANGUAGE).and_then(|v| v.to_str().ok());
        ctx.lang = p.lang.clone().unwrap_or_else(|| crate::i18n::negotiate(accept, &state.config.default_lang));
        /* The two holiday lists go back to the page, because this save can change one the user did
           not touch (`mirror_first_region`). Without the echo the page keeps its old hidden field
           and the NEXT save posts it, undoing the copy (found 2026-10-09). */
        let echo = serde_json::json!({
            "public": p.public_regions.clone().unwrap_or_default(),
            "school": p.school_regions.clone().unwrap_or_default(),
        });
        Ok((Some("flash-saved"), echo))
    }
    .await;
    let mut more = serde_json::Map::new();
    let r = r.map(|(msg, echo)| {
        more.insert("regions".into(), echo);
        msg
    });
    super::finish_more(&state, &session, &ctx, r, "prefs", more).await
}

/// The one-off copy between the two holiday lists: one region usually answers both, so the save
/// that FIRST names one fills the other as well (asked for 2026-10-09). `had` is what the two lists
/// held before this save, and that is what makes it one-off: only a list that was empty before and
/// is named now copies over. Any later save sees `had` set and leaves the other list alone — so a
/// user who empties one again keeps it empty, which a "both empty now" test would undo on the next
/// save of any unrelated field.
///
/// A region the user declined in the first steps is not filled in either: declining means wanting
/// none. That record is read only when a list was just named, so an ordinary save of this form
/// costs no extra statement.
async fn mirror_first_region(state: &SharedState, principal: &str, p: &mut prefs::Prefs, had: (bool, bool)) -> Result<(), AppError> {
    let public = p.public_regions.clone().unwrap_or_default();
    let school = p.school_regions.clone().unwrap_or_default();
    // "just named": empty before this save, non-empty now
    let (named_public, named_school) = (!had.0 && !public.is_empty(), !had.1 && !school.is_empty());
    let (fill_school, fill_public) = (named_public && school.is_empty() && !had.1, named_school && public.is_empty() && !had.0);
    if !fill_school && !fill_public {
        return Ok(());
    }
    use crate::domain::onboarding::Step;
    let rec = crate::domain::onboarding::get(&state.db, principal).await?;
    if fill_school && !rec.has(Step::School) {
        p.school_regions = Some(public);
    } else if fill_public && !rec.has(Step::Regions) {
        p.public_regions = Some(school);
    }
    Ok(())
}

#[derive(Deserialize)]
pub(super) struct HolidayDisplay {
    pub(super) public: Option<bool>,
    /// absent where the region has no school holidays (the switch is not shown): kept as it was
    pub(super) school: Option<bool>,
}

/// Show public holidays / school holidays in the calendar (its filter popover, app.js).
pub(super) async fn holiday_display_save(State(state): State<SharedState>, user: Principal, _: csrf::Token, crate::extract::Json(d): crate::extract::Json<HolidayDisplay>) -> AppResult<axum::http::StatusCode> {
    let _lock = state.guards.prefs(&user.id).await; // until prefs::set: no lost update
    let mut p = prefs::get(&state.db, &user.id).await?;
    if let Some(v) = d.public {
        p.holidays_enabled = v;
    }
    if let Some(v) = d.school {
        p.school_holidays_enabled = v;
    }
    prefs::set(&state.db, &user.id, &p).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub(super) struct SchemeChoice {
    pub(super) scheme: String,
}

/// The colour scheme from the button in the bar (app.js): auto | light | dark.
pub(super) async fn scheme_save(State(state): State<SharedState>, user: Principal, _: csrf::Token, crate::extract::Json(d): crate::extract::Json<SchemeChoice>) -> AppResult<axum::http::StatusCode> {
    let _lock = state.guards.prefs(&user.id).await; // until prefs::set: no lost update
    let mut p = prefs::get(&state.db, &user.id).await?;
    p.scheme = Some(d.scheme).filter(|s| s == "light" || s == "dark");
    prefs::set(&state.db, &user.id, &p).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// Contact table column choice (JSON array from app.js), stored per user.
pub(super) async fn columns_save(State(state): State<SharedState>, user: Principal, _: csrf::Token, crate::extract::Json(cols): crate::extract::Json<Vec<String>>) -> AppResult<axum::Json<Vec<String>>> {
    let cols: Vec<String> = cols.into_iter().filter(|c| crate::routes::contacts::ALL_COLUMNS.contains(&c.as_str())).collect();
    let _lock = state.guards.prefs(&user.id).await; // until prefs::set: no lost update
    let mut p = prefs::get(&state.db, &user.id).await?;
    p.contact_columns_json = Some(serde_json::to_string(&cols).unwrap());
    prefs::set(&state.db, &user.id, &p).await?;
    Ok(axum::Json(cols))
}

/// Task table column choice (JSON array from app.js), stored per user.
pub(super) async fn task_columns_save(State(state): State<SharedState>, user: Principal, _: csrf::Token, crate::extract::Json(cols): crate::extract::Json<Vec<String>>) -> AppResult<axum::Json<Vec<String>>> {
    let cols: Vec<String> = cols.into_iter().filter(|c| crate::routes::tasks::ALL_COLUMNS.contains(&c.as_str())).collect();
    let _lock = state.guards.prefs(&user.id).await; // until prefs::set: no lost update
    let mut p = prefs::get(&state.db, &user.id).await?;
    p.task_columns_json = Some(serde_json::to_string(&cols).unwrap());
    prefs::set(&state.db, &user.id, &p).await?;
    Ok(axum::Json(cols))
}

/// One filter row pinned (`on`) or unpinned as a quick switch (js/filter-pins.js); the other rows' pins stay,
/// so the calendar page and the contacts page (each knows only its own rows) cannot drop each other's.
#[derive(Deserialize)]
pub(super) struct FilterPin {
    key: String,
    on: bool,
}

/// Keys a filter row can have (macros.html `pin_star`): a calendar or an address book `<principal>/<id>`, or the
/// public / school holidays. Pins of rows that no longer exist stay harmless: a page draws only its own rows' pins.
fn pin_key_ok(k: &str) -> bool {
    match k.split_once(':') {
        Some(("cal" | "ab", id)) => id.len() <= 300 && id.split_once('/').is_some_and(|(p, c)| !p.is_empty() && !c.is_empty()),
        Some(("hol", h)) => h == "public" || h == "school",
        _ => false,
    }
}

/// At most this many pins per user; a new one past it drops the oldest (pins of deleted collections are never cleared).
const MAX_PINS: usize = 64;

pub(super) async fn filter_pin_save(State(state): State<SharedState>, user: Principal, _: csrf::Token, crate::extract::Json(pin): crate::extract::Json<FilterPin>) -> AppResult<axum::Json<Vec<String>>> {
    if !pin_key_ok(&pin.key) {
        return Err(AppError::bad("err-input-body"));
    }
    let _lock = state.guards.prefs(&user.id).await; // until prefs::set: no lost update
    let mut p = prefs::get(&state.db, &user.id).await?;
    let mut pins = p.pins();
    pins.retain(|k| *k != pin.key);
    if pin.on {
        if pins.len() >= MAX_PINS {
            pins.remove(0);
        }
        pins.push(pin.key);
    }
    p.filter_pins_json = Some(serde_json::to_string(&pins).unwrap());
    prefs::set(&state.db, &user.id, &p).await?;
    Ok(axum::Json(pins))
}

/// Contact list-view field choice (JSON array from app.js), stored per user.
pub(super) async fn list_fields_save(State(state): State<SharedState>, user: Principal, _: csrf::Token, crate::extract::Json(fields): crate::extract::Json<Vec<String>>) -> AppResult<axum::Json<Vec<String>>> {
    let fields: Vec<String> = fields.into_iter().filter(|c| crate::routes::contacts::ALL_LIST_FIELDS.contains(&c.as_str())).collect();
    let _lock = state.guards.prefs(&user.id).await; // until prefs::set: no lost update
    let mut p = prefs::get(&state.db, &user.id).await?;
    p.contact_list_fields_json = Some(serde_json::to_string(&fields).unwrap());
    prefs::set(&state.db, &user.id, &p).await?;
    Ok(axum::Json(fields))
}
