//! Sharing on a collection's settings page: share (after `GET /api/sharee` named whom), change a share's
//! access, end one or several shares; the owner's visibility levels (what sharees see of private and
//! confidential events). All for the collection's managers (`state::Need::Own`); a sharee removes a share
//! with the collection's own delete form (`collections::delete_calendar`).

use super::*;
use crate::state::{Access, Need, Policy};

/// The shares of the user's own collections as the settings page lists them, by `"cal:<p>/<id>"` /
/// `"ab:<p>/<id>"`.
pub struct ShareRow {
    pub sharee: String,
    pub label: String,
    pub is_group: bool,
    /// the group's current members, only for a viewer who is a member (domain::groups `list_for_user`)
    pub members: Option<Vec<String>>,
    pub write: bool,
}

pub(super) async fn share_rows(state: &SharedState, user: &Principal, cals: &[CalInfo], abs: &[AbInfo]) -> AppResult<HashMap<String, Vec<ShareRow>>> {
    let mut out = HashMap::new();
    let colls = state.collections();
    // the settings list the shares as they are now, not the lists' hints (`CalInfo::has_shares`, up to 30 s old)
    for c in cals.iter().filter(|c| c.shareable()) {
        let shares = colls.calendar_shares(&c.auth).await?;
        let mut rows = vec![];
        for s in shares {
            rows.push(row(state, user, s.sharee, s.access).await?);
        }
        if !rows.is_empty() {
            out.insert(format!("cal:{}/{}", c.principal, c.id), rows);
        }
    }
    for a in abs.iter().filter(|a| a.shareable()) {
        let shares = colls.addressbook_shares(&a.auth).await?;
        let mut rows = vec![];
        for s in shares {
            rows.push(row(state, user, s.sharee, s.access).await?);
        }
        if !rows.is_empty() {
            out.insert(format!("ab:{}/{}", a.principal, a.id), rows);
        }
    }
    Ok(out)
}

async fn row(state: &SharedState, user: &Principal, sharee: String, access: Access) -> AppResult<ShareRow> {
    use rustical_store::auth::{AuthenticationProvider, PrincipalType};
    let p = state.principals().get_principal(&sharee).await?;
    let is_group = p.as_ref().is_some_and(|p| p.principal_type == PrincipalType::Group);
    let members = if is_group && user.is_principal(&sharee) { Some(state.principals().list_members(&sharee).await?) } else { None };
    Ok(ShareRow { label: p.and_then(|p| p.displayname).unwrap_or_else(|| sharee.clone()), sharee, is_group, members, write: access == Access::Write })
}

/// A calendar's visibility levels (settings, its sharing block)
pub struct CalVis {
    pub private: u8,
    pub confidential: u8,
}

/// The visibility levels of each shareable own calendar, by `"<p>/<id>"` (as they are now: the lists' hints may be older)
pub(super) async fn cal_vis(state: &SharedState, cals: &[CalInfo]) -> AppResult<HashMap<String, CalVis>> {
    let mut out = HashMap::new();
    for c in cals.iter().filter(|c| c.shareable()) {
        let p = state.collections().visibility(&c.auth).await?;
        out.insert(format!("{}/{}", c.principal, c.id), CalVis { private: p.private, confidential: p.confidential });
    }
    Ok(out)
}

#[derive(Deserialize)]
pub(super) struct ShareForm {
    pub(super) csrf: String,
    pub(super) sharee: String,
    #[serde(default)]
    pub(super) access: String,
}

fn access_of(s: &str) -> Access {
    if s == "write" { Access::Write } else { Access::Read }
}

async fn ensure(state: &SharedState, user: &Principal, kinds: &str, owner: &str, id: &str) -> AppResult<crate::state::Authorized> {
    match kinds {
        "calendars" => collections::ensure_calendar(state, user, owner, id, Need::Own).await,
        "addressbooks" => collections::ensure_addressbook(state, user, owner, id, Need::Own).await,
        _ => Err(AppError::NotFound),
    }
}

pub(super) async fn share_calendar(state: State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path((owner, id)): Path<(String, String)>, f: Form<ShareForm>) -> Response {
    share(state, user, session, ctx, "calendars", owner, id, f).await
}

pub(super) async fn share_addressbook(state: State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path((owner, id)): Path<(String, String)>, f: Form<ShareForm>) -> Response {
    share(state, user, session, ctx, "addressbooks", owner, id, f).await
}

