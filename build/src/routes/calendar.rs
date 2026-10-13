//! /calendar — the calendar page and its fragments.
//!
//! Routes: `/calendar` (the page; `?fragment=1` = only the view part), and the JSON/HTML pieces the client
//! fetches: `/api/events` (instances of a window; POST creates an event), `/api/days`, `/api/weeks`,
//! `/api/density`.
//!
//! - The views are rendered on the server ([`crate::render`]). The client swaps in `?fragment=1` answers and
//!   refetches them on a live change (SSE).
//! - `date` (the picked day) is shared by all views. `?week=<Monday>` and `?month=YYYY-MM` name the week or
//!   month on screen when it is not the one `date` lies in (the client scrolls there).
//! - Only VEVENTs are drawn: tasks never show in the calendar.
//! - Deep links `?rule=<name>&back=<url>` open a category rule; the client reads them (js/editor/wiring.js).
//! - [`PageView`] is the extractor of every calendar-like page: [`PageCtx`] plus [`ViewCtx`] (zone, today,
//!   labels, the user's settings). [`render_view`] renders title and body of one view.
//! - The five year views behind "Mehr ▾" are [`crate::routes::more`].

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
use axum::extract::{FromRequestParts, State};
use crate::extract::Query;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use chrono::{Datelike, Duration, NaiveDate};
use rustical_store::auth::Principal;
use serde::Deserialize;
use std::collections::HashSet;

#[derive(Deserialize)]
pub struct CalQuery {
    #[serde(default)]
    pub view: Option<String>,
    #[serde(default, deserialize_with = "crate::extract::opt_day")]
    pub date: Option<NaiveDate>,
    #[serde(default)]
    pub cals: Option<String>,
    #[serde(default)]
    pub fragment: Option<String>,
    /// month view: the month on screen (`YYYY-MM`) when it is not the one `date` (the picked day) lies in
    #[serde(default)]
    pub month: Option<String>,
    /// week view: the Monday of the week on screen when it is not the one `date` lies in
    #[serde(default, deserialize_with = "crate::extract::opt_day")]
    pub week: Option<NaiveDate>,
}

#[derive(Template, WebTemplate)]
#[template(path = "calendar.html")]
pub struct CalendarPage {
    pub ctx: PageCtx,
    pub calendars: Vec<CalInfo>,
    /// which consumer of `editor.html` this page is: "event" or "task"
    pub editor_kind: &'static str,
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
    /// reminder presets of the event editor; `null` = the editor's defaults
    pub alarm_presets_json: String,
    /// snap points of the reminder strip (`events::AlarmSnaps`)
    pub alarm_snaps_json: String,
    /// the user's category rules (`domain::catrules`), applied by the editor
    pub category_rules_json: String,
    /// holiday switches of the filter popover, and whether the region has school holidays at all
    pub holidays_on: bool,
    pub school_on: bool,
    pub has_school: bool,
    /// `:root { --hol; --fer }` from the user's holiday colours
    pub holiday_css: String,
    /// the filter rows pinned as quick switches (`Prefs::pins`)
    pub pins: Vec<String>,
}

impl CalendarPage {
    /// a filter row is pinned as a quick switch (`Prefs::filter_pins_json`): `kind` cal | ab | hol
    pub fn pinned(&self, kind: &str, owner: &str, id: &str) -> bool {
        crate::domain::prefs::pinned(&self.pins, kind, owner, id)
    }
    /// a group calendar is in the filter: only then does "only mine" differ from "all"
    pub fn has_shared(&self) -> bool {
        self.calendars.iter().any(|c| c.is_group)
    }
}

#[derive(Template)]
#[template(path = "calendar_fragment.html")]
pub struct CalendarFragment {
    pub ctx: PageCtx,
    pub view: String,
    pub date: NaiveDate,
    pub title: String,
    pub body: String,
    pub view_label: String,
    pub in_more: bool,
}

pub struct ViewCtx {
    pub tz: chrono_tz::Tz,
    pub today: NaiveDate,
    pub l: L,
    /// the user's settings, read once per request (the zone above, holidays, editor presets)
    pub prefs: prefs::Prefs,
}

