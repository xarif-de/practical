//! Calendars and address books for the user and their groups.
//!
//! The collections a user sees are their own and those of their groups ([`owners`]). Create, update,
//! delete to the trash, restore and purge go through RustiCal's store; [`delete_object`] deletes one object.
//! Address-book colours are PractiCal's own (`practical_addressbook_colors`).

use crate::domain::history_text::Store;
use crate::error::{AppError, AppResult};
use crate::state::{AppState, Authorized, Need};
use chrono::NaiveDateTime;
use rustical_ical::CalendarObjectType;
use rustical_store::auth::{AuthenticationProvider, Principal};
use rustical_store::{Addressbook, Calendar, CalendarMetadata};

#[derive(Debug, Clone, serde::Serialize)]
pub struct Owner {
    pub id: String,
    pub label: String,
    pub is_group: bool,
}

/// How the user has a collection: their own (or a group's of theirs), or shared with them read-only / read-write
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum View {
    Own,
    Read,
    Write,
}

impl View {
    pub fn of(a: &Authorized) -> Self {
        match a.access() {
            None => Self::Own,
            Some(crate::state::Access::Read) => Self::Read,
            Some(crate::state::Access::Write) => Self::Write,
        }
    }
    pub fn is_copy(self) -> bool {
        self != Self::Own
    }
    /// The stronger of two ways to the same collection (own beats write beats read)
    fn rank(self) -> u8 {
        match self {
            Self::Own => 2,
            Self::Write => 1,
            Self::Read => 0,
        }
    }
}

/// A calendar as the user has it. `principal`/`id` = the path (home, name: what a form sends back, e.g.
/// `#ed-cal`); `owner`/`owner_id` the calendar itself; `auth` the checked access every read and write goes through.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CalInfo {
    #[serde(skip)]
    pub auth: Authorized,
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
    /// PractiCal writes nothing into it ([`is_read_only`]): a subscription, a calendar of `practical_readonly_calendars`,
    /// or a read-only share
    pub read_only: bool,
    /// a live PractiCalUploader device manages it (`uploader::managed`): it cannot be deleted here
    pub managed: bool,
    /// own, or shared read / write
    pub view: View,
    /// the owner's name, for a share
    pub shared_by: Option<String>,
    /// the owning principal
    pub owner: String,
    /// own and shared with someone
    pub has_shares: bool,
    /// the owner's visibility levels (private, confidential) when sharees are involved (own with shares, or a share)
    pub vis: Option<(u8, u8)>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AbInfo {
    #[serde(skip)]
    pub auth: Authorized,
    pub principal: String,
    pub owner_label: String,
    pub is_group: bool,
    pub id: String,
    pub displayname: String,
    /// practical_addressbook_colors (for a share: the sharee's colour of the share), else default_color(id)
    pub color: String,
    pub description: String,
    pub deleted_at: Option<NaiveDateTime>,
    pub synctoken: i64,
    pub view: View,
    pub shared_by: Option<String>,
    pub owner: String,
    pub has_shares: bool,
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
        if let Some(g) = state.principals().get_principal(gid).await? {
            out.push(Owner { id: g.id.clone(), label: g.displayname.clone().unwrap_or(g.id), is_group: true });
        }
    }
    Ok(out)
}

/// The user may act as `principal` (themselves or a group of theirs): to create collections there, or to
/// act on behalf of that principal without a collection (remote links, devices).
pub fn ensure_owner(user: &Principal, principal: &str) -> AppResult<()> {
    if user.is_principal(principal) { Ok(()) } else { Err(AppError::Forbidden) }
}

/// The calendar `(home, name)` of a request, checked for `need` (`state::Collections::ensure_calendar`).
pub async fn ensure_calendar(state: &AppState, user: &Principal, home: &str, name: &str, need: Need) -> AppResult<Authorized> {
    Ok(state.collections().ensure_calendar(user, home, name, need, false).await?.0)
}

/// The address book `(home, name)` of a request, checked for `need`.
pub async fn ensure_addressbook(state: &AppState, user: &Principal, home: &str, name: &str, need: Need) -> AppResult<Authorized> {
    Ok(state.collections().ensure_addressbook(user, home, name, need, false).await?.0)
}

