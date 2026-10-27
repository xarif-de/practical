//! Importing from, and keeping in sync with, a remote CalDAV/CardDAV server. Nextcloud is the
//! first; the protocol half is standard, so only signing in differs per provider.
//!
//! Two shapes, one client ([`dav`]):
//! - **import** runs once, through the existing `domain::transfer::import_new_*`, and stores no
//!   credential at all;
//! - **sync** keeps a link ([`Link`]) and runs rounds ([`round`]). Direction `both` or `pull`.
//!
//! The credential sits on the ACCOUNT, not the link: one browser login lets the user tick several
//! collections, and revoking one link must not break its siblings.
//!
//! The server address is typed in by the user and required. What keeps that safe is the resolver
//! ([`PublicOrAllowed`]): a name must resolve to a public address, so the field cannot be used to
//! reach the pod's neighbours. A deployment's own Nextcloud may resolve privately from inside the
//! cluster, which is what `PRACTICAL__REMOTE_PRIVATE_ORIGINS` exempts — configuration, not input.
//!
//! Every protocol decision here was measured against nextcloud 35.0.1 (`e2e/nextcloud-dev.sh`), and
//! each measurement sits next to the thing it decided: in [`dav::offerable`] for which collections
//! may be offered, in [`round`] for the order of a round and the two echoes, in [`seal`] for why a
//! stored credential is encrypted, and in `tests.rs`, whose fixtures are real answers.

pub mod dav;
pub mod login;
pub mod round;
pub mod sched;
pub mod take;
pub mod seal;
pub mod xml;

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use dav::{Dav, Kind, Secret};
use rustical_store::auth::Principal;
use sqlx::Row;

/// Caps, so a handful of links cannot turn into a request storm against a server that rate-limits.
pub const MAX_LINKS_PER_USER: usize = 8;
pub const MAX_LINKS_TOTAL: usize = 64;
/// The scheduler's floor; "sync now" bypasses it and is itself rate-limited.
pub const MIN_EVERY_MIN: i64 = 5;
pub const MAX_EVERY_MIN: i64 = 1440;
pub const DEFAULT_EVERY_MIN: i64 = 5;
/// A full reconciliation this often regardless of tokens: the server-agnostic backstop for
/// anything a token cannot tell us.
pub const FULL_EVERY_HOURS: i64 = 24;
/// A round lease older than this is from a process that died; PractiCal is one pod, so the
/// in-process mutex is what actually excludes concurrent rounds and this only matters on restart.
pub const LEASE_MINUTES: i64 = 10;

#[derive(Debug, Clone)]
pub struct Account {
    pub id: String,
    pub principal: String,
    pub base_url: String,
    pub login: String,
}

#[derive(Debug, Clone)]
pub struct Link {
    pub id: String,
    pub account_id: String,
    /// the user who made the link: account, credential, rights and the settings rows are theirs
    pub principal: String,
    /// whose the local collection is: `principal` itself or a group they are in (migration 20)
    pub owner: String,
    pub kind: Kind,
    pub local_id: String,
    pub href: String,
    pub two_way: bool,
    pub every_min: i64,
    pub sync_token: Option<String>,
    pub local_token: i64,
    pub full_at: Option<String>,
    /// when a full pass last RAN, clean or not: a permanently unreadable object must not hold the
    /// link in a full pass on every tick
    pub ran_full_at: Option<String>,
    pub ran_at: Option<String>,
    pub running_since: Option<String>,
    pub err: Option<String>,
}

fn row_to_link(r: &sqlx::sqlite::SqliteRow) -> AppResult<Link> {
    Ok(Link {
        id: r.get("id"),
        account_id: r.get("account_id"),
        principal: r.get("principal"),
        owner: r.get("owner"),
        kind: Kind::parse(r.get("kind"))?,
        local_id: r.get("local_id"),
        href: r.get("href"),
        two_way: r.get::<String, _>("direction") == "both",
        every_min: r.get("every_min"),
        sync_token: r.get("sync_token"),
        local_token: r.get("local_token"),
        full_at: r.get("full_at"),
        ran_full_at: r.get("ran_full_at"),
        ran_at: r.get("ran_at"),
        running_since: r.get("running_since"),
        err: r.get("err"),
    })
}

