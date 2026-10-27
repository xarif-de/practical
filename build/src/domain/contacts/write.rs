//! Writing: `ContactInput` → vCard 3.0 text (`build_vcf`), create / update / delete through the store.

use super::*;

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
    pub nickname: String,
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub additional: String,
    #[serde(default)]
    pub suffix: String,
    #[serde(default)]
    pub org: String,
    #[serde(default)]
    pub department: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub emails: Vec<Typed>,
    #[serde(default)]
    pub phones: Vec<Typed>,
    #[serde(default)]
    pub addresses: Vec<Address>,
    #[serde(default)]
    pub urls: Vec<Typed>,
    #[serde(default)]
    pub impp: Vec<Typed>,
    #[serde(default)]
    pub related: Vec<Typed>,
    #[serde(default)]
    pub dates: Vec<Dated>,
    #[serde(default)]
    pub birthday: String,
    #[serde(default)]
    pub anniversary: String,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub note: String,
}

pub(super) fn type_param(kind: &str) -> String {
    let k = kind.trim().to_uppercase();
    if k.is_empty() || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') { String::new() } else { format!(";TYPE={k}") }
}

/// `;TYPE=KIND[,PREF]` — vCard 3.0 spelling of the preferred flag (Apple and DAVx⁵ read it).
pub(super) fn type_pref(kind: &str, pref: bool) -> String {
    let t = type_param(kind);
    match (t.is_empty(), pref) {
        (true, true) => ";TYPE=PREF".into(),
        (false, true) => format!("{t},PREF"),
        _ => t,
    }
}

/// A single-word parameter value; anything else is dropped rather than written unquoted.
pub(super) fn word(v: &str) -> Option<&str> {
    let v = v.trim();
    (!v.is_empty() && v.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')).then_some(v)
}

pub(super) fn partial_to_vcard(v: &str) -> Option<String> {
    let n = norm_partial(v);
    if n.is_empty() { None } else if let Some(md) = n.strip_prefix("--") { Some(format!("--{}", md.replace('-', ""))) } else { Some(n.replace('-', "")) }
}

pub fn build_vcf(uid: &str, i: &ContactInput, preserved: &[String]) -> AppResult<String> {
    let given = i.given.trim();
    let family = i.family.trim();
    let mut full = i.full_name.trim().to_string();
    if full.is_empty() {
        full = format!("{given} {family}").trim().to_string();
    }
    if full.is_empty() {
        return Err(AppError::bad("err-name-required"));
    }
    let mut l = vec![
        "BEGIN:VCARD".to_string(),
        "VERSION:3.0".to_string(),
        "PRODID:-//xarif.de//PractiCal//DE".to_string(),
        format!("UID:{uid}"),
        format!("FN:{}", ical::escape(&full)),
        format!("N:{};{};{};{};{}", ical::escape(family), ical::escape(given), ical::escape(i.additional.trim()), ical::escape(i.prefix.trim()), ical::escape(i.suffix.trim())),
        format!("REV:{}", ical::fmt_utc_now()),
    ];
    if !i.nickname.trim().is_empty() {
        l.push(format!("NICKNAME:{}", ical::escape(i.nickname.trim())));
    }
    if !i.org.trim().is_empty() || !i.department.trim().is_empty() {
        l.push(format!("ORG:{};{}", ical::escape(i.org.trim()), ical::escape(i.department.trim())));
    }
    if !i.title.trim().is_empty() {
        l.push(format!("TITLE:{}", ical::escape(i.title.trim())));
    }
    for e in &i.emails {
        if !e.value.trim().is_empty() {
            l.push(format!("EMAIL{}:{}", type_pref(&e.kind, e.pref), ical::escape(e.value.trim())));
        }
    }
    for p in &i.phones {
        if !p.value.trim().is_empty() {
            l.push(format!("TEL{}:{}", type_pref(&p.kind, p.pref), ical::escape(p.value.trim())));
        }
    }
    for a in &i.addresses {
        if !a.one_line().is_empty() {
            l.push(format!("ADR{}:;;{};{};{};{};{}", type_pref(&a.kind, a.pref), ical::escape(a.street.trim()), ical::escape(a.city.trim()), ical::escape(a.region.trim()), ical::escape(a.code.trim()), ical::escape(a.country.trim())));
        }
    }
    for u in &i.urls {
        if !u.value.trim().is_empty() {
            l.push(format!("URL{}:{}", type_param(&u.kind), u.value.trim()));
        }
    }
    for m in &i.impp {
        if !m.value.trim().is_empty() {
            let svc = word(&m.kind).map(|w| format!(";X-SERVICE-TYPE={w}")).unwrap_or_default();
            l.push(format!("IMPP{svc}:{}", ical::escape(m.value.trim())));
        }
    }
    for r in &i.related {
        if !r.value.trim().is_empty() {
            l.push(format!("RELATED{};VALUE=text:{}", type_param(&r.kind), ical::escape(r.value.trim())));
        }
    }
    for d in &i.dates {
        if let Some(v) = partial_to_vcard(&d.date) {
            let label = word(&d.label).map(|w| format!(";X-ABLABEL={w}")).unwrap_or_default();
            l.push(format!("X-ABDATE{label}:{v}"));
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
    l.extend(preserved.iter().cloned());
    l.push("END:VCARD".into());
    Ok(l.iter().map(|x| ical::fold(x)).collect::<Vec<_>>().join("\r\n") + "\r\n")
}

pub async fn create_contact(state: &AppState, user: &Principal, i: ContactInput) -> AppResult<String> {
    create_contact_as(state, user, i, None).await
}

/// `keep_uid`: a move to another address book keeps the UID (see `events::create_event_as`).
pub async fn create_contact_as(state: &AppState, user: &Principal, i: ContactInput, keep_uid: Option<&str>) -> AppResult<String> {
    let ab = collections::ensure_addressbook(state, user, &i.principal, &i.ab_id, Need::Write).await?;
    let keep = match keep_uid {
        Some(u) if matches!(stored_card(state, &ab, u).await, Err(AppError::NotFound | AppError::Store(rustical_store::Error::NotFound))) => Some(u.to_string()),
        _ => None,
    };
    let uid = keep.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let obj = AddressObject::from_vcf(build_vcf(&uid, &i, &[])?)?;
    state.collections().put_address_object(&ab, &uid, obj, keep.is_some(), None).await?;
    Ok(uid)
}

pub async fn update_contact(state: &AppState, user: &Principal, object_id: &str, i: ContactInput, expected_etag: Option<&str>) -> AppResult<()> {
    let ab = collections::ensure_addressbook(state, user, &i.principal, &i.ab_id, Need::Write).await?;
    let (existing, etag) = stored_card(state, &ab, object_id).await?;
    if let Some(e) = expected_etag
        && !e.is_empty()
        && e != etag
    {
        return Err(AppError::conflict("err-changed-contact"));
    }
    let uid = parse_vcf(&existing)?.get_uid().unwrap_or(object_id).to_string();
    // the lines kept as written: a bare parameter among them as TYPE, or caldata rejects the saved card
    let keep = preserved_lines(&bare_params_as_type(&existing));
    let obj = AddressObject::from_vcf(build_vcf(&uid, &i, &keep)?)?;
    state.collections().put_address_object(&ab, object_id, obj, true, Some(&etag)).await?;
    Ok(())
}
