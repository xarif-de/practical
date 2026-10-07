//! Contacts on RustiCal's address books. Parsing through caldata's vCard
//! component; writing generates vCard 3.0 (the dialect Apple and DAVx⁵ both
//! round-trip) and stores it through the store crate.

use crate::domain::collections::{self, AbInfo};
use crate::domain::ical;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use caldata::VcardParser;
use caldata::component::{Component, VcardContact};
use chrono::{Datelike, NaiveDate};
use rustical_ical::AddressObject;
use rustical_store::auth::Principal;
use rustical_store::{AddressbookReadStore, AddressbookWriteStore};

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

/// One unfolded vCard content line with every parameter value — caldata's `get_param` returns only
/// the first value, which hides `TYPE=HOME,PREF` and Apple's repeated `;type=…` parameters.
#[derive(Debug, Clone, Default)]
pub struct RawLine {
    pub name: String,
    /// (NAME, value) per value; `TYPE=HOME,PREF` becomes two entries, a bare `CELL` (vCard 2.1) a TYPE
    pub params: Vec<(String, String)>,
    pub value: String,
}

fn unfold(vcf: &str) -> Vec<String> {
    let mut lines: Vec<String> = vec![];
    for raw in vcf.split("\r\n").flat_map(|l| l.split('\n')) {
        if let Some(cont) = raw.strip_prefix(' ').or_else(|| raw.strip_prefix('\t')) {
            if let Some(last) = lines.last_mut() {
                last.push_str(cont);
            }
        } else if !raw.trim().is_empty() {
            lines.push(raw.to_string());
        }
    }
    lines
}

/// Byte length of a content line's head (name and parameters): up to its first `:` outside double quotes.
fn head_len(l: &str) -> usize {
    let mut q = false;
    for (i, c) in l.char_indices() {
        match c {
            '"' => q = !q,
            ':' if !q => return i,
            _ => {}
        }
    }
    l.len()
}

/// Split at `sep` outside double quotes (the quotes stay in the parts).
fn split_unquoted(s: &str, sep: char) -> Vec<String> {
    let (mut out, mut cur, mut q) = (vec![], String::new(), false);
    for c in s.chars() {
        match c {
            '"' => {
                q = !q;
                cur.push(c);
            }
            c if c == sep && !q => out.push(std::mem::take(&mut cur)),
            c => cur.push(c),
        }
    }
    out.push(cur);
    out
}

pub fn raw_lines(vcf: &str) -> Vec<RawLine> {
    unfold(vcf)
        .into_iter()
        .filter_map(|l| {
            let h = head_len(&l);
            let (head, value) = (&l[..h], l.get(h + 1..).unwrap_or("").to_string());
            let mut it = split_unquoted(head, ';').into_iter();
            let name = it.next()?;
            let name = name.rsplit('.').next().unwrap_or(&name).to_ascii_uppercase();
            let mut params = vec![];
            for p in it {
                let (k, v) = p.split_once('=').map(|(k, v)| (k.to_ascii_uppercase(), v.to_string())).unwrap_or(("TYPE".into(), p.clone()));
                // a quoted list is a list too (RFC 6350 §5.6: TYPE="voice,home")
                for v in v.replace('"', "").split(',') {
                    params.push((k.clone(), v.to_string()));
                }
            }
            Some(RawLine { name, params, value })
        })
        .collect()
}

impl RawLine {
    fn param(&self, k: &str) -> Option<&str> {
        self.params.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }
    fn types(&self) -> impl Iterator<Item = String> + '_ {
        self.params.iter().filter(|(n, _)| n == "TYPE").map(|(_, v)| v.to_lowercase())
    }
    /// first TYPE that is a real kind (home/work/cell/…), not a flag
    fn kind(&self) -> String {
        self.types().find(|t| !t.is_empty() && !matches!(t.as_str(), "pref" | "voice" | "internet" | "text" | "x-mobile")).unwrap_or_default()
    }
    fn pref(&self) -> bool {
        self.types().any(|t| t == "pref") || self.param("PREF").is_some_and(|p| p.trim() == "1")
    }
}

