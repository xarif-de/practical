//! Login (OIDC via rustical_oidc, optional password), logout (local + IdP portal),
//! and the guard that sends anonymous browsers to /login.

use crate::csrf;
use crate::ctx::{PageCtx, SESSION_USER};
use crate::error::AppError;
use crate::state::SharedState;
use askama::Template;
use askama_web::WebTemplate;
use async_trait::async_trait;
use axum::extract::{ConnectInfo, Request, State};
use crate::extract::{Form, Query};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};
use axum::{Router, routing::get};
use openidconnect::{ClientId, ClientSecret, IssuerUrl, Scope};
use rustical_oidc::{OidcConfig, OidcServiceConfig, UserStore, oidc_router};
use rustical_store::auth::AuthenticationProvider;
use rustical_store_sqlite::SqlitePrincipalStore;
use serde::Deserialize;
use std::sync::Arc;
use tower_sessions::Session;
use url::Url;

pub const OIDC_CALLBACK_PATH: &str = "/login/oidc/callback";
/// The login form's CSRF token as a cookie of its own (double submit): an anonymous visit stores nothing
/// on the server (sessions.rs). Host-only, /login only, Strict: no other site's form brings it along.
pub const LOGIN_CSRF: &str = "practical_login_csrf";

/// The OIDC callback only ever LOOKS UP principals: a user who has never logged
/// in at RustiCal has no principal, and creating one here would make this UI a
/// second identity source (plan, "Authentifizierung" §1).
#[derive(Clone)]
pub struct LookupOnlyUserStore(pub Arc<SqlitePrincipalStore>);

#[async_trait]
impl UserStore for LookupOnlyUserStore {
    type Error = AppError;

    async fn user_exists(&self, id: &str) -> Result<bool, AppError> {
        Ok(self.0.get_principal(id).await?.is_some())
    }

    async fn ensure_user(&self, id: &str, _memberships: &[&str]) -> Result<(), AppError> {
        match self.0.get_principal(id).await? {
            Some(_) => Ok(()),
            None => Err(AppError::Unauthorized),
        }
    }
}

pub fn oidc_config(state: &SharedState) -> Result<Option<OidcConfig>, String> {
    let Some(o) = &state.config.oidc else { return Ok(None) };
    Ok(Some(OidcConfig {
        name: o.name.clone(),
        issuer: IssuerUrl::new(o.issuer.clone()).map_err(|e| format!("OIDC issuer: {e}"))?,
        client_id: ClientId::new(o.client_id.clone()),
        client_secret: o.client_secret.clone().map(ClientSecret::new),
        scopes: o.scopes.iter().cloned().map(Scope::new).collect(),
        // sign-up happens at RustiCal (dav host), never here
        allow_sign_up: false,
        require_group: None,
        claim_userid: Default::default(),
        additional_audiences: vec![],
        assign_memberships: Default::default(),
    }))
}

pub fn router(state: &SharedState) -> Result<Router<SharedState>, String> {
    let mut r = Router::new()
        .route("/login", get(get_login).post(post_login))
        .route("/logout", axum::routing::post(post_logout));
    if let Some(cfg) = oidc_config(state)? {
        r = r.nest_service(
            "/login/oidc",
            oidc_router(
                cfg,
                OidcServiceConfig {
                    default_redirect_path: "/calendar",
                    session_key_user_id: SESSION_USER,
                    callback_path: OIDC_CALLBACK_PATH,
                },
                LookupOnlyUserStore(state.principals.clone()),
            )
            .layer(axum::middleware::from_fn(cycle_on_login)),
        );
    }
    Ok(r)
}

