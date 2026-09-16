//! /tasks — plan D15 minimal view.

use crate::ctx::PageCtx;
use crate::domain::collections::CalInfo;
use crate::domain::events;
use crate::domain::tasks::{self, Task};
use crate::error::AppResult;
use crate::routes::calendar::view_ctx;
use crate::state::SharedState;
use askama::Template;
use askama_web::WebTemplate;
use axum::Router;
use axum::extract::State;
use axum::routing::get;
use rustical_store::auth::Principal;

#[derive(Template, WebTemplate)]
#[template(path = "tasks.html")]
pub struct TasksPage {
    pub ctx: PageCtx,
    pub calendars: Vec<CalInfo>,
    pub open: Vec<Task>,
    pub done: Vec<Task>,
    pub today: chrono::NaiveDate,
    pub tasks_json: String,
}

async fn page(State(state): State<SharedState>, user: Principal, ctx: PageCtx) -> AppResult<TasksPage> {
    let vc = view_ctx(&state, &user, &ctx.lang).await?;
    let calendars: Vec<CalInfo> = events::visible_calendars(&state, &user, None).await?.into_iter().filter(|c| c.components.iter().any(|x| x == "VTODO")).collect();
    let all = tasks::tasks_for(&state, &calendars, vc.tz).await?;
    let tasks_json = serde_json::to_string(&all).unwrap_or_else(|_| "[]".into());
    let (done, open): (Vec<Task>, Vec<Task>) = all.into_iter().partition(|t| t.completed);
    Ok(TasksPage { ctx, calendars, open, done, today: vc.today, tasks_json })
}

pub fn router() -> Router<SharedState> {
    Router::new().route("/tasks", get(page))
}
