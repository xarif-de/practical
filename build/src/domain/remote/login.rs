//! Nextcloud Login Flow v2: the one sign-in that works for a native login AND for SSO, because the
//! user's own browser does it. PractiCal never sees the account password.
//!
//! Measured end to end against nextcloud 35.0.1:
//! 1. `POST <base>/index.php/login/v2` with `User-Agent: PractiCal` → `{poll:{token,endpoint},login}`;
//!    the grant page shows the User-Agent as the app name.
//! 2. the user opens `login` in a browser and grants access.
//! 3. `POST <base>/login/v2/poll` with the token → 404 until granted, then ONCE 200 with
//!    `{server, loginName, appPassword}`. A second poll is 404 again.
//!
//! Two things the answer must not be trusted about:
//! - `server` is the URL the BROWSER used (measured: asked `localhost:8099`, got `127.0.0.1:8099`),
//!   so we keep our own allow-listed base and ignore it;
//! - the 200 arrives once, so a lost answer means starting over — the handle below is what the page
//!   polls through, and it is dropped as soon as it was used.

use crate::error::{AppError, AppResult};
use std::collections::HashMap;
use std::sync::Mutex;

/// How long a pending login may sit unused: the life of Nextcloud's own poll token.
const PENDING_SECS: i64 = 20 * 60;

#[derive(Debug, serde::Deserialize)]
struct StartAnswer {
    poll: Poll,
    login: String,
}

#[derive(Debug, serde::Deserialize)]
struct Poll {
    token: String,
    endpoint: String,
}

#[derive(Debug, serde::Deserialize)]
struct Granted {
    #[serde(rename = "loginName")]
    login_name: String,
    #[serde(rename = "appPassword")]
    app_password: String,
}

/// A login the user has not finished yet. Deliberately NOT in the session store, which is SQLite:
/// an app password reaches the whole Nextcloud account, so a one-off import must not put one at
/// rest at all.
struct Pending {
    principal: String,
    origin: String,
    token: String,
    endpoint: String,
    at: chrono::DateTime<chrono::Utc>,
    /// set once the poll succeeded; the import reads it from here and then drops the handle
    got: Option<(String, super::dav::Secret)>,
    /// the credential came from an account we already keep ([`Logins::adopt`]), so it is NOT ours
    /// to destroy: it belongs to a link the user still has, and revoking it would break that sync
    borrowed: bool,
}

#[derive(Default)]
pub struct Logins {
    by_handle: Mutex<HashMap<String, Pending>>,
}

impl Logins {
    /// Starts a login and returns (handle, browser URL). One pending login per user: a second start
    /// replaces the first, so an abandoned attempt cannot pile up.
    pub async fn start(&self, http: &reqwest::Client, origin: &str, principal: &str) -> AppResult<(String, String)> {
        let url = format!("{origin}/index.php/login/v2");
        // the transport failure is mapped, not swallowed: a private address is the likeliest one
        // and has a message of its own (`dav::net_err`)
        let r = http.post(&url).header(reqwest::header::USER_AGENT, "PractiCal").send().await.map_err(super::dav::net_err)?;
        if !r.status().is_success() {
            return Err(AppError::BadRequest(crate::error::Msg { key: "err-remote-status", args: vec![("status", r.status().as_u16().to_string())] }));
        }
        let a: StartAnswer = r.json().await.map_err(|_| AppError::bad("err-remote-login"))?;
        // the endpoint comes from the server: it must stay on the origin we were allowed to dial
        let endpoint: url::Url = a.poll.endpoint.parse().map_err(|_| AppError::bad("err-remote-login"))?;
        let want: url::Url = origin.parse().map_err(|_| AppError::bad("err-remote-login"))?;
        if (endpoint.scheme(), endpoint.host_str(), endpoint.port_or_known_default()) != (want.scheme(), want.host_str(), want.port_or_known_default()) {
            return Err(AppError::bad("err-remote-login"));
        }
        let handle = uuid::Uuid::new_v4().to_string();
        let mut m = self.by_handle.lock().unwrap();
        m.retain(|_, p| p.principal != principal && chrono::Utc::now().signed_duration_since(p.at).num_seconds() < PENDING_SECS);
        m.insert(
            handle.clone(),
            Pending {
                principal: principal.to_string(),
                origin: origin.to_string(),
                token: a.poll.token,
                endpoint: a.poll.endpoint,
                at: chrono::Utc::now(),
                got: None,
                borrowed: false,
            },
        );
        // the login URL is shown to the user, so it must be on the expected origin too
        let login: url::Url = a.login.parse().map_err(|_| AppError::bad("err-remote-login"))?;
        if (login.scheme(), login.host_str(), login.port_or_known_default()) != (want.scheme(), want.host_str(), want.port_or_known_default()) {
            return Err(AppError::bad("err-remote-login"));
        }
        Ok((handle, a.login))
    }

