//! Change history: every closed change of an event, task or contact, whatever made it — a save in
//! this UI (one change, however many fields), an import (one change for all its objects) or a DAV
//! client (read off RustiCal's change log; one change per object) — with the versions before and
//! after, so it can be undone later. Undo is itself a change; the undone one links to it.
//!
//! RustiCal keeps no old versions (`REPLACE INTO calendarobjects`) and its change log names no
//! content and no author, so this module keeps its own copy: `practical_history_heads` holds the last
//! version it saw of every object (`practical_history_blobs`, content-addressed), `practical_history_cursors`
//! how far it has read each collection's log. `ingest` reads what moved since and records it; UI
//! writes run between `begin` and `Guard::commit`, which hold one lock with the poller, so what
//! they wrote is attributed to them and nothing else is.

use crate::domain::history_text::{self as ht, Store};
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use rustical_ical::{AddressObject, CalendarObject};
use rustical_store::auth::Principal;
use rustical_store::{AddressbookWriteStore, CalendarWriteStore};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::collections::{BTreeMap, HashMap, HashSet};

mod ingest;
mod read;
mod undo;
pub use ingest::*;
pub use read::*;
pub use undo::*;

#[derive(Default)]
pub struct Hist {
    lock: tokio::sync::Mutex<()>,
    /// moves after every committed write of the history tables (ingest, undo, purge): the history
    /// document's cache key (routes/history.rs). Read before the tables, moved after the commit, so a
    /// document read before a write is never stored under the generation after it.
    generation: std::sync::atomic::AtomicU64,
}

impl Hist {
    pub fn generation(&self) -> u64 {
        self.generation.load(std::sync::atomic::Ordering::Acquire)
    }
    fn moved(&self) {
        self.generation.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    }
}

/// Who a change is attributed to. `src`: ui | import | undo (DAV changes are found, never begun).
pub struct Actor {
    pub src: &'static str,
    pub who: String,
    pub label: Option<String>,
    pub reverts: Option<i64>,
}

impl Actor {
    pub fn ui(user: &Principal) -> Actor {
        Actor { src: "ui", who: user.id.clone(), label: None, reverts: None }
    }
    pub fn import(user: &Principal, label: impl Into<String>) -> Actor {
        Actor { src: "import", who: user.id.clone(), label: Some(label.into()), reverts: None }
    }
}

type ObjKey = (Store, String, String, String);
type CollKey = (Store, String, String);

/// A UI write in progress: what it touched is recorded as its change on `commit`.
pub struct Guard<'a> {
    state: &'a AppState,
    _lock: tokio::sync::MutexGuard<'a, ()>,
    actor: Actor,
    objects: HashSet<ObjKey>,
    colls: HashSet<CollKey>,
}

/// Takes the history lock and first records what devices changed meanwhile, so that it is not
/// attributed to this write. A failing history never fails the write itself.
pub async fn begin(state: &AppState, actor: Actor) -> Guard<'_> {
    let lock = state.history.lock.lock().await;
    if let Err(e) = ingest(state, None).await {
        tracing::warn!("history: {e}");
    }
    Guard { state, _lock: lock, actor, objects: HashSet::new(), colls: HashSet::new() }
}

impl Guard<'_> {
    pub fn object(&mut self, store: Store, principal: &str, coll: &str, object_id: &str) {
        self.objects.insert((store, principal.into(), coll.into(), object_id.into()));
    }
    /// every object that moves in the collection is this write's (imports; a new collection)
    pub fn coll(&mut self, store: Store, principal: &str, coll: &str) {
        self.colls.insert((store, principal.into(), coll.into()));
    }
    /// Records the write; the id of its change, if anything changed.
    pub async fn commit(self) -> Option<i64> {
        match ingest(self.state, Some((&self.actor, &self.objects, &self.colls))).await {
            Ok(id) => id,
            Err(e) => {
                tracing::warn!("history: {e}");
                None
            }
        }
    }
}

/// Polls for DAV changes (the change poller's tick); `false` when the read failed.
pub async fn poll(state: &AppState) -> bool {
    let _lock = state.history.lock.lock().await;
    if let Err(e) = ingest(state, None).await {
        tracing::warn!("history: {e}");
        return false;
    }
    true
}

#[cfg(test)]
mod pinned;
