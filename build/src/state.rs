//! [`AppState`]: the process-wide state every handler gets (config, DB pool, RustiCal stores, caches, guards).
//!
//! The caches and their keys are tabled in the crate docs, section "Caches".

use crate::config::Config;
use crate::error::{AppError, AppResult, Msg};
use crate::i18n::I18n;
use axum::http::StatusCode;
use rustical_dav_push::VapidKeypair;
use rustical_store::access::{self, Resolved};
use rustical_store::auth::{AuthenticationProvider, Principal};
use rustical_store::calendar_store::CalendarQuery;
use rustical_store::{Addressbook, AddressbookReadStore, AddressbookShare, AddressbookWriteStore, Calendar, CalendarReadStore, CalendarShare, CalendarWriteStore, CopyRule, Preconditions, ShareStore, ShareViewUpdate};
use rustical_store_sqlite::{SqliteAddressbookStore, SqliteCalendarStore, SqlitePrincipalStore, SqliteShareStore};
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::broadcast;

// What routes need to name sharing: re-exported so nothing outside this module imports `access` or `shared`
pub use rustical_store::Access;
pub use rustical_store::access::{Need, ResolvedKind};
pub use rustical_store::shared::{AddressObject, CalendarObject, FULL_LEVEL, IfMatch, Policy};

/// A change in a collection, fanned out to SSE clients.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ChangeEvent {
    pub kind: &'static str, // "calendar" | "addressbook"
    pub principal: String,
    pub id: String,
    pub synctoken: i64,
}

pub struct AppState {
    pub config: Config,
    pub db: SqlitePool,
    principals: Arc<SqlitePrincipalStore>,
    // RustiCal's collection stores and the share store: only through [`Collections`], so every access to a
    // collection passes `ensure` (or a listing per user) first
    calendars: Arc<SqliteCalendarStore>,
    addressbooks: Arc<SqliteAddressbookStore>,
    shares: Arc<SqliteShareStore>,
    pub i18n: Arc<I18n>,
    pub changes: broadcast::Sender<ChangeEvent>,
    /// Development only (`config.dev_assets_dir`): fired when an asset file changes → `/dev/reload`.
    pub dev_reload: broadcast::Sender<()>,
    pub http: reqwest::Client,
    /// Separate from `http`: a remote DAV server needs a 60 s request timeout (a batched multiget
    /// does not fit in push's 10 s) and must not follow redirects.
    pub remote_http: reqwest::Client,
    /// Logins a user started but has not finished. In process on purpose: an app password reaches
    /// the whole Nextcloud account, so a one-off import must not put one at rest.
    pub remote_logins: Arc<crate::domain::remote::login::Logins>,
    /// Connecting a device: pending Login Flow v2 flows and the wizard's pairings (`domain::connect`).
    pub connect: crate::domain::connect::Connect,
    /// Per-link round locks and the echo filter of the sync loop.
    pub remote_runner: Arc<crate::domain::remote::sched::Runner>,
    /// geocoder proxy: throttle + cache (`domain::geo`)
    pub geo: crate::domain::geo::Geo,
    /// the top-bar search's per-user corpus (`domain::search`)
    pub search_index: crate::domain::search::IndexCache,
    /// parsed tasks per calendar and synctoken (`domain::tasks::tasks_for`)
    pub task_cache: crate::domain::tasks::TaskCache,
    /// derived data between requests, each keyed by synctoken (and zone/year/day/language where it depends on them)
    pub caches: Caches,
    /// the change history's lock between UI writes and the DAV poll (`domain::history`)
    pub history: crate::domain::history::Hist,
    pub vapid: Option<VapidKeypair>,
    pub started_at: chrono::DateTime<chrono::Utc>,
    /// the HTTP layer's process-wide state (shutdown signal, write locks, login throttle)
    pub guards: Guards,
}

pub type SharedState = Arc<AppState>;

/// Request results kept between requests (see each type); all synctoken-keyed, so a write is never
/// answered from before it.
#[derive(Default)]
pub struct Caches {
    pub instances: crate::domain::events::InstanceCache,
    pub density: crate::domain::events::DensityCache,
    pub obj_meta: crate::domain::events::ObjMetaCache,
    pub contacts: crate::domain::contacts::ContactCache,
    pub mentions: crate::routes::api::MentionCache,
    pub holidays: crate::domain::holidays::MemCache,
    /// a year of instances per calendar for the five year views (`routes::more::YearCache`)
    pub years: crate::routes::more::YearCache,
    /// the history document's changes and elements per user (`routes::history::HistoryCache`)
    pub history: crate::routes::history::HistoryCache,
    /// the read-only calendars and uploader devices (`domain::uploader::index`), kept until a write to them
    pub uploader: crate::domain::uploader::IndexCache,
}

