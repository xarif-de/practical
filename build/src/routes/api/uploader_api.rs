//! /api/uploader — PractiCalUploader's upload ([`crate::domain::uploader`]); 404 without `PRACTICAL__UPLOADER`.

use super::*;
use crate::domain::uploader;

/// `PUT /api/uploader`: the device's complete selection, one snapshot per calendar. HTTP Basic with the
/// device's app token, which also names the device (`uploader::device_for`).
pub(super) async fn upload(State(state): State<SharedState>, user: Principal, headers: HeaderMap, _: csrf::TokenOrBasic, Json(u): Json<uploader::Upload>) -> AppResult<axum::Json<uploader::Uploaded>> {
    if !state.config.uploader {
        return Err(AppError::NotFound);
    }
    let device = uploader::device_for(&state, &user, &headers).await?;
    uploader::seen(&state, &device).await?;
    Ok(axum::Json(uploader::upload(&state, &user, &device, u).await?))
}

/// `GET /api/uploader`: the device's own view (`uploader::info`), read only — for `practicaluploader status`.
/// 403 when the credentials are a user's but not an uploader device's (another app password).
pub(super) async fn info(State(state): State<SharedState>, user: Principal, headers: HeaderMap) -> AppResult<axum::Json<uploader::DeviceInfo>> {
    if !state.config.uploader {
        return Err(AppError::NotFound);
    }
    let device = uploader::device_for(&state, &user, &headers).await.map_err(|e| if matches!(e, AppError::NotFound) { AppError::Forbidden } else { e })?;
    uploader::seen(&state, &device).await?;
    Ok(axum::Json(uploader::info(&state, &device).await?))
}
