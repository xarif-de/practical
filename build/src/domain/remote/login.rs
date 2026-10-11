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

/// How long a credential stays usable after the batch that fetched it finished, before it is
/// revoked at the remote. Someone who imports a calendar usually has the next one right behind it,
/// and signing in again for each is work the sign-in screen teaches them to click through unread.
///
/// It lives in memory only, never in the database, so this is a window and not storage: a restart
/// inside it leaves a password at the remote, named after this app in its security settings, which
/// is already what happens when the page is closed mid-flow.
const KEEP_SECS: i64 = 15 * 60;

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
    /// the credential came from an account we already keep ([`Logins::adopt`]) or was handed to
    /// one ([`Logins::claim`]), so it is NOT ours to destroy: it belongs to a link the user still
    /// has, and revoking it would break that sync
    borrowed: bool,
    /// the batch that used it is over ([`Logins::park`]). Still usable for `KEEP_SECS`, then
    /// revoked — which is the one place a one-off import's password is destroyed.
    parked_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Makes room for a new login of `principal`: one pending login per user, so an abandoned attempt
/// cannot pile up.
///
/// A PARKED credential of theirs is not pending — it is still usable, and dropping it here would
/// leave its password valid at the remote with nothing left to revoke it with. It stays until its
/// window runs out (`sweep`), which is the only path that revokes.
fn make_room(m: &mut HashMap<String, Pending>, principal: &str, now: chrono::DateTime<chrono::Utc>) {
    m.retain(|_, p| p.principal != principal || p.parked_at.is_some());
    let _ = sweep(m, now);
}

