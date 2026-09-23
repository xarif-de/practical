//! Public and school holidays (plan D13) and the region choice behind them.
//!
//! Sources form a chain, tried in order until one answers (each with its own timeout):
//! - public holidays: OpenHolidays (36 countries, regions down to municipalities) →
//!   Nager.Date (204 countries, regions by ISO 3166-2 code) → the last cached answer;
//! - school holidays: OpenHolidays → the last cached answer (no other free source keeps
//!   them per year; ferien-api.de rate-limits, mehr-schulferien.de only has the next 12 months).
//!
//! Answers are cached per (country, subdivision, year, kind-lang) in `xui_holidays` for 30
//! days; OpenHolidays' subdivision lists the same way (year 0). Country and region *names* and
//! the region hierarchy of countries OpenHolidays does not cover come from Unicode CLDR,
//! built into the binary (`src/data/regions.json`, `tools/gen_regions.py`) — no request needed.
//!
//! A region is stored as its ISO 3166-2 code where it has one (DE-SN, AT-1), else as
//! OpenHolidays' own code (CH-AG-ZZ, a district); both APIs accept ISO codes.

use crate::error::AppResult;
use crate::state::AppState;
use chrono::{NaiveDate, Utc};
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

#[derive(serde::Deserialize)]
struct DataCountry {
    oh: u8,
    ng: u8,
    n: HashMap<String, String>,
}
#[derive(serde::Deserialize)]
struct DataSub {
    p: String,
    n: HashMap<String, String>,
}
#[derive(serde::Deserialize)]
struct Data {
    countries: HashMap<String, DataCountry>,
    subs: HashMap<String, DataSub>,
}

static DATA: LazyLock<Data> = LazyLock::new(|| serde_json::from_str(include_str!("../data/regions.json")).expect("src/data/regions.json"));

/// a name in `lang`, else English, else any
fn name_in(n: &HashMap<String, String>, lang: &str) -> Option<String> {
    n.get(lang).or_else(|| n.get("en")).or_else(|| n.values().next()).cloned()
}

fn covered_by_openholidays(country: &str) -> bool {
    DATA.countries.get(country).is_some_and(|c| c.oh == 1)
}
fn covered_by_nager(country: &str) -> bool {
    DATA.countries.get(country).is_some_and(|c| c.ng == 1)
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

/// The CLDR name of a country or subdivision code in `lang` (None for unknown codes).
pub fn region_name(code: &str, lang: &str) -> Option<String> {
    if code.len() == 2 {
        DATA.countries.get(code).and_then(|c| name_in(&c.n, lang))
    } else {
        DATA.subs.get(code).and_then(|s| name_in(&s.n, lang))
    }
}

// ---- cache ----

/// The cached JSON for a key and whether it is younger than CACHE_DAYS.
async fn cache_get(state: &AppState, country: &str, subdivision: &str, year: i32, kind: &str) -> AppResult<Option<(String, bool)>> {
    let row = sqlx::query("SELECT json, fetched_at FROM xui_holidays WHERE country = ? AND subdivision = ? AND year = ? AND kind = ?")
        .bind(country)
        .bind(subdivision)
        .bind(year)
        .bind(kind)
        .fetch_optional(&state.db)
        .await?;
    Ok(row.map(|r| {
        let fetched: String = r.get("fetched_at");
        let fresh = chrono::NaiveDateTime::parse_from_str(&fetched, "%Y-%m-%d %H:%M:%S").map(|f| (Utc::now().naive_utc() - f).num_days() < CACHE_DAYS).unwrap_or(false);
        (r.get("json"), fresh)
    }))
}

async fn cache_put(state: &AppState, country: &str, subdivision: &str, year: i32, kind: &str, json: &str) -> AppResult<()> {
    sqlx::query("INSERT OR REPLACE INTO xui_holidays (country, subdivision, year, kind, json, fetched_at) VALUES (?, ?, ?, ?, ?, CURRENT_TIMESTAMP)")
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
        .map(|h| Holiday { start: h.start_date, end: h.end_date, name: pick(&h.name, lang), kind: kind.to_string(), nationwide: h.nationwide, source: "openholidays".into() })
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
        .filter(|h| h.global || (!region.is_empty() && h.counties.as_ref().is_some_and(|c| c.iter().any(|x| *x == region))))
        .map(|h| Holiday { start: h.date, end: h.date, name: nager_name(&h, lang), kind: "public".into(), nationwide: h.global, source: "nager".into() })
        .collect())
}

/// Cached lookup through the source chain; when every source fails the stale cache (or
/// nothing) is returned — never an error to the view.
pub async fn get(state: &AppState, kind: &str, country: &str, subdivision: &str, year: i32, lang: &str) -> AppResult<Vec<Holiday>> {
    if !state.config.holidays_enabled || country.is_empty() {
        return Ok(vec![]);
    }
    let key = format!("{kind}-{lang}"); // names are in the page language
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
    let mut v: Vec<Region> = DATA
        .countries
        .iter()
        .map(|(code, c)| Region { code: code.clone(), name: name_in(&c.n, lang).unwrap_or_else(|| code.clone()), parent: String::new(), depth: 0, category: String::new(), school: c.oh == 1 })
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
    let mut level: Vec<Region> = DATA
        .subs
        .iter()
        .filter(|(_, s)| s.p == parent)
        .map(|(code, s)| Region { code: code.clone(), name: name_in(&s.n, lang).unwrap_or_else(|| code.clone()), parent: parent.to_string(), depth, category: String::new(), school: false })
        .collect();
    sort(&mut level);
    for r in level {
        let code = r.code.clone();
        out.push(r);
        cldr_tree(&code, lang, depth + 1, out);
    }
}

/// The subdivisions of `country`, all levels, parents first.
pub async fn subdivisions(state: &AppState, country: &str, lang: &str) -> AppResult<Vec<Region>> {
    if !DATA.countries.contains_key(country) {
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
    fn nager_regions_and_names() {
        assert_eq!(nager_region("CH-AG-ZZ"), "CH-AG");
        assert_eq!(nager_region("FR-69M"), "FR-69M");
        let h = |local: &str| NagerHoliday { date: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(), local_name: local.into(), name: "New Year's Day".into(), global: true, counties: None, types: vec!["Public".into()] };
        assert_eq!(nager_name(&h("Neujahr"), "de"), "Neujahr");
        assert_eq!(nager_name(&h("元日"), "de"), "New Year's Day");
        assert_eq!(nager_name(&h("Neujahr"), "en"), "New Year's Day");
    }
}
