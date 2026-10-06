#![warn(clippy::all)]
mod auth;
mod clock;
mod config;
mod csrf;
#[cfg(test)]
mod css_min;
mod ctx;
mod db;
mod domain;
mod error;
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
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

pub const RUSTICAL_TAG: &str = env!("RUSTICAL_TAG");

fn print_version() {
    println!("practical {} (rustical crates v{RUSTICAL_TAG})", env!("CARGO_PKG_VERSION"));
}

/// `check-rustical-image ghcr.io/lennart-k/rustical:0.16.2@sha256:…` → exit 0 iff
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
        Err(e) => (axum::http::StatusCode::SERVICE_UNAVAILABLE, format!("db: {e}")),
    }
}

fn build_router(state: &SharedState) -> Result<Router, String> {
    build_router_with(state, sessions::MemoryStore::default())
}

/// `session_store`: a test keeps a handle on it (testkit).
fn build_router_with(state: &SharedState, session_store: sessions::MemoryStore) -> Result<Router, String> {
    let lifetime = tower_sessions::cookie::time::Duration::hours(state.config.session_hours);
    let activity = sessions::Activity::new(session_store.clone(), lifetime);
    let session_layer = SessionManagerLayer::new(session_store)
        .with_name("practical_session")
        .with_secure(state.config.cookie_secure())
        .with_same_site(SameSite::Lax)
        .with_http_only(true)
        .with_expiry(Expiry::OnInactivity(lifetime));

    let protected = Router::new()
        .merge(routes::calendar::router())
        .merge(routes::contacts::router())
        .merge(routes::tasks::router())
        .merge(routes::history::router())
        .merge(routes::settings::router())
        .merge(routes::api::router())
        .route_layer(axum::middleware::from_fn(auth::require_user));

    Ok(Router::new()
        .route("/", get(routes::pages::root))
        .merge(auth::router(state)?)
        .merge(routes::dev::router(state.config.dev_assets_dir.is_some()))
        .merge(protected)
        .layer(axum::middleware::from_fn_with_state(state.clone(), csrf::origin_guard))
        .layer(AuthenticationLayer::new(state.principals.clone()))
        .layer(axum::middleware::from_fn_with_state(activity, sessions::activity))
        .layer(session_layer)
        .layer(CompressionLayer::new())
        // Merged AFTER the layers above, so none of them runs for these two: static files and the
        // kubelet probe read no session, look up no principal (a DB query per request with a session
        // cookie or Basic auth), need no CSRF check (GET only) and are never compressed per request
        // (assets come precompressed from build.rs).
        .merge(routes::assets::router())
        .route("/healthz", get(healthz))
        .layer(TraceLayer::new_for_http())
        .with_state(state.clone()))
}

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
        geo: Default::default(),
        search_index: Default::default(),
        task_cache: Default::default(),
        caches: Default::default(),
        history: Default::default(),
        vapid,
        started_at: chrono::Utc::now(),
        db,
        config,
    });
    let app = build_router(&state)?;
    tasks::spawn_change_poller(state.clone());
    tasks::spawn_trash_purger(state.clone());
    tasks::spawn_holiday_coverage(state.clone());
    tasks::spawn_dev_asset_watcher(state.clone());
    domain::push::spawn_reminder_loop(state.clone());

    let listener = tokio::net::TcpListener::bind(&state.config.bind).await.map_err(|e| format!("bind {}: {e}", state.config.bind))?;
    info!("practical {} serving on http://{} (public {})", env!("CARGO_PKG_VERSION"), state.config.bind, state.config.public_url);
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .map_err(|e| e.to_string())
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

    #[test]
    fn image_tag_check() {
        assert_eq!(check_rustical_image(&format!("ghcr.io/lennart-k/rustical:{}@sha256:abc", super::RUSTICAL_TAG)), 0);
        assert_eq!(check_rustical_image("ghcr.io/lennart-k/rustical:0.0.1@sha256:abc"), 1);
        assert_eq!(check_rustical_image("ghcr.io/lennart-k/rustical@sha256:abc"), 2);
    }

    /// The image carries build/LICENSE (the Docker context is build/); the repository's own is
    /// ../LICENSE. One AGPL-3.0 text, two places: they must not drift apart.
    #[test]
    fn the_image_license_is_the_repository_license() {
        assert_eq!(include_str!("../LICENSE"), include_str!("../../LICENSE"));
        assert!(include_str!("../LICENSE").contains("GNU AFFERO GENERAL PUBLIC LICENSE"));
    }
}
