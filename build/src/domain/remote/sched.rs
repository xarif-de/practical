//! The loop that runs sync rounds, and the lease that keeps two of them off the same link.
//!
//! Two things trigger a round: the minute tick (when `ran_at + every_min` has passed) and a local
//! change, which `tasks::spawn_change_poller` broadcasts within ~2 s. Both go through [`Runner`],
//! whose per-link mutex is what actually serialises them — `tasks::guarded` only catches panics.
//!
//! A round must not react to its own writes. The change poller broadcasts for any collection whose
//! synctoken moved, with no notion of who wrote it, so every pull that writes would otherwise buy a
//! further full round. [`Runner`] remembers the token each round produced and ignores events at or
//! below it.

use super::{Link, round};
use crate::state::{AppState, ChangeEvent, SharedState};
use crate::error::AppResult;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Per-link state the loop keeps in memory: the round lock, and the local synctoken the last round
/// produced (so its own echo is ignored).
#[derive(Default)]
struct LinkState {
    lock: Arc<tokio::sync::Mutex<()>>,
    /// a change event at or below this is this process's own write
    own_token: i64,
    /// a change arrived while a round was running: run once more when it ends
    again: bool,
}

impl LinkState {
    /// Does this caller get the turn? A link with a round in flight is asked to repeat, and a
    /// caller that reports no result to anybody then LEAVES — marking `again` is the whole value of
    /// that flag. Applying a pull of N objects raises N change events of our own, and each one
    /// waiting here would run a further round with its own request to the remote.
    ///
    /// `wait` belongs to the one caller whose result a user is looking at (the "sync now" button).
    fn take_turn(&mut self, wait: bool) -> bool {
        if self.lock.try_lock().is_err() {
            self.again = true;
            return wait;
        }
        true
    }
}

/// How many triggered rounds may run at once, across all links. See the acquire site.
static ROUNDS: std::sync::LazyLock<Arc<tokio::sync::Semaphore>> = std::sync::LazyLock::new(|| Arc::new(tokio::sync::Semaphore::new(4)));

/// A plain mutex over a small map: a handful of links, and every critical section is three field
/// accesses, so nothing here needs a concurrent map.
#[derive(Default)]
pub struct Runner {
    links: Mutex<HashMap<String, LinkState>>,
    /// manual "sync now" per link, so a button cannot be used as a request amplifier
    last_manual: Mutex<HashMap<String, chrono::DateTime<chrono::Utc>>>,
}

impl Runner {
    /// Runs one round under the link's lock. `force_full` reconciles the whole collection.
    ///
    /// A second caller for the same link waits rather than running in parallel: both would read the
    /// same tokens, both would write them last, and one round's progress would be discarded.
    ///
    /// `wait` is what a triggered round does NOT do. Marking `again` and returning is the whole
    /// value of that flag: applying a pull of N objects raises N change events of our own, and each
    /// one waiting here would run a further round with its own request to the remote — measured as
    /// one round per event behind a long one, where a single repeat covers them all. Only a caller
    /// that reports a result to the user (the "sync now" button) waits.
    pub async fn run(&self, state: &AppState, link: &Link, force_full: bool, wait: bool) -> AppResult<round::Report> {
        let lock = {
            let mut m = self.links.lock().unwrap();
            let e = m.entry(link.id.clone()).or_default();
            if !e.take_turn(wait) {
                return Ok(round::Report::default());
            }
            e.lock.clone()
        };
        let _held = lock.lock().await;
        // re-read: the link may have changed while we waited for the lock
        let link = super::link_by_id(state, &link.id).await?;
        self.claim(state, &link).await?;
        // recorded like any failed round, so the link's row says why it stopped
        if let Err(e) = super::may_write(state, &link).await {
            round::store_failure(state, &link, &e).await?;
            return Err(e);
        }
        let (_, dav) = super::dav_for(state, &link).await?;
        let mut out = round::run(state, &link, &dav, force_full).await;
        /* A change that landed WHILE this round ran is not in the change list it read, and the
           event for it was collapsed into `again` rather than starting a second round. One repeat
           picks it up; a second repeat is not needed, because that one's own events arrive after
           the lock is free. */
        if out.is_ok() && std::mem::take(&mut self.links.lock().unwrap().get_mut(&link.id).map(|e| std::mem::take(&mut e.again)).unwrap_or(false)) {
            let link = super::link_by_id(state, &link.id).await?;
            out = round::run(state, &link, &dav, false).await;
        }
        match &out {
            Ok(rep) => {
                let now = current_token(state, &link).await.unwrap_or_default();
                if let Some(e) = self.links.lock().unwrap().get_mut(&link.id) {
                    // everything up to the token this round produced is our own echo
                    e.own_token = now.max(e.own_token);
                    e.again = false;
                }
                if rep.wrote() {
                    tracing::info!("remote sync {}: {rep:?}", link.id);
                }
            }
            Err(e) => {
                tracing::warn!("remote sync {}: {e}", link.id);
                round::store_failure(state, &link, e).await?;
                // a revoked app password invalidates every link of the account, so it is recorded
                // once, on the account, instead of N times
                if matches!(e, crate::error::AppError::Rejected(axum::http::StatusCode::UNAUTHORIZED, _)) {
                    super::account_failed(state, &link.account_id, "err-remote-auth").await?;
                }
            }
        }
        out
    }

