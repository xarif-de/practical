//! Groups = RustiCal `group` principals owned by their members.
//!
//! - A group is the RustiCal principal `group.<slug>`. Any member adds or removes members, creates the
//!   group's collections and deletes the group.
//! - Deleting moves it to the trash: a row in `practical_trashed_groups` plus RustiCal's soft delete of its
//!   collections. When the last member leaves, the group goes to the trash too.
//! - [`purge_expired`] (hourly, [`crate::tasks::spawn_trash_purger`]) deletes trashed groups past
//!   `TRASH_RETENTION_DAYS`.
//! - Every run of trash, restore and purge first claims the group's row in the database ([`claim`]): during a
//!   rollout two pods run and both purge; only the holder acts, renews the claim before each step and stops
//!   when it lost it. Restore and purge themselves live in `state` ([`AppState::restore_trashed_group`],
//!   [`AppState::purge_trashed_group`]), which holds the stores. Only a group's OWN collections take part;
//!   collections shared with the group rest while nobody can reach it and are back after a restore.

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use chrono::{DateTime, Utc};
use rustical_store::auth::{AuthenticationProvider, Principal, PrincipalType};
use sqlx::Row;

pub const GROUP_PREFIX: &str = "group.";

#[derive(Debug, Clone, serde::Serialize)]
pub struct GroupInfo {
    pub id: String,
    pub displayname: String,
    pub members: Vec<String>,
    /// the members as the settings list them ([`shown`]), the user among them marked
    #[serde(skip)]
    pub shown: Vec<(Shown, bool)>,
    pub n_calendars: usize,
    pub n_addressbooks: usize,
}

/// A person as others see them in a list (a share line, a group's members): `key` is what the page's forms send
/// back (the server resolves it, `Collections::resolve_sharee`), `label` what is shown. Someone who keeps their
/// name and id out of the suggestions (`prefs::unlisted`) is shown and keyed by the address others found them
/// by, so neither their id nor their name reaches the page; without a known address their id stays (whoever
/// shared with them or added them typed it).
#[derive(Debug, Clone)]
pub struct Shown {
    pub key: String,
    pub label: String,
}

/// [`Shown`] of `id`; `unlisted` = `prefs::unlisted` (read once per page)
pub async fn shown(state: &AppState, unlisted: &std::collections::HashMap<String, (bool, bool)>, id: &str) -> AppResult<Shown> {
    if unlisted.get(id).is_some_and(|(no_id, _)| *no_id)
        && let Some(e) = state.collections().principal_email(id).await?
    {
        return Ok(Shown { key: e.clone(), label: e });
    }
    let label = state.principals().get_principal(id).await?.and_then(|p| p.displayname).filter(|n| !n.trim().is_empty()).unwrap_or_else(|| id.to_string());
    Ok(Shown { key: id.to_string(), label })
}

impl GroupInfo {
    /// An Apple device needs a CalDAV account of its own for this group: it has calendars (`user$group`,
    /// domain::connect `apple_profile`; its address books come with the user's CardDAV account).
    pub fn needs_cal_account(&self) -> bool {
        self.n_calendars > 0
    }
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
    let unlisted = crate::domain::prefs::unlisted(&state.db).await?;
    for gid in user.memberships_without_self() {
        let Some(p) = state.principals().get_principal(gid).await? else { continue };
        if p.principal_type != PrincipalType::Group {
            continue;
        }
        let members = state.principals().list_members(gid).await?;
        let mut rows = vec![];
        for m in &members {
            rows.push((shown(state, &unlisted, m).await?, m == &user.id));
        }
        out.push(GroupInfo {
            id: p.id.clone(),
            displayname: p.displayname.clone().unwrap_or_else(|| p.id.clone()),
            members,
            shown: rows,
            n_calendars: state.collections().group_calendar_count(gid).await?,
            n_addressbooks: state.collections().own_counts(gid).await.1,
        });
    }
    out.sort_by(|a, b| a.displayname.to_lowercase().cmp(&b.displayname.to_lowercase()));
    Ok(out)
}

pub async fn ensure_member(state: &AppState, user: &Principal, gid: &str) -> AppResult<Principal> {
    if !is_group_id(gid) {
        return Err(AppError::NotFound);
    }
    let group = state.principals().get_principal(gid).await?.ok_or(AppError::NotFound)?;
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
    if state.principals().get_principal(&id).await?.is_some() || trashed_row(state, &id).await?.is_some() {
        return Err(AppError::Conflict(crate::error::Msg { key: "err-group-exists", args: vec![("id", id.to_string())] }));
    }
    state
        .principals()
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
    if let Err(e) = state.principals().add_membership(&user.id, &id).await {
        let _ = state.principals().remove_principal(&id).await;
        return Err(e.into());
    }
    Ok(id)
}

