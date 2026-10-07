//! The HTTP layer's input gate: one place where every request body, query string and path segment is read
//! and checked, and one answer for what does not pass — `AppError` with a message key (localized for a
//! browser session by `ctx::localize_errors`), 400 for a malformed or invalid value (`err-input` names the
//! field), 413 for a body over the route's limit, 415 for a body that is not the expected type. axum's own
//! extractors answered English texts with 400/415/422 mixed, and a settings form then landed on a bare
//! text page.
//!
//! `Json`, `Form`, `Query`, `Path` replace axum's of the same name (same shape: `Json(x): Json<T>`).
//! `Form` on a `/settings/…` page form (not fetch) answers like every settings action: a flash and back to
//! the page. Field validators (`day`, `year`, `text`, `color`, `tz`) are serde `deserialize_with` functions,
//! so the handlers keep their plain types and everything after the gate may use plain arithmetic.

use crate::error::{AppError, Msg};
use crate::state::SharedState;
use axum::body::Bytes;
use axum::extract::{FromRequest, FromRequestParts, Request};
use axum::http::request::Parts;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use chrono::{Datelike, NaiveDate};
use serde::de::{DeserializeOwned, Error as _};
use serde::{Deserialize, Deserializer};

/// The value of cookie `name` (a choice app.js keeps per browser: `tk_back`, `ctview`), if the request has it.
pub fn cookie(headers: &axum::http::HeaderMap, name: &str) -> Option<String> {
    let want = format!("{name}=");
    headers.get_all(header::COOKIE).iter().filter_map(|v| v.to_str().ok()).flat_map(|v| v.split(';')).find_map(|c| c.trim().strip_prefix(want.as_str()).map(str::to_string))
}

/// The longest path segment (a calendar, address book or object id): CalDAV clients' ids are UUIDs or
/// Exchange-style hex ids with a suffix, far below this.
const MAX_SEGMENT: usize = 1024;

fn input(field: &str) -> AppError {
    if field.is_empty() || field == "." {
        return AppError::bad("err-input-body");
    }
    AppError::BadRequest(Msg { key: "err-input", args: vec![("field", field.to_string())] })
}

/// The field of a serde_path_to_error error: its path, or — for a missing field, which serde reports at
/// the struct — the name in "missing field `x`".
fn field_of<E: std::fmt::Display>(path: &serde_path_to_error::Path, inner: &E) -> String {
    let p = path.to_string();
    let msg = inner.to_string();
    match msg.strip_prefix("missing field `").and_then(|r| r.split('`').next()) {
        Some(f) if p == "." => f.to_string(),
        Some(f) => format!("{p}.{f}"),
        None => p,
    }
}

fn rejected(status: StatusCode, key: &'static str) -> AppError {
    AppError::Rejected(status, Msg { key, args: vec![] })
}

/// The body, within the route's `DefaultBodyLimit`.
async fn body<S: Send + Sync>(req: Request, state: &S) -> Result<Bytes, AppError> {
    Bytes::from_request(req, state).await.map_err(|e| match e.status() {
        StatusCode::PAYLOAD_TOO_LARGE => rejected(StatusCode::PAYLOAD_TOO_LARGE, "err-too-large"),
        _ => AppError::bad("err-input-body"),
    })
}

fn content_type_is(headers: &axum::http::HeaderMap, want: &str) -> bool {
    headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).and_then(|v| v.split(';').next()).map(|m| m.trim().to_ascii_lowercase()).is_some_and(|m| {
        m == want || (want == "application/json" && m.ends_with("+json") && m.contains('/'))
    })
}

fn from_json<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, AppError> {
    let mut de = serde_json::Deserializer::from_slice(bytes);
    let v = serde_path_to_error::deserialize(&mut de).map_err(|e| match e.inner().is_syntax() || e.inner().is_eof() {
        true => AppError::bad("err-input-body"),
        false => input(&field_of(e.path(), e.inner())),
    })?;
    // trailing data after the value is malformed too
    de.end().map_err(|_| AppError::bad("err-input-body"))?;
    Ok(v)
}

