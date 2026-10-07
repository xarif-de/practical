//! Development only (`PRACTICAL__DEV_ASSETS_DIR`, never set in production): `/dev/reload` is an
//! SSE stream that sends `boot` (this process' start time) once and `reload` whenever an asset file
//! changed (tasks::spawn_dev_asset_watcher). app.js reloads the page on a changed boot id — i.e.
//! after watchexec rebuilt and restarted the server — and on every `reload`.

use crate::state::SharedState;
use axum::Router;
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::get;
use futures_util::StreamExt;
use futures_util::stream::Stream;
use std::convert::Infallible;

async fn reload(State(state): State<SharedState>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let boot = Event::default().event("boot").data(state.started_at.timestamp_millis().to_string());
    let rx = state.dev_reload.subscribe();
    // unfold keeps the recv() future alive between polls (see api.rs tokio_stream_wrapper)
    let reloads = futures_util::stream::unfold(rx, |mut rx| async move {
        match rx.recv().await {
            Ok(()) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => Some((Ok(Event::default().event("reload").data("assets")), rx)),
            Err(tokio::sync::broadcast::error::RecvError::Closed) => None,
        }
    });
    Sse::new(futures_util::stream::iter([Ok(boot)]).chain(reloads).take_until(state.guards.stopping())).keep_alive(KeepAlive::default())
}

/// Empty router unless development mode is on.
pub fn router(dev: bool) -> Router<SharedState> {
    if dev { Router::new().route("/dev/reload", get(reload)) } else { Router::new() }
}
