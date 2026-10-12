//! One sync round for one link, and the rules that keep it from feeding itself.
//!
//! The order is fixed and each step exists for a measured reason:
//!
//! 1. **local first.** `sync_changes` hands back the collection's synctoken at read time, and THAT
//!    is the value stored at the end. Re-reading it after step 4 would skip every user edit made
//!    during the round, because applying pulled objects bumps the same counter.
//!    The synctoken is read BEFORE the change list, because the store's two statements are not one
//!    transaction: a write landing between them is already in the later token while not being in
//!    the list, and storing that token would hide it forever. Storing the earlier value re-reports
//!    it next round instead — at worst a second push, which echo suppression absorbs.
//! 2. **remote changes**, or a full listing when a token says it cannot be trusted.
//! 3. **resolve.** An object changed on both sides is resolved so that
//!    Nextcloud wins, the local version stays recoverable through the change history.
//! 4. **apply remote → local**, deletions before creates (an href move arrives as a 404 row and a
//!    200 row in one answer, and the wrong order would collide with the UID the deletion still
//!    holds), each write conditional on what is stored right now.
//! 5. **push local → remote**, conditionally.
//! 6. **store both tokens.**
//!
//! Two echoes have to be suppressed or the link runs forever on its own writes: a pulled object
//! bumps the local counter, and a pushed object bumps the remote one. `etag` catches the second,
//! `mine` the first, and an applied deletion is remembered for one round because a deletion has no
//! text to fingerprint.
//!
//! No network call happens inside a history guard: `history::begin` holds a process-wide mutex
//! until commit, so a guard spanning a round would block every user's writes for as long as
//! Nextcloud takes to answer.

use super::dav::{Changes, Dav, Kind, Wrote};
use super::{Link, Mapped};
use crate::domain::history_text::Store;
use crate::domain::history;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use rustical_store::{AddressbookReadStore, AddressbookWriteStore, CalendarReadStore, CalendarWriteStore};
use std::collections::{HashMap, HashSet};

/// What a round did, for the UI and the log.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub pulled: usize,
    pub pushed: usize,
    pub deleted_here: usize,
    pub deleted_there: usize,
    pub conflicts: usize,
    /// objects one side refused; counted and skipped, never retried in a loop
    pub refused: usize,
    /// of those, the LOCAL changes that did not reach the remote — the only reason to hold the
    /// local token back, because only they are still owed to the other side
    pub unpushed: usize,
    /// the remote counter was reset, so this round reconciled fully
    pub full: bool,
}

impl Report {
    pub fn wrote(&self) -> bool {
        self.pulled + self.pushed + self.deleted_here + self.deleted_there > 0
    }
}

/// The local side of a round: (object id, text) changed, and the ids deleted.
type LocalChanges = (Vec<(String, String)>, Vec<String>, i64);

async fn local_changes(state: &AppState, link: &Link) -> AppResult<LocalChanges> {
    match link.kind {
        Kind::Cal => {
            // the token first, for the reason in this module's header
            let before = state.calendars.get_calendar(&link.principal, &link.local_id, false).await?.synctoken;
            let (changed, gone, _) = state.calendars.sync_changes(&link.principal, &link.local_id, link.local_token).await?;
            Ok((changed.into_iter().map(|(id, o)| (id, o.get_ics().to_string())).collect(), gone, before))
        }
        Kind::Ab => {
            let before = state.addressbooks.get_addressbook(&link.principal, &link.local_id, false).await?.synctoken;
            let (changed, gone, _) = AddressbookReadStore::sync_changes(&*state.addressbooks, &link.principal, &link.local_id, link.local_token).await?;
            Ok((changed.into_iter().map(|(id, o)| (id, o.get_vcf().to_string())).collect(), gone, before))
        }
    }
}

/// The text RustiCal currently holds for one object, or None when it has none. Also the conditional
/// check of step 4: if this differs from the recorded `mine`, the object changed during the round.
async fn stored(state: &AppState, link: &Link, object_id: &str) -> Option<String> {
    match link.kind {
        Kind::Cal => state.calendars.get_object(&link.principal, &link.local_id, object_id, false).await.ok().map(|o| o.get_ics().to_string()),
        Kind::Ab => AddressbookReadStore::get_object(&*state.addressbooks, &link.principal, &link.local_id, object_id, false).await.ok().map(|o| o.get_vcf().to_string()),
    }
}