/// Adds `member` (an id, or an e-mail address as sharing takes one: `Collections::resolve_sharee`) to the group.
/// The settings field suggests only who lets others find them; nobody else is listed for the picking.
pub async fn add_member(state: &AppState, user: &Principal, gid: &str, member: &str) -> AppResult<()> {
    ensure_member(state, user, gid).await?;
    let member = state.collections().resolve_sharee(member).await?;
    let member = member.as_str();
    let target = state.principals().get_principal(member).await?.ok_or(AppError::NotFound)?;
    if target.principal_type != PrincipalType::Individual {
        return Err(AppError::bad("err-group-individuals"));
    }
    if target.memberships_without_self().contains(&gid) {
        return Ok(());
    }
    state.principals().add_membership(member, gid).await?;
    Ok(())
}

/// `member`: an id or the address it is shown by ([`Shown::key`])
pub async fn remove_member(state: &AppState, user: &Principal, gid: &str, member: &str) -> AppResult<RemoveOutcome> {
    ensure_member(state, user, gid).await?;
    let member = &state.collections().resolve_sharee(member).await.map_err(|_| AppError::NotFound)?;
    let member = member.as_str();
    let members = state.principals().list_members(gid).await?;
    if !members.iter().any(|m| m == member) {
        return Err(AppError::NotFound);
    }
    if members.len() == 1 {
        // last member leaves → the group would be ownerless; it goes to the trash
        // (restorable by that member) instead (plan "Fachliche Regeln").
        trash(state, user, gid, None).await?;
        return Ok(RemoveOutcome::GroupTrashed);
    }
    state.principals().remove_membership(member, gid).await?;
    Ok(RemoveOutcome::Removed)
}

/// How long a claim holds without a renewal (a run stuck that long is taken over)
pub(crate) const CLAIM_LEASE: chrono::Duration = chrono::Duration::minutes(10);

fn now_text() -> String {
    crate::clock::now().to_rfc3339()
}

/// A `claimed_at` that has already run out: frees a claim and keeps its prefix (the next `trash` merges)
fn expired_text() -> String {
    (crate::clock::now() - CLAIM_LEASE - chrono::Duration::seconds(1)).to_rfc3339()
}

fn lease_cutoff() -> String {
    (crate::clock::now() - CLAIM_LEASE).to_rfc3339()
}

/// Renews the claim before the next step. `Err` when another run took it over: stop at once.
pub(crate) async fn renew(state: &AppState, gid: &str, claim: &str) -> AppResult<()> {
    let n = sqlx::query("UPDATE practical_trashed_groups SET claimed_at = ? WHERE id = ? AND claim = ?").bind(now_text()).bind(gid).bind(claim).execute(&state.db).await?.rows_affected();
    if n == 0 {
        tracing::warn!("group {gid}: claim {claim} lost, stopping");
        return Err(AppError::conflict("err-group-busy"));
    }
    Ok(())
}

/// Gives a claim up after a failure: the prefix stays, `claimed_at` is expired (taken over at once).
pub(crate) async fn release_failed(state: &AppState, gid: &str, claim: &str) {
    if let Err(e) = sqlx::query("UPDATE practical_trashed_groups SET claimed_at = ? WHERE id = ? AND claim = ?").bind(expired_text()).bind(gid).bind(claim).execute(&state.db).await {
        tracing::warn!("group {gid}: releasing claim {claim}: {e}");
    }
}

/// Deletes the row the claim holds (last step of restore and purge).
pub(crate) async fn delete_claimed(state: &AppState, gid: &str, claim: &str) -> AppResult<()> {
    renew(state, gid, claim).await?;
    sqlx::query("DELETE FROM practical_trashed_groups WHERE id = ? AND claim = ?").bind(gid).bind(claim).execute(&state.db).await?;
    Ok(())
}

/// What a claim of restore or purge returns: the row's lists
pub(crate) struct Claimed {
    pub claim: String,
    pub members: Vec<String>,
    pub calendars: Vec<String>,
    pub addressbooks: Vec<String>,
}