/// Property names build_vcf writes itself; every other line of an existing vCard (PHOTO, X-…,
/// client-specific items) is carried over untouched so an edit here never drops what another
/// client stored.
const MANAGED: &[&str] = &[
    "BEGIN", "END", "VERSION", "PRODID", "UID", "REV", "FN", "N", "NICKNAME", "ORG", "TITLE", "EMAIL", "TEL", "ADR", "URL", "IMPP", "RELATED", "X-ABDATE", "BDAY", "ANNIVERSARY", "CATEGORIES", "NOTE",
];

/// Unfolded raw lines of `vcf` whose property is not managed by the editor.
pub fn preserved_lines(vcf: &str) -> Vec<String> {
    unfold(vcf)
        .into_iter()
        .filter(|l| {
            let head = l.split([';', ':']).next().unwrap_or("");
            let name = head.rsplit('.').next().unwrap_or(head).to_ascii_uppercase();
            !MANAGED.contains(&name.as_str())
        })
        .collect()
}

/// Parses one vCard; vCard 2.1 bare parameters are read as TYPE (`bare_params_as_type`).
pub fn parse_vcf(vcf: &str) -> AppResult<VcardContact> {
    Ok(VcardParser::from_slice(bare_params_as_type(vcf).as_bytes()).expect_one()?)
}

/// vCard 2.1 parameters without a name (`TEL;CELL;VOICE:…`, `EMAIL;INTERNET:…`; RFC 2426 §5: "The "TYPE="
/// prefix to parameter values is required. In [VCARD] this was optional.") written as `TYPE=` parameters. caldata's
/// content-line parser takes every parameter as `name=value` and rejects the whole card otherwise (caldata
/// 0.17.3 src/parser/content_line/mod.rs:62 `split_once('=')` → `MissingDelimiter`; `TEL;CELL;TYPE=HOME`
/// even parses, as one parameter named `CELL;TYPE`) — and RustiCal's store then hides the card. A card without such a parameter comes back unchanged (borrowed); one with it unfolded.
/// For reading and importing only: a stored card stays as it is until the user saves it here. Reported
/// upstream: https://github.com/lennart-k/caldata-rs/issues/12 — drop this once caldata reads such cards.
pub fn bare_params_as_type(vcf: &str) -> std::borrow::Cow<'_, str> {
    let bare = |l: &str| split_unquoted(&l[..head_len(l)], ';').iter().skip(1).any(|p| !p.contains('='));
    let lines = unfold(vcf);
    if !lines.iter().any(|l| bare(l)) {
        return std::borrow::Cow::Borrowed(vcf);
    }
    let mut out = String::with_capacity(vcf.len() + 32);
    for l in lines {
        let (head, value) = l.split_at(head_len(&l));
        let mut parts = split_unquoted(head, ';').into_iter();
        out.push_str(&parts.next().unwrap_or_default());
        for p in parts.filter(|p| !p.is_empty()) {
            out.push(';');
            if !p.contains('=') {
                out.push_str("TYPE=");
            }
            out.push_str(&p);
        }
        out.push_str(value);
        out.push_str("\r\n");
    }
    std::borrow::Cow::Owned(out)
}

/// RustiCal's ETag of a stored text (rustical_ical address_object.rs `get_etag`: the quoted hex SHA-256 of
/// the vCard text) — for a card the store hands out no `AddressObject` for (`stored_card`).
fn etag_of(vcf: &str) -> String {
    use sha2::Digest;
    let h = sha2::Sha256::digest(vcf.as_bytes());
    format!("\"{}\"", h.iter().map(|b| format!("{b:02x}")).collect::<String>())
}

/// The stored text and ETag of one card. A card caldata rejects (`bare_params_as_type`) has no
/// `AddressObject` — the store answers its parse error — and is read from the table as RustiCal stored it.
pub async fn stored_card(state: &AppState, principal: &str, ab_id: &str, object_id: &str) -> AppResult<(String, String)> {
    match AddressbookReadStore::get_object(&*state.addressbooks, principal, ab_id, object_id, false).await {
        Ok(obj) => Ok((obj.get_vcf().to_string(), obj.get_etag())),
        Err(rustical_store::Error::IcalError(_)) => {
            let row = sqlx::query("SELECT vcf FROM addressobjects WHERE principal = ? AND addressbook_id = ? AND id = ? AND deleted_at IS NULL").bind(principal).bind(ab_id).bind(object_id).fetch_optional(&state.db).await?;
            let vcf: String = sqlx::Row::get(&row.ok_or(AppError::NotFound)?, "vcf");
            let etag = etag_of(&vcf);
            Ok((vcf, etag))
        }
        Err(e) => Err(e.into()),
    }
}