/// Writes one pulled object locally. Its own history guard, opened and committed around the write
/// alone, so no network wait ever holds the global lock.
async fn put_local(state: &AppState, link: &Link, object_id: &str, text: &str) -> AppResult<String> {
    let mut g = history::begin(state, history::Actor::sync(&link.principal)).await;
    g.object(store_of(link.kind), &link.principal, &link.local_id, object_id);
    let fp = match link.kind {
        Kind::Cal => {
            let normalised = crate::domain::transfer::complete_ics(text);
            let o = rustical_ical::CalendarObject::import(&normalised, Some(super::parser_options()))?;
            let fp = o.get_etag();
            state.calendars.put_object(&link.principal, &link.local_id, object_id, o, true).await?;
            fp
        }
        Kind::Ab => {
            let normalised = crate::domain::contacts::bare_params_as_type(text);
            let o = rustical_ical::AddressObject::from_vcf(normalised.into_owned())?;
            let fp = o.get_etag();
            AddressbookWriteStore::put_object(&*state.addressbooks, &link.principal, &link.local_id, object_id, o, true).await?;
            fp
        }
    };
    g.commit().await;
    Ok(fp)
}

async fn del_local(state: &AppState, link: &Link, object_id: &str) -> AppResult<()> {
    let mut g = history::begin(state, history::Actor::sync(&link.principal)).await;
    g.object(store_of(link.kind), &link.principal, &link.local_id, object_id);
    // use_trashbin = true: a remote deletion stays recoverable through the change history's undo
    match link.kind {
        Kind::Cal => state.calendars.delete_object(&link.principal, &link.local_id, object_id, true).await?,
        Kind::Ab => AddressbookWriteStore::delete_object(&*state.addressbooks, &link.principal, &link.local_id, object_id, true).await?,
    }
    g.commit().await;
    Ok(())
}

const fn store_of(kind: Kind) -> Store {
    match kind {
        Kind::Cal => Store::Cal,
        Kind::Ab => Store::Ab,
    }
}

/// The UID of a text, which is the local object id we give a pulled object. Not the href basename:
/// a remote rename would then re-create the object locally and orphan its history, and RustiCal's
/// own UNIQUE (principal, cal_id, uid) — which a soft-deleted row still holds — would reject it.
fn uid_of(kind: Kind, text: &str) -> Option<String> {
    let want = match kind {
        Kind::Cal => "UID",
        Kind::Ab => "UID",
    };
    // unfolding first: a long UID is wrapped across lines
    let flat = crate::domain::events::unfold(&text.replace("\r\n", "\n"));
    flat.iter()
        .find(|l| crate::domain::events::prop_name(l) == want)
        .and_then(|l| l.split_once(':'))
        .map(|(_, v)| v.trim().to_string())
        .filter(|u| !u.is_empty() && !u.contains('/'))
}

/// May a local change be pushed? Only when the remote did not touch the same object since the last
/// round — if it did, §6's rule applies and the remote version is the one that survives, so pushing
/// would invert it. One predicate, used for both the changed and the deleted set.
pub fn pushable(object_id: &str, remote_touched: &impl Fn(&str) -> bool) -> bool {
    !remote_touched(object_id)
}

/// Is this entry of the remote change list our OWN last write coming back? A push bumps the remote
/// sync-token, so the next incremental round lists what we wrote, carrying the very etag the server
/// gave us for it. An etag we never recorded, or none at all, is not comparable and never an echo.
pub fn remote_echo(etag: &str, known: Option<&Mapped>) -> bool {
    !etag.is_empty() && known.is_some_and(|m| m.etag == etag)
}

/// The object ids the REMOTE side really changed or removed this round, which are the ones a local
/// change must not overwrite ([`pushable`]).
///
/// Our own echo is excluded, and that exclusion is the whole point: counted as a remote change, a
/// pushed object would block the NEXT local edit of itself, and because a skipped push is not a
/// refusal, `store_tokens` would advance the local token past that edit and no later round would
/// ever report it again. Measured as a silent loss of the second of two edits made within one
/// round's distance; for a local DELETE there is not even a later round that could repair it.
pub fn touched_remotely<'a>(
    changed: &[(String, String)],
    gone: &[String],
    known: &impl Fn(&str) -> Option<&'a Mapped>,
) -> HashSet<String> {
    changed
        .iter()
        .filter(|(h, e)| !remote_echo(e, known(h)))
        .filter_map(|(h, _)| known(h))
        .chain(gone.iter().filter_map(|h| known(h)))
        .map(|m| m.object_id.clone())
        .collect()
}

