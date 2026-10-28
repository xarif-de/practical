//! Modelled properties of the master VEVENT: links and attachments (URL/ATTACH/CONFERENCE, embedded files),
//! colour, ORGANIZER/ATTENDEE, EXDATE/RDATE, RRULE UNTIL.

use super::*;

/// URL / ATTACH / CONFERENCE as one list. FMTTYPE, FILENAME, SIZE and FEATURE are fields, every
/// other parameter (X-NC-…, X-…) rides along in `extra`. An embedded (BASE64) ATTACH is read as
/// `embedded` with its `bin_index` (position among the binary lines of the master); the client
/// sends it back with `keep=true` to retain it, and a new file as `data` (base64, input only).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Link {
    /// url | attach | conference
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub uri: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub embedded: bool,
    #[serde(default)]
    pub filename: String,
    /// FMTTYPE (media type)
    #[serde(default)]
    pub fmt: String,
    /// bytes: SIZE parameter, or the decoded length of an embedded file
    #[serde(default)]
    pub size: Option<u64>,
    /// CONFERENCE FEATURE values (AUDIO, VIDEO, CHAT, SCREEN, PHONE, FEED, MODERATOR)
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub bin_index: Option<usize>,
    #[serde(default)]
    pub keep: bool,
    #[serde(default, skip_serializing)]
    pub data: Option<String>,
    #[serde(default)]
    pub extra: Vec<(String, String)>,
}

/// 2 MiB per embedded file, 6 MiB of new files per save (the JSON body carries them as base64).
pub const EMBED_FILE_MAX: usize = 2 * 1024 * 1024;

pub const EMBED_TOTAL_MAX: usize = 6 * 1024 * 1024;

/// Decoded byte length of a base64 text (ignoring whitespace and `=` padding, which carries no data),
/// without decoding it: every 4 characters are 3 bytes, a tail of 2 or 3 characters 1 or 2 bytes (a
/// lone tail character holds no whole byte).
pub(super) fn base64_len(b64: &str) -> usize {
    let n = b64.bytes().filter(|b| !b.is_ascii_whitespace() && *b != b'=').count();
    n / 4 * 3 + [0, 0, 1, 2][n % 4]
}

pub(crate) fn is_binary_attach(params: &[(String, String)]) -> bool {
    params.iter().any(|(k, v)| (k == "VALUE" && v.eq_ignore_ascii_case("BINARY")) || k == "ENCODING")
}

pub(crate) fn links_of(props: &[String]) -> Vec<Link> {
    let mut bin = 0usize;
    props.iter().filter_map(|l| {
        let (name, params, value) = ical::split_line(l);
        let kind = match name.as_str() { "URL" => "url", "ATTACH" => "attach", "CONFERENCE" => "conference", _ => return None };
        let mut link = Link { kind: kind.into(), uri: value.trim().to_string(), ..Default::default() };
        let binary = kind == "attach" && is_binary_attach(&params);
        if binary {
            link.embedded = true;
            link.keep = true;
            link.uri = String::new();
            link.size = Some(base64_len(value.trim()) as u64);
            link.bin_index = Some(bin);
            bin += 1;
        }
        for (k, v) in params {
            match k.as_str() {
                "LABEL" => link.label = v.trim_matches('"').to_string(),
                "FMTTYPE" => link.fmt = v.trim_matches('"').to_string(),
                "FILENAME" | "X-FILENAME" => link.filename = v.trim_matches('"').trim_start_matches('/').to_string(),
                "SIZE" => link.size = v.trim().parse().ok().or(link.size),
                "FEATURE" => link.features = v.split(',').map(|f| f.trim().to_ascii_uppercase()).filter(|f| !f.is_empty()).collect(),
                "VALUE" | "ENCODING" => {}
                _ => link.extra.push((k, v)),
            }
        }
        Some(link)
    }).collect()
}

