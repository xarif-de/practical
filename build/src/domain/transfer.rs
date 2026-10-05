//! Import/export (plan D16) — export serialises RustiCal's objects with caldata's
//! generator, import parses with caldata and stores via the store crate
//! (`put_objects`), so nothing but RustiCal's own code touches the data.

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
    let ical = IcalCalendar::from_objects("-//xarif.de//PractiCal//DE".to_string(), objects.into_iter().map(|(_, o)| o.into()).collect(), vec![]);
    let name = cal.meta.displayname.clone().unwrap_or_else(|| cal_id.to_string());
    Ok((name, ical.generate()))
}

pub async fn export_addressbook(state: &AppState, user: &Principal, principal: &str, ab_id: &str) -> AppResult<(String, String)> {
    collections::ensure_owner(user, principal)?;
    let ab = state.addressbooks.get_addressbook(principal, ab_id, false).await?;
    let objects = AddressbookReadStore::get_objects(&*state.addressbooks, principal, ab_id).await?;
    let mut out = String::new();
    for (_, o) in objects {
        out.push_str(o.get_vcf().trim_end());
        out.push_str("\r\n");
    }
    Ok((ab.displayname.unwrap_or_else(|| ab_id.to_string()), out))
}

/// Imports every VEVENT/VTODO/VJOURNAL object of an .ics file into an EXISTING
/// calendar; objects with a known UID are overwritten (merge).
pub async fn import_calendar(state: &AppState, user: &Principal, principal: &str, cal_id: &str, ics: &[u8]) -> AppResult<usize> {
    collections::ensure_owner(user, principal)?;
    state.calendars.get_calendar(principal, cal_id, false).await?;
    let mut n = 0;
    let mut batch = vec![];
    for cal in IcalParser::from_slice(ics) {
        let cal = cal.map_err(|e| AppError::BadRequest(format!("ics: {e}")))?;
        for obj in cal.into_objects().map_err(|e| AppError::BadRequest(format!("ics: {e}")))? {
            let id = obj.get_uid().to_string();
            if id.is_empty() || id.contains('/') {
                continue;
            }
            batch.push((id, CalendarObject::from(obj)));
            n += 1;
        }
    }
    if batch.is_empty() {
        return Err(AppError::BadRequest("no calendar objects found".into()));
    }
    state.calendars.put_objects(principal, cal_id, batch, true).await?;
    Ok(n)
}

/// The objects of an .ics file for a NEW calendar and the component types it holds. The calendar-level
/// X-WR-* properties (name, description, colour, zone — the page reads them from the file) are dropped
/// before the objects are split, as RustiCal's own IMPORT does, so they do not land in every object.
fn new_calendar_objects(ics: &[u8]) -> AppResult<(Vec<CalendarObject>, Vec<CalendarObjectType>)> {
    let bad = |e: &dyn std::fmt::Display| AppError::BadRequest(format!("ics: {e}"));
    let (mut objects, mut found) = (vec![], vec![]);
    for cal in IcalParser::from_slice(ics) {
        let mut cal = cal.map_err(|e| bad(&e))?.mutable();
        for p in ["X-WR-CALNAME", "X-WR-CALDESC", "X-WR-CALCOLOR", "X-WR-TIMEZONE", "X-APPLE-CALENDAR-COLOR"] {
            cal.remove_property(p);
        }
        let cal = cal.build(&ParserOptions::default(), None).map_err(|e| bad(&e))?;
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
    Ok((objects, found))
}

/// Creates a calendar from an .ics file in one transaction (RustiCal's `import_calendar`): the
/// metadata as given, the component types given plus those the file holds. Returns (id, objects).
pub async fn import_new_calendar(state: &AppState, user: &Principal, n: collections::NewCalendar, ics: &[u8]) -> AppResult<(String, usize)> {
    collections::ensure_owner(user, &n.owner)?;
    if n.name.trim().is_empty() {
        return Err(AppError::BadRequest("name required".into()));
    }
    let (objects, found) = new_calendar_objects(ics)?;
    if objects.is_empty() {
        return Err(AppError::BadRequest("no calendar objects found".into()));
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
        principal: n.owner,
        id: id.clone(),
        // an unknown zone is left out: RustiCal's own IMPORT asserts on it
        timezone_id: Some(n.timezone.trim().to_string()).filter(|t| t.parse::<chrono_tz::Tz>().is_ok()),
        deleted_at: None,
        synctoken: 0,
        subscription_url: None,
        push_topic: uuid::Uuid::new_v4().to_string(),
        components,
    };
    state.calendars.import_calendar(cal, objects, false).await?;
    Ok((id, count))
}

/// Creates an address book from a .vcf file in one transaction (RustiCal's `import_addressbook`); a card
/// without UID gets one, as RustiCal's own IMPORT does. The colour (ours, not RustiCal's) is written
/// after the import succeeded. Returns (id, cards).
pub async fn import_new_addressbook(state: &AppState, user: &Principal, owner: &str, name: &str, color: &str, description: &str, vcf: &[u8]) -> AppResult<(String, usize)> {
    collections::ensure_owner(user, owner)?;
    if name.trim().is_empty() {
        return Err(AppError::BadRequest("name required".into()));
    }
    if !collections::valid_color(color) {
        return Err(AppError::BadRequest("color must be #rrggbb".into()));
    }
    let mut objects = vec![];
    for card in VcardParser::from_slice(vcf) {
        let mut card = card.map_err(|e| AppError::BadRequest(format!("vcf: {e}")))?;
        if card.get_uid().is_none() {
            let mut m = card.mutable();
            m.add_content_line(ContentLine { name: "UID".into(), value: uuid::Uuid::new_v4().to_string(), params: vec![].into() });
            card = m.build(&ParserOptions::default(), None).map_err(|e| AppError::BadRequest(format!("vcf: {e}")))?;
        }
        let id = card.get_uid().unwrap_or_default().to_string();
        if !id.is_empty() && !id.contains('/') {
            objects.push((id, AddressObject::from(card)));
        }
    }
    if objects.is_empty() {
        return Err(AppError::BadRequest("no vCards found".into()));
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

pub async fn import_addressbook(state: &AppState, user: &Principal, principal: &str, ab_id: &str, vcf: &[u8]) -> AppResult<usize> {
    collections::ensure_owner(user, principal)?;
    state.addressbooks.get_addressbook(principal, ab_id, false).await?;
    let mut n = 0;
    for card in VcardParser::from_slice(vcf) {
        let card = card.map_err(|e| AppError::BadRequest(format!("vcf: {e}")))?;
        let id = card.get_uid().map(str::to_string).unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        if id.contains('/') {
            continue;
        }
        let obj = AddressObject::from(card);
        AddressbookWriteStore::put_object(&*state.addressbooks, principal, ab_id, &id, obj, true).await?;
        n += 1;
    }
    if n == 0 {
        return Err(AppError::BadRequest("no vCards found".into()));
    }
    Ok(n)
}
