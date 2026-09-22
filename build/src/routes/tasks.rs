//! /tasks — the task views. The views themselves are drawn in the browser from one JSON island
//! (app.js, the `#taskroot` module); this route decides which tasks are in it. The editor comes
//! from `templates/editor.html`, the same dialog the calendar uses.

use crate::ctx::PageCtx;
use crate::domain::collections::CalInfo;
use crate::domain::prefs;
use crate::domain::tasks::{self, Task};
use crate::domain::events;
use crate::error::AppResult;
use crate::routes::calendar::view_ctx;
use crate::state::SharedState;
use askama::Template;
use askama_web::WebTemplate;
use axum::Router;
use axum::extract::{Query, State};
use axum::routing::get;
use chrono::NaiveDate;
use rustical_store::auth::Principal;

/// The views: groups, table, board, timeline, deck, history.
const VIEWS: &[&str] = &["groups", "table", "board", "timeline", "deck", "history"];
/// How far back a view shows finished tasks. Without a limit a year of ticked-off work
/// drowns every view, so this is one setting shared by all of them.
const LOOKBACKS: &[&str] = &["day", "week", "month", "quarter", "year", "all"];

fn lookback_days(v: &str) -> Option<i64> {
    match v {
        "day" => Some(1),
        "week" => Some(7),
        "month" => Some(31),
        "quarter" => Some(92),
        "year" => Some(365),
        _ => None,
    }
}

#[derive(serde::Deserialize, Default)]
pub struct TaskQuery {
    #[serde(default)]
    pub view: Option<String>,
    /// how far back finished tasks are shown
    #[serde(default)]
    pub back: Option<String>,
    #[serde(default)]
    pub cals: Option<String>,
}

fn one_of(v: Option<&String>, allowed: &[&'static str], default: &'static str) -> &'static str {
    v.and_then(|s| allowed.iter().find(|a| **a == s.as_str()).copied()).unwrap_or(default)
}

#[derive(Template, WebTemplate)]
#[template(path = "tasks.html")]
pub struct TasksPage {
    pub ctx: PageCtx,
    pub calendars: Vec<CalInfo>,
    /// which consumer of `editor.html` this page is
    pub editor_kind: &'static str,
    pub view: &'static str,
    pub back: &'static str,
    pub today: NaiveDate,
    pub timezone: String,
    pub cals_param: String,
    pub open_count: usize,
    pub hidden_done: usize,
    pub tasks_json: String,
    pub alarm_presets_json: String,
    pub alarm_snaps_json: String,
    pub category_rules_json: String,
}

/// A finished task is in range when it was finished (or, lacking a stamp, was due) inside the window.
fn in_lookback(t: &Task, today: NaiveDate, days: Option<i64>) -> bool {
    let Some(days) = days else { return true };
    match t.completed_at.map(|c| c.date()).or(t.due.map(|d| d.date())) {
        Some(d) => (today - d).num_days() <= days,
        None => false, // finished, no date at all: only the unlimited setting shows it
    }
}

async fn page(State(state): State<SharedState>, user: Principal, ctx: PageCtx, Query(q): Query<TaskQuery>) -> AppResult<TasksPage> {
    let vc = view_ctx(&state, &user, &ctx.lang).await?;
    let view = one_of(q.view.as_ref(), VIEWS, "groups");
    let back = one_of(q.back.as_ref(), LOOKBACKS, "month");
    let all_cals = events::visible_calendars(&state, &user, None).await?;
    let calendars: Vec<CalInfo> = all_cals.iter().filter(|c| c.components.iter().any(|x| x == "VTODO")).cloned().collect();
    let picked: Vec<CalInfo> = match q.cals.as_deref().filter(|s| !s.is_empty()) {
        Some(list) => {
            let keep: Vec<&str> = list.split(',').collect();
            calendars.iter().filter(|c| keep.contains(&format!("{}/{}", c.principal, c.id).as_str())).cloned().collect()
        }
        None => calendars.clone(),
    };
    let all = tasks::tasks_for(&state, &picked, vc.tz).await?;
    let open_count = all.iter().filter(|t| t.open()).count();
    let days = lookback_days(back);
    let shown: Vec<&Task> = all.iter().filter(|t| t.open() || in_lookback(t, vc.today, days)).collect();
    let hidden_done = all.len() - shown.len();
    let tasks_json = serde_json::to_string(&shown).unwrap_or_else(|_| "[]".into());
    let p = prefs::get(&state.db, &user.id).await?;
    Ok(TasksPage {
        ctx,
        calendars,
        editor_kind: "task",
        view,
        back,
        today: vc.today,
        timezone: vc.tz.name().to_string(),
        cals_param: q.cals.unwrap_or_default(),
        open_count,
        hidden_done,
        tasks_json,
        alarm_snaps_json: serde_json::to_string(&events::alarm_snaps(p.alarm_snaps_json.as_deref())).unwrap(),
        alarm_presets_json: p.alarm_presets_json.filter(|j| serde_json::from_str::<Vec<events::AlarmPreset>>(j).is_ok()).unwrap_or_else(|| "null".into()),
        category_rules_json: serde_json::to_string(&crate::domain::catrules::all(&state.db, &user.id, "task").await?).unwrap(),
    })
}

pub fn router() -> Router<SharedState> {
    Router::new().route("/tasks", get(page))
}
