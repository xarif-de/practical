//! Contacts on RustiCal's address books. Parsing through caldata's vCard
//! component; writing generates vCard 3.0 (the dialect Apple and DAVx⁵ both
//! round-trip) and stores it through the store crate.
//!
//! - [`Contact`] is the parsed card, [`ContactCache`] keeps them per address book and synctoken.
//! - [`matches()`]: address-book entries for an ORGANIZER/ATTENDEE (same e-mail, else family name plus a given
//!   name or its prefix; rooms and resources never).
//! - [`yearly_in`]: a yearly date in a given year (29 February → 28 February in a common year).
//! - Reading (vcard.rs): [`raw_lines`] (parameters from the raw text: caldata keeps only the first),
//!   [`preserved_lines`] (copied verbatim on a save), [`bare_params_as_type`] (vCard 2.1 bare parameters).
//! - Writing (write.rs): vCard 3.0. A `.vcf` import without UID gets one ([`crate::domain::transfer`]).

use crate::domain::collections::{self, AbInfo};
use crate::domain::ical;
use crate::error::{AppError, AppResult};
use crate::state::{AppState, Authorized, Need};
use caldata::VcardParser;
use caldata::component::{Component, VcardContact};
use chrono::{Datelike, NaiveDate};
use rustical_ical::AddressObject;
use rustical_store::auth::Principal;

mod vcard;
mod write;
pub use vcard::*;
pub use write::*;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Typed {
    pub kind: String,
    pub value: String,
    /// TYPE=PREF (vCard 3) / PREF=1 (vCard 4): the number or address to use first
    #[serde(default)]
    pub pref: bool,
}

/// ADR components we edit (PO box and extended address are dropped on write).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Address {
    pub kind: String,
    #[serde(default)]
    pub street: String,
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub city: String,
    #[serde(default)]
    pub region: String,
    #[serde(default)]
    pub country: String,
    #[serde(default)]
    pub pref: bool,
}

impl Address {
    pub fn one_line(&self) -> String {
        let place = format!("{} {}", self.code, self.city).trim().to_string();
        [self.street.as_str(), place.as_str(), self.region.as_str(), self.country.as_str()].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(", ")
    }
}

/// X-ABDATE with a label (name day, …): date as YYYY-MM-DD or --MM-DD.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Dated {
    pub label: String,
    pub date: String,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Contact {
    pub principal: String,
    pub ab_id: String,
    pub ab_name: String,
    /// the address book's colour (AbInfo.color), for the chips
    pub ab_color: String,
    pub is_group: bool,
    /// its address book is shared with the user to read (`AbInfo::read_only`): the editor stays in view mode
    #[serde(default)]
    pub read_only: bool,
    pub object_id: String,
    pub uid: String,
    pub full_name: String,
    pub given: String,
    pub family: String,
    pub nickname: String,
    pub prefix: String,
    pub additional: String,
    pub suffix: String,
    pub org: String,
    pub department: String,
    pub title: String,
    pub emails: Vec<Typed>,
    pub phones: Vec<Typed>,
    pub addresses: Vec<Address>,
    pub urls: Vec<Typed>,
    /// IMPP: kind = service (Signal, WhatsApp, Matrix, …), value = handle
    pub impp: Vec<Typed>,
    /// RELATED: kind = spouse/child/parent/…, value = the name
    pub related: Vec<Typed>,
    pub dates: Vec<Dated>,
    /// YYYY-MM-DD or --MM-DD
    pub birthday: String,
    pub anniversary: String,
    pub categories: Vec<String>,
    pub note: String,
    pub etag: String,
}

