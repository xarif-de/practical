//! Configuration from environment variables `PRACTICAL__<KEY>`.
//! Plain std::env on purpose (no figment/clap): ~15 keys, all strings.
//!
//! [`Config::from_env`] reads and checks them at start. The development switches (`DEV_*`) are listed in the
//! crate docs, section "Development mode"; the user-facing reference is `docs/configuration.md`.

use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    /// TCP bind address, default `[::]:4001`.
    pub bind: String,
    /// RustiCal's SQLite file — shared read-write with the rustical process.
    pub db_path: String,
    /// RustiCal's own HTTP endpoint (health wait + import helpers), default `http://127.0.0.1:4000`.
    pub rustical_url: String,
    /// Public base URL of this UI (cookies, OIDC redirect, logout return), e.g. `https://plan.xarif.de`.
    pub public_url: String,
    /// Public CalDAV/CardDAV base shown to users for client setup, e.g. `https://dav.xarif.de`.
    pub public_dav_url: String,
    pub oidc: Option<OidcSettings>,
    /// Offer RustiCal password login (validate_password against principals.password_hash).
    pub allow_password_login: bool,
    /// Where to send the browser after the local logout so the IdP session ends too.
    /// `{url}` is replaced by the URL-encoded public_url. Empty = local logout only.
    pub logout_url: String,
    /// Days a deleted group stays restorable.
    pub trash_retention_days: u32,
    /// Days a change stays in the change history (and can be undone).
    pub history_retention_days: u32,
    /// PEM (PKCS#8) private key for Web Push; None = push disabled.
    pub vapid_private_key_pem: Option<String>,
    /// `mailto:` or https contact for the VAPID JWT `sub` claim.
    pub vapid_subject: String,
    /// OpenHolidays defaults: ISO country + subdivision code, e.g. DE / DE-SN.
    pub holidays_country: String,
    pub holidays_subdivision: String,
    /// Disable the OpenHolidays fetch entirely (no egress).
    pub holidays_enabled: bool,
    pub default_timezone: String,
    pub default_lang: String,
    /// Session lifetime (inactivity) in hours.
    pub session_hours: i64,
    /// Development only: serve `/assets/*` from this directory on every request (no caching) and mount
    /// `/dev/reload` (SSE) so the browser reloads after a rebuild or an asset edit. Unset in production.
    pub dev_assets_dir: Option<String>,
    /// Development only, requires `dev_assets_dir`: GET /login signs this principal in without a
    /// password, so the in-memory sessions lost on every watchexec restart cost nothing.
    pub dev_login: Option<String>,
    /// Development only, requires `dev_assets_dir`: the fixed "now" (RFC 3339) of every server-side
    /// clock read (clock.rs) — reproducible pages for the e2e suite.
    pub dev_now: Option<chrono::DateTime<chrono::Utc>>,
    /// Development only, requires `dev_assets_dir`: the change poller's period in ms (tasks.rs `change_poll`;
    /// unset = 2 s, as in production) — the e2e suite's live-update tests wait for it.
    pub dev_poll_ms: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct OidcSettings {
    pub name: String,
    pub issuer: String,
    pub client_id: String,
    pub client_secret: Option<String>,
    pub scopes: Vec<String>,
}

fn var(key: &str) -> Option<String> {
    env::var(format!("PRACTICAL__{key}")).ok().filter(|v| !v.is_empty())
}

fn var_or(key: &str, default: &str) -> String {
    var(key).unwrap_or_else(|| default.to_string())
}

fn var_bool(key: &str, default: bool) -> Result<bool, String> {
    match var(key).as_deref() {
        None => Ok(default),
        Some("true" | "1" | "yes") => Ok(true),
        Some("false" | "0" | "no") => Ok(false),
        Some(other) => Err(format!("PRACTICAL__{key}: expected true/false, got {other:?}")),
    }
}

