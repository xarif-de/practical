//! /settings — groups (D2/D3/D6), collections (D4/D5), app tokens (D8),
//! trash, preferences (D9/D13/D14). All mutations are POST forms with CSRF.

use crate::csrf;
use crate::ctx::{PageCtx, set_flash};
use crate::domain::collections::{self, AbInfo, CalInfo, NewCalendar, Owner, PALETTE};
use crate::domain::events;
use crate::domain::groups::{self, GroupInfo, RemoveOutcome, TrashedGroup};
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
    pub holidays_country: String,
    pub holidays_subdivision: String,
    pub calendars_all: Vec<CalInfo>,
    pub vacation_kind: String,
    pub vacation_value: String,
    /// reminder strip snap points as typed ("2 min, 1 h, …")
    pub alarm_snaps_start: String,
    pub alarm_snaps_end: String,
}

fn vacation_rule(p: &Prefs) -> (String, String) {
    let v: serde_json::Value = p.vacation_rule_json.as_deref().and_then(|j| serde_json::from_str(j).ok()).unwrap_or(serde_json::json!({}));
    (v["kind"].as_str().unwrap_or("category").to_string(), v["value"].as_str().unwrap_or("Urlaub").to_string())
}

impl SettingsPage {
    pub fn lang_is(&self, l: &str) -> bool {
        self.prefs.lang.as_deref().unwrap_or(&self.ctx.lang) == l
    }
    pub fn scheme_is(&self, s: &str) -> bool {
        self.prefs.scheme.as_deref().unwrap_or("auto") == s
    }
}

async fn page(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx) -> AppResult<SettingsPage> {
    let new_token: Option<String> = session.remove(SESSION_NEW_TOKEN).await?;
    let snaps = events::alarm_snaps(prefs::get(&state.db, &user.id).await?.alarm_snaps_json.as_deref());
    Ok(SettingsPage {
        alarm_snaps_start: events::fmt_offsets(&snaps.start),
        alarm_snaps_end: events::fmt_offsets(&snaps.end),
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
        holidays_country: state.config.holidays_country.clone(),
        holidays_subdivision: state.config.holidays_subdivision.clone(),
        calendars_all: collections::calendars_for(&state, &user, false).await?,
        vacation_kind: vacation_rule(&prefs::get(&state.db, &user.id).await?).0,
        vacation_value: vacation_rule(&prefs::get(&state.db, &user.id).await?).1,
        ctx,
    })
}

/// Flash + redirect back to the section; store errors become error flashes so
/// the user never lands on a bare error page after a form post.
async fn finish(state: &SharedState, session: &Session, ctx: &PageCtx, result: AppResult<&str>, anchor: &str) -> Response {
    match result {
        Ok(key) => set_flash(session, "ok", state.i18n.t(&ctx.lang, key)).await,
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
        Ok("flash-group-created")
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
        Ok("flash-member-added")
    }
    .await;
    finish(&state, &session, &ctx, r, "groups").await
}

async fn group_remove_member(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path((gid, member)): Path<(String, String)>, Form(f): Form<Csrf>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        Ok(match groups::remove_member(&state, &user, &gid, &member).await? {
            RemoveOutcome::Removed => "flash-member-removed",
            RemoveOutcome::GroupTrashed => "flash-group-trashed",
        })
    }
    .await;
    finish(&state, &session, &ctx, r, "groups").await
}

async fn group_leave(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path(gid): Path<String>, Form(f): Form<Csrf>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        Ok(match groups::remove_member(&state, &user, &gid, &user.id).await? {
            RemoveOutcome::Removed => "flash-left-group",
            RemoveOutcome::GroupTrashed => "flash-group-trashed",
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
        Ok("flash-group-trashed")
    }
    .await;
    finish(&state, &session, &ctx, r, "groups").await
}

async fn group_restore(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path(gid): Path<String>, Form(f): Form<Csrf>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        groups::restore(&state, &user, &gid).await?;
        Ok("flash-group-restored")
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
        Ok("flash-group-purged")
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
        Ok("flash-calendar-created")
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
        Ok("flash-saved")
    }
    .await;
    finish(&state, &session, &ctx, r, "collections").await
}

async fn calendar_delete(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path((owner, id)): Path<(String, String)>, Form(f): Form<Csrf>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        collections::delete_calendar(&state, &user, &owner, &id, false).await?;
        Ok("flash-calendar-trashed")
    }
    .await;
    finish(&state, &session, &ctx, r, "collections").await
}

async fn calendar_restore(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path((owner, id)): Path<(String, String)>, Form(f): Form<Csrf>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        collections::restore_calendar(&state, &user, &owner, &id).await?;
        Ok("flash-restored")
    }
    .await;
    finish(&state, &session, &ctx, r, "trash").await
}

async fn calendar_purge(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path((owner, id)): Path<(String, String)>, Form(f): Form<Csrf>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        collections::delete_calendar(&state, &user, &owner, &id, true).await?;
        Ok("flash-purged")
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
        Ok("flash-addressbook-created")
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
        Ok("flash-saved")
    }
    .await;
    finish(&state, &session, &ctx, r, "collections").await
}

