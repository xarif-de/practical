//! /calendar — the calendar page and its fragments.

use crate::ctx::PageCtx;
use crate::domain::collections::CalInfo;
use crate::domain::events::{self, Instance};
use crate::domain::prefs;
use crate::error::AppResult;
use crate::render::{self, L};
use crate::state::SharedState;
use askama::Template;
use askama_web::WebTemplate;
use axum::Router;
use axum::extract::{Query, State};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use chrono::{Datelike, Duration, NaiveDate, Utc};
use rustical_store::auth::Principal;
use serde::Deserialize;
use std::collections::HashSet;

#[derive(Deserialize)]
pub struct CalQuery {
    #[serde(default)]
    pub view: Option<String>,
    #[serde(default)]
    pub date: Option<NaiveDate>,
    #[serde(default)]
    pub cals: Option<String>,
    #[serde(default)]
    pub fragment: Option<String>,
}

#[derive(Template, WebTemplate)]
#[template(path = "calendar.html")]
pub struct CalendarPage {
    pub ctx: PageCtx,
    pub calendars: Vec<CalInfo>,
    pub view: String,
    pub date: NaiveDate,
    pub title: String,
    pub body: String,
    pub picker: String,
    pub cals_param: String,
    pub timezone: String,
    pub today: NaiveDate,
    pub view_label: String,
    pub in_more: bool,
    pub presets_json: String,
    /// reminder presets of the event editor; `null` = the editor's defaults
    pub alarm_presets_json: String,
    /// snap points of the reminder strip (`events::AlarmSnaps`)
    pub alarm_snaps_json: String,
    /// the user's category rules (`domain::catrules`), applied by the editor
    pub category_rules_json: String,
}

#[derive(Template)]
#[template(path = "calendar_fragment.html")]
pub struct CalendarFragment {
    pub ctx: PageCtx,
    pub view: String,
    pub date: NaiveDate,
    pub title: String,
    pub body: String,
    pub cals_param: String,
    pub view_label: String,
    pub in_more: bool,
}

pub struct ViewCtx {
    pub tz: chrono_tz::Tz,
    pub today: NaiveDate,
    pub l: L,
}

pub async fn view_ctx(state: &SharedState, user: &Principal, lang: &str) -> AppResult<ViewCtx> {
    let p = prefs::get(&state.db, &user.id).await?;
    let tz: chrono_tz::Tz = p.timezone.as_deref().and_then(|t| t.parse().ok()).unwrap_or_else(|| state.config.default_timezone.parse().unwrap_or(chrono_tz::Europe::Berlin));
    let today = Utc::now().with_timezone(&tz).date_naive();
    Ok(ViewCtx { tz, today, l: L::new(lang) })
}

pub fn parse_cals(cals: Option<&str>) -> Option<HashSet<String>> {
    cals.filter(|c| !c.is_empty()).map(|c| c.split(',').map(str::to_string).collect())
}

pub fn view_label(view: &str, l: &L) -> (String, bool) {
    match view {
        "day" => (l.t("Tag", "Day").into(), false),
        "week" => (l.t("Woche", "Week").into(), false),
        "month" => (l.t("Monat", "Month").into(), false),
        "planner" => (l.t("Jahresplaner", "Year planner").into(), true),
        "weekends" => (l.t("Freie Wochenenden", "Free weekends").into(), true),
        "vacation" => (l.t("Urlaubsplaner", "Vacation planner").into(), true),
        "countdown" => (l.t("Countdown-Wand", "Countdown wall").into(), true),
        "numbers" => (l.t("Jahr in Zahlen", "Year in numbers").into(), true),
        _ => (l.t("Monat", "Month").into(), false),
    }
}

/// Renders title + body for a view; the window loaded depends on the view.
pub async fn render_view(state: &SharedState, user: &Principal, ctx: &PageCtx, view: &str, date: NaiveDate, cals: &[CalInfo], vc: &ViewCtx) -> AppResult<(String, String)> {
    let l = &vc.l;
    let (y, m) = (date.year(), date.month());
    let month_start = NaiveDate::from_ymd_opt(y, m, 1).unwrap();
    let grid_start = month_start - Duration::days(i64::from(month_start.weekday().num_days_from_monday()));
    let grid_end = grid_start + Duration::days(42);
    Ok(match view {
        "day" => {
            let inst = events::instances(state, cals, grid_start, grid_end, vc.tz).await?;
            let names: Vec<String> = cals.iter().map(|c| c.displayname.clone()).collect();
            (if l.de { format!("{}. {} {}", date.day(), l.mon(m), y) } else { format!("{} {}, {}", l.mon(m), date.day(), y) }, render::day_view(date, vc.today, &inst, l, &names.join(", ")))
        }
        "week" => {
            let inst = events::instances(state, cals, grid_start, grid_end, vc.tz).await?;
            (format!("{} {y}", l.mon(m)), render::week_view(date, vc.today, &inst, l))
        }
        "planner" | "weekends" | "vacation" | "countdown" | "numbers" => {
            crate::routes::more::render(state, user, ctx, view, date, cals, vc).await?
        }
        _ => {
            let inst = events::instances(state, cals, grid_start, grid_end, vc.tz).await?;
            let agenda_date = if date.month() == vc.today.month() && date.year() == vc.today.year() { vc.today } else { date };
            (format!("{} {y}", l.mon(m)), render::month_view(date, vc.today, &inst, l, agenda_date))
        }
    })
}