/// `PRACTICAL__DEV_POLL_MS` → the change poller's period; rejected outside development mode, and below 50 ms.
fn parse_dev_poll_ms(raw: Option<&str>, dev: bool) -> Result<Option<u64>, String> {
    let Some(raw) = raw else { return Ok(None) };
    if !dev {
        return Err("PRACTICAL__DEV_POLL_MS is only honoured together with PRACTICAL__DEV_ASSETS_DIR (development)".into());
    }
    raw.parse::<u64>().ok().filter(|ms| *ms >= 50).map(Some).ok_or_else(|| format!("PRACTICAL__DEV_POLL_MS: expected milliseconds ≥ 50, got {raw:?}"))
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let public_url = var_or("PUBLIC_URL", "http://localhost:4001");
        let public_url = public_url.trim_end_matches('/').to_string();
        let oidc = match var("OIDC__ISSUER") {
            Some(issuer) => Some(OidcSettings {
                name: var_or("OIDC__NAME", "SSO"),
                issuer,
                client_id: var("OIDC__CLIENT_ID").ok_or("PRACTICAL__OIDC__CLIENT_ID missing")?,
                client_secret: var("OIDC__CLIENT_SECRET"),
                scopes: var_or("OIDC__SCOPES", "openid profile groups")
                    .split_whitespace()
                    .map(str::to_string)
                    .collect(),
            }),
            None => None,
        };
        let allow_password_login = var_bool("ALLOW_PASSWORD_LOGIN", false)?;
        if oidc.is_none() && !allow_password_login {
            return Err("no login method: set PRACTICAL__OIDC__ISSUER or PRACTICAL__ALLOW_PASSWORD_LOGIN=true".into());
        }
        let trash_retention_days: u32 = var_or("TRASH_RETENTION_DAYS", "90")
            .parse()
            .map_err(|e| format!("PRACTICAL__TRASH_RETENTION_DAYS: {e}"))?;
        let history_retention_days: u32 = var_or("HISTORY_RETENTION_DAYS", "90")
            .parse()
            .map_err(|e| format!("PRACTICAL__HISTORY_RETENTION_DAYS: {e}"))?;
        if var("DEV_LOGIN").is_some() && var("DEV_ASSETS_DIR").is_none() {
            return Err("PRACTICAL__DEV_LOGIN is only honoured together with PRACTICAL__DEV_ASSETS_DIR (development)".into());
        }
        let dev_now = crate::clock::parse_dev_now(var("DEV_NOW").as_deref(), var("DEV_ASSETS_DIR").is_some())?;
        let dev_poll_ms = parse_dev_poll_ms(var("DEV_POLL_MS").as_deref(), var("DEV_ASSETS_DIR").is_some())?;
        let session_hours: i64 = var_or("SESSION_HOURS", "12")
            .parse()
            .map_err(|e| format!("PRACTICAL__SESSION_HOURS: {e}"))?;
        Ok(Self {
            bind: var_or("BIND", "[::]:4001"),
            db_path: var_or("DB_PATH", "/var/lib/rustical/db.sqlite3"),
            rustical_url: env::var("PRACTICAL__RUSTICAL_URL").unwrap_or_else(|_| "http://127.0.0.1:4000".into()).trim_end_matches('/').to_string(),
            public_dav_url: var_or("PUBLIC_DAV_URL", "https://dav.example.org").trim_end_matches('/').to_string(),
            public_url,
            oidc,
            allow_password_login,
            logout_url: var_or("LOGOUT_URL", ""),
            trash_retention_days,
            history_retention_days,
            vapid_private_key_pem: var("VAPID_PRIVATE_KEY_PEM").map(|v| v.replace("\\n", "\n")),
            vapid_subject: var_or("VAPID_SUBJECT", "mailto:admin@example.org"),
            holidays_country: var_or("HOLIDAYS_COUNTRY", "DE"),
            holidays_subdivision: var_or("HOLIDAYS_SUBDIVISION", ""),
            holidays_enabled: var_bool("HOLIDAYS_ENABLED", true)?,
            default_timezone: var_or("DEFAULT_TIMEZONE", "Europe/Berlin"),
            default_lang: var_or("DEFAULT_LANG", "de"),
            session_hours,
            dev_assets_dir: var("DEV_ASSETS_DIR"),
            dev_login: var("DEV_LOGIN"),
            dev_now,
            dev_poll_ms,
        })
    }

    pub fn public_origin(&self) -> String {
        url::Url::parse(&self.public_url)
            .map(|u| u.origin().ascii_serialization())
            .unwrap_or_else(|_| self.public_url.clone())
    }

    pub fn cookie_secure(&self) -> bool {
        self.public_url.starts_with("https://")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_poll_period_is_2_s_unless_development_shortens_it() {
        // production: no variable, or the variable without development mode (an error, not a silent change)
        assert_eq!(parse_dev_poll_ms(None, false), Ok(None));
        assert!(parse_dev_poll_ms(Some("250"), false).unwrap_err().contains("DEV_ASSETS_DIR"));
        assert_eq!(crate::tasks::change_poll(None), std::time::Duration::from_secs(2));
        assert_eq!(crate::tasks::change_poll(crate::testkit::config("unused.sqlite3").dev_poll_ms), std::time::Duration::from_secs(2));
        // development
        assert_eq!(parse_dev_poll_ms(Some("250"), true), Ok(Some(250)));
        assert_eq!(crate::tasks::change_poll(Some(250)), std::time::Duration::from_millis(250));
        assert!(parse_dev_poll_ms(Some("10"), true).is_err() && parse_dev_poll_ms(Some("x"), true).is_err());
    }
}