async fn addressbook_delete(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path((owner, id)): Path<(String, String)>, Form(f): Form<Csrf>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        collections::delete_addressbook(&state, &user, &owner, &id, false).await?;
        Ok("flash-addressbook-trashed")
    }
    .await;
    finish(&state, &session, &ctx, r, "collections").await
}

async fn addressbook_restore(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path((owner, id)): Path<(String, String)>, Form(f): Form<Csrf>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        collections::restore_addressbook(&state, &user, &owner, &id).await?;
        Ok("flash-restored")
    }
    .await;
    finish(&state, &session, &ctx, r, "trash").await
}

async fn addressbook_purge(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path((owner, id)): Path<(String, String)>, Form(f): Form<Csrf>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        collections::delete_addressbook(&state, &user, &owner, &id, true).await?;
        Ok("flash-purged")
    }
    .await;
    finish(&state, &session, &ctx, r, "trash").await
}

async fn token_create(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Form(f): Form<NameForm>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        let t = tokens::create(&state, &user, &f.name).await?;
        session.insert(SESSION_NEW_TOKEN, t).await?;
        Ok("flash-token-created")
    }
    .await;
    finish(&state, &session, &ctx, r, "tokens").await
}

async fn token_delete(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path(id): Path<String>, Form(f): Form<Csrf>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        tokens::delete(&state, &user, &id).await?;
        Ok("flash-token-deleted")
    }
    .await;
    finish(&state, &session, &ctx, r, "tokens").await
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
    holidays_enabled: Option<String>,
    #[serde(default)]
    school_holidays_enabled: Option<String>,
    #[serde(default)]
    holiday_subdivision: String,
    #[serde(default)]
    vacation_days: String,
    #[serde(default)]
    vacation_kind: String,
    #[serde(default)]
    vacation_value: String,
    #[serde(default)]
    alarm_snaps_start: String,
    #[serde(default)]
    alarm_snaps_end: String,
}

async fn prefs_save(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Form(f): Form<PrefsForm>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        let mut p = prefs::get(&state.db, &user.id).await?;
        p.lang = Some(f.lang.clone()).filter(|l| crate::i18n::LANGS.contains(&l.as_str()));
        p.scheme = Some(f.scheme.clone()).filter(|s| s == "light" || s == "dark");
        p.timezone = Some(f.timezone.trim().to_string()).filter(|t| !t.is_empty() && t.parse::<chrono_tz::Tz>().is_ok());
        p.holidays_enabled = f.holidays_enabled.is_some();
        p.school_holidays_enabled = f.school_holidays_enabled.is_some();
        p.holiday_subdivision = Some(f.holiday_subdivision.trim().to_uppercase()).filter(|s| !s.is_empty());
        p.vacation_days = f.vacation_days.trim().parse::<i64>().ok().filter(|d| (0..=366).contains(d));
        let kind = if f.vacation_kind == "calendar" { "calendar" } else { "category" };
        p.vacation_rule_json = Some(serde_json::json!({ "kind": kind, "value": f.vacation_value.trim() }).to_string());
        // reminder strip snap points: an empty list falls back to the built-in one; a bad token rejects the form
        let start = events::parse_offsets(&f.alarm_snaps_start).map_err(|t| AppError::BadRequest(format!("snap point: {t}")))?;
        let end = events::parse_offsets(&f.alarm_snaps_end).map_err(|t| AppError::BadRequest(format!("snap point: {t}")))?;
        let snaps = events::AlarmSnaps { start: if start.is_empty() { events::AlarmSnaps::default().start } else { start }, end };
        p.alarm_snaps_json = if snaps == events::AlarmSnaps::default() { None } else { Some(serde_json::to_string(&snaps).unwrap()) };
        prefs::set(&state.db, &user.id, &p).await?;
        if let Some(l) = &p.lang {
            session.insert(crate::ctx::SESSION_LANG, l).await?;
        }
        Ok("flash-saved")
    }
    .await;
    finish(&state, &session, &ctx, r, "prefs").await
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
    Ok(axum::Json(Imported { imported: crate::domain::transfer::import_calendar(&state, &user, &owner, &id, &body).await? }))
}

async fn import_addressbook(State(state): State<SharedState>, user: Principal, session: Session, headers: axum::http::HeaderMap, Path((owner, id)): Path<(String, String)>, body: axum::body::Bytes) -> AppResult<axum::Json<Imported>> {
    csrf::check(&session, &csrf::header_token(&headers)).await?;
    Ok(axum::Json(Imported { imported: crate::domain::transfer::import_addressbook(&state, &user, &owner, &id, &body).await? }))
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
        .route("/settings/columns", post(columns_save))
        .route("/settings/list-fields", post(list_fields_save))
        .route("/settings/export/calendars/{owner}/{id}", get(export_calendar))
        .route("/settings/export/addressbooks/{owner}/{id}", get(export_addressbook))
        .route("/settings/import/calendars/{owner}/{id}", post(import_calendar))
        .route("/settings/import/addressbooks/{owner}/{id}", post(import_addressbook))
}
