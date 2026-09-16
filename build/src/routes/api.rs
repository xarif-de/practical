//! JSON endpoints used by app.js: SSE change stream, event/contact/task CRUD,
//! search, push subscriptions. All mutations need the CSRF header (the origin
//! guard already rejected cross-site requests before we get here).

use crate::csrf;
use crate::domain::contacts::{self, Contact, ContactInput};
use crate::domain::events::{self, EventDetail, EventInput};
use crate::domain::geo;
use crate::domain::search;
use crate::domain::tasks::{self, TaskInput};
use crate::error::{AppError, AppResult};
use crate::routes::calendar::view_ctx;
use crate::state::SharedState;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{get, post};
use axum::Json;
use futures_util::StreamExt;
use futures_util::stream::Stream;
use rustical_store::auth::Principal;
use serde::Deserialize;
use std::convert::Infallible;
use tower_sessions::Session;

/// Live updates (plan NFR): one SSE stream per browser tab; events are filtered
/// to the collections the user can see (own principal + groups).
async fn changes(State(state): State<SharedState>, user: Principal) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.changes.subscribe();
    let mine: Vec<String> = user.memberships().into_iter().map(str::to_string).collect();
    let stream = tokio_stream_wrapper(rx).filter_map(move |ev| {
        let mine = mine.clone();
        async move {
            match ev {
                Ok(ev) if mine.iter().any(|m| m == &ev.principal) => Some(Ok(Event::default().event("change").json_data(&ev).unwrap_or_else(|_| Event::default()))),
                _ => None,
            }
        }
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// Broadcast receiver as a stream. `unfold` keeps the pending `recv()` future between polls, so its
/// waker survives; a fresh `recv()` per poll would drop the waiter and the stream only moved on the
/// SSE keep-alive tick (15 s) — measured 2026-09-16 with /dev/reload.
fn tokio_stream_wrapper(rx: tokio::sync::broadcast::Receiver<crate::state::ChangeEvent>) -> impl Stream<Item = Result<crate::state::ChangeEvent, ()>> {
    futures_util::stream::unfold(rx, |mut rx| async move {
        match rx.recv().await {
            Ok(v) => Some((Ok(v), rx)),
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => Some((Err(()), rx)),
            Err(tokio::sync::broadcast::error::RecvError::Closed) => None,
        }
    })
}

async fn check_csrf(session: &Session, headers: &HeaderMap) -> AppResult<()> {
    let has_basic = headers.get(axum::http::header::AUTHORIZATION).and_then(|v| v.to_str().ok()).is_some_and(|v| v.starts_with("Basic "));
    if has_basic {
        return Ok(());
    }
    csrf::check(session, &csrf::header_token(headers)).await
}

#[derive(serde::Serialize)]
pub struct Created {
    pub id: String,
}

// ---- events ----

pub async fn create_event(State(state): State<SharedState>, user: Principal, ctx: crate::ctx::PageCtx, session: Session, headers: HeaderMap, Json(input): Json<EventInput>) -> AppResult<Json<Created>> {
    check_csrf(&session, &headers).await?;
    let vc = view_ctx(&state, &user, &ctx.lang).await?;
    Ok(Json(Created { id: events::create_event(&state, &user, input, vc.tz).await? }))
}

async fn get_event(State(state): State<SharedState>, user: Principal, ctx: crate::ctx::PageCtx, Path((p, c, id)): Path<(String, String, String)>) -> AppResult<Json<EventDetail>> {
    let vc = view_ctx(&state, &user, &ctx.lang).await?;
    Ok(Json(events::get_event(&state, &user, &p, &c, &id, vc.tz).await?))
}

#[derive(Deserialize)]
struct EventUpdate {
    #[serde(flatten)]
    input: EventInput,
    #[serde(default)]
    etag: Option<String>,
}

async fn update_event(State(state): State<SharedState>, user: Principal, ctx: crate::ctx::PageCtx, session: Session, headers: HeaderMap, Path((p, c, id)): Path<(String, String, String)>, Json(u): Json<EventUpdate>) -> AppResult<Json<Created>> {
    check_csrf(&session, &headers).await?;
    let vc = view_ctx(&state, &user, &ctx.lang).await?;
    if u.input.principal != p || u.input.cal_id != c {
        // moving to another calendar = create there, delete here
        let target = u.input.clone();
        let new_id = events::create_event(&state, &user, target, vc.tz).await?;
        events::delete_event(&state, &user, &p, &c, &id).await?;
        return Ok(Json(Created { id: new_id }));
    }
    events::update_event(&state, &user, &id, u.input, u.etag.as_deref(), vc.tz).await?;
    Ok(Json(Created { id }))
}

async fn delete_event(State(state): State<SharedState>, user: Principal, session: Session, headers: HeaderMap, Path((p, c, id)): Path<(String, String, String)>) -> AppResult<Json<Created>> {
    check_csrf(&session, &headers).await?;
    events::delete_event(&state, &user, &p, &c, &id).await?;
    Ok(Json(Created { id }))
}

// ---- contacts ----

async fn get_contact(State(state): State<SharedState>, user: Principal, Path((p, a, id)): Path<(String, String, String)>) -> AppResult<Json<Contact>> {
    Ok(Json(contacts::get_contact(&state, &user, &p, &a, &id).await?))
}

async fn contact_vcf(State(state): State<SharedState>, user: Principal, Path((p, a, id)): Path<(String, String, String)>) -> AppResult<axum::response::Response> {
    use axum::response::IntoResponse;
    let vcf = contacts::get_vcf(&state, &user, &p, &a, &id).await?;
    Ok(([(axum::http::header::CONTENT_TYPE, "text/vcard; charset=utf-8".to_string()), (axum::http::header::CONTENT_DISPOSITION, format!("attachment; filename=\"{id}.vcf\""))], vcf).into_response())
}

async fn create_contact(State(state): State<SharedState>, user: Principal, session: Session, headers: HeaderMap, Json(input): Json<ContactInput>) -> AppResult<Json<Created>> {
    check_csrf(&session, &headers).await?;
    Ok(Json(Created { id: contacts::create_contact(&state, &user, input).await? }))
}

#[derive(Deserialize)]
struct ContactUpdate {
    #[serde(flatten)]
    input: ContactInput,
    #[serde(default)]
    etag: Option<String>,
}

async fn update_contact(State(state): State<SharedState>, user: Principal, session: Session, headers: HeaderMap, Path((p, a, id)): Path<(String, String, String)>, Json(u): Json<ContactUpdate>) -> AppResult<Json<Created>> {
    check_csrf(&session, &headers).await?;
    if u.input.principal != p || u.input.ab_id != a {
        let new_id = contacts::create_contact(&state, &user, u.input).await?;
        contacts::delete_contact(&state, &user, &p, &a, &id).await?;
        return Ok(Json(Created { id: new_id }));
    }
    contacts::update_contact(&state, &user, &id, u.input, u.etag.as_deref()).await?;
    Ok(Json(Created { id }))
}

async fn delete_contact(State(state): State<SharedState>, user: Principal, session: Session, headers: HeaderMap, Path((p, a, id)): Path<(String, String, String)>) -> AppResult<Json<Created>> {
    check_csrf(&session, &headers).await?;
    contacts::delete_contact(&state, &user, &p, &a, &id).await?;
    Ok(Json(Created { id }))
}

// ---- tasks ----

async fn create_task(State(state): State<SharedState>, user: Principal, session: Session, headers: HeaderMap, Json(input): Json<TaskInput>) -> AppResult<Json<Created>> {
    check_csrf(&session, &headers).await?;
    Ok(Json(Created { id: tasks::create_task(&state, &user, input).await? }))
}

#[derive(Deserialize)]
struct TaskUpdate {
    #[serde(flatten)]
    input: TaskInput,
    #[serde(default)]
    completed: bool,
}

async fn update_task(State(state): State<SharedState>, user: Principal, session: Session, headers: HeaderMap, Path((p, c, id)): Path<(String, String, String)>, Json(u): Json<TaskUpdate>) -> AppResult<Json<Created>> {
    check_csrf(&session, &headers).await?;
    if u.input.principal != p || u.input.cal_id != c {
        return Err(AppError::BadRequest("task cannot change calendar".into()));
    }
    tasks::update_task(&state, &user, &id, u.input, u.completed).await?;
    Ok(Json(Created { id }))
}

async fn delete_task(State(state): State<SharedState>, user: Principal, session: Session, headers: HeaderMap, Path((p, c, id)): Path<(String, String, String)>) -> AppResult<Json<Created>> {
    check_csrf(&session, &headers).await?;
    tasks::delete_task(&state, &user, &p, &c, &id).await?;
    Ok(Json(Created { id }))
}

// ---- search ----

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
}

async fn api_search(State(state): State<SharedState>, user: Principal, ctx: crate::ctx::PageCtx, Query(q): Query<SearchQuery>) -> AppResult<Json<search::SearchResult>> {
    let vc = view_ctx(&state, &user, &ctx.lang).await?;
    Ok(Json(search::search(&state, &user, &q.q, vc.today, vc.tz).await?))
}

// ---- push (plan D12) ----

#[derive(serde::Serialize)]
struct VapidKey {
    key: Option<String>,
}

async fn push_key(State(state): State<SharedState>) -> Json<VapidKey> {
    Json(VapidKey { key: state.vapid.as_ref().map(|k| k.public().encode_b64().0) })
}

#[derive(Deserialize)]
struct SubKeys {
    p256dh: String,
    auth: String,
}
#[derive(Deserialize)]
struct SubscribeBody {
    endpoint: String,
    keys: SubKeys,
    #[serde(default)]
    user_agent: Option<String>,
}

async fn push_subscribe(State(state): State<SharedState>, user: Principal, session: Session, headers: HeaderMap, Json(b): Json<SubscribeBody>) -> AppResult<Json<Created>> {
    check_csrf(&session, &headers).await?;
    if state.vapid.is_none() {
        return Err(AppError::BadRequest("push disabled".into()));
    }
    if !b.endpoint.starts_with("https://") {
        return Err(AppError::BadRequest("endpoint must be https".into()));
    }
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO xui_push_subscriptions (id, principal, endpoint, p256dh, auth, user_agent) VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT(endpoint) DO UPDATE SET principal=excluded.principal, p256dh=excluded.p256dh, auth=excluded.auth, user_agent=excluded.user_agent")
        .bind(&id)
        .bind(&user.id)
        .bind(&b.endpoint)
        .bind(&b.keys.p256dh)
        .bind(&b.keys.auth)
        .bind(b.user_agent.or_else(|| headers.get(axum::http::header::USER_AGENT).and_then(|v| v.to_str().ok()).map(str::to_string)))
        .execute(&state.db)
        .await?;
    Ok(Json(Created { id }))
}

#[derive(Deserialize)]
struct UnsubscribeBody {
    endpoint: String,
}

async fn push_unsubscribe(State(state): State<SharedState>, user: Principal, session: Session, headers: HeaderMap, Json(b): Json<UnsubscribeBody>) -> AppResult<Json<Created>> {
    check_csrf(&session, &headers).await?;
    sqlx::query("DELETE FROM xui_push_subscriptions WHERE principal = ? AND endpoint = ?").bind(&user.id).bind(&b.endpoint).execute(&state.db).await?;
    Ok(Json(Created { id: String::new() }))
}

#[derive(serde::Serialize)]
struct PushSub {
    endpoint: String,
    user_agent: Option<String>,
    created_at: String,
}

async fn push_list(State(state): State<SharedState>, user: Principal) -> AppResult<Json<Vec<PushSub>>> {
    use sqlx::Row;
    let rows = sqlx::query("SELECT endpoint, user_agent, created_at FROM xui_push_subscriptions WHERE principal = ? ORDER BY created_at").bind(&user.id).fetch_all(&state.db).await?;
    Ok(Json(rows.iter().map(|r| PushSub { endpoint: r.get("endpoint"), user_agent: r.get("user_agent"), created_at: r.get("created_at") }).collect()))
}

#[derive(Deserialize)]
struct GeoSearch {
    q: String,
    lat: Option<f64>,
    lon: Option<f64>,
}

#[derive(Deserialize)]
struct GeoReverse {
    lat: f64,
    lon: f64,
}

/// Address suggestions (Photon) — the browser asks only after a typing pause.
async fn geo_search(State(state): State<SharedState>, _user: Principal, ctx: crate::ctx::PageCtx, Query(q): Query<GeoSearch>) -> AppResult<Json<Vec<geo::Place>>> {
    let near = match (q.lat, q.lon) { (Some(a), Some(b)) => Some((a, b)), _ => None };
    Ok(Json(geo::search(&state, &q.q, &ctx.lang, near).await?))
}

/// Address at a position (Nominatim) — one click on the map dialog's button.
async fn geo_reverse(State(state): State<SharedState>, _user: Principal, ctx: crate::ctx::PageCtx, Query(q): Query<GeoReverse>) -> AppResult<Json<geo::Place>> {
    geo::reverse(&state, q.lat, q.lon, &ctx.lang).await?.map(Json).ok_or(AppError::NotFound)
}

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/api/geo/search", get(geo_search))
        .route("/api/geo/reverse", get(geo_reverse))
        .route("/api/changes", get(changes))
        .route("/api/events/{p}/{c}/{id}", get(get_event).put(update_event).delete(delete_event))
        .route("/api/contacts", post(create_contact))
        .route("/api/contacts/{p}/{a}/{id}", get(get_contact).put(update_contact).delete(delete_contact))
        .route("/api/contacts/{p}/{a}/{id}/vcf", get(contact_vcf))
        .route("/api/tasks", post(create_task))
        .route("/api/tasks/{p}/{c}/{id}", axum::routing::put(update_task).delete(delete_task))
        .route("/api/search", get(api_search))
        .route("/api/push/key", get(push_key))
        .route("/api/push/subscriptions", get(push_list).post(push_subscribe).delete(push_unsubscribe))
}