/// Process-wide state of the HTTP layer; every part starts empty.
#[derive(Default)]
pub struct Guards {
    /// `true` once the server stops (SIGTERM / Ctrl-C): open streams (SSE) end, so the graceful
    /// shutdown is not held up by connections that never finish on their own
    pub shutdown: tokio::sync::watch::Sender<bool>,
    /// one lock per user around the read-modify-write of `practical_user_settings` (`prefs::get` → change
    /// a field → `prefs::set` writes the whole row): two saves at once otherwise lose one of them
    pub prefs: std::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<tokio::sync::Mutex<()>>>>,
    /// failed password logins (`auth::LoginThrottle`)
    pub login: crate::auth::LoginThrottle,
}

impl Guards {
    /// The user's settings lock; hold the guard from `prefs::get` until after `prefs::set`.
    pub async fn prefs(&self, user: &str) -> tokio::sync::OwnedMutexGuard<()> {
        let m = self.prefs.lock().unwrap().entry(user.to_string()).or_default().clone();
        m.lock_owned().await
    }

    /// Resolves once the server is stopping.
    pub fn stopping(&self) -> impl std::future::Future<Output = ()> + Send + use<> {
        let mut rx = self.shutdown.subscribe();
        async move {
            let _ = rx.wait_for(|v| *v).await;
        }
    }
}

/// What [`AppState::new`] takes; every other part starts empty.
pub struct StateParts {
    pub config: Config,
    pub db: SqlitePool,
    pub i18n: Arc<I18n>,
    pub changes: broadcast::Sender<ChangeEvent>,
    pub dev_reload: broadcast::Sender<()>,
    pub http: reqwest::Client,
    pub remote_http: reqwest::Client,
    pub vapid: Option<VapidKeypair>,
    pub started_at: chrono::DateTime<chrono::Utc>,
}

impl AppState {
    /// The state with RustiCal's stores on `db`. Their change channels have no receiver: PractiCal learns
    /// of changes from the synctokens (`tasks::spawn_change_poller`), and a full channel only drops a note.
    pub fn new(p: StateParts) -> Self {
        let (cal_tx, _) = tokio::sync::mpsc::channel(64);
        let (ab_tx, _) = tokio::sync::mpsc::channel(64);
        let (share_tx, _) = tokio::sync::mpsc::channel(64);
        Self {
            principals: Arc::new(SqlitePrincipalStore::new(p.db.clone())),
            calendars: Arc::new(SqliteCalendarStore::new(p.db.clone(), cal_tx, true)),
            addressbooks: Arc::new(SqliteAddressbookStore::new(p.db.clone(), ab_tx, true)),
            shares: Arc::new(SqliteShareStore::new(p.db.clone(), share_tx)),
            i18n: p.i18n,
            changes: p.changes,
            dev_reload: p.dev_reload,
            http: p.http,
            remote_http: p.remote_http,
            remote_logins: Arc::new(crate::domain::remote::login::Logins::default()),
            connect: Default::default(),
            remote_runner: Arc::new(crate::domain::remote::sched::Runner::default()),
            geo: Default::default(),
            search_index: Default::default(),
            task_cache: Default::default(),
            caches: Default::default(),
            history: Default::default(),
            vapid: p.vapid,
            started_at: p.started_at,
            guards: Default::default(),
            db: p.db,
            config: p.config,
        }
    }

    /// RustiCal's principal store: principals, memberships, app tokens (nothing of collections).
    pub fn principals(&self) -> &Arc<SqlitePrincipalStore> {
        &self.principals
    }

