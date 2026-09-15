//! Background loops (tokio intervals, like RustiCal's own tasks.rs).

use crate::state::{ChangeEvent, SharedState};
use sqlx::Row;
use std::collections::HashMap;
use std::time::Duration;

/// Polls the collections' synctokens (26 µs per query measured on the real store)
/// and broadcasts the ones that moved — the SSE source for live updates.
pub fn spawn_change_poller(state: SharedState) {
    tokio::spawn(async move {
        let mut seen: HashMap<(String, String, String), i64> = HashMap::new();
        let mut first = true;
        let mut tick = tokio::time::interval(Duration::from_secs(2));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            for (kind, table) in [("calendar", "calendars"), ("addressbook", "addressbooks")] {
                let rows = match match table {
                    "calendars" => sqlx::query("SELECT principal, id, synctoken FROM calendars").fetch_all(&state.db).await,
                    _ => sqlx::query("SELECT principal, id, synctoken FROM addressbooks").fetch_all(&state.db).await,
                } {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::warn!("change poll: {e}");
                        continue;
                    }
                };
                for r in rows {
                    let key = (kind.to_string(), r.get::<String, _>("principal"), r.get::<String, _>("id"));
                    let tok: i64 = r.get("synctoken");
                    let prev = seen.insert(key.clone(), tok);
                    if !first && prev != Some(tok) {
                        let _ = state.changes.send(ChangeEvent { kind, principal: key.1, id: key.2, synctoken: tok });
                    }
                }
            }
            first = false;
        }
    });
}

/// Final deletion of trashed groups after the retention period (plan D6).
pub fn spawn_trash_purger(state: SharedState) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(3600));
        loop {
            tick.tick().await;
            match crate::domain::groups::purge_expired(&state).await {
                Ok(0) => {}
                Ok(n) => tracing::info!("trash purge: {n} group(s) deleted"),
                Err(e) => tracing::error!("trash purge: {e}"),
            }
        }
    });
}