/// Media types an attachment download is served as; any other FMTTYPE (`text/html`, `text/javascript`,
/// `image/svg+xml`, … — written by any client or group member) becomes `application/octet-stream`: served
/// from the app's origin under its own type, a stored script would be a `script-src 'self'` source.
const SAFE_MEDIA: &[&str] = &["application/pdf", "image/png", "image/jpeg", "image/gif", "image/webp", "image/heic", "text/plain"];

/// The n-th embedded ATTACH of a stored master: (media type, file name, bytes).
pub fn embedded_attachment(props: &[String], n: usize) -> Option<(String, String, Vec<u8>)> {
    use base64ct::Encoding;
    let line = props.iter().filter(|l| prop_name(l) == "ATTACH" && is_binary_attach(&ical::split_line(l).1)).nth(n)?;
    let (_, params, value) = ical::split_line(line);
    let get = |k: &str| params.iter().find(|(pk, _)| pk == k).map(|(_, v)| v.trim_matches('"').to_string());
    let fmt = get("FMTTYPE").map(|f| f.trim().to_ascii_lowercase()).filter(|f| SAFE_MEDIA.contains(&f.as_str())).unwrap_or_else(|| "application/octet-stream".into());
    let name = get("FILENAME").or_else(|| get("X-FILENAME")).map(|n| n.trim_start_matches('/').to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| format!("anhang-{}", n + 1));
    let clean: String = value.chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = base64ct::Base64::decode_vec(&clean).ok()?;
    Some((fmt, name, bytes))
}

pub(crate) fn link_lines(links: &[Link]) -> Vec<String> {
    let mut out = vec![];
    let mut url_written = false;
    for l in links {
        let uri = l.uri.trim();
        if l.embedded || uri.is_empty() || uri.contains(['\n', '\r']) {
            continue;
        }
        let mut name = match l.kind.as_str() { "url" => "URL", "attach" => "ATTACH", "conference" => "CONFERENCE", _ => continue };
        if name == "URL" {
            if url_written {
                name = "ATTACH"; // URL must not occur more than once: further websites are kept as ATTACH links
            }
            url_written = true;
        }
        let mut line = name.to_string();
        if name == "CONFERENCE" {
            line.push_str(";VALUE=URI");
        }
        line.push_str(&link_params(l, name == "CONFERENCE"));
        let label = l.label.trim().replace('"', "");
        if !label.is_empty() {
            line.push_str(&format!(";LABEL=\"{label}\"")); // LABEL is registered for CONFERENCE (RFC 7986); on URL/ATTACH it is an other-param every parser ignores
        }
        out.push(format!("{line}:{uri}"));
    }
    out
}

/// FMTTYPE / FILENAME / SIZE / FEATURE / extra as parameter text (leading `;`).
fn link_params(l: &Link, conference: bool) -> String {
    let mut p = String::new();
    let clean = |v: &str| v.trim().replace(['"', ';', ':', '\n', '\r'], "");
    if !l.fmt.trim().is_empty() {
        p.push_str(&format!(";FMTTYPE={}", clean(&l.fmt)));
    }
    if !l.filename.trim().is_empty() {
        let n = clean(&l.filename);
        p.push_str(&if n.contains([',', ' ']) { format!(";FILENAME=\"{n}\"") } else { format!(";FILENAME={n}") });
    }
    if let Some(sz) = l.size.filter(|_| !l.embedded) {
        p.push_str(&format!(";SIZE={sz}"));
    }
    if conference && !l.features.is_empty() {
        p.push_str(&format!(";FEATURE={}", l.features.iter().map(|f| clean(f).to_ascii_uppercase()).collect::<Vec<_>>().join(",")));
    }
    for (k, v) in &l.extra {
        p.push_str(&ical::param(k, v));
    }
    p
}

