//! Domain logic on RustiCal's store and the practical_* tables; routes/ calls in (no extractors or responses here).
//!
//! Layout: `<module>.rs` with its sub-modules in `<module>/`; `<module>/pinned.rs` pins today's behaviour
//! (characterisation tests), `<module>/tests.rs` holds the unit tests. The objects: [`events`], [`tasks`],
//! [`contacts`], their collections ([`collections`], [`groups`], [`tokens`]); the change history
//! ([`history`], [`history_text`]); per-user data ([`prefs`], [`catrules`], [`colors`], [`search_history`]);
//! search ([`search`]); outside sources ([`holidays`], [`geo`]); reminders ([`push`]); [`transfer`]
//! (import/export) and the text helpers in [`ical`].

pub mod catrules;
pub mod collections;
pub mod colors;
pub mod contacts;
pub mod events;
pub mod geo;
pub mod groups;
pub mod history;
pub mod history_text;
pub mod holidays;
pub mod ical;
pub mod onboarding;
pub mod prefs;
pub mod push;
pub mod search;
pub mod search_history;
pub mod tasks;
pub mod tokens;
pub mod transfer;