/// One round. The caller holds the link's lease.
pub async fn run(state: &AppState, link: &Link, dav: &Dav, force_full: bool) -> AppResult<Report> {
    let mut rep = Report::default();

    // --- 1. local
    let (local_changed, local_gone, local_token) = local_changes(state, link).await?;
    let map = super::mapping(state, &link.id).await?;
    let by_id: HashMap<&str, &Mapped> = map.iter().map(|m| (m.object_id.as_str(), m)).collect();
    let by_href: HashMap<&str, &Mapped> = map.iter().map(|m| (m.href.as_str(), m)).collect();
    let by_uid: HashMap<&str, &Mapped> = map.iter().map(|m| (m.uid.as_str(), m)).collect();

    // --- 2. remote
    let token = link.sync_token.clone().unwrap_or_default();
    /* A full pass is due on the first round, and then once a day as the server-agnostic backstop.
       `full_at` is written on every clean round, so a link whose objects all apply stays
       incremental. One object that can NEVER apply (no UID, a body the parser refuses) would
       otherwise hold the link in a full pass forever — a complete listing every five minutes — so
       a round that skipped something still records the attempt: `ran_full_at`. The retry then
       happens on the next DAILY pass, not on every tick. */
    let full_due = force_full
        || token.is_empty()
        || link
            .full_at
            .as_deref()
            .or(link.ran_full_at.as_deref())
            .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
            .is_none_or(|t| chrono::Utc::now().signed_duration_since(t.with_timezone(&chrono::Utc)).num_hours() >= super::FULL_EVERY_HOURS);
    let mut remote = if full_due { Changes::default() } else { dav.changes(&link.href, &token).await? };
    let full = full_due || remote.reset;
    if full {
        // a reset token, the first round, or the daily backstop: compare the whole listing
        let listing = dav.list(&link.href, link.kind).await?;
        let seen: HashSet<&str> = listing.iter().map(|(h, _)| h.as_str()).collect();
        remote = Changes {
            changed: listing.iter().filter(|(h, e)| by_href.get(h.as_str()).is_none_or(|m| &m.etag != e)).cloned().collect(),
            gone: map.iter().filter(|m| !seen.contains(m.href.as_str())).map(|m| m.href.clone()).collect(),
            // a full pass learns the collection's current token for the next incremental round
            token: dav.sync_token(&link.href).await.unwrap_or_default(),
            reset: false,
        };
        rep.full = true;
    }

    // --- 3. resolve: remote wins, so a UID on both sides drops out of the local set
    let known = |h: &str| by_href.get(h).copied();
    let remote_ids = touched_remotely(&remote.changed, &remote.gone, &known);

    // --- 4. apply remote → local. Deletions first.
    for href in &remote.gone {
        let Some(m) = by_href.get(href.as_str()) else { continue };
        if local_gone.contains(&m.object_id) {
            // both sides agree it is gone: nothing to do but forget it
            super::map_del(state, &link.id, &m.object_id).await?;
            continue;
        }
        del_local(state, link, &m.object_id).await?;
        super::map_del(state, &link.id, &m.object_id).await?;
        rep.deleted_here += 1;
    }

    /* The second echo: a push bumps the remote sync-token, so the NEXT incremental round lists the
       object we just wrote as "changed". Its ETag is the one we recorded, so comparing them drops
       it before any body is fetched — measured during development, where a pushed event was
       downloaded and rewritten locally on the following round, costing a request and a history
       entry for nothing. An etag the server did not give us (empty) is always fetched. */
    let hrefs: Vec<String> = remote.changed.iter().filter(|(h, e)| !remote_echo(e, known(h))).map(|(h, _)| h.clone()).collect();
    for batch in hrefs.chunks(crate::domain::transfer::IMPORT_CHUNK) {
        for (href, etag, body) in dav.bodies(&link.href, link.kind, batch).await? {
            let Some(uid) = uid_of(link.kind, &body) else {
                rep.refused += 1;
                continue;
            };
            // the local id is the UID; an href change is a mapping update, not a new object
            let object_id = by_href.get(href.as_str()).map_or_else(|| uid.clone(), |m| m.object_id.clone());
            /* Conditional: if what is stored differs from what the two sides last agreed on, the
               object changed during this round or behind our back — a conflict, not a plain write.
               Only asked where there IS an agreement: `by_id` first, so a first pull of 10 000
               objects does not do 10 000 pointless reads and parses for a counter. */
            if let Some(m) = by_id.get(object_id.as_str()) {
                if let Some(now) = stored(state, link, &object_id).await {
                    if super::fingerprint(link.kind, &now).unwrap_or_default() != m.mine {
                        rep.conflicts += 1;
                    }
                }
            }
            match put_local(state, link, &object_id, &body).await {
                Ok(mine) => {
                    super::map_put(state, &link.id, &Mapped { object_id, uid, href: href.clone(), etag: etag.clone(), mine }).await?;
                    rep.pulled += 1;
                }
                // one unacceptable object must not stall the link
                Err(e) => {
                    tracing::warn!("remote sync {}: object {href} not applied: {e}", link.id);
                    rep.refused += 1;
                }
            }
        }
    }

    if !link.two_way {
        store_tokens(state, link, &remote.token, local_token, full, rep.refused, rep.unpushed).await?;
        return Ok(rep);
    }

    // --- 5. push local → remote
    let remote_touched = |id: &str| remote_ids.contains(id);
    for id in &local_gone {
        if !pushable(id, &remote_touched) {
            // agreed, or resurrected by step 4: either way nothing to push
            continue;
        }
        let Some(m) = by_id.get(id.as_str()) else { continue };
        match dav.delete(&m.href, &m.etag).await? {
            Wrote::Ok { .. } => {
                super::map_del(state, &link.id, id).await?;
                rep.deleted_there += 1;
            }
            // 412 on a delete is ambiguous: already gone, or changed remotely. Measured: a
            // repeated identical DELETE answers 412, not 404.
            Wrote::Precondition | Wrote::UidTaken(_) => match dav.head(&m.href).await? {
                None => super::map_del(state, &link.id, id).await?,
                Some(_) => {
                    // it is still there and has moved on: remote wins, so pull it back
                    if let Some((etag, body)) = dav.get(&m.href).await? {
                        let uid = uid_of(link.kind, &body).unwrap_or_else(|| m.uid.clone());
                        let mine = put_local(state, link, id, &body).await?;
                        super::map_put(state, &link.id, &Mapped { object_id: id.clone(), uid, href: m.href.clone(), etag, mine }).await?;
                        rep.conflicts += 1;
                        rep.pulled += 1;
                    }
                }
            },
            Wrote::Refused(s) => {
                tracing::warn!("remote sync {}: delete of {} refused with {s}", link.id, m.href);
                rep.refused += 1;
                rep.unpushed += 1;
            }
        }
    }

    for (id, text) in &local_changed {
        if !pushable(id, &remote_touched) {
            continue;
        }
        let known = by_id.get(id.as_str());
        // echo: this is exactly the text we last wrote or pushed
        if let Some(m) = known {
            if super::fingerprint(link.kind, text).is_ok_and(|fp| fp == m.mine) {
                continue;
            }
        }
        let uid = uid_of(link.kind, text).unwrap_or_else(|| id.clone());
        let href = known.map_or_else(
            || {
                let base = link.href.trim_end_matches('/');
                format!("{base}/{}.{}", super::pct(&uid), link.kind.ext())
            },
            |m| m.href.clone(),
        );
        let etag = known.map(|m| m.etag.clone());
        match dav.put(&href, link.kind, etag.as_deref(), text.clone()).await? {
            Wrote::Ok { etag } => {
                // no ETag means the server rewrote the body (measured: a repaired PRODID answers
                // with X-Sabre-Ew-Gross and no ETag), so the stored object is not ours: read back
                let (etag, mine) = match etag {
                    Some(e) => (e, super::fingerprint(link.kind, text)?),
                    None => match dav.get(&href).await? {
                        Some((e, body)) => (e, put_local(state, link, id, &body).await?),
                        None => (String::new(), super::fingerprint(link.kind, text)?),
                    },
                };
                super::map_put(state, &link.id, &Mapped { object_id: id.clone(), uid, href, etag, mine }).await?;
                rep.pushed += 1;
            }
            /* The UID exists under another href. Remote wins, so we adopt THAT href and pull —
               never push, which would overwrite the remote object and invert the rule.
               The href comes from the server's own `no-uid-conflict` body (measured); the mapping
               is only a fallback. Falling back to the href we just tried cannot work: the server
               refused it precisely because the object lives elsewhere. */
            Wrote::UidTaken(named) => {
                match named.or_else(|| by_uid.get(uid.as_str()).map(|m| m.href.clone())) {
                    Some(other) => match dav.get(&other).await? {
                        Some((etag, body)) => {
                            let mine = put_local(state, link, id, &body).await?;
                            super::map_put(state, &link.id, &Mapped { object_id: id.clone(), uid, href: other, etag, mine }).await?;
                            rep.conflicts += 1;
                            rep.pulled += 1;
                        }
                        // it moved again between the 409 and this read: next round, with a fresh listing
                        None => {
                            rep.refused += 1;
                            rep.unpushed += 1;
                        }
                    },
                    // the server named no href and we know none: nothing can be adopted
                    None => {
                        rep.refused += 1;
                        rep.unpushed += 1;
                    }
                }
            }
            // 412 on a put: either a real conflict or an href that is not there
            Wrote::Precondition => match dav.get(&href).await? {
                Some((etag, body)) => {
                    let mine = put_local(state, link, id, &body).await?;
                    super::map_put(state, &link.id, &Mapped { object_id: id.clone(), uid, href, etag, mine }).await?;
                    rep.conflicts += 1;
                    rep.pulled += 1;
                }
                None => super::map_del(state, &link.id, id).await?,
            },
            Wrote::Refused(s) => {
                tracing::warn!("remote sync {}: put of {href} refused with {s}", link.id);
                rep.refused += 1;
                rep.unpushed += 1;
            }
        }
    }

    // --- 6. tokens
    store_tokens(state, link, &remote.token, local_token, full, rep.refused, rep.unpushed).await?;
    Ok(rep)
}

