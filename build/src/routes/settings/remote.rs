//! `/settings/remote/*` — signing in at a remote server, listing what may be imported or linked,
//! and managing the links.
//!
//! Every route is a POST with CSRF, the login starter included: it makes an outbound request, so a
//! cross-site form must not be able to trigger it. Bodies are JSON (the page is a fetch, like
//! `/settings/onboarding/confirm`), so `X-CSRF-Token` carries the token.
//!
//! Every link read and write is scoped `(id, principal)` and a miss answers 404, not 403 — a link
//! id is not enumerable, and "sync now" against a foreign link would otherwise dial that user's
//! Nextcloud with that user's credential.

use super::*;
use crate::domain::remote::{self, dav::Kind};

#[derive(Deserialize)]
pub(super) struct LoginQ {
    origin: String,
}

#[derive(serde::Serialize)]
pub(super) struct LoginStarted {
    handle: String,
    url: String,
}

/// Starts Nextcloud's Login Flow v2. The user's own browser signs in, which is why this one path
/// works for a native login and for SSO alike.
pub(super) async fn login(State(state): State<SharedState>, user: Principal, _: csrf::Token, crate::extract::Json(q): crate::extract::Json<LoginQ>) -> AppResult<Response> {
    let origin = remote::origin_of(&state, &q.origin)?;
    let (handle, url) = state.remote_logins.start(&state.remote_http, &origin, &user.id).await?;
    Ok(axum::Json(LoginStarted { handle, url }).into_response())
}

#[derive(Deserialize)]
pub(super) struct HandleQ {
    handle: String,
}

#[derive(serde::Serialize)]
pub(super) struct Polled {
    granted: bool,
}

/// One upstream poll per call; the page asks again every two seconds. No server-side loop, so
/// nothing keeps running when the user closes the page.
pub(super) async fn poll(State(state): State<SharedState>, user: Principal, _: csrf::Token, crate::extract::Json(q): crate::extract::Json<HandleQ>) -> AppResult<Response> {
    let granted = state.remote_logins.poll(&state.remote_http, &q.handle, &user.id).await?;
    Ok(axum::Json(Polled { granted }).into_response())
}

#[derive(serde::Serialize)]
pub(super) struct RemoteColl {
    href: String,
    name: String,
    color: String,
    writable: bool,
    components: Vec<String>,
}

#[derive(serde::Serialize)]
pub(super) struct Offer {
    calendars: Vec<RemoteColl>,
    addressbooks: Vec<RemoteColl>,
}

/// What this account may import or link: own collections only, and only those nextcloud does not
/// generate from other data. A shared one is left out on purpose — the server withholds
/// `CLASS:PRIVATE` from a sharee and reduces `CONFIDENTIAL` to a busy slot, so copying one would
/// quietly store wrong data.
pub(super) async fn collections_offered(State(state): State<SharedState>, user: Principal, _: csrf::Token, crate::extract::Json(q): crate::extract::Json<HandleQ>) -> AppResult<Response> {
    let (origin, login, secret) = state.remote_logins.granted(&q.handle, &user.id)?;
    let base: url::Url = origin.parse().map_err(|_| AppError::bad("err-remote-bad-server"))?;
    let dav = remote::dav::Dav::new(state.remote_http.clone(), base, login, secret);
    let (cal_home, ab_home) = dav.discover().await?;
    let principal = principal_of(&dav).await?;
    let mut out = Offer { calendars: vec![], addressbooks: vec![] };
    for c in dav.collections(&cal_home, Kind::Cal, &principal).await? {
        out.calendars.push(RemoteColl { href: c.href, name: c.name, color: c.color, writable: c.writable, components: c.components });
    }
    if !ab_home.is_empty() {
        for c in dav.collections(&ab_home, Kind::Ab, &principal).await? {
            out.addressbooks.push(RemoteColl { href: c.href, name: c.name, color: c.color, writable: c.writable, components: vec![] });
        }
    }
    Ok(axum::Json(out).into_response())
}

/// The principal the filter compares an owner against. Read again here rather than threaded out of
/// `discover`, because that is the only value the comparison may use.
async fn principal_of(dav: &remote::dav::Dav) -> AppResult<String> {
    dav.principal().await
}

#[derive(Deserialize)]
pub(super) struct TakeQ {
    handle: String,
    kind: String,
    href: String,
    #[serde(default, deserialize_with = "crate::extract::text")]
    name: String,
    #[serde(default, deserialize_with = "crate::extract::color")]
    color: String,
    /// present only for a link: "both" | "pull"
    #[serde(default)]
    direction: String,
    #[serde(default)]
    every_min: Option<i64>,
}

#[derive(serde::Serialize)]
pub(super) struct Took {
    id: String,
    owner: String,
    count: usize,
    link_id: Option<String>,
}