    /// The only way to calendars, address books, their objects and shares.
    pub fn collections(&self) -> Collections<'_> {
        Collections(self)
    }

    /// Restores a trashed group (`domain::groups`, module doc): claim the row; the actor must have the right
    /// (`groups::may_act`); memberships come back FIRST in one transaction (not for a group that still has
    /// members: an interrupted trash), and at least the actor's must stand, else stop without deleting the
    /// row; then exactly the collections the row lists; the row last. Every step renews the claim.
    pub async fn restore_trashed_group(&self, gid: &str, actor: &Principal) -> AppResult<()> {
        use crate::domain::groups;
        let Some(c) = groups::claim(self, gid, "restore", None).await? else {
            return Err(if groups::exists(self, gid).await? { AppError::conflict("err-group-busy") } else { AppError::NotFound });
        };
        match self.restore_steps(gid, actor, &c).await {
            Ok(()) => Ok(()),
            Err(e) => {
                groups::release_failed(self, gid, &c.claim).await;
                Err(e)
            }
        }
    }

    async fn restore_steps(&self, gid: &str, actor: &Principal, c: &crate::domain::groups::Claimed) -> AppResult<()> {
        use crate::domain::groups;
        if !groups::may_act(self, gid, &c.members, actor).await? {
            return Err(AppError::Forbidden);
        }
        if self.principals.get_principal(gid).await?.is_none() {
            return Err(AppError::conflict("err-group-gone"));
        }
        if self.principals.list_members(gid).await?.is_empty() {
            groups::renew(self, gid, &c.claim).await?;
            // one transaction; members deleted since are skipped
            self.principals.set_memberships(gid, &c.members).await?;
            if !self.principals.list_members(gid).await?.iter().any(|m| m == &actor.id) {
                return Err(AppError::conflict("err-group-gone"));
            }
        }
        // only what the trash put in the bin: a collection members had deleted before stays there
        for id in &c.calendars {
            groups::renew(self, gid, &c.claim).await?;
            match self.calendars.restore_calendar(gid, id).await {
                Ok(()) | Err(rustical_store::Error::NotFound) => {}
                Err(e) => return Err(e.into()),
            }
        }
        for id in &c.addressbooks {
            groups::renew(self, gid, &c.claim).await?;
            match self.addressbooks.restore_addressbook(gid, id).await {
                Ok(()) | Err(rustical_store::Error::NotFound) => {}
                Err(e) => return Err(e.into()),
            }
        }
        groups::delete_claimed(self, gid, &c.claim).await
    }

    /// Purges a trashed group: with `actor` (a former member, any time; the same right as restore), without
    /// (`purge_expired`: only past the retention). A group that lives again (has members) is not purged: only
    /// its stale row goes. Otherwise ALL its own collections, live and trashed, then the principal (which
    /// ends every share to and of it), then the row; every step renews the claim.
    pub async fn purge_trashed_group(&self, gid: &str, actor: Option<&Principal>) -> AppResult<()> {
        use crate::domain::groups;
        let older_than = actor.is_none().then(|| crate::clock::now() - chrono::Duration::days(i64::from(self.config.trash_retention_days)));
        let Some(c) = groups::claim(self, gid, "purge", older_than).await? else {
            return Err(if groups::exists(self, gid).await? { AppError::conflict("err-group-busy") } else { AppError::NotFound });
        };
        match self.purge_steps(gid, actor, &c).await {
            Ok(()) => Ok(()),
            Err(e) => {
                groups::release_failed(self, gid, &c.claim).await;
                Err(e)
            }
        }
    }

    async fn purge_steps(&self, gid: &str, actor: Option<&Principal>, c: &crate::domain::groups::Claimed) -> AppResult<()> {
        use crate::domain::groups;
        if let Some(actor) = actor
            && !groups::may_act(self, gid, &c.members, actor).await?
        {
            return Err(AppError::Forbidden);
        }
        if !self.principals.list_members(gid).await?.is_empty() {
            tracing::warn!("trashed group {gid} has members again: only its stale trash entry is removed");
            return groups::delete_claimed(self, gid, &c.claim).await;
        }
        let mut cals: Vec<String> = self.calendars.get_calendars(gid).await?.into_iter().map(|x| x.id).collect();
        cals.extend(self.calendars.get_deleted_calendars(gid).await?.into_iter().map(|x| x.id));
        for id in &cals {
            groups::renew(self, gid, &c.claim).await?;
            match self.calendars.delete_calendar(gid, id, false).await {
                Ok(()) | Err(rustical_store::Error::NotFound) => {}
                Err(e) => return Err(e.into()),
            }
        }
        let mut abs: Vec<String> = self.addressbooks.get_addressbooks(gid).await?.into_iter().map(|x| x.id).collect();
        abs.extend(self.addressbooks.get_deleted_addressbooks(gid).await?.into_iter().map(|x| x.id));
        for id in &abs {
            groups::renew(self, gid, &c.claim).await?;
            match self.addressbooks.delete_addressbook(gid, id, false).await {
                Ok(()) | Err(rustical_store::Error::NotFound) => {}
                Err(e) => return Err(e.into()),
            }
        }
        groups::renew(self, gid, &c.claim).await?;
        match self.principals.remove_principal(gid).await {
            Ok(()) | Err(rustical_store::Error::NotFound) => {}
            Err(e) => return Err(e.into()),
        }
        groups::delete_claimed(self, gid, &c.claim).await
    }

    /// The stores themselves, for tests that set up data without a user.
    #[cfg(test)]
    pub fn raw(&self) -> Raw<'_> {
        Raw { calendars: &self.calendars, addressbooks: &self.addressbooks }
    }
}

#[cfg(test)]
pub struct Raw<'a> {
    pub calendars: &'a SqliteCalendarStore,
    pub addressbooks: &'a SqliteAddressbookStore,
}

/// A collection a user may use as the path `(home, name)` names it: RustiCal's [`Resolved`] (owner, id, own or
/// a share and its access and policy) after the user's access to the home was checked. Only [`Collections`]
/// makes one, so holding one is the proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Authorized {
    resolved: Resolved,
    home: String,
    name: String,
}

impl std::hash::Hash for ResolvedKindKey<'_> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self.0 {
            ResolvedKind::Own => 0u8.hash(state),
            ResolvedKind::Copy { access, policy } => {
                (1u8, access.as_str(), policy.private, policy.confidential).hash(state);
            }
        }
    }
}

