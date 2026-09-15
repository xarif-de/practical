//! The four main pages. Phase 1 ships the chrome with the collection overview;
//! the calendar/contacts/tasks bodies grow in phase 2.

use crate::ctx::PageCtx;
use crate::domain::collections::{self, AbInfo, CalInfo};
use crate::error::AppResult;
use crate::state::SharedState;
use askama::Template;
use askama_web::WebTemplate;
use axum::Router;
use axum::extract::State;
use axum::response::Redirect;
use axum::routing::get;
use rustical_store::auth::Principal;

#[derive(Template, WebTemplate)]
#[template(path = "calendar.html")]
pub struct CalendarPage {
    pub ctx: PageCtx,
    pub calendars: Vec<CalInfo>,
}

#[derive(Template, WebTemplate)]
#[template(path = "contacts.html")]
pub struct ContactsPage {
    pub ctx: PageCtx,
    pub addressbooks: Vec<AbInfo>,
}

#[derive(Template, WebTemplate)]
#[template(path = "tasks.html")]
pub struct TasksPage {
    pub ctx: PageCtx,
    pub calendars: Vec<CalInfo>,
}

async fn calendar(State(state): State<SharedState>, user: Principal, ctx: PageCtx) -> AppResult<CalendarPage> {
    Ok(CalendarPage { calendars: collections::calendars_for(&state, &user, false).await?, ctx })
}

async fn contacts(State(state): State<SharedState>, user: Principal, ctx: PageCtx) -> AppResult<ContactsPage> {
    Ok(ContactsPage { addressbooks: collections::addressbooks_for(&state, &user, false).await?, ctx })
}

async fn tasks(State(state): State<SharedState>, user: Principal, ctx: PageCtx) -> AppResult<TasksPage> {
    let calendars = collections::calendars_for(&state, &user, false)
        .await?
        .into_iter()
        .filter(|c| c.components.iter().any(|x| x == "VTODO"))
        .collect();
    Ok(TasksPage { calendars, ctx })
}

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/calendar", get(calendar))
        .route("/contacts", get(contacts))
        .route("/tasks", get(tasks))
}

pub async fn root() -> Redirect {
    Redirect::to("/calendar")
}
