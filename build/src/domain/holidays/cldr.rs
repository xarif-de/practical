//! Built-in region data (CLDR) and its upkeep: packed name tables, which source covers which country, the weekly
//! CLDR release check and fetch, region names and source descriptions.

use super::*;

/// CLDR's names as fetched (`fetch_cldr`) and saved (`practical_holidays` "cldr-data"; the
/// format of src/data/regions.json, which build.rs packs into names.rs tables).
#[derive(serde::Deserialize, serde::Serialize)]
pub(super) struct DataCountry {
    #[serde(default)]
    pub(super) oh: u8,
    #[serde(default)]
    pub(super) ng: u8,
    pub(super) n: HashMap<String, String>,
}
#[derive(serde::Deserialize, serde::Serialize)]
pub(super) struct DataSub {
    pub(super) p: String,
    pub(super) n: HashMap<String, String>,
}
#[derive(serde::Deserialize, serde::Serialize)]
pub(super) struct Data {
    /// CLDR release the names come from ("release-48-2")
    #[serde(default)]
    pub(super) cldr: String,
    pub(super) countries: HashMap<String, DataCountry>,
    pub(super) subs: HashMap<String, DataSub>,
}

/// the CLDR release of the built-in names (src/data/regions.json)
pub(super) const BUILT_IN_CLDR: &str = include_str!(concat!(env!("OUT_DIR"), "/regions-cldr.txt"));
/// build.rs output, gzip-9: 5.0 KB for 257 countries, 84 KB for 5,046 subdivisions (decoded
/// 11 / 262 KB); each table is decoded once, when first needed — the subdivisions only by the settings
/// page (names, `cldr_tree`), not by the calendar views.
fn unpack(gz: &[u8]) -> Table {
    use std::io::Read;
    let mut text = String::new();
    flate2::read::GzDecoder::new(gz).read_to_string(&mut text).expect("embedded region names");
    text.shrink_to_fit();
    Table::new(text)
}
pub(super) static BUILT_IN_COUNTRIES: LazyLock<Table> = LazyLock::new(|| unpack(include_bytes!(concat!(env!("OUT_DIR"), "/regions-countries.gz"))));
pub(super) static BUILT_IN_SUBS: LazyLock<Table> = LazyLock::new(|| unpack(include_bytes!(concat!(env!("OUT_DIR"), "/regions-subs.gz"))));

/// A newer CLDR release's names, packed like the built-in ones.
pub(super) struct Names {
    pub(super) cldr: String,
    pub(super) countries: Table,
    pub(super) subs: Table,
}

pub(super) fn pack(d: &Data) -> Names {
    let src = |code: &String, meta: String, n: &HashMap<String, String>| names::Src { code: code.clone(), meta, names: names::LANGS.map(|l| n.get(l).cloned()), any: n.values().next().cloned() };
    let countries = d.countries.iter().map(|(code, c)| src(code, (c.oh + 2 * c.ng).to_string(), &c.n)).collect();
    let subs = d.subs.iter().map(|(code, s)| src(code, if s.p == code[..2.min(code.len())] { String::new() } else { s.p.clone() }, &s.n)).collect();
    Names { cldr: d.cldr.clone(), countries: Table::new(names::encode(countries)), subs: Table::new(names::encode(subs)) }
}

/// the names in use: the built-in ones, replaced by a newer CLDR release once `check_cldr` got one
static NEWER: std::sync::RwLock<Option<std::sync::Arc<Names>>> = std::sync::RwLock::new(None);

fn cldr_in_use() -> String {
    NEWER.read().unwrap().as_ref().map_or_else(|| BUILT_IN_CLDR.to_string(), |n| n.cldr.clone())
}

/// `f` on the country (`subs` false) or subdivision table in use
pub(super) fn with_table<T>(subs: bool, f: impl FnOnce(&Table) -> T) -> T {
    let newer = NEWER.read().unwrap().clone();
    match (newer, subs) {
        (Some(n), false) => f(&n.countries),
        (Some(n), true) => f(&n.subs),
        (None, false) => f(&BUILT_IN_COUNTRIES),
        (None, true) => f(&BUILT_IN_SUBS),
    }
}

/// Which countries each source covers: the built-in snapshot until `refresh_coverage` knows better.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub(super) struct Coverage {
    pub(super) oh: std::collections::BTreeSet<String>,
    pub(super) ng: std::collections::BTreeSet<String>,
}

