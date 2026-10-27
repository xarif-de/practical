//! Background loops (tokio intervals, like RustiCal's own tasks.rs).
//!
//! `serve` (main.rs) spawns them at start; the crate docs table their periods and work ("Background loops").
//! Every round runs inside [`guarded`]. The reminder loop lives in [`crate::domain::push`].

use crate::state::{ChangeEvent, SharedState};
use sqlx::Row;
use std::collections::HashMap;
use std::time::Duration;

/// Runs one round of a background loop: a panic inside it is logged and the loop goes on. A bare
/// `tokio::spawn` loop dies silently with its first panic — on 2026-10-05 a schema change killed the
/// push reminder loop that way (perf-baseline.md §0). `None` = the round panicked.
pub async fn guarded<T>(name: &str, round: impl std::future::Future<Output = T>) -> Option<T> {
    use futures_util::FutureExt;
    match std::panic::AssertUnwindSafe(round).catch_unwind().await {
        Ok(v) => Some(v),
        Err(p) => {
            let msg = p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_default();
            tracing::error!("{name}: a round panicked, the loop goes on: {msg}");
            None
        }
    }
}

/// The change poller's period: 2 s; `PRACTICAL__DEV_POLL_MS` (development only, config.rs) sets another.
pub fn change_poll(dev_poll_ms: Option<u64>) -> Duration {
    dev_poll_ms.map_or(Duration::from_secs(2), Duration::from_millis)
}

/// Polls the collections' synctokens (26 µs per query measured on the real store)
/// and broadcasts the ones that moved — the SSE source for live updates. Every 2 s (`change_poll`), but the six
/// queries (history + synctokens) only run when `db::Watch` saw a commit since the last round that
/// succeeded: without a write nothing they read can have moved.
pub fn spawn_change_poller(state: SharedState) {
    tokio::spawn(async move {
        let mut seen: HashMap<(String, String, String), i64> = HashMap::new();
        let mut first = true;
        let mut watch = crate::db::Watch::new(state.db.clone());
        let mut last: Option<(u64, i64)> = None;
        let mut tick = tokio::time::interval(change_poll(state.config.dev_poll_ms));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            // the stamp BEFORE the reads: a commit racing them moves it, so the next round looks again
            let stamp = watch.stamp().await;
            if stamp.is_some() && stamp == last {
                continue;
            }
            last = None;
            if let Some(true) = guarded("change poller", poll_changes(&state, &mut seen, first)).await {
                last = stamp;
            }
            first = false;
        }
    });
}

/// One round of the change poller; `false` when a read failed (the next round then reads again).
async fn poll_changes(state: &SharedState, seen: &mut HashMap<(String, String, String), i64>, first: bool) -> bool {
    // the change history first: a page reloading on the SSE event below then finds it recorded
    let mut ok = crate::domain::history::poll(state).await;
    for (kind, table) in [("calendar", "calendars"), ("addressbook", "addressbooks")] {
        let rows = match match table {
            "calendars" => sqlx::query("SELECT principal, id, synctoken FROM calendars").fetch_all(&state.db).await,
            _ => sqlx::query("SELECT principal, id, synctoken FROM addressbooks").fetch_all(&state.db).await,
        } {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("change poll: {e}");
                ok = false;
                continue;
            }
        };
        for r in rows {
            let key = (kind.to_string(), r.get::<String, _>("principal"), r.get::<String, _>("id"));
            let tok: i64 = r.get("synctoken");
            let prev = seen.insert(key.clone(), tok);
            if !first && prev != Some(tok) {
                // also to every share of it: the SSE stream sends an event only for a home its user may use
                if let Ok(homes) = state.collections().copy_homes(kind, &key.1, &key.2).await {
                    for (home, name) in homes {
                        let _ = state.changes.send(ChangeEvent { kind, principal: home, id: name, synctoken: tok });
                    }
                }
                let _ = state.changes.send(ChangeEvent { kind, principal: key.1, id: key.2, synctoken: tok });
            }
        }
    }
    ok
}

/// Revokes the app passwords whose keep-window ran out (`domain::remote::login::KEEP_SECS`).
///
/// A one-off import does not destroy its credential any more — it parks it, so the next collection
/// from the same server needs no second sign-in. Something has to come back for it afterwards, and
/// nothing else is guaranteed to: a user who imports one calendar and closes the tab would
/// otherwise leave a password valid at their Nextcloud. Runs regardless of
/// `PRACTICAL__REMOTE_SECRET_KEY`, because importing needs no key.
pub fn spawn_remote_login_reaper(state: SharedState) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(60));
        loop {
            tick.tick().await;
            guarded("remote login reaper", async {
                for (origin, login, secret) in state.remote_logins.expired() {
                    crate::domain::remote::login::revoke(&state.remote_http, &origin, &login, &secret).await;
                }
            })
            .await;
        }
    });
}

/// Final deletion of trashed groups after the retention period, the change history's,
/// the push sent-log's and the expired sessions' housekeeping.
/// How long after the start the purger leaves trashed groups alone: longer than a rollout's overlap, while the old
/// pod (which knows no group-trash claims) may still restore or purge the same group
pub(crate) const GROUP_PURGE_DELAY: Duration = Duration::from_secs(15 * 60);

