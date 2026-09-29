//! Calendars and address books for the user and their groups (plan D4/D5/D16).

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
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AbInfo {
    pub principal: String,
    pub owner_label: String,
    pub is_group: bool,
    pub id: String,
    pub displayname: String,
    /// xui_addressbook_colors, else default_color(id)
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
        color: c.meta.color.filter(|c| c.starts_with('#') && c.len() == 7).unwrap_or(fallback),
        description: c.meta.description.unwrap_or_default(),
        components: c.components.iter().map(|c| c.as_str().to_string()).collect(),
        timezone_id: c.timezone_id.unwrap_or_default(),
        subscription_url: c.subscription_url,
        deleted_at: c.deleted_at,
        synctoken: c.synctoken,
    }
}

fn ab_info(a: Addressbook, owner: &Owner, color: Option<String>) -> AbInfo {
    AbInfo {
        color: color.unwrap_or_else(|| default_color(&a.id)),
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
    let mut out = vec![];
    for o in owners(state, user).await? {
        let cals = if deleted { state.calendars.get_deleted_calendars(&o.id).await? } else { state.calendars.get_calendars(&o.id).await? };
        out.extend(cals.into_iter().map(|c| cal_info(c, &o)));
    }
    Ok(out)
}

pub async fn addressbooks_for(state: &AppState, user: &Principal, deleted: bool) -> AppResult<Vec<AbInfo>> {
    use sqlx::Row;
    let colors: std::collections::HashMap<(String, String), String> = sqlx::query("SELECT principal, id, color FROM xui_addressbook_colors")
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
        return Err(AppError::BadRequest("color must be #rrggbb".into()));
    }
    sqlx::query("INSERT INTO xui_addressbook_colors (principal, id, color) VALUES (?, ?, ?) ON CONFLICT(principal, id) DO UPDATE SET color=excluded.color")
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
        return Err(AppError::BadRequest("name required".into()));
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
    state.calendars.delete_calendar(owner, id, !hard).await?;
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
        return Err(AppError::BadRequest("name required".into()));
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
    if hard {
        sqlx::query("DELETE FROM xui_addressbook_colors WHERE principal = ? AND id = ?").bind(owner).bind(id).execute(&state.db).await?;
    }
    Ok(())
}

pub async fn restore_addressbook(state: &AppState, user: &Principal, owner: &str, id: &str) -> AppResult<()> {
    ensure_owner(user, owner)?;
    state.addressbooks.restore_addressbook(owner, id).await?;
    Ok(())
}
