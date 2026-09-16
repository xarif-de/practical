//! Pool on RustiCal's SQLite file plus the UI's OWN schema (`xui_*` tables).
//!
//! Rules (plan D6): RustiCal's migrations are never run from here
//! (`create_db_pool(.., migrate=false)`); our migrations live in `xui_migrations`
//! (never `_sqlx_migrations`, or rustical's own migrator aborts on "unknown
//! migration"); RustiCal's tables are touched only through its store crates.

use sqlx::{Row, SqlitePool};
use tracing::info;

pub async fn open(db_path: &str) -> Result<SqlitePool, sqlx::Error> {
    rustical_store_sqlite::create_db_pool(&format!("sqlite://{db_path}"), false).await
}

/// (version, sql) — append-only; never edit a shipped entry.
const MIGRATIONS: &[(i64, &str)] = &[
    (
        1,
        r#"
CREATE TABLE xui_trashed_groups (
    id TEXT PRIMARY KEY,
    displayname TEXT,
    members_json TEXT NOT NULL,
    calendars_json TEXT NOT NULL,
    addressbooks_json TEXT NOT NULL,
    deleted_by TEXT NOT NULL,
    deleted_at TEXT NOT NULL
);
CREATE TABLE xui_user_settings (
    principal TEXT PRIMARY KEY,
    lang TEXT,
    scheme TEXT,
    timezone TEXT,
    holidays_enabled INTEGER NOT NULL DEFAULT 1,
    school_holidays_enabled INTEGER NOT NULL DEFAULT 1,
    holiday_subdivision TEXT,
    vacation_days INTEGER,
    vacation_rule_json TEXT,
    presets_json TEXT,
    contact_columns_json TEXT,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE xui_push_subscriptions (
    id TEXT PRIMARY KEY,
    principal TEXT NOT NULL,
    endpoint TEXT NOT NULL UNIQUE,
    p256dh TEXT NOT NULL,
    auth TEXT NOT NULL,
    user_agent TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX xui_push_subscriptions_principal ON xui_push_subscriptions (principal);
CREATE TABLE xui_alarms_sent (
    principal TEXT NOT NULL,
    cal_id TEXT NOT NULL,
    object_id TEXT NOT NULL,
    trigger_at TEXT NOT NULL,
    sent_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (principal, cal_id, object_id, trigger_at)
);
CREATE TABLE xui_holidays (
    country TEXT NOT NULL,
    subdivision TEXT NOT NULL,
    year INTEGER NOT NULL,
    kind TEXT NOT NULL,
    json TEXT NOT NULL,
    fetched_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (country, subdivision, year, kind)
);
"#,
    ),
    (
        2,
        // fields shown per entry in the contacts LIST view (the table has contact_columns_json)
        "ALTER TABLE xui_user_settings ADD COLUMN contact_list_fields_json TEXT;",
    ),
];

pub async fn migrate(db: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS xui_migrations (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP)",
    )
    .execute(db)
    .await?;
    let applied: Vec<i64> = sqlx::query("SELECT version FROM xui_migrations ORDER BY version")
        .fetch_all(db)
        .await?
        .iter()
        .map(|r| r.get::<i64, _>("version"))
        .collect();
    for (version, sql) in MIGRATIONS {
        if applied.contains(version) {
            continue;
        }
        info!("applying xui migration {version}");
        let mut tx = db.begin_with(rustical_store_sqlite::BEGIN_IMMEDIATE).await?;
        sqlx::raw_sql(*sql).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO xui_migrations (version) VALUES (?)")
            .bind(version)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
    }
    Ok(())
}

/// Sanity check that this is a RustiCal database (its migrator has run at least once).
pub async fn rustical_schema_present(db: &SqlitePool) -> Result<bool, sqlx::Error> {
    let n: i64 = sqlx::query("SELECT count(*) AS n FROM sqlite_master WHERE type='table' AND name IN ('principals','calendars','addressbooks','_sqlx_migrations')")
        .fetch_one(db)
        .await?
        .get("n");
    Ok(n == 4)
}
