//! Calendar and address book forms: create, update, delete to the trash, restore, purge.

use super::*;

#[derive(Deserialize)]
pub(super) struct CalendarForm {
    pub(super) csrf: String,
    pub(super) owner: String,
    #[serde(deserialize_with = "crate::extract::text")]
    pub(super) name: String,
    #[serde(default, deserialize_with = "crate::extract::color")]
    pub(super) color: String,
    #[serde(default)]
    pub(super) description: String,
    #[serde(default)]
    comp_event: Option<String>,
    #[serde(default)]
    comp_todo: Option<String>,
    #[serde(default)]
    comp_journal: Option<String>,
    #[serde(default, deserialize_with = "crate::extract::tz")]
    pub(super) timezone: String,
}

pub(super) async fn calendar_create(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Form(f): Form<CalendarForm>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        let mut comps = vec![];
        if f.comp_event.is_some() { comps.push("VEVENT".to_string()); }
        if f.comp_todo.is_some() { comps.push("VTODO".to_string()); }
        if f.comp_journal.is_some() { comps.push("VJOURNAL".to_string()); }
        collections::create_calendar(
            &state,
            &user,
            NewCalendar {
                owner: f.owner,
                name: f.name,
                color: f.color,
                description: f.description,
                components: collections::parse_components(&comps),
                timezone: if f.timezone.is_empty() { state.config.default_timezone.clone() } else { f.timezone },
            },
        )
        .await?;
        Ok(None)
    }
    .await;
    finish(&state, &session, &ctx, r, "collections").await
}

#[derive(Deserialize)]
pub(super) struct CalendarUpdateForm {
    pub(super) csrf: String,
    #[serde(deserialize_with = "crate::extract::text")]
    pub(super) name: String,
    #[serde(default, deserialize_with = "crate::extract::color")]
    pub(super) color: String,
    #[serde(default)]
    pub(super) description: String,
}

pub(super) async fn calendar_update(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path((owner, id)): Path<(String, String)>, Form(f): Form<CalendarUpdateForm>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        collections::update_calendar(&state, &user, &owner, &id, &f.name, &f.color, &f.description).await?;
        Ok(Some("flash-saved"))
    }
    .await;
    finish(&state, &session, &ctx, r, "collections").await
}

pub(super) async fn calendar_delete(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path((owner, id)): Path<(String, String)>, Form(f): Form<Csrf>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        collections::delete_calendar(&state, &user, &owner, &id, false).await?;
        Ok(None)
    }
    .await;
    finish(&state, &session, &ctx, r, "collections").await
}

pub(super) async fn calendar_restore(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path((owner, id)): Path<(String, String)>, Form(f): Form<Csrf>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        collections::restore_calendar(&state, &user, &owner, &id).await?;
        Ok(None)
    }
    .await;
    finish(&state, &session, &ctx, r, "trash").await
}

pub(super) async fn calendar_purge(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path((owner, id)): Path<(String, String)>, Form(f): Form<Csrf>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        collections::delete_calendar(&state, &user, &owner, &id, true).await?;
        Ok(None)
    }
    .await;
    finish(&state, &session, &ctx, r, "trash").await
}

#[derive(Deserialize)]
pub(super) struct AddressbookForm {
    pub(super) csrf: String,
    pub(super) owner: String,
    #[serde(deserialize_with = "crate::extract::text")]
    pub(super) name: String,
    pub(super) color: String,
    #[serde(default)]
    pub(super) description: String,
}

pub(super) async fn addressbook_create(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Form(f): Form<AddressbookForm>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        collections::create_addressbook(&state, &user, &f.owner, &f.name, &f.color, &f.description).await?;
        Ok(None)
    }
    .await;
    finish(&state, &session, &ctx, r, "collections").await
}

#[derive(Deserialize)]
pub(super) struct AddressbookUpdateForm {
    pub(super) csrf: String,
    #[serde(deserialize_with = "crate::extract::text")]
    pub(super) name: String,
    pub(super) color: String,
    #[serde(default)]
    pub(super) description: String,
}

pub(super) async fn addressbook_update(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path((owner, id)): Path<(String, String)>, Form(f): Form<AddressbookUpdateForm>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        collections::update_addressbook(&state, &user, &owner, &id, &f.name, &f.color, &f.description).await?;
        Ok(Some("flash-saved"))
    }
    .await;
    finish(&state, &session, &ctx, r, "collections").await
}

pub(super) async fn addressbook_delete(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path((owner, id)): Path<(String, String)>, Form(f): Form<Csrf>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        collections::delete_addressbook(&state, &user, &owner, &id, false).await?;
        Ok(None)
    }
    .await;
    finish(&state, &session, &ctx, r, "collections").await
}

pub(super) async fn addressbook_restore(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path((owner, id)): Path<(String, String)>, Form(f): Form<Csrf>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        collections::restore_addressbook(&state, &user, &owner, &id).await?;
        Ok(None)
    }
    .await;
    finish(&state, &session, &ctx, r, "trash").await
}

pub(super) async fn addressbook_purge(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path((owner, id)): Path<(String, String)>, Form(f): Form<Csrf>) -> Response {
    let r = async {
        csrf::check(&session, &f.csrf).await?;
        collections::delete_addressbook(&state, &user, &owner, &id, true).await?;
        Ok(None)
    }
    .await;
    finish(&state, &session, &ctx, r, "trash").await
}