/// New embedded files as `ATTACH;…;ENCODING=BASE64;VALUE=BINARY:` lines, size-checked.
pub(crate) fn embedded_lines(links: &[Link]) -> AppResult<Vec<String>> {
    let mut out = vec![];
    let mut total = 0usize;
    for l in links.iter().filter(|l| l.embedded) {
        let Some(data) = l.data.as_deref() else { continue };
        let clean: String = data.chars().filter(|c| !c.is_whitespace()).collect();
        if clean.is_empty() || !clean.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'=') {
            return Err(AppError::bad("err-attach-base64"));
        }
        let n = base64_len(&clean);
        total += n;
        if n > EMBED_FILE_MAX || total > EMBED_TOTAL_MAX {
            return Err(AppError::BadRequest(crate::error::Msg { key: "err-attach-too-large", args: vec![("size", n.to_string()), ("file", EMBED_FILE_MAX.to_string()), ("total", EMBED_TOTAL_MAX.to_string())] }));
        }
        let mut line = "ATTACH".to_string();
        line.push_str(&link_params(l, false));
        line.push_str(";ENCODING=BASE64;VALUE=BINARY:");
        line.push_str(&clean);
        out.push(line);
    }
    Ok(out)
}

pub fn valid_color(c: &str) -> bool {
    !c.is_empty() && c.len() <= 32 && c.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '#')
}

pub(crate) fn one_of<'a>(v: &str, allowed: &[&'a str]) -> Option<&'a str> {
    let u = v.trim().to_ascii_uppercase();
    allowed.iter().copied().find(|a| *a == u)
}

/// The CLASS values the editor models
pub const CLASSES: [&str; 3] = ["PUBLIC", "PRIVATE", "CONFIDENTIAL"];

/// A CLASS value as written (`None`: "" from the editor, no CLASS line): a modelled one upper-case, any other
/// iana-token / x-name (RFC 5545 §3.8.1.3: `1*(ALPHA / DIGIT / "-")`, e.g. `X-SECRET` of another client) verbatim,
/// anything else (`X_SECRET`, `x secret`) PRIVATE. RustiCal shows every value but PUBLIC to sharees as PRIVATE
/// (rustical_ical visibility.rs `class_level`): an unknown class must survive a save, and a malformed one must not
/// turn the object PUBLIC (tests `it::sharing::an_unknown_class_survives_every_save`, `events::pinned::class_values`).
pub fn class_value(v: &str) -> Option<String> {
    let v = v.trim();
    if v.is_empty() {
        return None;
    }
    if let Some(k) = known_class(v) {
        return Some(k.to_string());
    }
    Some(if v.len() <= 64 && v.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') { v.to_string() } else { "PRIVATE".into() })
}

/// One of [`CLASSES`] by RustiCal's comparison: `trim().to_uppercase()`, Unicode (rustical_ical visibility.rs
/// `class_level`), so `confıdentıal` (dotless ı) is CONFIDENTIAL there and here alike
fn known_class(v: &str) -> Option<&'static str> {
    let u = v.trim().to_uppercase();
    CLASSES.into_iter().find(|k| *k == u)
}

/// The CLASS a stored object carries as the editor reads it: "" without a CLASS line, else [`class_value`] of its
/// value, an empty one PRIVATE (RustiCal's reading of it)
pub fn stored_class(v: Option<&str>) -> String {
    v.map(|v| class_value(v).unwrap_or_else(|| "PRIVATE".into())).unwrap_or_default()
}

/// The CLASS a list shows (task rows, the day view's mark): "" without one, PUBLIC and CONFIDENTIAL as they are,
/// every other value PRIVATE, as RustiCal treats it for sharees
pub fn shown_class(v: Option<&str>) -> String {
    match v.map(known_class) {
        None => String::new(),
        Some(Some(k)) if k != "PRIVATE" => k.into(),
        Some(_) => "PRIVATE".into(),
    }
}

