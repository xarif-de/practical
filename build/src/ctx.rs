//! Per-request page context for templates: language, user, CSRF token, flash.

use crate::i18n::negotiate;
use crate::state::SharedState;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use fluent_bundle::FluentValue;
use rustical_store::auth::Principal;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::sync::Arc;
use tower_sessions::Session;

pub const SESSION_LANG: &str = "lang";
pub const SESSION_CSRF: &str = "csrf";
pub const SESSION_FLASH: &str = "flash";
pub const SESSION_USER: &str = "user";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Flash {
    pub kind: String, // "ok" | "error"
    pub text: String,
}

#[derive(Clone)]
pub struct PageCtx {
    pub lang: String,
    pub user: Option<Principal>,
    pub csrf: String,
    pub path: String,
    pub flash: Option<Flash>,
    pub i18n: Arc<crate::i18n::I18n>,
    pub public_dav_url: String,
    pub push_enabled: bool,
    pub asset_v: &'static str,
    /// Development mode (assets from disk + `/dev/reload`); adds `data-dev` to <body>.
    pub dev: bool,
    /// The request asked for JSON (a form saved by fetch): answer instead of redirecting with a flash.
    pub wants_json: bool,
    /// The user's colour scheme: auto (the system's) | light | dark — rendered into <html>, so it
    /// holds on every device and nothing flickers
    pub scheme: String,
}

impl PageCtx {
    /// AGPL §13: every user of this network service is offered its source.
    pub fn source_url(&self) -> &'static str {
        env!("CARGO_PKG_REPOSITORY")
    }
    pub fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }
    /// the RustiCal release this build embeds (crates and the `rustical` binary)
    pub fn rustical_version(&self) -> &'static str {
        env!("RUSTICAL_TAG")
    }
    pub fn rustical_source_url(&self) -> String {
        format!("https://github.com/lennart-k/rustical/tree/v{}", env!("RUSTICAL_TAG"))
    }
    pub fn t(&self, key: &str) -> String {
        self.i18n.t(&self.lang, key)
    }
    pub fn t1(&self, key: &str, name: &str, value: &str) -> String {
        self.i18n.t1(&self.lang, key, name, FluentValue::from(value.to_string()))
    }
    pub fn tn<V: std::borrow::Borrow<i64>>(&self, key: &str, name: &str, value: V) -> String {
        self.i18n.t1(&self.lang, key, name, FluentValue::from(*value.borrow()))
    }
    /// The js-* strings of the page language as JSON for app.js (`<` escaped for the <script> block).
    pub fn js_i18n(&self) -> String {
        let m: serde_json::Map<String, serde_json::Value> = self.i18n.with_prefix(&self.lang, "js-").into_iter().map(|(k, v)| (k, serde_json::Value::String(v))).collect();
        serde_json::Value::Object(m).to_string().replace('<', "\\u003c")
    }
    pub fn locale(&self) -> &'static str {
        crate::i18n::locale(&self.lang)
    }
    /// (code, native name) of every UI language, for the language choice
    pub fn languages(&self) -> &'static [(&'static str, &'static str)] {
        crate::i18n::NATIVE
    }
    pub fn user_id(&self) -> &str {
        self.user.as_ref().map(|u| u.id.as_str()).unwrap_or("")
    }
    pub fn initials(&self) -> String {
        let name = self
            .user
            .as_ref()
            .map(|u| u.displayname.clone().unwrap_or_else(|| u.id.clone()))
            .unwrap_or_default();
        let mut s: String = name
            .split(|c: char| c == ' ' || c == '.' || c == '-')
            .filter_map(|p| p.chars().next())
            .take(2)
            .collect();
        if s.len() < 2 {
            s = name.chars().take(2).collect();
        }
        s.to_uppercase()
    }
    pub fn is_active(&self, prefix: &str) -> bool {
        self.path == prefix || self.path.starts_with(&format!("{prefix}/"))
    }
}

pub fn random_token() -> String {
    use rand::RngExt;
    let mut rng = rand::rng();
    let bytes: [u8; 24] = rng.random();
    use base64ct::Encoding;
    base64ct::Base64UrlUnpadded::encode_string(&bytes)
}

impl FromRequestParts<SharedState> for PageCtx {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, state: &SharedState) -> Result<Self, Infallible> {
        let session = parts.extensions.get::<Session>().cloned();
        let wants_json = parts.headers.get(axum::http::header::ACCEPT).and_then(|v| v.to_str().ok()).is_some_and(|v| v.contains("application/json"));
        let user = parts.extensions.get::<Principal>().cloned();
        let accept = parts
            .headers
            .get(axum::http::header::ACCEPT_LANGUAGE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let mut lang = None;
        let mut csrf = None;
        let mut flash = None;
        if let Some(session) = &session {
            lang = session.get::<String>(SESSION_LANG).await.ok().flatten();
            csrf = session.get::<String>(SESSION_CSRF).await.ok().flatten();
            if csrf.is_none() {
                let t = random_token();
                let _ = session.insert(SESSION_CSRF, &t).await;
                csrf = Some(t);
            }
            flash = session.remove::<Flash>(SESSION_FLASH).await.ok().flatten();
        }
        // user preference (DB) beats the session choice which beats Accept-Language
        let mut scheme = "auto".to_string();
        if let Some(u) = &user
            && let Ok((l, s)) = crate::domain::prefs::get_look(&state.db, &u.id).await
        {
            if lang.is_none() {
                lang = l;
            }
            if let Some(s) = s.filter(|s| s == "light" || s == "dark") {
                scheme = s;
            }
        }
        let lang = lang.unwrap_or_else(|| negotiate(accept.as_deref(), &state.config.default_lang));
        Ok(Self {
            lang,
            user,
            csrf: csrf.unwrap_or_default(),
            path: parts.uri.path().to_string(),
            flash,
            i18n: state.i18n.clone(),
            public_dav_url: state.config.public_dav_url.clone(),
            push_enabled: state.vapid.is_some(),
            asset_v: crate::routes::assets::asset_version(),
            dev: state.config.dev_assets_dir.is_some(),
            wants_json,
            scheme,
        })
    }
}

pub async fn set_flash(session: &Session, kind: &str, text: String) {
    let _ = session
        .insert(SESSION_FLASH, Flash { kind: kind.to_string(), text })
        .await;
}