fn from_urlencoded<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, AppError> {
    let de = serde_urlencoded::Deserializer::new(url::form_urlencoded::parse(bytes));
    serde_path_to_error::deserialize(de).map_err(|e| input(&field_of(e.path(), e.inner())))
}

/// A JSON body (`Content-Type: application/json` or `…+json`). Also the JSON answer, as axum's.
pub struct Json<T>(pub T);

impl<T: DeserializeOwned, S: Send + Sync> FromRequest<S> for Json<T> {
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, AppError> {
        if !content_type_is(req.headers(), "application/json") {
            return Err(rejected(StatusCode::UNSUPPORTED_MEDIA_TYPE, "err-content-type"));
        }
        from_json(&body(req, state).await?).map(Json)
    }
}

impl<T: serde::Serialize> IntoResponse for Json<T> {
    fn into_response(self) -> Response {
        axum::Json(self.0).into_response()
    }
}

/// A form body (`application/x-www-form-urlencoded`). A page form under /settings (no `Accept:
/// application/json`, i.e. not the autosave) that does not pass gets the settings answer: an error flash
/// and back to the page — never a bare text page.
pub struct Form<T>(pub T);

impl<T: DeserializeOwned + Send> FromRequest<SharedState> for Form<T> {
    type Rejection = Response;

    async fn from_request(req: Request, state: &SharedState) -> Result<Self, Response> {
        let settings = req.uri().path().starts_with("/settings/");
        let wants_json = req.headers().get(header::ACCEPT).and_then(|v| v.to_str().ok()).is_some_and(|a| a.contains("application/json"));
        // what the flash needs, before the body consumes the request
        let session = req.extensions().get::<tower_sessions::Session>().cloned();
        let user = req.extensions().get::<rustical_store::auth::Principal>().map(|p| p.id.clone());
        let accept = req.headers().get(header::ACCEPT_LANGUAGE).and_then(|v| v.to_str().ok()).map(str::to_string);
        let r = if content_type_is(req.headers(), "application/x-www-form-urlencoded") {
            match body(req, state).await {
                Ok(bytes) => from_urlencoded(&bytes),
                Err(e) => Err(e),
            }
        } else {
            Err(rejected(StatusCode::UNSUPPORTED_MEDIA_TYPE, "err-content-type"))
        };
        let e = match r {
            Ok(v) => return Ok(Form(v)),
            Err(e) if settings => e,
            Err(e) => return Err(e.into_response()),
        };
        // a settings form answers as its handler would (routes::settings::finish): the autosave reads
        // {ok, msg}, a page form gets an error flash and the page again
        let stored = match &user {
            Some(u) => crate::domain::prefs::get_look(&state.db, u).await.ok().and_then(|(l, _)| l),
            None => None,
        };
        let msg = e.user_message(&state.i18n, &stored.unwrap_or_else(|| crate::i18n::negotiate(accept.as_deref(), &state.config.default_lang)));
        if wants_json {
            return Err(axum::Json(serde_json::json!({ "ok": false, "msg": msg })).into_response());
        }
        match session {
            Some(session) => {
                crate::ctx::set_flash(&session, "error", msg).await;
                Err(Redirect::to("/settings").into_response())
            }
            None => Err(e.into_response()),
        }
    }
}

/// The query string.
pub struct Query<T>(pub T);

impl<T: DeserializeOwned, S: Send + Sync> FromRequestParts<S> for Query<T> {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, AppError> {
        from_urlencoded(parts.uri.query().unwrap_or("").as_bytes()).map(Query)
    }
}

/// The path parameters. Every segment is checked first: at most `MAX_SEGMENT` bytes, no control
/// characters, and no `/` (a `%2F` decodes to one) except in a `{*rest}` parameter.
pub struct Path<T>(pub T);

