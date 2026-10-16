//! PractiCal: a server-rendered web UI (axum + askama) on RustiCal's own SQLite store; this binary runs next to
//! RustiCal in one image and one pod.
//!
//! This page is the entry point of the API doc (`mise run doc`). It explains how a request flows through the
//! server, which module owns what, the rules every module keeps, and the background work. Files outside the
//! crate (assets, templates, locales, build.rs, e2e/, deploy/) are described in `docs/repo-overview.md`.
//!
//! Every source file states its purpose in its first line (`//!`). Read that before opening more. `rg`/`fd`
//! skip generated, vendored and golden data (`.ignore`; search those with `-u`).
//!
//! # How a request flows
//!
//! [`build_router_with`] stacks the layers, outermost first:
//!
//! 1. `TraceLayer` → [`error::catch_panic`] (the unlocalized one) → a `map_response` that adds the
//!    [`content_security_policy`] to HTML and `Cache-Control: private, no-cache` to HTML and JSON.
//! 2. `/assets/{name}` ([`routes::assets`]) and `/healthz` are merged *after* the layers below, so they run
//!    without session, principal lookup, CSRF check or per-request compression.
//! 3. `CompressionLayer` (gzip) → [`etag::revalidate`] (weak ETag, 304, brotli of the identity body) → the
//!    session layer ([`sessions::DbStore`]) → [`sessions::activity`] (extends the expiry) → RustiCal's
//!    `AuthenticationLayer` (the principal from the session or an HTTP Basic app token) →
//!    [`csrf::origin_guard`] → the router.
//! 4. The router: [`auth::router`] (`/login`, `/login/oidc*`, `/logout`), [`routes::connect::public_router`] (an
//!    app's Login Flow v2 start and poll, the only POSTs the origin guard lets through without browser metadata;
//!    `/connect/davx5`, an iPhone's profile link `/connect/apple/…`), [`routes::dev`] (`/dev/reload`,
//!    development only), `/` → `/calendar` ([`routes::pages`]), and the protected part:
//!    [`auth::require_user`] → [`ctx::localize_errors`] → [`error::catch_panic`] → the page and API routers
//!    [`routes::calendar`], [`routes::contacts`], [`routes::tasks`], [`routes::history`],
//!    [`routes::settings`], [`routes::connect`], [`routes::editor`], [`routes::api`] (each module's `router()` is merged here).
//!
//! Inside a handler:
//!
//! - Input comes through the validating extractors [`extract::Path`], [`extract::Query`], [`extract::Json`]
//!   and [`extract::Form`]: a rejection names the field.
//! - CSRF: [`csrf::Token`] or [`csrf::TokenOrBasic`] is extracted after the path and before the body, so a bad
//!   token with a bad body answers 403.
//! - The calendar pages extract [`routes::calendar::PageView`] (= [`ctx::PageCtx`] + [`routes::calendar::ViewCtx`]).
//! - The handler calls into [`domain`] (no HTTP types there), which reads and writes RustiCal's stores
//!   (`rustical_store_sqlite`: `SqliteCalendarStore`, `SqliteAddressbookStore`, `SqlitePrincipalStore`, held in
//!   [`state::AppState`]) and the own `practical_*` tables through sqlx.
//! - The answer is an askama template (`build/templates/`, context [`ctx::PageCtx`]), JSON, or an
//!   [`error::AppError`] (an HTTP status plus a message key, [`error::Msg`], rendered in the page language).
//!
//! UI writes run between [`domain::history::begin`] and [`domain::history::Guard::commit`]. They hold one lock
//! with the change poller, so each UI write becomes exactly one attributed change in the history.
//!
//! Live updates: [`tasks::spawn_change_poller`] → `state.changes` (a broadcast of [`state::ChangeEvent`]) →
//! `/api/changes` (SSE, [`routes::api`]) → app.js refetches the fragment it shows.
//!
//! # Module map
//!
//! | module | owns |
//! |---|---|
//! | [`state`] | [`state::AppState`] (config, pool, stores, i18n, change broadcast, caches, history lock, VAPID keys, guards), [`state::Caches`], [`state::Guards`] |
//! | [`config`] | [`config::Config::from_env`] (`PRACTICAL__*`) |
//! | [`db`] | pool, own migrations, schema check, [`db::Watch`] |
//! | [`auth`], [`sessions`], [`csrf`] | login and logout, the session store and its expiry, CSRF defence |
//! | [`ctx`], [`extract`], [`error`], [`etag`] | page context, input gate, error answers, HTTP validators |
//! | [`routes`] | HTTP handlers, one module per page or API area |
//! | [`render`] | HTML of the calendar views |
//! | [`domain`] | the logic on RustiCal's store and the own tables (no HTTP types) |
//! | [`i18n`] | Fluent translations, language negotiation, plural forms |
//! | [`bundle`], [`css_min`] | asset part order, the CSS minifier of build.rs |
//! | [`clock`] | the server's "now" (fixed in development) |
//! | [`tasks`] | background loops and [`tasks::guarded`] |
//!
//! Test-only modules are not in this doc: `testkit.rs` (a test router on a migrated temporary RustiCal
//! database, golden files, allocation counting) and `it/*.rs` (route-level tests on the synthetic world of
//! `it/seed.rs`: api_events, api_more, assets, auth, caches, history, http, pages, poison = the tier-0 chaos
//! corpus, sessions, settings, units, validators). Pinned behaviour of a module sits in `<module>/pinned.rs`,
//! its unit tests in `<module>/tests.rs`.
//!
//! # The shared database
//!
//! RustiCal and PractiCal run in the same pod, under the same uid, on one SQLite file in WAL mode.
//!
//! - PractiCal waits for RustiCal's `/ping` before it opens the file ([`wait_for_rustical`]): RustiCal runs its
//!   own migrations at its start.
//! - PractiCal never runs RustiCal's migrations ([`db::open`] calls `create_db_pool(.., migrate=false)`).
//! - RustiCal's tables are touched only through its store crates. Exceptions are read-only queries, e.g. the
//!   synctokens the change poller reads.
//! - Own tables are named `practical_*`. Their ledger is `practical_migrations`, never `_sqlx_migrations`:
//!   RustiCal's migrator aborts on a migration it does not know. `MIGRATIONS` in [`db`] is append-only;
//!   [`db::check_schema`] checks the schema at start (refuses a missing or retyped column, logs other drift).
//!
//! # Background loops
//!
//! [`serve`] spawns them; every round runs inside [`tasks::guarded`], so a panic in one round is logged and the
//! loop lives on.
//!
//! | loop | period | work |
//! |---|---|---|
//! | [`tasks::spawn_change_poller`] | 2 s (`PRACTICAL__DEV_POLL_MS` in development) | reads the [`db::Watch`] stamp; only after a commit by another connection: [`domain::history::poll`] (ingest), then the calendar and address-book synctokens → [`state::ChangeEvent`] |
//! | [`domain::push::spawn_reminder_loop`] | 60 s from start, first tick at once; only with a VAPID key | rebuilds its [`domain::push::Plan`] only when the stamp moved, a user's day changed or a retry is due (backoff 1…16 min); fire window [now−2 min, now+1 min] |
//! | [`tasks::spawn_trash_purger`] | 1 h, first round at start | history past `HISTORY_RETENTION_DAYS`, trashed groups past `TRASH_RETENTION_DAYS`, push sent-log older than 30 days, expired sessions, app tokens that never connected within a day ([`domain::tokens::sweep`]) |
//! | [`domain::remote::sched::spawn`] | 60 s; a link runs when its own interval elapsed, and a local change pushes at once | one `sync-collection` per due link (zero responses when nothing changed), then the two directions |
//! | [`tasks::spawn_holiday_coverage`] ([`domain::holidays::maintain`]) | 24 h; the first round only reads saved state | country coverage and CLDR release check, weekly (ETag, usually 304) |
//! | [`tasks::spawn_dev_asset_watcher`] | 300 ms, development only | mtimes of `assets/**` → `dev_reload` → `/dev/reload` ([`routes::dev`]) |
//!
//! Idle cost (bench statement log, 1 SSE client and 1 push subscriber, 60 s): 31 × `PRAGMA data_version` (the
//! poller every 2 s plus the reminder tick's), nothing else. A device in use adds at most one commit per hour and
//! process (RustiCal records its app token's use, `app_tokens.last_used`): one poll round and one plan rebuild.
//!
//! # Caches
//!
//! All caches live in [`state::AppState`] (most in [`state::Caches`]). The rule: a write is never answered
//! from before it. Keys therefore contain the synctoken, plus zone, day or language where the answer depends
//! on them. Test: `it/caches.rs` (mutate, then the cached read must equal the uncached answer).
//!
//! | cache | key | invalidation / bound |
//! |---|---|---|
//! | [`domain::events::InstanceCache`] | (principal, cal, synctoken, tz, from, to), the exact window | LRU above 4 MiB; an entry above ¼ of that is not kept |
//! | [`domain::events::DensityCache`] | (principal, cal) → (synctoken, tz, year) | ≤ 6 per calendar |
//! | [`domain::events::ObjMetaCache`], [`domain::tasks::TaskCache`], [`domain::contacts::ContactCache`] | (principal, collection) | caught up through the store's `sync_changes` when the synctoken moved |
//! | [`routes::api::MentionCache`] | user → (tz, today, [(principal, cal, synctoken, has VEVENT)]) | one per user |
//! | [`routes::more::YearCache`] | (principal, cal, synctoken, tz, year) | LRU above 6 MiB |
//! | [`routes::history::HistoryCache`] | user → key of everything the document is made of | LRU above 6 MiB |
//! | [`domain::search::IndexCache`] | user → fingerprint of the synctokens of the user's collections | rebuilt in part on a change; dropped after 30 min idle |
//! | [`domain::holidays::MemCache`] | (country, subdivision, year, kind-lang) | `cache_put` drops the key; ≤ 512 keys; freshness by `fetched_at` |
//! | [`sessions::DbStore`] (memory part) | session id | write-through; expired records dropped on writes, at most every 10 min |
//! | [`domain::geo::Geo`] | request string → places | 7 days; throttle 1 request/s per provider |
//! | [`domain::history::PlanMemo`] | — | lives for one request |
//!
//! # Assets
//!
//! The shipped bytes are made by `build.rs` into `OUT_DIR`; [`routes::assets`] serves them.
//!
//! - [`bundle::BUNDLES`] is the single source of truth for the part order of app.js and style.css (order = JS
//!   run order and CSS cascade). build.rs and the development mode both read it.
//! - build.rs joins the parts, minifies (oxc for JS, target ES2020; [`css_min`] for CSS), stores gzip-9 and
//!   brotli-11 forms and writes `OUT_DIR/assets.rs`. It also makes one `i18n-<lang>.js` per language from the
//!   `.ftl` files (the `js-*` keys with their plural forms) and two gzip name tables from `regions.json`.
//! - build.rs exports `RUSTICAL_TAG` ([`RUSTICAL_TAG`]): one tag for all `rustical_*` crates, equal to the
//!   Dockerfile's RustiCal image tag (`practical check-rustical-image` compares them).
//! - No inline script anywhere (CSP `script-src 'self'`); `?v=<hash>` URLs are immutable.
//!
//! # Development mode
//!
//! Four switches, all unset in production ([`config::Config`]):
//!
//! - `PRACTICAL__DEV_ASSETS_DIR`: assets from disk, `/dev/reload` ([`routes::assets`], [`routes::dev`]).
//! - `PRACTICAL__DEV_LOGIN`: GET /login signs this user in without a password.
//! - `PRACTICAL__DEV_NOW`: a fixed "now" ([`clock`]).
//! - `PRACTICAL__DEV_POLL_MS`: another change-poller period ([`tasks::change_poll`]).
//!
//! The last three are only honoured together with `DEV_ASSETS_DIR` ([`config::Config::from_env`] refuses
//! them otherwise).
//!
//! `mise run dev` sets the first two; the e2e servers (`e2e/lib/servers.mjs`) set all four.
//!
//! # Commands
//!
//! `practical serve` (default), `check-rustical-image <ref>` ([`check_rustical_image`]), `gen-vapid`,
//! `--version`. The global allocator is mimalloc (tests: a counting wrapper, `testkit::Counting`).
#![warn(clippy::all)]
// the API doc's logo + favicon: the Pages site's copy of build/assets/icon.svg (the doc is published under /api/)
#![doc(html_logo_url = "https://xarif.gitlab.io/docker/practical/icon.svg", html_favicon_url = "https://xarif.gitlab.io/docker/practical/icon.svg")]
mod auth;
mod bundle;
mod clock;
mod config;
mod csrf;
// in the API doc too (`doc`): build.rs, which uses it, is outside the crate docs
#[cfg(any(test, doc))]
mod css_min;
mod ctx;
mod db;
mod domain;
mod error;
mod etag;
mod extract;
mod i18n;
mod render;
mod routes;
mod sessions;
mod state;
mod tasks;
#[cfg(test)]
mod testkit;
#[cfg(test)]
mod it;