async fn page(State(state): State<SharedState>, user: Principal, ctx: PageCtx, Query(q): Query<CalQuery>) -> AppResult<Response> {
    let vc = view_ctx(&state, &user, &ctx.lang).await?;
    let view = q.view.clone().unwrap_or_else(|| "month".into());
    let date = q.date.unwrap_or(vc.today);
    let all_cals = events::visible_calendars(&state, &user, None).await?;
    let filter = parse_cals(q.cals.as_deref());
    let cals: Vec<CalInfo> = match &filter {
        Some(f) => all_cals.iter().filter(|c| f.contains(&format!("{}/{}", c.principal, c.id))).cloned().collect(),
        None => all_cals.clone(),
    };
    let (title, body) = render_view(&state, &user, &ctx, &view, date, &cals, &vc).await?;
    let (view_label, in_more) = view_label(&view, &vc.l);
    let cals_param = q.cals.clone().unwrap_or_default();
    if q.fragment.is_some() {
        let f = CalendarFragment { ctx, view, date, title, body, cals_param, view_label, in_more };
        return Ok(Html(f.render().map_err(|e| crate::error::AppError::Internal(e.to_string()))?).into_response());
    }
    let density = events::month_density(&state, &cals, date.year(), vc.tz).await.unwrap_or([0; 12]);
    let picker = render::picker(date.year(), date.month(), &density, &vc.l);
    let p = prefs::get(&state.db, &user.id).await?;
    Ok(CalendarPage {
        ctx,
        calendars: all_cals,
        view,
        date,
        title,
        body,
        picker,
        cals_param,
        timezone: vc.tz.name().to_string(),
        today: vc.today,
        view_label,
        in_more,
        presets_json: p.presets_json.unwrap_or_else(|| "[]".into()),
        alarm_snaps_json: serde_json::to_string(&events::alarm_snaps(p.alarm_snaps_json.as_deref())).unwrap(),
        alarm_presets_json: p.alarm_presets_json.filter(|j| serde_json::from_str::<Vec<events::AlarmPreset>>(j).is_ok()).unwrap_or_else(|| "null".into()),
        category_rules_json: serde_json::to_string(&crate::domain::catrules::all(&state.db, &user.id).await?).unwrap(),
    }
    .into_response())
}

/// JSON instances for a window (editor day line, agenda refresh).
#[derive(Deserialize)]
pub struct WindowQuery {
    pub start: NaiveDate,
    pub end: NaiveDate,
    #[serde(default)]
    pub cals: Option<String>,
}

pub async fn api_events(State(state): State<SharedState>, user: Principal, ctx: PageCtx, Query(q): Query<WindowQuery>) -> AppResult<axum::Json<Vec<Instance>>> {
    let vc = view_ctx(&state, &user, &ctx.lang).await?;
    if (q.end - q.start).num_days() > 400 || q.end < q.start {
        return Err(crate::error::AppError::BadRequest("window too large".into()));
    }
    let cals = events::visible_calendars(&state, &user, parse_cals(q.cals.as_deref()).as_ref()).await?;
    Ok(axum::Json(events::instances(&state, &cals, q.start, q.end, vc.tz).await?))
}

#[derive(Deserialize)]
pub struct DensityQuery {
    pub year: i32,
    #[serde(default)]
    pub cals: Option<String>,
}

pub async fn api_density(State(state): State<SharedState>, user: Principal, ctx: PageCtx, Query(q): Query<DensityQuery>) -> AppResult<axum::Json<[usize; 12]>> {
    let vc = view_ctx(&state, &user, &ctx.lang).await?;
    let cals = events::visible_calendars(&state, &user, parse_cals(q.cals.as_deref()).as_ref()).await?;
    Ok(axum::Json(events::month_density(&state, &cals, q.year, vc.tz).await?))
}

pub fn router() -> Router<SharedState> {
    Router::new().route("/calendar", get(page)).route("/api/events", get(api_events).post(crate::routes::api::create_event)).route("/api/density", get(api_density))
}
