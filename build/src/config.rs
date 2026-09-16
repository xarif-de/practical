//! Configuration from environment variables `RUSTICAL_UI__<KEY>`.
//! Plain std::env on purpose (no figment/clap): ~15 keys, all strings.

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
    /// Days a deleted group stays restorable (D6).
    pub trash_retention_days: u32,
    /// PEM (PKCS#8) private key for Web Push (D12); None = push disabled.
    pub vapid_private_key_pem: Option<String>,
    /// `mailto:` or https contact for the VAPID JWT `sub` claim.
    pub vapid_subject: String,
    /// OpenHolidays defaults (D13): ISO country + subdivision code, e.g. DE / DE-SN.
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
    env::var(format!("RUSTICAL_UI__{key}")).ok().filter(|v| !v.is_empty())
}

fn var_or(key: &str, default: &str) -> String {
    var(key).unwrap_or_else(|| default.to_string())
}

fn var_bool(key: &str, default: bool) -> Result<bool, String> {
    match var(key).as_deref() {
        None => Ok(default),
        Some("true" | "1" | "yes") => Ok(true),
        Some("false" | "0" | "no") => Ok(false),
        Some(other) => Err(format!("RUSTICAL_UI__{key}: expected true/false, got {other:?}")),
    }
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let public_url = var_or("PUBLIC_URL", "http://localhost:4001");
        let public_url = public_url.trim_end_matches('/').to_string();
        let oidc = match var("OIDC__ISSUER") {
            Some(issuer) => Some(OidcSettings {
                name: var_or("OIDC__NAME", "SSO"),
                issuer,
                client_id: var("OIDC__CLIENT_ID").ok_or("RUSTICAL_UI__OIDC__CLIENT_ID missing")?,
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
            return Err("no login method: set RUSTICAL_UI__OIDC__ISSUER or RUSTICAL_UI__ALLOW_PASSWORD_LOGIN=true".into());
        }
        let trash_retention_days: u32 = var_or("TRASH_RETENTION_DAYS", "90")
            .parse()
            .map_err(|e| format!("RUSTICAL_UI__TRASH_RETENTION_DAYS: {e}"))?;
        if var("DEV_LOGIN").is_some() && var("DEV_ASSETS_DIR").is_none() {
            return Err("RUSTICAL_UI__DEV_LOGIN is only honoured together with RUSTICAL_UI__DEV_ASSETS_DIR (development)".into());
        }
        let session_hours: i64 = var_or("SESSION_HOURS", "12")
            .parse()
            .map_err(|e| format!("RUSTICAL_UI__SESSION_HOURS: {e}"))?;
        Ok(Self {
            bind: var_or("BIND", "[::]:4001"),
            db_path: var_or("DB_PATH", "/var/lib/rustical/db.sqlite3"),
            rustical_url: env::var("RUSTICAL_UI__RUSTICAL_URL").unwrap_or_else(|_| "http://127.0.0.1:4000".into()).trim_end_matches('/').to_string(),
            public_dav_url: var_or("PUBLIC_DAV_URL", "https://dav.example.org").trim_end_matches('/').to_string(),
            public_url,
            oidc,
            allow_password_login,
            logout_url: var_or("LOGOUT_URL", ""),
            trash_retention_days,
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