impl Contact {
    pub fn initials(&self) -> String {
        let s: String = self.full_name.split_whitespace().filter_map(|w| w.chars().next()).take(2).collect();
        if s.is_empty() { "?".into() } else { s.to_uppercase() }
    }
    /// the page's default order and grouping: given name first (app.js regroups for the other sorts)
    fn given_or_full(&self) -> &str {
        if self.given.is_empty() { &self.full_name } else { &self.given }
    }
    pub fn given_letter(&self) -> String {
        self.given_or_full().chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "#".into())
    }
    pub fn given_key(&self) -> String {
        format!("{} {}", self.given_or_full(), self.family).to_lowercase()
    }
    pub fn sort_key(&self) -> String {
        format!("{} {}", if self.family.is_empty() { &self.full_name } else { &self.family }, self.given).to_lowercase()
    }
    pub fn primary_phone(&self) -> String {
        self.phones.first().map(|t| t.value.clone()).unwrap_or_default()
    }
    pub fn primary_email(&self) -> String {
        self.emails.first().map(|t| t.value.clone()).unwrap_or_default()
    }
    pub fn primary_address(&self) -> String {
        self.addresses.first().map(Address::one_line).unwrap_or_default()
    }
    pub fn category(&self) -> String {
        self.categories.first().cloned().unwrap_or_default()
    }
    /// birthday as DD.MM.YYYY / DD.MM.
    pub fn birthday_label(&self) -> String {
        fmt_partial(&self.birthday)
    }
    pub fn color_index(&self) -> usize {
        self.uid.bytes().fold(0usize, |a, b| a.wrapping_mul(31).wrapping_add(usize::from(b))) % 6 + 1
    }
}

pub fn fmt_partial(s: &str) -> String {
    let p: Vec<&str> = s.split('-').collect();
    match (s.starts_with("--"), p.len()) {
        (true, 4) => format!("{}.{}.", p[3], p[2]),
        (false, 3) => format!("{}.{}.{}", p[2], p[1], p[0]),
        _ => s.to_string(),
    }
}

/// (year if given, month, day) of a `norm_partial` date: `1984-02-29`, `--02-29`.
pub fn partial_parts(s: &str) -> Option<(Option<i32>, u32, u32)> {
    let (y, md) = match s.strip_prefix("--") {
        Some(md) => (None, md),
        None => {
            let (y, md) = s.split_once('-')?;
            (Some(y.parse().ok()?), md)
        }
    };
    let (m, d) = md.split_once('-')?;
    Some((y, m.parse().ok()?, d.parse().ok()?))
}

/// The day a yearly date (birthday, anniversary) falls on in `year`. A day its month lacks that year —
/// 29 February in a common year — is the month's last day, 28 February: where Apple's and Google's
/// calendars show such a birthday (German law would make the person a year older on 1 March only,
/// BGB §187(2), §188(2)/(3): the year ends with 28 February). One rule for every view.
pub fn yearly_in(year: i32, month: u32, day: u32) -> Option<NaiveDate> {
    if day == 0 {
        return None;
    }
    let first = NaiveDate::from_ymd_opt(year, month, 1)?;
    let last = first.checked_add_months(chrono::Months::new(1))?.pred_opt()?.day();
    NaiveDate::from_ymd_opt(year, month, day.min(last))
}

/// The first day on or after `today` the yearly date falls on (`yearly_in`).
pub fn next_yearly(month: u32, day: u32, today: NaiveDate) -> Option<NaiveDate> {
    yearly_in(today.year(), month, day).filter(|d| *d >= today).or_else(|| yearly_in(today.year() + 1, month, day))
}

/// Normalise BDAY/ANNIVERSARY values (19840312, 1984-03-12, --0312, --03-12, 19840312T...) → ISO.
pub fn norm_partial(v: &str) -> String {
    let v = v.split('T').next().unwrap_or("").trim();
    if let Some(md) = v.strip_prefix("--") {
        let md = md.replace('-', "");
        if md.len() == 4 && md.bytes().all(|b| b.is_ascii_digit()) {
            return format!("--{}-{}", &md[..2], &md[2..]);
        }
        return String::new();
    }
    let d = v.replace('-', "");
    if d.len() == 8 && d.chars().all(|c| c.is_ascii_digit()) {
        return format!("{}-{}-{}", &d[..4], &d[4..6], &d[6..]);
    }
    String::new()
}