/// Claims a trashed group's row for `kind` ("restore" / "purge"): only a row nobody holds (or whose claim ran
/// out); `older_than` (the purger) also needs `deleted_at` before it. `Ok(None)`: no such row, or held.
pub(crate) async fn claim(state: &AppState, gid: &str, kind: &str, older_than: Option<DateTime<Utc>>) -> AppResult<Option<Claimed>> {
    let claim = format!("{kind}:{}", uuid::Uuid::new_v4());
    let row = sqlx::query(
        "UPDATE practical_trashed_groups SET claim = ?, claimed_at = ? WHERE id = ? AND (claim IS NULL OR claimed_at < ?) AND (? IS NULL OR deleted_at < ?) RETURNING members_json, calendars_json, addressbooks_json",
    )
    .bind(&claim)
    .bind(now_text())
    .bind(gid)
    .bind(lease_cutoff())
    .bind(older_than.map(|t| t.to_rfc3339()))
    .bind(older_than.map(|t| t.to_rfc3339()))
    .fetch_optional(&state.db)
    .await?;
    Ok(row.map(|r| {
        let j = |k: &str| serde_json::from_str::<Vec<String>>(&r.get::<String, _>(k)).unwrap_or_default();
        Claimed { members: j("members_json"), calendars: j("calendars_json"), addressbooks: j("addressbooks_json"), claim }
    }))
}

/// Whether `actor` may restore or purge a trashed group: a group that still has members (an interrupted
/// trash) belongs to its CURRENT members; one without any to those it had when it was trashed.
pub(crate) async fn may_act(state: &AppState, gid: &str, recorded: &[String], actor: &Principal) -> AppResult<bool> {
    let current = state.principals().list_members(gid).await?;
    Ok(if current.is_empty() { recorded.iter().any(|m| m == &actor.id) } else { current.iter().any(|m| m == &actor.id) })
}

/// Soft-delete: memberships are detached, the group's own collections go to RustiCal's trash, the principal
/// stays until purge. `typed_name` = confirmation input. The row is claimed FIRST, before any effect; a row
/// that is already there is replaced only when it is stale (the group still has members and nobody holds the
/// claim): its collection lists are merged, its members are the current ones.
pub async fn trash(state: &AppState, user: &Principal, gid: &str, typed_name: Option<&str>) -> AppResult<()> {
    let group = ensure_member(state, user, gid).await?;
    let displayname = group.displayname.clone().unwrap_or_else(|| gid.to_string());
    if let Some(t) = typed_name
        && t.trim() != displayname
        && t.trim() != gid
    {
        return Err(AppError::bad("err-group-confirm"));
    }
    let members = state.principals().list_members(gid).await?;
    // the group's own collections only: a collection shared WITH the group is someone else's
    let (cal_ids, ab_ids) = state.collections().own_ids(gid).await?;
    let claim = format!("trash:{}", uuid::Uuid::new_v4());
    let now = now_text();
    {
        let mut tx = state.db.begin_with(rustical_store_sqlite::BEGIN_IMMEDIATE).await?;
        let old = sqlx::query("SELECT calendars_json, addressbooks_json, claim, claimed_at FROM practical_trashed_groups WHERE id = ?").bind(gid).fetch_optional(&mut *tx).await?;
        let (mut cals, mut abs) = (cal_ids.clone(), ab_ids.clone());
        if let Some(old) = old {
            let held = old.get::<Option<String>, _>("claim").is_some() && old.get::<Option<String>, _>("claimed_at").is_some_and(|t| t >= lease_cutoff());
            // `members` is not empty: this user is one, so the row is stale unless a run holds it
            if held {
                return Err(AppError::conflict("err-group-busy"));
            }
            let prefix = old.get::<Option<String>, _>("claim").unwrap_or_default();
            if prefix.starts_with("trash:") || prefix.starts_with("restore:") {
                // collections a failed run already put in the bin are still the group's
                let j = |k: &str| serde_json::from_str::<Vec<String>>(&old.get::<String, _>(k)).unwrap_or_default();
                for id in j("calendars_json") {
                    if !cals.contains(&id) {
                        cals.push(id);
                    }
                }
                for id in j("addressbooks_json") {
                    if !abs.contains(&id) {
                        abs.push(id);
                    }
                }
            }
            sqlx::query("DELETE FROM practical_trashed_groups WHERE id = ?").bind(gid).execute(&mut *tx).await?;
        }
        sqlx::query("INSERT INTO practical_trashed_groups (id, displayname, members_json, calendars_json, addressbooks_json, deleted_by, deleted_at, claim, claimed_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(gid)
            .bind(&displayname)
            .bind(serde_json::to_string(&members).unwrap())
            .bind(serde_json::to_string(&cals).unwrap())
            .bind(serde_json::to_string(&abs).unwrap())
            .bind(&user.id)
            .bind(&now)
            .bind(&claim)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
    }
    match trash_steps(state, gid, &claim, &cal_ids, &ab_ids).await {
        Ok(()) => {
            // fully in the bin; a crash before this line only leaves a claim that runs out
            sqlx::query("UPDATE practical_trashed_groups SET claim = NULL WHERE id = ? AND claim = ?").bind(gid).bind(&claim).execute(&state.db).await?;
            Ok(())
        }
        Err(e) => {
            release_failed(state, gid, &claim).await;
            Err(e)
        }
    }
}

async fn trash_steps(state: &AppState, gid: &str, claim: &str, cal_ids: &[String], ab_ids: &[String]) -> AppResult<()> {
    use crate::domain::remote::{dav::Kind, unlink_collection};
    // the links end here, as with any collection put in the bin; left to `may_write`, whether one
    // survived a quick restore would depend on whether a round ran in between
    for id in cal_ids {
        renew(state, gid, claim).await?;
        let (a, _) = state.collections().own_calendar(gid, id, false).await?;
        state.collections().delete_calendar(&a, false).await?;
        unlink_collection(state, gid, Kind::Cal, id).await;
    }
    for id in ab_ids {
        renew(state, gid, claim).await?;
        let (a, _) = state.collections().own_addressbook(gid, id, false).await?;
        state.collections().delete_addressbook(&a, false).await?;
        unlink_collection(state, gid, Kind::Ab, id).await;
    }
    renew(state, gid, claim).await?;
    // all memberships in one transaction; the row records who they were, so a member removed while this ran
    // has no right to the entry
    let removed = state.principals().clear_memberships(gid).await?;
    sqlx::query("UPDATE practical_trashed_groups SET members_json = ? WHERE id = ? AND claim = ?").bind(serde_json::to_string(&removed).unwrap()).bind(gid).bind(claim).execute(&state.db).await?;
    Ok(())
}

struct TrashRow {
    id: String,
    displayname: String,
    members: Vec<String>,
    deleted_by: String,
    deleted_at: DateTime<Utc>,
}

fn row_to_trash(r: &sqlx::sqlite::SqliteRow) -> TrashRow {
    let j = |k: &str| serde_json::from_str::<Vec<String>>(&r.get::<String, _>(k)).unwrap_or_default();
    TrashRow {
        id: r.get("id"),
        displayname: r.get::<Option<String>, _>("displayname").unwrap_or_default(),
        members: j("members_json"),
        deleted_by: r.get("deleted_by"),
        deleted_at: DateTime::parse_from_rfc3339(&r.get::<String, _>("deleted_at"))
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|_| crate::clock::now()),
    }
}

