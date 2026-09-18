//! Category rules: per user and category, values a category puts on an event — each as a *default*
//! ("Vorlage": prefilled, editable) or a *lock* ("Vorgabe": fixed in this UI). Whole cards (where,
//! repeat) carry one mode; the when and status cards carry a mode per setting, reminders and links a
//! mode per entry. Stored as JSON in `xui_category_rules`; the editor applies them client-side in the
//! order of the event's categories (the first wins per setting).

use crate::domain::events::{self, Alarm, Link};
use crate::error::{AppError, AppResult};
use sqlx::{Row, SqlitePool};
use std::collections::BTreeMap;

fn part_mode(m: &str) -> bool {
    matches!(m, "off" | "default" | "lock")
}
fn setting_mode(m: &str) -> bool {
    matches!(m, "default" | "lock")
}

/// A whole card: off | default | lock.
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

/// One setting: default | lock plus its value.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Setting<T> {
    pub mode: String,
    pub value: T,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct WhenRule {
    #[serde(default)]
    pub all_day: Option<Setting<bool>>,
    /// `principal/calendar id`; applies to new events only (an existing event cannot move)
    #[serde(default)]
    pub calendar: Option<Setting<String>>,
    #[serde(default)]
    pub color: Option<Setting<String>>,
}
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct WhereRule {
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub geo: Option<(f64, f64)>,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RuleAlarm {
    pub mode: String,
    #[serde(flatten)]
    pub alarm: Alarm,
}
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct AlarmRule {
    #[serde(default)]
    pub alarms: Vec<RuleAlarm>,
}
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct RepeatRule {
    #[serde(default)]
    pub rrule: String,
}
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct StatusRule {
    #[serde(default)]
    pub status: Option<Setting<String>>,
    #[serde(default)]
    pub transp: Option<Setting<String>>,
    #[serde(default)]
    pub class: Option<Setting<String>>,
    #[serde(default)]
    pub priority: Option<Setting<i64>>,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RuleLink {
    pub mode: String,
    #[serde(flatten)]
    pub link: Link,
}
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct LinksRule {
    #[serde(default)]
    pub links: Vec<RuleLink>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Rule {
    #[serde(default)]
    pub when: WhenRule,
    #[serde(default, rename = "where")]
    pub where_: Part<WhereRule>,
    #[serde(default)]
    pub alarm: AlarmRule,
    #[serde(default)]
    pub repeat: Part<RepeatRule>,
    #[serde(default)]
    pub status: StatusRule,
    #[serde(default)]
    pub links: LinksRule,
}

impl Rule {
    /// true when the rule sets anything at all
    pub fn is_empty(&self) -> bool {
        self.when.all_day.is_none() && self.when.calendar.is_none() && self.when.color.is_none() && self.where_.mode == "off" && self.alarm.alarms.is_empty() && self.repeat.mode == "off" && self.status.status.is_none() && self.status.transp.is_none() && self.status.class.is_none() && self.status.priority.is_none() && self.links.links.is_empty()
    }
}

pub fn validate(r: &Rule) -> AppResult<()> {
    let bad = |m: &str| AppError::BadRequest(format!("unknown rule mode {m}"));
    for m in [&r.where_.mode, &r.repeat.mode] {
        if !part_mode(m) {
            return Err(bad(m));
        }
    }
    let modes: Vec<&String> = r.when.all_day.iter().map(|s| &s.mode).chain(r.when.calendar.iter().map(|s| &s.mode)).chain(r.when.color.iter().map(|s| &s.mode)).chain(r.status.status.iter().map(|s| &s.mode)).chain(r.status.transp.iter().map(|s| &s.mode)).chain(r.status.class.iter().map(|s| &s.mode)).chain(r.status.priority.iter().map(|s| &s.mode)).chain(r.alarm.alarms.iter().map(|a| &a.mode)).chain(r.links.links.iter().map(|l| &l.mode)).collect();
    if let Some(m) = modes.iter().find(|m| !setting_mode(m)) {
        return Err(bad(m));
    }
    if let Some(c) = &r.when.color
        && !events::valid_color(&c.value)
    {
        return Err(AppError::BadRequest("invalid colour".into()));
    }
    if r.repeat.mode != "off" && !r.repeat.v.rrule.trim().is_empty() {
        events::validate_rrule(r.repeat.v.rrule.trim())?;
    }
    if r.alarm.alarms.len() > 20 || r.links.links.len() > 50 {
        return Err(AppError::BadRequest("too many rule entries".into()));
    }
    if r.links.links.iter().any(|l| l.link.embedded || l.link.data.is_some()) {
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
        let r: Rule = serde_json::from_str(r##"{"when":{"all_day":{"mode":"lock","value":true},"color":{"mode":"default","value":"#3478f6"}},"status":{"transp":{"mode":"default","value":"TRANSPARENT"}},"repeat":{"mode":"off","rrule":""},"alarm":{"alarms":[{"mode":"lock","minutes":30,"action":"DISPLAY"}]},"links":{"links":[{"mode":"default","kind":"url","uri":"https://a.example.org/"}]}}"##).unwrap();
        assert_eq!((r.when.all_day.as_ref().map(|s| (s.mode.as_str(), s.value)), r.status.transp.as_ref().map(|s| s.value.as_str()), r.where_.mode.as_str(), r.alarm.alarms[0].alarm.minutes, r.links.links[0].link.uri.as_str()), (Some(("lock", true)), Some("TRANSPARENT"), "off", 30, "https://a.example.org/"));
        validate(&r).unwrap();
        assert!(!r.is_empty() && Rule::default().is_empty());
        let out = serde_json::to_value(&r).unwrap();
        assert_eq!(out["alarm"]["alarms"][0]["mode"], "lock");
        assert_eq!(out["alarm"]["alarms"][0]["minutes"], 30);
        assert_eq!(out["when"]["calendar"], serde_json::Value::Null);
        let bad: Rule = serde_json::from_str(r#"{"where":{"mode":"maybe"}}"#).unwrap();
        assert!(validate(&bad).is_err());
        let bad: Rule = serde_json::from_str(r#"{"status":{"status":{"mode":"off","value":"CONFIRMED"}}}"#).unwrap();
        assert!(validate(&bad).is_err());
        let bad: Rule = serde_json::from_str(r#"{"repeat":{"mode":"lock","rrule":"FREQ=NEVER"}}"#).unwrap();
        assert!(validate(&bad).is_err());
    }
}
