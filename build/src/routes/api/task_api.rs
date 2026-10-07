//! /api/tasks: create, read, update, delete.

use super::*;

pub(super) async fn create_task(State(state): State<SharedState>, user: Principal, ctx: crate::ctx::PageCtx, session: Session, headers: HeaderMap, Json(input): Json<TaskInput>) -> AppResult<Json<Created>> {
    check_csrf(&session, &headers).await?;
    let vc = view_ctx(&state, &user, &ctx.lang).await?;
    let (p, c) = (input.principal.clone(), input.cal_id.clone());
    let mut g = history::begin(&state, Actor::ui(&user)).await;
    let id = tasks::create_task(&state, &user, input, vc.tz).await?;
    g.object(Store::Cal, &p, &c, &id);
    g.commit().await;
    Ok(Json(Created { id }))
}

pub(super) async fn get_task(State(state): State<SharedState>, user: Principal, ctx: crate::ctx::PageCtx, Path((p, c, id)): Path<(String, String, String)>) -> AppResult<Json<TaskDetail>> {
    let vc = view_ctx(&state, &user, &ctx.lang).await?;
    let mut d = tasks::get_task(&state, &user, &p, &c, &id, vc.tz).await?;
    events::resolve_related(&state, &user, &mut d.related, vc.tz).await?;
    Ok(Json(d))
}

#[derive(Deserialize)]
pub(super) struct TaskUpdate {
    #[serde(flatten)]
    pub(super) input: TaskInput,
    /// Sent by the tick boxes instead of a full input; `None` means "take the input as it is"
    #[serde(default)]
    completed: Option<bool>,
    #[serde(default)]
    pub(super) etag: Option<String>,
}

pub(super) async fn update_task(State(state): State<SharedState>, user: Principal, ctx: crate::ctx::PageCtx, session: Session, headers: HeaderMap, Path((p, c, id)): Path<(String, String, String)>, Json(u): Json<TaskUpdate>) -> AppResult<Json<Created>> {
    check_csrf(&session, &headers).await?;
    let vc = view_ctx(&state, &user, &ctx.lang).await?;
    let mut g = history::begin(&state, Actor::ui(&user)).await;
    g.object(Store::Cal, &p, &c, &id);
    match u.completed {
        // the tick box: "done"/"open" whatever else changed meanwhile; a version, when given, must match
        Some(done) => {
            if let Ok(etag) = precondition(u.etag.as_deref(), &headers) {
                crate::domain::collections::ensure_owner(&user, &p)?;
                if state.calendars.get_object(&p, &c, &id, false).await?.get_etag() != etag {
                    return Err(AppError::conflict("err-changed-task"));
                }
            }
            tasks::set_completed(&state, &user, &p, &c, &id, done, vc.tz).await?
        }
        None => {
            let etag = precondition(u.etag.as_deref(), &headers)?;
            if u.input.principal != p || u.input.cal_id != c {
                return Err(AppError::bad("err-task-calendar"));
            }
            tasks::update_task(&state, &user, &id, u.input, Some(&etag), vc.tz).await?
        }
    }
    g.commit().await;
    Ok(Json(Created { id }))
}

pub(super) async fn delete_task(State(state): State<SharedState>, user: Principal, session: Session, headers: HeaderMap, Path((p, c, id)): Path<(String, String, String)>) -> AppResult<Json<Created>> {
    check_csrf(&session, &headers).await?;
    let mut g = history::begin(&state, Actor::ui(&user)).await;
    g.object(Store::Cal, &p, &c, &id);
    tasks::delete_task(&state, &user, &p, &c, &id).await?;
    g.commit().await;
    Ok(Json(Created { id }))
}
