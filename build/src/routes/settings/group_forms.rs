//! Group forms: create, add/remove a member, leave, delete to the trash, restore, purge.

use super::*;

#[derive(Deserialize)]
pub(super) struct NameForm {
    pub(super) csrf: String,
    #[serde(deserialize_with = "crate::extract::text")]
    pub(super) name: String,
}

pub(super) async fn group_create(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Form(f): Form<NameForm>) -> Response {
    form(&state, &session, &ctx, &f.csrf, "groups", async {
        groups::create(&state, &user, &f.name).await?;
        Ok(None)
    })
    .await
}

#[derive(Deserialize)]
pub(super) struct MemberForm {
    pub(super) csrf: String,
    pub(super) member: String,
}

pub(super) async fn group_add_member(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path(gid): Path<String>, Form(f): Form<MemberForm>) -> Response {
    form(&state, &session, &ctx, &f.csrf, "groups", async {
        groups::add_member(&state, &user, &gid, &f.member).await?;
        Ok(None)
    })
    .await
}

pub(super) async fn group_remove_member(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path((gid, member)): Path<(String, String)>, Form(f): Form<Csrf>) -> Response {
    form(&state, &session, &ctx, &f.csrf, "groups", async {
        Ok(match groups::remove_member(&state, &user, &gid, &member).await? {
            RemoveOutcome::Removed => None,
            // the last member leaving trashes the group: the one outcome the page does not show
            RemoveOutcome::GroupTrashed => Some("flash-group-trashed"),
        })
    })
    .await
}

pub(super) async fn group_leave(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path(gid): Path<String>, Form(f): Form<Csrf>) -> Response {
    form(&state, &session, &ctx, &f.csrf, "groups", async {
        Ok(match groups::remove_member(&state, &user, &gid, &user.id).await? {
            RemoveOutcome::Removed => None,
            // the last member leaving trashes the group: the one outcome the page does not show
            RemoveOutcome::GroupTrashed => Some("flash-group-trashed"),
        })
    })
    .await
}

#[derive(Deserialize)]
pub(super) struct ConfirmForm {
    pub(super) csrf: String,
    confirm: String,
}

pub(super) async fn group_delete(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path(gid): Path<String>, Form(f): Form<ConfirmForm>) -> Response {
    form(&state, &session, &ctx, &f.csrf, "groups", async {
        groups::trash(&state, &user, &gid, Some(&f.confirm)).await.map_err(|e| match e {
            AppError::BadRequest(_) => AppError::bad("flash-confirm-mismatch"),
            e => e,
        })?;
        Ok(None)
    })
    .await
}

pub(super) async fn group_restore(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path(gid): Path<String>, Form(f): Form<Csrf>) -> Response {
    form(&state, &session, &ctx, &f.csrf, "trash", async {
        groups::restore(&state, &user, &gid).await?;
        Ok(None)
    })
    .await
}

pub(super) async fn group_purge(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path(gid): Path<String>, Form(f): Form<ConfirmForm>) -> Response {
    form(&state, &session, &ctx, &f.csrf, "trash", async {
        if f.confirm.trim() != gid {
            return Err(AppError::bad("flash-confirm-mismatch"));
        }
        groups::purge_by_member(&state, &user, &gid).await?;
        Ok(None)
    })
    .await
}