/// How strict a present CLASS is for sharees: 0 PUBLIC, 2 CONFIDENTIAL (never shown above PRIVATE: `Policy::new`),
/// 1 everything else — PRIVATE, an unknown, a malformed or an empty value (RustiCal's `class_level`)
fn class_rank(v: &str) -> u8 {
    match known_class(v) {
        Some("PUBLIC") => 0,
        Some("CONFIDENTIAL") => 2,
        _ => 1,
    }
}

/// The strictest CLASS among RECURRENCE-ID override blocks (`split_component`): "" (none or all public), PRIVATE (any
/// value but PUBLIC and CONFIDENTIAL counts as that, an empty one too) or CONFIDENTIAL. RustiCal takes the strictest of all components (rustical_ical
/// visibility.rs `calendar_level`), so the editor's hint combines it with the master's own.
pub fn overrides_class(blocks: &[String]) -> String {
    let rank = blocks.iter().flat_map(|b| unfold(&b.replace("\r\n", "\n"))).filter(|l| prop_name(l) == "CLASS").map(|l| class_rank(&ical::split_line(&l).2)).max().unwrap_or(0);
    ["", "PRIVATE", "CONFIDENTIAL"][usize::from(rank)].to_string()
}

/// ORGANIZER / ATTENDEE: the modelled parameters plus every other parameter verbatim (`extra`),
/// so RSVP, DELEGATED-FROM, X-… survive a save.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Person {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub email: String,
    /// REQ-PARTICIPANT | OPT-PARTICIPANT | CHAIR | NON-PARTICIPANT
    #[serde(default)]
    pub role: String,
    /// NEEDS-ACTION | ACCEPTED | DECLINED | TENTATIVE | DELEGATED
    #[serde(default)]
    pub status: String,
    /// INDIVIDUAL | GROUP | RESOURCE | ROOM
    #[serde(default)]
    pub cutype: String,
    #[serde(default)]
    pub extra: Vec<(String, String)>,
    /// address-book entries that look like this person (`contacts::matches`); output only
    #[serde(default, skip_deserializing, skip_serializing_if = "Vec::is_empty")]
    pub contacts: Vec<crate::domain::contacts::ContactHit>,
}

pub(super) fn persons_of(props: &[String], name: &str) -> Vec<Person> {
    props.iter().filter(|l| prop_name(l) == name).map(|l| {
        let (_, params, value) = ical::split_line(l);
        let mut p = Person { email: ical::strip_mailto(value.trim()).to_string(), ..Default::default() };
        for (k, v) in params {
            match k.as_str() {
                "CN" => p.name = v.trim_matches('"').to_string(),
                "ROLE" => p.role = v.to_ascii_uppercase(),
                "PARTSTAT" => p.status = v.to_ascii_uppercase(),
                "CUTYPE" => p.cutype = v.to_ascii_uppercase(),
                _ => p.extra.push((k, v)),
            }
        }
        p
    }).collect()
}

pub(super) fn person_line(name: &str, p: &Person) -> Option<String> {
    let email = p.email.trim();
    if email.is_empty() {
        return None;
    }
    let mut l = name.to_string();
    if !p.name.trim().is_empty() {
        l.push_str(&ical::param("CN", p.name.trim()));
    }
    for (k, v) in [("ROLE", &p.role), ("PARTSTAT", &p.status), ("CUTYPE", &p.cutype)] {
        if !v.trim().is_empty() {
            l.push_str(&ical::param(k, &v.trim().to_ascii_uppercase()));
        }
    }
    for (k, v) in &p.extra {
        l.push_str(&ical::param(k, v));
    }
    Some(format!("{l}:mailto:{email}"))
}

