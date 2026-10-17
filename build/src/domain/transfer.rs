//! Import/export — export serialises RustiCal's objects with caldata's
//! generator, import parses with caldata and stores via the store crate
//! (`put_objects`), so nothing but RustiCal's own code touches the data.
//!
//! - [`complete_ics`] adds what caldata needs and many files lack: PRODID, VERSION, UID, DTSTAMP.
//! - Limits: [`IMPORT_MAX_BYTES`] (8 MiB), [`IMPORT_MAX_OBJECTS`] (10 000), [`IMPORT_MAX_OBJECT_BYTES`]; the
//!   routes that import into a new collection accept a 32 MiB body ([`crate::routes::settings`]).
//! - `.ics` imports merge by UID (a known UID is overwritten).
//! - A new collection is prefilled from X-WR-CALNAME, X-WR-CALDESC, X-WR-CALCOLOR, X-WR-TIMEZONE (and
//!   X-APPLE-CALENDAR-COLOR); the X-WR-* lines are dropped from the stored objects.
//! - A `.vcf` contact without UID gets one.

use crate::domain::collections;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use caldata::component::{Component, ComponentMut, IcalCalendar};
use caldata::generator::Emitter;
use caldata::parser::{ContentLine, ParserOptions};
use caldata::{IcalParser, VcardParser};
use rustical_ical::{AddressObject, CalendarObject, CalendarObjectType};
use rustical_store::auth::Principal;
use rustical_store::{Addressbook, AddressbookReadStore, AddressbookWriteStore, Calendar, CalendarMetadata, CalendarReadStore, CalendarWriteStore};

pub async fn export_calendar(state: &AppState, user: &Principal, principal: &str, cal_id: &str) -> AppResult<(String, String)> {
    collections::ensure_owner(user, principal)?;
    let cal = state.calendars.get_calendar(principal, cal_id, false).await?;
    let objects = state.calendars.get_objects(principal, cal_id).await?;
    let ical = IcalCalendar::from_objects(PRODID.to_string(), objects.into_iter().map(|(_, o)| o.into()).collect(), vec![]);
    let name = cal.meta.displayname.clone().unwrap_or_else(|| cal_id.to_string());
    Ok((name, ical.generate()))
}

pub async fn export_addressbook(state: &AppState, user: &Principal, principal: &str, ab_id: &str) -> AppResult<(String, String)> {
    collections::ensure_owner(user, principal)?;
    let ab = state.addressbooks.get_addressbook(principal, ab_id, false).await?;
    // every card as stored, those the store cannot parse too (`contacts::ab_cards`)
    let mut out = String::new();
    for (_, vcf) in crate::domain::contacts::ab_cards(state, principal, ab_id).await? {
        out.push_str(vcf.trim_end());
        out.push_str("\r\n");
    }
    Ok((ab.displayname.unwrap_or_else(|| ab_id.to_string()), out))
}

/// PRODID of what PractiCal writes, and of an imported calendar that names no producer.
pub const PRODID: &str = "-//xarif.de//PractiCal//DE";

/// How an imported .ics is parsed. `rfc7809` lets a `TZID` stand without its `VTIMEZONE` block,
/// which RFC 7809 allows for IANA zones and which real exports rely on: measured, a nextcloud
/// `multiget` hands out an event with `DTSTART;TZID=Europe/Berlin` and no `VTIMEZONE` beside it,
/// and the strict parser refuses the whole file (`TZID Europe/Berlin does not refer to a valid
/// timezone`). RustiCal's own CalDAV endpoint accepts it too
/// (`rustical/crates/caldav/src/lib.rs`, `rfc7809: true`), so an import that refused it would be
/// stricter than the server a phone writes to.
pub fn options() -> ParserOptions {
    ParserOptions { rfc7809: true }
}

