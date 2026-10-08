//! Login (OIDC via rustical_oidc, optional password), logout (local + IdP portal),
//! and the guard that sends anonymous browsers to /login.
//!
//! The accounts are RustiCal's principals; PractiCal has no user table of its own.
//!
//! - OIDC: PractiCal is its own client at RustiCal's identity provider. The callback looks the principal up
//!   and, with `OIDC__ALLOW_SIGN_UP`, creates it at a first sign-in ([`SignInStore`]).
//! - Password login is optional (`PRACTICAL__ALLOW_PASSWORD_LOGIN`). An unknown name is checked against a
//!   dummy argon2 hash ([`dummy_hash`]), so it takes as long as a known one; [`LoginThrottle`] slows down
//!   repeated failures.
//! - [`require_user`] guards every page and API; [`safe_redirect`] keeps the post-login target on this site.
//! - The login form's CSRF token is a cookie of its own ([`LOGIN_CSRF`]), so an anonymous visit creates no
//!   session.
//! - Logout ends the local session and, with OIDC, continues to the IdP's `LOGOUT_URL`: Authelia has no
//!   `end_session_endpoint`, so this is its portal logout URL.

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
use rustical_store::auth::{AuthenticationProvider, Principal, PrincipalType};
use rustical_store_sqlite::SqlitePrincipalStore;
use serde::Deserialize;
use std::sync::Arc;
use tower_sessions::Session;
use url::Url;

pub const OIDC_CALLBACK_PATH: &str = "/login/oidc/callback";
/// The login form's CSRF token as a cookie of its own (double submit): an anonymous visit stores nothing
/// on the server (sessions.rs). Host-only, /login only, Strict: no other site's form brings it along.
pub const LOGIN_CSRF: &str = "practical_login_csrf";

/// Sign-in through the identity provider: looks the RustiCal principal up and, with
/// `OIDC__ALLOW_SIGN_UP`, creates it at a first sign-in that finds none — the same write RustiCal's
/// own frontend does (`rustical_frontend`'s `OidcUserStore::ensure_user`,
/// crates/frontend/src/oidc_user_store.rs:26, v0.16.4), on the same file through the same store crate.
///
/// Only a `PrincipalType::Individual` is an account to sign in to: users and groups share one table in
/// RustiCal, and its authentication middleware loads whatever the session names without looking at the
/// type (crates/store/src/auth/middleware.rs:77). An existing principal is therefore never written to
/// either, which also keeps a sign-in from wiping an account's name or password.
#[derive(Clone)]
pub struct SignInStore {
    pub principals: Arc<SqlitePrincipalStore>,
    /// `OIDC__ALLOW_SIGN_UP`: create a principal that does not exist yet.
    pub allow_sign_up: bool,
}

/// Why a sign-in found no account. Rendered as the login page's own message, not as a bare 401:
/// `rustical_oidc` hands our response straight back to the browser (routes.rs:233-234, v0.16.4), and
/// the page does not submit itself again while `error` is set (`auto_oidc` in [`get_login`]).
#[derive(Debug)]
pub enum SignInError {
    /// no principal, and sign-up is off
    NoAccount,
    /// the id from the identity provider cannot be a principal
    IdRejected,
    Store(AppError),
}

impl From<rustical_store::Error> for SignInError {
    fn from(e: rustical_store::Error) -> Self {
        Self::Store(e.into())
    }
}

impl IntoResponse for SignInError {
    fn into_response(self) -> Response {
        match self {
            Self::NoAccount => Redirect::to("/login?error=3").into_response(),
            Self::IdRejected => Redirect::to("/login?error=4").into_response(),
            Self::Store(e) => e.into_response(),
        }
    }
}

/// An id PractiCal is willing to CREATE; an existing principal is looked up, never checked by this.
/// RustiCal itself only forbids `:` and `$` (crates/store_sqlite/src/principal_store.rs:107), the rest
/// is ours: the id is a path segment of every CalDAV URL of that account, and `group.` is where
/// PractiCal's own groups live (`crate::domain::groups::GROUP_PREFIX`).
fn usable_principal_id(id: &str) -> bool {
    !id.is_empty()
        && id != "."
        && id != ".."
        && !id.starts_with(crate::domain::groups::GROUP_PREFIX)
        && !id.contains([':', '$', '/', '\\', '?', '#', '%'])
        && !id.chars().any(|c| c.is_whitespace() || c.is_control())
}

#[async_trait]
impl UserStore for SignInStore {
    type Error = SignInError;

    async fn user_exists(&self, id: &str) -> Result<bool, SignInError> {
        Ok(self.individual(id).await?.is_some())
    }

    async fn ensure_user(&self, id: &str, _memberships: &[&str]) -> Result<(), SignInError> {
        if self.individual(id).await?.is_some() {
            return Ok(());
        }
        if !self.allow_sign_up {
            tracing::info!("sign-in as {id:?}: no RustiCal principal, and sign-up is off");
            return Err(SignInError::NoAccount);
        }
        if !usable_principal_id(id) {
            tracing::warn!("sign-up as {id:?} refused: not a usable principal id");
            return Err(SignInError::IdRejected);
        }
        // as RustiCal's frontend does it: no display name (no claim carries one through
        // `rustical_oidc`), no password, no membership
        let fresh = Principal { id: id.to_owned(), displayname: None, principal_type: PrincipalType::Individual, password: None, memberships: vec![] };
        match self.principals.insert_principal(fresh, false).await {
            Ok(()) => {}
            // `AlreadyExists` means some principal holds the id although the lookup above let none
            // through: either the other half of a race between two first sign-ins of this id, or one
            // `individual` refused (a group without the `group.` prefix, a resource, a room —
            // `insert_principal` looks at no type, crates/store_sqlite/src/principal_store.rs:112,
            // v0.16.4). Reading it back is what tells the two apart; without this, signing up would
            // be a way INTO such a principal.
            Err(rustical_store::Error::AlreadyExists) => {
                return if self.individual(id).await?.is_some() { Ok(()) } else { Err(SignInError::IdRejected) };
            }
            Err(e) => return Err(e.into()),
        }
        tracing::info!("sign-up: created RustiCal principal {id:?}");
        Ok(())
    }
}