pub(super) static COVERAGE: LazyLock<std::sync::RwLock<Coverage>> = LazyLock::new(|| {
    // META of a country row: oh + 2·ng (names.rs)
    let pick = |bit: u8| BUILT_IN_COUNTRIES.rows().filter(|r| r.meta.parse::<u8>().unwrap_or(0) & bit != 0).map(|r| r.code.to_string()).collect();
    std::sync::RwLock::new(Coverage { oh: pick(1), ng: pick(2) })
});

pub(super) fn covered_by_openholidays(country: &str) -> bool {
    COVERAGE.read().unwrap().oh.contains(country)
}
pub(super) fn covered_by_nager(country: &str) -> bool {
    COVERAGE.read().unwrap().ng.contains(country)
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct OhCountry {
    pub(super) iso_code: String,
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct NagerCountry {
    country_code: String,
}

/// A fresh list replaces the known one unless it looks broken: empty, or less than half as
/// long (an outage answering `[]` must not wipe a source's countries).
pub(super) fn accept(known: &std::collections::BTreeSet<String>, fresh: Result<Vec<String>, String>, what: &str) -> std::collections::BTreeSet<String> {
    match fresh {
        Ok(list) if !list.is_empty() && list.len() * 2 >= known.len() => {
            let fresh: std::collections::BTreeSet<String> = list.into_iter().filter(|c| c.len() == 2 && c.chars().all(|x| x.is_ascii_uppercase())).collect();
            let added: Vec<_> = fresh.difference(known).collect();
            let gone: Vec<_> = known.difference(&fresh).collect();
            if !added.is_empty() || !gone.is_empty() {
                tracing::info!("holiday coverage {what}: added {added:?}, gone {gone:?}");
            }
            fresh
        }
        Ok(list) => {
            tracing::warn!("holiday coverage {what}: ignoring a list of {} (had {})", list.len(), known.len());
            known.clone()
        }
        Err(e) => {
            tracing::warn!("holiday coverage {what}: {e}");
            known.clone()
        }
    }
}

/// Refresh which countries OpenHolidays and Nager.Date cover; the answer is saved, so a
/// restart (even during an outage) starts from it.
pub(super) async fn refresh_coverage(state: &AppState) -> AppResult<()> {
    tracing::info!("holiday coverage: asking OpenHolidays and Nager.Date");
    let oh = get_json::<Vec<OhCountry>>(state, &format!("{OPENHOLIDAYS}/Countries")).await.map(|v| v.into_iter().map(|c| c.iso_code).collect());
    let ng = get_json::<Vec<NagerCountry>>(state, &format!("{NAGER}/AvailableCountries")).await.map(|v| v.into_iter().map(|c| c.country_code).collect());
    let known = COVERAGE.read().unwrap().clone();
    let next = Coverage { oh: accept(&known.oh, oh, "OpenHolidays"), ng: accept(&known.ng, ng, "Nager.Date") };
    if next != known {
        *COVERAGE.write().unwrap() = next.clone();
    }
    cache_put(state, "", "", 0, "coverage", &serde_json::to_string(&next).unwrap()).await
}

const COVERAGE_EVERY_H: i64 = 7 * 24;
const CLDR_CHECK_EVERY_H: i64 = 7 * 24;
pub(super) const CLDR: &str = "https://raw.githubusercontent.com/unicode-org/cldr";
const CLDR_LATEST: &str = "https://api.github.com/repos/unicode-org/cldr/releases/latest";

/// Upkeep of the region data, called daily by tasks.rs (and once at start). The first call only reads what was
/// saved; a request goes out when a check is due, a download only when there is something new.
pub async fn maintain(state: &AppState, first: bool) -> AppResult<()> {
    if first {
        if let Some((j, _)) = cache_aged(state, "", "", 0, "coverage").await?
            && let Ok(saved) = serde_json::from_str::<Coverage>(&j)
            && !saved.oh.is_empty()
        {
            *COVERAGE.write().unwrap() = saved;
        }
        if let Some((j, _)) = cache_aged(state, "", "", 0, "cldr-data").await?
            && let Ok(saved) = serde_json::from_str::<Data>(&j)
            && newer(&saved.cldr, BUILT_IN_CLDR)
        {
            tracing::info!("region names: {} (saved) over {} (built in)", saved.cldr, BUILT_IN_CLDR);
            *NEWER.write().unwrap() = Some(std::sync::Arc::new(pack(&saved)));
        }
    }
    if !state.config.holidays_enabled {
        return Ok(());
    }
    if cache_aged(state, "", "", 0, "coverage").await?.is_none_or(|(_, h)| h >= COVERAGE_EVERY_H) {
        refresh_coverage(state).await?;
    }
    if cache_aged(state, "", "", 0, "cldr-check").await?.is_none_or(|(_, h)| h >= CLDR_CHECK_EVERY_H) {
        check_cldr(state).await?;
    }
    Ok(())
}

/// "release-48-2" → [48, 2], so releases compare as numbers
pub(super) fn cldr_version(tag: &str) -> Vec<u32> {
    tag.split(|c: char| !c.is_ascii_digit()).filter(|p| !p.is_empty()).filter_map(|p| p.parse().ok()).collect()
}
pub(super) fn newer(tag: &str, than: &str) -> bool {
    !cldr_version(tag).is_empty() && cldr_version(tag) > cldr_version(than)
}

#[derive(serde::Deserialize, serde::Serialize, Default)]
pub(super) struct CldrCheck {
    etag: String,
    tag: String,
}

/// Ask GitHub for CLDR's latest release (with the last ETag, so an unchanged answer is a
/// bodiless 304); fetch and apply its names only when it is newer than the ones in use.
pub(super) async fn check_cldr(state: &AppState) -> AppResult<()> {
    let last: CldrCheck = cache_aged(state, "", "", 0, "cldr-check").await?.and_then(|(j, _)| serde_json::from_str(&j).ok()).unwrap_or_default();
    let mut req = state.http.get(CLDR_LATEST).header("Accept", "application/vnd.github+json").header("User-Agent", "PractiCal").timeout(TIMEOUT);
    if !last.etag.is_empty() {
        req = req.header("If-None-Match", &last.etag);
    }
    let check = match req.send().await {
        Ok(r) if r.status() == reqwest::StatusCode::NOT_MODIFIED => last,
        Ok(r) if r.status().is_success() => {
            let etag = r.headers().get("etag").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
            #[derive(serde::Deserialize)]
            struct Release {
                tag_name: String,
            }
            match r.json::<Release>().await {
                Ok(rel) => CldrCheck { etag, tag: rel.tag_name },
                Err(e) => {
                    tracing::warn!("cldr check: {e}");
                    return Ok(()); // no timestamp: tried again with the next round (a day, or the next start)
                }
            }
        }
        Ok(r) => {
            tracing::warn!("cldr check: HTTP {}", r.status());
            return Ok(());
        }
        Err(e) => {
            tracing::warn!("cldr check: {e}");
            return Ok(());
        }
    };
    let current = cldr_in_use();
    tracing::info!("cldr check: latest {}, in use {current}", check.tag);
    if newer(&check.tag, &current) {
        match fetch_cldr(state, &check.tag).await {
            Ok(d) => {
                tracing::info!("region names: CLDR {} replaces {current}", check.tag);
                cache_put(state, "", "", 0, "cldr-data", &serde_json::to_string(&d).unwrap()).await?;
                *NEWER.write().unwrap() = Some(std::sync::Arc::new(pack(&d)));
            }
            Err(e) => {
                tracing::warn!("cldr {}: {e}", check.tag);
                return Ok(()); // the check stays due, the download is retried with the next round
            }
        }
    }
    cache_put(state, "", "", 0, "cldr-check", &serde_json::to_string(&check).unwrap()).await
}

async fn get_text(state: &AppState, url: &str) -> Result<String, String> {
    let r = state.http.get(url).header("User-Agent", "PractiCal").timeout(Duration::from_secs(60)).send().await.map_err(|e| format!("{url}: {e}"))?;
    if !r.status().is_success() {
        return Err(format!("{url}: HTTP {}", r.status()));
    }
    r.text().await.map_err(|e| format!("{url}: {e}"))
}

/// CLDR's files for `tag`, read the way tools/gen_regions.py reads them.
async fn fetch_cldr(state: &AppState, tag: &str) -> Result<Data, String> {
    let base = format!("{CLDR}/{tag}/common");
    let mut countries: HashMap<String, DataCountry> = HashMap::new();
    let en = get_text(state, &format!("{base}/main/en.xml")).await?;
    for (code, _) in xml_items(&en, "territory") {
        if code.len() == 2 && code.chars().all(|c| c.is_ascii_uppercase()) && !["ZZ", "EU", "EZ", "UN", "QO", "XA", "XB"].contains(&code.as_str()) {
            countries.insert(code, DataCountry { oh: 0, ng: 0, n: HashMap::new() });
        }
    }
    if countries.len() < 200 {
        return Err(format!("only {} countries in main/en.xml", countries.len()));
    }
    let mut subs: HashMap<String, DataSub> = HashMap::new();
    let sup = get_text(state, &format!("{base}/supplemental/subdivisions.xml")).await?;
    for (parent, contains) in xml_subgroups(&sup) {
        let parent = if parent.len() == 2 { parent } else { iso(&parent) };
        if countries.contains_key(&parent[..2.min(parent.len())]) {
            for c in contains.split_whitespace() {
                subs.insert(iso(c), DataSub { p: parent.clone(), n: HashMap::new() });
            }
        }
    }
    for lang in crate::i18n::LANGS {
        let main = if *lang == "en" { en.clone() } else { get_text(state, &format!("{base}/main/{lang}.xml")).await? };
        for (code, name) in xml_items(&main, "territory") {
            if let Some(c) = countries.get_mut(&code) {
                c.n.insert(lang.to_string(), name);
            }
        }
        let sd = get_text(state, &format!("{base}/subdivisions/{lang}.xml")).await?;
        for (code, name) in xml_items(&sd, "subdivision") {
            if let Some(s) = subs.get_mut(&iso(&code)) {
                s.n.insert(lang.to_string(), name);
            }
        }
    }
    Ok(Data { cldr: tag.to_string(), countries, subs })
}

/// "desn" → "DE-SN", "fr69m" → "FR-69M"
pub(super) fn iso(cldr: &str) -> String {
    if cldr.len() <= 2 { cldr.to_uppercase() } else { format!("{}-{}", cldr[..2].to_uppercase(), cldr[2..].to_uppercase()) }
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let k = format!(" {name}=\"");
    let i = tag.find(&k)? + k.len();
    Some(tag[i..].split('"').next()?.to_string())
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// `<tag type="X">Text</tag>` entries without an `alt` variant, as (type, text). CLDR's files
/// are regular enough for this; a parser crate would be a dependency for one yearly download.
pub(super) fn xml_items(xml: &str, tag: &str) -> Vec<(String, String)> {
    let open = format!("<{tag} ");
    let close = format!("</{tag}>");
    let mut out = vec![];
    let mut rest = xml;
    while let Some(i) = rest.find(&open) {
        rest = &rest[i..];
        let Some(gt) = rest.find('>') else { break };
        let head = &rest[..gt];
        let body = &rest[gt + 1..];
        let Some(end) = body.find(&close) else { break };
        if !head.contains(" alt=\"") && !head.ends_with('/')
            && let Some(t) = attr(head, "type")
        {
            out.push((t, unescape(&body[..end])));
        }
        rest = &body[end..];
    }
    out
}

/// `<subgroup type="DE" contains="debb debe …"/>` as (type, contains)
pub(super) fn xml_subgroups(xml: &str) -> Vec<(String, String)> {
    xml.split("<subgroup ").skip(1).filter_map(|p| {
        let head = &p[..p.find('>')?];
        Some((attr(&format!(" {head}"), "type")?, attr(&format!(" {head}"), "contains")?))
    }).collect()
}
/// Is there a source for school holidays of `country`?
pub fn has_school(country: &str) -> bool {
    covered_by_openholidays(country)
}
/// The names of the public/school holiday sources for `country`, first to last.
pub fn sources(country: &str) -> (Vec<&'static str>, Vec<&'static str>) {
    let mut public = vec![];
    if covered_by_openholidays(country) {
        public.push("OpenHolidays");
    }
    if covered_by_nager(country) {
        public.push("Nager.Date");
    }
    let school = if has_school(country) { vec!["OpenHolidays"] } else { vec![] };
    (public, school)
}

/// A chosen region for the settings list: its own name and the path above it ("Deutschland › Sachsen").
pub async fn describe(state: &AppState, code: &str, lang: &str) -> AppResult<(String, String)> {
    let (country, sub) = split(code);
    let cname = region_name(country, lang).unwrap_or_else(|| country.to_string());
    if sub.is_empty() {
        return Ok((cname, String::new()));
    }
    let list = subdivisions(state, country, lang).await?;
    let mut path = vec![];
    let mut name = region_name(code, lang).unwrap_or_else(|| code.to_string());
    if let Some(r) = list.iter().find(|r| r.code == code) {
        name = r.name.clone();
        let mut p = r.parent.clone();
        while let Some(x) = list.iter().find(|x| x.code == p).filter(|_| path.len() < 5) {
            path.push(x.name.clone());
            p = x.parent.clone();
        }
    }
    path.push(cname);
    path.reverse();
    Ok((name, path.join(" › ")))
}

/// The CLDR name of a country or subdivision code in `lang` (None for unknown codes).
pub fn region_name(code: &str, lang: &str) -> Option<String> {
    with_table(code.len() != 2, |t| t.find(code).and_then(|r| r.name(lang)).map(str::to_string))
}