/// The required properties a sloppy .ics leaves out (RFC 5545 §3.6, §3.8.4.7, §3.8.7.2), supplied before it is
/// parsed: PRODID and VERSION of each VCALENDAR, UID and DTSTAMP of each VEVENT/VTODO/VJOURNAL in it. Without
/// VERSION, UID or DTSTAMP caldata rejects the file; without PRODID it parses, but the objects stored from it
/// lack one and RustiCal's own reader rejects each (`CalendarObject::from_ics` → MissingProperty("PRODID")), so
/// the store hides them (skip_broken). A complete file comes back unchanged (borrowed).
pub fn complete_ics(ics: &str) -> std::borrow::Cow<'_, str> {
    let lines = crate::domain::events::unfold(&ics.replace("\r\n", "\n"));
    // open components: (name, index of its BEGIN line in `out`, properties seen directly in it)
    let mut stack: Vec<(String, usize, Vec<String>)> = vec![];
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut added = false;
    for l in lines {
        if let Some(name) = l.strip_prefix("BEGIN:") {
            stack.push((name.trim().to_string(), out.len(), vec![]));
        } else if l.starts_with("END:") {
            if let Some((name, begin, seen)) = stack.pop() {
                let parent = stack.last().map(|f| f.0.as_str());
                let need: &[(&str, String)] = &match (name.as_str(), parent) {
                    ("VCALENDAR", None) => vec![("PRODID", PRODID.to_string()), ("VERSION", "2.0".into())],
                    ("VEVENT" | "VTODO" | "VJOURNAL", Some("VCALENDAR")) => vec![("UID", uuid::Uuid::new_v4().to_string()), ("DTSTAMP", crate::domain::ical::fmt_utc_now())],
                    _ => vec![],
                };
                let missing: Vec<String> = need.iter().filter(|(p, _)| !seen.iter().any(|s| s == p)).map(|(p, v)| format!("{p}:{v}")).collect();
                added |= !missing.is_empty();
                out.splice(begin + 1..begin + 1, missing);
            }
        } else if let Some(top) = stack.last_mut() {
            top.2.push(crate::domain::events::prop_name(&l));
        }
        out.push(l);
    }
    if added { std::borrow::Cow::Owned(out.join("\r\n") + "\r\n") } else { std::borrow::Cow::Borrowed(ics) }
}

/// The objects RustiCal will read back (`CalendarObject::from_ics` of the text stored): an object it would
/// hide is left out (and logged), so an import counts what really got stored.
fn readable(objects: Vec<CalendarObject>) -> Vec<CalendarObject> {
    objects.into_iter().filter(|o| match CalendarObject::from_ics(o.get_ics().to_string()) {
        Ok(_) => true,
        Err(e) => {
            tracing::warn!("import: object {} skipped, RustiCal could not read it back: {e}", o.get_uid());
            false
        }
    }).collect()
}

/// Import limits (the pod has 192 MiB). A file is read in chunks of `IMPORT_CHUNK` objects and written in chunks of
/// as many, so the parse costs one chunk plus the file's object texts (25.8 MiB / 60 000 events peaked at 904 MiB
/// parsed whole). What stays proportional is the history's record of the import
/// (`history::ingest` holds every object's text twice): 35 000 objects (15 MiB) still peaked at +198 MiB RSS in a
/// release build, hence the object cap.
pub const IMPORT_MAX_BYTES: usize = 8 * 1024 * 1024;
pub const IMPORT_MAX_OBJECTS: usize = 10_000;
/// One object (an event with its overrides and embedded files): the editor's own request limit.
pub const IMPORT_MAX_OBJECT_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const IMPORT_CHUNK: usize = 500;

fn too_large(size: usize, limit: usize) -> AppError {
    AppError::BadRequest(crate::error::Msg { key: "err-import-too-large", args: vec![("size", size.to_string()), ("limit", limit.to_string())] })
}

