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

fn build_router(state: &SharedState) -> Result<Router, String> {
    build_router_with(state, sessions::DbStore::new(state.db.clone()))
}

/// `session_store`: a test keeps a handle on it (testkit).
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
        .merge(routes::history::router())
        .merge(routes::settings::router())
        .merge(routes::api::router())
        .route_layer(axum::middleware::from_fn(error::catch_panic))
        .route_layer(axum::middleware::from_fn_with_state(state.clone(), ctx::localize_errors))
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
    db::check_schema(&db).await?;

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
        guards: Default::default(),
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
        c.oidc = Some(crate::config::OidcSettings { name: "A".into(), issuer: "https://auth.example.org/realm/x".into(), client_id: "c".into(), client_secret: None, scopes: vec![] });
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
