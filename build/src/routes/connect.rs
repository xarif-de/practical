//! Connecting a device ([`crate::domain::connect`]): the Login Flow v2 endpoints an app calls and the page
//! where the user confirms, plus the JSON the "add a device" wizard of /settings reads.
//!
//! - [`public_router`] (no login): `POST /index.php/login/v2` (start) and `POST /index.php/login/v2/poll/{id}`.
//!   The app sends no browser metadata, so [`crate::csrf::origin_guard`] lets exactly these two paths through
//!   ([`is_app_endpoint`]); neither reads the session: the start hands out nothing, the poll is authorised by
//!   its token alone.
//! - [`router`] (behind `require_user`): the confirmation page `GET|POST /index.php/login/v2/flow/{id}` (a
//!   form with the session's CSRF token), `POST /settings/connect/pair` and `GET /settings/connect/status`.

use crate::csrf;
use crate::ctx::PageCtx;
use crate::domain::connect::{self, Grant, NAME_MAX};
use crate::domain::tokens;
use crate::error::AppResult;
use crate::extract::{Form, Path, Query};
use crate::state::SharedState;
use askama::Template;
use askama_web::WebTemplate;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use rustical_store::auth::Principal;
use serde::Deserialize;
use tower_sessions::Session;

/// The two app endpoints: no session, no browser — exempt from the origin check.
pub fn is_app_endpoint(method: &axum::http::Method, path: &str) -> bool {
    method == axum::http::Method::POST && (path == "/index.php/login/v2" || path.strip_prefix("/index.php/login/v2/poll/").is_some_and(|id| !id.is_empty() && !id.contains('/')))
}

pub fn public_router() -> Router<SharedState> {
    Router::new().route("/index.php/login/v2", post(start)).route("/index.php/login/v2/poll/{id}", post(poll))
}

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/index.php/login/v2/flow/{id}", get(confirm_page).post(confirm))
        .route("/settings/connect/pair", post(pair))
        .route("/settings/connect/status", get(status))
}

#[derive(serde::Serialize)]
struct StartAnswer {
    poll: PollInfo,
    login: String,
}

#[derive(serde::Serialize)]
struct PollInfo {
    token: String,
    endpoint: String,
}

/// `POST /index.php/login/v2`: a new flow; `login` is where the app sends the browser.
async fn start(State(state): State<SharedState>, headers: HeaderMap) -> Json<StartAnswer> {
    let app = headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).unwrap_or("");
    let (id, token) = state.connect.flows.start(app, chrono::Utc::now());
    let base = &state.config.public_url;
    Json(StartAnswer { poll: PollInfo { token, endpoint: format!("{base}/index.php/login/v2/poll/{id}") }, login: format!("{base}/index.php/login/v2/flow/{id}") })
}

#[derive(Deserialize)]
struct PollForm {
    #[serde(default)]
    token: String,
}

/// `POST /index.php/login/v2/poll/{id}`: 404 until confirmed, then the credentials ONCE (Nextcloud's contract).
async fn poll(State(state): State<SharedState>, Path(id): Path<String>, Form(f): Form<PollForm>) -> Response {
    match state.connect.flows.poll(&id, &f.token, chrono::Utc::now()) {
        Some(g) => Json(g).into_response(),
        None => (StatusCode::NOT_FOUND, "not granted").into_response(),
    }
}

#[derive(Template, WebTemplate)]
#[template(path = "connect.html")]
pub struct ConfirmPage {
    pub ctx: PageCtx,
    pub id: String,
    /// "ask" (the form), "done" (confirmed), "gone" (unknown, expired or already confirmed)
    pub state: &'static str,
    pub app: String,
    pub name: String,
}

/// The app's name for the page: "DAVx⁵" for DAVx⁵ (`DAVx5/<version> (<package>)`, davx5-ose `ProductIds.kt`),
/// else the User-Agent's first word.
fn app_label(ua: &str) -> String {
    if ua.starts_with("DAVx5") {
        return "DAVx⁵".into();
    }
    ua.split([' ', '/']).next().filter(|s| !s.is_empty()).unwrap_or("?").chars().take(40).collect()
}

async fn confirm_page(State(state): State<SharedState>, _user: Principal, ctx: PageCtx, Path(id): Path<String>) -> ConfirmPage {
    let Some(ua) = state.connect.flows.pending(&id, chrono::Utc::now()) else {
        return ConfirmPage { ctx, id, state: "gone", app: String::new(), name: String::new() };
    };
    let app = app_label(&ua);
    let name = if ua.starts_with("DAVx5") { ctx.t("connect-name-android") } else { app.clone() };
    ConfirmPage { ctx, id, state: "ask", app, name }
}

