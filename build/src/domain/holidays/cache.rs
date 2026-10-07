//! The `practical_holidays` answer cache (30 days) and its in-memory front.

use super::*;

/// The cached JSON for a key and whether it is younger than CACHE_DAYS.
pub(super) async fn cache_get(state: &AppState, country: &str, subdivision: &str, year: i32, kind: &str) -> AppResult<Option<(String, bool)>> {
    Ok(cache_aged(state, country, subdivision, year, kind).await?.map(|(j, hours)| (j, hours < CACHE_DAYS * 24)))
}

/// The cached JSON for a key and its age in hours.
pub(super) async fn cache_aged(state: &AppState, country: &str, subdivision: &str, year: i32, kind: &str) -> AppResult<Option<(String, i64)>> {
    Ok(cache_row(state, country, subdivision, year, kind).await?.map(|(j, fetched)| (j, age_hours(fetched))))
}

/// The cached JSON for a key and when it was fetched (None: unreadable stamp, i.e. stale).
pub(super) async fn cache_row(state: &AppState, country: &str, subdivision: &str, year: i32, kind: &str) -> AppResult<Option<(String, Option<chrono::NaiveDateTime>)>> {
    let row = sqlx::query("SELECT json, fetched_at FROM practical_holidays WHERE country = ? AND subdivision = ? AND year = ? AND kind = ?")
        .bind(country)
        .bind(subdivision)
        .bind(year)
        .bind(kind)
        .fetch_optional(&state.db)
        .await?;
    Ok(row.map(|r| {
        let fetched: String = r.get("fetched_at");
        (r.get("json"), chrono::NaiveDateTime::parse_from_str(&fetched, "%Y-%m-%d %H:%M:%S").ok())
    }))
}

pub(super) fn age_hours(fetched: Option<chrono::NaiveDateTime>) -> i64 {
    fetched.map(|f| (crate::clock::now().naive_utc() - f).num_hours()).unwrap_or(i64::MAX)
}

/// `get`'s rows of `practical_holidays`, parsed, so the views do not read and parse them per request.
/// `cache_put` (the only writer) drops a key; freshness is judged from `fetched_at` on every read, as
/// before. At most `MEM_MAX` keys (country × subdivision × year × kind-language).
#[derive(Default)]
pub struct MemCache(pub(super) std::sync::Mutex<HashMap<(String, String, i32, String), (std::sync::Arc<Vec<Holiday>>, Option<chrono::NaiveDateTime>)>>);

pub(super) const MEM_MAX: usize = 512;

#[cfg(test)]
pub async fn cache_put_for_test(state: &AppState, country: &str, subdivision: &str, year: i32, kind: &str, json: &str) {
    cache_put(state, country, subdivision, year, kind, json).await.unwrap();
}

pub(super) async fn cache_put(state: &AppState, country: &str, subdivision: &str, year: i32, kind: &str, json: &str) -> AppResult<()> {
    sqlx::query("INSERT OR REPLACE INTO practical_holidays (country, subdivision, year, kind, json, fetched_at) VALUES (?, ?, ?, ?, ?, CURRENT_TIMESTAMP)")
        .bind(country)
        .bind(subdivision)
        .bind(year)
        .bind(kind)
        .bind(json)
        .execute(&state.db)
        .await?;
    state.caches.holidays.0.lock().unwrap().remove(&(country.to_string(), subdivision.to_string(), year, kind.to_string()));
    Ok(())
}

pub(super) async fn get_json<T: serde::de::DeserializeOwned>(state: &AppState, url: &str) -> Result<T, String> {
    let resp = state.http.get(url).header("Accept", "application/json").timeout(TIMEOUT).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("{url}: HTTP {}", resp.status()));
    }
    resp.json().await.map_err(|e| format!("{url}: {e}"))
}