/// Session fixation: a login gets a new session id, as the password login does (`post_login`). rustical_oidc's
/// callback only inserts the user into the session the browser brought — whose id a sibling site of the same
/// parent domain could have set before. The id changes once the callback has put a (new) user in.
pub(crate) async fn cycle_on_login(request: Request, next: Next) -> Response {
    let Some(session) = request.extensions().get::<Session>().cloned() else { return next.run(request).await };
    let before = session.get::<String>(SESSION_USER).await.ok().flatten();
    let res = next.run(request).await;
    let after = session.get::<String>(SESSION_USER).await.ok().flatten();
    if after.is_some() && after != before && let Err(e) = session.cycle_id().await {
        tracing::error!("OIDC login: new session id: {e}");
        return AppError::Session(e).into_response();
    }
    res
}

#[derive(Template, WebTemplate)]
#[template(path = "login.html")]
pub struct LoginPage {
    pub ctx: PageCtx,
    pub redirect_uri: Option<String>,
    pub oidc_name: Option<String>,
    pub allow_password_login: bool,
    pub auto_oidc: bool,
    /// the message above the form (`login_error`)
    pub error: Option<&'static str>,
}

/// `/login?error=…`: 2 = too many failed attempts (`LoginThrottle`), anything else = the login failed.
fn login_error(e: Option<&str>) -> Option<&'static str> {
    e.map(|e| if e == "2" { "login-too-many" } else { "login-failed" })
}

#[derive(Deserialize)]
pub struct LoginQuery {
    redirect_uri: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    manual: Option<String>,
}

pub async fn get_login(State(state): State<SharedState>, session: Session, ctx: PageCtx, Query(q): Query<LoginQuery>) -> Response {
    if ctx.user.is_some() {
        return Redirect::to("/calendar").into_response();
    }
    if let (Some(id), Some(_)) = (&state.config.dev_login, &state.config.dev_assets_dir)
        && let Ok(Some(user)) = state.principals.get_principal(id).await
    {
        tracing::warn!("development auto-login as {}", user.id);
        let _ = session.insert(SESSION_USER, user.id).await;
        return Redirect::to(&safe_redirect(q.redirect_uri.as_deref())).into_response();
    }
    let oidc_name = state.config.oidc.as_ref().map(|o| o.name.clone());
    let allow_password_login = state.config.allow_password_login;
    let mut ctx = ctx;
    let mut cookie = None;
    if allow_password_login {
        let t = crate::ctx::random_token();
        let secure = if state.config.cookie_secure() { "; Secure" } else { "" };
        cookie = Some(format!("{LOGIN_CSRF}={t}; Path=/login; Max-Age=3600; HttpOnly; SameSite=Strict{secure}"));
        ctx.csrf = t;
    }
    let page = LoginPage {
        auto_oidc: oidc_name.is_some() && !allow_password_login && q.error.is_none() && q.manual.is_none(),
        ctx,
        redirect_uri: q.redirect_uri.filter(|r| r.starts_with('/') && !r.starts_with("//")),
        oidc_name,
        allow_password_login,
        error: login_error(q.error.as_deref()),
    };
    match cookie {
        Some(c) => ([(header::SET_COOKIE, c)], page).into_response(),
        None => page.into_response(),
    }
}

/// The value of cookie `name` in the request.
fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get_all(header::COOKIE).iter().filter_map(|v| v.to_str().ok()).flat_map(|v| v.split(';')).find_map(|c| c.trim().strip_prefix(name)?.strip_prefix('='))
}

#[derive(Deserialize)]
pub struct LoginForm {
    username: String,
    password: String,
    redirect_uri: Option<String>,
    csrf: String,
}

