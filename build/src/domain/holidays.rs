//! Public and school holidays (plan D13) and the region choice behind them.
//!
//! Sources form a chain, tried in order until one answers (each with its own timeout):
//! - public holidays: OpenHolidays (36 countries, regions down to municipalities) →
//!   Nager.Date (204 countries, regions by ISO 3166-2 code) → the last cached answer;
//! - school holidays: OpenHolidays → the last cached answer (no other free source keeps
//!   them per year; ferien-api.de rate-limits, mehr-schulferien.de only has the next 12 months).
//!
//! Answers are cached per (country, subdivision, year, kind-lang) in `practical_holidays` for 30
//! days; OpenHolidays' subdivision lists the same way (year 0). Country and region *names* and
//! the region hierarchy of countries OpenHolidays does not cover come from Unicode CLDR,
//! built into the binary (`src/data/regions.json`, `tools/gen_regions.py`) — no request needed.
//! Upkeep is lazy (`maintain`, daily from tasks.rs; at start it only reads what is saved):
//! which source covers which country is asked at most weekly (`refresh_coverage`), CLDR is asked
//! at most weekly whether a newer release exists (`check_cldr`: one small request, usually a
//! 304) and only then are its files fetched — about once a year. Saved answers live in
//! `practical_holidays` (year 0) and win over the built-in snapshot when newer.
//!
//! A region is stored as its ISO 3166-2 code where it has one (DE-SN, AT-1), else as
//! OpenHolidays' own code (CH-AG-ZZ, a district); both APIs accept ISO codes. A bare country
//! code (DE) stands for the whole country. A user picks a list of regions per kind
//! (`for_regions`): what several regions share is shown once; what only some of them have
//! carries their short codes ("Buß- und Bettag (SN)").

use crate::error::AppResult;
use crate::state::AppState;
use chrono::NaiveDate;
use sqlx::Row;
use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::Duration;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Holiday {
    pub start: NaiveDate,
    pub end: NaiveDate, // inclusive
    pub name: String,
    pub kind: String, // public | school
    #[serde(default)]
    pub nationwide: bool,
    /// which source answered (openholidays | nager)
    #[serde(default)]
    pub source: String,
    /// the subdivisions (ISO codes) a regional holiday applies to, as the source says
    #[serde(default)]
    pub regions: Vec<String>,
    /// `for_regions`: the chosen regions it applies to when not all of them (short codes, shown)
    #[serde(skip)]
    pub tags: Vec<String>,
}

impl Holiday {
    /// the name as the views show it: "Winterferien (SN, BY)" when it is not every chosen region's
    pub fn label(&self) -> String {
        if self.tags.is_empty() { self.name.clone() } else { format!("{} ({})", self.name, self.tags.join(", ")) }
    }
}

