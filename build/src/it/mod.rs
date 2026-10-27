//! Route-level tests: the real router (`build_router`) on a temporary RustiCal database
//! (`testkit::TestApp`), driven like the browser and app.js drive it.

mod api_events;
mod api_more;
mod assets;
mod auth;
mod caches;
mod connect;
mod guards;
mod history;
mod http;
mod onboarding;
mod remote;
mod pages;
mod poison;
pub mod seed;
mod sessions;
mod settings;
mod sharing;
mod units;
mod uploader;
mod validators;
