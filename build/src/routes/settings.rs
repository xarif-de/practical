//! /settings — groups (D2/D3/D6), collections (D4/D5), app tokens (D8),
//! trash, preferences (D9/D13/D14). All mutations are POST forms with CSRF.

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
use axum::extract::{Path, State};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Form, Router};
use rustical_store::auth::{AppToken, Principal};
use serde::Deserialize;
use std::collections::HashMap;
use tower_sessions::Session;

const SESSION_NEW_TOKEN: &str = "new_token";

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
    let new_token: Option<String> = session.remove(SESSION_NEW_TOKEN).await?;
    let snaps = events::alarm_snaps(prefs::get(&state.db, &user.id).await?.alarm_snaps_json.as_deref());
    let p = prefs::get(&state.db, &user.id).await?;
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
        calendars: collections::calendars_for(&state, &user, false).await?,
        addressbooks: collections::addressbooks_for(&state, &user, false).await?,
        deleted_calendars: collections::calendars_for(&state, &user, true).await?,
        deleted_addressbooks: collections::addressbooks_for(&state, &user, true).await?,
        tokens: tokens::list(&state, &user).await?,
        new_token,
        prefs: prefs::get(&state.db, &user.id).await?,
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
async fn api_subdivisions(State(state): State<SharedState>, _user: Principal, ctx: PageCtx, axum::extract::Query(q): axum::extract::Query<CountryQuery>) -> AppResult<axum::Json<Vec<Region>>> {
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
            Err(AppError::Forbidden) => (false, Some(state.i18n.t(&ctx.lang, "flash-forbidden"))),
            Err(AppError::NotFound) => (false, Some(state.i18n.t(&ctx.lang, "flash-not-found"))),
            Err(AppError::BadRequest(m) | AppError::Conflict(m)) => (false, Some(m)),
            Err(e) => (false, Some(e.to_string())),
        };
        return axum::Json(serde_json::json!({ "ok": ok, "msg": msg })).into_response();
    }
    match result {
        Ok(None) => {}
        Ok(Some(key)) => set_flash(session, "ok", state.i18n.t(&ctx.lang, key)).await,
        Err(AppError::Forbidden) => set_flash(session, "error", state.i18n.t(&ctx.lang, "flash-forbidden")).await,
        Err(AppError::NotFound) => set_flash(session, "error", state.i18n.t(&ctx.lang, "flash-not-found")).await,
        Err(AppError::BadRequest(m) | AppError::Conflict(m)) => set_flash(session, "error", m).await,
        Err(e) => set_flash(session, "error", e.to_string()).await,
    }
    Redirect::to(&format!("/settings#{anchor}")).into_response()
}

#[derive(Deserialize)]
struct Csrf {
    csrf: String,
}

#[derive(Deserialize)]
struct NameForm {
    csrf: String,
    name: String,
}

async fn group_create(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Form(f): Form<NameForm>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        groups::create(&state, &user, &f.name).await?;
        Ok(None)
    }
    .await;
    finish(&state, &session, &ctx, r, "groups").await
}

#[derive(Deserialize)]
struct MemberForm {
    csrf: String,
    member: String,
}

async fn group_add_member(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path(gid): Path<String>, Form(f): Form<MemberForm>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        groups::add_member(&state, &user, &gid, &f.member).await?;
        Ok(None)
    }
    .await;
    finish(&state, &session, &ctx, r, "groups").await
}

async fn group_remove_member(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path((gid, member)): Path<(String, String)>, Form(f): Form<Csrf>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        Ok(match groups::remove_member(&state, &user, &gid, &member).await? {
            RemoveOutcome::Removed => None,
            // the last member leaving trashes the group: the one outcome the page does not show
            RemoveOutcome::GroupTrashed => Some("flash-group-trashed"),
        })
    }
    .await;
    finish(&state, &session, &ctx, r, "groups").await
}

async fn group_leave(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path(gid): Path<String>, Form(f): Form<Csrf>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        Ok(match groups::remove_member(&state, &user, &gid, &user.id).await? {
            RemoveOutcome::Removed => None,
            // the last member leaving trashes the group: the one outcome the page does not show
            RemoveOutcome::GroupTrashed => Some("flash-group-trashed"),
        })
    }
    .await;
    finish(&state, &session, &ctx, r, "groups").await
}

#[derive(Deserialize)]
struct ConfirmForm {
    csrf: String,
    confirm: String,
}