/// EXDATE/RDATE values as local strings in the user's zone ("YYYY-MM-DD" for DATE values).
/// PERIOD-valued RDATEs are not modelled and get dropped on save.
pub(super) fn dates_of(props: &[String], name: &str, tz: chrono_tz::Tz) -> Vec<String> {
    let mut out = vec![];
    for l in props.iter().filter(|l| prop_name(l) == name) {
        let (_, params, value) = ical::split_line(l);
        let param = |k: &str| params.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        let is_date = param("VALUE").is_some_and(|v| v.eq_ignore_ascii_case("DATE"));
        let src_tz = param("TZID").and_then(|t| ical::parse_tz(&t));
        for v in value.split(',') {
            let v = v.trim();
            if is_date || v.len() == 8 {
                if let Ok(d) = NaiveDate::parse_from_str(v, "%Y%m%d") {
                    out.push(d.format("%Y-%m-%d").to_string());
                }
            } else if let Ok(dt) = NaiveDateTime::parse_from_str(v.trim_end_matches('Z'), "%Y%m%dT%H%M%S") {
                let local = if v.ends_with('Z') {
                    chrono::Utc.from_utc_datetime(&dt).with_timezone(&tz).naive_local()
                } else if let Some(stz) = src_tz {
                    stz.from_local_datetime(&dt).single().map(|d| d.with_timezone(&tz).naive_local()).unwrap_or(dt)
                } else {
                    dt
                };
                out.push(local.format("%Y-%m-%dT%H:%M:%S").to_string());
            }
        }
    }
    out
}

pub(super) fn date_lines(name: &str, values: &[String], all_day: bool, tz: chrono_tz::Tz) -> Vec<String> {
    let mut out = vec![];
    for v in values {
        let v = v.trim();
        if all_day || v.len() == 10 {
            if let Some(d) = v.get(..v.len().min(10)).and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()).filter(|d| ical::year_ok(*d)) {
                out.push(format!("{name};VALUE=DATE:{}", ical::fmt_date(d)));
            }
        } else if let Ok(dt) = NaiveDateTime::parse_from_str(v, "%Y-%m-%dT%H:%M:%S").or_else(|_| NaiveDateTime::parse_from_str(v, "%Y-%m-%dT%H:%M"))
            && ical::year_ok(dt.date())
        {
            out.push(format!("{name};TZID={}:{}", tz.name(), ical::fmt_dt(dt)));
        }
    }
    out
}

/// The RRULE's UNTIL as a local date, whatever form it was written in.
pub(super) fn rrule_until_local(rrule: &str, tz: chrono_tz::Tz) -> Option<NaiveDate> {
    let rrule = ical::rrule_canon(rrule);
    let v = rrule.split(';').find_map(|p| p.strip_prefix("UNTIL="))?;
    if let Ok(dt) = NaiveDateTime::parse_from_str(v.trim_end_matches('Z'), "%Y%m%dT%H%M%S") {
        return Some(if v.ends_with('Z') { chrono::Utc.from_utc_datetime(&dt).with_timezone(&tz).date_naive() } else { dt.date() });
    }
    NaiveDate::parse_from_str(v.get(..v.len().min(8))?, "%Y%m%d").ok()
}

/// A bare `UNTIL=YYYYMMDD` on a timed event becomes the end of that local day in UTC (RFC 5545
/// wants UNTIL in the DTSTART value type); on an all-day event a date-time UNTIL becomes a date.
pub(super) fn normalize_until(rrule: &str, all_day: bool, tz: chrono_tz::Tz) -> String {
    ical::rrule_canon(rrule).split(';').map(|p| {
        let Some(v) = p.strip_prefix("UNTIL=") else { return p.to_string() };
        let date = rrule_until_local(&format!("UNTIL={v}"), tz);
        match date {
            Some(d) if all_day => format!("UNTIL={}", ical::fmt_date(d)),
            Some(d) if v.len() <= 8 && ical::year_ok(d) => format!("UNTIL={}", ical::to_utc(d.and_hms_opt(23, 59, 59).unwrap(), tz).format("%Y%m%dT%H%M%SZ")),
            _ => p.to_string(),
        }
    }).collect::<Vec<_>>().join(";")
}