/// One VCALENDAR of a file as slices of its text: its own lines (BEGIN line, properties with their
/// continuation lines), its VTIMEZONE blocks, and every other component block grouped by UID — a master and
/// its RECURRENCE-ID overrides are one object (caldata `into_objects`) wherever they stand in the file.
#[derive(Default)]
struct CalText<'a> {
    head: Vec<&'a str>,
    zones: Vec<&'a str>,
    /// (UID, blocks) in the order of first appearance; a component without UID is a group of its own
    groups: Vec<(Option<String>, Vec<&'a str>)>,
}

/// The VCALENDARs of an .ics text, without parsing it (names compared case-insensitively, as `unfold` canonicalises them).
fn split_calendars(text: &str) -> Vec<CalText<'_>> {
    let mut out = vec![];
    let mut cur: Option<CalText> = None;
    let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let (mut depth, mut comp_start, mut comp_zone, mut uid, mut in_uid) = (0usize, 0usize, false, None::<String>, false);
    let mut pos = 0;
    while pos < text.len() {
        let end = text[pos..].find('\n').map_or(text.len(), |i| pos + i + 1);
        let raw = &text[pos..end];
        let line = raw.trim_end_matches(['\r', '\n']);
        let start = pos;
        pos = end;
        let word = |w: &str| line.get(..w.len()).is_some_and(|h| h.eq_ignore_ascii_case(w));
        if line.starts_with([' ', '\t']) {
            if depth == 1 && let Some(c) = cur.as_mut() {
                c.head.push(raw);
            } else if in_uid && let Some(u) = uid.as_mut() {
                u.push_str(&line[1..]);
            }
            continue;
        }
        in_uid = false;
        match depth {
            0 if word("BEGIN:VCALENDAR") => {
                depth = 1;
                index.clear();
                cur = Some(CalText { head: vec![raw], ..Default::default() });
            }
            0 => {}
            1 if word("END:VCALENDAR") => {
                depth = 0;
                out.extend(cur.take());
            }
            1 if word("BEGIN:") => {
                depth = 2;
                (comp_start, comp_zone, uid) = (start, word("BEGIN:VTIMEZONE"), None);
            }
            1 => cur.iter_mut().for_each(|c| c.head.push(raw)),
            _ if word("BEGIN:") => depth += 1,
            _ if word("END:") => {
                depth -= 1;
                if depth == 1 && let Some(c) = cur.as_mut() {
                    let block = &text[comp_start..end];
                    if comp_zone {
                        c.zones.push(block);
                    } else if let Some(u) = uid.take().filter(|u| !u.is_empty()) {
                        match index.get(&u) {
                            Some(&i) => c.groups[i].1.push(block),
                            None => {
                                index.insert(u.clone(), c.groups.len());
                                c.groups.push((Some(u), vec![block]));
                            }
                        }
                    } else {
                        c.groups.push((None, vec![block]));
                    }
                }
            }
            2 if word("UID:") || word("UID;") => {
                uid = Some(line.split_once(':').map(|(_, v)| v.trim().to_string()).unwrap_or_default());
                in_uid = true;
            }
            _ => {}
        }
    }
    out
}

/// One parse unit: the calendar's own lines and zones around `groups`.
fn chunk_text(cal: &CalText, groups: &[(Option<String>, Vec<&str>)]) -> String {
    let mut s = String::new();
    for part in cal.head.iter().chain(&cal.zones).chain(groups.iter().flat_map(|g| &g.1)) {
        s.push_str(part);
        if !part.ends_with('\n') {
            s.push_str("\r\n");
        }
    }
    s.push_str("END:VCALENDAR\r\n");
    s
}