pub async fn view_ctx(state: &SharedState, user: &Principal, lang: &str) -> AppResult<ViewCtx> {
    let p = prefs::get(&state.db, &user.id).await?;
    let tz: chrono_tz::Tz = p.timezone.as_deref().and_then(|t| t.parse().ok()).unwrap_or_else(|| state.config.default_timezone.parse().unwrap_or(chrono_tz::Europe::Berlin));
    let today = crate::clock::today_in(&tz);
    Ok(ViewCtx { tz, today, l: L::new(lang, state.i18n.clone()), prefs: p })
}

/// Extractor: the page context and the user's view context in its language — `PageView(ctx, vc)`.
pub struct PageView(pub PageCtx, pub ViewCtx);

impl FromRequestParts<SharedState> for PageView {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut axum::http::request::Parts, state: &SharedState) -> Result<Self, Response> {
        let user = Principal::from_request_parts(parts, state).await.map_err(IntoResponse::into_response)?;
        let Ok(ctx) = PageCtx::from_request_parts(parts, state).await;
        let vc = view_ctx(state, &user, &ctx.lang).await.map_err(IntoResponse::into_response)?;
        Ok(PageView(ctx, vc))
    }
}

pub fn parse_cals(cals: Option<&str>) -> Option<HashSet<String>> {
    cals.filter(|c| !c.is_empty()).map(|c| c.split(',').map(str::to_string).collect())
}

pub fn view_label(view: &str, l: &L) -> (String, bool) {
    match view {
        "day" => (l.t("view-day").into(), false),
        "week" => (l.t("view-week").into(), false),
        "month" => (l.t("view-month").into(), false),
        "planner" => (l.t("v-year-planner").into(), false),
        "weekends" => (l.t("view-weekends").into(), true),
        "vacation" => (l.t("view-vacation").into(), true),
        "countdown" => (l.t("view-countdown").into(), true),
        "numbers" => (l.t("view-numbers").into(), true),
        _ => (l.t("view-month").into(), false),
    }
}

/// Renders title + body for a view; the window loaded depends on the view.
/// `shown` is the month or week view's month / week on screen; the other views follow `date`.
pub async fn render_view(state: &SharedState, user: &Principal, ctx: &PageCtx, view: &str, date: NaiveDate, shown: Option<NaiveDate>, cals: &[CalInfo], vc: &ViewCtx) -> AppResult<(String, String)> {
    let l = &vc.l;
    // every view adds weeks to its date: keep it where that stays representable (chrono panics past ±262 143)
    if !crate::domain::ical::year_ok(date) || shown.is_some_and(|s| !crate::domain::ical::year_ok(s)) {
        return Err(crate::error::AppError::bad("err-date-range"));
    }
    let (y, m) = (date.year(), date.month());
    let month_start = NaiveDate::from_ymd_opt(y, m, 1).unwrap();
    let grid_start = month_start - Duration::days(i64::from(month_start.weekday().num_days_from_monday()));
    let grid_end = grid_start + Duration::days(42);
    Ok(match view {
        "day" => {
            let (ds, de) = render::day_window(date);
            let inst = events::instances(state, cals, ds.min(grid_start), de.max(grid_end), vc.tz).await?;
            let hol = crate::domain::holidays::marks(state, &vc.prefs, ds, de, &ctx.lang).await?;
            let names: Vec<String> = cals.iter().map(|c| c.displayname.clone()).collect();
            // the document in the middle shows links, coordinates, the rule and the marking, none of
            // which an Instance carries — they are read for the handful of events of THIS day only,
            // never for the whole window. A calendar the user may see but not own is skipped.
            let mut details = std::collections::HashMap::new();
            for i in inst.iter().filter(|i| i.start_date() <= date && i.last_date() >= date) {
                if details.contains_key(&i.key()) {
                    continue;
                }
                if let Ok(d) = events::get_event(state, user, &i.principal, &i.cal_id, &i.object_id, vc.tz).await {
                    details.insert(i.key(), d);
                }
            }
            (l.ta("date-day-title", &[("day", i64::from(date.day()).into()), ("month", l.mon(m).into()), ("year", i64::from(y).into())]), render::day_view(date, vc.today, &inst, l, &names.join(", "), &details, &hol))
        }
        "week" => {
            let date = shown.unwrap_or(date);
            let (ws, we) = render::week_window(date);
            // the union covers the week stream and the density panel's month grid, whose month is
            // the focus week's — not necessarily the one `date` falls in
            let hm = render::monday_of(date);
            let hf = NaiveDate::from_ymd_opt(hm.year(), hm.month(), 1).unwrap();
            let hs = hf - Duration::days(i64::from(hf.weekday().num_days_from_monday()));
            let inst = events::instances(state, cals, ws.min(hs), we.max(hs + Duration::days(42)), vc.tz).await?;
            let hol = crate::domain::holidays::marks(state, &vc.prefs, ws, we, &ctx.lang).await?;
            (l.week_title(date), render::week_view(date, vc.today, &inst, l, &hol))
        }
        "planner" | "weekends" | "vacation" | "countdown" | "numbers" => {
            crate::routes::more::render(state, user, ctx, view, date, cals, vc).await?
        }
        _ => {
            // scrolling turns the month without touching the picked day; the agenda shows the picked
            // day while it is on screen, else today, else the month's first
            let same = |a: NaiveDate, b: NaiveDate| a.year() == b.year() && a.month() == b.month();
            let month = shown.unwrap_or(date);
            let month_start = NaiveDate::from_ymd_opt(month.year(), month.month(), 1).unwrap();
            let grid_start = month_start - Duration::days(i64::from(month_start.weekday().num_days_from_monday()));
            let grid_end = grid_start + Duration::days(42);
            let inst = events::instances(state, cals, grid_start, grid_end, vc.tz).await?;
            let hol = crate::domain::holidays::marks(state, &vc.prefs, grid_start, grid_end, &ctx.lang).await?;
            let agenda_date = if same(date, month) { date } else if same(vc.today, month) { vc.today } else { month_start };
            (format!("{} {}", l.mon(month.month()), month.year()), render::month_view(month, date, vc.today, &inst, l, agenda_date, &hol))
        }
    })
}