impl CalInfo {
    /// The user's own live calendar that can be shared (not a subscription, not a birthday calendar)
    pub fn shareable(&self) -> bool {
        !self.view.is_copy() && self.subscription_url.is_none() && self.deleted_at.is_none() && !self.id.starts_with("_birthdays_")
    }

    /// Only the access, for code that reads one object of a calendar it was handed by a request
    pub fn bare(auth: Authorized) -> Self {
        Self {
            principal: auth.home().to_string(),
            owner_label: String::new(),
            is_group: false,
            id: auth.name().to_string(),
            displayname: String::new(),
            color: String::new(),
            description: String::new(),
            components: vec![],
            timezone_id: String::new(),
            subscription_url: None,
            deleted_at: None,
            synctoken: 0,
            read_only: false,
            managed: false,
            view: View::of(&auth),
            shared_by: None,
            owner: auth.owner().to_string(),
            has_shares: false,
            vis: None,
            auth,
        }
    }
}

fn cal_info(a: Authorized, c: Calendar, owner: &Owner) -> CalInfo {
    let fallback = default_color(&c.id);
    let view = View::of(&a);
    CalInfo {
        principal: a.home().to_string(),
        owner_label: owner.label.clone(),
        is_group: owner.is_group,
        id: a.name().to_string(),
        displayname: c.meta.displayname.unwrap_or_default(),
        // any CalDAV client sets this property: only #rrggbb may reach a style attribute
        color: c.meta.color.filter(|c| valid_color(c)).unwrap_or(fallback),
        description: c.meta.description.unwrap_or_default(),
        components: c.components.iter().map(|c| c.as_str().to_string()).collect(),
        timezone_id: c.timezone_id.unwrap_or_default(),
        read_only: c.subscription_url.is_some() || view == View::Read,
        managed: false,
        subscription_url: c.subscription_url,
        deleted_at: c.deleted_at,
        synctoken: c.synctoken,
        view,
        shared_by: None,
        owner: a.owner().to_string(),
        has_shares: false,
        vis: None,
        auth: a,
    }
}

/// The one read-only predicate of a calendar `(owner, id)`: a subscribed calendar (the next refresh would overwrite a
/// change) or one PractiCal keeps read-only (`practical_readonly_calendars`, e.g. filled by PractiCalUploader,
/// `domain::uploader`). Every PractiCal write into a calendar asks [`ensure_writable`]; CalDAV clients are not
/// covered (RustiCal grants them write).
pub async fn is_read_only(state: &AppState, owner: &str, cal_id: &str) -> AppResult<bool> {
    if crate::domain::uploader::index(state).await?.ro.contains_key(&(owner.to_string(), cal_id.to_string())) {
        return Ok(true);
    }
    Ok(match state.collections().own_calendar(owner, cal_id, true).await {
        Ok((_, c)) => c.subscription_url.is_some(),
        Err(AppError::NotFound | AppError::Store(rustical_store::Error::NotFound)) => false, // the write that follows answers for a missing calendar
        Err(e) => return Err(e),
    })
}

/// `Err` for a write into a read-only calendar: a read-only share (`err-share-read-only`), a subscription
/// (`err-subscribed-read-only`, the message it always had), else `err-calendar-read-only` ([`is_read_only`]).
pub async fn ensure_writable(state: &AppState, a: &Authorized) -> AppResult<()> {
    if !a.allows(Need::Write) {
        return Err(AppError::bad("err-share-read-only"));
    }
    if !is_read_only(state, a.owner(), a.id()).await? {
        return Ok(());
    }
    let subscribed = state.collections().own_calendar(a.owner(), a.id(), true).await.is_ok_and(|(_, c)| c.subscription_url.is_some());
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
    crate::domain::uploader::changed(state);
    Ok(())
}

impl AbInfo {
    /// The user's own live address book (every one can be shared)
    pub fn shareable(&self) -> bool {
        !self.view.is_copy() && self.deleted_at.is_none()
    }

