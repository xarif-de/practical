//! Public and school holidays from the OpenHolidays API (plan D13), cached per
//! (country, subdivision, year, kind) in `xui_holidays` for 30 days.

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

/// Cached lookup; a failed fetch returns the stale cache or an empty list (never an error to the view).
pub async fn get(state: &AppState, kind: &str, country: &str, subdivision: &str, year: i32, lang: &str) -> AppResult<Vec<Holiday>> {
    if !state.config.holidays_enabled {
        return Ok(vec![]);
    }
    let row = sqlx::query("SELECT json, fetched_at FROM xui_holidays WHERE country = ? AND subdivision = ? AND year = ? AND kind = ?")
        .bind(country)
        .bind(subdivision)
        .bind(year)
        .bind(kind)
        .fetch_optional(&state.db)
        .await?;
    let cached: Option<(Vec<Holiday>, bool)> = row.map(|r| {
        let json: String = r.get("json");
        let fetched: String = r.get("fetched_at");
        let fresh = chrono::NaiveDateTime::parse_from_str(&fetched, "%Y-%m-%d %H:%M:%S").map(|f| (Utc::now().naive_utc() - f).num_days() < CACHE_DAYS).unwrap_or(false);
        (serde_json::from_str(&json).unwrap_or_default(), fresh)
    });
    if let Some((h, true)) = &cached {
        return Ok(h.clone());
    }
    match fetch(state, kind, country, subdivision, year, lang).await {
        Ok(h) => {
            sqlx::query("INSERT OR REPLACE INTO xui_holidays (country, subdivision, year, kind, json, fetched_at) VALUES (?, ?, ?, ?, ?, CURRENT_TIMESTAMP)")
                .bind(country)
                .bind(subdivision)
                .bind(year)
                .bind(kind)
                .bind(serde_json::to_string(&h).unwrap())
                .execute(&state.db)
                .await?;
            Ok(h)
        }
        Err(e) => {
            tracing::warn!("openholidays {kind} {country}/{subdivision} {year}: {e}");
            Ok(cached.map(|(h, _)| h).unwrap_or_default())
        }
    }
}

/// Is `d` a public holiday (used by the vacation counter, D14)?
pub fn is_holiday(hols: &[Holiday], d: NaiveDate) -> Option<&Holiday> {
    hols.iter().find(|h| h.kind == "public" && h.start <= d && d <= h.end)
}