async fn group_delete(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path(gid): Path<String>, Form(f): Form<ConfirmForm>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        groups::trash(&state, &user, &gid, Some(&f.confirm)).await.map_err(|e| match e {
            AppError::BadRequest(_) => AppError::BadRequest(state.i18n.t(&ctx.lang, "flash-confirm-mismatch")),
            e => e,
        })?;
        Ok(None)
    }
    .await;
    finish(&state, &session, &ctx, r, "groups").await
}

async fn group_restore(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path(gid): Path<String>, Form(f): Form<Csrf>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        groups::restore(&state, &user, &gid).await?;
        Ok(None)
    }
    .await;
    finish(&state, &session, &ctx, r, "trash").await
}

async fn group_purge(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path(gid): Path<String>, Form(f): Form<ConfirmForm>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        if f.confirm.trim() != gid {
            return Err(AppError::BadRequest(state.i18n.t(&ctx.lang, "flash-confirm-mismatch")));
        }
        groups::purge_by_member(&state, &user, &gid).await?;
        Ok(None)
    }
    .await;
    finish(&state, &session, &ctx, r, "trash").await
}

#[derive(Deserialize)]
struct CalendarForm {
    csrf: String,
    owner: String,
    name: String,
    #[serde(default)]
    color: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    comp_event: Option<String>,
    #[serde(default)]
    comp_todo: Option<String>,
    #[serde(default)]
    comp_journal: Option<String>,
    #[serde(default)]
    timezone: String,
}

async fn calendar_create(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Form(f): Form<CalendarForm>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        let mut comps = vec![];
        if f.comp_event.is_some() { comps.push("VEVENT".to_string()); }
        if f.comp_todo.is_some() { comps.push("VTODO".to_string()); }
        if f.comp_journal.is_some() { comps.push("VJOURNAL".to_string()); }
        collections::create_calendar(
            &state,
            &user,
            NewCalendar {
                owner: f.owner,
                name: f.name,
                color: f.color,
                description: f.description,
                components: collections::parse_components(&comps),
                timezone: if f.timezone.is_empty() { state.config.default_timezone.clone() } else { f.timezone },
            },
        )
        .await?;
        Ok(None)
    }
    .await;
    finish(&state, &session, &ctx, r, "collections").await
}

#[derive(Deserialize)]
struct CalendarUpdateForm {
    csrf: String,
    name: String,
    #[serde(default)]
    color: String,
    #[serde(default)]
    description: String,
}

async fn calendar_update(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path((owner, id)): Path<(String, String)>, Form(f): Form<CalendarUpdateForm>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        collections::update_calendar(&state, &user, &owner, &id, &f.name, &f.color, &f.description).await?;
        Ok(Some("flash-saved"))
    }
    .await;
    finish(&state, &session, &ctx, r, "collections").await
}

async fn calendar_delete(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path((owner, id)): Path<(String, String)>, Form(f): Form<Csrf>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        collections::delete_calendar(&state, &user, &owner, &id, false).await?;
        Ok(None)
    }
    .await;
    finish(&state, &session, &ctx, r, "collections").await
}

async fn calendar_restore(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path((owner, id)): Path<(String, String)>, Form(f): Form<Csrf>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        collections::restore_calendar(&state, &user, &owner, &id).await?;
        Ok(None)
    }
    .await;
    finish(&state, &session, &ctx, r, "trash").await
}

async fn calendar_purge(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path((owner, id)): Path<(String, String)>, Form(f): Form<Csrf>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        collections::delete_calendar(&state, &user, &owner, &id, true).await?;
        Ok(None)
    }
    .await;
    finish(&state, &session, &ctx, r, "trash").await
}

#[derive(Deserialize)]
struct AddressbookForm {
    csrf: String,
    owner: String,
    name: String,
    color: String,
    #[serde(default)]
    description: String,
}

async fn addressbook_create(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Form(f): Form<AddressbookForm>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        collections::create_addressbook(&state, &user, &f.owner, &f.name, &f.color, &f.description).await?;
        Ok(None)
    }
    .await;
    finish(&state, &session, &ctx, r, "collections").await
}

#[derive(Deserialize)]
struct AddressbookUpdateForm {
    csrf: String,
    name: String,
    color: String,
    #[serde(default)]
    description: String,
}

async fn addressbook_update(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path((owner, id)): Path<(String, String)>, Form(f): Form<AddressbookUpdateForm>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        collections::update_addressbook(&state, &user, &owner, &id, &f.name, &f.color, &f.description).await?;
        Ok(Some("flash-saved"))
    }
    .await;
    finish(&state, &session, &ctx, r, "collections").await
}