/// A contact as a search hit or as the address-book entry behind an ORGANIZER/ATTENDEE.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ContactHit {
    pub key: String,
    pub name: String,
    pub sub: String,
    pub initials: String,
    pub color_index: usize,
    /// `sub` is the birthday: the client puts a cake in front of it
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub bday: bool,
    /// why the top-bar search found it (its "all results" dialog)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub why: Vec<crate::domain::search::Why>,
}

impl ContactHit {
    pub fn of(c: &Contact, sub: String) -> Self {
        ContactHit { key: format!("{}/{}/{}", c.principal, c.ab_id, c.object_id), name: c.full_name.clone(), sub, initials: c.initials(), color_index: c.color_index(), bday: false, why: vec![] }
    }
}

/// Address-book entries that look like this ORGANIZER/ATTENDEE. An exact e-mail match wins; without one
/// the names are compared: equal, or same family name with given names equal or one a prefix of the
/// other ("Max" ~ "Maximilian"). A single-word name never matches (it would hit every namesake); rooms
/// and resources are never matched. At most five hits, the caller offers a choice when there are several.
pub fn matches(all: &[Contact], name: &str, email: &str, cutype: &str) -> Vec<ContactHit> {
    if matches!(cutype, "ROOM" | "RESOURCE") {
        return vec![];
    }
    let sub = |c: &Contact| { let e = c.primary_email(); if !e.is_empty() { e } else { c.org.clone() } };
    let email = email.trim().to_lowercase();
    if !email.is_empty() {
        let by_mail: Vec<ContactHit> = all.iter().filter(|c| c.emails.iter().any(|e| e.value.trim().eq_ignore_ascii_case(&email))).map(|c| ContactHit::of(c, sub(c))).collect();
        if !by_mail.is_empty() {
            return by_mail;
        }
    }
    let toks = |s: &str| s.to_lowercase().split(|ch: char| !ch.is_alphanumeric()).filter(|t| t.len() >= 2).map(str::to_string).collect::<Vec<_>>();
    let n = toks(name);
    let Some((n_last, n_given)) = n.split_last() else { return vec![] };
    let Some(n_first) = n_given.first() else { return vec![] };
    let similar = |c: &Contact| {
        let full = toks(&c.full_name);
        let Some((c_last, c_given)) = full.split_last() else { return false };
        full == n || (c_last == n_last && c_given.first().is_some_and(|c_first| c_first.starts_with(n_first.as_str()) || n_first.starts_with(c_first.as_str())))
    };
    all.iter().filter(|c| similar(c)).take(5).map(|c| ContactHit::of(c, sub(c))).collect()
}

pub async fn contacts_for(state: &AppState, abs: &[AbInfo]) -> AppResult<Vec<Contact>> {
    let mut out = vec![];
    for ab in abs {
        for c in ab_contacts(state, ab).await?.contacts.values() {
            let mut c = c.clone();
            // name and colour change without the synctoken moving: always the current ones
            (c.ab_name, c.ab_color, c.is_group, c.read_only) = (ab.displayname.clone(), ab.color.clone(), ab.is_group, ab.read_only);
            out.push(c);
        }
    }
    out.sort_by_key(|c| c.sort_key());
    Ok(out)
}

fn cached_contact(ab: &AbInfo, id: &str, vcf: &str, etag: String) -> Option<Contact> {
    match parse_vcf(vcf) {
        Ok(vc) => crate::domain::events::per_object(&format!("{}/{}/{id}", ab.principal, ab.id), || contact_from(&vc, &raw_lines(vcf), ab, id, etag)),
        Err(e) => {
            tracing::warn!("contact {}/{}/{}: {e}", ab.principal, ab.id, id);
            None
        }
    }
}

/// The parsed contacts of one address book at a synctoken (RustiCal moves it on every object write,
/// store_sqlite addressbook_store/mod.rs `log_object_operation`). Keyed by object id: the store reads
/// an address book in that order (primary-key index), which the stable sort in `contacts_for` keeps
/// for equal names.
pub struct AbContacts {
    token: i64,
    contacts: std::collections::BTreeMap<String, Contact>,
}