/// Parses one chunk into the objects RustiCal will read back. `new_calendar`: the calendar-level X-WR-* lines
/// (name, description, colour, zone — the page reads them from the file) are dropped before the objects are split,
/// as RustiCal's own IMPORT does, so they do not land in every object; `found` collects the component types.
/// A panic in caldata's parser (`DURATION:P99999999999W` unwraps an overflow, caldata 0.17.3 src/types/duration.rs:56)
/// fails the import like any unreadable file instead of dropping the connection.
fn parse_chunk(text: &str, new_calendar: bool, found: &mut Vec<CalendarObjectType>) -> AppResult<Vec<CalendarObject>> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| parse_chunk_inner(text, new_calendar, found))).unwrap_or_else(|_| Err(AppError::bad1("err-ics", "detail", "a value is out of range")))
}

fn parse_chunk_inner(text: &str, new_calendar: bool, found: &mut Vec<CalendarObjectType>) -> AppResult<Vec<CalendarObject>> {
    let bad = |e: &dyn std::fmt::Display| AppError::bad1("err-ics", "detail", e);
    let mut objects = vec![];
    for cal in IcalParser::from_slice(complete_ics(text).as_bytes()).with_options(options()) {
        let mut cal = cal.map_err(|e| bad(&e))?;
        if new_calendar {
            let mut m = cal.mutable();
            for p in ["X-WR-CALNAME", "X-WR-CALDESC", "X-WR-CALCOLOR", "X-WR-TIMEZONE", "X-APPLE-CALENDAR-COLOR"] {
                m.remove_property(p);
            }
            cal = m.build(&options(), None).map_err(|e| bad(&e))?;
        }
        for (has, t) in [(!cal.events.is_empty(), CalendarObjectType::Event), (!cal.todos.is_empty(), CalendarObjectType::Todo), (!cal.journals.is_empty(), CalendarObjectType::Journal)] {
            if has && !found.contains(&t) {
                found.push(t);
            }
        }
        for obj in cal.into_objects().map_err(|e| bad(&e))? {
            let id = obj.get_uid().to_string();
            if !id.is_empty() && !id.contains('/') {
                objects.push(CalendarObject::from(obj));
            }
        }
    }
    Ok(readable(objects))
}

/// Every object of an .ics file as (UID, text RustiCal reads back), in file order, a later object with the same
/// UID replacing an earlier one; and the component types found. The whole file is checked before anything is
/// written (a broken last object fails the import, nothing is stored); the parsed form lives one chunk long.
async fn file_objects(ics: &[u8], new_calendar: bool) -> AppResult<(Vec<(String, String)>, Vec<CalendarObjectType>)> {
    if ics.len() > IMPORT_MAX_BYTES {
        return Err(too_large(ics.len(), IMPORT_MAX_BYTES));
    }
    let text = String::from_utf8_lossy(ics);
    let cals = split_calendars(&text);
    if cals.is_empty() {
        // no complete VCALENDAR: caldata names what is wrong with the text (or finds nothing in it)
        parse_chunk(&text, new_calendar, &mut vec![])?;
    }
    let n: usize = cals.iter().map(|c| c.groups.len()).sum();
    if n > IMPORT_MAX_OBJECTS {
        return Err(AppError::BadRequest(crate::error::Msg { key: "err-import-too-many", args: vec![("count", n.to_string()), ("limit", IMPORT_MAX_OBJECTS.to_string())] }));
    }
    if let Some(big) = cals.iter().flat_map(|c| &c.groups).map(|g| g.1.iter().map(|b| b.len()).sum::<usize>()).find(|b| *b > IMPORT_MAX_OBJECT_BYTES) {
        return Err(too_large(big, IMPORT_MAX_OBJECT_BYTES));
    }
    let (mut out, mut found): (Vec<(String, String)>, Vec<CalendarObjectType>) = (vec![], vec![]);
    let mut at: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for cal in &cals {
        // an empty VCALENDAR still tells its component types (none) and must parse
        let chunks: Vec<&[(Option<String>, Vec<&str>)]> = if cal.groups.is_empty() { vec![&[]] } else { cal.groups.chunks(IMPORT_CHUNK).collect() };
        for chunk in chunks {
            for o in parse_chunk(&chunk_text(cal, chunk), new_calendar, &mut found)? {
                let (uid, ics) = (o.get_uid().to_string(), o.get_ics().to_string());
                match at.get(&uid) {
                    Some(&i) => out[i].1 = ics,
                    None => {
                        at.insert(uid.clone(), out.len());
                        out.push((uid, ics));
                    }
                }
            }
            // the parse is CPU-bound: let the runtime's other requests through between chunks
            tokio::task::yield_now().await;
        }
    }
    Ok((out, found))
}