async fn addressbook_delete(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path((owner, id)): Path<(String, String)>, Form(f): Form<Csrf>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        collections::delete_addressbook(&state, &user, &owner, &id, false).await?;
        Ok(None)
    }
    .await;
    finish(&state, &session, &ctx, r, "collections").await
}

async fn addressbook_restore(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path((owner, id)): Path<(String, String)>, Form(f): Form<Csrf>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        collections::restore_addressbook(&state, &user, &owner, &id).await?;
        Ok(None)
    }
    .await;
    finish(&state, &session, &ctx, r, "trash").await
}

async fn addressbook_purge(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path((owner, id)): Path<(String, String)>, Form(f): Form<Csrf>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        collections::delete_addressbook(&state, &user, &owner, &id, true).await?;
        Ok(None)
    }
    .await;
    finish(&state, &session, &ctx, r, "trash").await
}

async fn token_create(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Form(f): Form<NameForm>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        let t = tokens::create(&state, &user, &f.name).await?;
        session.insert(SESSION_NEW_TOKEN, t).await?;
        Ok(None)
    }
    .await;
    finish(&state, &session, &ctx, r, "account").await
}

async fn token_delete(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path(id): Path<String>, Form(f): Form<Csrf>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        tokens::delete(&state, &user, &id).await?;
        Ok(None)
    }
    .await;
    finish(&state, &session, &ctx, r, "account").await
}

#[derive(Deserialize)]
struct PrefsForm {
    csrf: String,
    #[serde(default)]
    lang: String,
    #[serde(default)]
    scheme: String,
    #[serde(default)]
    timezone: String,
    #[serde(default)]
    holiday_color: String,
    #[serde(default)]
    school_color: String,
    /// comma lists of region codes; absent = unchanged
    public_regions: Option<String>,
    school_regions: Option<String>,
    #[serde(default)]
    vacation_days: String,
    #[serde(default)]
    vacation_kind: String,
    #[serde(default)]
    vacation_category: String,
    #[serde(default)]
    vacation_calendar: String,
    #[serde(default)]
    vacation_title: String,
    #[serde(default)]
    alarm_snaps_start: String,
    #[serde(default)]
    alarm_snaps_end: String,
    /// days the search history is kept (prefs::HISTORY_DAYS); absent = unchanged
    search_history_days: Option<String>,
}

async fn prefs_save(State(state): State<SharedState>, user: Principal, session: Session, mut ctx: PageCtx, Form(f): Form<PrefsForm>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        let mut p = prefs::get(&state.db, &user.id).await?;
        p.lang = Some(f.lang.clone()).filter(|l| crate::i18n::LANGS.contains(&l.as_str()));
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
        p.vacation_days = f.vacation_days.trim().parse::<i64>().ok().filter(|d| (0..=366).contains(d));
        // vacation rule: the chosen kind with its own field; a title pattern must parse
        let (kind, value) = match f.vacation_kind.as_str() {
            "calendar" => ("calendar", f.vacation_calendar.trim()),
            "title" => ("title", f.vacation_title.trim()),
            _ => ("category", f.vacation_category.trim()),
        };
        if kind == "title" && !value.is_empty() {
            prefs::VacationRule::title_regex(value).map_err(|e| AppError::BadRequest(format!("{}: {e}", ctx.t("vacation-title-bad"))))?;
        }
        p.vacation_rule_json = Some(serde_json::to_string(&prefs::VacationRule { v: 2, kind: kind.into(), value: value.into() }).unwrap());
        // reminder strip snap points: an empty list falls back to the built-in one; a bad token rejects the form
        let start = events::parse_offsets(&f.alarm_snaps_start).map_err(|t| AppError::BadRequest(format!("snap point: {t}")))?;
        let end = events::parse_offsets(&f.alarm_snaps_end).map_err(|t| AppError::BadRequest(format!("snap point: {t}")))?;
        let snaps = events::AlarmSnaps { start: if start.is_empty() { events::AlarmSnaps::default().start } else { start }, end };
        p.alarm_snaps_json = if snaps == events::AlarmSnaps::default() { None } else { Some(serde_json::to_string(&snaps).unwrap()) };
        if let Some(d) = f.search_history_days.as_deref().and_then(|d| d.parse::<i64>().ok()).filter(|d| prefs::HISTORY_DAYS.contains(d)) {
            p.search_history_days = Some(d);
        }
        prefs::set(&state.db, &user.id, &p).await?;
        if let Some(l) = &p.lang {
            session.insert(crate::ctx::SESSION_LANG, l).await?;
        }
        // the answer ("Saved.") speaks the language just chosen
        if let Some(l) = p.lang.clone() {
            ctx.lang = l;
        }
        Ok(Some("flash-saved"))
    }
    .await;
    finish(&state, &session, &ctx, r, "prefs").await
}

