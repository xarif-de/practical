//! Connecting a device ([`crate::domain::connect`]): the Login Flow v2 endpoints an app calls and the page
//! where the user confirms, plus the JSON the "add a device" wizard of /settings reads.
//!
//! - [`public_router`] (no login): `POST /index.php/login/v2` (start) and `POST /index.php/login/v2/poll/{id}`.
//!   The app sends no browser metadata, so [`crate::csrf::origin_guard`] lets exactly these two paths through
//!   ([`is_app_endpoint`]); neither reads the session: the start hands out nothing, the poll is authorised by
//!   its token alone. Also `GET /connect/davx5`, the target of the wizard's QR code, and a profile link's
//!   page `GET /connect/apple/{id}` and file `GET /connect/apple/{id}/PractiCal.mobileconfig`: the
//!   configuration profile an iPhone or iPad installs.
//! - [`router`] (behind `require_user`): the confirmation page `GET|POST /index.php/login/v2/flow/{id}` (a
//!   form with the session's CSRF token), `POST /settings/connect/pair`, `GET /settings/connect/status`,
//!   `POST /settings/connect/apple` (a profile link) and `POST /settings/connect/manual` (an iPhone's password
//!   for its manual setup).

use crate::csrf;
use crate::ctx::PageCtx;
use crate::domain::connect::{self, Grant, NAME_MAX};
use crate::domain::groups::GroupInfo;
use crate::domain::tokens;
use crate::error::{AppError, AppResult};
use crate::extract::{Form, Path, Query};
use crate::state::SharedState;
use askama::Template;
use askama_web::WebTemplate;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use rustical_store::auth::{AuthenticationProvider, Principal};
use serde::Deserialize;
use tower_sessions::Session;

/// The two app endpoints: no session, no browser — exempt from the origin check.
pub fn is_app_endpoint(method: &axum::http::Method, path: &str) -> bool {
    method == axum::http::Method::POST && (path == "/index.php/login/v2" || path.strip_prefix("/index.php/login/v2/poll/").is_some_and(|id| !id.is_empty() && !id.contains('/')))
}

/// Open to anyone: a body is at most a poll token (`token=` + 64 characters).
pub fn public_router() -> Router<SharedState> {
    Router::new()
        .route("/index.php/login/v2", post(start))
        .route("/index.php/login/v2/poll/{id}", post(poll))
        .route("/connect/davx5", get(open_davx5))
        .route("/connect/apple/{id}", get(apple_page))
        .route("/connect/apple/{id}/manual", get(apple_manual))
        .route("/connect/apple/{id}/{file}", get(apple_download))
        .layer(DefaultBodyLimit::max(1024))
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
        .route("/settings/connect/apple", post(apple_link))
        .route("/settings/connect/manual", post(manual_password))
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
    state.connect.pairs.connected(&taken.user.id, &taken.name, false);
    Ok(Json(Grant { server: state.config.public_dav_url.clone(), login_name: taken.user.id, app_password }).into_response())
}

#[derive(Template, WebTemplate)]
#[template(path = "connect.html")]
pub struct ConfirmPage {
    pub ctx: PageCtx,
    pub id: String,
    /// "ask" (the form), "done" (confirmed), "gone" (unknown, expired or another user's); a profile link's
    /// pages: "apple" (`app` "other": a browser that cannot install it, `name` the link), "apple-manual" (the
    /// setup by hand: `dav`, `account`, `groups`), "gone-apple"
    pub state: &'static str,
    pub app: String,
    pub name: String,
    /// after the confirmation: what DAVx⁵ will show next (its account name, the collections to switch on)
    pub account: String,
    pub calendars: Vec<String>,
    pub addressbooks: Vec<String>,
    /// this server's host, as the app's browser tab shows it
    pub host: String,
    /// "apple-manual": the DAV server, and per group with calendars (its name, the login `user$group`)
    pub dav: String,
    pub groups: Vec<(String, String)>,
}

