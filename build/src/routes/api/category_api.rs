//! /api/categories, /api/titles, /api/colors, /api/category-rules: the category and title index, colour changes,
//! per-user category rules.

use super::*;

#[derive(serde::Serialize)]
pub struct CategoryInfo { pub name: String, pub count: usize, pub rule: bool }

#[derive(serde::Deserialize, Default)]
pub struct ScopeQ {
    /// "event" (the default) or "task" — category rules are kept apart per component
    #[serde(default)]
    pub scope: Option<String>,
}
pub(super) fn scope_of(q: &ScopeQ) -> &str {
    match q.scope.as_deref() {
        Some("task") => "task",
        _ => "event",
    }
}

/// Every category of the visible calendars with its count, plus rule-only categories (count 0).
pub(super) async fn categories(State(state): State<SharedState>, user: Principal, PageView(_, vc): PageView, Query(q): Query<ScopeQ>) -> AppResult<Json<Vec<CategoryInfo>>> {
    let rules = crate::domain::catrules::all(&state.db, &user.id, scope_of(&q)).await?;
    let mut out: Vec<CategoryInfo> = events::category_counts(&state, &user, scope_of(&q), vc.tz).await?.into_iter().map(|(name, count)| CategoryInfo { rule: rules.contains_key(&name), name, count }).collect();
    for name in rules.keys() {
        if !out.iter().any(|c| &c.name == name) {
            out.push(CategoryInfo { name: name.clone(), count: 0, rule: true });
        }
    }
    Ok(Json(out))
}

/// Event titles with their counts: [[title, n], …], most used first.
pub(super) async fn titles(State(state): State<SharedState>, user: Principal) -> AppResult<Json<Vec<(String, usize)>>> {
    Ok(Json(events::title_counts(&state, &user).await?))
}

/// The colours in use, for the colour picker and the settings page "Farben".
pub(super) async fn colors_get(State(state): State<SharedState>, user: Principal) -> AppResult<Json<Vec<crate::domain::colors::Source>>> {
    Ok(Json(crate::domain::colors::sources(&state, &user).await?))
}

#[derive(Deserialize)]
pub(super) struct ColorSet {
    color: String,
}

#[derive(Deserialize)]
pub(super) struct CategoryRecolor {
    scope: String,
    pub(super) name: String,
    from: String,
    to: String,
    /// false: only count
    #[serde(default)]
    apply: bool,
}

#[derive(serde::Serialize)]
pub(super) struct Count {
    pub(super) count: usize,
}

/// A category's colour changed from `from` to `to`: how many events/tasks with that category still carry
/// `from`, and with `apply` give them `to`.
pub(super) async fn color_category(State(state): State<SharedState>, user: Principal, _: csrf::TokenOrBasic, Json(c): Json<CategoryRecolor>) -> AppResult<Json<Count>> {
    use crate::domain::colors;
    let scope = if c.scope == "task" { "task" } else { "event" };
    let keys = colors::category_objects(&state, &user, scope, &c.name, &c.from).await?;
    if c.apply {
        let to = c.to.trim().to_lowercase();
        // all of them are one change in the history
        let mut g = history::begin(&state, Actor { label: Some(format!("colors:{}", c.name)), ..Actor::ui(&user) }).await;
        for k in &keys {
            let [p, cal, id] = k.splitn(3, '/').collect::<Vec<_>>()[..] else { continue };
            g.object(Store::Cal, p, cal, id);
            colors::set_object_color(&state, &user, p, cal, id, &to).await?;
        }
        g.commit().await;
    }
    Ok(Json(Count { count: keys.len() }))
}

#[derive(Deserialize)]
pub(super) struct ColorBatch {
    pub(super) changes: Vec<crate::domain::colors::Change>,
}

#[derive(serde::Serialize)]
pub(super) struct BatchDone {
    pub(super) count: usize,
    failed: usize,
}

/// The colour optimiser's result in one go: every change it lists; a failing one is counted, not fatal.
pub(super) async fn colors_batch(State(state): State<SharedState>, user: Principal, _: csrf::TokenOrBasic, Json(b): Json<ColorBatch>) -> AppResult<Json<BatchDone>> {
    if b.changes.len() > 5000 {
        return Err(AppError::bad("err-too-many-changes"));
    }
    let mut g = history::begin(&state, Actor { label: Some("colors:".into()), ..Actor::ui(&user) }).await;
    for ch in &b.changes {
        if let ("event" | "task", [p, cal, id]) = (ch.kind.as_str(), ch.key.split('/').collect::<Vec<_>>().as_slice()) {
            g.object(Store::Cal, p, cal, id);
        }
    }
    // holiday colours are a read-modify-write of the user's settings (colors::apply)
    let _lock = match b.changes.iter().any(|c| matches!(c.kind.as_str(), "holiday" | "school")) {
        true => Some(state.guards.prefs(&user.id).await),
        false => None,
    };
    let (count, failed) = crate::domain::colors::apply(&state, &user, &b.changes).await?;
    g.commit().await;
    Ok(Json(BatchDone { count, failed }))
}

/// One element's colour: calendar and addressbook `{principal}/{id}`, event and task
/// `{principal}/{calendar}/{object}` ("" = the calendar's again).
pub(super) async fn color_put(State(state): State<SharedState>, user: Principal, Path((kind, key)): Path<(String, String)>, _: csrf::TokenOrBasic, Json(c): Json<ColorSet>) -> AppResult<axum::http::StatusCode> {
    use crate::domain::colors;
    let parts: Vec<&str> = key.split('/').collect();
    let color = c.color.trim().to_lowercase();
    match (kind.as_str(), parts.as_slice()) {
        ("calendar", [p, id]) => colors::set_calendar_color(&state, &user, p, id, &color).await?,
        ("addressbook", [p, id]) => crate::domain::collections::set_addressbook_color_checked(&state, &user, p, id, &color).await?,
        ("event" | "task", [p, cal, id]) => {
            let mut g = history::begin(&state, Actor::ui(&user)).await;
            g.object(Store::Cal, p, cal, id);
            colors::set_object_color(&state, &user, p, cal, id, &color).await?;
            g.commit().await;
        }
        _ => return Err(AppError::bad("err-colour-target")),
    }
    Ok(axum::http::StatusCode::NO_CONTENT)
}

pub(super) async fn category_rules_get(State(state): State<SharedState>, user: Principal, Query(q): Query<ScopeQ>) -> AppResult<Json<std::collections::BTreeMap<String, crate::domain::catrules::Rule>>> {
    Ok(Json(crate::domain::catrules::all(&state.db, &user.id, scope_of(&q)).await?))
}

pub(super) async fn category_rule_put(State(state): State<SharedState>, user: Principal, Path(name): Path<String>, Query(q): Query<ScopeQ>, _: csrf::TokenOrBasic, Json(rule): Json<crate::domain::catrules::Rule>) -> AppResult<Json<crate::domain::catrules::Rule>> {
    crate::domain::catrules::put(&state.db, &user.id, scope_of(&q), &name, &rule).await?;
    Ok(Json(rule))
}

pub(super) async fn category_rule_delete(State(state): State<SharedState>, user: Principal, Path(name): Path<String>, Query(q): Query<ScopeQ>, _: csrf::TokenOrBasic) -> AppResult<Json<Created>> {
    crate::domain::catrules::delete(&state.db, &user.id, scope_of(&q), &name).await?;
    Ok(Json(Created { id: name }))
}
