//! Groups = RustiCal `group` principals owned by their members.

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use chrono::{DateTime, Utc};
use rustical_store::auth::{AuthenticationProvider, Principal, PrincipalType};
use rustical_store::{AddressbookReadStore, AddressbookWriteStore, CalendarReadStore, CalendarWriteStore};
use sqlx::Row;

pub const GROUP_PREFIX: &str = "group.";

#[derive(Debug, Clone, serde::Serialize)]
pub struct GroupInfo {
    pub id: String,
    pub displayname: String,
    pub members: Vec<String>,
    pub n_calendars: usize,
    pub n_addressbooks: usize,
}

#[derive(Debug, Clone)]
pub struct TrashedGroup {
    pub id: String,
    pub displayname: String,
    pub deleted_by: String,
    pub deleted_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

pub enum RemoveOutcome {
    Removed,
    GroupTrashed,
}

/// `Familie & Freunde` → `familie-freunde`. ASCII only: the id becomes part of
/// CalDAV URLs and RustiCal forbids `:` and `$`.
pub fn slugify(name: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in name.trim().to_lowercase().chars() {
        let mapped: &str = match c {
            'ä' => "ae",
            'ö' => "oe",
            'ü' => "ue",
            'ß' => "ss",
            c if c.is_ascii_alphanumeric() => {
                out.push(c);
                dash = false;
                continue;
            }
            _ => "",
        };
        if mapped.is_empty() {
            if !dash && !out.is_empty() {
                out.push('-');
                dash = true;
            }
        } else {
            out.push_str(mapped);
            dash = false;
        }
    }
    let s = out.trim_end_matches('-').to_string();
    s.chars().take(40).collect()
}

pub fn is_group_id(id: &str) -> bool {
    id.starts_with(GROUP_PREFIX)
}

pub async fn list_for_user(state: &AppState, user: &Principal) -> AppResult<Vec<GroupInfo>> {
    let mut out = vec![];
    for gid in user.memberships_without_self() {
        let Some(p) = state.principals.get_principal(gid).await? else { continue };
        if p.principal_type != PrincipalType::Group {
            continue;
        }
        out.push(GroupInfo {
            id: p.id.clone(),
            displayname: p.displayname.clone().unwrap_or_else(|| p.id.clone()),
            members: state.principals.list_members(gid).await?,
            n_calendars: state.calendars.get_calendars(gid).await?.len(),
            n_addressbooks: state.addressbooks.get_addressbooks(gid).await?.len(),
        });
    }
    out.sort_by(|a, b| a.displayname.to_lowercase().cmp(&b.displayname.to_lowercase()));
    Ok(out)
}

/// Individuals that can be added to a group (everyone with a RustiCal account).
pub async fn individuals(state: &AppState) -> AppResult<Vec<Principal>> {
    let mut all: Vec<Principal> = state
        .principals
        .get_principals()
        .await?
        .into_iter()
        .filter(|p| p.principal_type == PrincipalType::Individual)
        .collect();
    all.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(all)
}

pub async fn ensure_member(state: &AppState, user: &Principal, gid: &str) -> AppResult<Principal> {
    if !is_group_id(gid) {
        return Err(AppError::NotFound);
    }
    let group = state.principals.get_principal(gid).await?.ok_or(AppError::NotFound)?;
    if group.principal_type != PrincipalType::Group {
        return Err(AppError::NotFound);
    }
    if !user.memberships_without_self().contains(&gid) {
        return Err(AppError::Forbidden);
    }
    Ok(group)
}

pub async fn create(state: &AppState, user: &Principal, name: &str) -> AppResult<String> {
    let slug = slugify(name);
    if slug.is_empty() {
        return Err(AppError::bad("err-group-empty-id"));
    }
    let id = format!("{GROUP_PREFIX}{slug}");
    if state.principals.get_principal(&id).await?.is_some() || trashed_row(state, &id).await?.is_some() {
        return Err(AppError::Conflict(crate::error::Msg { key: "err-group-exists", args: vec![("id", id.to_string())] }));
    }
    state
        .principals
        .insert_principal(
            Principal {
                id: id.clone(),
                displayname: Some(name.trim().to_string()),
                principal_type: PrincipalType::Group,
                password: None,
                memberships: vec![],
            },
            false,
        )
        .await?;
    // no transaction across the store trait: compensate if the membership fails
    if let Err(e) = state.principals.add_membership(&user.id, &id).await {
        let _ = state.principals.remove_principal(&id).await;
        return Err(e.into());
    }
    Ok(id)
}

pub async fn add_member(state: &AppState, user: &Principal, gid: &str, member: &str) -> AppResult<()> {
    ensure_member(state, user, gid).await?;
    let target = state.principals.get_principal(member).await?.ok_or(AppError::NotFound)?;
    if target.principal_type != PrincipalType::Individual {
        return Err(AppError::bad("err-group-individuals"));
    }
    if target.memberships_without_self().contains(&gid) {
        return Ok(());
    }
    state.principals.add_membership(member, gid).await?;
    Ok(())
}

pub async fn remove_member(state: &AppState, user: &Principal, gid: &str, member: &str) -> AppResult<RemoveOutcome> {
    ensure_member(state, user, gid).await?;
    let members = state.principals.list_members(gid).await?;
    if !members.iter().any(|m| m == member) {
        return Err(AppError::NotFound);
    }
    if members.len() == 1 {
        // last member leaves → the group would be ownerless; it goes to the trash
        // (restorable by that member) instead (plan "Fachliche Regeln").
        trash(state, user, gid, None).await?;
        return Ok(RemoveOutcome::GroupTrashed);
    }
    state.principals.remove_membership(member, gid).await?;
    Ok(RemoveOutcome::Removed)
}

/// Soft-delete: memberships are detached, collections go to RustiCal's trash,
/// the principal stays until purge. `typed_name` = confirmation input.
pub async fn trash(state: &AppState, user: &Principal, gid: &str, typed_name: Option<&str>) -> AppResult<()> {
    let group = ensure_member(state, user, gid).await?;
    let displayname = group.displayname.clone().unwrap_or_else(|| gid.to_string());
    if let Some(t) = typed_name
        && t.trim() != displayname
        && t.trim() != gid
    {
        return Err(AppError::bad("err-group-confirm"));
    }
    let members = state.principals.list_members(gid).await?;
    let cal_ids: Vec<String> = state.calendars.get_calendars(gid).await?.into_iter().map(|c| c.id).collect();
    let ab_ids: Vec<String> = state.addressbooks.get_addressbooks(gid).await?.into_iter().map(|a| a.id).collect();
    for id in &cal_ids {
        state.calendars.delete_calendar(gid, id, true).await?;
    }
    for id in &ab_ids {
        state.addressbooks.delete_addressbook(gid, id, true).await?;
    }
    for m in &members {
        state.principals.remove_membership(m, gid).await?;
    }
    sqlx::query(
        "INSERT OR REPLACE INTO practical_trashed_groups (id, displayname, members_json, calendars_json, addressbooks_json, deleted_by, deleted_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(gid)
    .bind(&displayname)
    .bind(serde_json::to_string(&members).unwrap())
    .bind(serde_json::to_string(&cal_ids).unwrap())
    .bind(serde_json::to_string(&ab_ids).unwrap())
    .bind(&user.id)
    .bind(crate::clock::now().to_rfc3339())
    .execute(&state.db)
    .await?;
    Ok(())
}

struct TrashRow {
    id: String,
    displayname: String,
    members: Vec<String>,
    calendars: Vec<String>,
    addressbooks: Vec<String>,
    deleted_by: String,
    deleted_at: DateTime<Utc>,
}

fn row_to_trash(r: &sqlx::sqlite::SqliteRow) -> TrashRow {
    let j = |k: &str| serde_json::from_str::<Vec<String>>(&r.get::<String, _>(k)).unwrap_or_default();
    TrashRow {
        id: r.get("id"),
        displayname: r.get::<Option<String>, _>("displayname").unwrap_or_default(),
        members: j("members_json"),
        calendars: j("calendars_json"),
        addressbooks: j("addressbooks_json"),
        deleted_by: r.get("deleted_by"),
        deleted_at: DateTime::parse_from_rfc3339(&r.get::<String, _>("deleted_at"))
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|_| crate::clock::now()),
    }
}

async fn trashed_row(state: &AppState, gid: &str) -> AppResult<Option<TrashRow>> {
    Ok(sqlx::query("SELECT id, displayname, members_json, calendars_json, addressbooks_json, deleted_by, deleted_at FROM practical_trashed_groups WHERE id = ?")
        .bind(gid)
        .fetch_optional(&state.db)
        .await?
        .as_ref()
        .map(row_to_trash))
}

fn to_public(r: TrashRow, retention_days: u32) -> TrashedGroup {
    TrashedGroup {
        expires_at: r.deleted_at + chrono::Duration::days(i64::from(retention_days)),
        id: r.id,
        displayname: r.displayname,
        deleted_by: r.deleted_by,
        deleted_at: r.deleted_at,
    }
}

pub async fn trashed_for_user(state: &AppState, user: &Principal) -> AppResult<Vec<TrashedGroup>> {
    let rows = sqlx::query("SELECT id, displayname, members_json, calendars_json, addressbooks_json, deleted_by, deleted_at FROM practical_trashed_groups ORDER BY deleted_at DESC")
        .fetch_all(&state.db)
        .await?;
    Ok(rows
        .iter()
        .map(row_to_trash)
        .filter(|r| r.members.iter().any(|m| m == &user.id))
        .map(|r| to_public(r, state.config.trash_retention_days))
        .collect())
}

pub async fn restore(state: &AppState, user: &Principal, gid: &str) -> AppResult<()> {
    let row = trashed_row(state, gid).await?.ok_or(AppError::NotFound)?;
    if !row.members.iter().any(|m| m == &user.id) {
        return Err(AppError::Forbidden);
    }
    if state.principals.get_principal(gid).await?.is_none() {
        return Err(AppError::conflict("err-group-gone"));
    }
    for id in &row.calendars {
        match state.calendars.restore_calendar(gid, id).await {
            Ok(()) | Err(rustical_store::Error::NotFound) => {}
            Err(e) => return Err(e.into()),
        }
    }
    for id in &row.addressbooks {
        match state.addressbooks.restore_addressbook(gid, id).await {
            Ok(()) | Err(rustical_store::Error::NotFound) => {}
            Err(e) => return Err(e.into()),
        }
    }
    for m in &row.members {
        if state.principals.get_principal(m).await?.is_some()
            && let Err(e) = state.principals.add_membership(m, gid).await
        {
            tracing::warn!("restore {gid}: membership {m}: {e}");
        }
    }
    sqlx::query("DELETE FROM practical_trashed_groups WHERE id = ?").bind(gid).execute(&state.db).await?;
    Ok(())
}

/// Final deletion (by a former member from the trash page, or by the purger).
pub async fn purge(state: &AppState, gid: &str) -> AppResult<()> {
    let row = trashed_row(state, gid).await?.ok_or(AppError::NotFound)?;
    for id in &row.calendars {
        match state.calendars.delete_calendar(gid, id, false).await {
            Ok(()) | Err(rustical_store::Error::NotFound) => {}
            Err(e) => return Err(e.into()),
        }
    }
    for id in &row.addressbooks {
        match state.addressbooks.delete_addressbook(gid, id, false).await {
            Ok(()) | Err(rustical_store::Error::NotFound) => {}
            Err(e) => return Err(e.into()),
        }
    }
    match state.principals.remove_principal(gid).await {
        Ok(()) | Err(rustical_store::Error::NotFound) => {}
        Err(e) => return Err(e.into()),
    }
    sqlx::query("DELETE FROM practical_trashed_groups WHERE id = ?").bind(gid).execute(&state.db).await?;
    Ok(())
}

pub async fn purge_by_member(state: &AppState, user: &Principal, gid: &str) -> AppResult<()> {
    let row = trashed_row(state, gid).await?.ok_or(AppError::NotFound)?;
    if !row.members.iter().any(|m| m == &user.id) {
        return Err(AppError::Forbidden);
    }
    purge(state, gid).await
}

pub async fn purge_expired(state: &AppState) -> AppResult<usize> {
    let cutoff = crate::clock::now() - chrono::Duration::days(i64::from(state.config.trash_retention_days));
    let rows = sqlx::query("SELECT id, displayname, members_json, calendars_json, addressbooks_json, deleted_by, deleted_at FROM practical_trashed_groups").fetch_all(&state.db).await?;
    let mut n = 0;
    for r in rows.iter().map(row_to_trash) {
        if r.deleted_at < cutoff {
            tracing::info!("purging trashed group {} (deleted {})", r.id, r.deleted_at);
            purge(state, &r.id).await?;
            n += 1;
        }
    }
    Ok(n)
}

#[cfg(test)]
mod pinned;

#[cfg(test)]
mod tests {
    use super::slugify;

    #[test]
    fn slugs() {
        assert_eq!(slugify("Familie & Freunde"), "familie-freunde");
        assert_eq!(slugify("  Müller-Lüdenscheid  "), "mueller-luedenscheid");
        assert_eq!(slugify("$$$"), "");
        assert_eq!(slugify("a:b$c"), "a-b-c");
    }
}
