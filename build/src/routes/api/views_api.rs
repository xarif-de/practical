//! /api/views: the view configurations of settings `#views` ([`crate::domain::views`]) — the user's own, the calendars
//! a rule can name, and what each person who shares calendars with the user offers.

use super::*;
use crate::domain::collections;
use crate::domain::views::{self, Shared, ViewConfig};

#[derive(serde::Serialize)]
pub struct CalRow {
    /// canonical "owner/id" (what a rule names)
    pub key: String,
    pub name: String,
    pub color: String,
    /// the owner's name, for a share
    pub by: Option<String>,
}

#[derive(serde::Serialize)]
pub struct ViewsPage {
    pub config: ViewConfig,
    pub calendars: Vec<CalRow>,
    pub people: Vec<Shared>,
    pub icons: &'static [&'static str],
}

async fn page(state: &SharedState, user: &Principal, config: ViewConfig, cals: Vec<collections::CalInfo>) -> AppResult<ViewsPage> {
    let people = views::shared_with(state, &user.id, &cals).await?;
    let calendars = cals.iter().filter(|c| c.components.is_empty() || c.components.iter().any(|x| x == "VEVENT")).map(|c| CalRow { key: views::canonical(c), name: c.displayname.clone(), color: c.color.clone(), by: c.shared_by.clone() }).collect();
    Ok(ViewsPage { config, calendars, people, icons: views::ICONS })
}

pub(super) async fn views_get(State(state): State<SharedState>, user: Principal) -> AppResult<Json<ViewsPage>> {
    let c = views::load(&state.db, &user.id).await?;
    let cals = collections::calendars_for(&state, &user, false).await?;
    Ok(Json(page(&state, &user, c, cals).await?))
}

/// The whole configuration at once (the page sends it after every change); the choices about people who no longer
/// share calendars with the user are dropped.
pub(super) async fn views_put(State(state): State<SharedState>, user: Principal, _: csrf::TokenOrBasic, Json(c): Json<ViewConfig>) -> AppResult<Json<ViewsPage>> {
    let mut c = c.validated().map_err(AppError::bad)?;
    let cals = collections::calendars_for(&state, &user, false).await?;
    let sharers: std::collections::HashSet<String> = cals.iter().filter(|x| x.view.is_copy() && x.owner != user.id).map(|x| x.owner.clone()).collect();
    for m in [&mut c.year, &mut c.countdown] {
        m.adopt.retain(|k, _| sharers.contains(k));
    }
    c.vacation.people.retain(|k, _| sharers.contains(k));
    views::save(&state.db, &user.id, &c).await?;
    Ok(Json(page(&state, &user, c, cals).await?))
}
