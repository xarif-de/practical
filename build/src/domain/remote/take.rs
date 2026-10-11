//! Creating a local collection from a remote one: the one-off import, and the first round of a new
//! link. Both reuse `domain::transfer`'s importer unchanged, so a remote import and a file import
//! produce the same thing and share every limit and every fix.
//!
//! A link always CREATES its local collection. Merging two already-populated collections is out of
//! scope: there is no way to tell a duplicate from an intentional copy, and getting it wrong would
//! quietly delete a user's data on one of the two sides.

use super::dav::{Dav, Kind};
use super::round;
use crate::domain::{collections, transfer};
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use rustical_store::auth::Principal;

/// Everything in a remote collection as one document, in batches so peak memory is one batch.
/// Refuses an oversized collection BEFORE downloading any body — the href listing comes first, so
/// the object count is known in advance.
async fn fetch_all(dav: &Dav, kind: Kind, href: &str) -> AppResult<Vec<String>> {
    let listing = dav.list(href, kind).await?;
    if listing.len() > transfer::IMPORT_MAX_OBJECTS {
        return Err(AppError::BadRequest(crate::error::Msg {
            key: "err-import-too-many",
            args: vec![("count", listing.len().to_string()), ("limit", transfer::IMPORT_MAX_OBJECTS.to_string())],
        }));
    }
    let hrefs: Vec<String> = listing.into_iter().map(|(h, _)| h).collect();
    let mut out = vec![];
    for batch in hrefs.chunks(transfer::IMPORT_CHUNK) {
        for (_, _, body) in dav.bodies(href, kind, batch).await? {
            if body.len() > transfer::IMPORT_MAX_OBJECT_BYTES {
                return Err(AppError::BadRequest(crate::error::Msg {
                    key: "err-import-too-large",
                    args: vec![("size", body.len().to_string()), ("limit", transfer::IMPORT_MAX_OBJECT_BYTES.to_string())],
                }));
            }
            out.push(body);
        }
        // the parse is CPU-bound: let other requests through between batches
        tokio::task::yield_now().await;
    }
    Ok(out)
}

/// The remote collection's own metadata, so the local copy starts with its name and colour instead
/// of a placeholder.
async fn meta(dav: &Dav, kind: Kind, href: &str) -> AppResult<super::dav::Collection> {
    let principal = dav.principal().await?;
    let home = href.trim_end_matches('/').rsplit_once('/').map_or_else(|| href.to_string(), |(h, _)| format!("{h}/"));
    dav.collections(&home, kind, &principal)
        .await?
        .into_iter()
        .find(|c| c.href.trim_end_matches('/') == href.trim_end_matches('/'))
        // not offerable any more (shared since, deleted, generated) → refuse rather than guess
        .ok_or_else(|| AppError::bad("err-remote-not-offered"))
}

/// Joins the fetched objects into one document the existing importer accepts: an .ics file with
/// every VCALENDAR after another, or a .vcf with every card.
fn as_document(kind: Kind, parts: &[String]) -> String {
    match kind {
        // the importer splits VCALENDARs itself, so concatenating is enough and keeps every
        // object's own VTIMEZONE with it
        Kind::Cal | Kind::Ab => parts.iter().map(|p| p.trim_end().to_string()).collect::<Vec<_>>().join("\r\n") + "\r\n",
    }
}

/// A one-off copy. Returns (local id, objects).
pub async fn import(state: &AppState, user: &Principal, dav: &Dav, kind: Kind, href: &str, name: &str, color: &str) -> AppResult<(String, usize)> {
    let m = meta(dav, kind, href).await?;
    let parts = fetch_all(dav, kind, href).await?;
    create_local(state, user, kind, &m, name, color, &parts).await
}

/// Creates the local collection and the link, then runs the first round so the mapping is complete
/// and the user sees a result immediately.
pub async fn link(
    state: &AppState,
    user: &Principal,
    account_id: &str,
    kind: Kind,
    href: &str,
    name: &str,
    color: &str,
    two_way: bool,
    every_min: i64,
) -> AppResult<(String, usize, String)> {
    // every refusal happens BEFORE the local collection exists: an error after that would leave an
    // empty calendar behind that the user did not ask for
    super::room_for_a_link(state, user, account_id, kind, href).await?;
    let link_id = {
        let m = meta_with(state, user, account_id, kind, href).await?;
        // a collection we may not write to can only ever be pulled
        let two_way = two_way && m.writable;
        let (local_id, _) = create_local(state, user, kind, &m, name, color, &[]).await?;
        super::create_link(state, user, account_id, kind, &local_id, href, two_way, every_min).await?
    };
    let link = super::link_of(state, user, &link_id).await?;
    let (_, dav) = super::dav_for(state, &link).await?;
    // the first round is a full one by definition: there is no token yet
    let rep = round::run(state, &link, &dav, true).await?;
    Ok((link.local_id, rep.pulled, link_id))
}

async fn meta_with(state: &AppState, user: &Principal, account_id: &str, kind: Kind, href: &str) -> AppResult<super::dav::Collection> {
    let (a, s) = {
        let probe = super::Link {
            id: String::new(),
            account_id: account_id.to_string(),
            principal: user.id.clone(),
            kind,
            local_id: String::new(),
            href: href.to_string(),
            two_way: false,
            every_min: super::DEFAULT_EVERY_MIN,
            sync_token: None,
            local_token: 0,
            full_at: None,
            ran_full_at: None,
            ran_at: None,
            running_since: None,
            err: None,
        };
        super::account_of(state, &probe).await?
    };
    // the origin was checked when the link was made; the resolver checks every dial anyway
    let base: url::Url = a.base_url.parse().map_err(|_| AppError::bad("err-remote-bad-server"))?;
    let dav = Dav::new(state.remote_http.clone(), base, a.login, s);
    meta(&dav, kind, href).await
}

/// The local collection, through the existing importer. An empty object list means "create it
/// empty", which is what a link does before its first round.
async fn create_local(state: &AppState, user: &Principal, kind: Kind, m: &super::dav::Collection, name: &str, color: &str, parts: &[String]) -> AppResult<(String, usize)> {
    let name = if name.trim().is_empty() { m.name.clone() } else { name.trim().to_string() };
    let color = if color.trim().is_empty() { m.color.clone() } else { color.trim().to_string() };
    let color = if collections::valid_color(&color) { color } else { collections::PALETTE[0].to_string() };
    match kind {
        Kind::Cal => {
            let n = collections::NewCalendar {
                owner: user.id.clone(),
                name,
                color,
                description: m.description.clone(),
                components: collections::parse_components(&if m.components.is_empty() { vec!["VEVENT".to_string()] } else { m.components.clone() }),
                timezone: if m.timezone.is_empty() { state.config.default_timezone.clone() } else { m.timezone.clone() },
            };
            if parts.is_empty() {
                let id = collections::create_calendar(state, user, n).await?;
                Ok((id, 0))
            } else {
                transfer::import_new_calendar(state, user, n, as_document(kind, parts).as_bytes()).await
            }
        }
        Kind::Ab => {
            if parts.is_empty() {
                let id = collections::create_addressbook(state, user, &user.id, &name, &color, &m.description).await?;
                Ok((id, 0))
            } else {
                transfer::import_new_addressbook(state, user, &user.id, &name, &color, &m.description, as_document(kind, parts).as_bytes()).await
            }
        }
    }
}
