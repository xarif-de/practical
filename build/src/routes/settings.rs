//! /settings — groups, collections, app tokens, trash, preferences, import/export. All mutations are POST
//! forms with CSRF (the form field, or X-CSRF-Token for the JSON saves).
//!
//! Every form handler answers through one envelope, [`form`] → [`finish`]: a redirect back to the page
//! section with a flash (or JSON for a fetch). The forms live in the sub-modules (groups, collections,
//! tokens, preferences, transfer = export and import). The two routes that import into a new collection
//! accept a 32 MiB body.

use crate::csrf;
use crate::ctx::{PageCtx, set_flash};
use crate::domain::collections::{self, AbInfo, CalInfo, NewCalendar, Owner, PALETTE};
use crate::domain::events;
use crate::domain::groups::{self, GroupInfo, RemoveOutcome, TrashedGroup};
use crate::domain::history::{self, Actor};
use crate::domain::history_text::Store;
use crate::domain::holidays::{self, Region};
use crate::domain::prefs::{self, Prefs};
use crate::domain::tokens;
use crate::error::{AppError, AppResult};
use crate::state::SharedState;
use askama::Template;
use askama_web::WebTemplate;
use axum::extract::State;
use crate::extract::{Form, Path};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::Router;
use rustical_store::auth::{AppToken, Principal};
use serde::Deserialize;
use std::collections::HashMap;
use tower_sessions::Session;

mod group_forms;
mod collection_forms;
mod token_forms;
mod pref_forms;
mod transfer;
use group_forms::*;
use collection_forms::*;
use token_forms::*;
use pref_forms::*;
use transfer::*;

pub(crate) const SESSION_NEW_TOKEN: &str = "new_token";

#[derive(Template, WebTemplate)]
#[template(path = "settings.html")]
pub struct SettingsPage {
    pub ctx: PageCtx,
    pub groups: Vec<GroupInfo>,
    pub individuals: Vec<Principal>,
    pub trashed: Vec<TrashedGroup>,
    pub calendars: Vec<CalInfo>,
    pub addressbooks: Vec<AbInfo>,
    pub deleted_calendars: Vec<CalInfo>,
    pub deleted_addressbooks: Vec<AbInfo>,
    pub tokens: Vec<AppToken>,
    pub new_token: Option<String>,
    pub prefs: Prefs,
    pub owners: Vec<Owner>,
    pub palette: Vec<String>,
    pub retention_days: u32,
    pub default_timezone: String,
    /// the regions of public / school holidays (codes, in the user's order), comma-joined for the form
    pub public_regions: String,
    pub school_regions: String,
    /// name and path of every chosen region ("Sachsen", "Deutschland")
    pub region_names: HashMap<String, (String, String)>,
    /// the pickers' data (countries, time zones, chosen regions' names) as JSON for app.js
    pub picker_json: String,
    /// `:root { --hol; --fer }` from the user's holiday colours ("" = the theme's)
    pub holiday_css: String,
    /// the vacation rule as the page edits it: its kind and one value per kind (the other two are
    /// the defaults the page offers when the kind is switched)
    pub vacation_kind: String,
    pub vacation_category: String,
    pub vacation_calendar: String,
    pub vacation_title: String,
    /// the calendars a rule can name: "principal/id" and the name
    pub vacation_calendars: Vec<(String, String)>,
    /// the rule names a calendar the user no longer sees: kept as an option so a save does not change it
    pub vacation_calendar_unknown: bool,
    /// the rule in the overview row: „Urlaub“, „Privat“ (a calendar's name), /^Urlaub/
    pub vacation_summary: String,
    /// reminder strip snap points as the server writes them ("2 min, 1 h, …"), and the built-in lists
    pub alarm_snaps_start: String,
    pub alarm_snaps_end: String,
    pub alarm_snaps_default_start: String,
    pub alarm_snaps_default_end: String,
}

/// The rule as the page edits it: (kind, value). A rule from before v2 ("category or title word")
/// becomes the category when one of that name is in use, else the whole word in the title.
async fn vacation_rule(state: &SharedState, user: &Principal, p: &Prefs) -> AppResult<(String, String)> {
    let r = prefs::VacationRule::of(p);
    if !r.legacy() {
        return Ok((if matches!(r.kind.as_str(), "calendar" | "title") { r.kind } else { "category".into() }, r.value));
    }
    let v = r.value.trim();
    if let Some((name, _)) = events::category_counts(state, user, "event", chrono_tz::UTC).await?.into_iter().find(|(c, _)| c.to_lowercase() == v.to_lowercase()) {
        return Ok(("category".into(), name));
    }
    Ok(("title".into(), if v.is_empty() { String::new() } else { format!(r"\b{}\b", regex::escape(v)) }))
}