#[derive(Deserialize)]
struct HolidayDisplay {
    public: Option<bool>,
    /// absent where the region has no school holidays (the switch is not shown): kept as it was
    school: Option<bool>,
}

/// Show public holidays / school holidays in the calendar (its filter popover, app.js).
async fn holiday_display_save(State(state): State<SharedState>, user: Principal, session: Session, headers: axum::http::HeaderMap, axum::Json(d): axum::Json<HolidayDisplay>) -> AppResult<axum::http::StatusCode> {
    csrf::check(&session, &csrf::header_token(&headers)).await?;
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
struct SchemeChoice {
    scheme: String,
}

/// The colour scheme from the button in the bar (app.js): auto | light | dark.
async fn scheme_save(State(state): State<SharedState>, user: Principal, session: Session, headers: axum::http::HeaderMap, axum::Json(d): axum::Json<SchemeChoice>) -> AppResult<axum::http::StatusCode> {
    csrf::check(&session, &csrf::header_token(&headers)).await?;
    let mut p = prefs::get(&state.db, &user.id).await?;
    p.scheme = Some(d.scheme).filter(|s| s == "light" || s == "dark");
    prefs::set(&state.db, &user.id, &p).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// Contact table column choice (JSON array from app.js), stored per user.
async fn columns_save(State(state): State<SharedState>, user: Principal, session: Session, headers: axum::http::HeaderMap, axum::Json(cols): axum::Json<Vec<String>>) -> AppResult<axum::Json<Vec<String>>> {
    csrf::check(&session, &csrf::header_token(&headers)).await?;
    let cols: Vec<String> = cols.into_iter().filter(|c| crate::routes::contacts::ALL_COLUMNS.contains(&c.as_str())).collect();
    let mut p = prefs::get(&state.db, &user.id).await?;
    p.contact_columns_json = Some(serde_json::to_string(&cols).unwrap());
    prefs::set(&state.db, &user.id, &p).await?;
    Ok(axum::Json(cols))
}

/// Task table column choice (JSON array from app.js), stored per user.
async fn task_columns_save(State(state): State<SharedState>, user: Principal, session: Session, headers: axum::http::HeaderMap, axum::Json(cols): axum::Json<Vec<String>>) -> AppResult<axum::Json<Vec<String>>> {
    csrf::check(&session, &csrf::header_token(&headers)).await?;
    let cols: Vec<String> = cols.into_iter().filter(|c| crate::routes::tasks::ALL_COLUMNS.contains(&c.as_str())).collect();
    let mut p = prefs::get(&state.db, &user.id).await?;
    p.task_columns_json = Some(serde_json::to_string(&cols).unwrap());
    prefs::set(&state.db, &user.id, &p).await?;
    Ok(axum::Json(cols))
}

/// Contact list-view field choice (JSON array from app.js), stored per user.
async fn list_fields_save(State(state): State<SharedState>, user: Principal, session: Session, headers: axum::http::HeaderMap, axum::Json(fields): axum::Json<Vec<String>>) -> AppResult<axum::Json<Vec<String>>> {
    csrf::check(&session, &csrf::header_token(&headers)).await?;
    let fields: Vec<String> = fields.into_iter().filter(|c| crate::routes::contacts::ALL_LIST_FIELDS.contains(&c.as_str())).collect();
    let mut p = prefs::get(&state.db, &user.id).await?;
    p.contact_list_fields_json = Some(serde_json::to_string(&fields).unwrap());
    prefs::set(&state.db, &user.id, &p).await?;
    Ok(axum::Json(fields))
}

fn download(name: &str, ext: &str, mime: &'static str, body: String) -> Response {
    let safe: String = name.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    (
        [(axum::http::header::CONTENT_TYPE, mime.to_string()), (axum::http::header::CONTENT_DISPOSITION, format!("attachment; filename=\"{safe}.{ext}\""))],
        body,
    )
        .into_response()
}

async fn export_calendar(State(state): State<SharedState>, user: Principal, Path((owner, id)): Path<(String, String)>) -> AppResult<Response> {
    let (name, ics) = crate::domain::transfer::export_calendar(&state, &user, &owner, &id).await?;
    Ok(download(&name, "ics", "text/calendar; charset=utf-8", ics))
}

async fn export_addressbook(State(state): State<SharedState>, user: Principal, Path((owner, id)): Path<(String, String)>) -> AppResult<Response> {
    let (name, vcf) = crate::domain::transfer::export_addressbook(&state, &user, &owner, &id).await?;
    Ok(download(&name, "vcf", "text/vcard; charset=utf-8", vcf))
}

#[derive(serde::Serialize)]
struct Imported {
    imported: usize,
}

/// Raw .ics body (app.js reads the chosen file and POSTs it — no multipart dependency).
async fn import_calendar(State(state): State<SharedState>, user: Principal, session: Session, headers: axum::http::HeaderMap, Path((owner, id)): Path<(String, String)>, body: axum::body::Bytes) -> AppResult<axum::Json<Imported>> {
    csrf::check(&session, &csrf::header_token(&headers)).await?;
    let mut g = history::begin(&state, Actor::import(&user, "")).await;
    g.coll(Store::Cal, &owner, &id);
    let imported = crate::domain::transfer::import_calendar(&state, &user, &owner, &id, &body).await?;
    g.commit().await;
    Ok(axum::Json(Imported { imported }))
}

#[derive(serde::Deserialize)]
struct NewImportQ {
    owner: String,
    name: String,
    #[serde(default)]
    color: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    timezone: String,
    /// "VEVENT,VTODO" — the file's own types are added on the server
    #[serde(default)]
    components: String,
}

#[derive(serde::Serialize)]
struct ImportedNew {
    id: String,
    owner: String,
    imported: usize,
}

/// A new calendar from an .ics file: the metadata in the query (the page prefills it from the file's
/// X-WR-* lines), the raw file as the body.
async fn import_new_calendar(State(state): State<SharedState>, user: Principal, session: Session, headers: axum::http::HeaderMap, axum::extract::Query(q): axum::extract::Query<NewImportQ>, body: axum::body::Bytes) -> AppResult<axum::Json<ImportedNew>> {
    csrf::check(&session, &csrf::header_token(&headers)).await?;
    let comps: Vec<String> = q.components.split(',').map(|c| c.trim().to_string()).filter(|c| !c.is_empty()).collect();
    let owner = q.owner.clone();
    let n = NewCalendar {
        owner: q.owner,
        name: q.name,
        color: q.color,
        description: q.description,
        components: if comps.is_empty() { vec![] } else { collections::parse_components(&comps) },
        timezone: if q.timezone.trim().is_empty() { state.config.default_timezone.clone() } else { q.timezone },
    };
    let mut g = history::begin(&state, Actor::import(&user, n.name.trim())).await;
    let (id, imported) = crate::domain::transfer::import_new_calendar(&state, &user, n, &body).await?;
    g.coll(Store::Cal, &owner, &id);
    g.commit().await;
    Ok(axum::Json(ImportedNew { id, owner, imported }))
}

#[derive(serde::Deserialize)]
struct NewAbImportQ {
    owner: String,
    name: String,
    color: String,
    #[serde(default)]
    description: String,
}

/// A new address book from a .vcf file: the metadata in the query, the raw file as the body.
async fn import_new_addressbook(State(state): State<SharedState>, user: Principal, session: Session, headers: axum::http::HeaderMap, axum::extract::Query(q): axum::extract::Query<NewAbImportQ>, body: axum::body::Bytes) -> AppResult<axum::Json<ImportedNew>> {
    csrf::check(&session, &csrf::header_token(&headers)).await?;
    let mut g = history::begin(&state, Actor::import(&user, q.name.trim())).await;
    let (id, imported) = crate::domain::transfer::import_new_addressbook(&state, &user, &q.owner, &q.name, q.color.trim(), &q.description, &body).await?;
    g.coll(Store::Ab, &q.owner, &id);
    g.commit().await;
    Ok(axum::Json(ImportedNew { id, owner: q.owner, imported }))
}

async fn import_addressbook(State(state): State<SharedState>, user: Principal, session: Session, headers: axum::http::HeaderMap, Path((owner, id)): Path<(String, String)>, body: axum::body::Bytes) -> AppResult<axum::Json<Imported>> {
    csrf::check(&session, &csrf::header_token(&headers)).await?;
    let mut g = history::begin(&state, Actor::import(&user, "")).await;
    g.coll(Store::Ab, &owner, &id);
    let imported = crate::domain::transfer::import_addressbook(&state, &user, &owner, &id, &body).await?;
    g.commit().await;
    Ok(axum::Json(Imported { imported }))
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