pub async fn post_login(State(state): State<SharedState>, session: Session, headers: HeaderMap, peer: Option<axum::Extension<ConnectInfo<std::net::SocketAddr>>>, Form(f): Form<LoginForm>) -> Result<Response, AppError> {
    if !state.config.allow_password_login {
        return Err(AppError::Forbidden);
    }
    // the login page's own token (cookie = field), else that of a session already logged in (switching users)
    if !cookie(&headers, LOGIN_CSRF).is_some_and(|c| !c.is_empty() && csrf::same(c.as_bytes(), f.csrf.as_bytes())) {
        csrf::check(&session, &f.csrf).await?;
    }
    let target = safe_redirect(f.redirect_uri.as_deref());
    let keys = LoginThrottle::keys(&headers, peer.map(|p| p.0.0), &f.username);
    let failed = || Redirect::to(&format!("/login?error=1&redirect_uri={}", urlenc(&target))).into_response();
    if !state.guards.login.allows(&keys) {
        // the same answer for every user name, existing or not, right password or not
        tracing::warn!("password login as {:?}: too many failed attempts", f.username);
        return Ok(Redirect::to(&format!("/login?error=2&redirect_uri={}", urlenc(&target))).into_response());
    }
    match state.principals.validate_password(&f.username, &f.password).await? {
        Some(user) => {
            session.cycle_id().await?;
            session.insert(SESSION_USER, user.id).await?;
            let secure = if state.config.cookie_secure() { "; Secure" } else { "" };
            Ok(([(header::SET_COOKIE, format!("{LOGIN_CSRF}=; Path=/login; Max-Age=0; HttpOnly; SameSite=Strict{secure}"))], Redirect::to(&target)).into_response())
        }
        None => {
            // an unknown user (or one without a password) costs one hash check too: the answer time tells
            // nothing about which names exist (RustiCal returns at once for them)
            if state.principals.get_principal(&f.username).await?.is_none_or(|p| p.password.is_none()) {
                let pw = f.password.clone();
                let _ = tokio::task::spawn_blocking(move || password_auth::verify_password(pw, dummy_hash())).await;
            }
            state.guards.login.failed(&keys);
            tracing::warn!("failed password login as {:?}", f.username); // Debug: a typed CR/LF cannot forge log lines
            Ok(failed())
        }
    }
}

/// An argon2 hash of nothing in particular (RustiCal's CLI hashes passwords with argon2's defaults, which
/// `password_auth::generate_hash` uses), made on the first failed login.
fn dummy_hash() -> &'static str {
    static H: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    H.get_or_init(|| password_auth::generate_hash(crate::ctx::random_token()))
}

/// Failed password logins per (client, user name) and per client: a token bucket each, refilled over time;
/// a login is refused while either is empty. Only failures take a token, so a correct login never waits
/// behind others' mistakes unless they came from the same client. (Production logs in with OIDC only.)
#[derive(Default)]
pub struct LoginThrottle(std::sync::Mutex<std::collections::HashMap<String, (f64, std::time::Instant)>>);

/// (bucket size, seconds per token): 5 tries per name then one a minute; 30 per client then one per 10 s
const PER_NAME: (f64, f64) = (5.0, 60.0);
const PER_CLIENT: (f64, f64) = (30.0, 10.0);

impl LoginThrottle {
    /// The client: the address Traefik appended last to X-Forwarded-For (a client can only prepend to it),
    /// else the peer.
    pub fn keys(headers: &HeaderMap, peer: Option<std::net::SocketAddr>, user: &str) -> [String; 2] {
        let fwd = headers.get_all("x-forwarded-for").iter().filter_map(|v| v.to_str().ok()).flat_map(|v| v.split(',')).map(str::trim).filter(|v| !v.is_empty()).last().map(str::to_string);
        let client = fwd.or_else(|| peer.map(|p| p.ip().to_string())).unwrap_or_default();
        [format!("n {client} {}", user.trim().to_lowercase()), format!("c {client}")]
    }

    fn level(entry: Option<&(f64, std::time::Instant)>, (size, per): (f64, f64), now: std::time::Instant) -> f64 {
        entry.map_or(size, |(t, at)| (t + now.duration_since(*at).as_secs_f64() / per).min(size))
    }

    pub fn allows(&self, keys: &[String; 2]) -> bool {
        let g = self.0.lock().unwrap();
        let now = std::time::Instant::now();
        Self::level(g.get(&keys[0]), PER_NAME, now) >= 1.0 && Self::level(g.get(&keys[1]), PER_CLIENT, now) >= 1.0
    }

