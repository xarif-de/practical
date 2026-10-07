//! Pool on RustiCal's SQLite file plus the UI's OWN schema (`practical_*` tables).
//!
//! Rules (plan D6): RustiCal's migrations are never run from here
//! (`create_db_pool(.., migrate=false)`); our migrations live in `practical_migrations`
//! (never `_sqlx_migrations`, or rustical's own migrator aborts on "unknown
//! migration"); RustiCal's tables are touched only through its store crates.

use sqlx::{Row, SqlitePool};
use tracing::info;

pub async fn open(db_path: &str) -> Result<SqlitePool, sqlx::Error> {
    rustical_store_sqlite::create_db_pool(&format!("sqlite://{db_path}"), false).await
}

/// "Has anything been written since I last looked?" for the background loops, at the cost of one
/// `PRAGMA data_version` (no table read). SQLite moves that value iff a commit by ANY OTHER
/// connection happened in between — the pool's, RustiCal's process — so the connection is taken out
/// of the pool for good (`detach`) and never writes. The value is per connection: a new one starts a
/// new series, hence the generation in the stamp — a reconnect never compares equal to before.
pub struct Watch {
    pool: SqlitePool,
    conn: Option<sqlx::SqliteConnection>,
    generation: u64,
}

impl Watch {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool, conn: None, generation: 0 }
    }

    /// The current stamp; `None` (= "assume changed") when the connection fails.
    pub async fn stamp(&mut self) -> Option<(u64, i64)> {
        if self.conn.is_none() {
            match self.pool.acquire().await {
                Ok(c) => {
                    self.conn = Some(c.detach());
                    self.generation += 1;
                }
                Err(e) => {
                    tracing::warn!("db watch: {e}");
                    return None;
                }
            }
        }
        let conn = self.conn.as_mut()?;
        match sqlx::query_scalar::<_, i64>("PRAGMA data_version").fetch_one(&mut *conn).await {
            Ok(v) => Some((self.generation, v)),
            Err(e) => {
                tracing::warn!("db watch: {e}");
                self.conn = None;
                None
            }
        }
    }
}

