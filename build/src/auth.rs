//! Login (OIDC via rustical_oidc, optional password), logout (local + IdP portal),
//! and the guard that sends anonymous browsers to /login.

use crate::csrf;
use crate::ctx::{PageCtx, SESSION_USER};
use crate::error::AppError;
use crate::state::SharedState;
use askama::Template;
use askama_web::WebTemplate;
use async_trait::async_trait;
use axum::extract::{Query, Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};
use axum::{Form, Router, routing::get};
use openidconnect::{ClientId, ClientSecret, IssuerUrl, Scope};
use rustical_oidc::{OidcConfig, OidcServiceConfig, UserStore, oidc_router};
use rustical_store::auth::AuthenticationProvider;
use rustical_store_sqlite::SqlitePrincipalStore;
use serde::Deserialize;
use std::sync::Arc;
use tower_sessions::Session;
use url::Url;

pub const OIDC_CALLBACK_PATH: &str = "/login/oidc/callback";

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
            ),
        );
    }
    Ok(r)
}

#[derive(Template, WebTemplate)]
#[template(path = "login.html")]
pub struct LoginPage {
    pub ctx: PageCtx,
    pub redirect_uri: Option<String>,
    pub oidc_name: Option<String>,
    pub allow_password_login: bool,
    pub auto_oidc: bool,
    pub error: bool,
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
    LoginPage {
        auto_oidc: oidc_name.is_some() && !allow_password_login && q.error.is_none() && q.manual.is_none(),
        ctx,
        redirect_uri: q.redirect_uri.filter(|r| r.starts_with('/') && !r.starts_with("//")),
        oidc_name,
        allow_password_login,
        error: q.error.is_some(),
    }
    .into_response()
}

#[derive(Deserialize)]
pub struct LoginForm {
    username: String,
    password: String,
    redirect_uri: Option<String>,
    csrf: String,
}

pub async fn post_login(State(state): State<SharedState>, session: Session, Form(f): Form<LoginForm>) -> Result<Response, AppError> {
    if !state.config.allow_password_login {
        return Err(AppError::Forbidden);
    }
    csrf::check(&session, &f.csrf).await?;
    let target = safe_redirect(f.redirect_uri.as_deref());
    match state.principals.validate_password(&f.username, &f.password).await? {
        Some(user) => {
            session.cycle_id().await?;
            session.insert(SESSION_USER, user.id).await?;
            Ok(Redirect::to(&target).into_response())
        }
        None => {
            tracing::warn!("failed password login as {:?}", f.username); // Debug: a typed CR/LF cannot forge log lines
            Ok(Redirect::to(&format!("/login?error=1&redirect_uri={}", urlenc(&target))).into_response())
        }
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