/// [`ResolvedKind`] as a cache key part
struct ResolvedKindKey<'a>(&'a ResolvedKind);

impl Authorized {
    /// The principal the collection belongs to
    pub fn owner(&self) -> &str {
        self.resolved.owner()
    }
    /// The collection's id at its owner
    pub fn id(&self) -> &str {
        self.resolved.id()
    }
    /// The home of the path (the owner for an own collection, the sharee for a share)
    pub fn home(&self) -> &str {
        &self.home
    }
    /// The collection's name in the home
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn kind(&self) -> &ResolvedKind {
        self.resolved.kind()
    }
    pub fn is_own(&self) -> bool {
        self.resolved.is_own()
    }
    pub fn allows(&self, need: Need) -> bool {
        self.resolved.allows(need)
    }
    /// The access of a share; `None` for an own collection
    pub fn access(&self) -> Option<Access> {
        match self.kind() {
            ResolvedKind::Own => None,
            ResolvedKind::Copy { access, .. } => Some(*access),
        }
    }
    /// The visibility policy that shapes what this path shows; `None` for an own collection (everything)
    pub fn policy(&self) -> Option<Policy> {
        match self.kind() {
            ResolvedKind::Own => None,
            ResolvedKind::Copy { policy, .. } => Some(*policy),
        }
    }
    /// The level an object has here ([`FULL_LEVEL`] in an own collection)
    pub fn level(&self, object: &CalendarObject) -> u8 {
        rustical_store::shared::level(&self.resolved, object)
    }
    /// A cache key part for what this path shows: `(owner, id)` + view + `(home, name)`
    pub fn cache_key(&self) -> String {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        ResolvedKindKey(self.kind()).hash(&mut h);
        format!("{}\u{1f}{}\u{1f}{:x}\u{1f}{}\u{1f}{}", self.owner(), self.id(), h.finish(), self.home, self.name)
    }

    /// An own collection for unit tests that need no store
    #[cfg(test)]
    pub fn test_own(owner: &str, id: &str) -> Self {
        Self { resolved: Resolved::for_tests(owner.to_string(), id.to_string(), ResolvedKind::Own), home: owner.to_string(), name: id.to_string() }
    }

    fn checked(&self, need: Need) -> AppResult<()> {
        if self.allows(need) { Ok(()) } else { Err(AppError::Forbidden) }
    }
}

/// A store error of a write as the user reads it: the rules of a share (422 with its message, so the editor's
/// writer does not offer "keep mine"), a changed object (409, as an ETag conflict), a refused share (400).
fn write_error(e: rustical_store::Error) -> AppError {
    use rustical_store::Error as E;
    let rejected = |key: &'static str| AppError::Rejected(StatusCode::UNPROCESSABLE_ENTITY, Msg { key, args: vec![] });
    match e {
        E::CopyRule(CopyRule::TrashHref) => rejected("err-share-trash-href"),
        E::CopyRule(CopyRule::ForeignUid) => rejected("err-share-foreign-uid"),
        E::CopyRule(CopyRule::UidChanged) => rejected("err-share-uid-changed"),
        E::CopyRule(CopyRule::Level) => rejected("err-share-level"),
        E::PreconditionFailed => AppError::conflict("err-changed-event"),
        E::Forbidden => AppError::Forbidden,
        E::ShareRejected(r) => AppError::bad(match r {
            rustical_store::ShareRejection::Owner => "err-share-owner",
            rustical_store::ShareRejection::OwnerMember => "err-share-owner-member",
            rustical_store::ShareRejection::UnknownSharee => "err-share-unknown",
            rustical_store::ShareRejection::AmbiguousSharee => "err-share-ambiguous",
            rustical_store::ShareRejection::NotShareable => "err-share-not-shareable",
        }),
        E::InvalidVisibility => AppError::bad("err-visibility"),
        other => AppError::Store(other),
    }
}

/// Calendars, address books, their objects and their shares — always for a user ([`Collections::calendars`],
/// [`Collections::ensure_calendar`]) or for an [`Authorized`] collection.
pub struct Collections<'a>(&'a AppState);

