//! Public and school holidays (plan D13) and the region choice behind them.
//!
//! Sources form a chain, tried in order until one answers (each with its own timeout):
//! - public holidays: OpenHolidays (36 countries, regions down to municipalities) →
//!   Nager.Date (204 countries, regions by ISO 3166-2 code) → the last cached answer;
//! - school holidays: OpenHolidays → the last cached answer (no other free source keeps
//!   them per year; ferien-api.de rate-limits, mehr-schulferien.de only has the next 12 months).
//!
//! Answers are cached per (country, subdivision, year, kind-lang) in `practical_holidays` for 30
//! days; OpenHolidays' subdivision lists the same way (year 0). Country and region *names* and
//! the region hierarchy of countries OpenHolidays does not cover come from Unicode CLDR,
//! built into the binary (`src/data/regions.json`, `tools/gen_regions.py`) — no request needed.
//! Upkeep is lazy (`maintain`, daily from tasks.rs; at start it only reads what is saved):
//! which source covers which country is asked at most weekly (`refresh_coverage`), CLDR is asked
//! at most weekly whether a newer release exists (`check_cldr`: one small request, usually a
//! 304) and only then are its files fetched — about once a year. Saved answers live in
//! `practical_holidays` (year 0) and win over the built-in snapshot when newer.
//!
//! A region is stored as its ISO 3166-2 code where it has one (DE-SN, AT-1), else as
//! OpenHolidays' own code (CH-AG-ZZ, a district); both APIs accept ISO codes. A bare country
//! code (DE) stands for the whole country. A user picks a list of regions per kind
//! (`for_regions`): what several regions share is shown once; what only some of them have
//! carries their short codes ("Buß- und Bettag (SN)").

use crate::error::AppResult;
use crate::state::AppState;
use chrono::NaiveDate;
use sqlx::Row;
use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::Duration;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Holiday {
    pub start: NaiveDate,
    pub end: NaiveDate, // inclusive
    pub name: String,
    pub kind: String, // public | school
    #[serde(default)]
    pub nationwide: bool,
    /// which source answered (openholidays | nager)
    #[serde(default)]
    pub source: String,
    /// the subdivisions (ISO codes) a regional holiday applies to, as the source says
    #[serde(default)]
    pub regions: Vec<String>,
    /// `for_regions`: the chosen regions it applies to when not all of them (short codes, shown)
    #[serde(skip)]
    pub tags: Vec<String>,
}

impl Holiday {
    /// the name as the views show it: "Winterferien (SN, BY)" when it is not every chosen region's
    pub fn label(&self) -> String {
        if self.tags.is_empty() { self.name.clone() } else { format!("{} ({})", self.name, self.tags.join(", ")) }
    }
}

#[derive(serde::Deserialize)]
struct ApiName {
    language: String,
    text: String,
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiHoliday {
    start_date: NaiveDate,
    end_date: NaiveDate,
    name: Vec<ApiName>,
    #[serde(default)]
    nationwide: bool,
    #[serde(default)]
    subdivisions: Vec<ApiSubRef>,
}
#[derive(serde::Deserialize)]
struct ApiSubRef {
    #[serde(default)]
    code: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NagerHoliday {
    date: NaiveDate,
    local_name: String,
    name: String,
    #[serde(default)]
    global: bool,
    #[serde(default)]
    counties: Option<Vec<String>>,
    #[serde(default)]
    types: Vec<String>,
}

const OPENHOLIDAYS: &str = "https://openholidaysapi.org";
const NAGER: &str = "https://date.nager.at/api/v3";
const CACHE_DAYS: i64 = 30;
/// one link of the chain may take this long before the next is asked
const TIMEOUT: Duration = Duration::from_secs(8);

mod names;
use names::Table;

mod cldr;
mod cache;
mod fetch;
mod regions;
pub use cldr::*;
pub use cache::*;
pub use fetch::*;
pub use regions::*;

#[cfg(test)]
mod pinned;

#[cfg(test)]
mod tests;