/// `AbContacts` per address book, caught up through the store's change log like `tasks::TaskCache`:
/// a page load no longer parses every vCard twice and hashes it (≈ 300 contacts: 9 → 1 ms per /contacts
/// on the synthetic DB).
#[derive(Default)]
pub struct ContactCache(std::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<AbContacts>>>);

async fn ab_contacts(state: &AppState, ab: &AbInfo) -> AppResult<std::sync::Arc<AbContacts>> {
    // the address book, its view and its path (`state::Authorized::cache_key`)
    let k = ab.auth.cache_key();
    let cached = state.caches.contacts.0.lock().unwrap().get(&k).cloned();
    if let Some(c) = &cached
        && c.token >= ab.synctoken
    {
        return Ok(c.clone());
    }
    // `ab.synctoken` was read before the objects: a write racing this read is applied again next time
    // a changed card the store cannot parse fails its whole change list (store_sqlite `_sync_changes`):
    // then the book is read anew, which takes such a card from the table (`unparsed_cards`)
    let changes = match &cached {
        Some(c) => match state.collections().address_sync_changes(&ab.auth, c.token).await {
            Ok(ch) => Some(ch),
            Err(AppError::Store(rustical_store::Error::IcalError(_))) => None,
            Err(e) => return Err(e),
        },
        None => None,
    };
    let fresh = match (cached, changes) {
        (Some(c), Some((changed, deleted, _))) => {
            let mut contacts = c.contacts.clone();
            for id in deleted {
                contacts.remove(&id);
            }
            for (id, obj) in changed {
                contacts.remove(&id);
                if let Some(ct) = cached_contact(ab, &id, obj.get_vcf(), obj.get_etag()) {
                    contacts.insert(id, ct);
                }
            }
            AbContacts { token: ab.synctoken, contacts }
        }
        _ => {
            let objs = state.collections().address_objects(&ab.auth).await?;
            let mut contacts: std::collections::BTreeMap<String, Contact> = objs.into_iter().filter_map(|(id, obj)| cached_contact(ab, &id, obj.get_vcf(), obj.get_etag()).map(|c| (id, c))).collect();
            let n_store = contacts.len();
            for (id, vcf) in unparsed_cards(state, &ab.auth, &|id| contacts.contains_key(id), n_store).await? {
                if let Some(c) = cached_contact(ab, &id, &vcf, etag_of(&vcf)) {
                    contacts.insert(id, c);
                }
            }
            AbContacts { token: ab.synctoken, contacts }
        }
    };
    let fresh = std::sync::Arc::new(fresh);
    let mut m = state.caches.contacts.0.lock().unwrap();
    if m.get(&k).is_none_or(|c| c.token <= fresh.token) {
        m.insert(k, fresh.clone());
    }
    Ok(fresh)
}

pub async fn get_contact(state: &AppState, user: &Principal, principal: &str, ab_id: &str, object_id: &str) -> AppResult<Contact> {
    let (auth, ab_row) = state.collections().ensure_addressbook(user, principal, ab_id, Need::Read, false).await?;
    let mut ab = AbInfo::bare(auth);
    ab.is_group = principal.starts_with("group.");
    ab.displayname = ab_row.displayname.unwrap_or_default();
    let (vcf, etag) = stored_card(state, &ab.auth, object_id).await?;
    Ok(contact_from(&parse_vcf(&vcf)?, &raw_lines(&vcf), &ab, object_id, etag))
}

/// The stored vCard text of one contact (download button).
pub async fn get_vcf(state: &AppState, user: &Principal, principal: &str, ab_id: &str, object_id: &str) -> AppResult<String> {
    let ab = collections::ensure_addressbook(state, user, principal, ab_id, Need::Read).await?;
    Ok(stored_card(state, &ab, object_id).await?.0)
}

#[cfg(test)]
mod pinned;

#[cfg(test)]
mod tests;
