//! App token forms: create (shown once on the next page view) and delete.

use super::*;

pub(super) async fn token_create(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Form(f): Form<NameForm>) -> Response {
    // the new password shows once, on the page that made it ("add a device" › manual)
    form(&state, &session, &ctx, &f.csrf, "manual", async {
        let t = tokens::create(&state, &user, &f.name).await?;
        session.insert(SESSION_NEW_TOKEN, t).await?;
        Ok(None)
    })
    .await
}

pub(super) async fn token_delete(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path(id): Path<String>, Form(f): Form<Csrf>) -> Response {
    form(&state, &session, &ctx, &f.csrf, "account", async {
        tokens::delete(&state, &user, &id).await?;
        Ok(None)
    })
    .await
}

#[derive(Deserialize)]
pub(super) struct UploaderForm {
    pub(super) csrf: String,
    #[serde(deserialize_with = "crate::extract::text")]
    pub(super) name: String,
    pub(super) owner: String,
}

/// "Add a device" › PractiCalUploader: its token and device row (`uploader::setup`); the block shows once on
/// #add-uploader. 404 without `PRACTICAL__UPLOADER`.
pub(super) async fn uploader_create(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Form(f): Form<UploaderForm>) -> Response {
    if !state.config.uploader {
        return AppError::NotFound.into_response();
    }
    form(&state, &session, &ctx, &f.csrf, "add-uploader", async {
        let made = crate::domain::uploader::setup(&state, &user, &f.name, &f.owner).await?;
        session.insert(SESSION_NEW_UPLOADER, made).await?;
        Ok(None)
    })
    .await
}

/// Deletes an uploader device; every field `trash-<calendar id>` sends that calendar to the trash, the others
/// stay read-only without a device (`uploader::delete_device`). Works without the flag too: a device made while
/// it was on must stay removable.
pub(super) async fn uploader_delete(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path(id): Path<String>, Form(f): Form<std::collections::HashMap<String, String>>) -> Response {
    let csrf = f.get("csrf").cloned().unwrap_or_default();
    let trash = f.keys().filter_map(|k| k.strip_prefix("trash-")).map(str::to_string).collect();
    form(&state, &session, &ctx, &csrf, "account", async {
        crate::domain::uploader::delete_device(&state, &user, &id, &trash).await?;
        Ok(None)
    })
    .await
}