/// A one-off import: fetches everything, creates the local collection through the existing
/// importer, and stores no credential at all.
pub(super) async fn import(State(state): State<SharedState>, user: Principal, _: csrf::Token, crate::extract::Json(q): crate::extract::Json<TakeQ>) -> AppResult<Response> {
    let kind = Kind::parse(&q.kind)?;
    let (origin, login, secret) = state.remote_logins.granted(&q.handle, &user.id)?;
    let base: url::Url = origin.parse().map_err(|_| AppError::bad("err-remote-bad-server"))?;
    let dav = remote::dav::Dav::new(state.remote_http.clone(), base, login.clone(), secret.clone());
    let (id, count) = remote::take::import(&state, &user, &dav, kind, &q.href, &q.name, &q.color).await?;
    /* The credential is NOT destroyed here: a user may take several collections from one sign-in,
       and each one is its own request. Doing it per import left the second request without a
       password (measured: picking two calendars imported one). `/settings/remote/done` is what
       ends the sign-in, and the handle expires on its own if the page is simply left. */
    Ok(axum::Json(Took { id, owner: user.id, count, link_id: None }).into_response())
}

/// Creates the local collection AND the link, then runs the first round at once so the user sees
/// the result instead of waiting out an interval.
pub(super) async fn link(State(state): State<SharedState>, user: Principal, _: csrf::Token, crate::extract::Json(q): crate::extract::Json<TakeQ>) -> AppResult<Response> {
    if !remote::sync_enabled(&state) {
        return Err(AppError::bad("err-remote-no-key"));
    }
    let kind = Kind::parse(&q.kind)?;
    let (origin, login, secret) = state.remote_logins.granted(&q.handle, &user.id)?;

    let account = remote::upsert_account(&state, &user, &origin, &login, &secret).await?;
    let (id, count, link_id) = remote::take::link(
        &state,
        &user,
        &account,
        kind,
        &q.href,
        &q.name,
        &q.color,
        q.direction != "pull",
        q.every_min.unwrap_or(remote::DEFAULT_EVERY_MIN),
    )
    .await?;
    Ok(axum::Json(Took { id, owner: user.id, count, link_id: Some(link_id) }).into_response())
}

/// Ends a sign-in: forgets the handle and destroys the app password at the remote. Called once the
/// user is done taking collections, which is the only moment we know they are.
///
/// Not reaching it (the page is closed mid-way) leaves an app password behind at the remote, named
/// after this app in its security settings, where the user can revoke it. The alternative — ending
/// the sign-in after the first import — breaks taking two collections at once, which is worse.
pub(super) async fn done(State(state): State<SharedState>, user: Principal, _: csrf::Token, crate::extract::Json(q): crate::extract::Json<HandleQ>) -> AppResult<Response> {
    if let Ok((origin, login, secret)) = state.remote_logins.granted(&q.handle, &user.id) {
        remote::login::revoke(&state.remote_http, &origin, &login, &secret).await;
    }
    state.remote_logins.forget(&q.handle, &user.id);
    Ok(axum::Json(Polled { granted: false }).into_response())
}

#[derive(Deserialize)]
pub(super) struct LinkEdit {
    #[serde(default)]
    direction: Option<String>,
    #[serde(default)]
    every_min: Option<i64>,
    #[serde(default)]
    action: Option<String>,
}

#[derive(serde::Serialize)]
pub(super) struct LinkState {
    ran_at: Option<String>,
    err: Option<String>,
    pulled: usize,
    pushed: usize,
    conflicts: usize,
}

/// Direction, interval, "sync now" and removal, all on the caller's own link.
pub(super) async fn edit(State(state): State<SharedState>, user: Principal, Path(id): Path<String>, _: csrf::Token, crate::extract::Json(q): crate::extract::Json<LinkEdit>) -> AppResult<Response> {
    let link = remote::link_of(&state, &user, &id).await?;
    match q.action.as_deref() {
        Some("remove") => {
            remote::delete_link(&state, &user, &id).await?;
            return Ok(axum::Json(LinkState { ran_at: None, err: None, pulled: 0, pushed: 0, conflicts: 0 }).into_response());
        }
        Some("now") => {
            if !state.remote_runner.manual_ok(&id) {
                return Err(AppError::bad("err-remote-wait"));
            }
            /* A round makes as many requests as the collection needs, each with its own 60 s
               timeout, so without a ceiling this handler could hold a connection for minutes.
               The round runs in its OWN task: dropping the future here would abort it mid-write,
               and a timeout is a slow server, not a reason to leave half a collection behind. So
               only the waiting ends — the round finishes and its result shows up in the link row. */
            let (st, l) = (state.clone(), link.clone());
            let job = tokio::spawn(async move { st.remote_runner.run(&st, &l, false).await });
            let rep = match tokio::time::timeout(std::time::Duration::from_secs(90), job).await {
                Ok(Ok(r)) => r?,
                // the task panicked (logged by its own guard) or is still running
                Ok(Err(_)) | Err(_) => crate::domain::remote::round::Report::default(),
            };
            let after = remote::link_of(&state, &user, &id).await?;
            return Ok(axum::Json(LinkState { ran_at: after.ran_at, err: after.err, pulled: rep.pulled, pushed: rep.pushed, conflicts: rep.conflicts }).into_response());
        }
        _ => {}
    }
    remote::set_link(&state, &user, &id, q.direction.as_deref().map(|d| d != "pull"), q.every_min).await?;
    let after = remote::link_of(&state, &user, &id).await?;
    Ok(axum::Json(LinkState { ran_at: after.ran_at, err: after.err, pulled: 0, pushed: 0, conflicts: 0 }).into_response())
}
