//! Public and school holidays from the OpenHolidays API (plan D13), cached per
//! (country, subdivision, year, kind) in `xui_holidays` for 30 days. The same table
//! caches the API's country and subdivision lists (year 0, kind `countries-<lang>` /
//! `subdivisions-<lang>`) for the region choice in the settings.

use crate::error::AppResult;
use crate::state::AppState;
use chrono::{NaiveDate, Utc};
use sqlx::Row;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Holiday {
    pub start: NaiveDate,
    pub end: NaiveDate, // inclusive
    pub name: String,
    pub kind: String, // public | school
    #[serde(default)]
    pub nationwide: bool,
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

const API: &str = "https://openholidaysapi.org";
const CACHE_DAYS: i64 = 30;

async fn fetch(state: &AppState, kind: &str, country: &str, subdivision: &str, year: i32, lang: &str) -> Result<Vec<Holiday>, String> {
    let path = if kind == "school" { "SchoolHolidays" } else { "PublicHolidays" };
    let mut url = format!("{API}/{path}?countryIsoCode={country}&languageIsoCode={}&validFrom={year}-01-01&validTo={year}-12-31", lang.to_uppercase());
    if !subdivision.is_empty() {
        url.push_str(&format!("&subdivisionCode={subdivision}"));
    }
    let resp = state.http.get(&url).header("Accept", "application/json").send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("{url}: HTTP {}", resp.status()));
    }
    let items: Vec<ApiHoliday> = resp.json().await.map_err(|e| e.to_string())?;
    Ok(items
        .into_iter()
        .map(|h| Holiday {
            start: h.start_date,
            end: h.end_date,
            name: h.name.iter().find(|n| n.language.eq_ignore_ascii_case(lang)).or(h.name.first()).map(|n| n.text.clone()).unwrap_or_default(),
            kind: kind.to_string(),
            nationwide: h.nationwide,
        })
        .collect())
}

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

/// Cached lookup; a failed fetch returns the stale cache or an empty list (never an error to the view).
pub async fn get(state: &AppState, kind: &str, country: &str, subdivision: &str, year: i32, lang: &str) -> AppResult<Vec<Holiday>> {
    if !state.config.holidays_enabled || country.is_empty() {
        return Ok(vec![]);
    }
    let cached: Option<(Vec<Holiday>, bool)> = cache_get(state, country, subdivision, year, kind).await?.map(|(j, fresh)| (serde_json::from_str(&j).unwrap_or_default(), fresh));
    if let Some((h, true)) = &cached {
        return Ok(h.clone());
    }
    match fetch(state, kind, country, subdivision, year, lang).await {
        Ok(h) => {
            cache_put(state, country, subdivision, year, kind, &serde_json::to_string(&h).unwrap()).await?;
            Ok(h)
        }
        Err(e) => {
            tracing::warn!("openholidays {kind} {country}/{subdivision} {year}: {e}");
            Ok(cached.map(|(h, _)| h).unwrap_or_default())
        }
    }
}

/// A country or subdivision for the region choice. Subdivisions come flattened in display
/// order (parents before their children), `depth` 0 = the top level (Bundesland, Kanton …).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Region {
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub depth: u8,
    /// what the level is called in `lang` ("Bundesland", "Kanton", "Provinz" …)
    #[serde(default)]
    pub category: String,
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

fn pick(names: &[ApiName], lang: &str) -> String {
    names.iter().find(|n| n.language.eq_ignore_ascii_case(lang)).or(names.first()).map(|n| n.text.clone()).unwrap_or_default()
}

/// sort key: letters without their accents, so "Österreich" sorts with the O's
fn fold(s: &str) -> String {
    s.to_lowercase().chars().map(|c| match c { 'ä' | 'à' | 'á' | 'â' => 'a', 'ö' | 'ó' | 'ô' => 'o', 'ü' | 'ú' | 'û' => 'u', 'é' | 'è' | 'ê' => 'e', 'ß' => 's', 'č' => 'c', 'š' => 's', 'ž' => 'z', o => o }).collect()
}