#[derive(serde::Deserialize)]
struct ApiName {
    language: String,
    text: String,
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiHoliday {
    start_date: NaiveDate,
    end_date: NaiveDate,
    name: Vec<ApiName>,
    #[serde(default)]
    nationwide: bool,
    #[serde(default)]
    subdivisions: Vec<ApiSubRef>,
}
#[derive(serde::Deserialize)]
struct ApiSubRef {
    #[serde(default)]
    code: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NagerHoliday {
    date: NaiveDate,
    local_name: String,
    name: String,
    #[serde(default)]
    global: bool,
    #[serde(default)]
    counties: Option<Vec<String>>,
    #[serde(default)]
    types: Vec<String>,
}

const OPENHOLIDAYS: &str = "https://openholidaysapi.org";
const NAGER: &str = "https://date.nager.at/api/v3";
const CACHE_DAYS: i64 = 30;
/// one link of the chain may take this long before the next is asked
const TIMEOUT: Duration = Duration::from_secs(8);

// ---- built-in region data (CLDR) ----

#[derive(serde::Deserialize, serde::Serialize)]
struct DataCountry {
    #[serde(default)]
    oh: u8,
    #[serde(default)]
    ng: u8,
    n: HashMap<String, String>,
}
#[derive(serde::Deserialize, serde::Serialize)]
struct DataSub {
    p: String,
    n: HashMap<String, String>,
}
#[derive(serde::Deserialize, serde::Serialize)]
struct Data {
    /// CLDR release the names come from ("release-48-2")
    #[serde(default)]
    cldr: String,
    countries: HashMap<String, DataCountry>,
    subs: HashMap<String, DataSub>,
}

static BUILT_IN: LazyLock<Data> = LazyLock::new(|| serde_json::from_str(include_str!("../data/regions.json")).expect("src/data/regions.json"));
/// the names in use: the built-in ones, replaced by a newer CLDR release once `check_cldr` got one
static NEWER: std::sync::RwLock<Option<std::sync::Arc<Data>>> = std::sync::RwLock::new(None);

fn with_data<T>(f: impl FnOnce(&Data) -> T) -> T {
    let newer = NEWER.read().unwrap().clone();
    match newer {
        Some(d) => f(&d),
        None => f(&BUILT_IN),
    }
}

/// a name in `lang`, else English, else any
fn name_in(n: &HashMap<String, String>, lang: &str) -> Option<String> {
    n.get(lang).or_else(|| n.get("en")).or_else(|| n.values().next()).cloned()
}

/// Which countries each source covers: the built-in snapshot until `refresh_coverage` knows better.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
struct Coverage {
    oh: std::collections::BTreeSet<String>,
    ng: std::collections::BTreeSet<String>,
}

static COVERAGE: LazyLock<std::sync::RwLock<Coverage>> = LazyLock::new(|| {
    let pick = |f: fn(&DataCountry) -> bool| BUILT_IN.countries.iter().filter(|(_, c)| f(c)).map(|(k, _)| k.clone()).collect();
    std::sync::RwLock::new(Coverage { oh: pick(|c| c.oh == 1), ng: pick(|c| c.ng == 1) })
});

fn covered_by_openholidays(country: &str) -> bool {
    COVERAGE.read().unwrap().oh.contains(country)
}
fn covered_by_nager(country: &str) -> bool {
    COVERAGE.read().unwrap().ng.contains(country)
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct OhCountry {
    iso_code: String,
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NagerCountry {
    country_code: String,
}

/// A fresh list replaces the known one unless it looks broken: empty, or less than half as
/// long (an outage answering `[]` must not wipe a source's countries).
fn accept(known: &std::collections::BTreeSet<String>, fresh: Result<Vec<String>, String>, what: &str) -> std::collections::BTreeSet<String> {
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
async fn refresh_coverage(state: &AppState) -> AppResult<()> {
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
const CLDR: &str = "https://raw.githubusercontent.com/unicode-org/cldr";
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
            && newer(&saved.cldr, &BUILT_IN.cldr)
        {
            tracing::info!("region names: {} (saved) over {} (built in)", saved.cldr, BUILT_IN.cldr);
            *NEWER.write().unwrap() = Some(std::sync::Arc::new(saved));
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
fn cldr_version(tag: &str) -> Vec<u32> {
    tag.split(|c: char| !c.is_ascii_digit()).filter(|p| !p.is_empty()).filter_map(|p| p.parse().ok()).collect()
}
fn newer(tag: &str, than: &str) -> bool {
    !cldr_version(tag).is_empty() && cldr_version(tag) > cldr_version(than)
}

#[derive(serde::Deserialize, serde::Serialize, Default)]
struct CldrCheck {
    etag: String,
    tag: String,
}

/// Ask GitHub for CLDR's latest release (with the last ETag, so an unchanged answer is a
/// bodiless 304); fetch and apply its names only when it is newer than the ones in use.
async fn check_cldr(state: &AppState) -> AppResult<()> {
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
    let current = with_data(|d| d.cldr.clone());
    tracing::info!("cldr check: latest {}, in use {current}", check.tag);
    if newer(&check.tag, &current) {
        match fetch_cldr(state, &check.tag).await {
            Ok(d) => {
                tracing::info!("region names: CLDR {} replaces {current}", check.tag);
                cache_put(state, "", "", 0, "cldr-data", &serde_json::to_string(&d).unwrap()).await?;
                *NEWER.write().unwrap() = Some(std::sync::Arc::new(d));
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
fn iso(cldr: &str) -> String {
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
fn xml_items(xml: &str, tag: &str) -> Vec<(String, String)> {
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
fn xml_subgroups(xml: &str) -> Vec<(String, String)> {
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
    with_data(|d| if code.len() == 2 { d.countries.get(code).and_then(|c| name_in(&c.n, lang)) } else { d.subs.get(code).and_then(|s| name_in(&s.n, lang)) })
}

// ---- cache ----

/// The cached JSON for a key and whether it is younger than CACHE_DAYS.
async fn cache_get(state: &AppState, country: &str, subdivision: &str, year: i32, kind: &str) -> AppResult<Option<(String, bool)>> {
    Ok(cache_aged(state, country, subdivision, year, kind).await?.map(|(j, hours)| (j, hours < CACHE_DAYS * 24)))
}

/// The cached JSON for a key and its age in hours.
async fn cache_aged(state: &AppState, country: &str, subdivision: &str, year: i32, kind: &str) -> AppResult<Option<(String, i64)>> {
    let row = sqlx::query("SELECT json, fetched_at FROM practical_holidays WHERE country = ? AND subdivision = ? AND year = ? AND kind = ?")
        .bind(country)
        .bind(subdivision)
        .bind(year)
        .bind(kind)
        .fetch_optional(&state.db)
        .await?;
    Ok(row.map(|r| {
        let fetched: String = r.get("fetched_at");
        let hours = chrono::NaiveDateTime::parse_from_str(&fetched, "%Y-%m-%d %H:%M:%S").map(|f| (crate::clock::now().naive_utc() - f).num_hours()).unwrap_or(i64::MAX);
        (r.get("json"), hours)
    }))
}

async fn cache_put(state: &AppState, country: &str, subdivision: &str, year: i32, kind: &str, json: &str) -> AppResult<()> {
    sqlx::query("INSERT OR REPLACE INTO practical_holidays (country, subdivision, year, kind, json, fetched_at) VALUES (?, ?, ?, ?, ?, CURRENT_TIMESTAMP)")
        .bind(country)
        .bind(subdivision)
        .bind(year)
        .bind(kind)
        .bind(json)
        .execute(&state.db)
        .await?;
    Ok(())
}

async fn get_json<T: serde::de::DeserializeOwned>(state: &AppState, url: &str) -> Result<T, String> {
    let resp = state.http.get(url).header("Accept", "application/json").timeout(TIMEOUT).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("{url}: HTTP {}", resp.status()));
    }
    resp.json().await.map_err(|e| format!("{url}: {e}"))
}

// ---- holidays ----

fn pick(names: &[ApiName], lang: &str) -> String {
    names.iter().find(|n| n.language.eq_ignore_ascii_case(lang)).or_else(|| names.iter().find(|n| n.language.eq_ignore_ascii_case("en"))).or(names.first()).map(|n| n.text.clone()).unwrap_or_default()
}

async fn from_openholidays(state: &AppState, kind: &str, country: &str, subdivision: &str, year: i32, lang: &str) -> Result<Vec<Holiday>, String> {
    let path = if kind == "school" { "SchoolHolidays" } else { "PublicHolidays" };
    let mut url = format!("{OPENHOLIDAYS}/{path}?countryIsoCode={country}&languageIsoCode={}&validFrom={year}-01-01&validTo={year}-12-31", lang.to_uppercase());
    if !subdivision.is_empty() {
        url.push_str(&format!("&subdivisionCode={subdivision}"));
    }
    let items: Vec<ApiHoliday> = get_json(state, &url).await?;
    Ok(items
        .into_iter()
        .map(|h| Holiday { start: h.start_date, end: h.end_date, name: pick(&h.name, lang), kind: kind.to_string(), nationwide: h.nationwide, source: "openholidays".into(), regions: h.subdivisions.into_iter().map(|s| s.code).filter(|c| !c.is_empty()).collect(), tags: vec![] })
        .collect())
}

/// ISO code Nager.Date knows for a stored region: OpenHolidays' deeper codes (CH-AG-ZZ)
/// fall back to their first level (CH-AG).
fn nager_region(subdivision: &str) -> String {
    subdivision.splitn(3, '-').take(2).collect::<Vec<_>>().join("-")
}

/// Nager.Date names a holiday in the country's language and in English; a Latin-script local
/// name is kept (a German reader of Austrian holidays wants "Neujahr"), others read English.
fn nager_name(h: &NagerHoliday, lang: &str) -> String {
    if lang == "en" || h.local_name.chars().any(|c| c.is_alphabetic() && (c as u32) > 0x024F) { h.name.clone() } else { h.local_name.clone() }
}

async fn from_nager(state: &AppState, country: &str, subdivision: &str, year: i32, lang: &str) -> Result<Vec<Holiday>, String> {
    let items: Vec<NagerHoliday> = get_json(state, &format!("{NAGER}/PublicHolidays/{year}/{country}")).await?;
    let region = nager_region(subdivision);
    Ok(items
        .into_iter()
        .filter(|h| h.types.iter().any(|t| t == "Public"))
        // the whole country: its regional holidays too (tagged with their regions by for_regions)
        .filter(|h| h.global || region.is_empty() || h.counties.as_ref().is_some_and(|c| c.iter().any(|x| *x == region)))
        .map(|h| Holiday { start: h.date, end: h.date, name: nager_name(&h, lang), kind: "public".into(), nationwide: h.global, source: "nager".into(), regions: if h.global { vec![] } else { h.counties.clone().unwrap_or_default() }, tags: vec![] })
        .collect())
}

/// Cached lookup through the source chain; when every source fails the stale cache (or
/// nothing) is returned — never an error to the view.
pub async fn get(state: &AppState, kind: &str, country: &str, subdivision: &str, year: i32, lang: &str) -> AppResult<Vec<Holiday>> {
    if !state.config.holidays_enabled || country.is_empty() {
        return Ok(vec![]);
    }
    let key = format!("{kind}2-{lang}"); // names are in the page language; 2: entries carry `regions`
    let cached: Option<(Vec<Holiday>, bool)> = cache_get(state, country, subdivision, year, &key).await?.map(|(j, fresh)| (serde_json::from_str(&j).unwrap_or_default(), fresh));
    if let Some((h, true)) = &cached {
        return Ok(h.clone());
    }
    let mut errors = vec![];
    let mut answer = None;
    if covered_by_openholidays(country) {
        match from_openholidays(state, kind, country, subdivision, year, lang).await {
            Ok(h) => answer = Some(h),
            Err(e) => errors.push(e),
        }
    }
    if answer.is_none() && kind == "public" && covered_by_nager(country) {
        match from_nager(state, country, subdivision, year, lang).await {
            Ok(h) => answer = Some(h),
            Err(e) => errors.push(e),
        }
    }
    match answer {
        Some(h) => {
            cache_put(state, country, subdivision, year, &key, &serde_json::to_string(&h).unwrap()).await?;
            Ok(h)
        }
        None => {
            if !errors.is_empty() {
                tracing::warn!("holidays {kind} {country}/{subdivision} {year}: every source failed: {}", errors.join("; "));
            }
            Ok(cached.map(|(h, _)| h).unwrap_or_default())
        }
    }
}

/// (country, subdivision) of a stored region code: "DE" → ("DE", ""), "DE-SN" → ("DE", "DE-SN").
pub fn split(code: &str) -> (&str, &str) {
    match code.get(..2) {
        Some(c) if code.len() > 2 => (c, code),
        _ => (code, ""),
    }
}

/// Is `code` a region code as stored (DE, DE-SN, CH-AG-ZZ, FR-69M)?
pub fn valid_code(code: &str) -> bool {
    let mut parts = code.split('-');
    parts.next().is_some_and(|c| c.len() == 2 && c.chars().all(|x| x.is_ascii_uppercase())) && parts.all(|p| !p.is_empty() && p.len() <= 6 && p.chars().all(|x| x.is_ascii_uppercase() || x.is_ascii_digit()))
}

/// Short codes for tags: without the country when every chosen region lies in one ("SN").
fn shorten(codes: &[String], regions: &[String]) -> Vec<String> {
    let one = regions.first().map(|r| split(r).0).filter(|c| regions.iter().all(|r| split(r).0 == *c));
    codes.iter().map(|c| one.and_then(|o| c.strip_prefix(o).and_then(|x| x.strip_prefix('-'))).unwrap_or(c).to_string()).collect()
}

/// Several chosen regions of one country that all have a holiday: the country stands for them
/// ("DE" instead of "DE-BY, DE-SN").
fn collapse(mut codes: Vec<String>, covers: &std::collections::BTreeSet<usize>, regions: &[String]) -> Vec<String> {
    let mut countries: Vec<&str> = regions.iter().map(|r| split(r).0).collect();
    countries.dedup();
    for c in countries {
        let of_c: Vec<usize> = (0..regions.len()).filter(|i| split(&regions[*i]).0 == c).collect();
        if of_c.len() > 1 && of_c.iter().all(|i| covers.contains(i)) {
            let at = codes.iter().position(|x| split(x).0 == c).unwrap_or(codes.len());
            codes.retain(|x| split(x).0 != c);
            codes.insert(at.min(codes.len()), c.to_string());
        }
    }
    codes
}

/// The holidays of several regions in one list: a holiday (same name and dates) several regions
/// share appears once; `tags` names the regions it applies to unless it applies to all of them.
/// A whole country counts as covered by a nationwide holiday only; a regional one names its
/// subdivisions.
pub async fn for_regions(state: &AppState, kind: &str, regions: &[String], year: i32, lang: &str) -> AppResult<Vec<Holiday>> {
    let mut per = vec![];
    for r in regions {
        let (country, sub) = split(r);
        per.push(get(state, kind, country, sub, year, lang).await?);
    }
    Ok(merge(regions, per))
}

fn merge(regions: &[String], per: Vec<Vec<Holiday>>) -> Vec<Holiday> {
    // (holiday, codes it applies to, indexes of the chosen regions it fully covers)
    let mut acc: Vec<(Holiday, Vec<String>, std::collections::BTreeSet<usize>)> = vec![];
    for (i, hs) in per.into_iter().enumerate() {
        let (country, sub) = split(&regions[i]);
        for h in hs {
            let full = !sub.is_empty() || h.nationwide || h.regions.is_empty();
            let codes: Vec<String> = if !sub.is_empty() { vec![sub.to_string()] } else if full { vec![country.to_string()] } else { h.regions.clone() };
            let at = acc.iter().position(|(x, _, _)| x.start == h.start && x.end == h.end && x.name == h.name);
            let (_, have, covers) = match at {
                Some(k) => &mut acc[k],
                None => {
                    acc.push((h, vec![], Default::default()));
                    acc.last_mut().unwrap()
                }
            };
            for c in codes {
                if !have.contains(&c) {
                    have.push(c);
                }
            }
            if full {
                covers.insert(i);
            }
        }
    }
    let mut out: Vec<Holiday> = acc
        .into_iter()
        .map(|(mut h, codes, covers)| {
            h.tags = if covers.len() == regions.len() { vec![] } else { shorten(&collapse(codes, &covers, regions), regions) };
            h
        })
        .collect();
    out.sort_by(|a, b| (a.start, &a.name).cmp(&(b.start, &b.name)));
    out
}

/// A public holiday or school holiday on one day, as the day/week/month views show it.
#[derive(Debug, Clone)]
pub struct Mark {
    pub name: String,
    /// the chosen regions it applies to when not all of them (`Holiday::tags`)
    pub tags: Vec<String>,
    pub school: bool,
    /// the first day of the holiday (block) this day belongs to
    pub start: NaiveDate,
}

/// The holidays the user asked for (settings), by day, for `from..to` — named, in `lang`.
/// Public ones come first on a day; the years in the window are fetched (cached) once each.
pub async fn marks(state: &AppState, user: &str, from: NaiveDate, to: NaiveDate, lang: &str) -> AppResult<HashMap<NaiveDate, Vec<Mark>>> {
    use chrono::Datelike;
    let p = crate::domain::prefs::get(&state.db, user).await?;
    let mut out: HashMap<NaiveDate, Vec<Mark>> = HashMap::new();
    // a block across New Year (Weihnachtsferien) comes back for both years: count it once
    let mut seen = std::collections::HashSet::new();
    for (kind, on) in [("public", p.holidays_enabled), ("school", p.school_holidays_enabled)] {
        if !on {
            continue;
        }
        let regions = p.regions(&state.config, kind == "school");
        for year in from.year()..=to.year() {
            for h in for_regions(state, kind, &regions, year, lang).await? {
                if !seen.insert((kind, h.start, h.end, h.name.clone(), h.tags.clone())) {
                    continue;
                }
                let mut d = h.start.max(from);
                while d <= h.end && d < to {
                    out.entry(d).or_default().push(Mark { name: h.name.clone(), tags: h.tags.clone(), school: kind == "school", start: h.start });
                    let Some(next) = d.succ_opt() else { break };
                    d = next;
                }
            }
        }
    }
    Ok(out)
}

// ---- region choice ----

/// A country or subdivision for the region choice. Subdivisions come flattened in display
/// order (parents before their children); `parent` is the country for the top level.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Region {
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub parent: String,
    #[serde(default)]
    pub depth: u8,
    /// what the level is called in `lang` ("Bundesland", "Kanton", "Provinz" …), where known
    #[serde(default)]
    pub category: String,
    /// countries: a source for school holidays exists
    #[serde(default)]
    pub school: bool,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiRegion {
    #[serde(default)]
    iso_code: String,
    #[serde(default)]
    code: String,
    #[serde(default)]
    name: Vec<ApiName>,
    #[serde(default)]
    category: Vec<ApiName>,
    #[serde(default)]
    children: Vec<ApiRegion>,
}

/// sort key: letters without their accents, so "Österreich" sorts with the O's
fn fold(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'ä' | 'à' | 'á' | 'â' | 'ã' | 'å' => 'a',
            'ö' | 'ò' | 'ó' | 'ô' | 'õ' => 'o',
            'ü' | 'ù' | 'ú' | 'û' => 'u',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ç' | 'č' | 'ć' => 'c',
            'ñ' => 'n',
            'ß' | 'š' => 's',
            'ž' => 'z',
            o => o,
        })
        .collect()
}

fn sort(v: &mut [Region]) {
    v.sort_by_key(|r| fold(&r.name));
}

/// The countries a holiday source covers, named in `lang` (built in, no request).
pub fn countries(lang: &str) -> Vec<Region> {
    let cov = COVERAGE.read().unwrap().clone();
    let mut v: Vec<Region> = cov
        .oh
        .union(&cov.ng)
        .map(|code| Region { code: code.clone(), name: region_name(code, lang).unwrap_or_else(|| code.clone()), parent: String::new(), depth: 0, category: String::new(), school: cov.oh.contains(code) })
        .collect();
    sort(&mut v);
    v
}

/// OpenHolidays' tree, flattened; a region is named from the API in `lang`, else from CLDR
/// (its ISO code), else from the API in any language.
fn flatten(items: Vec<ApiRegion>, lang: &str, parent: &str, depth: u8, out: &mut Vec<Region>) {
    let mut level: Vec<(Region, Vec<ApiRegion>)> = items
        .into_iter()
        .map(|r| {
            let code = if r.iso_code.is_empty() { r.code.clone() } else { r.iso_code.clone() };
            let exact = r.name.iter().find(|n| n.language.eq_ignore_ascii_case(lang)).map(|n| n.text.clone());
            let name = exact.or_else(|| region_name(&code, lang)).unwrap_or_else(|| pick(&r.name, lang));
            let cat = r.category.iter().find(|n| n.language.eq_ignore_ascii_case(lang)).map(|n| n.text.clone()).unwrap_or_default();
            (Region { code, name, parent: parent.to_string(), depth, category: cat, school: false }, r.children)
        })
        .collect();
    level.sort_by_key(|(r, _)| fold(&r.name));
    for (r, children) in level {
        let code = r.code.clone();
        out.push(r);
        flatten(children, lang, &code, depth + 1, out);
    }
}

/// CLDR's hierarchy below `parent`, flattened the same way (countries OpenHolidays lacks).
fn cldr_tree(parent: &str, lang: &str, depth: u8, out: &mut Vec<Region>) {
    let mut level: Vec<Region> = with_data(|d| {
        d.subs
            .iter()
            .filter(|(_, s)| s.p == parent)
            .map(|(code, s)| Region { code: code.clone(), name: name_in(&s.n, lang).unwrap_or_else(|| code.clone()), parent: parent.to_string(), depth, category: String::new(), school: false })
            .collect()
    });
    sort(&mut level);
    for r in level {
        let code = r.code.clone();
        out.push(r);
        cldr_tree(&code, lang, depth + 1, out);
    }
}

/// The subdivisions of `country`, all levels, parents first.
pub async fn subdivisions(state: &AppState, country: &str, lang: &str) -> AppResult<Vec<Region>> {
    if !covered_by_openholidays(country) && !covered_by_nager(country) {
        return Ok(vec![]);
    }
    if covered_by_openholidays(country) {
        let key = format!("subdivisions2-{lang}"); // 2: entries carry `parent` (the first format did not)
        let cached: Option<(Vec<Region>, bool)> = cache_get(state, country, "", 0, &key).await?.map(|(j, fresh)| (serde_json::from_str(&j).unwrap_or_default(), fresh));
        if let Some((r, true)) = &cached {
            return Ok(r.clone());
        }
        let url = format!("{OPENHOLIDAYS}/Subdivisions?countryIsoCode={country}&languageIsoCode={}", lang.to_uppercase());
        match get_json::<Vec<ApiRegion>>(state, &url).await {
            Ok(items) => {
                let mut out = vec![];
                flatten(items, lang, country, 0, &mut out);
                cache_put(state, country, "", 0, &key, &serde_json::to_string(&out).unwrap()).await?;
                return Ok(out);
            }
            Err(e) => {
                tracing::warn!("openholidays subdivisions {country}: {e}");
                if let Some((r, _)) = cached {
                    return Ok(r);
                }
            }
        }
    }
    let mut out = vec![];
    cldr_tree(country, lang, 0, &mut out);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(code: &str, iso: &str, name: &str, children: Vec<ApiRegion>) -> ApiRegion {
        ApiRegion { iso_code: iso.into(), code: code.into(), name: vec![ApiName { language: "DE".into(), text: name.into() }], category: vec![ApiName { language: "DE".into(), text: "Kanton".into() }], children }
    }

    #[test]
    fn subdivisions_flatten_parents_first_sorted_without_accents() {
        let mut out = vec![];
        flatten(vec![r("CH-ZH", "CH-ZH", "Zürich", vec![r("CH-ZH-B", "", "Bülach", vec![]), r("CH-ZH-A", "", "Affoltern", vec![])]), r("CH-AG", "CH-AG", "Aargau", vec![])], "de", "CH", 0, &mut out);
        let got: Vec<(&str, &str, u8)> = out.iter().map(|x| (x.code.as_str(), x.parent.as_str(), x.depth)).collect();
        assert_eq!(got, [("CH-AG", "CH", 0), ("CH-ZH", "CH", 0), ("CH-ZH-A", "CH-ZH", 1), ("CH-ZH-B", "CH-ZH", 1)]);
        assert_eq!(out[0].category, "Kanton");
        assert!(fold("Österreich") < fold("Polen"));
    }

    #[test]
    fn iso_code_wins_and_cldr_names_what_the_api_lacks() {
        // OpenHolidays calls Burgenland AT-BL but its ISO code is AT-1; no French name from the API
        let mut out = vec![];
        flatten(vec![r("AT-BL", "AT-1", "Burgenland", vec![])], "fr", "AT", 0, &mut out);
        assert_eq!(out[0].code, "AT-1");
        assert_eq!(out[0].name, "Burgenland");
        assert_eq!(out[0].category, "");
    }

    #[test]
    fn built_in_names_cover_every_language() {
        for l in crate::i18n::LANGS {
            assert!(region_name("DE", l).is_some(), "country name {l}");
            assert!(region_name("DE-SN", l).is_some(), "subdivision name {l}");
        }
        assert_eq!(region_name("US-CA", "de").as_deref(), Some("Kalifornien"));
        assert!(countries("de").len() > 100);
        let mut us = vec![];
        cldr_tree("US", "en", 0, &mut us);
        assert!(us.iter().any(|r| r.code == "US-TX" && r.name == "Texas"));
    }

    #[test]
    fn cldr_versions_and_xml() {
        assert!(newer("release-49", "release-48-2"));
        assert!(newer("release-48-2", "release-48"));
        assert!(!newer("release-48", "release-48-2"));
        assert!(!newer("", "release-48"));
        assert_eq!(BUILT_IN.cldr, "release-48-2");
        let xml = r#"<territories><territory type="DE">Deutschland</territory><territory type="US" alt="short">USA</territory><territory type="US">Vereinigte Staaten</territory><territory type="BA">Bosnien &amp; Herzegowina</territory></territories>
<subdivision type="desn" draft="contributed">Sachsen</subdivision>
<subgroup type="DE" contains="debb desn"/><subgroup type="frara" contains="fr01 fr69m"/>"#;
        assert_eq!(xml_items(xml, "territory"), [("DE".into(), "Deutschland".into()), ("US".into(), "Vereinigte Staaten".into()), ("BA".into(), "Bosnien & Herzegowina".into())]);
        assert_eq!(xml_items(xml, "subdivision"), [("desn".into(), "Sachsen".into())]);
        assert_eq!(xml_subgroups(xml), [("DE".into(), "debb desn".into()), ("frara".into(), "fr01 fr69m".into())]);
        assert_eq!(iso("fr69m"), "FR-69M");
    }

    #[test]
    fn a_broken_coverage_list_keeps_the_known_one() {
        let known: std::collections::BTreeSet<String> = ["AT", "CH", "DE", "FR"].map(String::from).into();
        assert_eq!(accept(&known, Ok(vec![]), "t"), known);
        assert_eq!(accept(&known, Ok(vec!["DE".into()]), "t"), known);
        assert_eq!(accept(&known, Err("down".into()), "t"), known);
        let grown = accept(&known, Ok(["AT", "CH", "DE", "FR", "IT"].map(String::from).to_vec()), "t");
        assert!(grown.contains("IT") && grown.len() == 5);
        assert!(region_name("KR", "de").is_some(), "names for countries no source covers yet are built in");
    }

    fn hol(name: &str, day: u32, nationwide: bool, regions: &[&str]) -> Holiday {
        let d = NaiveDate::from_ymd_opt(2026, 11, day).unwrap();
        Holiday { start: d, end: d, name: name.into(), kind: "public".into(), nationwide, source: String::new(), regions: regions.iter().map(|r| r.to_string()).collect(), tags: vec![] }
    }

    #[test]
    fn several_regions_merge_and_tag_what_not_all_share() {
        let regions: Vec<String> = ["DE-SN", "DE-BY"].map(String::from).into();
        let sn = vec![hol("Buß- und Bettag", 18, false, &["DE-SN"]), hol("Neujahr", 2, true, &[])];
        let by = vec![hol("Allerheiligen", 1, false, &["DE-BY"]), hol("Neujahr", 2, true, &[])];
        let got: Vec<String> = merge(&regions, vec![sn, by]).iter().map(Holiday::label).collect();
        assert_eq!(got, ["Allerheiligen (BY)", "Neujahr", "Buß- und Bettag (SN)"]);
        // a whole country: covered by nationwide ones only; regional ones name their subdivisions
        let de: Vec<String> = vec!["DE".into()];
        let got: Vec<String> = merge(&de, vec![vec![hol("Neujahr", 2, true, &[]), hol("Allerheiligen", 1, false, &["DE-BW", "DE-BY"])]]).iter().map(Holiday::label).collect();
        assert_eq!(got, ["Allerheiligen (BW, BY)", "Neujahr"]);
        // two countries keep the country in the tag
        let two: Vec<String> = ["DE-SN", "AT"].map(String::from).into();
        let got: Vec<String> = merge(&two, vec![vec![hol("Neujahr", 2, true, &[])], vec![hol("Neujahr", 2, true, &[]), hol("Leopold", 15, false, &["AT-3"])]]).iter().map(Holiday::label).collect();
        assert_eq!(got, ["Neujahr", "Leopold (AT-3)"]);
        // every chosen region of one country has it: the country stands for them
        let three: Vec<String> = ["DE-BY", "DE-SN", "AT"].map(String::from).into();
        let xmas = || vec![hol("1. Weihnachtsfeiertag", 25, true, &[])];
        let got: Vec<String> = merge(&three, vec![xmas(), xmas(), vec![hol("Christtag", 25, true, &[])]]).iter().map(Holiday::label).collect();
        assert_eq!(got, ["1. Weihnachtsfeiertag (DE)", "Christtag (AT)"]);
        assert_eq!(split("CH-AG-ZZ"), ("CH", "CH-AG-ZZ"));
        assert!(valid_code("DE") && valid_code("FR-69M") && valid_code("CH-AG-ZZ"));
        assert!(!valid_code("de") && !valid_code("D") && !valid_code("DE-") && !valid_code("DE,AT"));
    }

    #[test]
    fn nager_regions_and_names() {
        assert_eq!(nager_region("CH-AG-ZZ"), "CH-AG");
        assert_eq!(nager_region("FR-69M"), "FR-69M");
        let h = |local: &str| NagerHoliday { date: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(), local_name: local.into(), name: "New Year's Day".into(), global: true, counties: None, types: vec!["Public".into()] };
        assert_eq!(nager_name(&h("Neujahr"), "de"), "Neujahr");
        assert_eq!(nager_name(&h("元日"), "de"), "New Year's Day");
        assert_eq!(nager_name(&h("Neujahr"), "en"), "New Year's Day");
    }
}