/// The cards of an address book the store left out of `get_objects` (with skip_broken, every card caldata
/// rejects): (id, stored text) of each live object whose id is not in `have`. One count query when none is
/// missing.
async fn unparsed_cards(state: &AppState, principal: &str, ab_id: &str, have: &(dyn Fn(&str) -> bool + Sync), n_have: usize) -> AppResult<Vec<(String, String)>> {
    let n: i64 = sqlx::Row::get(&sqlx::query("SELECT count(*) AS n FROM addressobjects WHERE principal = ? AND addressbook_id = ? AND deleted_at IS NULL").bind(principal).bind(ab_id).fetch_one(&state.db).await?, "n");
    if usize::try_from(n).unwrap_or(0) <= n_have {
        return Ok(vec![]);
    }
    let rows = sqlx::query("SELECT id, vcf FROM addressobjects WHERE principal = ? AND addressbook_id = ? AND deleted_at IS NULL").bind(principal).bind(ab_id).fetch_all(&state.db).await?;
    Ok(rows.iter().map(|r| (sqlx::Row::get::<String, _>(r, "id"), sqlx::Row::get::<String, _>(r, "vcf"))).filter(|(id, _)| !have(id)).collect())
}

/// Every card of an address book as (id, stored text): the store's, plus those it hides (`unparsed_cards`).
pub async fn ab_cards(state: &AppState, principal: &str, ab_id: &str) -> AppResult<Vec<(String, String)>> {
    let mut out: Vec<(String, String)> = AddressbookReadStore::get_objects(&*state.addressbooks, principal, ab_id).await?.into_iter().map(|(id, o)| (id, o.get_vcf().to_string())).collect();
    let have: std::collections::HashSet<String> = out.iter().map(|(id, _)| id.clone()).collect();
    out.extend(unparsed_cards(state, principal, ab_id, &|id| have.contains(id), have.len()).await?);
    Ok(out)
}

pub fn contact_from(vc: &VcardContact, raw: &[RawLine], ab: &AbInfo, object_id: &str, etag: String) -> Contact {
    let n: Vec<String> = vc.name.as_ref().map(|n| n.0.split(';').map(ical::unescape).collect()).unwrap_or_default();
    let full_name = vc.full_name.first().map(|f| ical::unescape(&f.0)).unwrap_or_default();
    let family = n.first().cloned().unwrap_or_default();
    let given = n.get(1).cloned().unwrap_or_default();
    let full_name = if full_name.is_empty() { format!("{given} {family}").trim().to_string() } else { full_name };
    let part = |i: usize| n.get(i).cloned().unwrap_or_default();
    fn lines<'a>(raw: &'a [RawLine], name: &'a str) -> impl Iterator<Item = &'a RawLine> + 'a {
        raw.iter().filter(move |l| l.name == name)
    }
    let typed = |name: &str| -> Vec<Typed> {
        lines(raw, name).map(|l| Typed { kind: l.kind(), value: ical::unescape(&l.value), pref: l.pref() }).filter(|t| !t.value.is_empty()).collect()
    };
    let addresses: Vec<Address> = lines(raw, "ADR")
        .map(|l| {
            let parts: Vec<String> = l.value.split(';').map(ical::unescape).collect();
            let g = |i: usize| parts.get(i).cloned().unwrap_or_default();
            // ADR: pobox;ext;street;locality;region;postal;country — pobox/ext fold into the street
            let street = [g(0), g(1), g(2)].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(", ");
            Address { kind: l.kind(), street, city: g(3), region: g(4), code: g(5), country: g(6), pref: l.pref() }
        })
        .filter(|a| !a.one_line().is_empty())
        .collect();
    let org_parts: Vec<String> = ical::text(vc, "ORG").map(|o| o.split(';').map(str::to_string).collect()).unwrap_or_default();
    let impp: Vec<Typed> = lines(raw, "IMPP")
        .map(|l| Typed { kind: l.param("X-SERVICE-TYPE").map(str::to_string).unwrap_or_else(|| l.kind()), value: ical::unescape(&l.value), pref: false })
        .filter(|t| !t.value.is_empty())
        .collect();
    let dates: Vec<Dated> = lines(raw, "X-ABDATE")
        .map(|l| Dated { label: l.param("X-ABLABEL").map(str::to_string).unwrap_or_default(), date: norm_partial(&l.value) })
        .filter(|d| !d.date.is_empty())
        .collect();
    Contact {
        principal: ab.principal.clone(),
        ab_id: ab.id.clone(),
        ab_name: ab.displayname.clone(),
        ab_color: ab.color.clone(),
        is_group: ab.is_group,
        object_id: object_id.to_string(),
        uid: vc.get_uid().unwrap_or(object_id).to_string(),
        full_name,
        given,
        family,
        nickname: ical::text(vc, "NICKNAME").unwrap_or_default(),
        prefix: part(3),
        additional: part(2),
        suffix: part(4),
        org: org_parts.first().cloned().unwrap_or_default(),
        department: org_parts.get(1).cloned().unwrap_or_default(),
        title: ical::text(vc, "TITLE").unwrap_or_default(),
        emails: typed("EMAIL"),
        phones: typed("TEL"),
        addresses,
        urls: typed("URL"),
        impp,
        related: typed("RELATED"),
        dates,
        birthday: vc.get_property("BDAY").map(|l| norm_partial(&l.value)).unwrap_or_default(),
        anniversary: vc.get_property("ANNIVERSARY").map(|l| norm_partial(&l.value)).unwrap_or_default(),
        categories: ical::list(vc, "CATEGORIES"),
        note: ical::text(vc, "NOTE").unwrap_or_default(),
        etag,
    }
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
            (c.ab_name, c.ab_color, c.is_group) = (ab.displayname.clone(), ab.color.clone(), ab.is_group);
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
pub struct ContactCache(std::sync::Mutex<std::collections::HashMap<(String, String), std::sync::Arc<AbContacts>>>);