impl SignInStore {
    /// The principal under `id`, if it is a user. A group or a resource is no account to sign in to.
    async fn individual(&self, id: &str) -> Result<Option<Principal>, SignInError> {
        let found = self.principals.get_principal(id).await?;
        if let Some(p) = &found
            && p.principal_type != PrincipalType::Individual
        {
            tracing::warn!("sign-in as {id:?} refused: that principal is a {:?}, not a user", p.principal_type);
            return Ok(None);
        }
        Ok(found)
    }
}

/// RustiCal's OIDC client configuration from ours. Separate from [`router`] so a test can read what
/// the library is handed.
pub fn oidc_config(o: &crate::config::OidcSettings) -> Result<OidcConfig, String> {
    let mut cfg = OidcConfig {
        name: o.name.clone(),
        issuer: IssuerUrl::new(o.issuer.clone()).map_err(|e| format!("OIDC issuer: {e}"))?,
        client_id: ClientId::new(o.client_id.clone()),
        client_secret: o.client_secret.clone().map(ClientSecret::new),
        scopes: o.scopes.iter().cloned().map(Scope::new).collect(),
        // Always true, whatever `OIDC__ALLOW_SIGN_UP` says: the flag's only use upstream is the early
        // return that answers an unknown user with a bare 401 (`rustical_oidc` routes.rs:228, v0.16.4).
        // [`SignInStore`] makes the decision instead and renders it as the login page's own message.
        allow_sign_up: true,
        // No group gate of our own: who may sign in is the identity provider's decision, and upstream
        // answers a failed one with a bare 401 that this login page cannot replace (routes.rs:207-210,
        // crates/oidc/src/error.rs, v0.16.4).
        require_group: None,
        claim_userid: Default::default(),
        additional_audiences: vec![],
        // Empty on purpose: a PractiCal group belongs to its members, who add and remove each other
        // (`crate::domain::groups`), so no mapping from identity-provider groups reassigns them.
        assign_memberships: Default::default(),
    };
    // Through its own `Deserialize` (which also reads RustiCal's configuration file, so the spellings
    // cannot drift apart): the type of that field is not nameable outside `rustical_oidc`.
    cfg.claim_userid = serde_json::from_value(serde_json::Value::String(o.claim_userid.clone())).map_err(|e| format!("OIDC claim_userid: {e}"))?;
    Ok(cfg)
}

pub fn router(state: &SharedState) -> Result<Router<SharedState>, String> {
    let mut r = Router::new()
        .route("/login", get(get_login).post(post_login))
        .route("/logout", axum::routing::post(post_logout));
    if let Some(o) = &state.config.oidc {
        r = r.nest_service(
            "/login/oidc",
            oidc_router(
                oidc_config(o)?,
                OidcServiceConfig {
                    default_redirect_path: "/calendar",
                    session_key_user_id: SESSION_USER,
                    callback_path: OIDC_CALLBACK_PATH,
                },
                SignInStore { principals: state.principals.clone(), allow_sign_up: o.allow_sign_up },
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
    /// `OIDC__ALLOW_SIGN_UP`: the closing hint says where a new account comes from.
    pub sign_up: bool,
    /// `error=3`: the sign-in found no account and sign-up is off — the page offers the way to RustiCal.
    pub no_account: bool,
    /// the message above the form (`login_error`)
    pub error: Option<&'static str>,
}

/// `/login?error=…`: 2 = too many failed attempts (`LoginThrottle`), 3 = no principal and sign-up is off,
/// 4 = the identity provider's name cannot be a principal ([`SignInError`]), anything else = the login failed.
fn login_error(e: Option<&str>) -> Option<&'static str> {
    e.map(|e| match e {
        "2" => "login-too-many",
        "3" => "login-no-account",
        "4" => "login-id-rejected",
        _ => "login-failed",
    })
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
        // only where an OIDC callback can send it: without SSO there is no sign-in that finds no account
        no_account: q.error.as_deref() == Some("3") && oidc_name.is_some(),
        ctx,
        redirect_uri: q.redirect_uri.filter(|r| r.starts_with('/') && !r.starts_with("//")),
        oidc_name,
        allow_password_login,
        sign_up: state.config.oidc.as_ref().is_some_and(|o| o.allow_sign_up),
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
/// (Authelia has no end_session_endpoint, so this is its portal logout URL).
pub async fn post_logout(State(state): State<SharedState>, session: Session, Form(f): Form<LogoutForm>) -> Result<Response, AppError> {
    csrf::check(&session, &f.csrf).await?;
    // the push subscriptions this session made end with it (shared devices); before the flush, which drops the list
    crate::domain::push::forget_on_logout(&state, &session).await;
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
    // a live stream cannot read a 401 (EventSource hides the status and asks again): it is told in the stream, which then
    // ends; app.js answers the `auth` event with the login (practical.authLost), as any request does on a 401
    if request.method() == axum::http::Method::GET && request.headers().get(header::ACCEPT).and_then(|v| v.to_str().ok()).is_some_and(|a| a.contains("text/event-stream")) {
        return ([(header::CONTENT_TYPE, "text/event-stream"), (header::CACHE_CONTROL, "no-cache")], "event: auth\ndata: login\n\n").into_response();
    }
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
