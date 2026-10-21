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
    Ok(axum::Json(uploader::upload(&state, &user, &device, u).await?))
}
