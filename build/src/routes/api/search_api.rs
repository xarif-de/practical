//! /api/search, /api/events/suggest, /api/search/history.

use super::*;

#[derive(Deserialize)]
pub(super) struct SearchQuery {
    #[serde(deserialize_with = "crate::extract::text")]
    pub(super) q: String,
    /// "1": the "all results" dialog (200 per category) instead of the drop-down's 5
    #[serde(default)]
    all: Option<String>,
}

pub(super) async fn api_search(State(state): State<SharedState>, user: Principal, ctx: crate::ctx::PageCtx, Query(q): Query<SearchQuery>) -> AppResult<Json<search::SearchResult>> {
    let vc = view_ctx(&state, &user, &ctx.lang).await?;
    let limit = if q.all.as_deref() == Some("1") { 200 } else { 5 };
    Ok(Json(search::search(&state, &user, &q.q, vc.today, vc.tz, &ctx.lang, limit).await?))
}

#[derive(Deserialize)]
pub(super) struct SuggestQuery {
    #[serde(deserialize_with = "crate::extract::text")]
    pub(super) q: String,
}

/// The event editor's title suggestions for a new event (`search::suggest`): three past events.
pub(super) async fn event_suggest(State(state): State<SharedState>, user: Principal, ctx: crate::ctx::PageCtx, Query(q): Query<SuggestQuery>) -> AppResult<Json<Vec<search::Suggestion>>> {
    let vc = view_ctx(&state, &user, &ctx.lang).await?;
    let now = crate::clock::now().with_timezone(&vc.tz).naive_local();
    Ok(Json(search::suggest(&state, &user, &q.q, now, vc.tz, &ctx.lang, 3).await?))
}

pub(super) async fn history_get(State(state): State<SharedState>, user: Principal) -> AppResult<Json<Vec<crate::domain::search_history::Entry>>> {
    let days = prefs::get(&state.db, &user.id).await?.history_days();
    Ok(Json(crate::domain::search_history::list(&state.db, &user.id, days).await?))
}

#[derive(Deserialize)]
pub(super) struct HistoryIn {
    pub(super) q: String,
    #[serde(default)]
    pub(super) target: Option<serde_json::Value>,
}

pub(super) async fn history_post(State(state): State<SharedState>, user: Principal, session: Session, headers: HeaderMap, Json(b): Json<HistoryIn>) -> AppResult<Json<bool>> {
    check_csrf(&session, &headers).await?;
    if b.q.chars().count() > 200 || b.target.as_ref().is_some_and(|t| t.to_string().len() > 2000) {
        return Err(AppError::bad("err-too-long"));
    }
    let days = prefs::get(&state.db, &user.id).await?.history_days();
    crate::domain::search_history::record(&state.db, &user.id, &b.q, b.target.as_ref(), days).await?;
    Ok(Json(days != 0))
}

pub(super) async fn history_clear(State(state): State<SharedState>, user: Principal, session: Session, headers: HeaderMap) -> AppResult<Json<bool>> {
    check_csrf(&session, &headers).await?;
    crate::domain::search_history::clear(&state.db, &user.id).await?;
    Ok(Json(true))
}

pub(super) async fn history_delete(State(state): State<SharedState>, user: Principal, session: Session, headers: HeaderMap, Path(id): Path<i64>) -> AppResult<Json<bool>> {
    check_csrf(&session, &headers).await?;
    crate::domain::search_history::remove(&state.db, &user.id, id).await?;
    Ok(Json(true))
}