pub(super) async fn unshare_calendar(state: State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path((owner, id)): Path<(String, String)>, body: String) -> Response {
    unshare(state, user, session, ctx, "calendars", owner, id, body).await
}

pub(super) async fn unshare_addressbook(state: State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path((owner, id)): Path<(String, String)>, body: String) -> Response {
    unshare(state, user, session, ctx, "addressbooks", owner, id, body).await
}

/// Shares with `sharee` (an id, or an e-mail address); sharing again with the same principal changes only the
/// access (the store's upsert).
#[allow(clippy::too_many_arguments)]
async fn share(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, kinds: &str, owner: String, id: String, Form(f): Form<ShareForm>) -> Response {
    form(&state, &session, &ctx, &f.csrf, &format!("{}-{owner}-{id}", if kinds == "calendars" { "cal" } else { "ab" }), async {
        let a = ensure(&state, &user, kinds, &owner, &id).await?;
        let sharee = f.sharee.trim();
        if kinds == "calendars" {
            state.collections().share_calendar(&a, sharee, access_of(&f.access), &user).await?;
        } else {
            state.collections().share_addressbook(&a, sharee, access_of(&f.access), &user).await?;
        }
        Ok(Some("flash-shared"))
    })
    .await
}

/// Ends the shares of every `sharee` field (one or several at once).
#[allow(clippy::too_many_arguments)]
async fn unshare(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, kinds: &str, owner: String, id: String, body: String) -> Response {
    let fields: Vec<(String, String)> = url::form_urlencoded::parse(body.as_bytes()).into_owned().collect();
    let csrf = fields.iter().find(|(k, _)| k == "csrf").map(|(_, v)| v.clone()).unwrap_or_default();
    form(&state, &session, &ctx, &csrf, &format!("{}-{owner}-{id}", if kinds == "calendars" { "cal" } else { "ab" }), async {
        let a = ensure(&state, &user, kinds, &owner, &id).await?;
        let sharees: Vec<&str> = fields.iter().filter(|(k, _)| k == "sharee").map(|(_, v)| v.as_str()).collect();
        if sharees.is_empty() {
            return Err(AppError::bad("err-share-none-picked"));
        }
        for s in sharees {
            let r = if kinds == "calendars" { state.collections().unshare_calendar(&a, s).await } else { state.collections().unshare_addressbook(&a, s).await };
            // already gone (the sharee removed it meanwhile) is what was wanted
            match r {
                Ok(()) | Err(AppError::Store(rustical_store::Error::NotFound)) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(Some("flash-unshared"))
    })
    .await
}

#[derive(Deserialize)]
pub(super) struct VisibilityForm {
    pub(super) csrf: String,
    pub(super) private: u8,
    pub(super) confidential: u8,
}

/// A calendar's levels; confidential never above private (refused, as the store and the table refuse it).
pub(super) async fn calendar_visibility(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, Path((owner, id)): Path<(String, String)>, Form(f): Form<VisibilityForm>) -> Response {
    form(&state, &session, &ctx, &f.csrf, &format!("cal-{owner}-{id}"), async {
        let policy = Policy::new(f.private, f.confidential).ok_or_else(|| AppError::bad("err-visibility"))?;
        let a = collections::ensure_calendar(&state, &user, &owner, &id, Need::Own).await?;
        state.collections().set_visibility(&a, policy).await?;
        Ok(Some("flash-saved"))
    })
    .await
}

#[derive(Deserialize)]
pub(super) struct ShareeQuery {
    q: String,
}

#[derive(serde::Serialize)]
pub(super) struct Sharee {
    id: String,
    name: String,
    group: bool,
    /// only for a member of the group
    members: Option<Vec<String>>,
}

/// Whom a name or e-mail address means, before anything is shared (the confirmation shows it). A group's
/// members only for its members: anyone could otherwise read them by typing `group.<slug>`.
pub(super) async fn sharee(State(state): State<SharedState>, user: Principal, crate::extract::Query(q): crate::extract::Query<ShareeQuery>) -> AppResult<axum::Json<Sharee>> {
    let id = state.collections().resolve_sharee(&q.q).await?;
    let r = row(&state, &user, id, Access::Read).await?;
    Ok(axum::Json(Sharee { id: r.sharee, name: r.label, group: r.is_group, members: r.members }))
}
