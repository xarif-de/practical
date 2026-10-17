//! JSON endpoints used by app.js: SSE change stream, event/contact/task CRUD,
//! search, push subscriptions. All mutations need the CSRF header (the origin
//! guard already rejected cross-site requests before we get here).
//!
//! - `/api/changes` ([`changes`]): the SSE stream of the [`ChangeEvent`](crate::state::ChangeEvent)s the change poller broadcasts.
//! - Create and delete go through one envelope each, [`create_object`] and [`delete_object`].
//! - The body limit is 8 MiB: embedded files ride in the JSON body as base64.
//! - One sub-module per area: events (+ mentions, alarm presets; [`MentionCache`]), categories, contacts,
//!   tasks, search, push, geo, and PractiCalUploader's upload (its own 32 MiB limit).

use crate::csrf;
use crate::domain::contacts::{self, Contact, ContactInput};
use crate::domain::events::{self, EventDetail, EventInput};
use crate::domain::geo;
use crate::domain::history::{self, Actor};
use crate::domain::history_text::Store;
use crate::domain::prefs;
use crate::domain::search;
use crate::domain::tasks::{self, TaskDetail, TaskInput};
use crate::error::{AppError, AppResult};
use crate::routes::calendar::PageView;
use crate::state::SharedState;
use axum::Router;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{get, post};
use crate::extract::{Json, Path, Query};
use futures_util::StreamExt;
use futures_util::stream::Stream;
use rustical_store::auth::Principal;
use rustical_store::CalendarReadStore;
use serde::Deserialize;
use std::convert::Infallible;
use tower_sessions::Session;

mod event_api;
mod category_api;
mod contact_api;
mod task_api;
mod search_api;
mod push_api;
mod geo_api;
mod uploader_api;
pub use event_api::*;
pub use category_api::*;
use contact_api::*;
use task_api::*;
use search_api::*;
use push_api::*;
use geo_api::*;

/// Live updates: one SSE stream per browser tab; events are filtered
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
    // ends when the server stops: an open stream would hold the graceful shutdown until SIGKILL
    Sse::new(stream.take_until(state.guards.stopping())).keep_alive(KeepAlive::default())
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

/// The version a write is based on: the body's `etag` (app.js sends the one it read), else `If-Match`.
/// None: the write would overwrite blindly — refused with 428 (a client re-reads, then writes).
fn precondition(body: Option<&str>, headers: &HeaderMap) -> AppResult<String> {
    body.filter(|e| !e.is_empty())
        .map(str::to_string)
        .or_else(|| headers.get(axum::http::header::IF_MATCH).and_then(|v| v.to_str().ok()).map(|v| v.trim().to_string()).filter(|v| !v.is_empty() && v != "*"))
        .ok_or_else(|| AppError::Rejected(axum::http::StatusCode::PRECONDITION_REQUIRED, crate::error::Msg { key: "err-etag-required", args: vec![] }))
}

/// POST of one event, task or contact: one change in the history. The history guard is taken before
/// `create` runs (a future does nothing before it is awaited), so the write is attributed to the user.
async fn create_object(state: &SharedState, user: &Principal, store: Store, p: &str, c: &str, create: impl Future<Output = AppResult<String>>) -> AppResult<Json<Created>> {
    let mut g = history::begin(state, Actor::ui(user)).await;
    let id = create.await?;
    g.object(store, p, c, &id);
    g.commit().await;
    Ok(Json(Created { id }))
}

/// DELETE of one event, task or contact: one change in the history.
async fn delete_object(state: &SharedState, user: &Principal, store: Store, p: &str, c: &str, id: String) -> AppResult<Json<Created>> {
    let mut g = history::begin(state, Actor::ui(user)).await;
    g.object(store, p, c, &id);
    crate::domain::collections::delete_object(state, user, store, p, c, &id).await?;
    g.commit().await;
    Ok(Json(Created { id }))
}

#[derive(serde::Serialize)]
pub struct Created {
    pub id: String,
}

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/api/alarm-presets", get(alarm_presets_get).put(alarm_presets_put))
        .route("/api/geo/search", get(geo_search))
        .route("/api/geo/reverse", get(geo_reverse))
        .route("/api/changes", get(changes))
        .route("/api/events/suggest", get(event_suggest))
        .route("/api/events/{p}/{c}/{id}", get(get_event).put(update_event).delete(delete_event))
        .route("/api/events/{p}/{c}/{id}/move", post(move_event))
        .route("/api/events/{p}/{c}/{id}/attach/{n}", get(event_attachment))
        .route("/api/contacts", post(create_contact))
        .route("/api/contacts/{p}/{a}/{id}", get(get_contact).put(update_contact).delete(delete_contact))
        .route("/api/contacts/{p}/{a}/{id}/vcf", get(contact_vcf))
        .route("/api/tasks", post(create_task))
        .route("/api/tasks/{p}/{c}/{id}", get(get_task).put(update_task).delete(delete_task))
        .route("/api/search", get(api_search))
        .route("/api/search/history", get(history_get).post(history_post).delete(history_clear))
        .route("/api/search/history/{id}", axum::routing::delete(history_delete))
        .route("/api/mentions", get(mentions))
        .route("/api/categories", get(categories))
        .route("/api/titles", get(titles))
        .route("/api/colors", get(colors_get))
        .route("/api/colors/category", post(color_category))
        .route("/api/colors/batch", post(colors_batch))
        .route("/api/colors/{kind}/{*key}", axum::routing::put(color_put))
        .route("/api/category-rules", get(category_rules_get))
        .route("/api/category-rules/{name}", axum::routing::put(category_rule_put).delete(category_rule_delete))
        .route("/api/push/key", get(push_key))
        .route("/api/push/subscriptions", get(push_list).post(push_subscribe).delete(push_unsubscribe))
        .layer(axum::extract::DefaultBodyLimit::max(8 * 1024 * 1024)) // embedded files ride in the JSON body as base64
        // after the layer above: a snapshot is bigger than an edit (uploader::MAX_BODY)
        .route("/api/uploader/calendars/{id}", axum::routing::put(uploader_api::upload).layer(axum::extract::DefaultBodyLimit::max(crate::domain::uploader::MAX_BODY)))
}