impl SettingsPage {
    pub fn lang_is(&self, l: &str) -> bool {
        self.prefs.lang.as_deref().unwrap_or(&self.ctx.lang) == l
    }
    /// a region list as its row shows it: "Sachsen, Bayern", "3 regions" from three on, "none"
    pub fn regions_text(&self, school: bool) -> String {
        let list: Vec<&str> = if school { &self.school_regions } else { &self.public_regions }.split(',').filter(|c| !c.is_empty()).collect();
        match list.len() {
            0 => self.ctx.t("set-regions-none"),
            1 | 2 => list.iter().map(|c| self.region_names.get(*c).map(|n| n.0.clone()).unwrap_or_else(|| c.to_string())).collect::<Vec<_>>().join(", "),
            n => self.ctx.tn("set-regions-n", "n", n as i64),
        }
    }
    /// the colour pickers' values: the user's colour, else the (light) theme's
    /// "Holidays: OpenHolidays, Nager.Date · School holidays: OpenHolidays"
    pub fn sources_text(&self) -> String {
        // over every chosen country, in chain order
        let mut public = vec![];
        let mut school = vec![];
        for code in self.public_regions.split(',').chain(self.school_regions.split(',')).filter(|c| !c.is_empty()) {
            let (p, s) = holidays::sources(holidays::split(code).0);
            public.extend(p);
            school.extend(s);
        }
        for v in [&mut public, &mut school] {
            v.sort_by_key(|x| usize::from(*x != "OpenHolidays"));
            v.dedup();
        }
        let none = self.ctx.t("set-src-none");
        let list = |v: Vec<&str>| if v.is_empty() { none.clone() } else { v.join(" → ") };
        let mut args = fluent_bundle::FluentArgs::new();
        args.set("public", list(public));
        args.set("school", list(school));
        self.ctx.i18n.t_args(&self.ctx.lang, "set-sources", Some(&args))
    }
    /// the time zone as shown in its row: the chosen one or "Default (Europe/Berlin)"
    pub fn tz_name(&self) -> String {
        match &self.prefs.timezone {
            Some(t) => t.replace('_', " "),
            None => self.ctx.t1("set-tz-default", "tz", &self.default_timezone),
        }
    }
    pub fn scheme_is(&self, s: &str) -> bool {
        self.prefs.scheme.as_deref().unwrap_or("auto") == s
    }
    /// VEVENT/VTODO/VJOURNAL as words ("Termine, Aufgaben")
    pub fn comps(&self, c: &[String]) -> String {
        c.iter().map(|k| match k.as_str() { "VEVENT" => self.ctx.t("set-comp-event"), "VTODO" => self.ctx.t("set-comp-todo"), "VJOURNAL" => self.ctx.t("set-comp-journal"), o => o.to_string() }).collect::<Vec<_>>().join(", ")
    }
}