/// (version, sql) — append-only; never change what a shipped entry does. (2026-10-05, applied to
/// existing databases once by hand: table prefix xui_ → practical_; column user_settings.presets_json dropped.)
const MIGRATIONS: &[(i64, &str)] = &[
    (
        1,
        r#"
CREATE TABLE practical_trashed_groups (
    id TEXT PRIMARY KEY,
    displayname TEXT,
    members_json TEXT NOT NULL,
    calendars_json TEXT NOT NULL,
    addressbooks_json TEXT NOT NULL,
    deleted_by TEXT NOT NULL,
    deleted_at TEXT NOT NULL
);
CREATE TABLE practical_user_settings (
    principal TEXT PRIMARY KEY,
    lang TEXT,
    scheme TEXT,
    timezone TEXT,
    holidays_enabled INTEGER NOT NULL DEFAULT 1,
    school_holidays_enabled INTEGER NOT NULL DEFAULT 1,
    holiday_subdivision TEXT,
    vacation_days INTEGER,
    vacation_rule_json TEXT,
    contact_columns_json TEXT,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE practical_push_subscriptions (
    id TEXT PRIMARY KEY,
    principal TEXT NOT NULL,
    endpoint TEXT NOT NULL UNIQUE,
    p256dh TEXT NOT NULL,
    auth TEXT NOT NULL,
    user_agent TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX practical_push_subscriptions_principal ON practical_push_subscriptions (principal);
CREATE TABLE practical_alarms_sent (
    principal TEXT NOT NULL,
    cal_id TEXT NOT NULL,
    object_id TEXT NOT NULL,
    trigger_at TEXT NOT NULL,
    sent_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (principal, cal_id, object_id, trigger_at)
);
CREATE TABLE practical_holidays (
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
        "ALTER TABLE practical_user_settings ADD COLUMN contact_list_fields_json TEXT;",
    ),
    (
        3,
        // colour per address book — RustiCal's addressbooks table carries none (calendars do)
        "CREATE TABLE practical_addressbook_colors (principal TEXT NOT NULL, id TEXT NOT NULL, color TEXT NOT NULL, PRIMARY KEY (principal, id));",
    ),
    (
        4,
        // reminder presets of the event editor, per user (NULL = the built-in defaults)
        "ALTER TABLE practical_user_settings ADD COLUMN alarm_presets_json TEXT;",
    ),
    (
        5,
        // snap points of the reminder strip, per user (NULL = the built-in list)
        "ALTER TABLE practical_user_settings ADD COLUMN alarm_snaps_json TEXT;",
    ),
    (
        6,
        // per-user category rules: defaults and fixed values a category applies to an event (JSON, see domain::catrules)
        "CREATE TABLE practical_category_rules (principal TEXT NOT NULL, category TEXT NOT NULL, rule_json TEXT NOT NULL, updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, PRIMARY KEY (principal, category));",
    ),
    (
        7,
        // a rule belongs to events or to tasks; everything stored so far was written by the event editor
        // the primary key has to grow, and SQLite cannot alter one — rebuild and copy;
        // everything stored so far was written by the event editor
        "CREATE TABLE practical_category_rules_new (principal TEXT NOT NULL, scope TEXT NOT NULL DEFAULT 'event', category TEXT NOT NULL, rule_json TEXT NOT NULL, updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, PRIMARY KEY (principal, scope, category));\n\
         INSERT INTO practical_category_rules_new (principal, scope, category, rule_json, updated_at) SELECT principal, 'event', category, rule_json, updated_at FROM practical_category_rules;\n\
         DROP TABLE practical_category_rules;\n\
         ALTER TABLE practical_category_rules_new RENAME TO practical_category_rules;",
    ),
    (
        8,
        // the columns of the task table, per user (NULL = routes::tasks::DEFAULT_COLUMNS)
        "ALTER TABLE practical_user_settings ADD COLUMN task_columns_json TEXT;",
    ),
    (
        9,
        // the holiday country, per user (NULL = config HOLIDAYS_COUNTRY)
        "ALTER TABLE practical_user_settings ADD COLUMN holiday_country TEXT;",
    ),
    (
        10,
        // the user's colours for public holidays and school holidays (NULL = the theme's)
        "ALTER TABLE practical_user_settings ADD COLUMN holiday_color TEXT;
         ALTER TABLE practical_user_settings ADD COLUMN school_color TEXT;",
    ),
    (
        11,
        // the regions of public holidays and of school holidays, each a comma list of region codes
        // ("DE-SN,DE-BY"; "" = none). NULL = the single region of holiday_country/holiday_subdivision
        // (prefs::Prefs::regions), so a region chosen before stays as the list's only entry.
        "ALTER TABLE practical_user_settings ADD COLUMN public_regions TEXT;
         ALTER TABLE practical_user_settings ADD COLUMN school_regions TEXT;",
    ),
    (
        12,
        // the top-bar search's history (domain::search_history): one row per query (q_norm = folded),
        // with the result last opened from it; kept for search_history_days (NULL = 90, 0 = not recorded, -1 = unlimited)
        "CREATE TABLE practical_search_history (id INTEGER PRIMARY KEY AUTOINCREMENT, principal TEXT NOT NULL, q TEXT NOT NULL, q_norm TEXT NOT NULL, target_json TEXT, used_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, uses INTEGER NOT NULL DEFAULT 1, UNIQUE (principal, q_norm));
         ALTER TABLE practical_user_settings ADD COLUMN search_history_days INTEGER;",
    ),
    (
        13,
        // the change history (domain::history): changes with their items, the versions before/after as
        // content-addressed blobs, the last version seen of every object and how far each collection's
        // change log is read
        "CREATE TABLE practical_history_blobs (hash TEXT PRIMARY KEY, body TEXT NOT NULL);
         CREATE TABLE practical_history_changes (id INTEGER PRIMARY KEY AUTOINCREMENT, at TEXT NOT NULL, src TEXT NOT NULL, who TEXT, device TEXT, label TEXT,
             reverts INTEGER, undone_by INTEGER, inactive_by INTEGER, tech INTEGER NOT NULL DEFAULT 0);
         CREATE INDEX practical_history_changes_at ON practical_history_changes (at);
         CREATE TABLE practical_history_items (change_id INTEGER NOT NULL, idx INTEGER NOT NULL, store TEXT NOT NULL, principal TEXT NOT NULL, coll TEXT NOT NULL,
             object_id TEXT NOT NULL, uid TEXT NOT NULL, kind TEXT NOT NULL, act TEXT NOT NULL, from_principal TEXT, from_coll TEXT, from_object TEXT,
             before_hash TEXT, after_hash TEXT, title TEXT NOT NULL, attrs TEXT NOT NULL, PRIMARY KEY (change_id, idx));
         CREATE INDEX practical_history_items_uid ON practical_history_items (store, uid);
         CREATE TABLE practical_history_heads (store TEXT NOT NULL, principal TEXT NOT NULL, coll TEXT NOT NULL, object_id TEXT NOT NULL, uid TEXT NOT NULL, hash TEXT NOT NULL,
             PRIMARY KEY (store, principal, coll, object_id));
         CREATE INDEX practical_history_heads_uid ON practical_history_heads (store, principal, coll, uid);
         CREATE TABLE practical_history_cursors (store TEXT NOT NULL, principal TEXT NOT NULL, coll TEXT NOT NULL, synctoken INTEGER NOT NULL, PRIMARY KEY (store, principal, coll));",
    ),
    (
        14,
        // the login sessions (sessions::DbStore), so a restart logs nobody out: id = SHA-256 of the cookie's
        // session id, data = JSON, expiry = Unix seconds (the hourly purge deletes by it)
        "CREATE TABLE practical_sessions (id TEXT PRIMARY KEY, data TEXT NOT NULL, expiry INTEGER NOT NULL);
         CREATE INDEX practical_sessions_expiry ON practical_sessions (expiry);",
    ),
];

pub async fn migrate(db: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS practical_migrations (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP)",
    )
    .execute(db)
    .await?;
    let applied: Vec<i64> = sqlx::query("SELECT version FROM practical_migrations ORDER BY version")
        .fetch_all(db)
        .await?
        .iter()
        .map(|r| r.get::<i64, _>("version"))
        .collect();
    for (version, sql) in MIGRATIONS {
        if applied.contains(version) {
            continue;
        }
        info!("applying migration {version}");
        let mut tx = db.begin_with(rustical_store_sqlite::BEGIN_IMMEDIATE).await?;
        sqlx::raw_sql(*sql).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO practical_migrations (version) VALUES (?)")
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

/// The `practical_*` schema as the code expects it: every table and column a replay of `MIGRATIONS` on an
/// empty database creates, and no migration newer than this binary. A start against a missing, unexpected or
/// retyped column or a newer migration is refused (`Err`) — on 2026-10-05 a hand-applied change (table
/// prefix, a dropped column) left a schema the migrations did not describe, and that showed only as 500s at
/// runtime while /healthz stayed green. Other differences (primary key, NOT NULL, default) come back as
/// `Ok(drift)` for the log: the copy of production of 2026-10-05 has one (practical_category_rules' key is
/// (principal, category) plus a unique index with scope, not migration 7's (principal, scope, category)),
/// and refusing it would stop the service.
pub async fn check_schema(db: &SqlitePool) -> Result<Vec<String>, String> {
    let newest = MIGRATIONS.last().map_or(0, |m| m.0);
    let applied: Option<i64> = sqlx::query_scalar("SELECT max(version) FROM practical_migrations").fetch_one(db).await.map_err(|e| e.to_string())?;
    if let Some(v) = applied.filter(|v| *v > newest) {
        return Err(format!("the database has practical migration {v}, this binary knows up to {newest} (an older image on a newer database?) — refusing to start"));
    }
    let mut expected = <sqlx::SqliteConnection as sqlx::Connection>::connect("sqlite::memory:").await.map_err(|e| e.to_string())?;
    for (_, sql) in MIGRATIONS {
        sqlx::raw_sql(*sql).execute(&mut expected).await.map_err(|e| e.to_string())?;
    }
    let want = practical_columns(&mut expected).await.map_err(|e| e.to_string())?;
    let mut live = db.acquire().await.map_err(|e| e.to_string())?;
    let have = practical_columns(&mut *live).await.map_err(|e| e.to_string())?;
    let (mut fatal, mut drift) = (vec![], vec![]);
    for (col, def) in &want {
        match have.get(col) {
            None => fatal.push(format!("missing {}.{}", col.0, col.1)),
            Some(d) if d.split(' ').next() != def.split(' ').next() => fatal.push(format!("{}.{} is {d}, expected {def}", col.0, col.1)),
            Some(d) if d != def => drift.push(format!("{}.{} is {d}, expected {def}", col.0, col.1)),
            Some(_) => {}
        }
    }
    fatal.extend(have.keys().filter(|c| !want.contains_key(*c)).map(|c| format!("unexpected {}.{}", c.0, c.1)));
    if fatal.is_empty() { Ok(drift) } else { Err(format!("the practical_* schema differs from its migrations — refusing to start: {}", fatal.join("; "))) }
}

/// (table, column) → "type notnull=… default=… pk=…" of every `practical_*` table but the migration log.
async fn practical_columns(conn: &mut sqlx::SqliteConnection) -> Result<std::collections::BTreeMap<(String, String), String>, sqlx::Error> {
    let tables: Vec<String> = sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type = 'table' AND name LIKE 'practical\\_%' ESCAPE '\\' AND name != 'practical_migrations'").fetch_all(&mut *conn).await?;
    let mut out = std::collections::BTreeMap::new();
    for t in tables {
        for r in sqlx::query("SELECT name, type, \"notnull\", dflt_value, pk FROM pragma_table_info(?)").bind(&t).fetch_all(&mut *conn).await? {
            let def = format!("{} notnull={} default={:?} pk={}", r.get::<String, _>("type"), r.get::<i64, _>("notnull"), r.get::<Option<String>, _>("dflt_value"), r.get::<i64, _>("pk"));
            out.insert((t.clone(), r.get::<String, _>("name")), def);
        }
    }
    Ok(out)
}
