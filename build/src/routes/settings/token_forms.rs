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
