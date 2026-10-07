//! App token forms: create (shown once on the next page view) and delete.

use super::*;

pub(super) async fn token_create(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Form(f): Form<NameForm>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        let t = tokens::create(&state, &user, &f.name).await?;
        session.insert(SESSION_NEW_TOKEN, t).await?;
        Ok(None)
    }
    .await;
    finish(&state, &session, &ctx, r, "account").await
}

pub(super) async fn token_delete(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path(id): Path<String>, Form(f): Form<Csrf>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        tokens::delete(&state, &user, &id).await?;
        Ok(None)
    }
    .await;
    finish(&state, &session, &ctx, r, "account").await
}
