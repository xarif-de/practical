//! Contacts on RustiCal's address books. Parsing through caldata's vCard
//! component; writing generates vCard 3.0 (the dialect Apple and DAVx⁵ both
//! round-trip) and stores it through the store crate.

use crate::domain::collections::{self, AbInfo};
use crate::domain::ical;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use caldata::VcardParser;
use caldata::component::{Component, VcardContact};
use rustical_ical::AddressObject;
use rustical_store::auth::Principal;
use rustical_store::{AddressbookReadStore, AddressbookWriteStore};

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Typed {
    pub kind: String,
    pub value: String,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Contact {
    pub principal: String,
    pub ab_id: String,
    pub ab_name: String,
    /// stable colour per address book (collections::default_color), for the list chip
    pub ab_color: String,
    pub is_group: bool,
    pub object_id: String,
    pub uid: String,
    pub full_name: String,
    pub given: String,
    pub family: String,
    pub org: String,
    pub title: String,
    pub emails: Vec<Typed>,
    pub phones: Vec<Typed>,
    pub addresses: Vec<Typed>,
    /// YYYY-MM-DD or --MM-DD
    pub birthday: String,
    pub anniversary: String,
    pub categories: Vec<String>,
    pub note: String,
    pub url: String,
    pub etag: String,
}

impl Contact {
    pub fn initials(&self) -> String {
        let s: String = self.full_name.split_whitespace().filter_map(|w| w.chars().next()).take(2).collect();
        if s.is_empty() { "?".into() } else { s.to_uppercase() }
    }
    pub fn letter(&self) -> String {
        let key = if !self.family.is_empty() { &self.family } else { &self.full_name };
        key.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "#".into())
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
        self.addresses.first().map(|t| t.value.clone()).unwrap_or_default()
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

/// Normalise BDAY/ANNIVERSARY values (19840312, 1984-03-12, --0312, --03-12, 19840312T...) → ISO.
pub fn norm_partial(v: &str) -> String {
    let v = v.split('T').next().unwrap_or("").trim();
    if let Some(md) = v.strip_prefix("--") {
        let md = md.replace('-', "");
        if md.len() == 4 {
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

fn kind_of(line: &caldata::parser::ContentLine) -> String {
    line.params
        .get_param("TYPE")
        .map(|t| t.to_lowercase())
        .unwrap_or_default()
        .split(',')
        .find(|t| !t.is_empty() && *t != "pref" && *t != "voice" && *t != "internet")
        .unwrap_or("")
        .to_string()
}

pub fn parse_vcf(vcf: &str) -> AppResult<VcardContact> {
    Ok(VcardParser::from_slice(vcf.as_bytes()).expect_one()?)
}

pub fn contact_from(vc: &VcardContact, ab: &AbInfo, object_id: &str, etag: String) -> Contact {
    let n: Vec<String> = vc.name.as_ref().map(|n| n.0.split(';').map(ical::unescape).collect()).unwrap_or_default();
    let full_name = vc.full_name.first().map(|f| ical::unescape(&f.0)).unwrap_or_default();
    let family = n.first().cloned().unwrap_or_default();
    let given = n.get(1).cloned().unwrap_or_default();
    let full_name = if full_name.is_empty() { format!("{given} {family}").trim().to_string() } else { full_name };
    let typed = |name: &str| -> Vec<Typed> {
        vc.get_named_properties(name).map(|l| Typed { kind: kind_of(l), value: ical::unescape(&l.value) }).filter(|t| !t.value.is_empty()).collect()
    };
    let addresses: Vec<Typed> = vc
        .get_named_properties("ADR")
        .map(|l| {
            let parts: Vec<String> = l.value.split(';').map(ical::unescape).collect();
            // ADR: pobox;ext;street;locality;region;postal;country
            let street = parts.get(2).cloned().unwrap_or_default();
            let city = format!("{} {}", parts.get(5).cloned().unwrap_or_default(), parts.get(3).cloned().unwrap_or_default()).trim().to_string();
            let country = parts.get(6).cloned().unwrap_or_default();
            let v = [street, city, country].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", ");
            Typed { kind: kind_of(l), value: v }
        })
        .filter(|t| !t.value.is_empty())
        .collect();
    Contact {
        principal: ab.principal.clone(),
        ab_id: ab.id.clone(),
        ab_name: ab.displayname.clone(),
        ab_color: collections::default_color(&ab.id),
        is_group: ab.is_group,
        object_id: object_id.to_string(),
        uid: vc.get_uid().unwrap_or(object_id).to_string(),
        full_name,
        given,
        family,
        org: ical::text(vc, "ORG").map(|o| o.split(';').next().unwrap_or("").to_string()).unwrap_or_default(),
        title: ical::text(vc, "TITLE").unwrap_or_default(),
        emails: typed("EMAIL"),
        phones: typed("TEL"),
        addresses,
        birthday: vc.get_property("BDAY").map(|l| norm_partial(&l.value)).unwrap_or_default(),
        anniversary: vc.get_property("ANNIVERSARY").map(|l| norm_partial(&l.value)).unwrap_or_default(),
        categories: ical::list(vc, "CATEGORIES"),
        note: ical::text(vc, "NOTE").unwrap_or_default(),
        url: ical::text(vc, "URL").unwrap_or_default(),
        etag,
    }
}

pub async fn contacts_for(state: &AppState, abs: &[AbInfo]) -> AppResult<Vec<Contact>> {
    let mut out = vec![];
    for ab in abs {
        for (id, obj) in AddressbookReadStore::get_objects(&*state.addressbooks, &ab.principal, &ab.id).await? {
            match parse_vcf(obj.get_vcf()) {
                Ok(vc) => out.push(contact_from(&vc, ab, &id, obj.get_etag())),
                Err(e) => tracing::warn!("contact {}/{}/{}: {e}", ab.principal, ab.id, id),
            }
        }
    }
    out.sort_by_key(|c| c.sort_key());
    Ok(out)
}

pub async fn get_contact(state: &AppState, user: &Principal, principal: &str, ab_id: &str, object_id: &str) -> AppResult<Contact> {
    collections::ensure_owner(user, principal)?;
    let ab_row = state.addressbooks.get_addressbook(principal, ab_id, false).await?;
    let ab = AbInfo { principal: principal.into(), owner_label: String::new(), is_group: principal.starts_with("group."), id: ab_id.into(), displayname: ab_row.displayname.unwrap_or_default(), description: String::new(), deleted_at: None, synctoken: 0 };
    let obj = AddressbookReadStore::get_object(&*state.addressbooks, principal, ab_id, object_id, false).await?;
    Ok(contact_from(&parse_vcf(obj.get_vcf())?, &ab, object_id, obj.get_etag()))
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct ContactInput {
    pub principal: String,
    pub ab_id: String,
    #[serde(default)]
    pub given: String,
    #[serde(default)]
    pub family: String,
    #[serde(default)]
    pub full_name: String,
    #[serde(default)]
    pub org: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub emails: Vec<Typed>,
    #[serde(default)]
    pub phones: Vec<Typed>,
    #[serde(default)]
    pub addresses: Vec<Typed>,
    #[serde(default)]
    pub birthday: String,
    #[serde(default)]
    pub anniversary: String,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub url: String,
}

fn type_param(kind: &str) -> String {
    let k = kind.trim().to_uppercase();
    if k.is_empty() || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') { String::new() } else { format!(";TYPE={k}") }
}

fn partial_to_vcard(v: &str) -> Option<String> {
    let n = norm_partial(v);
    if n.is_empty() { None } else if let Some(md) = n.strip_prefix("--") { Some(format!("--{}", md.replace('-', ""))) } else { Some(n.replace('-', "")) }
}

pub fn build_vcf(uid: &str, i: &ContactInput) -> AppResult<String> {
    let given = i.given.trim();
    let family = i.family.trim();
    let mut full = i.full_name.trim().to_string();
    if full.is_empty() {
        full = format!("{given} {family}").trim().to_string();
    }
    if full.is_empty() {
        return Err(AppError::BadRequest("name required".into()));
    }
    let mut l = vec![
        "BEGIN:VCARD".to_string(),
        "VERSION:3.0".to_string(),
        "PRODID:-//xarif.de//rustical-ui//DE".to_string(),
        format!("UID:{uid}"),
        format!("FN:{}", ical::escape(&full)),
        format!("N:{};{};;;", ical::escape(family), ical::escape(given)),
        format!("REV:{}", ical::fmt_utc_now()),
    ];
    if !i.org.trim().is_empty() {
        l.push(format!("ORG:{}", ical::escape(i.org.trim())));
    }
    if !i.title.trim().is_empty() {
        l.push(format!("TITLE:{}", ical::escape(i.title.trim())));
    }
    for e in &i.emails {
        if !e.value.trim().is_empty() {
            l.push(format!("EMAIL{}:{}", type_param(&e.kind), ical::escape(e.value.trim())));
        }
    }
    for p in &i.phones {
        if !p.value.trim().is_empty() {
            l.push(format!("TEL{}:{}", type_param(&p.kind), ical::escape(p.value.trim())));
        }
    }
    for a in &i.addresses {
        // free-text address goes into the street component; clients show it verbatim
        if !a.value.trim().is_empty() {
            l.push(format!("ADR{}:;;{};;;;", type_param(&a.kind), ical::escape(a.value.trim())));
        }
    }
    if let Some(b) = partial_to_vcard(&i.birthday) {
        l.push(format!("BDAY:{b}"));
    }
    if let Some(a) = partial_to_vcard(&i.anniversary) {
        l.push(format!("ANNIVERSARY:{a}"));
    }
    let cats: Vec<String> = i.categories.iter().map(|c| c.trim()).filter(|c| !c.is_empty()).map(ical::escape).collect();
    if !cats.is_empty() {
        l.push(format!("CATEGORIES:{}", cats.join(",")));
    }
    if !i.note.trim().is_empty() {
        l.push(format!("NOTE:{}", ical::escape(i.note.trim())));
    }
    if !i.url.trim().is_empty() {
        l.push(format!("URL:{}", i.url.trim()));
    }
    l.push("END:VCARD".into());
    Ok(l.iter().map(|x| ical::fold(x)).collect::<Vec<_>>().join("\r\n") + "\r\n")
}

pub async fn create_contact(state: &AppState, user: &Principal, i: ContactInput) -> AppResult<String> {
    collections::ensure_owner(user, &i.principal)?;
    let uid = uuid::Uuid::new_v4().to_string();
    let obj = AddressObject::from_vcf(build_vcf(&uid, &i)?)?;
    state.addressbooks.put_object(&i.principal, &i.ab_id, &uid, obj, false).await?;
    Ok(uid)
}

pub async fn update_contact(state: &AppState, user: &Principal, object_id: &str, i: ContactInput, expected_etag: Option<&str>) -> AppResult<()> {
    collections::ensure_owner(user, &i.principal)?;
    let existing = AddressbookReadStore::get_object(&*state.addressbooks, &i.principal, &i.ab_id, object_id, false).await?;
    if let Some(e) = expected_etag
        && !e.is_empty()
        && e != existing.get_etag()
    {
        return Err(AppError::Conflict("the contact was changed elsewhere; reload".into()));
    }
    let uid = parse_vcf(existing.get_vcf())?.get_uid().unwrap_or(object_id).to_string();
    let obj = AddressObject::from_vcf(build_vcf(&uid, &i)?)?;
    state.addressbooks.put_object(&i.principal, &i.ab_id, object_id, obj, true).await?;
    Ok(())
}

pub async fn delete_contact(state: &AppState, user: &Principal, principal: &str, ab_id: &str, object_id: &str) -> AppResult<()> {
    collections::ensure_owner(user, principal)?;
    AddressbookWriteStore::delete_object(&*state.addressbooks, principal, ab_id, object_id, true).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vcard_roundtrip() {
        let i = ContactInput {
            principal: "p".into(),
            ab_id: "a".into(),
            given: "Christin".into(),
            family: "Lang".into(),
            emails: vec![Typed { kind: "home".into(), value: "christin@example.org".into() }],
            phones: vec![Typed { kind: "cell".into(), value: "+49 160 2223344".into() }],
            addresses: vec![Typed { kind: "home".into(), value: "Parkstraße 12, 10115 Berlin".into() }],
            birthday: "1984-03-12".into(),
            categories: vec!["Familie".into()],
            note: "Allergie: Nüsse".into(),
            ..Default::default()
        };
        let vcf = build_vcf("u1", &i).unwrap();
        let vc = parse_vcf(&vcf).unwrap();
        let ab = AbInfo { principal: "p".into(), owner_label: String::new(), is_group: false, id: "a".into(), displayname: "Privat".into(), description: String::new(), deleted_at: None, synctoken: 0 };
        let c = contact_from(&vc, &ab, "u1", String::new());
        assert_eq!(c.full_name, "Christin Lang");
        assert_eq!(c.family, "Lang");
        assert_eq!(c.primary_email(), "christin@example.org");
        assert_eq!(c.phones[0].kind, "cell");
        assert_eq!(c.primary_address(), "Parkstraße 12, 10115 Berlin");
        assert_eq!(c.birthday, "1984-03-12");
        assert_eq!(c.birthday_label(), "12.03.1984");
        assert_eq!(c.categories, vec!["Familie"]);
        assert_eq!(c.letter(), "L");
        assert_eq!(norm_partial("--0312"), "--03-12");
        assert_eq!(fmt_partial("--03-12"), "12.03.");
    }
}
