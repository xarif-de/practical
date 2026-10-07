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
use axum::extract::State;
use crate::extract::Query;
use axum::routing::get;
use chrono::NaiveDate;
use rustical_store::auth::Principal;

/// The views: groups, table, board, timeline, deck, history.
const VIEWS: &[&str] = &["groups", "table", "board", "timeline", "deck", "history"];
/// How far back a view shows finished tasks. Without a limit a year of ticked-off work
/// drowns every view, so this is one setting shared by all of them.
const LOOKBACKS: &[&str] = &["day", "week", "month", "quarter", "year", "all"];
/// Finished tasks reach back as far as the lookback allows, which is unbounded by design — they
/// arrive in pages as the views scroll. Open tasks are the working set and always travel whole.
pub const DONE_CHUNK: usize = 120;
/// Optional columns of the table view with their label's locale key; the title is always there.
const COLUMNS: &[(&str, &str)] = &[
    ("cal", "ed-calendar"), ("start", "ed-start"), ("due", "ed-due"), ("status", "ed-status"), ("percent", "ed-percent"), ("priority", "ed-priority"),
    ("completed", "ed-completed-at"), ("categories", "ed-card-cats"), ("location", "ed-card-where"), ("class", "ed-class"), ("created", "tk-col-created"), ("description", "ed-card-note"),
    ("alarms", "ed-card-alarm"), ("links", "ed-card-links"),
];
pub const ALL_COLUMNS: &[&str] = &["cal", "start", "due", "status", "percent", "priority", "completed", "categories", "location", "class", "created", "description", "alarms", "links"];
const DEFAULT_COLUMNS: &[&str] = &["cal", "start", "due", "status", "percent", "priority", "completed", "categories"];

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

/// The VTODO calendars the user left switched on; an empty filter means all of them.
fn pick_calendars(all: &[CalInfo], cals: Option<&str>) -> Vec<CalInfo> {
    let todo: Vec<CalInfo> = all.iter().filter(|c| c.components.iter().any(|x| x == "VTODO")).cloned().collect();
    match cals.filter(|s| !s.is_empty()) {
        Some(list) => {
            let keep: Vec<&str> = list.split(',').collect();
            todo.into_iter().filter(|c| keep.contains(&format!("{}/{}", c.principal, c.id).as_str())).collect()
        }
        None => todo,
    }
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
    pub done_total: usize,
    pub tasks_json: String,
    pub alarm_presets_json: String,
    pub alarm_snaps_json: String,
    pub category_rules_json: String,
    /// the table view's visible columns
    pub columns: Vec<String>,
}

impl TasksPage {
    /// a group calendar is in the filter: only then does "only mine" differ from "all"
    pub fn has_shared(&self) -> bool {
        self.calendars.iter().any(|c| c.is_group)
    }
}

impl TasksPage {
    pub fn col(&self, name: &str) -> bool {
        self.columns.iter().any(|c| c == name)
    }
    pub fn column_list(&self) -> &'static [(&'static str, &'static str)] {
        COLUMNS
    }
}

/// The order every view starts from: the newest completion first. A page of this list is a
/// valid prefix of what the "due date" and "completion date" sorts show, so paging is safe there;
/// any other sort needs the whole list and the views ask for it.
fn by_completion(a: &Task, b: &Task) -> std::cmp::Ordering {
    let key = |t: &Task| t.completed_at.or(t.due);
    key(b).cmp(&key(a)).then(a.summary.cmp(&b.summary))
}

/// A finished task is in range when it was finished (or, lacking a stamp, was due) inside the window.
fn in_lookback(t: &Task, today: NaiveDate, days: Option<i64>) -> bool {
    let Some(days) = days else { return true };
    match t.completed_at.map(|c| c.date()).or(t.due.map(|d| d.date())) {
        Some(d) => (today - d).num_days() <= days,
        None => false, // finished, no date at all: only the unlimited setting shows it
    }
}

/// The lookback last picked in this browser (app.js sets the `tk_back` cookie on a change), so the
/// page reached from the navigation keeps it; a `back` in the URL wins, a search hit's `back=all` too.
fn back_cookie(headers: &axum::http::HeaderMap) -> Option<String> {
    headers.get_all(axum::http::header::COOKIE).iter().filter_map(|v| v.to_str().ok()).flat_map(|v| v.split(';')).find_map(|c| c.trim().strip_prefix("tk_back=").map(str::to_string))
}