/// The checked texts again as objects for the store, one chunk at a time.
fn store_chunk(chunk: &[(String, String)]) -> AppResult<Vec<(String, CalendarObject)>> {
    chunk.iter().map(|(uid, ics)| Ok((uid.clone(), CalendarObject::from_ics(ics.clone())?))).collect()
}

/// Imports every VEVENT/VTODO/VJOURNAL object of an .ics file into an EXISTING
/// calendar; objects with a known UID are overwritten (merge). Written in chunks of `IMPORT_CHUNK` objects (one
/// transaction each, so other users' saves get the write lock in between) after the whole file was checked.
pub async fn import_calendar(state: &AppState, user: &Principal, principal: &str, cal_id: &str, ics: &[u8]) -> AppResult<usize> {
    collections::ensure_owner(user, principal)?;
    state.calendars.get_calendar(principal, cal_id, false).await?;
    collections::ensure_writable(state, principal, cal_id).await?;
    let (objects, _) = file_objects(ics, false).await?;
    if objects.is_empty() {
        return Err(AppError::bad("err-no-calendar-objects"));
    }
    for chunk in objects.chunks(IMPORT_CHUNK) {
        state.calendars.put_objects(principal, cal_id, store_chunk(chunk)?, true).await?;
    }
    Ok(objects.len())
}

/// Creates a calendar from an .ics file (RustiCal's `import_calendar` with the first chunk, `put_objects` for the
/// rest — the whole file is checked first): the metadata as given, the component types given plus those the
/// file holds. Returns (id, objects).
pub async fn import_new_calendar(state: &AppState, user: &Principal, n: collections::NewCalendar, ics: &[u8]) -> AppResult<(String, usize)> {
    collections::ensure_owner(user, &n.owner)?;
    if n.name.trim().is_empty() {
        return Err(AppError::bad("err-name-required"));
    }
    let (objects, found) = file_objects(ics, true).await?;
    if objects.is_empty() {
        return Err(AppError::bad("err-no-calendar-objects"));
    }
    let mut components = n.components;
    for t in found {
        if !components.contains(&t) {
            components.push(t);
        }
    }
    let count = objects.len();
    let id = uuid::Uuid::new_v4().to_string();
    let cal = Calendar {
        meta: CalendarMetadata {
            displayname: Some(n.name.trim().to_string()),
            order: 0,
            description: Some(n.description.trim().to_string()).filter(|d| !d.is_empty()),
            color: Some(n.color.trim().to_string()).filter(|c| !c.is_empty()),
        },
        principal: n.owner.clone(),
        id: id.clone(),
        // an unknown zone is left out: RustiCal's own IMPORT asserts on it
        timezone_id: Some(n.timezone.trim().to_string()).filter(|t| t.parse::<chrono_tz::Tz>().is_ok()),
        deleted_at: None,
        synctoken: 0,
        subscription_url: None,
        push_topic: uuid::Uuid::new_v4().to_string(),
        components,
    };
    let mut chunks = objects.chunks(IMPORT_CHUNK);
    let first = chunks.next().map(store_chunk).transpose()?.unwrap_or_default();
    state.calendars.import_calendar(cal, first.into_iter().map(|(_, o)| o).collect(), false).await?;
    for chunk in chunks {
        state.calendars.put_objects(&n.owner, &id, store_chunk(chunk)?, false).await?;
    }
    Ok((id, count))
}

