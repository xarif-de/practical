//! Per-user settings in `xui_user_settings`.

use sqlx::{Row, SqlitePool};

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Prefs {
    pub lang: Option<String>,
    pub scheme: Option<String>,
    pub timezone: Option<String>,
    pub holidays_enabled: bool,
    pub school_holidays_enabled: bool,
    pub holiday_subdivision: Option<String>,
    pub vacation_days: Option<i64>,
    pub vacation_rule_json: Option<String>,
    pub presets_json: Option<String>,
    pub contact_columns_json: Option<String>,
}

impl Prefs {
    pub fn defaults() -> Self {
        Self { holidays_enabled: true, school_holidays_enabled: true, ..Default::default() }
    }
}

pub async fn get(db: &SqlitePool, principal: &str) -> Result<Prefs, sqlx::Error> {
    let row = sqlx::query("SELECT * FROM xui_user_settings WHERE principal = ?")
        .bind(principal)
        .fetch_optional(db)
        .await?;
    Ok(match row {
        None => Prefs::defaults(),
        Some(r) => Prefs {
            lang: r.get("lang"),
            scheme: r.get("scheme"),
            timezone: r.get("timezone"),
            holidays_enabled: r.get::<i64, _>("holidays_enabled") != 0,
            school_holidays_enabled: r.get::<i64, _>("school_holidays_enabled") != 0,
            holiday_subdivision: r.get("holiday_subdivision"),
            vacation_days: r.get("vacation_days"),
            vacation_rule_json: r.get("vacation_rule_json"),
            presets_json: r.get("presets_json"),
            contact_columns_json: r.get("contact_columns_json"),
        },
    })
}

pub async fn get_lang(db: &SqlitePool, principal: &str) -> Result<Option<String>, sqlx::Error> {
    Ok(sqlx::query("SELECT lang FROM xui_user_settings WHERE principal = ?")
        .bind(principal)
        .fetch_optional(db)
        .await?
        .and_then(|r| r.get::<Option<String>, _>("lang")))
}

pub async fn set(db: &SqlitePool, principal: &str, p: &Prefs) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO xui_user_settings (principal, lang, scheme, timezone, holidays_enabled, school_holidays_enabled, holiday_subdivision, vacation_days, vacation_rule_json, presets_json, contact_columns_json, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP)
         ON CONFLICT(principal) DO UPDATE SET lang=excluded.lang, scheme=excluded.scheme, timezone=excluded.timezone,
           holidays_enabled=excluded.holidays_enabled, school_holidays_enabled=excluded.school_holidays_enabled,
           holiday_subdivision=excluded.holiday_subdivision, vacation_days=excluded.vacation_days,
           vacation_rule_json=excluded.vacation_rule_json, presets_json=excluded.presets_json,
           contact_columns_json=excluded.contact_columns_json, updated_at=CURRENT_TIMESTAMP",
    )
    .bind(principal)
    .bind(&p.lang)
    .bind(&p.scheme)
    .bind(&p.timezone)
    .bind(i64::from(p.holidays_enabled))
    .bind(i64::from(p.school_holidays_enabled))
    .bind(&p.holiday_subdivision)
    .bind(p.vacation_days)
    .bind(&p.vacation_rule_json)
    .bind(&p.presets_json)
    .bind(&p.contact_columns_json)
    .execute(db)
    .await?;
    Ok(())
}