    /// Only the access (see `CalInfo::bare`)
    pub fn bare(auth: Authorized) -> Self {
        Self {
            principal: auth.home().to_string(),
            owner_label: String::new(),
            is_group: false,
            id: auth.name().to_string(),
            displayname: String::new(),
            color: String::new(),
            description: String::new(),
            deleted_at: None,
            synctoken: 0,
            view: View::of(&auth),
            shared_by: None,
            owner: auth.owner().to_string(),
            has_shares: false,
            auth,
        }
    }
}

fn ab_info(auth: Authorized, a: Addressbook, owner: &Owner, color: Option<String>) -> AbInfo {
    AbInfo {
        color: color.filter(|c| valid_color(c)).unwrap_or_else(|| default_color(&a.id)), // as cal_info: a style attribute takes it
        principal: auth.home().to_string(),
        owner_label: owner.label.clone(),
        is_group: owner.is_group,
        id: auth.name().to_string(),
        displayname: a.displayname.unwrap_or_default(),
        description: a.description.unwrap_or_default(),
        deleted_at: a.deleted_at,
        synctoken: a.synctoken,
        view: View::of(&auth),
        shared_by: None,
        owner: auth.owner().to_string(),
        has_shares: false,
        auth,
    }
}

/// The display name of a principal (for "shared by"), its id if it has none
async fn label_of(state: &AppState, labels: &mut std::collections::HashMap<String, String>, id: &str) -> String {
    if let Some(l) = labels.get(id) {
        return l.clone();
    }
    let l = state.principals().get_principal(id).await.ok().flatten().and_then(|p| p.displayname).unwrap_or_else(|| id.to_string());
    labels.insert(id.to_string(), l.clone());
    l
}

/// The same collection reached twice (own through a group and shared, or shared to the user and a group):
/// once, the strongest way (own, else write, else read); first come first kept among equals.
fn strongest<T>(items: Vec<T>, key: impl Fn(&T) -> (String, String), view: impl Fn(&T) -> View) -> Vec<T> {
    let mut best: std::collections::HashMap<(String, String), usize> = std::collections::HashMap::new();
    for (i, it) in items.iter().enumerate() {
        let k = key(it);
        match best.get(&k) {
            Some(&j) if view(&items[j]).rank() >= view(it).rank() => {}
            _ => {
                best.insert(k, i);
            }
        }
    }
    let keep: std::collections::HashSet<usize> = best.into_values().collect();
    items.into_iter().enumerate().filter(|(i, _)| keep.contains(i)).map(|(_, it)| it).collect()
}

pub async fn calendars_for(state: &AppState, user: &Principal, deleted: bool) -> AppResult<Vec<CalInfo>> {
    let ro = crate::domain::uploader::index(state).await?;
    let owners = owners(state, user).await?;
    let managed = crate::domain::uploader::managed_for(state, user, &owners.iter().map(|o| o.id.clone()).collect()).await?;
    let mut labels = std::collections::HashMap::new();
    let mut out = vec![];
    let colls = state.collections();
    for (a, c) in colls.calendars(user, deleted).await? {
        let Some(home) = owners.iter().find(|o| o.id == a.home()) else { continue };
        let mut c = cal_info(a, c, home);
        let key = (c.owner.clone(), c.auth.id().to_string());
        c.read_only |= ro.ro.contains_key(&key);
        if c.view.is_copy() {
            c.shared_by = Some(label_of(state, &mut labels, &c.owner).await);
            c.vis = c.auth.policy().map(|p| (p.private, p.confidential));
        } else {
            c.managed = managed.contains(&key);
            if !deleted && !colls.calendar_shares(&c.auth).await?.is_empty() {
                c.has_shares = true;
                let p = colls.visibility(&c.owner).await?;
                c.vis = Some((p.private, p.confidential));
            }
        }
        out.push(c);
    }
    Ok(strongest(out, |c| (c.owner.clone(), c.auth.id().to_string()), |c| c.view))
}

