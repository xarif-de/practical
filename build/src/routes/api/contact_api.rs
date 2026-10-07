//! /api/contacts: read (JSON, .vcf), create, update, delete.

use super::*;

pub(super) async fn get_contact(State(state): State<SharedState>, user: Principal, Path((p, a, id)): Path<(String, String, String)>) -> AppResult<Json<Contact>> {
    Ok(Json(contacts::get_contact(&state, &user, &p, &a, &id).await?))
}

pub(super) async fn contact_vcf(State(state): State<SharedState>, user: Principal, Path((p, a, id)): Path<(String, String, String)>) -> AppResult<axum::response::Response> {
    use axum::response::IntoResponse;
    let vcf = contacts::get_vcf(&state, &user, &p, &a, &id).await?;
    Ok(([(axum::http::header::CONTENT_TYPE, "text/vcard; charset=utf-8".to_string()), (axum::http::header::CONTENT_DISPOSITION, format!("attachment; filename=\"{id}.vcf\""))], vcf).into_response())
}

pub(super) async fn create_contact(State(state): State<SharedState>, user: Principal, _: csrf::TokenOrBasic, Json(input): Json<ContactInput>) -> AppResult<Json<Created>> {
    let (p, a) = (input.principal.clone(), input.ab_id.clone());
    let mut g = history::begin(&state, Actor::ui(&user)).await;
    let id = contacts::create_contact(&state, &user, input).await?;
    g.object(Store::Ab, &p, &a, &id);
    g.commit().await;
    Ok(Json(Created { id }))
}

#[derive(Deserialize)]
pub(super) struct ContactUpdate {
    #[serde(flatten)]
    pub(super) input: ContactInput,
    #[serde(default)]
    pub(super) etag: Option<String>,
}

pub(super) async fn update_contact(State(state): State<SharedState>, user: Principal, headers: HeaderMap, Path((p, a, id)): Path<(String, String, String)>, _: csrf::TokenOrBasic, Json(u): Json<ContactUpdate>) -> AppResult<Json<Created>> {
    let etag = precondition(u.etag.as_deref(), &headers)?;
    let mut g = history::begin(&state, Actor::ui(&user)).await;
    g.object(Store::Ab, &p, &a, &id);
    if u.input.principal != p || u.input.ab_id != a {
        crate::domain::collections::ensure_owner(&user, &p)?;
        let (tp, ta) = (u.input.principal.clone(), u.input.ab_id.clone());
        let (old, old_etag) = contacts::stored_card(&state, &p, &a, &id).await?;
        if old_etag != etag {
            return Err(AppError::conflict("err-changed-contact"));
        }
        let uid = contacts::parse_vcf(&old)?.get_uid().unwrap_or(&id).to_string();
        let new_id = contacts::create_contact_as(&state, &user, u.input, Some(&uid)).await?;
        g.object(Store::Ab, &tp, &ta, &new_id);
        crate::domain::collections::delete_object(&state, &user, Store::Ab, &p, &a, &id).await?;
        g.commit().await;
        return Ok(Json(Created { id: new_id }));
    }
    contacts::update_contact(&state, &user, &id, u.input, Some(&etag)).await?;
    g.commit().await;
    Ok(Json(Created { id }))
}

pub(super) async fn delete_contact(State(state): State<SharedState>, user: Principal, Path((p, a, id)): Path<(String, String, String)>, _: csrf::TokenOrBasic) -> AppResult<Json<Created>> {
    delete_object(&state, &user, Store::Ab, &p, &a, id).await
}
