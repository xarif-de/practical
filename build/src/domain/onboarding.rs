//! First-start onboarding: which steps a user still has to deal with, and the record of what they
//! already confirmed (`practical_user_settings.onboarding_json`).
//!
//! Two kinds of condition, on purpose:
//!
//! - **Confirmed** ([`Step::Lang`]): a mark in our own table, because the language column of a
//!   fresh user already holds a value nobody chose.
//! - **Either** ([`Step::Tz`], [`Step::Regions`], [`Step::School`]): done by a value the user can
//!   only have put there on purpose, OR by the mark. These columns start EMPTY and no form fills
//!   them by itself: the time-zone picker's first row ("Standard: …") commits `""`, and holidays
//!   have no default at all ([`crate::domain::prefs::Prefs::regions`]). So a non-empty column is
//!   the user's own answer — picking a city answers the time zone even if it is the city that was
//!   proposed (2026-10-09). The mark carries the other answer, "I want none of this", for which
//!   there is no value to store.
//! - **Derived** ([`Step::Calendar`], [`Step::Addressbook`]): the collection exists, created or
//!   imported — no mark needed, the thing itself is the evidence.
//!
//! Public and school holidays are two points, not one: a user may want the one and not the other,
//! and each is set in its own row of the settings (asked for 2026-10-09).
//!
//! [`Record::completed`] is the end, and it also ends the cost: while the setup is open every page
//! view counts the user's collections, and once it is through nothing is read beyond the one
//! column. A user who later deletes every calendar is therefore not asked again (deliberate: this
//! is a FIRST start, and `Einstellungen › Erste Schritte` reopens it on demand). Only the server
//! decides that it is through ([`complete`]): the browser asks, and is refused while a point is
//! open, so a stale or confused tab cannot shut the setup down over the user's head.

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

/// What the user still has to do. The order is the order the hints offer them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Step {
    Lang,
    Tz,
    /// public holiday regions
    Regions,
    /// school holiday regions
    School,
    Calendar,
    Addressbook,
}

impl Step {
    pub fn as_str(self) -> &'static str {
        match self {
            Step::Lang => "lang",
            Step::Tz => "tz",
            Step::Regions => "regions",
            Step::School => "school",
            Step::Calendar => "calendar",
            Step::Addressbook => "addressbook",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "lang" => Step::Lang,
            "tz" => Step::Tz,
            "regions" => Step::Regions,
            "school" => Step::School,
            "calendar" => Step::Calendar,
            "addressbook" => Step::Addressbook,
            _ => return None,
        })
    }
    /// A step that can be answered by a mark ("none, thanks"), as opposed to one the data proves.
    pub fn is_confirmable(self) -> bool {
        matches!(self, Step::Lang | Step::Tz | Step::Regions | Step::School)
    }
}

/// Our record of the user's onboarding (`onboarding_json`; absent = a fresh user).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Record {
    /// schema of this record; an unknown version is read as a fresh one (`get`)
    #[serde(default = "one")]
    pub v: u8,
    /// the confirmed steps ([`Step::is_confirmable`]); unknown names are kept so a downgrade loses nothing
    #[serde(default)]
    pub confirmed: Vec<String>,
    /// everything was satisfied once — stop counting collections on every page view
    #[serde(default)]
    pub completed: bool,
}

fn one() -> u8 {
    1
}

pub const VERSION: u8 = 1;

impl Record {
    pub fn has(&self, s: Step) -> bool {
        self.confirmed.iter().any(|c| c == s.as_str())
    }
    fn add(&mut self, s: Step) {
        if !self.has(s) {
            self.confirmed.push(s.as_str().to_string());
        }
    }
    /// The steps still open, given what the user already has.
    pub fn open(&self, f: Facts) -> Vec<Step> {
        let mut v = vec![];
        if !self.has(Step::Lang) {
            v.push(Step::Lang);
        }
        // a zone the user picked answers this point as well as the mark does: the picker's
        // "Standard" row stores nothing, so a stored zone is always an explicit choice
        if !self.has(Step::Tz) && !f.tz_set {
            v.push(Step::Tz);
        }
        if !self.has(Step::Regions) && f.regions == 0 {
            v.push(Step::Regions);
        }
        if !self.has(Step::School) && f.school == 0 {
            v.push(Step::School);
        }
        if f.calendars == 0 {
            v.push(Step::Calendar);
        }
        if f.addressbooks == 0 {
            v.push(Step::Addressbook);
        }
        v
    }
    /// Whether anything should be shown at all (the gate and the hints).
    pub fn active(&self) -> bool {
        !self.completed
    }
}

/// What the user already has, as the open list needs it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Facts {
    pub calendars: usize,
    pub addressbooks: usize,
    /// a time zone of the user's own, not the server's proposal
    pub tz_set: bool,
    /// named public holiday regions
    pub regions: usize,
    /// named school holiday regions
    pub school: usize,
}

