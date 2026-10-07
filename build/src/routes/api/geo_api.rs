//! /api/geo: place search and reverse lookup.

use super::*;

#[derive(Deserialize)]
pub(super) struct GeoSearch {
    pub(super) q: String,
    lat: Option<f64>,
    lon: Option<f64>,
}

#[derive(Deserialize)]
pub(super) struct GeoReverse {
    lat: f64,
    lon: f64,
}

/// Address suggestions (Photon) — the browser asks only after a typing pause.
pub(super) async fn geo_search(State(state): State<SharedState>, user: Principal, ctx: crate::ctx::PageCtx, Query(q): Query<GeoSearch>) -> AppResult<Json<Vec<geo::Place>>> {
    let near = match (q.lat, q.lon) { (Some(a), Some(b)) => Some((a, b)), _ => None };
    let home = crate::domain::prefs::get(&state.db, &user.id).await?.home_country(&state.config);
    Ok(Json(geo::search(&state, &q.q, &ctx.lang, near, &home).await?))
}

/// Address at a position (Nominatim) — one click on the map dialog's button.
pub(super) async fn geo_reverse(State(state): State<SharedState>, user: Principal, ctx: crate::ctx::PageCtx, Query(q): Query<GeoReverse>) -> AppResult<Json<geo::Place>> {
    let home = crate::domain::prefs::get(&state.db, &user.id).await?.home_country(&state.config);
    geo::reverse(&state, q.lat, q.lon, &ctx.lang, &home).await?.map(Json).ok_or(AppError::NotFound)
}