async fn page(State(state): State<SharedState>, user: Principal, ctx: PageCtx, headers: axum::http::HeaderMap, Query(q): Query<TaskQuery>) -> AppResult<TasksPage> {
    let vc = view_ctx(&state, &user, &ctx.lang).await?;
    let view = one_of(q.view.as_ref(), VIEWS, "groups");
    let back = one_of(q.back.as_ref().or(back_cookie(&headers).as_ref()), LOOKBACKS, "month");
    let all_cals = events::visible_calendars(&state, &user, None).await?;
    let calendars: Vec<CalInfo> = all_cals.iter().filter(|c| c.components.iter().any(|x| x == "VTODO")).cloned().collect();
    let picked = pick_calendars(&all_cals, q.cals.as_deref());
    let all = tasks::tasks_for(&state, &picked, vc.tz).await?;
    let days = lookback_days(back);
    let (open, rest): (Vec<Task>, Vec<Task>) = all.into_iter().partition(|t| t.open());
    let mut done: Vec<Task> = rest.into_iter().filter(|t| in_lookback(t, vc.today, days)).collect();
    done.sort_by(by_completion);
    let open_count = open.len();
    let done_total = done.len();
    // the history view's heat map needs a count per day for the whole window, not just the page
    let mut per_day: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for t in &done {
        if let Some(c) = t.completed_at {
            *per_day.entry(c.date().to_string()).or_default() += 1;
        }
    }
    // the views name what there is, not what is loaded, so the two tails are counted apart
    let cancelled_total = done.iter().filter(|t| t.status == "CANCELLED").count();
    let first: Vec<&Task> = done.iter().take(DONE_CHUNK).collect();
    let tasks_json = serde_json::to_string(&serde_json::json!({
        "open": open, "done": first, "done_total": done_total,
        "completed_total": done_total - cancelled_total, "cancelled_total": cancelled_total, "per_day": per_day
    }))
    .unwrap_or_else(|_| "{}".into());
    let tasks_json = crate::render::script_json(tasks_json);
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
        done_total,
        tasks_json,
        alarm_snaps_json: crate::render::script_json(serde_json::to_string(&events::alarm_snaps(p.alarm_snaps_json.as_deref())).unwrap()),
        alarm_presets_json: crate::render::script_json(p.alarm_presets_json.filter(|j| serde_json::from_str::<Vec<events::AlarmPreset>>(j).is_ok()).unwrap_or_else(|| "null".into())),
        category_rules_json: crate::render::script_json(serde_json::to_string(&crate::domain::catrules::all(&state.db, &user.id, "task").await?).unwrap()),
        columns: p.task_columns_json.and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_else(|| DEFAULT_COLUMNS.iter().map(|s| s.to_string()).collect()),
    })
}

/// One page of the finished tasks, in the same order the page's first chunk uses.
/// The views ask for the next one as it scrolls into reach.
async fn done_page(State(state): State<SharedState>, user: Principal, ctx: PageCtx, Query(q): Query<PageQuery>) -> AppResult<axum::Json<DonePage>> {
    let vc = view_ctx(&state, &user, &ctx.lang).await?;
    let back = one_of(q.back.as_ref(), LOOKBACKS, "month");
    let all_cals = events::visible_calendars(&state, &user, None).await?;
    let picked = pick_calendars(&all_cals, q.cals.as_deref());
    let days = lookback_days(back);
    let mut done: Vec<Task> = tasks::tasks_for(&state, &picked, vc.tz).await?.into_iter().filter(|t| !t.open() && in_lookback(t, vc.today, days)).collect();
    done.sort_by(by_completion);
    let total = done.len();
    let offset = q.offset.unwrap_or(0).min(total);
    let limit = q.limit.unwrap_or(DONE_CHUNK).clamp(1, 500);
    let items: Vec<Task> = done.into_iter().skip(offset).take(limit).collect();
    Ok(axum::Json(DonePage { total, offset, items }))
}

#[derive(serde::Deserialize, Default)]
pub struct PageQuery {
    #[serde(default)]
    pub back: Option<String>,
    #[serde(default)]
    pub cals: Option<String>,
    #[serde(default)]
    pub offset: Option<usize>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(serde::Serialize)]
pub struct DonePage {
    pub total: usize,
    pub offset: usize,
    pub items: Vec<Task>,
}

pub fn router() -> Router<SharedState> {
    Router::new().route("/tasks", get(page)).route("/api/tasks/done", get(done_page))
}

#[cfg(test)]
mod pinned;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lookback_cookie_is_found_among_others() {
        let mut h = axum::http::HeaderMap::new();
        h.insert(axum::http::header::COOKIE, "id=abc; tk_back=year; x=1".parse().unwrap());
        assert_eq!(back_cookie(&h).as_deref(), Some("year"));
        assert_eq!(back_cookie(&axum::http::HeaderMap::new()), None);
    }

    #[test]
    fn every_column_has_a_label_and_the_defaults_exist() {
        assert_eq!(COLUMNS.iter().map(|(c, _)| *c).collect::<Vec<_>>(), ALL_COLUMNS);
        assert!(DEFAULT_COLUMNS.iter().all(|c| ALL_COLUMNS.contains(c)));
    }
}