async fn page(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx) -> AppResult<SettingsPage> {
    let new_token: Option<String> = if session.get_value(SESSION_NEW_TOKEN).await?.is_some() { session.remove(SESSION_NEW_TOKEN).await? } else { None }; // no session write without one
    let p = prefs::get(&state.db, &user.id).await?;
    let snaps = events::alarm_snaps(p.alarm_snaps_json.as_deref());
    let (public_regions, school_regions) = (p.regions(&state.config, false), p.regions(&state.config, true));
    let mut region_names = HashMap::new();
    for code in public_regions.iter().chain(&school_regions) {
        if !region_names.contains_key(code) {
            region_names.insert(code.clone(), holidays::describe(&state, code, &ctx.lang).await?);
        }
    }
    let cals_all = collections::calendars_for(&state, &user, false).await?;
    let (vac_kind, vac_value) = vacation_rule(&state, &user, &p).await?;
    let vac_summary = match vac_kind.as_str() {
        "title" => format!("/{vac_value}/"),
        "calendar" => format!("„{}“", cals_all.iter().find(|c| format!("{}/{}", c.principal, c.id) == vac_value).map_or(vac_value.as_str(), |c| c.displayname.as_str())),
        _ => format!("„{vac_value}“"),
    };
    Ok(SettingsPage {
        alarm_snaps_start: events::fmt_offsets(&snaps.start),
        alarm_snaps_end: events::fmt_offsets(&snaps.end),
        alarm_snaps_default_start: events::fmt_offsets(&events::AlarmSnaps::default().start),
        alarm_snaps_default_end: events::fmt_offsets(&events::AlarmSnaps::default().end),
        groups: groups::list_for_user(&state, &user).await?,
        individuals: groups::individuals(&state).await?,
        trashed: groups::trashed_for_user(&state, &user).await?,
        calendars: cals_all.clone(),
        addressbooks: collections::addressbooks_for(&state, &user, false).await?,
        deleted_calendars: collections::calendars_for(&state, &user, true).await?,
        deleted_addressbooks: collections::addressbooks_for(&state, &user, true).await?,
        tokens: tokens::list(&state, &user).await?,
        new_token,
        prefs: p.clone(),
        owners: collections::owners(&state, &user).await?,
        palette: PALETTE.iter().map(|s| s.to_string()).collect(),
        retention_days: state.config.trash_retention_days,
        default_timezone: state.config.default_timezone.clone(),
        picker_json: crate::render::script_json(serde_json::json!({ "countries": holidays::countries(&ctx.lang), "zones": timezones(), "names": region_names }).to_string()),
        public_regions: public_regions.join(","),
        school_regions: school_regions.join(","),
        region_names,
        holiday_css: p.holiday_css(),
        vacation_summary: vac_summary,
        vacation_kind: vac_kind.clone(),
        vacation_category: if vac_kind == "category" { vac_value.clone() } else { String::new() },
        vacation_title: if vac_kind == "title" { vac_value.clone() } else if vac_kind == "category" && !vac_value.is_empty() { format!(r"\b{}\b", regex::escape(&vac_value)) } else { r"\bUrlaub\b".into() },
        vacation_calendar_unknown: vac_kind == "calendar" && !vac_value.is_empty() && !cals_all.iter().any(|c| format!("{}/{}", c.principal, c.id) == vac_value),
        vacation_calendar: if vac_kind == "calendar" { vac_value } else { String::new() },
        vacation_calendars: cals_all.iter().filter(|c| c.components.is_empty() || c.components.iter().any(|x| x == "VEVENT")).map(|c| (format!("{}/{}", c.principal, c.id), c.displayname.clone())).collect(),
        ctx,
    })
}

/// The IANA zones a person would pick, grouped by continent/ocean (legacy aliases such as
/// "US/Eastern" or "Etc/GMT+1" left out).
fn timezones() -> Vec<(String, Vec<(String, String)>)> {
    const AREAS: [&str; 10] = ["Africa", "America", "Antarctica", "Arctic", "Asia", "Atlantic", "Australia", "Europe", "Indian", "Pacific"];
    let mut out: Vec<(String, Vec<(String, String)>)> = AREAS.iter().map(|a| (a.to_string(), vec![])).collect();
    for tz in chrono_tz::TZ_VARIANTS {
        let name = tz.name();
        if let Some((area, rest)) = name.split_once('/')
            && let Some(g) = out.iter_mut().find(|(a, _)| a == area)
        {
            g.1.push((name.to_string(), rest.replace('_', " ").replace('/', " / ")));
        }
    }
    out.retain(|(_, v)| !v.is_empty());
    out
}

/// GET /api/holidays/countries — the countries a holiday source covers, in the page language.
async fn api_countries(_user: Principal, ctx: PageCtx) -> axum::Json<Vec<Region>> {
    axum::Json(holidays::countries(&ctx.lang))
}

#[derive(Deserialize)]
struct CountryQuery {
    country: String,
}

/// GET /api/holidays/subdivisions?country=CH — its subdivisions, all levels, parents first.
async fn api_subdivisions(State(state): State<SharedState>, _user: Principal, ctx: PageCtx, crate::extract::Query(q): crate::extract::Query<CountryQuery>) -> AppResult<axum::Json<Vec<Region>>> {
    Ok(axum::Json(holidays::subdivisions(&state, &q.country.to_uppercase(), &ctx.lang).await?))
}