impl ConfirmPage {
    fn new(ctx: PageCtx, id: String, state: &'static str, host: String) -> Self {
        ConfirmPage { ctx, id, state, app: String::new(), name: String::new(), account: String::new(), calendars: vec![], addressbooks: vec![], host, dav: String::new(), groups: vec![] }
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

#[derive(Deserialize)]
struct ManualQuery {
    /// the device's name (the wizard runs on it, or names it)
    name: Option<String>,
}

#[derive(serde::Serialize)]
struct AppleLink {
    /// the link's page (the QR code's target), and the profile itself (the button on the device)
    url: String,
    file: String,
    qr: String,
    /// the QR code of the link's manual guide (`…/manual`, the iPhone by hand from a computer)
    manual_qr: String,
    /// the asking browser cannot install a profile ([`connect::installs_profiles`]): the device is told to use Safari
    other: bool,
}

/// `POST /settings/connect/apple`: a profile link for this user, and its QR codes (the wizard asks for a new
/// one before this one expires).
async fn apple_link(State(state): State<SharedState>, user: Principal, headers: HeaderMap, _t: csrf::Token) -> Json<AppleLink> {
    let id = state.connect.links.create(&user.id, chrono::Utc::now());
    let url = format!("{}/connect/apple/{id}", state.config.public_url);
    Json(AppleLink { qr: connect::qr_svg(&url), manual_qr: connect::qr_svg(&format!("{url}/manual")), file: format!("{url}/{APPLE_FILE}"), url, other: !connect::installs_profiles(user_agent(&headers)) })
}

fn user_agent(headers: &HeaderMap) -> &str {
    headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).unwrap_or("")
}

#[derive(serde::Serialize)]
struct ManualPassword {
    name: String,
    password: String,
}

/// `POST /settings/connect/manual?name=iPhone`: the password of the iPhone's manual setup (#add-ios-values), an
/// app token named after the device; the wizard asks once per run, when the password is first asked for (copied
/// or shown), and shows it as dots.
async fn manual_password(State(state): State<SharedState>, user: Principal, Query(q): Query<ManualQuery>, _t: csrf::Token) -> AppResult<Response> {
    let name: String = q.name.as_deref().unwrap_or("iPhone").chars().filter(|c| !c.is_control()).take(NAME_MAX).collect();
    let (name, password) = tokens::create_unique(&state, &user, if name.trim().is_empty() { "iPhone" } else { &name }).await?;
    Ok(no_store(Json(ManualPassword { name, password }).into_response()))
}

/// A response that carries a password: never into a cache (the app's default `private, no-cache` lets a browser
/// keep it on disk), as the profile itself.
fn no_store(mut r: Response) -> Response {
    r.headers_mut().insert(header::CACHE_CONTROL, header::HeaderValue::from_static("no-store"));
    r
}

/// A profile link's answer: the profile, or the manual setup's password as JSON.
fn apple_answer(profile: bool, made: String) -> Response {
    if profile {
        let disposition = format!("attachment; filename=\"{APPLE_FILE}\"");
        no_store(([(header::CONTENT_TYPE, "application/x-apple-aspen-config"), (header::CONTENT_DISPOSITION, disposition.as_str())], made).into_response())
    } else {
        no_store(Json(serde_json::json!({ "password": made })).into_response())
    }
}

/// The page of an unknown or expired profile link.
fn apple_gone(state: &SharedState, ctx: PageCtx, id: String) -> Response {
    (StatusCode::GONE, ConfirmPage::new(ctx, id, "gone-apple", connect::host_of(&state.config.public_url))).into_response()
}

/// The profile's file name: Safari installs a `.mobileconfig` (support.apple.com/guide/profile-manager/pmdbd71ebc9).
pub const APPLE_FILE: &str = "PractiCal.mobileconfig";

/// `GET /connect/apple/{id}`: the page the QR code opens on the iPhone — the download button and how to
/// install the profile; a browser that cannot install profiles is told to use Safari (Firefox and
/// DuckDuckGo only save the file: firefox-ios #25476, duckduckgo/iOS #830). Public, like the download.
async fn apple_page(State(state): State<SharedState>, ctx: PageCtx, headers: HeaderMap, Path(id): Path<String>) -> Response {
    let Some(user) = state.connect.links.user(&id, chrono::Utc::now()) else {
        return apple_gone(&state, ctx, id);
    };
    let app = if connect::installs_profiles(user_agent(&headers)) { String::new() } else { "other".into() };
    ConfirmPage { app, account: user, name: format!("{}/connect/apple/{id}", state.config.public_url), ..ConfirmPage::new(ctx, id, "apple", connect::host_of(&state.config.public_url)) }.into_response()
}

/// `GET /connect/apple/{id}/manual`: the iPhone's manual setup on the iPhone, opened from a computer's code
/// without a login: the way through Settings and the values, each with a copy button; the password comes
/// from `…/password` when it is first copied. Public, like the profile.
async fn apple_manual(State(state): State<SharedState>, ctx: PageCtx, Path(id): Path<String>) -> AppResult<Response> {
    let Some(user) = state.connect.links.open_manual(&id, chrono::Utc::now()) else {
        return Ok(apple_gone(&state, ctx, id));
    };
    let principal = state.principals().get_principal(&user).await?.ok_or(AppError::NotFound)?;
    let groups = crate::domain::groups::list_for_user(&state, &principal).await?.into_iter().filter(GroupInfo::needs_cal_account).map(|g| (g.displayname, format!("{user}${}", g.id))).collect();
    Ok(ConfirmPage { account: user, dav: state.config.public_dav_url.clone(), groups, ..ConfirmPage::new(ctx, id, "apple-manual", connect::host_of(&state.config.public_url)) }.into_response())
}

/// `GET /connect/apple/{id}/PractiCal.mobileconfig` (the configuration profile) or `…/password` (the manual
/// setup's password, JSON), made at the first request with an app token of its own (the device's name:
/// [`connect::apple_device_name`]); the user's open wizards learn the name and which of the two it took. Public:
/// the camera opens the link in a browser that may not be signed in; the link is the secret
/// ([`connect::Links::create`]: briefly, one device — its client address and User-Agent — and one of the two
/// kinds). A second request of that device while it is made waits for it.
async fn apple_download(State(state): State<SharedState>, ctx: PageCtx, headers: HeaderMap, peer: Option<axum::Extension<axum::extract::ConnectInfo<std::net::SocketAddr>>>, Path((id, file)): Path<(String, String)>) -> AppResult<Response> {
    let ua = user_agent(&headers);
    let profile = file == APPLE_FILE;
    // the kind is part of the device: a link that gave a password gives no profile, and the other way round
    let device = format!("{} {} {ua}", if profile { "profile" } else { "password" }, crate::auth::client_addr(&headers, peer.map(|p| p.0.0)));
    let links = &state.connect.links;
    let user = loop {
        let changed = links.changed.notified();
        tokio::pin!(changed);
        changed.as_mut().enable(); // before the look: a profile finished in between still wakes it
        match (profile || file == "password").then(|| links.open(&id, &device, chrono::Utc::now())).flatten() {
            Some(connect::Opened::Again(made)) => return Ok(apple_answer(profile, made)),
            Some(connect::Opened::Make(user)) => break user,
            Some(connect::Opened::Busy) if tokio::time::timeout(std::time::Duration::from_secs(10), changed).await.is_ok() => continue,
            _ => return Ok(apple_gone(&state, ctx, id)),
        }
    };
    let name = connect::apple_device_name(ua);
    let description = ctx.t1("apple-profile-text", "user", &user);
    // made in a task of its own: a client that goes away must not leave the link taken or a token half made
    let (st, link) = (state.clone(), id.clone());
    let task = tokio::spawn(async move {
        let made = match profile {
            true => make_profile(&st, &user, name, &description).await,
            false => async { tokens::create_unique(&st, &st.principals().get_principal(&user).await?.ok_or(AppError::NotFound)?, name).await }.await,
        };
        st.connect.links.made(&id, made.as_ref().ok().map(|(_, p)| p.as_str()), chrono::Utc::now());
        if let Ok((name, _)) = &made {
            st.connect.pairs.connected(&user, name, !profile);
        }
        made.map(|(_, p)| p)
    });
    let made = task.await.map_err(|e| {
        state.connect.links.made(&link, None, chrono::Utc::now()); // it panicked: the link is fresh again
        AppError::Internal(e.to_string())
    })??;
    Ok(apple_answer(profile, made))
}

/// The profile of `user`'s accounts with a new app token ([`tokens::create_unique`] of `name`). Returns (the
/// token's name, the profile); the token is made last: nothing can fail after it.
async fn make_profile(state: &SharedState, user: &str, name: &str, description: &str) -> AppResult<(String, String)> {
    let user = state.principals().get_principal(user).await?.ok_or(AppError::NotFound)?;
    let mut accounts = vec![connect::AppleAccount { carddav: false, principal: user.id.clone(), description: "PractiCal".into() }, connect::AppleAccount { carddav: true, principal: user.id.clone(), description: "PractiCal".into() }];
    // a group's calendars need an account of their own (connect::apple_profile); its address books do not:
    // Apple's Contacts follows every addressbook-home-set entry (macOS 26.2 fetched the group principals,
    // RustiCal issue #180), and RustiCal's own profile has one CardDAV account
    for g in crate::domain::groups::list_for_user(state, &user).await?.into_iter().filter(GroupInfo::needs_cal_account) {
        accounts.push(connect::AppleAccount { carddav: false, principal: g.id, description: format!("PractiCal · {}", g.displayname) });
    }
    let (name, password) = tokens::create_unique(state, &user, name).await?;
    let host = connect::host_of(&state.config.public_url);
    let profile = connect::apple_profile(&connect::AppleProfile { dav_url: &state.config.public_dav_url, user: &user.id, password: &password, host: &host, name: "PractiCal", description }, &accounts);
    Ok((name, profile))
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