use axum::Router;
use axum::http::header;
use axum::routing::get;
use config::Config;
use rustical_store::auth::middleware::AuthenticationLayer;
use rustical_store_sqlite::{SqliteAddressbookStore, SqliteCalendarStore, SqlitePrincipalStore};
use state::{AppState, SharedState};
use std::sync::Arc;
use std::time::Duration;
use tower_http::compression::CompressionLayer;
use tower_http::trace::TraceLayer;
use tower_sessions::cookie::SameSite;
use tower_sessions::{Expiry, SessionManagerLayer};
use tracing::{info, warn};

// see Cargo.toml (measurement) and the Dockerfile (MIMALLOC_* options that keep RSS at musl level)
#[cfg(not(test))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
// tests: the same allocator, counting each thread's bytes (`testkit::peak_of`)
#[cfg(test)]
#[global_allocator]
static GLOBAL: testkit::Counting = testkit::Counting;

pub const RUSTICAL_TAG: &str = env!("RUSTICAL_TAG");

fn print_version() {
    println!("practical {} (rustical crates v{RUSTICAL_TAG})", env!("CARGO_PKG_VERSION"));
}

/// `check-rustical-image registry.gitlab.com/xarif/docker/rustical:v0.16.6-xarif.2@sha256:…` → exit 0 iff
/// the image tag equals the crate tag this binary was built against.
fn check_rustical_image(reference: &str) -> i32 {
    let no_digest = reference.split('@').next().unwrap_or(reference);
    let last = no_digest.rsplit('/').next().unwrap_or(no_digest);
    let tag = last.split_once(':').map(|(_, t)| t).unwrap_or("").trim_start_matches('v');
    if tag.is_empty() {
        eprintln!("no tag in image reference {reference:?}");
        return 2;
    }
    if tag == RUSTICAL_TAG {
        println!("OK: rustical image tag {tag} == practical crate tag {RUSTICAL_TAG}");
        0
    } else {
        eprintln!("MISMATCH: rustical image tag {tag} != practical crate tag {RUSTICAL_TAG} — bump docker/practical to the same tag and publish before rolling this image");
        1
    }
}