    /// Writes the lease. PractiCal is one pod, so the mutex above is the real exclusion; this only
    /// shows a round in flight across a restart.
    async fn claim(&self, state: &AppState, link: &Link) -> AppResult<()> {
        sqlx::query("UPDATE practical_remote_links SET running_since = ? WHERE id = ?")
            .bind(crate::clock::now().to_rfc3339())
            .bind(&link.id)
            .execute(&state.db)
            .await?;
        Ok(())
    }

    pub fn manual_ok(&self, link_id: &str) -> bool {
        let now = chrono::Utc::now();
        let mut m = self.last_manual.lock().unwrap();
        if m.get(link_id).is_some_and(|t| now.signed_duration_since(*t).num_seconds() < 60) {
            return false;
        }
        m.insert(link_id.to_string(), now);
        true
    }

    /// True when a change event is not this process's own round writing.
    fn foreign(&self, link_id: &str, synctoken: i64) -> bool {
        self.links.lock().unwrap().get(link_id).is_none_or(|e| synctoken > e.own_token)
    }
}

async fn current_token(state: &AppState, link: &Link) -> AppResult<i64> {
    use rustical_store::{AddressbookReadStore, CalendarReadStore};
    Ok(match link.kind {
        super::dav::Kind::Cal => state.calendars.get_calendar(&link.owner, &link.local_id, false).await?.synctoken,
        super::dav::Kind::Ab => state.addressbooks.get_addressbook(&link.owner, &link.local_id, false).await?.synctoken,
    })
}

/// Every link whose interval has elapsed, or whose lease is stale from a dead process.
async fn due(state: &AppState) -> AppResult<Vec<Link>> {
    let links = super::all_links(state).await?;
    let now = chrono::Utc::now();
    let stale = |t: &Option<String>| {
        t.as_deref()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .is_none_or(|t| now.signed_duration_since(t.with_timezone(&chrono::Utc)).num_minutes() >= super::LEASE_MINUTES)
    };
    Ok(links
        .into_iter()
        .filter(|l| {
            let ran = l.ran_at.as_deref().and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok());
            let elapsed = ran.is_none_or(|t| now.signed_duration_since(t.with_timezone(&chrono::Utc)).num_minutes() >= l.every_min);
            elapsed && stale(&l.running_since)
        })
        .collect())
}

/// The sync loop. One minute between ticks; a link runs when its own interval elapsed, so the
/// common case is a single tiny `sync-collection` that answers "nothing changed".
pub fn spawn(state: SharedState) {
    if !super::sync_enabled(&state) {
        return;
    }
    let pusher = state.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(60));
        loop {
            tick.tick().await;
            crate::tasks::guarded("remote sync", async {
                let links = match due(&state).await {
                    Ok(l) => l,
                    Err(e) => {
                        tracing::error!("remote sync: listing links: {e}");
                        return;
                    }
                };
                for link in links {
                    // sequential on purpose: a parallel fan-out would hit one server with N
                    // requests at once, and the whole point of the interval is to be gentle
                    let _ = state.remote_runner.run(&state, &link, false, true).await;
                }
            })
            .await;
        }
    });

    // local change → push at once, without waiting for the interval
    tokio::spawn(async move {
        let mut rx = pusher.changes.subscribe();
        loop {
            let ev = match rx.recv().await {
                Ok(ev) => ev,
                // the channel dropped events because this consumer fell behind. A normal state,
                // not a reason to stop pushing until the next restart — but events are missing, so
                // the minute tick is what catches up and this one just carries on.
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("remote push: {n} change event(s) dropped, the interval catches up");
                    continue;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            };
            let ChangeEvent { kind, principal, id, synctoken } = ev;
            let k = if kind == "calendar" { super::dav::Kind::Cal } else { super::dav::Kind::Ab };
            let link = match super::link_for_collection(&pusher, &principal, k, &id).await {
                Ok(Some(l)) if l.two_way => l,
                _ => continue,
            };
            if !pusher.remote_runner.foreign(&link.id, synctoken) {
                continue;
            }
            /* The interval is sequential on purpose; this path spawns, so it needs its own ceiling.
               Every round in flight holds its answer buffer (up to `dav::MAX_RESPONSE`), and
               `MAX_LINKS_TOTAL` of them at once is more than the pod has. Four is past any real
               fan-out and still bounded. */
            let Ok(permit) = ROUNDS.clone().acquire_owned().await else { break };
            let state = pusher.clone();
            tokio::spawn(async move {
                let _permit = permit;
                crate::tasks::guarded("remote push", async {
                    // nobody is looking at this result: a busy link is asked to repeat instead
                    let _ = state.remote_runner.run(&state, &link, false, false).await;
                })
                .await;
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::LinkState;

    /// The rule that keeps a pull's own change events from each running a round of their own.
    #[test]
    fn a_busy_link_is_asked_to_repeat_and_the_caller_leaves() {
        let mut e = LinkState::default();
        assert!(e.take_turn(false), "a free link runs at once");
        assert!(!e.again, "and has nothing to repeat");

        let held = e.lock.clone();
        let _guard = held.try_lock().expect("the lock is free in this test");
        assert!(!e.take_turn(false), "a triggered round leaves while one is in flight");
        assert!(e.again, "and asks the running one to repeat");

        e.again = false;
        assert!(e.take_turn(true), "the button waits for its result");
        assert!(e.again, "and still asks for the repeat, so nothing is lost either way");
    }
}