pub async fn addressbooks_for(state: &AppState, user: &Principal, deleted: bool) -> AppResult<Vec<AbInfo>> {
    use sqlx::Row;
    let colors: std::collections::HashMap<(String, String), String> = sqlx::query("SELECT principal, id, color FROM practical_addressbook_colors")
        .fetch_all(&state.db)
        .await?
        .into_iter()
        .map(|r| ((r.get("principal"), r.get("id")), r.get("color")))
        .collect();
    let owners = owners(state, user).await?;
    let mut labels = std::collections::HashMap::new();
    let mut out = vec![];
    let colls = state.collections();
    for (auth, a) in colls.addressbooks(user, deleted).await? {
        let Some(home) = owners.iter().find(|o| o.id == auth.home()) else { continue };
        let copy = !auth.is_own();
        // a sharee's colour is its own (the share row), the owner's PractiCal colour otherwise
        let color = if copy { colls.addressbook_share_color(&auth).await?.or_else(|| colors.get(&(auth.owner().to_string(), auth.id().to_string())).cloned()) } else { colors.get(&(auth.owner().to_string(), auth.id().to_string())).cloned() };
        let mut a = ab_info(auth, a, home, color);
        if copy {
            a.shared_by = Some(label_of(state, &mut labels, &a.owner).await);
        } else if !deleted {
            a.has_shares = !colls.addressbook_shares(&a.auth).await?.is_empty();
        }
        out.push(a);
    }
    Ok(strongest(out, |a| (a.owner.clone(), a.auth.id().to_string()), |a| a.view))
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

/// The colour alone (settings page "Farben", colour picker): for a share the sharee's own.
pub async fn set_addressbook_color_checked(state: &AppState, user: &Principal, owner: &str, id: &str, color: &str) -> AppResult<()> {
    let a = ensure_addressbook(state, user, owner, id, Need::Read).await?;
    if a.is_own() {
        return set_addressbook_color(state, a.owner(), a.id(), color).await;
    }
    if !valid_color(color) {
        return Err(AppError::bad("err-color"));
    }
    state.collections().update_addressbook(&a, None, Some(color.to_string()), None).await
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
        .collections()
        .create_calendar(
            user,
            Calendar {
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
            },
        )
        .await?;
    Ok(id)
}

/// Name, colour, description of a calendar the user owns; for a share the sharee's own name and colour.
pub async fn update_calendar(state: &AppState, user: &Principal, owner: &str, id: &str, name: &str, color: &str, description: &str) -> AppResult<()> {
    let a = ensure_calendar(state, user, owner, id, Need::Read).await?;
    let name = Some(name.trim().to_string()).filter(|n| !n.is_empty());
    let description = a.is_own().then(|| description.trim().to_string());
    state.collections().update_calendar(&a, name, Some(color.trim().to_string()), description).await
}

/// Own: into the trash, or `hard` purged. A share: ended for the user's home (the owner keeps the calendar).
pub async fn delete_calendar(state: &AppState, user: &Principal, owner: &str, id: &str, hard: bool) -> AppResult<()> {
    let (a, c) = state.collections().ensure_calendar(user, owner, id, Need::Read, true).await?;
    if !a.is_own() {
        return state.collections().delete_calendar(&a, false).await;
    }
    // a calendar a PractiCalUploader device manages is removed there (deselected) or with the device; one it
    // deselected waits in the trash and may be purged from there
    let in_trash = c.deleted_at.is_some();
    if !in_trash && crate::domain::uploader::managed(state).await?.contains(&(a.owner().to_string(), a.id().to_string())) {
        return Err(AppError::bad("err-calendar-managed"));
    }
    state.collections().delete_calendar(&a, hard).await?;
    if hard {
        // purged: nothing is left to restore or take over (the uploader's key would point at nothing)
        sqlx::query("DELETE FROM practical_readonly_calendars WHERE principal = ? AND calendar_id = ?").bind(a.owner()).bind(a.id()).execute(&state.db).await?;
        crate::domain::uploader::changed(state);
    }
    // a sync link is reached through its collection, so it has to end with it
    crate::domain::remote::unlink_collection(state, a.owner(), crate::domain::remote::dav::Kind::Cal, a.id()).await;
    Ok(())
}

pub async fn restore_calendar(state: &AppState, user: &Principal, owner: &str, id: &str) -> AppResult<()> {
    let (a, _) = state.collections().ensure_calendar(user, owner, id, Need::Own, true).await?;
    state.collections().restore_calendar(&a).await?;
    crate::domain::uploader::release(state, a.owner(), a.id()).await
}

pub async fn create_addressbook(state: &AppState, user: &Principal, owner: &str, name: &str, color: &str, description: &str) -> AppResult<String> {
    ensure_owner(user, owner)?;
    if name.trim().is_empty() {
        return Err(AppError::bad("err-name-required"));
    }
    let id = uuid::Uuid::new_v4().to_string();
    set_addressbook_color(state, owner, &id, color).await?;
    state
        .collections()
        .create_addressbook(
            user,
            Addressbook {
                id: id.clone(),
                principal: owner.to_string(),
                displayname: Some(name.trim().to_string()),
                description: Some(description.trim().to_string()).filter(|d| !d.is_empty()),
                deleted_at: None,
                synctoken: 0,
                push_topic: uuid::Uuid::new_v4().to_string(),
            },
        )
        .await?;
    Ok(id)
}

/// Name, colour, description of an address book the user owns; for a share the sharee's own name and colour.
pub async fn update_addressbook(state: &AppState, user: &Principal, owner: &str, id: &str, name: &str, color: &str, description: &str) -> AppResult<()> {
    let a = ensure_addressbook(state, user, owner, id, Need::Read).await?;
    if !valid_color(color) {
        return Err(AppError::bad("err-color"));
    }
    let name = Some(name.trim().to_string()).filter(|n| !n.is_empty());
    if a.is_own() {
        set_addressbook_color(state, a.owner(), a.id(), color).await?;
        return state.collections().update_addressbook(&a, name, None, Some(description.trim().to_string())).await;
    }
    state.collections().update_addressbook(&a, name, Some(color.to_string()), None).await
}

pub async fn delete_addressbook(state: &AppState, user: &Principal, owner: &str, id: &str, hard: bool) -> AppResult<()> {
    let (a, _) = state.collections().ensure_addressbook(user, owner, id, Need::Read, true).await?;
    if !a.is_own() {
        return state.collections().delete_addressbook(&a, false).await;
    }
    state.collections().delete_addressbook(&a, hard).await?;
    crate::domain::remote::unlink_collection(state, a.owner(), crate::domain::remote::dav::Kind::Ab, a.id()).await;
    if hard {
        sqlx::query("DELETE FROM practical_addressbook_colors WHERE principal = ? AND id = ?").bind(a.owner()).bind(a.id()).execute(&state.db).await?;
    }
    Ok(())
}

pub async fn restore_addressbook(state: &AppState, user: &Principal, owner: &str, id: &str) -> AppResult<()> {
    let (a, _) = state.collections().ensure_addressbook(user, owner, id, Need::Own, true).await?;
    state.collections().restore_addressbook(&a).await
}

/// Deletes one event, task (`Store::Cal`) or contact (`Store::Ab`) of a collection the user may write
/// (`(principal, coll)` = the path). Through a share always into the owner's trash bin.
pub async fn delete_object(state: &AppState, user: &Principal, store: Store, principal: &str, coll: &str, object_id: &str) -> AppResult<()> {
    match store {
        Store::Cal => {
            let a = ensure_calendar(state, user, principal, coll, Need::Read).await?;
            ensure_writable(state, &a).await?;
            state.collections().delete_object(&a, object_id, None).await?
        }
        Store::Ab => {
            let a = ensure_addressbook(state, user, principal, coll, Need::Write).await?;
            state.collections().delete_address_object(&a, object_id).await?
        }
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
        assert_eq!(cal_info(Authorized::test_own("p", "c"), cal("#Ab12eF"), &owner).color, "#Ab12eF");
        // seven bytes, but markup: the fallback takes its place
        for bad in ["#\"><b>x", "#;x:y()", "#ff00zz"] {
            assert_eq!(cal_info(Authorized::test_own("p", "c"), cal(bad), &owner).color, default_color("c"), "{bad}");
        }
    }
}
