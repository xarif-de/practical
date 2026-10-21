//! Calendars and address books for the user and their groups.
//!
//! The collections a user sees are their own and those of their groups ([`owners`]). Create, update,
//! delete to the trash, restore and purge go through RustiCal's store; [`delete_object`] deletes one object.
//! Address-book colours are PractiCal's own (`practical_addressbook_colors`).

use crate::domain::history_text::Store;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use chrono::NaiveDateTime;
use rustical_ical::CalendarObjectType;
use rustical_store::auth::{AuthenticationProvider, Principal};
use rustical_store::{Addressbook, AddressbookReadStore, AddressbookWriteStore, Calendar, CalendarMetadata, CalendarReadStore, CalendarWriteStore};

#[derive(Debug, Clone, serde::Serialize)]
pub struct Owner {
    pub id: String,
    pub label: String,
    pub is_group: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CalInfo {
    pub principal: String,
    pub owner_label: String,
    pub is_group: bool,
    pub id: String,
    pub displayname: String,
    pub color: String,
    pub description: String,
    pub components: Vec<String>,
    pub timezone_id: String,
    pub subscription_url: Option<String>,
    pub deleted_at: Option<NaiveDateTime>,
    pub synctoken: i64,
    /// PractiCal writes nothing into it ([`is_read_only`]): a subscription, or a calendar of `practical_readonly_calendars`
    pub read_only: bool,
    /// a live PractiCalUploader device manages it (`uploader::managed`): it cannot be deleted here
    pub managed: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AbInfo {
    pub principal: String,
    pub owner_label: String,
    pub is_group: bool,
    pub id: String,
    pub displayname: String,
    /// practical_addressbook_colors, else default_color(id)
    pub color: String,
    pub description: String,
    pub deleted_at: Option<NaiveDateTime>,
    pub synctoken: i64,
}

pub const PALETTE: &[&str] = &["#0a84ff", "#30b46c", "#ff3b30", "#ff9f0a", "#af52de", "#32ade6", "#ff2d55", "#5856d6", "#a2845e", "#8e8e93"];

/// Stable fallback colour for a calendar without one (RustiCal allows NULL).
pub fn default_color(id: &str) -> String {
    let h = id.bytes().fold(0usize, |a, b| a.wrapping_mul(31).wrapping_add(usize::from(b)));
    PALETTE[h % PALETTE.len()].to_string()
}

pub async fn owners(state: &AppState, user: &Principal) -> AppResult<Vec<Owner>> {
    let mut out = vec![Owner {
        id: user.id.clone(),
        label: user.displayname.clone().unwrap_or_else(|| user.id.clone()),
        is_group: false,
    }];
    for gid in user.memberships_without_self() {
        if let Some(g) = state.principals.get_principal(gid).await? {
            out.push(Owner { id: g.id.clone(), label: g.displayname.clone().unwrap_or(g.id), is_group: true });
        }
    }
    Ok(out)
}

pub fn ensure_owner(user: &Principal, principal: &str) -> AppResult<()> {
    if user.is_principal(principal) { Ok(()) } else { Err(AppError::Forbidden) }
}

fn cal_info(c: Calendar, owner: &Owner) -> CalInfo {
    let fallback = default_color(&c.id);
    CalInfo {
        principal: c.principal,
        owner_label: owner.label.clone(),
        is_group: owner.is_group,
        id: c.id,
        displayname: c.meta.displayname.unwrap_or_default(),
        // any CalDAV client sets this property: only #rrggbb may reach a style attribute
        color: c.meta.color.filter(|c| valid_color(c)).unwrap_or(fallback),
        description: c.meta.description.unwrap_or_default(),
        components: c.components.iter().map(|c| c.as_str().to_string()).collect(),
        timezone_id: c.timezone_id.unwrap_or_default(),
        read_only: c.subscription_url.is_some(),
        managed: false,
        subscription_url: c.subscription_url,
        deleted_at: c.deleted_at,
        synctoken: c.synctoken,
    }
}

/// The calendars of `practical_readonly_calendars` (all users: a handful of rows).
async fn read_only_rows(state: &AppState) -> AppResult<std::collections::HashSet<(String, String)>> {
    use sqlx::Row;
    Ok(sqlx::query("SELECT principal, calendar_id FROM practical_readonly_calendars").fetch_all(&state.db).await?.into_iter().map(|r| (r.get("principal"), r.get("calendar_id"))).collect())
}

/// The one read-only predicate: a subscribed calendar (the next refresh would overwrite a change) or one
/// PractiCal keeps read-only (`practical_readonly_calendars`, e.g. filled by PractiCalUploader, `domain::uploader`). Every PractiCal
/// write into a calendar asks [`ensure_writable`]; CalDAV clients are not covered (RustiCal grants them write).
pub async fn is_read_only(state: &AppState, principal: &str, cal_id: &str) -> AppResult<bool> {
    let row: Option<i64> = sqlx::query_scalar("SELECT 1 FROM practical_readonly_calendars WHERE principal = ? AND calendar_id = ?").bind(principal).bind(cal_id).fetch_optional(&state.db).await?;
    if row.is_some() {
        return Ok(true);
    }
    Ok(match state.calendars.get_calendar(principal, cal_id, true).await {
        Ok(c) => c.subscription_url.is_some(),
        Err(rustical_store::Error::NotFound) => false, // the write that follows answers for a missing calendar
        Err(e) => return Err(e.into()),
    })
}

/// `Err` for a write into a read-only calendar ([`is_read_only`]): `err-subscribed-read-only` for a
/// subscription (the message it always had), else `err-calendar-read-only`.
pub async fn ensure_writable(state: &AppState, principal: &str, cal_id: &str) -> AppResult<()> {
    if !is_read_only(state, principal, cal_id).await? {
        return Ok(());
    }
    let subscribed = state.calendars.get_calendar(principal, cal_id, true).await.is_ok_and(|c| c.subscription_url.is_some());
    Err(AppError::bad(if subscribed { "err-subscribed-read-only" } else { "err-calendar-read-only" }))
}

/// Marks a calendar read-only for PractiCal (`source`: who fills it). Tests only: the uploader writes its rows
/// with key and device (`uploader::upload`).
#[cfg(test)]
pub async fn set_read_only(state: &AppState, principal: &str, cal_id: &str, source: &str) -> AppResult<()> {
    sqlx::query("INSERT INTO practical_readonly_calendars (principal, calendar_id, source) VALUES (?, ?, ?) ON CONFLICT(principal, calendar_id) DO UPDATE SET source = excluded.source")
        .bind(principal)
        .bind(cal_id)
        .bind(source)
        .execute(&state.db)
        .await?;
    Ok(())
}

fn ab_info(a: Addressbook, owner: &Owner, color: Option<String>) -> AbInfo {
    AbInfo {
        color: color.filter(|c| valid_color(c)).unwrap_or_else(|| default_color(&a.id)), // as cal_info: a style attribute takes it
        principal: a.principal,
        owner_label: owner.label.clone(),
        is_group: owner.is_group,
        id: a.id,
        displayname: a.displayname.unwrap_or_default(),
        description: a.description.unwrap_or_default(),
        deleted_at: a.deleted_at,
        synctoken: a.synctoken,
    }
}

pub async fn calendars_for(state: &AppState, user: &Principal, deleted: bool) -> AppResult<Vec<CalInfo>> {
    let ro = read_only_rows(state).await?;
    let managed = crate::domain::uploader::managed(state).await?;
    let mut out = vec![];
    for o in owners(state, user).await? {
        let cals = if deleted { state.calendars.get_deleted_calendars(&o.id).await? } else { state.calendars.get_calendars(&o.id).await? };
        out.extend(cals.into_iter().map(|c| cal_info(c, &o)).map(|mut c| {
            let key = (c.principal.clone(), c.id.clone());
            c.read_only |= ro.contains(&key);
            c.managed = managed.contains(&key);
            c
        }));
    }
    Ok(out)
}

pub async fn addressbooks_for(state: &AppState, user: &Principal, deleted: bool) -> AppResult<Vec<AbInfo>> {
    use sqlx::Row;
    let colors: std::collections::HashMap<(String, String), String> = sqlx::query("SELECT principal, id, color FROM practical_addressbook_colors")
        .fetch_all(&state.db)
        .await?
        .into_iter()
        .map(|r| ((r.get("principal"), r.get("id")), r.get("color")))
        .collect();
    let mut out = vec![];
    for o in owners(state, user).await? {
        let abs = if deleted { state.addressbooks.get_deleted_addressbooks(&o.id).await? } else { state.addressbooks.get_addressbooks(&o.id).await? };
        out.extend(abs.into_iter().map(|a| {
            let c = colors.get(&(a.principal.clone(), a.id.clone())).cloned();
            ab_info(a, &o, c)
        }));
    }
    Ok(out)
}

/// `#rrggbb` only — the value lands in a style attribute.
pub fn valid_color(c: &str) -> bool {
    c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|ch| ch.is_ascii_hexdigit())
}

async fn set_addressbook_color(state: &AppState, owner: &str, id: &str, color: &str) -> AppResult<()> {
    if !valid_color(color) {
        return Err(AppError::bad("err-color"));
    }
    sqlx::query("INSERT INTO practical_addressbook_colors (principal, id, color) VALUES (?, ?, ?) ON CONFLICT(principal, id) DO UPDATE SET color=excluded.color")
        .bind(owner)
        .bind(id)
        .bind(color)
        .execute(&state.db)
        .await?;
    Ok(())
}

/// The colour alone (settings page "Farben", colour picker).
pub async fn set_addressbook_color_checked(state: &AppState, user: &Principal, owner: &str, id: &str, color: &str) -> AppResult<()> {
    ensure_owner(user, owner)?;
    state.addressbooks.get_addressbook(owner, id, false).await?;
    set_addressbook_color(state, owner, id, color).await
}

pub struct NewCalendar {
    pub owner: String,
    pub name: String,
    pub color: String,
    pub description: String,
    pub components: Vec<CalendarObjectType>,
    pub timezone: String,
}

pub fn parse_components(s: &[String]) -> Vec<CalendarObjectType> {
    let mut v: Vec<CalendarObjectType> = s
        .iter()
        .filter_map(|c| match c.as_str() {
            "VEVENT" => Some(CalendarObjectType::Event),
            "VTODO" => Some(CalendarObjectType::Todo),
            "VJOURNAL" => Some(CalendarObjectType::Journal),
            _ => None,
        })
        .collect();
    if v.is_empty() {
        v.push(CalendarObjectType::Event);
    }
    v
}

pub async fn create_calendar(state: &AppState, user: &Principal, n: NewCalendar) -> AppResult<String> {
    ensure_owner(user, &n.owner)?;
    if n.name.trim().is_empty() {
        return Err(AppError::bad("err-name-required"));
    }
    let id = uuid::Uuid::new_v4().to_string();
    state
        .calendars
        .insert_calendar(Calendar {
            meta: CalendarMetadata {
                displayname: Some(n.name.trim().to_string()),
                order: 0,
                description: Some(n.description.trim().to_string()).filter(|d| !d.is_empty()),
                color: Some(n.color.trim().to_string()).filter(|c| !c.is_empty()),
            },
            principal: n.owner,
            id: id.clone(),
            timezone_id: Some(n.timezone.trim().to_string()).filter(|t| !t.is_empty()),
            deleted_at: None,
            synctoken: 0,
            subscription_url: None,
            push_topic: uuid::Uuid::new_v4().to_string(),
            components: n.components,
        })
        .await?;
    Ok(id)
}

pub async fn update_calendar(state: &AppState, user: &Principal, owner: &str, id: &str, name: &str, color: &str, description: &str) -> AppResult<()> {
    ensure_owner(user, owner)?;
    let mut c = state.calendars.get_calendar(owner, id, false).await?;
    if !name.trim().is_empty() {
        c.meta.displayname = Some(name.trim().to_string());
    }
    c.meta.color = Some(color.trim().to_string()).filter(|c| !c.is_empty());
    c.meta.description = Some(description.trim().to_string()).filter(|d| !d.is_empty());
    state.calendars.update_calendar(owner, id, c).await?;
    Ok(())
}

pub async fn delete_calendar(state: &AppState, user: &Principal, owner: &str, id: &str, hard: bool) -> AppResult<()> {
    ensure_owner(user, owner)?;
    // a calendar a PractiCalUploader device manages is removed there (deselected) or with the device; one it
    // deselected waits in the trash and may be purged from there
    let in_trash = state.calendars.get_calendar(owner, id, true).await.is_ok_and(|c| c.deleted_at.is_some());
    if !in_trash && crate::domain::uploader::managed(state).await?.contains(&(owner.to_string(), id.to_string())) {
        return Err(AppError::bad("err-calendar-managed"));
    }
    state.calendars.delete_calendar(owner, id, !hard).await?;
    if hard {
        // purged: nothing is left to restore or take over (the uploader's key would point at nothing)
        sqlx::query("DELETE FROM practical_readonly_calendars WHERE principal = ? AND calendar_id = ?").bind(owner).bind(id).execute(&state.db).await?;
    }
    // a sync link is reached through its collection, so it has to end with it
    crate::domain::remote::unlink_collection(state, owner, crate::domain::remote::dav::Kind::Cal, id).await;
    Ok(())
}

pub async fn restore_calendar(state: &AppState, user: &Principal, owner: &str, id: &str) -> AppResult<()> {
    ensure_owner(user, owner)?;
    state.calendars.restore_calendar(owner, id).await?;
    Ok(())
}

pub async fn create_addressbook(state: &AppState, user: &Principal, owner: &str, name: &str, color: &str, description: &str) -> AppResult<String> {
    ensure_owner(user, owner)?;
    if name.trim().is_empty() {
        return Err(AppError::bad("err-name-required"));
    }
    let id = uuid::Uuid::new_v4().to_string();
    set_addressbook_color(state, owner, &id, color).await?;
    state
        .addressbooks
        .insert_addressbook(Addressbook {
            id: id.clone(),
            principal: owner.to_string(),
            displayname: Some(name.trim().to_string()),
            description: Some(description.trim().to_string()).filter(|d| !d.is_empty()),
            deleted_at: None,
            synctoken: 0,
            push_topic: uuid::Uuid::new_v4().to_string(),
        })
        .await?;
    Ok(id)
}

pub async fn update_addressbook(state: &AppState, user: &Principal, owner: &str, id: &str, name: &str, color: &str, description: &str) -> AppResult<()> {
    ensure_owner(user, owner)?;
    let mut a = state.addressbooks.get_addressbook(owner, id, false).await?;
    set_addressbook_color(state, owner, id, color).await?;
    if !name.trim().is_empty() {
        a.displayname = Some(name.trim().to_string());
    }
    a.description = Some(description.trim().to_string()).filter(|d| !d.is_empty());
    state.addressbooks.update_addressbook(owner, id, a).await?;
    Ok(())
}

pub async fn delete_addressbook(state: &AppState, user: &Principal, owner: &str, id: &str, hard: bool) -> AppResult<()> {
    ensure_owner(user, owner)?;
    state.addressbooks.delete_addressbook(owner, id, !hard).await?;
    crate::domain::remote::unlink_collection(state, owner, crate::domain::remote::dav::Kind::Ab, id).await;
    if hard {
        sqlx::query("DELETE FROM practical_addressbook_colors WHERE principal = ? AND id = ?").bind(owner).bind(id).execute(&state.db).await?;
    }
    Ok(())
}

pub async fn restore_addressbook(state: &AppState, user: &Principal, owner: &str, id: &str) -> AppResult<()> {
    ensure_owner(user, owner)?;
    state.addressbooks.restore_addressbook(owner, id).await?;
    Ok(())
}

/// Deletes one event, task (`Store::Cal`) or contact (`Store::Ab`) of a collection the user owns.
pub async fn delete_object(state: &AppState, user: &Principal, store: Store, principal: &str, coll: &str, object_id: &str) -> AppResult<()> {
    ensure_owner(user, principal)?;
    match store {
        Store::Cal => {
            ensure_writable(state, principal, coll).await?;
            state.calendars.delete_object(principal, coll, object_id, true).await?
        }
        Store::Ab => AddressbookWriteStore::delete_object(&*state.addressbooks, principal, coll, object_id, true).await?,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_colour_must_be_hex() {
        let owner = Owner { id: "p".into(), label: "p".into(), is_group: false };
        let cal = |color: &str| Calendar {
            meta: CalendarMetadata { displayname: None, order: 0, description: None, color: Some(color.into()) },
            principal: "p".into(),
            id: "c".into(),
            timezone_id: None,
            deleted_at: None,
            synctoken: 0,
            subscription_url: None,
            push_topic: String::new(),
            components: vec![],
        };
        assert_eq!(cal_info(cal("#Ab12eF"), &owner).color, "#Ab12eF");
        // seven bytes, but markup: the fallback takes its place
        for bad in ["#\"><b>x", "#;x:y()", "#ff00zz"] {
            assert_eq!(cal_info(cal(bad), &owner).color, default_color("c"), "{bad}");
        }
    }
}