impl Collections<'_> {
    fn authorized(resolved: Resolved, home: &str, name: &str) -> Authorized {
        Authorized { resolved, home: home.to_string(), name: name.to_string() }
    }

    /// The user's calendars: their own, their groups', and those shared with them or a group of theirs. With
    /// `deleted`, the trashed own ones (a share of a trashed calendar does not exist for its sharees).
    pub async fn calendars(&self, user: &Principal, deleted: bool) -> AppResult<Vec<(Authorized, Calendar)>> {
        let s = self.0;
        let mut out = vec![];
        for home in user.memberships() {
            if deleted {
                for c in s.calendars.get_deleted_calendars(home).await? {
                    let (r, c) = access::resolve_calendar(&*s.shares, &*s.calendars, home, &c.id.clone(), true).await?;
                    out.push((Self::authorized(r, home, &c.id), c));
                }
                continue;
            }
            for (r, c) in access::own_calendars(&*s.calendars, home).await? {
                out.push((Self::authorized(r, home, &c.id), c));
            }
            for (name, r, c) in access::shared_calendars(&*s.shares, &*s.calendars, home).await? {
                out.push((Self::authorized(r, home, &name), c));
            }
        }
        Ok(out)
    }

    /// The user's address books; see [`Self::calendars`].
    pub async fn addressbooks(&self, user: &Principal, deleted: bool) -> AppResult<Vec<(Authorized, Addressbook)>> {
        let s = self.0;
        let mut out = vec![];
        for home in user.memberships() {
            if deleted {
                for a in s.addressbooks.get_deleted_addressbooks(home).await? {
                    let (r, a) = access::resolve_addressbook(&*s.shares, &*s.addressbooks, home, &a.id.clone(), true).await?;
                    out.push((Self::authorized(r, home, &a.id), a));
                }
                continue;
            }
            for (r, a) in access::own_addressbooks(&*s.addressbooks, home).await? {
                out.push((Self::authorized(r, home, &a.id), a));
            }
            for (name, r, a) in access::shared_addressbooks(&*s.shares, &*s.addressbooks, home).await? {
                out.push((Self::authorized(r, home, &name), a));
            }
        }
        Ok(out)
    }

    /// The number of own calendars and address books of `home` (onboarding facts; no content).
    pub async fn own_counts(&self, home: &str) -> (usize, usize) {
        let s = self.0;
        (s.calendars.get_calendars(home).await.map_or(0, |v| v.len()), s.addressbooks.get_addressbooks(home).await.map_or(0, |v| v.len()))
    }

    /// The calendar `(home, name)` for a request of `user` that needs `need`: 403 if the user may not use the
    /// home or the calendar grants less, 404 if there is none. `show_deleted` finds an own one in the trash.
    pub async fn ensure_calendar(&self, user: &Principal, home: &str, name: &str, need: Need, show_deleted: bool) -> AppResult<(Authorized, Calendar)> {
        if !access::may_use_home(user, home) {
            return Err(AppError::Forbidden);
        }
        let s = self.0;
        let (r, c) = access::resolve_calendar(&*s.shares, &*s.calendars, home, name, show_deleted).await?;
        let a = Self::authorized(r, home, name);
        a.checked(need)?;
        Ok((a, c))
    }

    /// The address book `(home, name)`; see [`Self::ensure_calendar`].
    pub async fn ensure_addressbook(&self, user: &Principal, home: &str, name: &str, need: Need, show_deleted: bool) -> AppResult<(Authorized, Addressbook)> {
        if !access::may_use_home(user, home) {
            return Err(AppError::Forbidden);
        }
        let s = self.0;
        let (r, a) = access::resolve_addressbook(&*s.shares, &*s.addressbooks, home, name, show_deleted).await?;
        let auth = Self::authorized(r, home, name);
        auth.checked(need)?;
        Ok((auth, a))
    }

    /// An own calendar without a user: for work a user started that runs later on that user's behalf (a
    /// PractiCalUploader device of its owner, a remote sync link of its user), checked by its caller.
    pub async fn own_calendar(&self, owner: &str, id: &str, show_deleted: bool) -> AppResult<(Authorized, Calendar)> {
        let s = self.0;
        let (r, c) = access::resolve_calendar(&*s.shares, &*s.calendars, owner, id, show_deleted).await?;
        if !r.is_own() {
            return Err(AppError::NotFound);
        }
        Ok((Self::authorized(r, owner, id), c))
    }

    /// An own address book without a user; see [`Self::own_calendar`].
    pub async fn own_addressbook(&self, owner: &str, id: &str, show_deleted: bool) -> AppResult<(Authorized, Addressbook)> {
        let s = self.0;
        let (r, a) = access::resolve_addressbook(&*s.shares, &*s.addressbooks, owner, id, show_deleted).await?;
        if !r.is_own() {
            return Err(AppError::NotFound);
        }
        Ok((Self::authorized(r, owner, id), a))
    }

    /// The ids of the live own calendars and address books of `home` (a group's trash; ids only).
    pub async fn own_ids(&self, home: &str) -> AppResult<(Vec<String>, Vec<String>)> {
        let s = self.0;
        Ok((
            s.calendars.get_calendars(home).await?.into_iter().map(|c| c.id).collect(),
            s.addressbooks.get_addressbooks(home).await?.into_iter().map(|a| a.id).collect(),
        ))
    }

    /// The calendars an account for the group home would show: its own and those shared with it (Apple
    /// devices reach both only through a `user$group` account; `groups::GroupInfo::needs_cal_account`).
    pub async fn group_calendar_count(&self, gid: &str) -> AppResult<usize> {
        let s = self.0;
        Ok(s.calendars.get_calendars(gid).await?.len() + s.shares.calendars_shared_with(gid).await?.len())
    }

    /// The collection behind a path as the change history keys it: `(owner, id)` for a share, the path itself
    /// otherwise (no content; the write that names the path was checked by its route).
    pub async fn history_key(&self, addressbook: bool, home: &str, name: &str) -> (String, String) {
        let s = self.0;
        let target = if addressbook {
            s.shares.get_addressbook_share(home, name).await.ok().flatten().map(|x| (x.owner, x.addressbook_id))
        } else {
            s.shares.get_calendar_share(home, name).await.ok().flatten().map(|x| (x.owner, x.cal_id))
        };
        target.unwrap_or_else(|| (home.to_string(), name.to_string()))
    }

    /// The homes and names a collection is shared to, without content (SSE fans a change out to them).
    pub async fn copy_homes(&self, kind: &str, owner: &str, id: &str) -> AppResult<Vec<(String, String)>> {
        let s = self.0;
        Ok(if kind == "addressbook" {
            s.shares.list_addressbook_shares(owner, id).await?.into_iter().map(|x| (x.sharee, x.name)).collect()
        } else {
            s.shares.list_calendar_shares(owner, id).await?.into_iter().map(|x| (x.sharee, x.name)).collect()
        })
    }

    // --- objects ---

    pub async fn objects(&self, a: &Authorized) -> AppResult<Vec<(String, CalendarObject)>> {
        Ok(rustical_store::shared::objects(&*self.0.calendars, &a.resolved).await?)
    }

    pub async fn object(&self, a: &Authorized, object_id: &str) -> AppResult<CalendarObject> {
        Ok(rustical_store::shared::object(&*self.0.calendars, &a.resolved, object_id, false).await?)
    }

    pub async fn sync_changes(&self, a: &Authorized, token: i64) -> AppResult<(Vec<(String, CalendarObject)>, Vec<String>, i64)> {
        Ok(rustical_store::shared::sync_changes(&*self.0.calendars, &a.resolved, token).await?)
    }

    pub async fn query(&self, a: &Authorized, q: CalendarQuery) -> AppResult<Vec<(String, CalendarObject)>> {
        Ok(rustical_store::shared::query(&*self.0.calendars, &a.resolved, q).await?)
    }

    /// The calendar's current synctoken (an own calendar's, also the owner's for a share)
    pub async fn calendar_token(&self, a: &Authorized) -> AppResult<i64> {
        Ok(self.0.calendars.get_calendar(a.owner(), a.id(), false).await?.synctoken)
    }

    /// Writes an object (module doc of `rustical_store::shared`: the rules of a share apply in one transaction).
    /// `etag`: the version the writer based it on (`None`: no condition). Returns the ETag of what the writer
    /// sees now if that is what it sent.
    pub async fn put_object(&self, a: &Authorized, object_id: &str, object: CalendarObject, overwrite: bool, etag: Option<&str>) -> AppResult<Option<String>> {
        a.checked(Need::Write)?;
        let if_match = etag.and_then(|e| e.parse::<rustical_store::shared::ETag>().ok()).map(IfMatch::from);
        let pre = Preconditions { if_match: if_match.as_ref(), if_none_match: None };
        rustical_store::shared::put_object(&*self.0.calendars, &*self.0.shares, &a.resolved, object_id, object, overwrite, pre).await.map_err(write_error)
    }

    pub async fn put_objects(&self, a: &Authorized, objects: Vec<(String, CalendarObject)>, overwrite: bool) -> AppResult<()> {
        a.checked(Need::Write)?;
        rustical_store::shared::put_objects(&*self.0.calendars, &*self.0.shares, &a.resolved, objects, overwrite).await.map_err(write_error)
    }

    /// Into the trash bin (always, through a share)
    pub async fn delete_object(&self, a: &Authorized, object_id: &str, etag: Option<&str>) -> AppResult<()> {
        a.checked(Need::Write)?;
        let if_match = etag.and_then(|e| e.parse::<rustical_store::shared::ETag>().ok()).map(IfMatch::from);
        let pre = Preconditions { if_match: if_match.as_ref(), if_none_match: None };
        rustical_store::shared::delete_object(&*self.0.calendars, &*self.0.shares, &a.resolved, object_id, true, pre).await.map_err(write_error)
    }

    pub async fn address_objects(&self, a: &Authorized) -> AppResult<Vec<(String, AddressObject)>> {
        Ok(rustical_store::shared::address_objects(&*self.0.addressbooks, &a.resolved).await?)
    }

    pub async fn address_object(&self, a: &Authorized, object_id: &str) -> AppResult<AddressObject> {
        Ok(rustical_store::shared::address_object(&*self.0.addressbooks, &a.resolved, object_id, false).await?)
    }

    pub async fn address_sync_changes(&self, a: &Authorized, token: i64) -> AppResult<(Vec<(String, AddressObject)>, Vec<String>, i64)> {
        Ok(rustical_store::shared::address_sync_changes(&*self.0.addressbooks, &a.resolved, token).await?)
    }

    pub async fn put_address_object(&self, a: &Authorized, object_id: &str, object: AddressObject, overwrite: bool, etag: Option<&str>) -> AppResult<Option<String>> {
        a.checked(Need::Write)?;
        let if_match = etag.and_then(|e| e.parse::<rustical_store::shared::ETag>().ok()).map(IfMatch::from);
        let pre = Preconditions { if_match: if_match.as_ref(), if_none_match: None };
        rustical_store::shared::put_address_object(&*self.0.addressbooks, &*self.0.shares, &a.resolved, object_id, object, overwrite, pre).await.map_err(write_error)
    }

    pub async fn delete_address_object(&self, a: &Authorized, object_id: &str) -> AppResult<()> {
        a.checked(Need::Write)?;
        rustical_store::shared::delete_address_object(&*self.0.addressbooks, &*self.0.shares, &a.resolved, object_id, true, Preconditions::default()).await.map_err(write_error)
    }

    // --- collections themselves ---

    /// Creates a calendar in `cal.principal`, which the user must be (or be a member of).
    pub async fn create_calendar(&self, user: &Principal, cal: Calendar) -> AppResult<()> {
        if !access::may_use_home(user, &cal.principal) {
            return Err(AppError::Forbidden);
        }
        Ok(self.0.calendars.insert_calendar(cal).await?)
    }

    /// Creates an address book in `ab.principal`; see [`Self::create_calendar`].
    pub async fn create_addressbook(&self, user: &Principal, ab: Addressbook) -> AppResult<()> {
        if !access::may_use_home(user, &ab.principal) {
            return Err(AppError::Forbidden);
        }
        Ok(self.0.addressbooks.insert_addressbook(ab).await?)
    }

    /// Imports a calendar into the user's `cal.principal` (the store merges into an existing one when asked).
    pub async fn import_calendar(&self, user: &Principal, cal: Calendar, objects: Vec<CalendarObject>, merge: bool) -> AppResult<()> {
        if !access::may_use_home(user, &cal.principal) {
            return Err(AppError::Forbidden);
        }
        Ok(self.0.calendars.import_calendar(cal, objects, merge).await?)
    }

    /// Creates an address book with cards in the user's `ab.principal` in one transaction.
    pub async fn import_addressbook(&self, user: &Principal, ab: Addressbook, objects: Vec<(String, AddressObject)>) -> AppResult<()> {
        if !access::may_use_home(user, &ab.principal) {
            return Err(AppError::Forbidden);
        }
        Ok(self.0.addressbooks.import_addressbook(ab, objects, false).await?)
    }

    /// Changes name, colour, description. For a share only the sharee's own name and colour (its view).
    pub async fn update_calendar(&self, a: &Authorized, name: Option<String>, color: Option<String>, description: Option<String>) -> AppResult<()> {
        let s = self.0;
        if a.is_own() {
            let mut c = s.calendars.get_calendar(a.owner(), a.id(), false).await?;
            if let Some(n) = name {
                c.meta.displayname = Some(n);
            }
            if let Some(col) = color {
                c.meta.color = Some(col).filter(|c| !c.is_empty());
            }
            if let Some(d) = description {
                c.meta.description = Some(d).filter(|d| !d.is_empty());
            }
            return Ok(s.calendars.update_calendar(a.owner(), a.id(), c).await?);
        }
        let update = ShareViewUpdate { displayname: name.map(|n| Some(n).filter(|n| !n.is_empty())), color: color.map(|c| Some(c).filter(|c| !c.is_empty())), order: None };
        Ok(s.shares.update_calendar_share_view(a.home(), a.name(), update).await?)
    }

    /// Changes name, colour, description of an address book (the colour is PractiCal's own: for a share the
    /// sharee's, in its share row).
    pub async fn update_addressbook(&self, a: &Authorized, name: Option<String>, color: Option<String>, description: Option<String>) -> AppResult<()> {
        let s = self.0;
        if a.is_own() {
            let mut ab = s.addressbooks.get_addressbook(a.owner(), a.id(), false).await?;
            if let Some(n) = name {
                ab.displayname = Some(n);
            }
            if let Some(d) = description {
                ab.description = Some(d).filter(|d| !d.is_empty());
            }
            return Ok(s.addressbooks.update_addressbook(a.owner(), a.id(), ab).await?);
        }
        let update = ShareViewUpdate { displayname: name.map(|n| Some(n).filter(|n| !n.is_empty())), color: color.map(|c| Some(c).filter(|c| !c.is_empty())), order: None };
        Ok(s.shares.update_addressbook_share_view(a.home(), a.name(), update).await?)
    }

    /// Own: into the trash (or `hard`: purged). A share: ends it for this home (the owner keeps everything).
    pub async fn delete_calendar(&self, a: &Authorized, hard: bool) -> AppResult<()> {
        let s = self.0;
        if a.is_own() {
            return Ok(s.calendars.delete_calendar(a.owner(), a.id(), !hard).await?);
        }
        Ok(s.shares.unshare_calendar(a.owner(), a.id(), a.home()).await?)
    }

    pub async fn delete_addressbook(&self, a: &Authorized, hard: bool) -> AppResult<()> {
        let s = self.0;
        if a.is_own() {
            return Ok(s.addressbooks.delete_addressbook(a.owner(), a.id(), !hard).await?);
        }
        Ok(s.shares.unshare_addressbook(a.owner(), a.id(), a.home()).await?)
    }

    /// Own collections only.
    pub async fn restore_calendar(&self, a: &Authorized) -> AppResult<()> {
        a.checked(Need::Own)?;
        Ok(self.0.calendars.restore_calendar(a.owner(), a.id()).await?)
    }

    pub async fn restore_addressbook(&self, a: &Authorized) -> AppResult<()> {
        a.checked(Need::Own)?;
        Ok(self.0.addressbooks.restore_addressbook(a.owner(), a.id()).await?)
    }

    // --- sharing ---

    pub async fn calendar_shares(&self, a: &Authorized) -> AppResult<Vec<CalendarShare>> {
        a.checked(Need::Own)?;
        Ok(self.0.shares.list_calendar_shares(a.owner(), a.id()).await?)
    }

    pub async fn addressbook_shares(&self, a: &Authorized) -> AppResult<Vec<AddressbookShare>> {
        a.checked(Need::Own)?;
        Ok(self.0.shares.list_addressbook_shares(a.owner(), a.id()).await?)
    }

    /// Shares (or changes the access of a share) with `sharee` (an id or an e-mail address); `actor` did it.
    pub async fn share_calendar(&self, a: &Authorized, sharee: &str, access: Access, actor: &Principal) -> AppResult<CalendarShare> {
        a.checked(Need::Own)?;
        self.0.shares.share_calendar(a.owner(), a.id(), sharee, access, Some(&actor.id)).await.map_err(write_error)
    }

    pub async fn share_addressbook(&self, a: &Authorized, sharee: &str, access: Access, actor: &Principal) -> AppResult<AddressbookShare> {
        a.checked(Need::Own)?;
        self.0.shares.share_addressbook(a.owner(), a.id(), sharee, access, Some(&actor.id)).await.map_err(write_error)
    }

    /// A sharee's own colour of a shared address book (`None`: the owner's, or an own one)
    pub async fn addressbook_share_color(&self, a: &Authorized) -> AppResult<Option<String>> {
        if a.is_own() {
            return Ok(None);
        }
        Ok(self.0.shares.get_addressbook_share(a.home(), a.name()).await?.and_then(|s| s.color))
    }

    pub async fn unshare_calendar(&self, a: &Authorized, sharee: &str) -> AppResult<()> {
        a.checked(Need::Own)?;
        Ok(self.0.shares.unshare_calendar(a.owner(), a.id(), sharee).await?)
    }

    pub async fn unshare_addressbook(&self, a: &Authorized, sharee: &str) -> AppResult<()> {
        a.checked(Need::Own)?;
        Ok(self.0.shares.unshare_addressbook(a.owner(), a.id(), sharee).await?)
    }

    /// The principal an input names (its id, or an exact e-mail address), as the share store will resolve it.
    pub async fn resolve_sharee(&self, input: &str) -> AppResult<String> {
        let input = input.trim();
        let by_id = self.0.principals.get_principal(input).await?.map(|p| p.id);
        let by_email = self.0.shares.resolve_email(input).await?;
        match (by_id, by_email) {
            (Some(id), Some(other)) if id != other => Err(write_error(rustical_store::ShareRejection::AmbiguousSharee.into())),
            (Some(id), _) | (None, Some(id)) => Ok(id),
            (None, None) => Err(write_error(rustical_store::ShareRejection::UnknownSharee.into())),
        }
    }

    /// How objects of `owner` with CLASS:PRIVATE/CONFIDENTIAL appear in its shares (no content: readable by all).
    pub async fn visibility(&self, owner: &str) -> AppResult<Policy> {
        Ok(self.0.shares.get_visibility(owner).await?)
    }

    /// Sets the policy of `owner`, which `user` must be or be a member of.
    pub async fn set_visibility(&self, user: &Principal, owner: &str, policy: Policy) -> AppResult<()> {
        if !access::may_use_home(user, owner) {
            return Err(AppError::Forbidden);
        }
        self.0.shares.set_visibility(owner, policy).await.map_err(write_error)
    }
}
