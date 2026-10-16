//! App tokens: same semantics as RustiCal's frontend — random 64-char
//! secret, pbkdf2-hashed by the store, shown once as `<id-prefix>_<secret>`.

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use chrono::{DateTime, NaiveDateTime, Utc};
use rustical_store::auth::{AppToken, AuthenticationProvider, Principal};

/// A device that has not connected within this long after its token was made is removed ([`sweep`]): set up
/// half-way, or never finished. Asked for on 2026-10-16.
pub const CONNECT_GRACE: chrono::TimeDelta = chrono::TimeDelta::hours(24);

/// How a token shows in the device list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seen {
    /// RustiCal recorded its last use (at most once per hour, so the time is that coarse).
    At(DateTime<Utc>),
    /// Made before RustiCal recorded uses ([`since`]): in use or not, nobody knows.
    Unknown,
    /// Made since then and never used: not (yet) a connected device. Hidden; [`sweep`] deletes it after [`CONNECT_GRACE`].
    Never,
}

pub fn seen(t: &AppToken, since: Option<DateTime<Utc>>) -> Seen {
    match (t.last_used, since, t.created_at) {
        (Some(at), _, _) => Seen::At(at),
        (None, Some(s), Some(c)) if c >= s => Seen::Never,
        _ => Seen::Unknown,
    }
}

/// When RustiCal started to record token uses here (migration 22); `None` on a database without the row.
pub async fn since(state: &AppState) -> AppResult<Option<DateTime<Utc>>> {
    let raw: Option<String> = sqlx::query_scalar("SELECT since FROM practical_token_use_since WHERE id = 1").fetch_optional(&state.db).await?;
    Ok(raw.and_then(|s| NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S").ok()).map(|n| n.and_utc()))
}

/// The user's connected devices: every token except those that never connected, newest use first.
pub async fn devices(state: &AppState, user: &Principal) -> AppResult<Vec<(AppToken, Seen)>> {
    let since = since(state).await?;
    let mut d: Vec<(AppToken, Seen)> = list(state, user).await?.into_iter().map(|t| { let s = seen(&t, since); (t, s) }).filter(|(_, s)| *s != Seen::Never).collect();
    d.sort_by_key(|(t, s)| std::cmp::Reverse(match s { Seen::At(at) => Some(*at), _ => t.created_at }));
    Ok(d)
}

/// Deletes every user's tokens that never connected within [`CONNECT_GRACE`]. Returns how many.
pub async fn sweep(state: &AppState) -> AppResult<usize> {
    let since = since(state).await?;
    let cutoff = Utc::now() - CONNECT_GRACE;
    let mut n = 0;
    for p in state.principals.get_principals().await? {
        for t in state.principals.get_app_tokens(&p.id).await? {
            if seen(&t, since) == Seen::Never && t.created_at.is_some_and(|c| c < cutoff) {
                state.principals.remove_app_token(&p.id, &t.id).await?;
                n += 1;
            }
        }
    }
    Ok(n)
}

pub async fn list(state: &AppState, user: &Principal) -> AppResult<Vec<AppToken>> {
    let mut t = state.principals.get_app_tokens(&user.id).await?;
    t.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(t)
}

pub fn generate_secret() -> String {
    use rand::RngExt;
    use rand::distr::Alphanumeric;
    rand::rng().sample_iter(Alphanumeric).map(char::from).take(64).collect()
}

/// Returns the one-time visible token string.
pub async fn create(state: &AppState, user: &Principal, name: &str) -> AppResult<String> {
    if name.trim().is_empty() {
        return Err(AppError::bad("err-name-required"));
    }
    let secret = generate_secret();
    let mut id = state.principals.add_app_token(&user.id, name.trim().to_string(), secret.clone()).await?;
    id.truncate(4);
    Ok(format!("{id}_{secret}"))
}

/// [`create`] under `name`, or "`name` (2)" and so on when a device of that name is connected already: two rows
/// that look alike could not be told apart when one of them is to go. Returns (the name, the token).
pub async fn create_unique(state: &AppState, user: &Principal, name: &str) -> AppResult<(String, String)> {
    let taken: Vec<String> = list(state, user).await?.into_iter().map(|t| t.name).collect();
    let name = (1..).map(|n| if n == 1 { name.trim().to_string() } else { format!("{} ({n})", name.trim()) }).find(|n| !taken.contains(n)).unwrap_or_default();
    let token = create(state, user, &name).await?;
    Ok((name, token))
}

pub async fn delete(state: &AppState, user: &Principal, id: &str) -> AppResult<()> {
    // the store scopes by (principal, id); verify ownership explicitly anyway
    if !state.principals.get_app_tokens(&user.id).await?.iter().any(|t| t.id == id) {
        return Err(AppError::NotFound);
    }
    state.principals.remove_app_token(&user.id, id).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn tok(created: Option<DateTime<Utc>>, used: Option<DateTime<Utc>>) -> AppToken {
        AppToken { id: "i".into(), name: "n".into(), token: String::new().into(), created_at: created, last_used: used }
    }

    #[test]
    fn seen_classifies_by_use_and_by_when_recording_began() {
        let at = |h: u32| Utc.with_ymd_and_hms(2026, 10, 16, h, 0, 0).unwrap();
        let since = Some(at(12));
        assert_eq!(seen(&tok(Some(at(13)), Some(at(14))), since), Seen::At(at(14)));
        assert_eq!(seen(&tok(Some(at(13)), None), since), Seen::Never, "made after recording began, never used");
        assert_eq!(seen(&tok(Some(at(12)), None), since), Seen::Never, "made at the moment recording began");
        assert_eq!(seen(&tok(Some(at(11)), None), since), Seen::Unknown, "made before: nobody knows");
        assert_eq!(seen(&tok(None, None), since), Seen::Unknown);
        assert_eq!(seen(&tok(Some(at(13)), None), None), Seen::Unknown, "no recording start: never hide");
    }
}