#[derive(Deserialize)]
struct ConfirmForm {
    /// missing = wrong: 403 like a wrong one, not a 400 about the form
    #[serde(default)]
    csrf: String,
    #[serde(default, deserialize_with = "crate::extract::text")]
    name: String,
    #[serde(default)]
    app: String,
}

/// The user confirms: a new app token named after the device, handed to the flow and announced to the
/// user's open wizards.
async fn confirm(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path(id): Path<String>, Form(f): Form<ConfirmForm>) -> AppResult<ConfirmPage> {
    csrf::check(&session, &f.csrf).await?;
    let now = chrono::Utc::now();
    let gone = || ConfirmPage { ctx: ctx.clone(), id: id.clone(), state: "gone", app: String::new(), name: String::new() };
    if state.connect.flows.pending(&id, now).is_none() {
        return Ok(gone());
    }
    let mut name: String = f.name.trim().chars().take(NAME_MAX).collect();
    if name.is_empty() {
        name = ctx.t("connect-name-android");
    }
    let password = tokens::create(&state, &user, &name).await?;
    if !state.connect.flows.grant(&id, Grant { server: state.config.public_dav_url.clone(), login_name: user.id.clone(), app_password: password.clone() }, now) {
        // expired between the two checks: the token nobody will fetch goes again
        let prefix = password.split('_').next().unwrap_or_default();
        if let Some(t) = tokens::list(&state, &user).await?.into_iter().find(|t| t.name == name && t.id.starts_with(prefix)) {
            tokens::delete(&state, &user, &t.id).await?;
        }
        return Ok(gone());
    }
    state.connect.pairs.connected(&user.id, &name);
    Ok(ConfirmPage { ctx, id, state: "done", app: f.app, name })
}

#[derive(serde::Serialize)]
struct Code {
    url: String,
    svg: String,
}

#[derive(serde::Serialize)]
struct StoreCode {
    key: &'static str,
    url: &'static str,
    short: &'static str,
    svg: String,
}

#[derive(serde::Serialize)]
struct PairAnswer {
    pair: String,
    /// continues the wizard on the other device at the install step / at the connect step
    install: Code,
    connect: Code,
    stores: Vec<StoreCode>,
}

#[derive(Deserialize)]
struct PairForm {
    /// a pairing the page already has (it was opened through one): kept if it is the user's
    #[serde(default)]
    pair: String,
}

/// `POST /settings/connect/pair`: the wizard's pairing and its QR codes (made here: the page gets them only
/// when the wizard runs).
async fn pair(State(state): State<SharedState>, user: Principal, _t: csrf::Token, Form(f): Form<PairForm>) -> Json<PairAnswer> {
    let now = chrono::Utc::now();
    let id = if !f.pair.is_empty() && state.connect.pairs.status(&f.pair, &user.id, now).is_some() { f.pair } else { state.connect.pairs.create(&user.id, now) };
    let base = &state.config.public_url;
    let code = |hash: &str| {
        let url = format!("{base}/settings?pair={id}#{hash}");
        Code { svg: connect::qr_svg(&url), url }
    };
    Json(PairAnswer {
        install: code("add-android-install"),
        connect: code("add-android-connect"),
        stores: connect::STORES.iter().map(|s| StoreCode { key: s.key, url: s.url, short: s.short, svg: connect::qr_svg(s.url) }).collect(),
        pair: id,
    })
}

#[derive(Deserialize)]
struct StatusQuery {
    pair: String,
}

/// `GET /settings/connect/status?pair=…`: opened on the other device, and the device that got connected.
async fn status(State(state): State<SharedState>, user: Principal, Query(q): Query<StatusQuery>) -> Response {
    match state.connect.pairs.status(&q.pair, &user.id, chrono::Utc::now()) {
        Some(s) => Json(s).into_response(),
        None => (StatusCode::NOT_FOUND, Json(serde_json::json!({ "gone": true }))).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_two_app_endpoints_skip_the_origin_check() {
        let post = axum::http::Method::POST;
        assert!(is_app_endpoint(&post, "/index.php/login/v2"));
        assert!(is_app_endpoint(&post, "/index.php/login/v2/poll/abc"));
        assert!(!is_app_endpoint(&post, "/index.php/login/v2/flow/abc"), "the confirmation is a browser form");
        assert!(!is_app_endpoint(&post, "/index.php/login/v2/poll/abc/x"));
        assert!(!is_app_endpoint(&post, "/index.php/login/v2/poll/"));
        assert!(!is_app_endpoint(&axum::http::Method::GET, "/index.php/login/v2"));
    }

    #[test]
    fn the_app_label() {
        assert_eq!(app_label("DAVx5/4.5.20-ose (at.bitfire.davdroid)"), "DAVx⁵");
        assert_eq!(app_label("Thunderbird/128"), "Thunderbird");
        assert_eq!(app_label(""), "?");
    }
}