/// Whether this round purges trashed groups ([`GROUP_PURGE_DELAY`]). `tokio::time::Instant` keeps running in
/// development and e2e, where `crate::clock` is frozen.
pub(crate) fn group_purge_due(started: tokio::time::Instant, now: tokio::time::Instant) -> bool {
    now.duration_since(started) >= GROUP_PURGE_DELAY
}

pub fn spawn_trash_purger(state: SharedState) {
    tokio::spawn(async move {
        let started = tokio::time::Instant::now();
        let mut tick = tokio::time::interval(Duration::from_secs(3600));
        loop {
            tick.tick().await;
            guarded("purger", async {
                match crate::domain::history::purge(&state).await {
                    Ok(0) => {}
                    Ok(n) => tracing::info!("history purge: {n} change(s) past the retention"),
                    Err(e) => tracing::error!("history purge: {e}"),
                }
                match if group_purge_due(started, tokio::time::Instant::now()) { crate::domain::groups::purge_expired(&state).await } else { Ok(0) } {
                    Ok(0) => {}
                    Ok(n) => tracing::info!("trash purge: {n} group(s) deleted"),
                    Err(e) => tracing::error!("trash purge: {e}"),
                }
                if let Err(e) = crate::domain::push::purge_sent(&state).await {
                    tracing::error!("push sent-log purge: {e}");
                }
                if let Err(e) = crate::sessions::purge(&state.db).await {
                    tracing::error!("session purge: {e}");
                }
                match crate::domain::tokens::sweep(&state).await {
                    Ok(0) => {}
                    Ok(n) => tracing::info!("device sweep: {n} token(s) never connected within a day"),
                    Err(e) => tracing::error!("device sweep: {e}"),
                }
                match crate::domain::uploader::sweep(&state).await {
                    Ok(0) => {}
                    Ok(n) => tracing::info!("device sweep: {n} PractiCalUploader device(s) never connected within a day"),
                    Err(e) => tracing::error!("device sweep: {e}"),
                }
            })
            .await;
        }
    });
}

/// Development only (`PRACTICAL__DEV_ASSETS_DIR`): polls the mtimes of the asset tree (sub-directories
/// included: the parts of app.js and style.css, src/bundle.rs) every 300 ms and broadcasts on `dev_reload`
/// when anything changed — the browser then reloads (`/dev/reload`).
pub fn spawn_dev_asset_watcher(state: SharedState) {
    let Some(dir) = state.config.dev_assets_dir.clone() else { return };
    tracing::warn!("development mode: serving assets from {dir} and mounting /dev/reload");
    tokio::spawn(async move {
        let mut last: Option<Vec<(String, std::time::SystemTime)>> = None;
        let mut tick = tokio::time::interval(Duration::from_millis(300));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            guarded("asset watcher", async {
                let now = asset_stamps(&dir).await;
                if last.as_ref().is_some_and(|l| *l != now) {
                    tracing::info!("assets changed — browsers reload");
                    let _ = state.dev_reload.send(());
                }
                last = Some(now);
            })
            .await;
        }
    });
}

/// Path and mtime of every file under `dir`, sorted.
pub(crate) async fn asset_stamps(dir: &str) -> Vec<(String, std::time::SystemTime)> {
    let (mut now, mut dirs) = (Vec::new(), vec![std::path::PathBuf::from(dir)]);
    while let Some(d) = dirs.pop() {
        if let Ok(mut rd) = tokio::fs::read_dir(&d).await {
            while let Ok(Some(e)) = rd.next_entry().await {
                let Ok(m) = e.metadata().await else { continue };
                if m.is_dir() {
                    dirs.push(e.path());
                } else if let Ok(t) = m.modified() {
                    now.push((e.path().to_string_lossy().into_owned(), t));
                }
            }
        }
    }
    now.sort();
    now
}

/// Region data upkeep (domain::holidays::maintain): at start and then daily; the first round
/// only reads what is saved, requests go out when a check is due (coverage and CLDR weekly).
pub fn spawn_holiday_coverage(state: SharedState) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(24 * 3600));
        let mut first = true;
        loop {
            tick.tick().await;
            if let Some(Err(e)) = guarded("region data upkeep", crate::domain::holidays::maintain(&state, first)).await {
                tracing::warn!("region data upkeep: {e}");
            }
            first = false;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A change in a shared calendar reaches its sharees' homes too (the SSE stream lets through only a home
    /// its user may use: erin's group gets it, dave nothing of bob's)
    #[tokio::test]
    async fn a_change_fans_out_to_the_shares() {
        let w = crate::it::seed::seed_shared().await;
        let state = w.s.app.state.clone();
        let mut rx = state.changes.subscribe();
        let mut seen = HashMap::new();
        assert!(poll_changes(&state, &mut seen, true).await);
        w.s.app.put_ics("bob", &w.team, "neu", &crate::testkit::vevent("neu", "Neu", "DTSTART:20261020T100000Z\nDTEND:20261020T110000Z")).await;
        assert!(poll_changes(&state, &mut seen, false).await);
        let mut got = vec![];
        while let Ok(ev) = rx.try_recv() {
            got.push((ev.kind, ev.principal, ev.id));
        }
        got.sort();
        let mut want = vec![("calendar", "bob".to_string(), w.team.clone()), ("calendar", "group.team".to_string(), w.team_copy.clone())];
        want.sort();
        assert_eq!(got, want);
    }
}
