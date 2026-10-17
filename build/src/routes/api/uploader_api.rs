//! /api/uploader — PractiCalUploader's snapshot upload ([`crate::domain::uploader`]); 404 without `PRACTICAL__UPLOADER`.

use super::*;
use crate::domain::uploader;

/// `PUT /api/uploader/calendars/{id}`: the device's whole snapshot (HTTP Basic with its app token).
pub(super) async fn upload(State(state): State<SharedState>, user: Principal, Path(cal_id): Path<String>, _: csrf::TokenOrBasic, Json(u): Json<uploader::Upload>) -> AppResult<axum::Json<uploader::Counts>> {
    if !state.config.uploader {
        return Err(AppError::NotFound);
    }
    Ok(axum::Json(uploader::upload(&state, &user, &cal_id, u).await?))
}
