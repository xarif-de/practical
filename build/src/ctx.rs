//! Per-request page context for templates: language, user, CSRF token, flash.
//!
//! - [`PageCtx`] is extracted by every page handler. Its language is the user's stored preference, else the
//!   best match of `Accept-Language` ([`crate::i18n::negotiate`]).
//! - [`set_flash`] stores a one-shot message that the next page view shows and removes.
//! - [`localize_errors`] renders an [`AppError`](crate::error::AppError) of a browser session in the page
//!   language.

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
    /// Development mode (assets from disk + `/dev/reload`); adds `data-dev` to `<body>`.
    pub dev: bool,
    /// The request asked for JSON (a form saved by fetch): answer instead of redirecting with a flash.
    pub wants_json: bool,
    /// The user's colour scheme: auto (the system's) | light | dark — rendered into `<html>`, so it
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
    /// A plural message for a data attribute app.js formats (`I18n::plural_json`: its forms, the number as `{n}`).
    pub fn tp(&self, key: &str) -> String {
        self.i18n.plural_json(&self.lang, key)
    }
    pub fn tn<V: std::borrow::Borrow<i64>>(&self, key: &str, name: &str, value: V) -> String {
        self.i18n.t1(&self.lang, key, name, FluentValue::from(*value.borrow()))
    }
    /// The script with app.js' strings (js-*) in the page language (build.rs `i18n_js`; a language
    /// without a file reads English, as `I18n::t` does).
    pub fn i18n_src(&self) -> &'static str {
        crate::routes::assets::url(&format!("i18n-{}.js", self.lang)).or_else(|| crate::routes::assets::url("i18n-en.js")).expect("i18n-en.js")
    }
    /// `/assets/<name>?v=<its own version>` for the `<link>`/`<script>` tags: a change of one file leaves the
    /// other files' URLs (and the browser's immutable copies of them) as they are.
    pub fn asset(&self, name: &str) -> &'static str {
        crate::routes::assets::url(name).expect("an asset")
    }
    /// The icon sprite (`<use href="{icons}#i-…">`, macros.html `ico`).
    pub fn icons(&self) -> &'static str {
        crate::routes::assets::icons()
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
        if s.chars().count() < 2 {
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
            csrf = session.get::<String>(SESSION_CSRF).await.ok().flatten();
            // only a logged-in session gets a token: an anonymous request starts no session (the login
            // form has its own token, auth::LOGIN_CSRF)
            if csrf.is_none() && session.get::<String>(SESSION_USER).await.ok().flatten().is_some() {
                let t = random_token();
                let _ = session.insert(SESSION_CSRF, &t).await;
                csrf = Some(t);
            }
            // removed only when there is one: an unconditional remove marks every page view as a session
            // write; the use of the session is marked instead (sessions::activity extends it)
            if session.get_value(SESSION_FLASH).await.ok().flatten().is_some() {
                flash = session.remove::<Flash>(SESSION_FLASH).await.ok().flatten();
            }
            if let Some(used) = parts.extensions.get::<crate::sessions::Used>() {
                used.mark();
            }
        }
        // the user's stored language, else the browser's (Accept-Language): "automatic" (no stored choice)
        // follows the browser at once. The DB row is read on every request, so nothing else keeps a copy.
        let mut scheme = "auto".to_string();
        if let Some(u) = &user
            && let Ok((l, s)) = crate::domain::prefs::get_look(&state.db, &u.id).await
        {
            lang = l;
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
            dev: state.config.dev_assets_dir.is_some(),
            wants_json,
            scheme,
        })
    }
}

/// Error responses of the UI in the user's language: an `AppError` puts its user message (`error::Msg`) on the
/// response; a request of a logged-in browser session gets that message, in the page language, as the body
/// (the editor and the settings show it after "Fehler 404:"). Clients without a session (app tokens, Basic
/// auth) keep the error's own text.
pub async fn localize_errors(axum::extract::State(state): axum::extract::State<SharedState>, req: axum::extract::Request, next: axum::middleware::Next) -> axum::response::Response {
    let session = req.extensions().get::<Session>().cloned();
    let user = req.extensions().get::<Principal>().map(|u| u.id.clone());
    let accept = req.headers().get(axum::http::header::ACCEPT_LANGUAGE).and_then(|v| v.to_str().ok()).map(str::to_string);
    let res = next.run(req).await;
    let Some(text) = res.extensions().get::<crate::error::Msg>().cloned() else { return res };
    let Some(session) = session else { return res };
    if session.get::<String>(SESSION_USER).await.ok().flatten().is_none() {
        return res;
    }
    let stored = match &user {
        Some(u) => crate::domain::prefs::get_look(&state.db, u).await.ok().and_then(|(l, _)| l),
        None => None,
    };
    let lang = stored.unwrap_or_else(|| negotiate(accept.as_deref(), &state.config.default_lang));
    // the answer's other headers stay (Retry-After of a 503, Cache-Control)
    let (mut parts, _) = res.into_parts();
    parts.headers.remove(axum::http::header::CONTENT_LENGTH);
    axum::response::Response::from_parts(parts, axum::body::Body::from(text.render(&state.i18n, &lang)))
}

pub async fn set_flash(session: &Session, kind: &str, text: String) {
    let _ = session
        .insert(SESSION_FLASH, Flash { kind: kind.to_string(), text })
        .await;
}