/// Waits until RustiCal answers `/ping`: it runs its schema migrations at start, and PractiCal must never
/// open the file before.
async fn wait_for_rustical(url: &str) {
    // RustiCal runs its schema migrations at start; we must never open the file before.
    let client = reqwest::Client::builder().timeout(Duration::from_secs(3)).build().unwrap();
    let ping = format!("{url}/ping");
    let mut n = 0u32;
    loop {
        match client.get(&ping).send().await {
            Ok(r) if r.status().is_success() => {
                info!("rustical answers on {ping}");
                return;
            }
            Ok(r) => warn!("waiting for rustical: {ping} → {}", r.status()),
            Err(e) => {
                if n % 10 == 0 {
                    warn!("waiting for rustical: {ping}: {e}");
                }
            }
        }
        n += 1;
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn healthz(axum::extract::State(state): axum::extract::State<SharedState>) -> (axum::http::StatusCode, String) {
    match sqlx::query("SELECT 1").execute(&state.db).await {
        Ok(_) => (axum::http::StatusCode::OK, "ok".into()),
        // public (the kubelet's probe needs no login): the reason only in the log
        Err(e) => {
            warn!("healthz: {e}");
            (axum::http::StatusCode::SERVICE_UNAVAILABLE, "db unavailable".into())
        }
    }
}

/// The Content-Security-Policy of every HTML response. No inline script at all (layout.html loads
/// theme.js and the i18n script; app.js submits the auto-login form); style attributes stay
/// (`'unsafe-inline'` for style-src only); images: the map dialog's OpenStreetMap tiles (app.js
/// `L.tileLayer`) and `data:` (Leaflet sets its 1×1 GIF `emptyImageUrl` on every tile it unloads); forms post here, and the two that are answered with a redirect to the identity
/// provider (POST /login/oidc → its authorization endpoint, on the issuer's host; POST /logout → the
/// portal logout URL) need that origin too — browsers apply form-action to the redirect.
pub fn content_security_policy(cfg: &config::Config) -> String {
    let mut form = vec!["'self'".to_string()];
    for u in [cfg.oidc.as_ref().map(|o| o.issuer.as_str()), Some(cfg.logout_url.as_str())].into_iter().flatten() {
        if let Ok(u) = url::Url::parse(u)
            && let o = u.origin().ascii_serialization()
            && o != "null"
            && !form.contains(&o)
        {
            form.push(o);
        }
    }
    format!(
        "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: https://tile.openstreetmap.org; connect-src 'self'; worker-src 'self'; object-src 'none'; base-uri 'self'; form-action {}; frame-ancestors 'none'",
        form.join(" ")
    )
}

/// The whole application router with the production session store (layer order: crate docs, "How a
/// request flows").
fn build_router(state: &SharedState) -> Result<Router, String> {
    build_router_with(state, sessions::DbStore::new(state.db.clone()))
}

/// [`build_router`] with a given session store. `session_store`: a test keeps a handle on it (testkit).
fn build_router_with(state: &SharedState, session_store: sessions::DbStore) -> Result<Router, String> {
    let lifetime = tower_sessions::cookie::time::Duration::hours(state.config.session_hours);
    let activity = sessions::Activity::new(session_store.clone(), lifetime);
    let session_layer = SessionManagerLayer::new(session_store)
        .with_name("practical_session")
        .with_secure(state.config.cookie_secure())
        .with_same_site(SameSite::Lax)
        .with_http_only(true)
        .with_expiry(Expiry::OnInactivity(lifetime));

    let csp = axum::http::HeaderValue::from_str(&content_security_policy(&state.config)).map_err(|e| format!("Content-Security-Policy: {e}"))?;
    let protected = Router::new()
        .merge(routes::calendar::router())
        .merge(routes::contacts::router())
        .merge(routes::tasks::router())
        .merge(routes::editor::router())
        .merge(routes::history::router())
        .merge(routes::settings::router())
        .merge(routes::connect::router())
        .merge(routes::api::router())
        .route_layer(axum::middleware::from_fn(error::catch_panic))
        .route_layer(axum::middleware::from_fn_with_state(state.clone(), ctx::localize_errors))
        .route_layer(axum::middleware::from_fn(auth::require_user));

    Ok(Router::new()
        .route("/", get(routes::pages::root))
        .merge(auth::router(state)?)
        .merge(routes::connect::public_router())
        .merge(routes::dev::router(state.config.dev_assets_dir.is_some()))
        .merge(protected)
        .layer(axum::middleware::from_fn_with_state(state.clone(), csrf::origin_guard))
        .layer(AuthenticationLayer::new(state.principals.clone()))
        .layer(axum::middleware::from_fn_with_state(activity, sessions::activity))
        .layer(session_layer)
        // inside the compression: the tag is taken of the identity body (and br made of it), the 304 has none
        .layer(axum::middleware::from_fn(etag::revalidate))
        // gzip for what etag::revalidate did not already encode as brotli (a client without br, other types)
        .layer(CompressionLayer::new())
        // Merged AFTER the layers above, so none of them runs for these two: static files and the
        // kubelet probe read no session, look up no principal (a DB query per request with a session
        // cookie or Basic auth), need no CSRF check (GET only) and are never compressed per request
        // (assets come precompressed from build.rs).
        .merge(routes::assets::router())
        .route("/healthz", get(healthz))
        .layer(axum::middleware::map_response(move |mut r: axum::response::Response| {
            let csp = csp.clone();
            async move {
                let ct = r.headers().get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("");
                let (html, json) = (ct.starts_with("text/html"), ct.starts_with("application/json"));
                if html {
                    r.headers_mut().entry(header::CONTENT_SECURITY_POLICY).or_insert(csp);
                }
                // pages and JSON carry personal data: never kept by a shared cache, revalidated before a browser
                // reuses them (`no-cache`, not `no-store`: the back/forward cache stays; assets set their own)
                if html || json {
                    r.headers_mut().entry(header::CACHE_CONTROL).or_insert(axum::http::HeaderValue::from_static("private, no-cache"));
                }
                r
            }
        }))
        // the pages outside `protected` (login, OIDC, assets) and the middleware above: unlocalized
        .layer(axum::middleware::from_fn(error::catch_panic))
        .layer(TraceLayer::new_for_http())
        .with_state(state.clone()))
}

/// `practical serve`: config → wait for RustiCal → open, check and migrate the database → [`AppState`] →
/// router → background loops → listen until SIGTERM/Ctrl-C, then a graceful shutdown (at most
/// [`SHUTDOWN_GRACE`]) and the database close.
async fn serve() -> Result<(), String> {
    let config = Config::from_env()?;
    if let Some(at) = config.dev_now {
        warn!("development clock: every server-side \"now\" is fixed at {at}");
        clock::fix(at);
    }
    if config.rustical_url.is_empty() {
        // development only: no rustical process next to us (a copy of the DB)
        warn!("PRACTICAL__RUSTICAL_URL is empty — not waiting for rustical (development mode)");
    } else {
        wait_for_rustical(&config.rustical_url).await;
    }
    let db = db::open(&config.db_path).await.map_err(|e| format!("open {}: {e}", config.db_path))?;
    if !db::rustical_schema_present(&db).await.map_err(|e| e.to_string())? {
        return Err(format!("{} is not a migrated RustiCal database", config.db_path));
    }
    db::migrate(&db).await.map_err(|e| format!("migrations: {e}"))?;
    for d in db::check_schema(&db).await? {
        tracing::error!("schema drift (the service runs; a repair is a migration): {d}");
    }

    let (cal_tx, _cal_rx) = tokio::sync::mpsc::channel(64);
    let (ab_tx, _ab_rx) = tokio::sync::mpsc::channel(64);
    let vapid = match &config.vapid_private_key_pem {
        Some(pem) => Some(rustical_dav_push::VapidKeypair::from_pem(pem).map_err(|e| format!("VAPID key: {e}"))?),
        None => None,
    };
    let (changes, _) = tokio::sync::broadcast::channel(256);
    let (dev_reload, _) = tokio::sync::broadcast::channel(16);
    let state: SharedState = Arc::new(AppState {
        principals: Arc::new(SqlitePrincipalStore::new(db.clone())),
        calendars: Arc::new(SqliteCalendarStore::new(db.clone(), cal_tx, true)),
        addressbooks: Arc::new(SqliteAddressbookStore::new(db.clone(), ab_tx, true)),
        i18n: i18n::I18n::load(),
        changes,
        dev_reload,
        http: reqwest::Client::builder().timeout(Duration::from_secs(20)).build().unwrap(),
        remote_http: crate::domain::remote::client(&config.remote_private_origins),
        remote_logins: Arc::new(crate::domain::remote::login::Logins::default()),
        connect: Default::default(),
        remote_runner: Arc::new(crate::domain::remote::sched::Runner::default()),
        geo: Default::default(),
        search_index: Default::default(),
        task_cache: Default::default(),
        caches: Default::default(),
        history: Default::default(),
        vapid,
        started_at: chrono::Utc::now(),
        guards: Default::default(),
        db,
        config,
    });
    let app = build_router(&state)?;
    tasks::spawn_change_poller(state.clone());
    tasks::spawn_trash_purger(state.clone());
    crate::domain::remote::sched::spawn(state.clone());
    crate::tasks::spawn_remote_login_reaper(state.clone());
    tasks::spawn_holiday_coverage(state.clone());
    tasks::spawn_dev_asset_watcher(state.clone());
    domain::push::spawn_reminder_loop(state.clone());

    let listener = tokio::net::TcpListener::bind(&state.config.bind).await.map_err(|e| format!("bind {}: {e}", state.config.bind))?;
    info!("practical {} serving on http://{} (public {})", env!("CARGO_PKG_VERSION"), state.config.bind, state.config.public_url);
    let stop = state.clone();
    let server = axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).with_graceful_shutdown(async move {
        stop_signal().await;
        info!("stopping: no new connections, open requests finish (at most {} s)", SHUTDOWN_GRACE.as_secs());
        stop.guards.shutdown.send_replace(true);
    });
    let stopping = state.guards.stopping();
    let r = tokio::select! {
        r = server => r.map_err(|e| e.to_string()),
        () = async { stopping.await; tokio::time::sleep(SHUTDOWN_GRACE).await } => {
            warn!("requests still open after {} s: stopping anyway", SHUTDOWN_GRACE.as_secs());
            Ok(())
        }
    };
    // the last WAL frames into the database file; every write was committed before (sessions too)
    state.db.close().await;
    info!("stopped");
    r
}

/// Inside the pod's terminationGracePeriodSeconds (30, kubernetes/rustical), with room for the DB close.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(20);

/// SIGTERM (Kubernetes stopping the pod; PID 1 in the image, where the kernel drops a signal without a
/// handler, so the pod used to wait the full grace period for SIGKILL) or Ctrl-C.
async fn stop_signal() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("SIGTERM handler");
        tokio::select! {
            _ = term.recv() => {}
            _ = tokio::signal::ctrl_c() => {}
        }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("--version" | "version") => print_version(),
        Some("check-rustical-image") => {
            let Some(r) = args.get(2) else {
                eprintln!("usage: practical check-rustical-image <image-ref>");
                std::process::exit(2);
            };
            std::process::exit(check_rustical_image(r));
        }
        Some("gen-vapid") => {
            let kp = rustical_dav_push::VapidKeypair::generate_p256();
            println!("{}", kp.to_pem().unwrap());
            println!("# public key (b64url): {}", kp.public().encode_b64().0);
        }
        Some("serve") | None => {
            tracing_subscriber::fmt()
                .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,tower_http=warn".into()))
                .init();
            if let Err(e) = serve().await {
                tracing::error!("{e}");
                std::process::exit(1);
            }
        }
        Some(other) => {
            eprintln!("unknown command {other:?}; commands: serve | check-rustical-image <ref> | gen-vapid | --version");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::check_rustical_image;

    /// form-action names the identity provider's origin (login and logout redirect there), once
    #[test]
    fn content_security_policy_allows_the_identity_provider() {
        let mut c = crate::testkit::config("/nonexistent.sqlite3");
        assert!(super::content_security_policy(&c).contains("; form-action 'self'; frame-ancestors 'none'"));
        c.oidc = Some(crate::config::OidcSettings { name: "A".into(), issuer: "https://auth.example.org/realm/x".into(), client_id: "c".into(), scopes: vec![], ..crate::testkit::oidc_settings() });
        c.logout_url = "https://auth.example.org/logout?rd={url}".into();
        assert!(super::content_security_policy(&c).contains("; form-action 'self' https://auth.example.org; "));
        c.logout_url = "https://sso.example.net:8443/bye".into();
        let p = super::content_security_policy(&c);
        assert!(p.contains("; form-action 'self' https://auth.example.org https://sso.example.net:8443; "), "{p}");
        for d in ["default-src 'self'", "script-src 'self';", "style-src 'self' 'unsafe-inline'", "img-src 'self' data: https://tile.openstreetmap.org;", "connect-src 'self'", "worker-src 'self'", "object-src 'none'", "base-uri 'self'", "frame-ancestors 'none'"] {
            assert!(p.contains(d), "{d}: {p}");
        }
    }

    #[test]
    fn image_tag_check() {
        assert_eq!(check_rustical_image(&format!("registry.gitlab.com/xarif/docker/rustical:v{}@sha256:abc", super::RUSTICAL_TAG)), 0);
        assert_eq!(check_rustical_image("registry.gitlab.com/xarif/docker/rustical:v0.0.1-xarif.1@sha256:abc"), 1);
        assert_eq!(check_rustical_image("registry.gitlab.com/xarif/docker/rustical@sha256:abc"), 2);
    }

    /// The image carries build/LICENSE (the Docker context is build/); the repository's own is
    /// ../LICENSE. One AGPL-3.0 text, two places: they must not drift apart.
    #[test]
    fn the_image_license_is_the_repository_license() {
        assert_eq!(include_str!("../LICENSE"), include_str!("../../LICENSE"));
        assert!(include_str!("../LICENSE").contains("GNU AFFERO GENERAL PUBLIC LICENSE"));
    }
}