/// Redirect back to the section. A success says something only when the reloaded
/// page cannot show it (a save leaves the same values on screen); store errors
/// become error flashes so the user never lands on a bare error page after a form post.
async fn finish(state: &SharedState, session: &Session, ctx: &PageCtx, result: AppResult<Option<&str>>, anchor: &str) -> Response {
    // a form saved by fetch (settings autosave) gets its message back instead of a flash for the next page
    if ctx.wants_json {
        let (ok, msg) = match result {
            Ok(key) => (true, key.map(|k| state.i18n.t(&ctx.lang, k))),
            Err(e) => (false, Some(failed(state, ctx, &e))),
        };
        return axum::Json(serde_json::json!({ "ok": ok, "msg": msg })).into_response();
    }
    match result {
        Ok(None) => {}
        Ok(Some(key)) => set_flash(session, "ok", state.i18n.t(&ctx.lang, key)).await,
        Err(e) => set_flash(session, "error", failed(state, ctx, &e)).await,
    }
    Redirect::to(&format!("/settings#{anchor}")).into_response()
}

/// A settings form action: the form's CSRF field, then `op` (an async block does nothing before it is
/// awaited, so nothing of it runs on a bad token), answered by `finish`.
async fn form(state: &SharedState, session: &Session, ctx: &PageCtx, token: &str, anchor: &str, op: impl Future<Output = AppResult<Option<&'static str>>>) -> Response {
    let r = match csrf::check(session, token).await {
        Ok(()) => op.await,
        Err(e) => Err(e),
    };
    finish(state, session, ctx, r, anchor).await
}

/// The user's message for a failed settings action (`AppError::user_message`); a server-side failure is
/// logged with its own text, which the user does not see.
fn failed(state: &SharedState, ctx: &PageCtx, e: &AppError) -> String {
    if e.user_text().is_some_and(|u| u.key == "flash-failed") {
        tracing::error!("{e}");
    }
    e.user_message(&state.i18n, &ctx.lang)
}

#[derive(Deserialize)]
struct Csrf {
    csrf: String,
}

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/settings", get(page))
        .route("/settings/groups", post(group_create))
        .route("/settings/groups/{gid}/members", post(group_add_member))
        .route("/settings/groups/{gid}/members/{member}/remove", post(group_remove_member))
        .route("/settings/groups/{gid}/leave", post(group_leave))
        .route("/settings/groups/{gid}/delete", post(group_delete))
        .route("/settings/trash/groups/{gid}/restore", post(group_restore))
        .route("/settings/trash/groups/{gid}/purge", post(group_purge))
        .route("/settings/calendars", post(calendar_create))
        .route("/settings/calendars/{owner}/{id}/update", post(calendar_update))
        .route("/settings/calendars/{owner}/{id}/delete", post(calendar_delete))
        .route("/settings/calendars/{owner}/{id}/restore", post(calendar_restore))
        .route("/settings/calendars/{owner}/{id}/purge", post(calendar_purge))
        .route("/settings/addressbooks", post(addressbook_create))
        .route("/settings/addressbooks/{owner}/{id}/update", post(addressbook_update))
        .route("/settings/addressbooks/{owner}/{id}/delete", post(addressbook_delete))
        .route("/settings/addressbooks/{owner}/{id}/restore", post(addressbook_restore))
        .route("/settings/addressbooks/{owner}/{id}/purge", post(addressbook_purge))
        .route("/settings/tokens", post(token_create))
        .route("/settings/tokens/{id}/delete", post(token_delete))
        .route("/settings/prefs", post(prefs_save))
        .route("/settings/holiday-display", post(holiday_display_save))
        .route("/settings/scheme", post(scheme_save))
        .route("/api/holidays/countries", get(api_countries))
        .route("/api/holidays/subdivisions", get(api_subdivisions))
        .route("/settings/columns", post(columns_save))
        .route("/settings/list-fields", post(list_fields_save))
        .route("/settings/task-columns", post(task_columns_save))
        .route("/settings/export/calendars/{owner}/{id}", get(export_calendar))
        .route("/settings/export/addressbooks/{owner}/{id}", get(export_addressbook))
        .route("/settings/import/calendars", post(import_new_calendar).layer(axum::extract::DefaultBodyLimit::max(32 * 1024 * 1024)))
        .route("/settings/import/calendars/{owner}/{id}", post(import_calendar))
        .route("/settings/import/addressbooks", post(import_new_addressbook).layer(axum::extract::DefaultBodyLimit::max(32 * 1024 * 1024)))
        .route("/settings/import/addressbooks/{owner}/{id}", post(import_addressbook))
}

#[cfg(test)]
mod pinned;