/// Creates an address book from a .vcf file in one transaction (RustiCal's `import_addressbook`); a card
/// without UID gets one, as RustiCal's own IMPORT does. The colour (ours, not RustiCal's) is written
/// after the import succeeded. Returns (id, cards).
/// Every card of a .vcf file as (id, object), all parsed before the caller writes any (a broken third card
/// used to answer 400 with the first two already stored). A card without UID gets one, as
/// RustiCal's own IMPORT does; a UID with a slash is skipped. Same size limits as an .ics import.
fn file_cards(vcf: &[u8]) -> AppResult<Vec<(String, AddressObject)>> {
    if vcf.len() > IMPORT_MAX_BYTES {
        return Err(too_large(vcf.len(), IMPORT_MAX_BYTES));
    }
    let mut objects = vec![];
    for card in VcardParser::from_slice(crate::domain::contacts::bare_params_as_type(&String::from_utf8_lossy(vcf)).as_bytes()) {
        let mut card = card.map_err(|e| AppError::bad1("err-vcf", "detail", e))?;
        if card.get_uid().is_none() {
            let mut m = card.mutable();
            m.add_content_line(ContentLine { name: "UID".into(), value: uuid::Uuid::new_v4().to_string(), params: vec![].into() });
            card = m.build(&ParserOptions::default(), None).map_err(|e| AppError::bad1("err-vcf", "detail", e))?;
        }
        let id = card.get_uid().unwrap_or_default().to_string();
        if !id.is_empty() && !id.contains('/') {
            objects.push((id, AddressObject::from(card)));
        }
        if objects.len() > IMPORT_MAX_OBJECTS {
            return Err(AppError::BadRequest(crate::error::Msg { key: "err-import-too-many", args: vec![("count", format!("> {IMPORT_MAX_OBJECTS}")), ("limit", IMPORT_MAX_OBJECTS.to_string())] }));
        }
    }
    Ok(objects)
}

pub async fn import_new_addressbook(state: &AppState, user: &Principal, owner: &str, name: &str, color: &str, description: &str, vcf: &[u8]) -> AppResult<(String, usize)> {
    collections::ensure_owner(user, owner)?;
    if name.trim().is_empty() {
        return Err(AppError::bad("err-name-required"));
    }
    if !collections::valid_color(color) {
        return Err(AppError::bad("err-color"));
    }
    let objects = file_cards(vcf)?;
    if objects.is_empty() {
        return Err(AppError::bad("err-no-vcards"));
    }
    let count = objects.len();
    let id = uuid::Uuid::new_v4().to_string();
    let ab = Addressbook {
        principal: owner.to_string(),
        id: id.clone(),
        displayname: Some(name.trim().to_string()),
        description: Some(description.trim().to_string()).filter(|d| !d.is_empty()),
        deleted_at: None,
        synctoken: 0,
        push_topic: uuid::Uuid::new_v4().to_string(),
    };
    state.addressbooks.import_addressbook(ab, objects, false).await?;
    collections::set_addressbook_color_checked(state, user, owner, &id, color).await?;
    Ok((id, count))
}

/// Merges a .vcf file into an existing address book (a known UID is overwritten). Every card is parsed first;
/// the store has no batch write for cards, so they are then written one by one.
pub async fn import_addressbook(state: &AppState, user: &Principal, principal: &str, ab_id: &str, vcf: &[u8]) -> AppResult<usize> {
    collections::ensure_owner(user, principal)?;
    state.addressbooks.get_addressbook(principal, ab_id, false).await?;
    let cards = file_cards(vcf)?;
    if cards.is_empty() {
        return Err(AppError::bad("err-no-vcards"));
    }
    let n = cards.len();
    for (id, obj) in cards {
        AddressbookWriteStore::put_object(&*state.addressbooks, principal, ab_id, &id, obj, true).await?;
    }
    Ok(n)
}

#[cfg(test)]
mod pinned;