    pub fn failed(&self, keys: &[String; 2]) {
        let mut g = self.0.lock().unwrap();
        let now = std::time::Instant::now();
        if g.len() > 10_000 {
            // full buckets carry no state
            g.retain(|k, v| Self::level(Some(v), if k.starts_with('n') { PER_NAME } else { PER_CLIENT }, now) < if k.starts_with('n') { PER_NAME.0 } else { PER_CLIENT.0 });
        }
        for (k, rule) in keys.iter().zip([PER_NAME, PER_CLIENT]) {
            let level = Self::level(g.get(k), rule, now);
            g.insert(k.clone(), ((level - 1.0).max(0.0), now));
        }
    }

    /// Test clock: every bucket is `d` older.
    #[cfg(test)]
    pub fn pass(&self, d: std::time::Duration) {
        self.0.lock().unwrap().values_mut().for_each(|(_, at)| *at -= d);
    }
}

#[derive(Deserialize)]
pub struct LogoutForm {
    csrf: String,
}

/// Local logout, then hand the browser to the IdP so its session ends as well
/// (plan D7; Authelia has no end_session_endpoint, so this is its portal logout URL).
pub async fn post_logout(State(state): State<SharedState>, session: Session, Form(f): Form<LogoutForm>) -> Result<Response, AppError> {
    csrf::check(&session, &f.csrf).await?;
    // the push subscriptions this session made end with it (shared devices): w3/push's
    // `domain::push::forget_on_logout(&state, &session)` is called here, before the flush, at its merge
    session.flush().await?;
    let cfg = &state.config;
    if cfg.oidc.is_some() && !cfg.logout_url.is_empty() {
        let target = cfg.logout_url.replace("{url}", &urlenc(&cfg.public_url));
        return Ok(Redirect::to(&target).into_response());
    }
    Ok(Redirect::to("/login?manual=1").into_response())
}

/// A local path to return to after login, else /calendar. Browsers read `\` as `/` and drop
/// tab/CR/LF inside a URL, so `/\evil.example` or `/<TAB>/evil.example` would leave the site.
pub fn safe_redirect(r: Option<&str>) -> String {
    match r {
        Some(p) if p.starts_with('/') && !p.starts_with("//") && !p.starts_with("/login") && !p.contains(['\\', '\t', '\n', '\r']) => p.to_string(),
        _ => "/calendar".to_string(),
    }
}

pub fn urlenc(s: &str) -> String {
    let mut u = Url::parse("http://x/").unwrap();
    u.query_pairs_mut().append_pair("r", s);
    u.query().unwrap_or("").trim_start_matches("r=").to_string()
}

/// Everything below /calendar, /contacts, /tasks, /settings, /api needs a user.
pub async fn require_user(request: Request, next: Next) -> Response {
    if request.extensions().get::<rustical_store::auth::Principal>().is_some() {
        return next.run(request).await;
    }
    let wants_html = request
        .headers()
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|a| a.contains("text/html"));
    if request.method() == axum::http::Method::GET && wants_html {
        let path = request.uri().path_and_query().map(|p| p.as_str().to_string()).unwrap_or_default();
        return Redirect::to(&format!("/login?redirect_uri={}", urlenc(&path))).into_response();
    }
    (StatusCode::UNAUTHORIZED, "login required").into_response()
}

#[cfg(test)]
mod tests {
    use super::safe_redirect;

    #[test]
    fn redirect_stays_on_this_site() {
        assert_eq!(safe_redirect(Some("/tasks?view=board#x")), "/tasks?view=board#x");
        for bad in ["//evil.example", "/\\evil.example", "/\t/evil.example", "/\n/evil.example", "https://evil.example", "/login?r=/x", "evil"] {
            assert_eq!(safe_redirect(Some(bad)), "/calendar", "{bad:?}");
        }
        assert_eq!(safe_redirect(None), "/calendar");
    }
}
