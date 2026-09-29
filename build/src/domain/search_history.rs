//! The top-bar search's history (`xui_search_history`): queries the user finished — opened a result
//! from, or pressed Enter on — with the result last opened (`target`, the client's JSON, so Tab can
//! go straight there). One row per folded query; kept for the user's `Prefs::history_days`.

use crate::domain::search::fold;
use sqlx::{Row, SqlitePool};

/// entries kept per user whatever the retention
const MAX: i64 = 300;

#[derive(Debug, serde::Serialize)]
pub struct Entry {
    pub id: i64,
    pub q: String,
    pub target: Option<serde_json::Value>,
    pub used_at: String,
    pub uses: i64,
}

async fn prune(db: &SqlitePool, principal: &str, days: i64) -> Result<(), sqlx::Error> {
    if days > 0 {
        sqlx::query("DELETE FROM xui_search_history WHERE principal = ? AND used_at < datetime('now', ?)").bind(principal).bind(format!("-{days} days")).execute(db).await?;
    }
    sqlx::query("DELETE FROM xui_search_history WHERE principal = ? AND id NOT IN (SELECT id FROM xui_search_history WHERE principal = ? ORDER BY used_at DESC, id DESC LIMIT ?)")
        .bind(principal)
        .bind(principal)
        .bind(MAX)
        .execute(db)
        .await?;
    Ok(())
}

/// Newest first; expired entries go first.
pub async fn list(db: &SqlitePool, principal: &str, days: i64) -> Result<Vec<Entry>, sqlx::Error> {
    prune(db, principal, days).await?;
    let rows = sqlx::query("SELECT id, q, target_json, used_at, uses FROM xui_search_history WHERE principal = ? ORDER BY used_at DESC, id DESC").bind(principal).fetch_all(db).await?;
    Ok(rows
        .iter()
        .map(|r| Entry {
            id: r.get("id"),
            q: r.get("q"),
            target: r.get::<Option<String>, _>("target_json").and_then(|t| serde_json::from_str(&t).ok()),
            used_at: r.get("used_at"),
            uses: r.get("uses"),
        })
        .collect())
}

/// A finished query; `days` 0 records nothing. A later use of the same query updates it
/// (a result opened now replaces the one kept, Enter alone keeps it).
pub async fn record(db: &SqlitePool, principal: &str, q: &str, target: Option<&serde_json::Value>, days: i64) -> Result<(), sqlx::Error> {
    let q = q.trim();
    let norm = fold(q);
    if days == 0 || norm.chars().count() < 2 {
        return Ok(());
    }
    let target = target.map(|t| t.to_string());
    sqlx::query(
        "INSERT INTO xui_search_history (principal, q, q_norm, target_json, used_at) VALUES (?, ?, ?, ?, datetime('now'))
         ON CONFLICT (principal, q_norm) DO UPDATE SET q = excluded.q, target_json = COALESCE(excluded.target_json, target_json), used_at = excluded.used_at, uses = uses + 1",
    )
    .bind(principal)
    .bind(q)
    .bind(&norm)
    .bind(target)
    .execute(db)
    .await?;
    prune(db, principal, days).await
}

pub async fn remove(db: &SqlitePool, principal: &str, id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM xui_search_history WHERE principal = ? AND id = ?").bind(principal).bind(id).execute(db).await?;
    Ok(())
}

pub async fn clear(db: &SqlitePool, principal: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM xui_search_history WHERE principal = ?").bind(principal).execute(db).await?;
    Ok(())
}