/// What the browser gets (`<script id="onboarding">` in the page frame, and `GET /api/onboarding`).
/// Only what a hint actually says: the region lists were in here too and no hint ever named them.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct View {
    /// the open steps, in the order they are offered ("lang" first); empty = nothing left to do
    pub open: Vec<String>,
    /// the zone the time-zone hint proposes (`%tz`)
    pub tz: String,
}

/// The browser's view of an active record: counts the user's collections and names the zone the
/// hints talk about. `None` once the record is through, and then nothing is counted — that is what
/// keeps a finished user's page views at the single `get_look` statement. While it is open, one
/// further read of the preferences row is the price of the two holiday lists and the zone.
pub async fn view(state: &crate::state::SharedState, principal: &str, rec: &Record) -> Option<View> {
    if !rec.active() {
        return None;
    }
    let p = crate::domain::prefs::get(&state.db, principal).await.unwrap_or_else(|_| crate::domain::prefs::Prefs::defaults());
    let facts = facts(state, principal, &p).await;
    Some(View {
        open: rec.open(facts).iter().map(|s| s.as_str().to_string()).collect(),
        tz: p.timezone.unwrap_or_else(|| state.config.default_timezone.clone()),
    })
}

/// What the user already has, from their preferences and their collections. Takes the preferences
/// the caller has read, so no page view reads that row twice.
pub async fn facts(state: &crate::state::SharedState, principal: &str, p: &crate::domain::prefs::Prefs) -> Facts {
    use rustical_store::{AddressbookReadStore, CalendarReadStore};
    Facts {
        calendars: state.calendars.get_calendars(principal).await.map_or(0, |v| v.len()),
        addressbooks: state.addressbooks.get_addressbooks(principal).await.map_or(0, |v| v.len()),
        tz_set: p.timezone.as_deref().is_some_and(|z| !z.trim().is_empty()),
        regions: p.regions(false).len(),
        school: p.regions(true).len(),
    }
}

/// Reads the record. A row without JSON, broken JSON or a newer `v` reads as a fresh record: the
/// worst case is that the user is offered the first steps again, never a 500.
pub async fn get(db: &SqlitePool, principal: &str) -> Result<Record, sqlx::Error> {
    let raw: Option<Option<String>> = sqlx::query_scalar("SELECT onboarding_json FROM practical_user_settings WHERE principal = ?")
        .bind(principal)
        .fetch_optional(db)
        .await?;
    Ok(parse(raw.flatten().as_deref()))
}

/// The record from its stored JSON (`None`, unparsable or a newer `v` → fresh).
pub fn parse(raw: Option<&str>) -> Record {
    raw.and_then(|s| serde_json::from_str::<Record>(s).ok()).filter(|r| r.v <= VERSION).unwrap_or_default()
}

/// Writes the record. Its own statement, not part of [`crate::domain::prefs::set`]: that one writes
/// every column from a value read earlier, which would carry a stale record back over a fresh mark.
pub async fn put(db: &SqlitePool, principal: &str, rec: &Record) -> Result<(), sqlx::Error> {
    let json = serde_json::to_string(rec).unwrap_or_default();
    sqlx::query(
        "INSERT INTO practical_user_settings (principal, onboarding_json) VALUES (?, ?)
         ON CONFLICT(principal) DO UPDATE SET onboarding_json = excluded.onboarding_json, updated_at = CURRENT_TIMESTAMP",
    )
    .bind(principal)
    .bind(json)
    .execute(db)
    .await?;
    Ok(())
}

/// Records a confirmation. Hold the prefs guard across read and write.
pub async fn confirm(db: &SqlitePool, principal: &str, step: Step) -> Result<Record, sqlx::Error> {
    let mut rec = get(db, principal).await?;
    rec.v = VERSION;
    rec.add(step);
    put(db, principal, &rec).await?;
    Ok(rec)
}

/// Ends the setup, but only when nothing is open: the browser asks for this, and a tab working from
/// a stale list would otherwise mark a user done with points still to do (audit, 2026-10-09).
/// Answers whether it took.
pub async fn complete(db: &SqlitePool, principal: &str, facts: Facts) -> Result<bool, sqlx::Error> {
    let mut rec = get(db, principal).await?;
    if !rec.open(facts).is_empty() {
        return Ok(false);
    }
    rec.v = VERSION;
    rec.completed = true;
    put(db, principal, &rec).await?;
    Ok(true)
}