async fn page(State(state): State<SharedState>, user: Principal, PageView(ctx, vc): PageView, Query(q): Query<CalQuery>) -> AppResult<Response> {
    let view = q.view.clone().unwrap_or_else(|| "month".into());
    let date = q.date.unwrap_or(vc.today);
    let all_cals = events::visible_calendars(&state, &user, None).await?;
    let filter = parse_cals(q.cals.as_deref());
    let cals: Vec<CalInfo> = match &filter {
        Some(f) => all_cals.iter().filter(|c| f.contains(&format!("{}/{}", c.principal, c.id))).cloned().collect(),
        None => all_cals.clone(),
    };
    let shown = if view == "week" { q.week } else { q.month.as_deref().and_then(|m| NaiveDate::parse_from_str(&format!("{m}-01"), "%Y-%m-%d").ok()).filter(|d| crate::extract::YEARS.contains(&d.year())) };
    let (title, body) = render_view(&state, &user, &ctx, &view, date, shown, &cals, &vc).await?;
    let (view_label, in_more) = view_label(&view, &vc.l);
    let cals_param = q.cals.clone().unwrap_or_default();
    if q.fragment.is_some() {
        let f = CalendarFragment { ctx, view, date, title, body, view_label, in_more };
        return Ok(Html(f.render().map_err(|e| crate::error::AppError::Internal(e.to_string()))?).into_response());
    }
    let density = events::month_density(&state, &cals, date.year(), vc.tz).await.unwrap_or([0; 12]);
    let picker = render::picker(date.year(), date.month(), &density, &vc.l);
    let p = vc.prefs;
    let isl = crate::routes::editor::islands(&state, &user, &p, "event").await?;
    let has_school = p.regions(true).iter().any(|r| crate::domain::holidays::has_school(crate::domain::holidays::split(r).0));
    Ok(CalendarPage {
        holidays_on: p.holidays_enabled,
        school_on: p.school_holidays_enabled,
        has_school,
        holiday_css: p.holiday_css(),
        pins: p.pins(),
        editor_kind: "event",
        ctx,
        calendars: all_cals, // = routes::editor::calendars(&all_cals, "event"), without the copy
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
        alarm_snaps_json: isl.alarm_snaps_json,
        alarm_presets_json: isl.alarm_presets_json,
        category_rules_json: isl.category_rules_json,
    }
    .into_response())
}

