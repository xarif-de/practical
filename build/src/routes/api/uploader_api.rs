//! /api/uploader — PractiCalUploader's upload ([`crate::domain::uploader`]); 404 without `PRACTICAL__UPLOADER`.
//! Outside the session-guarded routes ([`super::device_router`]): the device's own secret authenticates it
//! ([`Device`]), never a session or an app token.

use super::*;
use crate::domain::uploader;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};

/// The device of the request and its user ([`uploader::authenticate`]): 404 without the flag, else 401 for any
/// credentials but a device's. Extracted before the body: an unknown caller's 32 MiB are not read.
pub(super) struct Device(Principal, uploader::Device);

impl FromRequestParts<SharedState> for Device {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &SharedState) -> Result<Self, Response> {
        if !state.config.uploader {
            return Err(AppError::NotFound.into_response());
        }
        // only the header counts: a cookie session's principal (RustiCal's AuthenticationLayer adds it) never does
        let basic = parts.headers.get(axum::http::header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Basic "));
        let raw = basic.and_then(|b| { use base64ct::Encoding; base64ct::Base64::decode_vec(b.trim()).ok() }).and_then(|r| String::from_utf8(r).ok()).unwrap_or_default();
        let found = match raw.split_once(':') {
            Some((user, token)) => uploader::authenticate(state, user, token).await,
            None => Ok(None),
        };
        match found {
            Ok(Some((p, d))) => {
                uploader::seen(state, &d).await.map_err(IntoResponse::into_response)?;
                Ok(Device(p, d))
            }
            Ok(None) => Err((axum::http::StatusCode::UNAUTHORIZED, "login required").into_response()),
            Err(e) => Err(e.into_response()),
        }
    }
}

/// `PUT /api/uploader`: the device's complete selection, one snapshot per calendar.
pub(super) async fn upload(State(state): State<SharedState>, Device(user, device): Device, Json(u): Json<uploader::Upload>) -> AppResult<axum::Json<uploader::Uploaded>> {
    Ok(axum::Json(uploader::upload(&state, &user, &device, u).await?))
}

/// `GET /api/uploader`: the device's own view (`uploader::info`), read only — for `practicaluploader status`.
pub(super) async fn info(State(state): State<SharedState>, Device(user, device): Device) -> AppResult<axum::Json<uploader::DeviceInfo>> {
    Ok(axum::Json(uploader::info(&state, &user, &device).await?))
}