/// Whether the group has a trash row
pub(crate) async fn exists(state: &AppState, gid: &str) -> AppResult<bool> {
    Ok(sqlx::query("SELECT 1 FROM practical_trashed_groups WHERE id = ?").bind(gid).fetch_optional(&state.db).await?.is_some())
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
    let mut out = vec![];
    for r in rows.iter().map(row_to_trash) {
        // an interrupted trash (the group still has members): its current members see it, nobody else
        if may_act(state, &r.id, &r.members, user).await? {
            out.push(to_public(r, state.config.trash_retention_days));
        }
    }
    Ok(out)
}

pub async fn restore(state: &AppState, user: &Principal, gid: &str) -> AppResult<()> {
    state.restore_trashed_group(gid, user).await
}

/// Final deletion by a former member from the trash page (any time).
pub async fn purge_by_member(state: &AppState, user: &Principal, gid: &str) -> AppResult<()> {
    state.purge_trashed_group(gid, Some(user)).await
}

/// Trashed groups past `TRASH_RETENTION_DAYS`. A failing group is counted and logged; the others go on.
pub async fn purge_expired(state: &AppState) -> AppResult<usize> {
    let cutoff = crate::clock::now() - chrono::Duration::days(i64::from(state.config.trash_retention_days));
    let rows = sqlx::query("SELECT id, displayname, members_json, calendars_json, addressbooks_json, deleted_by, deleted_at FROM practical_trashed_groups").fetch_all(&state.db).await?;
    let (mut n, mut failed) = (0, 0);
    for r in rows.iter().map(row_to_trash) {
        if r.deleted_at < cutoff {
            tracing::info!("purging trashed group {} (deleted {})", r.id, r.deleted_at);
            match state.purge_trashed_group(&r.id, None).await {
                Ok(()) => n += 1,
                Err(e) => {
                    failed += 1;
                    tracing::warn!("purging trashed group {}: {e}", r.id);
                }
            }
        }
    }
    if failed > 0 {
        tracing::warn!("{failed} trashed group(s) could not be purged this round");
    }
    Ok(n)
}

#[cfg(test)]
mod pinned;
#[cfg(test)]
mod trash_tests;

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