async fn ab_contacts(state: &AppState, ab: &AbInfo) -> AppResult<std::sync::Arc<AbContacts>> {
    let k = (ab.principal.clone(), ab.id.clone());
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
        Some(c) => match AddressbookReadStore::sync_changes(&*state.addressbooks, &ab.principal, &ab.id, c.token).await {
            Ok(ch) => Some(ch),
            Err(rustical_store::Error::IcalError(_)) => None,
            Err(e) => return Err(e.into()),
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
            let objs = AddressbookReadStore::get_objects(&*state.addressbooks, &ab.principal, &ab.id).await?;
            let mut contacts: std::collections::BTreeMap<String, Contact> = objs.into_iter().filter_map(|(id, obj)| cached_contact(ab, &id, obj.get_vcf(), obj.get_etag()).map(|c| (id, c))).collect();
            let n_store = contacts.len();
            for (id, vcf) in unparsed_cards(state, &ab.principal, &ab.id, &|id| contacts.contains_key(id), n_store).await? {
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
    collections::ensure_owner(user, principal)?;
    let ab_row = state.addressbooks.get_addressbook(principal, ab_id, false).await?;
    let ab = AbInfo { color: String::new(), principal: principal.into(), owner_label: String::new(), is_group: principal.starts_with("group."), id: ab_id.into(), displayname: ab_row.displayname.unwrap_or_default(), description: String::new(), deleted_at: None, synctoken: 0 };
    let (vcf, etag) = stored_card(state, principal, ab_id, object_id).await?;
    Ok(contact_from(&parse_vcf(&vcf)?, &raw_lines(&vcf), &ab, object_id, etag))
}

/// The stored vCard text of one contact (download button).
pub async fn get_vcf(state: &AppState, user: &Principal, principal: &str, ab_id: &str, object_id: &str) -> AppResult<String> {
    collections::ensure_owner(user, principal)?;
    Ok(stored_card(state, principal, ab_id, object_id).await?.0)
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

fn type_param(kind: &str) -> String {
    let k = kind.trim().to_uppercase();
    if k.is_empty() || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') { String::new() } else { format!(";TYPE={k}") }
}

/// `;TYPE=KIND[,PREF]` — vCard 3.0 spelling of the preferred flag (Apple and DAVx⁵ read it).
fn type_pref(kind: &str, pref: bool) -> String {
    let t = type_param(kind);
    match (t.is_empty(), pref) {
        (true, true) => ";TYPE=PREF".into(),
        (false, true) => format!("{t},PREF"),
        _ => t,
    }
}

/// A single-word parameter value; anything else is dropped rather than written unquoted.
fn word(v: &str) -> Option<&str> {
    let v = v.trim();
    (!v.is_empty() && v.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')).then_some(v)
}

fn partial_to_vcard(v: &str) -> Option<String> {
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
    collections::ensure_owner(user, &i.principal)?;
    let keep = match keep_uid {
        Some(u) if matches!(stored_card(state, &i.principal, &i.ab_id, u).await, Err(AppError::NotFound | AppError::Store(rustical_store::Error::NotFound))) => Some(u.to_string()),
        _ => None,
    };
    let uid = keep.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let obj = AddressObject::from_vcf(build_vcf(&uid, &i, &[])?)?;
    state.addressbooks.put_object(&i.principal, &i.ab_id, &uid, obj, keep.is_some()).await?;
    Ok(uid)
}

pub async fn update_contact(state: &AppState, user: &Principal, object_id: &str, i: ContactInput, expected_etag: Option<&str>) -> AppResult<()> {
    collections::ensure_owner(user, &i.principal)?;
    let (existing, etag) = stored_card(state, &i.principal, &i.ab_id, object_id).await?;
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
    state.addressbooks.put_object(&i.principal, &i.ab_id, object_id, obj, true).await?;
    Ok(())
}

pub async fn delete_contact(state: &AppState, user: &Principal, principal: &str, ab_id: &str, object_id: &str) -> AppResult<()> {
    collections::ensure_owner(user, principal)?;
    AddressbookWriteStore::delete_object(&*state.addressbooks, principal, ab_id, object_id, true).await?;
    Ok(())
}

#[cfg(test)]
mod pinned;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn person_matches_address_book() {
        let c = |id: &str, name: &str, mail: &str| Contact { principal: "p".into(), ab_id: "a".into(), object_id: id.into(), full_name: name.into(), emails: if mail.is_empty() { vec![] } else { vec![Typed { kind: "home".into(), value: mail.into(), pref: false }] }, ..Default::default() };
        let all = vec![c("1", "Erika Musterfrau", "erika@example.org"), c("2", "Max Mustermann", "max.mustermann@firma.de"), c("3", "Maximilian Mustermann", "mm@example.org"), c("4", "Max Müller", ""), c("5", "Thomas Steinbach", "Thomas@Example.org")];
        let keys = |v: Vec<ContactHit>| v.into_iter().map(|h| h.key).collect::<Vec<_>>();
        // e-mail wins, case-insensitively, even when the name differs
        assert_eq!(keys(matches(&all, "T. Steinbach", "thomas@example.org", "")), vec!["p/a/5"]);
        assert_eq!(matches(&all, "Erika Musterfrau", "erika@example.org", "INDIVIDUAL")[0].sub, "erika@example.org");
        // no e-mail hit: similar names, several candidates
        assert_eq!(keys(matches(&all, "Max Mustermann", "max@example.org", "")), vec!["p/a/2", "p/a/3"]);
        assert_eq!(keys(matches(&all, "Erika Musterfrau", "", "")), vec!["p/a/1"]);
        // a lone first name, an unknown person, a room: nothing
        assert!(matches(&all, "Max", "", "").is_empty());
        assert!(matches(&all, "Lena Beispiel", "lena@example.org", "").is_empty());
        assert!(matches(&all, "Max Mustermann", "max.mustermann@firma.de", "ROOM").is_empty());
    }

    #[test]
    fn yearly_dates() {
        use crate::testkit::date;
        assert_eq!((partial_parts("1984-02-29"), partial_parts("--02-29"), partial_parts("irgendwann")), (Some((Some(1984), 2, 29)), Some((None, 2, 29)), None));
        // a day the month lacks is its last day: 29 February in a common year is the 28th
        assert_eq!((yearly_in(2027, 2, 29), yearly_in(2028, 2, 29), yearly_in(2026, 4, 31)), (Some(date("2027-02-28")), Some(date("2028-02-29")), Some(date("2026-04-30"))));
        assert_eq!((yearly_in(2026, 13, 1), yearly_in(2026, 2, 0)), (None, None));
        // today counts; past it, the next year's day (a leap year's own 29th)
        assert_eq!(next_yearly(2, 29, date("2027-02-28")), Some(date("2027-02-28")));
        assert_eq!(next_yearly(2, 29, date("2027-03-01")), Some(date("2028-02-29")));
        assert_eq!(next_yearly(2, 29, date("2026-10-14")), Some(date("2027-02-28")));
        assert_eq!(next_yearly(12, 31, date("2026-12-31")), Some(date("2026-12-31")));
    }

    #[test]
    fn vcard_roundtrip() {
        let i = ContactInput {
            principal: "p".into(),
            ab_id: "a".into(),
            given: "Christin".into(),
            family: "Lang".into(),
            nickname: "Chrissi".into(),
            department: "Vertrieb".into(),
            org: "ACME".into(),
            emails: vec![Typed { kind: "home".into(), value: "christin@example.org".into(), pref: true }],
            phones: vec![Typed { kind: "cell".into(), value: "+49 160 2223344".into(), pref: false }],
            addresses: vec![Address { kind: "home".into(), street: "Parkstraße 12".into(), code: "10115".into(), city: "Berlin".into(), country: "Deutschland".into(), ..Default::default() }],
            urls: vec![Typed { kind: "mastodon".into(), value: "https://example.social/@c".into(), pref: false }],
            impp: vec![Typed { kind: "Signal".into(), value: "+49 160 2223344".into(), pref: false }],
            related: vec![Typed { kind: "spouse".into(), value: "Max Lang".into(), pref: false }],
            dates: vec![Dated { label: "Namenstag".into(), date: "--07-24".into() }],
            birthday: "1984-03-12".into(),
            categories: vec!["Familie".into()],
            note: "Allergie: Nüsse".into(),
            ..Default::default()
        };
        let vcf = build_vcf("u1", &i, &["PHOTO;ENCODING=b;TYPE=JPEG:/9j/4AAQ".to_string(), "X-FOO:bar".to_string()]).unwrap();
        assert!(vcf.contains("X-FOO:bar"), "preserved lines are written back");
        let vc = parse_vcf(&vcf).unwrap();
        let ab = AbInfo { color: String::new(), principal: "p".into(), owner_label: String::new(), is_group: false, id: "a".into(), displayname: "Privat".into(), description: String::new(), deleted_at: None, synctoken: 0 };
        let c = contact_from(&vc, &raw_lines(&vcf), &ab, "u1", String::new());
        assert_eq!(c.full_name, "Christin Lang");
        assert_eq!(c.family, "Lang");
        assert_eq!(c.primary_email(), "christin@example.org");
        assert_eq!(c.phones[0].kind, "cell");
        assert_eq!(c.primary_address(), "Parkstraße 12, 10115 Berlin, Deutschland");
        assert_eq!(c.addresses[0].code, "10115");
        assert!(c.emails[0].pref && !c.phones[0].pref);
        assert_eq!((c.nickname.as_str(), c.org.as_str(), c.department.as_str()), ("Chrissi", "ACME", "Vertrieb"));
        assert_eq!(c.urls[0].kind, "mastodon");
        assert_eq!(c.impp[0].kind, "Signal");
        assert_eq!((c.related[0].kind.as_str(), c.related[0].value.as_str()), ("spouse", "Max Lang"));
        assert_eq!((c.dates[0].label.as_str(), c.dates[0].date.as_str()), ("Namenstag", "--07-24"));
        assert_eq!(preserved_lines(&vcf), vec!["PHOTO;ENCODING=b;TYPE=JPEG:/9j/4AAQ".to_string(), "X-FOO:bar".to_string()]);
        assert_eq!(c.birthday, "1984-03-12");
        assert_eq!(c.birthday_label(), "12.03.1984");
        assert_eq!(c.categories, vec!["Familie"]);
        assert_eq!((c.given_letter().as_str(), c.given_key().as_str()), ("C", "christin lang"));
        assert_eq!(norm_partial("--0312"), "--03-12");
        assert_eq!(fmt_partial("--03-12"), "12.03.");
    }
}