/// Stores what the round learned. A round that had to skip objects is NOT a clean round.
///
/// The two tokens are kept apart on purpose, because the two sides forget differently:
///
/// - the REMOTE token may always advance. A body nextcloud will not accept, or one our parser will
///   not read, is refused again next time; replaying it forever would stall every later change.
///   `full_at` is left unset instead, so the next round reconciles the remote side fully and the
///   skipped objects are offered again.
/// - the LOCAL token may only advance on a clean round. `sync_changes` reports only
///   `synctoken > ?` (**source** `rustical … calendar_store/mod.rs`, the changelog query), so an
///   object whose push failed would never appear in any later change list — and no full
///   reconciliation brings it back, because that one rebuilds the REMOTE side. The user's change
///   would be lost silently. Keeping the old value re-reports the whole batch next round, which
///   costs a few requests that echo suppression then drops.
/// Writes what the round learned. `refused` is every skipped object and decides the message and the
/// full-pass stamp; `unpushed` is only the LOCAL changes that did not arrive, and only they hold the
/// local token back.
///
/// The two are separate because one unreadable REMOTE object would otherwise freeze the local token
/// for good: it is refused on every round, and a frozen token makes every round re-read and re-parse
/// the collection's whole change list to suppress echoes — for an object the local side never owed
/// anybody.
pub async fn store_tokens(state: &AppState, link: &Link, remote: &str, local: i64, full: bool, refused: usize, unpushed: usize) -> AppResult<()> {
    let now = crate::clock::now().to_rfc3339();
    let clean = refused == 0;
    let owed = unpushed > 0;
    sqlx::query(
        "UPDATE practical_remote_links
           SET sync_token = ?, local_token = CASE WHEN ? THEN local_token ELSE ? END, ran_at = ?,
               full_at = CASE WHEN ? AND ? THEN ? ELSE full_at END,
               ran_full_at = CASE WHEN ? THEN ? ELSE ran_full_at END,
               err = CASE WHEN ? THEN NULL ELSE 'err-remote-skipped' END,
               running_since = NULL
         WHERE id = ?",
    )
    .bind(if remote.is_empty() { None } else { Some(remote) })
    .bind(owed)
    .bind(local)
    .bind(&now)
    .bind(full)
    .bind(clean)
    .bind(&now)
    .bind(full)
    .bind(&now)
    .bind(clean)
    .bind(&link.id)
    .execute(&state.db)
    .await?;
    Ok(())
}