impl<T: DeserializeOwned + Send, S: Send + Sync> FromRequestParts<S> for Path<T> {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, AppError> {
        let raw = axum::extract::RawPathParams::from_request_parts(parts, state).await.map_err(|_| AppError::bad("err-input-body"))?;
        let wildcard = parts.extensions.get::<axum::extract::MatchedPath>().map(|m| m.as_str().contains("{*")).unwrap_or(false);
        for (k, v) in &raw {
            if v.len() > MAX_SEGMENT || v.chars().any(char::is_control) || (v.contains('/') && !(wildcard && k == "key")) {
                return Err(input(k));
            }
        }
        match axum::extract::Path::<T>::from_request_parts(parts, state).await {
            Ok(axum::extract::Path(v)) => Ok(Path(v)),
            Err(axum::extract::rejection::PathRejection::FailedToDeserializePathParams(e)) => {
                let only = raw.iter().map(|(k, _)| k).collect::<Vec<_>>();
                Err(input(match e.kind() {
                    axum::extract::path::ErrorKind::ParseErrorAtKey { key, .. } | axum::extract::path::ErrorKind::InvalidUtf8InPathParam { key } => key,
                    // a single parameter (`Path<i64>`) is reported without its name
                    _ if only.len() == 1 => only[0],
                    _ => "",
                }))
            }
            Err(_) => Err(AppError::bad("err-input-body")),
        }
    }
}

// ---- field validators (serde `deserialize_with`) ----

/// The years a date may have: chrono's arithmetic is safe far beyond them (±262 000 years), so a view of
/// any such day, ± its weeks and months, never overflows (before: year 262 142 → a panic per request).
pub const YEARS: std::ops::RangeInclusive<i32> = 1..=9999;

fn check_day<E: serde::de::Error>(d: NaiveDate) -> Result<NaiveDate, E> {
    if YEARS.contains(&d.year()) { Ok(d) } else { Err(E::custom("year out of range")) }
}

/// `YYYY-MM-DD` within `YEARS`.
pub fn day<'de, D: Deserializer<'de>>(d: D) -> Result<NaiveDate, D::Error> {
    check_day(NaiveDate::deserialize(d)?)
}

pub fn opt_day<'de, D: Deserializer<'de>>(d: D) -> Result<Option<NaiveDate>, D::Error> {
    Option::<NaiveDate>::deserialize(d)?.map(check_day).transpose()
}

pub fn year<'de, D: Deserializer<'de>>(d: D) -> Result<i32, D::Error> {
    let y = i32::deserialize(d)?;
    if YEARS.contains(&y) { Ok(y) } else { Err(D::Error::custom("year out of range")) }
}

/// A short text (search query, name): at most 200 characters, no control characters but tab/newline.
pub fn text<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let s = String::deserialize(d)?;
    if s.chars().count() > 200 || s.chars().any(|c| c.is_control() && c != '\t' && c != '\n' && c != '\r') {
        return Err(D::Error::custom("text too long or with control characters"));
    }
    Ok(s)
}

/// A collection's colour: `#rrggbb`, `#rrggbbaa` (what DAV clients such as Apple's write, and the edit form
/// sends back unchanged) or empty (= the default); trimmed. Before, any text — CR/LF included — went into
/// the calendar's X-WR-CALCOLOR and its exports.
pub fn color<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let s = String::deserialize(d)?;
    let t = s.trim();
    if t.is_empty() || ((t.len() == 7 || t.len() == 9) && t.starts_with('#') && t[1..].chars().all(|c| c.is_ascii_hexdigit())) { Ok(s) } else { Err(D::Error::custom("not #rrggbb")) }
}

/// An IANA time zone name, or empty (= the default zone).
pub fn tz<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let s = String::deserialize(d)?;
    if s.trim().is_empty() || s.trim().parse::<chrono_tz::Tz>().is_ok() { Ok(s) } else { Err(D::Error::custom("unknown time zone")) }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_cookie_is_found_among_others() {
        let mut h = axum::http::HeaderMap::new();
        h.insert(axum::http::header::COOKIE, "id=abc; xtk_back=day; tk_back=year; x=1".parse().unwrap());
        h.append(axum::http::header::COOKIE, "ctview=table".parse().unwrap());
        assert_eq!(super::cookie(&h, "tk_back").as_deref(), Some("year"));
        assert_eq!(super::cookie(&h, "ctview").as_deref(), Some("table"));
        assert_eq!(super::cookie(&axum::http::HeaderMap::new(), "tk_back"), None);
    }
}