    /// Polls once. `Ok(false)` = not granted yet, the page asks again. Never loops server-side, so
    /// nothing keeps running when the user walks away.
    pub async fn poll(&self, http: &reqwest::Client, handle: &str, principal: &str) -> AppResult<bool> {
        let (token, endpoint) = {
            let m = self.by_handle.lock().unwrap();
            let p = m.get(handle).filter(|p| p.principal == principal).ok_or(AppError::NotFound)?;
            if p.got.is_some() {
                return Ok(true);
            }
            (p.token.clone(), p.endpoint.clone())
        };
        let r = http
            .post(&endpoint)
            .header(reqwest::header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(format!("token={}", super::pct(&token)))
            .send()
            .await
            .map_err(super::dav::net_err)?;
        if r.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(false);
        }
        if !r.status().is_success() {
            return Err(AppError::BadRequest(crate::error::Msg { key: "err-remote-status", args: vec![("status", r.status().as_u16().to_string())] }));
        }
        let g: Granted = r.json().await.map_err(|_| AppError::bad("err-remote-login"))?;
        let mut m = self.by_handle.lock().unwrap();
        if let Some(p) = m.get_mut(handle).filter(|p| p.principal == principal) {
            p.got = Some((g.login_name, super::dav::Secret(g.app_password)));
        }
        Ok(true)
    }

    /// (origin, login, secret) of a granted handle, without consuming it — a user may link several
    /// collections from one login.
    pub fn granted(&self, handle: &str, principal: &str) -> AppResult<(String, String, super::dav::Secret)> {
        let m = self.by_handle.lock().unwrap();
        let p = m.get(handle).filter(|p| p.principal == principal).ok_or(AppError::NotFound)?;
        let (login, secret) = p.got.clone().ok_or(AppError::NotFound)?;
        Ok((p.origin.clone(), login, secret))
    }

    /// Puts a credential we already keep into the map and returns a handle for it, so a page can
    /// go on as if a sign-in had just been granted. Marked `borrowed`: [`Logins::is_borrowed`] is
    /// what stops the import from revoking a password that an existing link depends on.
    pub fn adopt(&self, origin: &str, principal: &str, login: &str, secret: super::dav::Secret) -> String {
        let handle = uuid::Uuid::new_v4().to_string();
        let mut m = self.by_handle.lock().unwrap();
        m.retain(|_, p| p.principal != principal && chrono::Utc::now().signed_duration_since(p.at).num_seconds() < PENDING_SECS);
        m.insert(
            handle.clone(),
            Pending {
                principal: principal.to_string(),
                origin: origin.to_string(),
                token: String::new(),
                endpoint: String::new(),
                at: chrono::Utc::now(),
                got: Some((login.to_string(), secret)),
                borrowed: true,
            },
        );
        handle
    }

    /// Whether this handle's credential belongs to an account we keep. A borrowed one must survive
    /// the batch that used it.
    pub fn is_borrowed(&self, handle: &str, principal: &str) -> bool {
        let m = self.by_handle.lock().unwrap();
        m.get(handle).is_some_and(|p| p.principal == principal && p.borrowed)
    }

    /// Forgets a handle: after a one-off import (whose app password is also revoked at Nextcloud)
    /// or when the user leaves the page.
    pub fn forget(&self, handle: &str, principal: &str) {
        let mut m = self.by_handle.lock().unwrap();
        if m.get(handle).is_some_and(|p| p.principal == principal) {
            m.remove(handle);
        }
    }
}

/// Destroys an app password at Nextcloud. Used right after a one-off import and when the last link
/// of an account goes, so a credential we no longer need does not stay valid.
pub async fn revoke(http: &reqwest::Client, origin: &str, login: &str, secret: &super::dav::Secret) {
    let url = format!("{origin}/ocs/v2.php/core/apppassword");
    let r = http.delete(&url).basic_auth(login, Some(&secret.0)).header("OCS-APIRequest", "true").send().await;
    match r {
        Ok(r) if r.status().is_success() => tracing::info!("remote: app password revoked at {origin}"),
        // best effort: the user can always revoke it in Nextcloud, and the link is gone either way
        Ok(r) => tracing::warn!("remote: revoking the app password at {origin} answered {}", r.status()),
        Err(_) => tracing::warn!("remote: revoking the app password at {origin} failed"),
    }
}