/// JSON instances for a window (editor day line, agenda refresh).
#[derive(Deserialize)]
pub struct WindowQuery {
    #[serde(deserialize_with = "crate::extract::day")]
    pub start: NaiveDate,
    #[serde(deserialize_with = "crate::extract::day")]
    pub end: NaiveDate,
    #[serde(default)]
    pub cals: Option<String>,
}

pub async fn api_events(State(state): State<SharedState>, user: Principal, PageView(_, vc): PageView, Query(q): Query<WindowQuery>) -> AppResult<axum::Json<Vec<Instance>>> {
    if (q.end - q.start).num_days() > 400 || q.end < q.start {
        return Err(crate::error::AppError::bad("err-window-too-large"));
    }
    let cals = events::visible_calendars(&state, &user, parse_cals(q.cals.as_deref()).as_ref()).await?;
    let mut inst = events::instances(&state, &cals, q.start, q.end, vc.tz).await?;
    events::limit_notes(&mut inst);
    Ok(axum::Json(inst))
}

/// Ledger rows for the endless day list (app.js `mountLedger`).
#[derive(Deserialize)]
pub struct DaysQuery {
    #[serde(deserialize_with = "crate::extract::day")]
    pub start: NaiveDate,
    #[serde(default)]
    pub count: Option<u32>,
    #[serde(default)]
    pub cals: Option<String>,
}

pub async fn api_days(State(state): State<SharedState>, user: Principal, PageView(ctx, vc): PageView, Query(q): Query<DaysQuery>) -> AppResult<Response> {
    let count = q.count.unwrap_or(30).clamp(1, 120);
    if !crate::domain::ical::year_ok(q.start) {
        return Err(crate::error::AppError::bad("err-date-range"));
    }
    let end = q.start + Duration::days(i64::from(count));
    let cals = events::visible_calendars(&state, &user, parse_cals(q.cals.as_deref()).as_ref()).await?;
    let inst = events::instances(&state, &cals, q.start, end, vc.tz).await?;
    let hol = crate::domain::holidays::marks(&state, &vc.prefs, q.start, end, &ctx.lang).await?;
    Ok(Html(render::day_rows(q.start, count as usize, vc.today, None, &inst, &vc.l, &hol)).into_response())
}

/// Week blocks for the endless week stream (app.js `mountWeeks`).
#[derive(Deserialize)]
pub struct WeeksQuery {
    #[serde(deserialize_with = "crate::extract::day")]
    pub start: NaiveDate,
    #[serde(default)]
    pub count: Option<usize>,
    #[serde(default)]
    pub cals: Option<String>,
}

pub async fn api_weeks(State(state): State<SharedState>, user: Principal, PageView(ctx, vc): PageView, Query(q): Query<WeeksQuery>) -> AppResult<Response> {
    let count = q.count.unwrap_or(8).clamp(1, 16);
    if !crate::domain::ical::year_ok(q.start) {
        return Err(crate::error::AppError::bad("err-date-range"));
    }
    let start = render::monday_of(q.start);
    let end = start + Duration::days(7 * count as i64);
    let cals = events::visible_calendars(&state, &user, parse_cals(q.cals.as_deref()).as_ref()).await?;
    let inst = events::instances(&state, &cals, start, end, vc.tz).await?;
    let hol = crate::domain::holidays::marks(&state, &vc.prefs, start, end, &ctx.lang).await?;
    Ok(Html(render::week_blocks(start, count, vc.today, &inst, &vc.l, &hol)).into_response())
}

#[derive(Deserialize)]
pub struct DensityQuery {
    #[serde(deserialize_with = "crate::extract::year")]
    pub year: i32,
    #[serde(default)]
    pub cals: Option<String>,
}

pub async fn api_density(State(state): State<SharedState>, user: Principal, PageView(_, vc): PageView, Query(q): Query<DensityQuery>) -> AppResult<axum::Json<[usize; 12]>> {
    let cals = events::visible_calendars(&state, &user, parse_cals(q.cals.as_deref()).as_ref()).await?;
    Ok(axum::Json(events::month_density(&state, &cals, q.year, vc.tz).await?))
}

pub fn router() -> Router<SharedState> {
    Router::new().route("/calendar", get(page)).route("/api/events", get(api_events).post(crate::routes::api::create_event)).route("/api/density", get(api_density)).route("/api/weeks", get(api_weeks)).route("/api/days", get(api_days))
}