/// Reopens it from the settings: forget the completion, keep the confirmations.
pub async fn reopen(db: &SqlitePool, principal: &str) -> Result<(), sqlx::Error> {
    let mut rec = get(db, principal).await?;
    rec.v = VERSION;
    rec.completed = false;
    put(db, principal, &rec).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_record_opens_every_step() {
        let r = Record::default();
        assert_eq!(r.open(Facts::default()), vec![Step::Lang, Step::Tz, Step::Regions, Step::School, Step::Calendar, Step::Addressbook]);
        assert!(r.active());
    }

    /// Everything a user could already have, so a test can take one thing away at a time.
    const ALL: Facts = Facts { calendars: 1, addressbooks: 1, tz_set: true, regions: 1, school: 1 };

    #[test]
    fn a_collection_closes_its_own_step_only() {
        let r = Record::default();
        assert_eq!(r.open(Facts { calendars: 0, ..ALL }), vec![Step::Lang, Step::Calendar]);
        assert_eq!(r.open(Facts { addressbooks: 0, ..ALL }), vec![Step::Lang, Step::Addressbook]);
        assert_eq!(r.open(ALL), vec![Step::Lang], "only the language still wants its mark");
    }

    /// The picker's "Standard" row stores nothing, so a stored zone can only be one the user
    /// picked — including the city that was proposed to them.
    #[test]
    fn a_stored_time_zone_answers_the_time_zone_point() {
        let r = Record::default();
        assert_eq!(r.open(Facts { tz_set: false, ..ALL }), vec![Step::Lang, Step::Tz]);
        assert_eq!(r.open(Facts { tz_set: true, ..ALL }), vec![Step::Lang], "picking a city is the answer");
        let mut declined = Record::default();
        declined.add(Step::Tz);
        assert_eq!(declined.open(Facts { tz_set: false, ..ALL }), vec![Step::Lang], "and so is the mark");
    }

    /// Public and school holidays are separate points: each is closed by its own region or its own
    /// mark, and neither answers for the other.
    #[test]
    fn the_two_holiday_points_are_independent() {
        let r = Record::default();
        assert_eq!(r.open(Facts { regions: 0, school: 0, ..ALL }), vec![Step::Lang, Step::Regions, Step::School]);
        assert_eq!(r.open(Facts { regions: 1, school: 0, ..ALL }), vec![Step::Lang, Step::School]);
        assert_eq!(r.open(Facts { regions: 0, school: 1, ..ALL }), vec![Step::Lang, Step::Regions]);
        let mut only_public_declined = Record::default();
        only_public_declined.add(Step::Regions);
        assert_eq!(
            only_public_declined.open(Facts { regions: 0, school: 0, ..ALL }),
            vec![Step::Lang, Step::School],
            "declining public holidays says nothing about school holidays"
        );
    }

    #[test]
    fn confirmations_close_their_steps() {
        let mut r = Record::default();
        r.add(Step::Lang);
        r.add(Step::Lang); // twice is once
        r.add(Step::Tz);
        assert_eq!(r.confirmed, vec!["lang", "tz"]);
        assert_eq!(r.open(Facts { tz_set: false, regions: 0, school: 0, ..ALL }), vec![Step::Regions, Step::School]);
    }

    #[test]
    fn completed_is_the_end() {
        let mut r = Record::default();
        r.completed = true;
        assert!(!r.active(), "a finished setup never returns by itself");
        assert_eq!(r.open(Facts::default()).len(), 6, "the open list stays truthful; `active` decides whether it is shown");
    }

    #[test]
    fn unreadable_or_newer_records_read_as_fresh() {
        assert_eq!(parse(None).confirmed.len(), 0);
        assert_eq!(parse(Some("not json")).confirmed.len(), 0);
        assert_eq!(parse(Some(r#"{"v":99,"confirmed":["lang"]}"#)).confirmed.len(), 0, "a newer record is not guessed at");
        // a record written before the nap was dropped: the unknown field is ignored, the rest reads
        let r = parse(Some(r#"{"v":1,"confirmed":["lang","tz"],"snoozed_at":"2026-10-09T12:00:00Z"}"#));
        assert!(r.has(Step::Lang) && r.has(Step::Tz) && !r.has(Step::Regions));
        assert!(!r.completed);
    }

    #[test]
    fn unknown_confirmations_survive_a_round_trip() {
        // a record written by a newer build, read and written by this one: nothing of it is dropped
        let r = parse(Some(r#"{"v":1,"confirmed":["lang","something-later"]}"#));
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("something-later"));
    }

    #[test]
    fn step_names_round_trip() {
        for s in [Step::Lang, Step::Tz, Step::Regions, Step::School, Step::Calendar, Step::Addressbook] {
            assert_eq!(Step::parse(s.as_str()), Some(s));
        }
        assert_eq!(Step::parse("nonsense"), None);
        assert!([Step::Lang, Step::Tz, Step::Regions, Step::School].iter().all(|s| s.is_confirmable()));
        assert!(!Step::Calendar.is_confirmable() && !Step::Addressbook.is_confirmable());
    }
}
