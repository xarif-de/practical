//! Connecting a device ([`crate::domain::connect`]): the Login Flow v2 endpoints an app calls and the page
//! where the user confirms, plus the JSON the "add a device" wizard of /settings reads.
//!
//! - [`public_router`] (no login): `POST /index.php/login/v2` (start) and `POST /index.php/login/v2/poll/{id}`.
//!   The app sends no browser metadata, so [`crate::csrf::origin_guard`] lets exactly these two paths through
//!   ([`is_app_endpoint`]); neither reads the session: the start hands out nothing, the poll is authorised by
//!   its token alone. Also `GET /connect/davx5`, the target of the wizard's QR code.
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
use axum::extract::{DefaultBodyLimit, State};
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

/// Open to anyone: a body is at most a poll token (`token=` + 64 characters).
pub fn public_router() -> Router<SharedState> {
    Router::new().route("/index.php/login/v2", post(start)).route("/index.php/login/v2/poll/{id}", post(poll)).route("/connect/davx5", get(open_davx5)).layer(DefaultBodyLimit::max(1024))
}

/// `GET /connect/davx5`: the target of the wizard's QR code — a camera app opens `https://` links only, so the
/// code points here and this answers with the `intent:` link that opens DAVx⁵ in its login (on a Samsung
/// phone, 2026-10-14: DAVx⁵ opened straight away). Public: it reveals nothing but this server's host.
async fn open_davx5(State(state): State<SharedState>) -> Response {
    axum::response::Redirect::to(&connect::davx5_intent(&state.config.public_url)).into_response()
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
/// The app token is made now, as the app takes it, and the user's open wizards learn the device's name.
async fn poll(State(state): State<SharedState>, Path(id): Path<String>, Form(f): Form<PollForm>) -> AppResult<Response> {
    let Some(taken) = state.connect.flows.poll(&id, &f.token, chrono::Utc::now()) else {
        return Ok((StatusCode::NOT_FOUND, "not granted").into_response());
    };
    let app_password = match tokens::create(&state, &taken.user, &taken.name).await {
        Ok(p) => p,
        Err(e) => {
            state.connect.flows.restore(taken); // the app may ask again
            return Err(e);
        }
    };
    state.connect.pairs.connected(&taken.user.id, &taken.name);
    Ok(Json(Grant { server: state.config.public_dav_url.clone(), login_name: taken.user.id, app_password }).into_response())
}

#[derive(Template, WebTemplate)]
#[template(path = "connect.html")]
pub struct ConfirmPage {
    pub ctx: PageCtx,
    pub id: String,
    /// "ask" (the form), "done" (confirmed), "gone" (unknown, expired or another user's)
    pub state: &'static str,
    pub app: String,
    pub name: String,
    /// after the confirmation: what DAVx⁵ will show next (its account name, the collections to switch on)
    pub account: String,
    pub calendars: Vec<String>,
    pub addressbooks: Vec<String>,
    /// this server's host, as the app's browser tab shows it
    pub host: String,
}

impl ConfirmPage {
    fn new(ctx: PageCtx, id: String, state: &'static str, host: String) -> Self {
        ConfirmPage { ctx, id, state, app: String::new(), name: String::new(), account: String::new(), calendars: vec![], addressbooks: vec![], host }
    }
}

/// The app's name for the page: "DAVx⁵" for DAVx⁵ (`DAVx5/<version> (<package>)`, davx5-ose `ProductIds.kt`),
/// else the User-Agent's first word.
fn app_label(ua: &str) -> String {
    if ua.starts_with("DAVx5") {
        return "DAVx⁵".into();
    }
    ua.split([' ', '/']).next().filter(|s| !s.is_empty()).unwrap_or("?").chars().take(40).collect()
}

/// The device name the form suggests (and takes when left empty).
fn default_name(ctx: &PageCtx, ua: &str) -> String {
    if ua.starts_with("DAVx5") { ctx.t("connect-name-android") } else { app_label(ua) }
}

/// The page of a flow: the form while pending; the result for the user who confirmed it (a reload, a second tap).
async fn confirm_page(State(state): State<SharedState>, user: Principal, ctx: PageCtx, Path(id): Path<String>) -> AppResult<ConfirmPage> {
    let now = chrono::Utc::now();
    let host = connect::host_of(&state.config.public_url);
    if let Some(ua) = state.connect.flows.pending(&id, now) {
        return Ok(ConfirmPage { app: app_label(&ua), name: default_name(&ctx, &ua), ..ConfirmPage::new(ctx, id, "ask", host) });
    }
    let confirmed = state.connect.flows.confirmed(&id, &user.id, now);
    done(&state, &user, ctx, id, host, confirmed).await
}

/// "done" with what DAVx⁵ asks next (this user's account and collections), or "gone" for `None`.
async fn done(state: &SharedState, user: &Principal, ctx: PageCtx, id: String, host: String, confirmed: Option<(String, String)>) -> AppResult<ConfirmPage> {
    let Some((ua, name)) = confirmed else { return Ok(ConfirmPage::new(ctx, id, "gone", host)) };
    let books: Vec<String> = crate::domain::collections::addressbooks_for(state, user, false).await?.into_iter().map(|a| a.displayname).collect();
    let calendars = connect::davx5_calendars(crate::domain::collections::calendars_for(state, user, false).await?.into_iter().map(|c| c.displayname), &books);
    Ok(ConfirmPage { app: app_label(&ua), name, account: user.id.clone(), calendars, addressbooks: books, ..ConfirmPage::new(ctx, id, "done", host) })
}

#[derive(Deserialize)]
struct ConfirmForm {
    /// missing = wrong: 403 like a wrong one, not a 400 about the form
    #[serde(default)]
    csrf: String,
    #[serde(default, deserialize_with = "crate::extract::text")]
    name: String,
}

/// The user confirms: the flow learns who and the device's name (the app token follows when the app fetches
/// it, [`poll`]).
async fn confirm(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path(id): Path<String>, Form(f): Form<ConfirmForm>) -> AppResult<ConfirmPage> {
    csrf::check(&session, &f.csrf).await?;
    let now = chrono::Utc::now();
    let mut name: String = f.name.chars().take(NAME_MAX).collect::<String>().trim().to_string();
    if name.is_empty() {
        name = state.connect.flows.pending(&id, now).map(|ua| default_name(&ctx, &ua)).unwrap_or_default();
    }
    let confirmed = state.connect.flows.grant(&id, &user, &name, now);
    done(&state, &user, ctx, id, connect::host_of(&state.config.public_url), confirmed).await
}

#[derive(serde::Serialize)]
struct PairAnswer<'a> {
    pair: String,
    #[serde(flatten)]
    codes: &'a connect::Codes,
}

/// `POST /settings/connect/pair`: a pairing for this run of the wizard, and its QR codes (sent here: the page
/// gets them only when the wizard runs).
async fn pair(State(state): State<SharedState>, user: Principal, _t: csrf::Token) -> Response {
    let pair = state.connect.pairs.create(&user.id, chrono::Utc::now());
    Json(PairAnswer { pair, codes: state.connect.codes(&state.config.public_url) }).into_response()
}

#[derive(Deserialize)]
struct StatusQuery {
    pair: String,
}

/// `GET /settings/connect/status?pair=…`: the device that got connected meanwhile, if any.
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
