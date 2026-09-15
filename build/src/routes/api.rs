//! JSON/SSE endpoints used by app.js.

use crate::state::SharedState;
use axum::Router;
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::get;
use futures_util::stream::Stream;
use futures_util::StreamExt;
use rustical_store::auth::Principal;
use std::convert::Infallible;

/// Live updates (plan NFR): one SSE stream per browser tab; events are filtered
/// to the collections the user can see (own principal + groups).
async fn changes(State(state): State<SharedState>, user: Principal) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.changes.subscribe();
    let mine: Vec<String> = user.memberships().into_iter().map(str::to_string).collect();
    let stream = tokio_stream_wrapper(rx).filter_map(move |ev| {
        let mine = mine.clone();
        async move {
            match ev {
                Ok(ev) if mine.iter().any(|m| m == &ev.principal) => {
                    Some(Ok(Event::default().event("change").json_data(&ev).unwrap_or_else(|_| Event::default())))
                }
                _ => None,
            }
        }
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

fn tokio_stream_wrapper(
    mut rx: tokio::sync::broadcast::Receiver<crate::state::ChangeEvent>,
) -> impl Stream<Item = Result<crate::state::ChangeEvent, ()>> {
    futures_util::stream::poll_fn(move |cx| {
        let fut = rx.recv();
        futures_util::pin_mut!(fut);
        match fut.poll(cx) {
            std::task::Poll::Ready(Ok(v)) => std::task::Poll::Ready(Some(Ok(v))),
            std::task::Poll::Ready(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => std::task::Poll::Ready(Some(Err(()))),
            std::task::Poll::Ready(Err(_)) => std::task::Poll::Ready(None),
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    })
}

pub fn router() -> Router<SharedState> {
    Router::new().route("/api/changes", get(changes))
}
