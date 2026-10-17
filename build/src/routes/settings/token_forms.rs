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
    #[serde(deserialize_with = "crate::extract::text")]
    pub(super) calendar: String,
}

/// "Add a device" › PractiCalUploader: its token and read-only calendar (`uploader::setup`); the values show once
/// on #add-uploader. 404 without `PRACTICAL__UPLOADER`.
pub(super) async fn uploader_create(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Form(f): Form<UploaderForm>) -> Response {
    if !state.config.uploader {
        return AppError::NotFound.into_response();
    }
    form(&state, &session, &ctx, &f.csrf, "add-uploader", async {
        let made = crate::domain::uploader::setup(&state, &user, &f.name, &f.calendar).await?;
        session.insert(SESSION_NEW_UPLOADER, made).await?;
        Ok(None)
    })
    .await
}
