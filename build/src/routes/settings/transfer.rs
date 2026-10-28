//! Export (.ics/.vcf download) and import, into an existing or a new collection.

use super::*;

fn download(name: &str, ext: &str, mime: &'static str, body: String) -> Response {
    let safe: String = name.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    (
        [(axum::http::header::CONTENT_TYPE, mime.to_string()), (axum::http::header::CONTENT_DISPOSITION, crate::routes::attachment(&format!("{safe}.{ext}")))],
        body,
    )
        .into_response()
}

pub(super) async fn export_calendar(State(state): State<SharedState>, user: Principal, Path((owner, id)): Path<(String, String)>) -> AppResult<Response> {
    let (name, ics) = crate::domain::transfer::export_calendar(&state, &user, &owner, &id).await?;
    Ok(download(&name, "ics", "text/calendar; charset=utf-8", ics))
}

pub(super) async fn export_addressbook(State(state): State<SharedState>, user: Principal, Path((owner, id)): Path<(String, String)>) -> AppResult<Response> {
    let (name, vcf) = crate::domain::transfer::export_addressbook(&state, &user, &owner, &id).await?;
    Ok(download(&name, "vcf", "text/vcard; charset=utf-8", vcf))
}

#[derive(serde::Serialize)]
pub(super) struct Imported {
    imported: usize,
    /// objects a share refused (only when there were some: settings.js tells their number)
    #[serde(skip_serializing_if = "is_zero")]
    refused: usize,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// Raw .ics body (app.js reads the chosen file and POSTs it — no multipart dependency).
pub(super) async fn import_calendar(State(state): State<SharedState>, user: Principal, Path((owner, id)): Path<(String, String)>, _: csrf::Token, body: axum::body::Bytes) -> AppResult<axum::Json<Imported>> {
    let mut g = history::begin(&state, Actor::import(&user, "")).await;
    g.coll(Store::Cal, &owner, &id);
    let (imported, refused) = crate::domain::transfer::import_calendar(&state, &user, &owner, &id, &body).await?;
    g.commit().await;
    Ok(axum::Json(Imported { imported, refused }))
}

#[derive(serde::Deserialize)]
pub(super) struct NewImportQ {
    pub(super) owner: String,
    #[serde(deserialize_with = "crate::extract::text")]
    pub(super) name: String,
    #[serde(default, deserialize_with = "crate::extract::color")]
    pub(super) color: String,
    #[serde(default)]
    pub(super) description: String,
    #[serde(default, deserialize_with = "crate::extract::tz")]
    pub(super) timezone: String,
    /// "VEVENT,VTODO" — the file's own types are added on the server
    #[serde(default)]
    pub(super) components: String,
}

#[derive(serde::Serialize)]
pub(super) struct ImportedNew {
    pub(super) id: String,
    pub(super) owner: String,
    imported: usize,
}

/// A new calendar from an .ics file: the metadata in the query (the page prefills it from the file's
/// X-WR-* lines), the raw file as the body.
pub(super) async fn import_new_calendar(State(state): State<SharedState>, user: Principal, crate::extract::Query(q): crate::extract::Query<NewImportQ>, _: csrf::Token, body: axum::body::Bytes) -> AppResult<axum::Json<ImportedNew>> {
    let comps: Vec<String> = q.components.split(',').map(|c| c.trim().to_string()).filter(|c| !c.is_empty()).collect();
    let owner = q.owner.clone();
    let n = NewCalendar {
        owner: q.owner,
        name: q.name,
        color: q.color,
        description: q.description,
        components: if comps.is_empty() { vec![] } else { collections::parse_components(&comps) },
        timezone: if q.timezone.trim().is_empty() { state.config.default_timezone.clone() } else { q.timezone },
    };
    let mut g = history::begin(&state, Actor::import(&user, n.name.trim())).await;
    let (id, imported) = crate::domain::transfer::import_new_calendar(&state, &user, n, &body).await?;
    g.coll(Store::Cal, &owner, &id);
    g.commit().await;
    Ok(axum::Json(ImportedNew { id, owner, imported }))
}

#[derive(serde::Deserialize)]
pub(super) struct NewAbImportQ {
    pub(super) owner: String,
    #[serde(deserialize_with = "crate::extract::text")]
    pub(super) name: String,
    pub(super) color: String,
    #[serde(default)]
    pub(super) description: String,
}

/// A new address book from a .vcf file: the metadata in the query, the raw file as the body.
pub(super) async fn import_new_addressbook(State(state): State<SharedState>, user: Principal, crate::extract::Query(q): crate::extract::Query<NewAbImportQ>, _: csrf::Token, body: axum::body::Bytes) -> AppResult<axum::Json<ImportedNew>> {
    let mut g = history::begin(&state, Actor::import(&user, q.name.trim())).await;
    let (id, imported) = crate::domain::transfer::import_new_addressbook(&state, &user, &q.owner, &q.name, q.color.trim(), &q.description, &body).await?;
    g.coll(Store::Ab, &q.owner, &id);
    g.commit().await;
    Ok(axum::Json(ImportedNew { id, owner: q.owner, imported }))
}

pub(super) async fn import_addressbook(State(state): State<SharedState>, user: Principal, Path((owner, id)): Path<(String, String)>, _: csrf::Token, body: axum::body::Bytes) -> AppResult<axum::Json<Imported>> {
    let mut g = history::begin(&state, Actor::import(&user, "")).await;
    g.coll(Store::Ab, &owner, &id);
    let imported = crate::domain::transfer::import_addressbook(&state, &user, &owner, &id, &body).await?;
    g.commit().await;
    Ok(axum::Json(Imported { imported, refused: 0 }))
}