/// Drops what is of no use any more and hands back the credentials that must be revoked first.
/// Parked ones are kept for `KEEP_SECS`; a borrowed one is never revoked, whatever its age.
fn sweep(m: &mut HashMap<String, Pending>, now: chrono::DateTime<chrono::Utc>) -> Vec<(String, String, super::dav::Secret)> {
    let mut out = vec![];
    m.retain(|_, p| {
        if let Some(t) = p.parked_at {
            if now.signed_duration_since(t).num_seconds() < KEEP_SECS {
                return true;
            }
            if !p.borrowed && let Some((login, secret)) = p.got.clone() {
                out.push((p.origin.clone(), login, secret));
            }
            return false;
        }
        now.signed_duration_since(p.at).num_seconds() < PENDING_SECS
    });
    out
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
        make_room(&mut m, principal, chrono::Utc::now());
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
                parked_at: None,
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
    /// go on as if a sign-in had just been granted. Marked `borrowed`, which is what keeps `sweep`
    /// from revoking a password an existing link depends on.
    pub fn adopt(&self, origin: &str, principal: &str, login: &str, secret: super::dav::Secret) -> String {
        let handle = uuid::Uuid::new_v4().to_string();
        let mut m = self.by_handle.lock().unwrap();
        make_room(&mut m, principal, chrono::Utc::now());
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
                parked_at: None,
            },
        );
        handle
    }

    /// Marks a credential as belonging to an account from now on: a link stores it sealed, so the
    /// copy in here must not be revoked when its window runs out.
    pub fn claim(&self, handle: &str, principal: &str) {
        let mut m = self.by_handle.lock().unwrap();
        if let Some(p) = m.get_mut(handle).filter(|p| p.principal == principal) {
            p.borrowed = true;
        }
    }

    /// The batch is over: keep the credential for `KEEP_SECS` instead of revoking it now, so the
    /// next import or link from the same server needs no sign-in. [`Logins::expired`] is what
    /// revokes it afterwards.
    pub fn park(&self, handle: &str, principal: &str) {
        let mut m = self.by_handle.lock().unwrap();
        if let Some(p) = m.get_mut(handle).filter(|p| p.principal == principal) {
            p.parked_at = Some(chrono::Utc::now());
        }
    }

    /// A handle of this user's for this address whose credential is already granted, taken back
    /// into use. This is what makes a second import or a first link skip the sign-in.
    pub fn reusable(&self, origin: &str, principal: &str) -> Option<String> {
        let mut m = self.by_handle.lock().unwrap();
        let now = chrono::Utc::now();
        let _ = sweep(&mut m, now);
        let handle = m.iter().find(|(_, p)| p.principal == principal && p.origin == origin && p.got.is_some()).map(|(h, _)| h.clone())?;
        if let Some(p) = m.get_mut(&handle) {
            p.parked_at = None; // in use again: its window starts over when the next batch ends
        }
        Some(handle)
    }

    /// Takes out every parked credential whose window has run out, for the caller to revoke. Called
    /// from the minute tick ([`crate::tasks::spawn_remote_login_reaper`]), because nothing else is
    /// guaranteed to happen: a user who imports once and leaves must not leave a password behind.
    pub fn expired(&self) -> Vec<(String, String, super::dav::Secret)> {
        let mut m = self.by_handle.lock().unwrap();
        sweep(&mut m, chrono::Utc::now())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn pending(origin: &str, parked_ago: Option<i64>, borrowed: bool, got: bool) -> Pending {
        let now = chrono::Utc::now();
        Pending {
            principal: "alice".into(),
            origin: origin.into(),
            token: String::new(),
            endpoint: String::new(),
            at: now,
            got: got.then(|| ("alice".to_string(), super::super::dav::Secret("pw".into()))),
            borrowed,
            parked_at: parked_ago.map(|s| now - chrono::Duration::seconds(s)),
        }
    }

    /// The keep window, and the one credential that is never ours to revoke. A one-off import parks
    /// its password instead of destroying it, so these three rules are the only thing standing
    /// between "one sign-in per migration" and "a password left valid at the user's Nextcloud".
    #[test]
    fn a_parked_credential_is_revoked_once_its_window_ran_out() {
        let now = chrono::Utc::now();
        let mut m = HashMap::new();
        m.insert("fresh".to_string(), pending("https://fresh.example.org", Some(KEEP_SECS - 60), false, true));
        m.insert("stale".to_string(), pending("https://stale.example.org", Some(KEEP_SECS + 60), false, true));
        m.insert("acct".to_string(), pending("https://acct.example.org", Some(KEEP_SECS * 10), true, true));
        m.insert("open".to_string(), pending("https://open.example.org", None, false, false));

        let out = sweep(&mut m, now);
        assert_eq!(out.len(), 1, "exactly the one past its window: {out:?}");
        assert_eq!(out[0].0, "https://stale.example.org");
        assert_eq!(out[0].2.0, "pw", "the password has to come out, or there is nothing to revoke with");

        assert!(m.contains_key("fresh"), "a credential inside its window must stay usable");
        assert!(!m.contains_key("stale"), "a revoked one must not be offered again");
        assert!(!m.contains_key("acct"), "one that belongs to an account is dropped, never revoked");
        assert!(m.contains_key("open"), "a sign-in still waiting for the user is not parked at all");

        // and a second pass has nothing left: a revoke must not be attempted twice
        assert!(sweep(&mut m, now).is_empty());
    }

    /// A new sign-in clears the user's unfinished one — but NOT a parked credential. Dropping that
    /// here would leave its app password valid at the remote with nothing left to revoke it with,
    /// which is worse than the pile-up the clearing is for.
    #[test]
    fn a_new_sign_in_keeps_a_parked_credential() {
        let now = chrono::Utc::now();
        let mut m = HashMap::new();
        m.insert("parked".to_string(), pending("https://a.example.org", Some(60), false, true));
        m.insert("waiting".to_string(), pending("https://b.example.org", None, false, false));
        let mut other = pending("https://c.example.org", None, false, false);
        other.principal = "bob".into();
        m.insert("bobs".to_string(), other);

        make_room(&mut m, "alice", now);
        assert!(m.contains_key("parked"), "a parked credential was dropped without being revoked");
        assert!(!m.contains_key("waiting"), "the user's unfinished sign-in must give way");
        assert!(m.contains_key("bobs"), "another user's login is none of this one's business");
    }

    /// A login the user never finished still ages out, with nothing to revoke: no password was
    /// ever granted for it.
    #[test]
    fn an_unfinished_login_ages_out_silently() {
        let now = chrono::Utc::now();
        let mut m = HashMap::new();
        let mut old = pending("https://x.example.org", None, false, false);
        old.at = now - chrono::Duration::seconds(PENDING_SECS + 60);
        m.insert("old".to_string(), old);
        assert!(sweep(&mut m, now).is_empty(), "there is no password to revoke");
        assert!(m.is_empty(), "it must not sit there for ever");
    }
}
