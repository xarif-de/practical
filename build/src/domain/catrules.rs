//! Category rules: per user and category, values a category puts on an event — as a *default*
//! (prefilled, editable) or a *lock* (fixed in this UI). Stored as JSON in `xui_category_rules`;
//! the editor applies them client-side in the order of the event's categories (the first wins).

use crate::domain::events::{self, Alarm, Link};
use crate::error::{AppError, AppResult};
use sqlx::{Row, SqlitePool};
use std::collections::BTreeMap;

/// off | default | lock
fn valid_mode(m: &str) -> bool {
    matches!(m, "off" | "default" | "lock")
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Part<T> {
    #[serde(default = "off")]
    pub mode: String,
    #[serde(flatten)]
    pub v: T,
}
fn off() -> String {
    "off".into()
}
impl<T: Default> Default for Part<T> {
    fn default() -> Self {
        Part { mode: off(), v: T::default() }
    }
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct WhenRule {
    #[serde(default)]
    pub all_day: Option<bool>,
    /// `principal/calendar id`; applies to new events only (an existing event cannot move)
    #[serde(default)]
    pub calendar: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
}
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct WhereRule {
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub geo: Option<(f64, f64)>,
}
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct AlarmRule {
    #[serde(default)]
    pub alarms: Vec<Alarm>,
}
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct RepeatRule {
    #[serde(default)]
    pub rrule: String,
}
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct StatusRule {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub transp: String,
    #[serde(default)]
    pub class: String,
    #[serde(default)]
    pub priority: Option<i64>,
}
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct LinksRule {
    #[serde(default)]
    pub links: Vec<Link>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Rule {
    #[serde(default)]
    pub when: Part<WhenRule>,
    #[serde(default, rename = "where")]
    pub where_: Part<WhereRule>,
    #[serde(default)]
    pub alarm: Part<AlarmRule>,
    #[serde(default)]
    pub repeat: Part<RepeatRule>,
    #[serde(default)]
    pub status: Part<StatusRule>,
    #[serde(default)]
    pub links: Part<LinksRule>,
}

pub fn validate(r: &Rule) -> AppResult<()> {
    for m in [&r.when.mode, &r.where_.mode, &r.alarm.mode, &r.repeat.mode, &r.status.mode, &r.links.mode] {
        if !valid_mode(m) {
            return Err(AppError::BadRequest(format!("unknown rule mode {m}")));
        }
    }
    if let Some(c) = &r.when.v.color
        && !c.is_empty()
        && !events::valid_color(c)
    {
        return Err(AppError::BadRequest("invalid colour".into()));
    }
    if !r.repeat.v.rrule.trim().is_empty() {
        events::validate_rrule(r.repeat.v.rrule.trim())?;
    }
    if r.alarm.v.alarms.len() > 20 || r.links.v.links.len() > 50 {
        return Err(AppError::BadRequest("too many rule entries".into()));
    }
    if r.links.v.links.iter().any(|l| l.embedded || l.data.is_some()) {
        return Err(AppError::BadRequest("a rule cannot embed files".into()));
    }
    Ok(())
}

pub async fn all(db: &SqlitePool, principal: &str) -> AppResult<BTreeMap<String, Rule>> {
    let rows = sqlx::query("SELECT category, rule_json FROM xui_category_rules WHERE principal = ? ORDER BY category").bind(principal).fetch_all(db).await?;
    let mut out = BTreeMap::new();
    for r in rows {
        let name: String = r.get("category");
        if let Ok(rule) = serde_json::from_str::<Rule>(&r.get::<String, _>("rule_json")) {
            out.insert(name, rule);
        }
    }
    Ok(out)
}

pub async fn put(db: &SqlitePool, principal: &str, category: &str, rule: &Rule) -> AppResult<()> {
    let name = category.trim();
    if name.is_empty() || name.len() > 80 || name.contains([',', '\n', '\r']) {
        return Err(AppError::BadRequest("invalid category name".into()));
    }
    validate(rule)?;
    sqlx::query("INSERT INTO xui_category_rules (principal, category, rule_json, updated_at) VALUES (?, ?, ?, CURRENT_TIMESTAMP) ON CONFLICT(principal, category) DO UPDATE SET rule_json = excluded.rule_json, updated_at = CURRENT_TIMESTAMP")
        .bind(principal).bind(name).bind(serde_json::to_string(rule).unwrap()).execute(db).await?;
    Ok(())
}

pub async fn delete(db: &SqlitePool, principal: &str, category: &str) -> AppResult<()> {
    sqlx::query("DELETE FROM xui_category_rules WHERE principal = ? AND category = ?").bind(principal).bind(category.trim()).execute(db).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_json_shape_and_validation() {
        let r: Rule = serde_json::from_str(r##"{"when":{"mode":"lock","all_day":true,"color":"#3478f6"},"status":{"mode":"default","transp":"TRANSPARENT"},"repeat":{"mode":"off","rrule":""}}"##).unwrap();
        assert_eq!((r.when.mode.as_str(), r.when.v.all_day, r.status.v.transp.as_str(), r.alarm.mode.as_str()), ("lock", Some(true), "TRANSPARENT", "off"));
        validate(&r).unwrap();
        let bad: Rule = serde_json::from_str(r#"{"when":{"mode":"maybe"}}"#).unwrap();
        assert!(validate(&bad).is_err());
        let bad: Rule = serde_json::from_str(r#"{"repeat":{"mode":"lock","rrule":"FREQ=NEVER"}}"#).unwrap();
        assert!(validate(&bad).is_err());
        let out = serde_json::to_value(&r).unwrap();
        assert_eq!(out["where"]["mode"], "off");
        assert_eq!(out["when"]["all_day"], true);
    }
}
