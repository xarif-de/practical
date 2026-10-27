//! Reading vCards: unfolded raw lines with every parameter, the lines a save preserves verbatim, parsing
//! (incl. other clients' dialects) into `Contact`.

use super::*;

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
    pub(super) fn types(&self) -> impl Iterator<Item = String> + '_ {
        self.params.iter().filter(|(n, _)| n == "TYPE").map(|(_, v)| v.to_lowercase())
    }
    /// first TYPE that is a real kind (home/work/cell/…), not a flag
    pub(super) fn kind(&self) -> String {
        self.types().find(|t| !t.is_empty() && !matches!(t.as_str(), "pref" | "voice" | "internet" | "text" | "x-mobile")).unwrap_or_default()
    }
    pub(super) fn pref(&self) -> bool {
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
/// prefix to parameter values is required. In \[VCARD\] this was optional.") written as `TYPE=` parameters. caldata's
/// content-line parser takes every parameter as `name=value` and rejects the whole card otherwise (caldata
/// 0.17.3 src/parser/content_line/mod.rs:62 `split_once('=')` → `MissingDelimiter`; `TEL;CELL;TYPE=HOME`
/// even parses, as one parameter named `CELL;TYPE`) — and RustiCal's store then hides the card. A card without such a parameter comes back unchanged (borrowed); one with it unfolded.
/// For reading and importing only: a stored card stays as it is until the user saves it here. Reported
/// upstream: <https://github.com/lennart-k/caldata-rs/issues/12> — drop this once caldata reads such cards.
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
pub(super) fn etag_of(vcf: &str) -> String {
    use sha2::Digest;
    let h = sha2::Sha256::digest(vcf.as_bytes());
    format!("\"{}\"", h.iter().map(|b| format!("{b:02x}")).collect::<String>())
}

/// The stored text and ETag of one card. A card caldata rejects (`bare_params_as_type`) has no
/// `AddressObject` — the store answers its parse error — and is read from the table as RustiCal stored it.
pub async fn stored_card(state: &AppState, ab: &Authorized, object_id: &str) -> AppResult<(String, String)> {
    match state.collections().address_object(ab, object_id).await {
        Ok(obj) => Ok((obj.get_vcf().to_string(), obj.get_etag())),
        Err(AppError::Store(rustical_store::Error::IcalError(_))) => {
            // the address book itself (`(owner, id)`), never the path of a share
            let row = sqlx::query("SELECT vcf FROM addressobjects WHERE principal = ? AND addressbook_id = ? AND id = ? AND deleted_at IS NULL").bind(ab.owner()).bind(ab.id()).bind(object_id).fetch_optional(&state.db).await?;
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
pub(super) async fn unparsed_cards(state: &AppState, ab: &Authorized, have: &(dyn Fn(&str) -> bool + Sync), n_have: usize) -> AppResult<Vec<(String, String)>> {
    let (principal, ab_id) = (ab.owner(), ab.id());
    let n: i64 = sqlx::Row::get(&sqlx::query("SELECT count(*) AS n FROM addressobjects WHERE principal = ? AND addressbook_id = ? AND deleted_at IS NULL").bind(principal).bind(ab_id).fetch_one(&state.db).await?, "n");
    if usize::try_from(n).unwrap_or(0) <= n_have {
        return Ok(vec![]);
    }
    let rows = sqlx::query("SELECT id, vcf FROM addressobjects WHERE principal = ? AND addressbook_id = ? AND deleted_at IS NULL").bind(principal).bind(ab_id).fetch_all(&state.db).await?;
    Ok(rows.iter().map(|r| (sqlx::Row::get::<String, _>(r, "id"), sqlx::Row::get::<String, _>(r, "vcf"))).filter(|(id, _)| !have(id)).collect())
}

/// Every card of an address book as (id, stored text): the store's, plus those it hides (`unparsed_cards`).
pub async fn ab_cards(state: &AppState, ab: &Authorized) -> AppResult<Vec<(String, String)>> {
    let mut out: Vec<(String, String)> = state.collections().address_objects(ab).await?.into_iter().map(|(id, o)| (id, o.get_vcf().to_string())).collect();
    let have: std::collections::HashSet<String> = out.iter().map(|(id, _)| id.clone()).collect();
    out.extend(unparsed_cards(state, ab, &|id| have.contains(id), have.len()).await?);
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
