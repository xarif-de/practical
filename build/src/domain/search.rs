//! Universal search (top bar): events, contacts and tasks ranked by relevance, and a "jump to"
//! suggestion when the query looks like a date.
//!
//! Matching (`score_doc`): the query is folded (lower case, diacritics off) and split into words;
//! EVERY word must match some field. A word scores by how it matches — a whole word 1.0, a word
//! prefix .85, run together over word boundaries from a word's start .75 (4+ letters: "dietmarstein" ~
//! "Dietmar Steinbach"), starts of consecutive words .65 (5+ letters, each part 2+: "dietstein"),
//! inside a word .6 (3+ letters), a typo (edit distance 1 from 5 letters on, 2 from 8,
//! against a word's start; a 4-letter word would match too much: "demo" ~ "devops") .5/.38 — times the field's weight; the document's text score is the
//! mean over the words plus a bonus when the whole query starts or equals the main field.
//! Tiers (`search`): exact kinds first; while no clear hit (≥ .85) and fewer than 3 above the floor,
//! loosen — typos/letters left out in the fields that carry them, then everywhere with up to half the
//! words missing (dropped again when some document has every word). The floor (`cutoff`) is relative to
//! the best: clear best hits drop what is far below them, a vague best keeps nearly everything.
//! Ranking adds per kind: events the nearness of the next (else the last) occurrence, upcoming
//! weighing more than past; tasks being open and a near due date.
//!
//! Cost: the searchable corpus of a user (±2 years of occurrences, one document per series; all
//! contacts; all tasks — pre-folded) is built once and kept in `IndexCache`, keyed by a fingerprint
//! of the user's collections' synctokens (one cheap query): any change, also over CalDAV, rebuilds
//! it; nothing else does. A search then only scores in memory.
//!
//! - [`IndexCache`] is per user and dropped after 30 min idle; a change rebuilds only the parts it touched.
//! - Kind prefixes (query.rs): `termin:`/`event:`, `kontakt:`/`contact:`, `aufgabe:`/`task:`, plus French and
//!   Spanish.
//! - Answer sizes: the drop-down asks for 5 per kind, the "all results" dialog for 200 per kind
//!   (`/api/search`, [`crate::routes::api`]).
//! - Title templates ([`suggest`]): past events offered while a new event's title is typed; at least 2
//!   letters, at most 3 offers (`/api/events/suggest`). What the new event takes over is the client's
//!   (js/editor/templates.js, docs/repo-overview.md).

use crate::domain::collections;
use crate::domain::contacts;
use crate::domain::events::{self, Instance};
use crate::domain::tasks;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime};
use rustical_store::auth::Principal;
use sqlx::Row;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Debug, serde::Serialize)]
pub struct SearchResult {
    pub jump: Option<JumpTo>,
    /// the category a prefix narrowed the search to ("event" | "contact" | "task")
    pub scope: Option<&'static str>,
    pub events: Vec<EventHit>,
    pub contacts: Vec<ContactHit>,
    /// open tasks first; also used by the event editor's "link a task" search
    pub tasks: Vec<TaskHit>,
    /// matches per category before the limit
    pub totals: Totals,
}

#[derive(Debug, Default, serde::Serialize)]
pub struct Totals {
    pub events: usize,
    pub contacts: usize,
    pub tasks: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TaskHit {
    pub key: String,
    pub uid: String,
    pub summary: String,
    pub due: Option<NaiveDate>,
    pub completed: bool,
    pub cal_name: String,
    pub color: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub why: Vec<Why>,
}

/// Why a hit was found: a field (code, the client names it), its text around the match, whether the
/// row shows that field anyway, and how each query word matched in it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Why {
    pub f: &'static str,
    pub s: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub shown: bool,
    pub hits: Vec<WhyHit>,
}

/// One query word's match: the word, the kind (`Kind::code`) and, for a typo, the word it resembles.
#[derive(Debug, Clone, serde::Serialize)]
pub struct WhyHit {
    pub w: String,
    pub k: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub m: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct JumpTo {
    pub date: NaiveDate,
    pub label: String,
    pub view: &'static str,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct EventHit {
    pub key: String,
    pub uid: String,
    pub summary: String,
    /// the occurrence shown: the next one from today, else the last
    pub date: NaiveDate,
    pub time: String,
    pub color: String,
    pub cal_name: String,
    /// occurrences in the searched window (±2 years); > 1 for a series
    pub count: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub why: Vec<Why>,
}

pub use crate::domain::contacts::ContactHit;

mod query;
mod score;
mod index;
mod rank;
pub use query::*;
pub use score::*;
pub use index::*;
pub use rank::*;

#[cfg(test)]
mod pinned;

#[cfg(test)]
mod tests;