/// The resolver every remote request goes through: a name must resolve to a PUBLIC address, or the
/// request is refused before a connection is opened.
///
/// This is what makes a typed-in server address safe. Without it the server would dial whatever it
/// is told — including its own neighbours: in a cluster the pod reaches the API server, the
/// database and every other service, and a name under someone else's control can point at any of
/// them. That is a server-side request forgery, and the address field would be the way in.
///
/// `allow`: host names from `PRACTICAL__REMOTE_PRIVATE_ORIGINS`, exempt from the check. Needed
/// because a deployment's OWN Nextcloud can resolve to a private address from inside the cluster
/// (`my.xarif.de` resolves to Traefik's ClusterIP there), so without the exemption the one server
/// the instance exists for would be the one it cannot reach. The list is configuration, never user
/// input, so no request can ask for the exemption.
#[derive(Clone)]
struct PublicOrAllowed {
    allow: std::sync::Arc<Vec<String>>,
}

impl reqwest::dns::Resolve for PublicOrAllowed {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_string();
        let allow = self.allow.clone();
        Box::pin(async move {
            let all: Vec<std::net::SocketAddr> = tokio::net::lookup_host((host.as_str(), 0)).await?.collect();
            if allow.iter().any(|h| h == &host) {
                return Ok(Box::new(all.into_iter()) as reqwest::dns::Addrs);
            }
            let public: Vec<std::net::SocketAddr> = all.into_iter().filter(|a| crate::domain::push::public_ip(a.ip())).collect();
            if public.is_empty() {
                // this text is logged; the user is told `err-remote-private`
                return Err(format!("{host}: no public address").into());
            }
            Ok(Box::new(public.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

/// The HTTP client for remote servers. Separate from the push client because the timeouts differ by
/// an order of magnitude: a 500-object multiget does not fit in push's 10 s.
///
/// `redirect::Policy::none()` — discovery follows its single `.well-known` hop by hand, bounded to
/// the same origin ([`Dav::discover`]).
pub fn client(private_origins: &[String]) -> reqwest::Client {
    let allow = exempt_hosts(private_origins);
    reqwest::Client::builder()
        /* Measured against nextcloud 35.0.1: the PROPFIND that lists a home answered 10 963 bytes
           plain and 853 gzipped, and every round of every link makes requests of this shape. The
           cap in `dav::send` counts DECOMPRESSED bytes, because the stream it reads is already
           decoded, so a compressed answer cannot smuggle more past it. */
        .gzip(true)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .dns_resolver(std::sync::Arc::new(PublicOrAllowed { allow: std::sync::Arc::new(allow) }))
        .connect_timeout(std::time::Duration::from_secs(5))
        .timeout(std::time::Duration::from_secs(60))
        .user_agent("PractiCal")
        .build()
        .expect("the remote client builds from constants")
}

/// The host names of the exempt origins: what [`PublicOrAllowed`] compares. A configured origin
/// carries a scheme and may carry a port, neither of which a resolved name has.
fn exempt_hosts(origins: &[String]) -> Vec<String> {
    origins.iter().filter_map(|o| o.parse::<url::Url>().ok()).filter_map(|u| u.host_str().map(str::to_string)).collect()
}

/// "both" or "pull", and nothing else. Anything unknown was a typo on the way in and used to mean
/// `both`, which is the direction that WRITES to the user's server — a default no mistake should
/// reach.
pub fn two_way_from(direction: &str) -> AppResult<bool> {
    match direction {
        "both" => Ok(true),
        "pull" => Ok(false),
        _ => Err(AppError::bad("err-remote-bad-direction")),
    }
}

/// The origin of a server address the user typed, or an error naming what is wrong with it.
///
/// Only the SHAPE is checked here. WHETHER it may be dialled is decided by the resolver above, at
/// connection time, when the name's real addresses are known — a check against the name alone could
/// not do it anyway, because DNS may answer differently a moment later.
///
/// - the field is required: an empty address is `err-remote-no-server`, so nothing starts without one
/// - `https` only; `http` is refused except for localhost in a development build, because a
///   credential and a whole calendar would otherwise travel in the clear
/// - no userinfo, no path, no query: `https://a@b/c?d` is not a server address, and taking one would
///   make the part that decides where we connect (the host) the part hardest to read
pub fn origin_of(state: &AppState, typed: &str) -> AppResult<String> {
    let typed = typed.trim();
    if typed.is_empty() {
        return Err(AppError::bad("err-remote-no-server"));
    }
    // "my.xarif.de" is what a user types; assuming https beats refusing it
    let with_scheme = if typed.contains("://") { typed.to_string() } else { format!("https://{typed}") };
    let u: url::Url = with_scheme.parse().map_err(|_| AppError::bad("err-remote-bad-server"))?;
    let host = u.host_str().filter(|h| !h.is_empty()).ok_or_else(|| AppError::bad("err-remote-bad-server"))?;
    let local = host == "localhost" || host == "127.0.0.1" || host == "::1";
    if u.scheme() != "https" && !(u.scheme() == "http" && local && state.config.dev_login.is_some()) {
        return Err(AppError::bad("err-remote-https"));
    }
    if !u.username().is_empty() || u.password().is_some() || u.query().is_some() || !matches!(u.path(), "" | "/") {
        return Err(AppError::bad("err-remote-bad-server"));
    }
    let origin = match u.port() {
        Some(p) => format!("{}://{host}:{p}", u.scheme()),
        None => format!("{}://{host}", u.scheme()),
    };
    /* An ADDRESS, not a name, never reaches the resolver: reqwest connects to an IP literal
       directly, so `PublicOrAllowed` is not consulted at all. Measured — `https://10.43.0.1`
       answered "timeout", which is what a cluster-internal service would answer only because
       nothing happened to listen there. So the same rule is applied here, where the address is
       already known, and the exemption list is honoured so an operator can still name one.
       `url::Host` is what parsed the host, so this costs no second parse of untrusted text. */
    let exempt = &state.config.remote_private_origins;
    if let url::Host::Ipv4(_) | url::Host::Ipv6(_) = u.host().ok_or_else(|| AppError::bad("err-remote-bad-server"))? {
        let ip: std::net::IpAddr = host.trim_matches(['[', ']']).parse().map_err(|_| AppError::bad("err-remote-bad-server"))?;
        if !crate::domain::push::public_ip(ip) && !exempt.contains(&origin) {
            return Err(AppError::bad("err-remote-private"));
        }
    }
    /* The resolver can only match a NAME — DNS carries no port — so an exemption would otherwise
       cover every port of that host, and the configuration says `scheme://host[:port]`. Here the
       port is known, so an address whose host is exempt must be the exempt ORIGIN itself.
       `config::remote_private_origins` normalises to the very format built above. */
    if exempt_hosts(exempt).iter().any(|h| h == host) && !exempt.contains(&origin) {
        return Err(AppError::bad("err-remote-private"));
    }
    Ok(origin)
}

/// Percent-encodes one path or form value. Same rule as `domain::geo::enc`; kept here so the
/// remote module has no cross-dependency on the geocoder for three lines.
pub fn pct(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    for b in v.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Sync needs a key to seal the app password; import stores none, so it needs nothing configured
/// and is always offered.
pub fn sync_enabled(state: &AppState) -> bool {
    state.config.remote_secret_key.is_some()
}

// ---------------------------------------------------------------- accounts

/// Finds or creates the account row for one granted login, and (re)seals the secret — a fresh login
/// for an account we already know replaces a revoked password without losing its links.
pub async fn upsert_account(state: &AppState, user: &Principal, origin: &str, login: &str, secret: &Secret) -> AppResult<String> {
    let key = state.config.remote_secret_key.as_deref().ok_or_else(|| AppError::bad("err-remote-no-key"))?;
    let existing: Option<String> = sqlx::query_scalar("SELECT id FROM practical_remote_accounts WHERE principal = ? AND base_url = ? AND login = ?")
        .bind(&user.id)
        .bind(origin)
        .bind(login)
        .fetch_optional(&state.db)
        .await?;
    let id = existing.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let sealed = seal::seal(key, &id, &user.id, &secret.0)?;
    sqlx::query(
        "INSERT INTO practical_remote_accounts (id, principal, base_url, login, secret, err) VALUES (?, ?, ?, ?, ?, NULL)
           ON CONFLICT(id) DO UPDATE SET secret = excluded.secret, err = NULL",
    )
    .bind(&id)
    .bind(&user.id)
    .bind(origin)
    .bind(login)
    .bind(&sealed)
    .execute(&state.db)
    .await?;
    Ok(id)
}

/// The account of a link, with its secret opened. Scoped by principal, so a link id alone never
/// reaches another user's credential.
pub async fn account_of(state: &AppState, link: &Link) -> AppResult<(Account, Secret)> {
    let key = state.config.remote_secret_key.as_deref().ok_or_else(|| AppError::bad("err-remote-no-key"))?;
    let r = sqlx::query("SELECT id, principal, base_url, login, secret FROM practical_remote_accounts WHERE id = ? AND principal = ?")
        .bind(&link.account_id)
        .bind(&link.principal)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;
    let sealed: Vec<u8> = r.get("secret");
    let a = Account { id: r.get("id"), principal: r.get("principal"), base_url: r.get("base_url"), login: r.get("login") };
    let s = seal::open(key, &a.id, &a.principal, &sealed)?;
    Ok((a, s))
}

/// A ready-to-use client for a link.
pub async fn dav_for(state: &AppState, link: &Link) -> AppResult<(Account, Dav)> {
    let (a, s) = account_of(state, link).await?;
    // the stored origin was checked when the link was made; the resolver checks every dial anyway
    let base: url::Url = a.base_url.parse().map_err(|_| AppError::bad("err-remote-bad-server"))?;
    Ok((a.clone(), Dav::new(state.remote_http.clone(), base, a.login.clone(), s)))
}

/// Marks every link of an account as blocked by a revoked password. One revoked secret invalidates
/// all of them, so it is reported once, on the account.
pub async fn account_failed(state: &AppState, account_id: &str, msg: &str) -> AppResult<()> {
    sqlx::query("UPDATE practical_remote_accounts SET err = ? WHERE id = ?").bind(msg).bind(account_id).execute(&state.db).await?;
    Ok(())
}

// ---------------------------------------------------------------- links

/// Every link of every user: what the background loop iterates. Only the loop calls this; a request
/// handler must use [`links_of`], which is scoped to its user.
pub async fn all_links(state: &AppState) -> AppResult<Vec<Link>> {
    sqlx::query("SELECT * FROM practical_remote_links ORDER BY ran_at IS NOT NULL, ran_at")
        .fetch_all(&state.db)
        .await?
        .iter()
        .map(row_to_link)
        .collect()
}

/// One link by id, without a user scope: for the background loop, which re-reads a link after
/// waiting for its lock. Never reachable from a request — those go through [`link_of`].
pub async fn link_by_id(state: &AppState, id: &str) -> AppResult<Link> {
    let r = sqlx::query("SELECT * FROM practical_remote_links WHERE id = ?").bind(id).fetch_optional(&state.db).await?.ok_or(AppError::NotFound)?;
    row_to_link(&r)
}

/// A user's links, each already carrying its account's error if there is one — a revoked app
/// password is recorded once, on the account, and every link of it is blocked by it.
pub async fn links_of(state: &AppState, user: &Principal) -> AppResult<Vec<Link>> {
    let rows = sqlx::query(
        "SELECT l.*, a.err AS account_err FROM practical_remote_links l
           JOIN practical_remote_accounts a ON a.id = l.account_id
         WHERE l.principal = ? ORDER BY l.kind, l.local_id",
    )
    .bind(&user.id)
    .fetch_all(&state.db)
    .await?;
    rows.iter()
        .map(|r| {
            let mut l = row_to_link(r)?;
            l.err = r.get::<Option<String>, _>("account_err").or(l.err);
            Ok(l)
        })
        .collect()
}

/// One link of THIS user. A foreign id answers NotFound (404, not 403) so ids are not enumerable.
pub async fn link_of(state: &AppState, user: &Principal, id: &str) -> AppResult<Link> {
    let r = sqlx::query("SELECT * FROM practical_remote_links WHERE id = ? AND principal = ?")
        .bind(id)
        .bind(&user.id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;
    row_to_link(&r)
}

/// The link of a local collection, if it has one. Used by the sync loop to decide whether a local
/// change is worth pushing.
///
/// It does NOT make a `pull` collection read-only: PractiCal's own write paths do not consult it,
/// and a third-party CalDAV client talking to RustiCal could not be stopped anyway. A local edit in
/// a `pull` collection is simply overwritten the next time that object changes remotely, and the
/// link row in the settings says the direction is one-way.
pub async fn link_for_collection(state: &AppState, owner: &str, kind: Kind, local_id: &str) -> AppResult<Option<Link>> {
    let r = sqlx::query("SELECT * FROM practical_remote_links WHERE owner = ? AND kind = ? AND local_id = ?")
        .bind(owner)
        .bind(kind.as_str())
        .bind(local_id)
        .fetch_optional(&state.db)
        .await?;
    r.as_ref().map(row_to_link).transpose()
}

/// Everything that can refuse a new link, checked BEFORE the local collection is created — an
/// error after that point would leave an empty calendar the user has to delete by hand. The UNIQUE
/// constraints still guard the race; this only turns the common cases into a message.
pub async fn room_for_a_link(state: &AppState, user: &Principal, account_id: &str, kind: Kind, href: &str) -> AppResult<()> {
    let mine: i64 = sqlx::query_scalar("SELECT count(*) FROM practical_remote_links WHERE principal = ?").bind(&user.id).fetch_one(&state.db).await?;
    let all: i64 = sqlx::query_scalar("SELECT count(*) FROM practical_remote_links").fetch_one(&state.db).await?;
    if mine as usize >= MAX_LINKS_PER_USER || all as usize >= MAX_LINKS_TOTAL {
        return Err(AppError::bad("err-remote-too-many"));
    }
    let taken: i64 = sqlx::query_scalar("SELECT count(*) FROM practical_remote_links WHERE account_id = ? AND kind = ? AND href = ?")
        .bind(account_id)
        .bind(kind.as_str())
        .bind(href)
        .fetch_one(&state.db)
        .await?;
    if taken > 0 {
        return Err(AppError::bad("err-remote-already-linked"));
    }
    Ok(())
}

/// `owner` was checked by the route and again when its collection was created.
#[allow(clippy::too_many_arguments)]
pub async fn create_link(state: &AppState, user: &Principal, owner: &str, account_id: &str, kind: Kind, local_id: &str, href: &str, two_way: bool, every_min: i64) -> AppResult<String> {
    room_for_a_link(state, user, account_id, kind, href).await?;
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO practical_remote_links (id, account_id, principal, owner, kind, local_id, href, direction, every_min, local_token)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 0)",
    )
    .bind(&id)
    .bind(account_id)
    .bind(&user.id)
    .bind(owner)
    .bind(kind.as_str())
    .bind(local_id)
    .bind(href)
    .bind(if two_way { "both" } else { "pull" })
    .bind(every_min.clamp(MIN_EVERY_MIN, MAX_EVERY_MIN))
    .execute(&state.db)
    .await?;
    Ok(id)
}

pub async fn set_link(state: &AppState, user: &Principal, id: &str, two_way: Option<bool>, every_min: Option<i64>) -> AppResult<()> {
    let link = link_of(state, user, id).await?;
    sqlx::query("UPDATE practical_remote_links SET direction = ?, every_min = ? WHERE id = ? AND principal = ?")
        .bind(if two_way.unwrap_or(link.two_way) { "both" } else { "pull" })
        .bind(every_min.unwrap_or(link.every_min).clamp(MIN_EVERY_MIN, MAX_EVERY_MIN))
        .bind(id)
        .bind(&user.id)
        .execute(&state.db)
        .await?;
    Ok(())
}

/// Removes a link. The local collection and its data stay. When this was the account's last link,
/// the app password is destroyed at Nextcloud and the account row goes with it — otherwise a
/// sibling link would stop working.
pub async fn delete_link(state: &AppState, user: &Principal, id: &str) -> AppResult<()> {
    let link = link_of(state, user, id).await?;
    drop_link(state, &link).await
}

/// [`delete_link`] without the caller check, for a link that has to go whoever asks: its collection
/// is being deleted, and on a group collection that may be any member, not the one who made it.
async fn drop_link(state: &AppState, link: &Link) -> AppResult<()> {
    let creds = account_of(state, link).await.ok();
    sqlx::query("DELETE FROM practical_remote_links WHERE id = ? AND principal = ?").bind(&link.id).bind(&link.principal).execute(&state.db).await?;
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM practical_remote_links WHERE account_id = ?").bind(&link.account_id).fetch_one(&state.db).await?;
    if left == 0 {
        if let Some((a, s)) = creds {
            login::revoke(&state.remote_http, &a.base_url, &a.login, &s).await;
        }
        sqlx::query("DELETE FROM practical_remote_accounts WHERE id = ? AND principal = ?").bind(&link.account_id).bind(&link.principal).execute(&state.db).await?;
    }
    Ok(())
}

/// Whether the link's user may still write its collection, checked before every round (the user is
/// read fresh: a membership is not part of the link). When not — they left the group, or the group
/// went to the bin, which lifts every membership (`groups::trash`) — the link ENDS here: its row is
/// shown only on a collection its user can see, so a link kept "paused" could be neither seen nor
/// stopped, and would hold its app password and a place under the link cap for good. One check
/// covers every way out of a group, the CLI's included.
pub async fn may_write(state: &AppState, link: &Link) -> AppResult<()> {
    use rustical_store::auth::AuthenticationProvider;
    if link.owner == link.principal {
        return Ok(());
    }
    if state.principals().get_principal(&link.principal).await?.is_some_and(|u| u.is_principal(&link.owner)) {
        return Ok(());
    }
    drop_link(state, link).await?;
    Err(AppError::Forbidden)
}

/// Ends the link of a collection that is being deleted, if it has one. A link lives on its
/// collection's row and page (`RemoteLinkRow`), so a link whose collection is gone would have no
/// way out while its rounds kept running. Called for the bin as well as for a purge: restoring a
/// collection brings back its entries, not its sign-in, which the user can set up again.
///
/// Never fails the deletion it accompanies: a remote that cannot be reached must not keep a user
/// from throwing a collection away. The app password is then left at the remote, where its own
/// security settings can revoke it — the same case as a page closed mid-import (`login::revoke`).
///
/// Found by the collection's OWNER, not by the caller: a group collection may be deleted by any
/// member, and the link then ends — with the app password of the member who made it, when it was
/// that account's last link — the same as for a collection of one's own.
pub async fn unlink_collection(state: &AppState, owner: &str, kind: dav::Kind, local_id: &str) {
    if let Ok(Some(link)) = link_for_collection(state, owner, kind, local_id).await
        && let Err(e) = drop_link(state, &link).await
    {
        tracing::warn!(link = %link.id, error = %e, "could not end the link of a deleted collection");
    }
}

/// How many addresses the field offers back. Three is what fits a dropdown without becoming a
/// list to read; the user can always type.
pub const SERVERS_OFFERED: usize = 3;

/// Remembers an address the user signed in at successfully. Only the address — the table holds no
/// credential, so this is safe for a one-off import too, which keeps nothing else.
pub async fn remember_server(state: &AppState, user: &Principal, origin: &str) {
    let r = sqlx::query(
        "INSERT INTO practical_remote_servers (principal, origin, used_at) VALUES (?, ?, ?)
           ON CONFLICT(principal, origin) DO UPDATE SET used_at = excluded.used_at",
    )
    .bind(&user.id)
    .bind(origin)
    .bind(crate::clock::now().to_rfc3339())
    .execute(&state.db)
    .await;
    if let Err(e) = r {
        // a suggestion list is a convenience: failing to write it must not fail the sign-in
        tracing::warn!(error = %e, "could not remember the remote server address");
    }
}

/// The addresses to offer, most recently used first.
pub async fn servers_of(state: &AppState, user: &Principal) -> AppResult<Vec<String>> {
    Ok(sqlx::query_scalar(
        "SELECT origin FROM practical_remote_servers WHERE principal = ? ORDER BY used_at DESC LIMIT ?",
    )
    .bind(&user.id)
    .bind(SERVERS_OFFERED as i64)
    .fetch_all(&state.db)
    .await?)
}

/// The account for an address the user already granted, if there is one. This is what lets a second
/// import or a second link skip the whole browser sign-in: the credential is already here and
/// already reaches this server, so asking for a new one buys nothing.
///
/// Only an account with a KEPT credential can be reused, which means one that carries a link. A
/// one-off import revokes its app password at the remote, so after one there is nothing to reuse —
/// that is the point of revoking it.
pub async fn account_for_origin(state: &AppState, user: &Principal, origin: &str) -> AppResult<Option<(Account, Secret)>> {
    let Some(key) = state.config.remote_secret_key.as_deref() else { return Ok(None) };
    let row = sqlx::query(
        "SELECT a.id, a.principal, a.base_url, a.login, a.secret
           FROM practical_remote_accounts a
          WHERE a.principal = ? AND a.base_url = ? AND a.err IS NULL
            AND EXISTS (SELECT 1 FROM practical_remote_links l WHERE l.account_id = a.id)
          LIMIT 1",
    )
    .bind(&user.id)
    .bind(origin)
    .fetch_optional(&state.db)
    .await?;
    let Some(r) = row else { return Ok(None) };
    let sealed: Vec<u8> = r.get("secret");
    let a = Account { id: r.get("id"), principal: r.get("principal"), base_url: r.get("base_url"), login: r.get("login") };
    // a secret that will not open is not a reason to refuse: the sign-in is still offered
    match seal::open(key, &a.id, &a.principal, &sealed) {
        Ok(s) => Ok(Some((a, s))),
        Err(e) => {
            tracing::warn!(account = %a.id, error = %e, "stored credential will not open; asking for a new sign-in");
            Ok(None)
        }
    }
}

// ---------------------------------------------------------------- object mapping

/// What the two sides last agreed on for one object.
#[derive(Debug, Clone)]
pub struct Mapped {
    pub object_id: String,
    pub uid: String,
    pub href: String,
    pub etag: String,
    /// fingerprint of the text RUSTICAL STORED, which is not the text either side sent
    pub mine: String,
}

pub async fn mapping(state: &AppState, link_id: &str) -> AppResult<Vec<Mapped>> {
    Ok(sqlx::query("SELECT object_id, uid, href, etag, mine FROM practical_remote_objects WHERE link_id = ?")
        .bind(link_id)
        .fetch_all(&state.db)
        .await?
        .iter()
        .map(|r| Mapped { object_id: r.get("object_id"), uid: r.get("uid"), href: r.get("href"), etag: r.get("etag"), mine: r.get("mine") })
        .collect())
}

/// Written immediately after the write it describes, in its own statement — never batched to the
/// end of a round. An abort then costs a re-read instead of a double push plus a 412 storm.
pub async fn map_put(state: &AppState, link_id: &str, m: &Mapped) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO practical_remote_objects (link_id, object_id, uid, href, etag, mine) VALUES (?, ?, ?, ?, ?, ?)
           ON CONFLICT(link_id, object_id) DO UPDATE SET uid = excluded.uid, href = excluded.href, etag = excluded.etag, mine = excluded.mine",
    )
    .bind(link_id)
    .bind(&m.object_id)
    .bind(&m.uid)
    .bind(&m.href)
    .bind(&m.etag)
    .bind(&m.mine)
    .execute(&state.db)
    .await?;
    Ok(())
}

pub async fn map_del(state: &AppState, link_id: &str, object_id: &str) -> AppResult<()> {
    sqlx::query("DELETE FROM practical_remote_objects WHERE link_id = ? AND object_id = ?").bind(link_id).bind(object_id).execute(&state.db).await?;
    Ok(())
}

/// How a remote body is parsed: the same options the file import uses
/// ([`crate::domain::transfer::options`]), so a calendar taken from a server and the same calendar
/// exported to a file are read by one rule.
pub fn parser_options() -> caldata::parser::ParserOptions {
    crate::domain::transfer::options()
}

/// The fingerprint RustiCal will hold for a text, which is what makes echo suppression work.
///
/// The two kinds differ and must not be unified: `CalendarObject::import` re-serialises from the
/// parse tree (so `get_etag` hashes UID + regenerated text), while `AddressObject` has NO `import`
/// and `from_vcf` caches its input. Both are applied to the text AFTER our own write-path
/// normalisation (`complete_ics` / `bare_params_as_type`), because that is what reaches the store.
pub fn fingerprint(kind: Kind, text: &str) -> AppResult<String> {
    Ok(match kind {
        Kind::Cal => {
            let normalised = crate::domain::transfer::complete_ics(text);
            rustical_ical::CalendarObject::import(&normalised, Some(parser_options()))?.get_etag()
        }
        Kind::Ab => {
            let normalised = crate::domain::contacts::bare_params_as_type(text);
            rustical_ical::AddressObject::from_vcf(normalised.into_owned())?.get_etag()
        }
    })
}

#[cfg(test)]
mod tests;