fn flatten(items: Vec<ApiRegion>, lang: &str, depth: u8, out: &mut Vec<Region>) {
    let mut items: Vec<(String, ApiRegion)> = items.into_iter().map(|r| (pick(&r.name, lang), r)).collect();
    items.sort_by_key(|(n, _)| fold(n));
    for (name, r) in items {
        out.push(Region { code: r.code, name, depth, category: pick(&r.category, lang) });
        flatten(r.children, lang, depth + 1, out);
    }
}

async fn fetch_regions(state: &AppState, url: &str) -> Result<Vec<ApiRegion>, String> {
    let resp = state.http.get(url).header("Accept", "application/json").send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("{url}: HTTP {}", resp.status()));
    }
    resp.json().await.map_err(|e| e.to_string())
}

/// Cached region list; a failed fetch returns the stale cache or nothing.
async fn regions(state: &AppState, country: &str, kind: &str, url: String, lang: &str, countries: bool) -> AppResult<Vec<Region>> {
    let cached: Option<(Vec<Region>, bool)> = cache_get(state, country, "", 0, kind).await?.map(|(j, fresh)| (serde_json::from_str(&j).unwrap_or_default(), fresh));
    if let Some((r, true)) = &cached {
        return Ok(r.clone());
    }
    match fetch_regions(state, &url).await {
        Ok(items) => {
            let mut out = vec![];
            if countries {
                out = items.into_iter().map(|r| Region { name: pick(&r.name, lang), code: r.iso_code, depth: 0, category: String::new() }).collect();
                out.sort_by_key(|r| fold(&r.name));
            } else {
                flatten(items, lang, 0, &mut out);
            }
            cache_put(state, country, "", 0, kind, &serde_json::to_string(&out).unwrap()).await?;
            Ok(out)
        }
        Err(e) => {
            tracing::warn!("openholidays {kind} {country}: {e}");
            Ok(cached.map(|(r, _)| r).unwrap_or_default())
        }
    }
}

/// The countries OpenHolidays knows, named in `lang`.
pub async fn countries(state: &AppState, lang: &str) -> AppResult<Vec<Region>> {
    let url = format!("{API}/Countries?languageIsoCode={}", lang.to_uppercase());
    regions(state, "", &format!("countries-{lang}"), url, lang, true).await
}

/// The subdivisions of `country` (all levels), named in `lang` where the API has it.
pub async fn subdivisions(state: &AppState, country: &str, lang: &str) -> AppResult<Vec<Region>> {
    if country.len() != 2 || !country.chars().all(|c| c.is_ascii_uppercase()) {
        return Ok(vec![]);
    }
    let url = format!("{API}/Subdivisions?countryIsoCode={country}&languageIsoCode={}", lang.to_uppercase());
    regions(state, country, &format!("subdivisions-{lang}"), url, lang, false).await
}

/// Is `d` a public holiday (used by the vacation counter, D14)?
pub fn is_holiday(hols: &[Holiday], d: NaiveDate) -> Option<&Holiday> {
    hols.iter().find(|h| h.kind == "public" && h.start <= d && d <= h.end)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(code: &str, name: &str, children: Vec<ApiRegion>) -> ApiRegion {
        ApiRegion { iso_code: String::new(), code: code.into(), name: vec![ApiName { language: "DE".into(), text: name.into() }], category: vec![ApiName { language: "DE".into(), text: "Kanton".into() }], children }
    }

    #[test]
    fn subdivisions_flatten_parents_first_sorted_without_accents() {
        let mut out = vec![];
        flatten(vec![r("CH-ZH", "Zürich", vec![r("CH-ZH-B", "Bülach", vec![]), r("CH-ZH-A", "Affoltern", vec![])]), r("CH-AG", "Aargau", vec![])], "de", 0, &mut out);
        let got: Vec<(&str, u8)> = out.iter().map(|x| (x.code.as_str(), x.depth)).collect();
        assert_eq!(got, [("CH-AG", 0), ("CH-ZH", 0), ("CH-ZH-A", 1), ("CH-ZH-B", 1)]);
        assert_eq!(out[0].category, "Kanton");
        assert!(fold("Österreich") < fold("Polen"));
    }
}
