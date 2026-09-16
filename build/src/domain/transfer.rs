//! Import/export (plan D16) — export serialises RustiCal's objects with caldata's
//! generator, import parses with caldata and stores via the store crate
//! (`put_objects`), so nothing but RustiCal's own code touches the data.

use crate::domain::collections;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use caldata::component::IcalCalendar;
use caldata::generator::Emitter;
use caldata::{IcalParser, VcardParser};
use rustical_ical::{AddressObject, CalendarObject};
use rustical_store::auth::Principal;
use rustical_store::{AddressbookReadStore, AddressbookWriteStore, CalendarReadStore, CalendarWriteStore};

pub async fn export_calendar(state: &AppState, user: &Principal, principal: &str, cal_id: &str) -> AppResult<(String, String)> {
    collections::ensure_owner(user, principal)?;
    let cal = state.calendars.get_calendar(principal, cal_id, false).await?;
    let objects = state.calendars.get_objects(principal, cal_id).await?;
    let ical = IcalCalendar::from_objects("-//xarif.de//rustical-ui//DE".to_string(), objects.into_iter().map(|(_, o)| o.into()).collect(), vec![]);
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