/// Records a failure without clearing the tokens: the next round picks up where this one stopped.
///
/// Only ARGUMENT-FREE keys are stored. The link row is rendered with a plain `ctx.t`, so a key
/// carrying `{ $status }` would print the placeholder verbatim and log a missing-argument warning
/// on every page build. Keys that need an argument are mapped to the one that says the same thing
/// without it.
pub async fn store_failure(state: &AppState, link: &Link, err: &AppError) -> AppResult<()> {
    // the message key, never a body or header dump — those can carry the credential
    let msg = match err {
        AppError::Rejected(axum::http::StatusCode::UNAUTHORIZED, _) => "err-remote-auth",
        AppError::NotFound => "err-remote-gone",
        AppError::BadRequest(m) | AppError::Conflict(m) | AppError::Rejected(_, m) => match m.key {
            // these three carry a placeholder, so they cannot be shown as they are
            "err-remote-status" | "err-remote-busy" | "err-remote-net" | "err-remote-big" => "err-remote-unreachable",
            k => k,
        },
        _ => "err-remote-unreachable",
    };
    sqlx::query("UPDATE practical_remote_links SET err = ?, ran_at = ?, running_since = NULL WHERE id = ?")
        .bind(&msg)
        .bind(crate::clock::now().to_rfc3339())
        .bind(&link.id)
        .execute(&state.db)
        .await?;
    Ok(())
}
