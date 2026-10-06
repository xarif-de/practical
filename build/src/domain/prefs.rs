//! Per-user settings in `practical_user_settings`.

use sqlx::{Row, SqlitePool};

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Prefs {
    pub lang: Option<String>,
    pub scheme: Option<String>,
    pub timezone: Option<String>,
    pub holidays_enabled: bool,
    pub school_holidays_enabled: bool,
    /// the single region before region lists (read only, as the lists' fallback): ISO country
    /// (NULL = config); "" in `holiday_subdivision` = the whole country
    pub holiday_country: Option<String>,
    pub holiday_subdivision: Option<String>,
    /// regions of public / school holidays: comma lists of region codes ("" = none; NULL = `region`)
    pub public_regions: Option<String>,
    pub school_regions: Option<String>,
    /// colours of public holidays / school holidays in the views (`#rrggbb`; NULL = the theme's)
    pub holiday_color: Option<String>,
    pub school_color: Option<String>,
    pub vacation_days: Option<i64>,
    pub vacation_rule_json: Option<String>,
    pub contact_columns_json: Option<String>,
    pub contact_list_fields_json: Option<String>,
    /// reminder presets of the event editor (`Vec<events::AlarmPreset>` as JSON)
    pub alarm_presets_json: Option<String>,
    /// snap points of the reminder strip (`events::AlarmSnaps` as JSON)
    pub alarm_snaps_json: Option<String>,
    /// columns of the task table (JSON array of `routes::tasks::ALL_COLUMNS` names)
    pub task_columns_json: Option<String>,
    /// days the search history is kept (NULL = 90, 0 = not recorded, -1 = unlimited; `history_days`)
    pub search_history_days: Option<i64>,
}

/// The choices of the search history's retention (days; 0 = off, -1 = unlimited).
pub const HISTORY_DAYS: [i64; 6] = [0, 7, 30, 90, 365, -1];

impl Prefs {
    pub fn history_days(&self) -> i64 {
        self.search_history_days.filter(|d| HISTORY_DAYS.contains(d)).unwrap_or(90)
    }
    pub fn defaults() -> Self {
        Self { holidays_enabled: true, school_holidays_enabled: true, ..Default::default() }
    }
    /// `:root` custom properties for the user's holiday colours ("" when both are the theme's)
    pub fn holiday_css(&self) -> String {
        let mut v = String::new();
        if let Some(c) = self.holiday_color.as_deref().filter(|c| is_hex_color(c)) {
            v.push_str(&format!("--hol:{c};"));
        }
        if let Some(c) = self.school_color.as_deref().filter(|c| is_hex_color(c)) {
            v.push_str(&format!("--fer:{c};"));
        }
        if v.is_empty() { v } else { format!(":root{{{v}}}") }
    }
    /// The single region of before the lists: the user's choice, else the configured default
    /// (the default subdivision only belongs to the default country).
    fn region(&self, cfg: &crate::config::Config) -> (String, String) {
        let country = self.holiday_country.clone().unwrap_or_else(|| cfg.holidays_country.clone());
        let sub = self.holiday_subdivision.clone().unwrap_or_else(|| if country == cfg.holidays_country { cfg.holidays_subdivision.clone() } else { String::new() });
        (country, sub)
    }
    /// The regions of public (`school` false) or school holidays, in the user's order.
    pub fn regions(&self, cfg: &crate::config::Config, school: bool) -> Vec<String> {
        match if school { &self.school_regions } else { &self.public_regions } {
            Some(list) => list.split(',').filter(|c| !c.is_empty()).map(str::to_string).collect(),
            None => {
                let (country, sub) = self.region(cfg);
                let one = if sub.is_empty() { country } else { sub };
                if one.is_empty() { vec![] } else { vec![one] }
            }
        }
    }
    /// The user's own region: the first of the public holidays' list (vacation days and bridge
    /// days count by it), else of the school holidays'.
    pub fn own_region(&self, cfg: &crate::config::Config) -> Option<String> {
        self.regions(cfg, false).into_iter().next().or_else(|| self.regions(cfg, true).into_iter().next())
    }
    /// The user's home country (address search prefers it): the own region's, else the configured one.
    pub fn home_country(&self, cfg: &crate::config::Config) -> String {
        self.own_region(cfg).map(|r| crate::domain::holidays::split(&r).0.to_string()).unwrap_or_else(|| cfg.holidays_country.clone())
    }
}

/// The columns are named, never `*`: sqlx keeps a prepared statement's column list per connection,
/// and after a column was dropped from the live table `SELECT *` read past the end of the shorter
/// row — the panic that ended the push reminder loop on 2026-10-05 (perf-baseline.md §0).
pub async fn get(db: &SqlitePool, principal: &str) -> Result<Prefs, sqlx::Error> {
    let row = sqlx::query("SELECT lang, scheme, timezone, holidays_enabled, school_holidays_enabled, holiday_country, holiday_subdivision, public_regions, school_regions, holiday_color, school_color, vacation_days, vacation_rule_json, contact_columns_json, contact_list_fields_json, alarm_presets_json, alarm_snaps_json, task_columns_json, search_history_days FROM practical_user_settings WHERE principal = ?")
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
            holiday_country: r.get("holiday_country"),
            holiday_subdivision: r.get("holiday_subdivision"),
            public_regions: r.get("public_regions"),
            school_regions: r.get("school_regions"),
            holiday_color: r.get("holiday_color"),
            school_color: r.get("school_color"),
            vacation_days: r.get("vacation_days"),
            vacation_rule_json: r.get("vacation_rule_json"),
            contact_columns_json: r.get("contact_columns_json"),
            contact_list_fields_json: r.get("contact_list_fields_json"),
            alarm_presets_json: r.get("alarm_presets_json"),
            alarm_snaps_json: r.get("alarm_snaps_json"),
            task_columns_json: r.get("task_columns_json"),
            search_history_days: r.get("search_history_days"),
        },
    })
}

/// Language and colour scheme — what every page needs (`PageCtx`), in one query.
pub async fn get_look(db: &SqlitePool, principal: &str) -> Result<(Option<String>, Option<String>), sqlx::Error> {
    Ok(sqlx::query("SELECT lang, scheme FROM practical_user_settings WHERE principal = ?")
        .bind(principal)
        .fetch_optional(db)
        .await?
        .map(|r| (r.get("lang"), r.get("scheme")))
        .unwrap_or_default())
}

pub async fn set(db: &SqlitePool, principal: &str, p: &Prefs) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO practical_user_settings (principal, lang, scheme, timezone, holidays_enabled, school_holidays_enabled, holiday_country, holiday_subdivision, public_regions, school_regions, holiday_color, school_color, vacation_days, vacation_rule_json, contact_columns_json, contact_list_fields_json, alarm_presets_json, alarm_snaps_json, task_columns_json, search_history_days, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP)
         ON CONFLICT(principal) DO UPDATE SET lang=excluded.lang, scheme=excluded.scheme, timezone=excluded.timezone,
           holidays_enabled=excluded.holidays_enabled, school_holidays_enabled=excluded.school_holidays_enabled,
           holiday_country=excluded.holiday_country, holiday_subdivision=excluded.holiday_subdivision,
           public_regions=excluded.public_regions, school_regions=excluded.school_regions,
           holiday_color=excluded.holiday_color, school_color=excluded.school_color, vacation_days=excluded.vacation_days,
           vacation_rule_json=excluded.vacation_rule_json,
           contact_columns_json=excluded.contact_columns_json, contact_list_fields_json=excluded.contact_list_fields_json,
           alarm_presets_json=excluded.alarm_presets_json, alarm_snaps_json=excluded.alarm_snaps_json, task_columns_json=excluded.task_columns_json, search_history_days=excluded.search_history_days, updated_at=CURRENT_TIMESTAMP",
    )
    .bind(principal)
    .bind(&p.lang)
    .bind(&p.scheme)
    .bind(&p.timezone)
    .bind(i64::from(p.holidays_enabled))
    .bind(i64::from(p.school_holidays_enabled))
    .bind(&p.holiday_country)
    .bind(&p.holiday_subdivision)
    .bind(&p.public_regions)
    .bind(&p.school_regions)
    .bind(&p.holiday_color)
    .bind(&p.school_color)
    .bind(p.vacation_days)
    .bind(&p.vacation_rule_json)
    .bind(&p.contact_columns_json)
    .bind(&p.contact_list_fields_json)
    .bind(&p.alarm_presets_json)
    .bind(&p.alarm_snaps_json)
    .bind(&p.task_columns_json)
    .bind(p.search_history_days)
    .execute(db)
    .await?;
    Ok(())
}

/// `#rrggbb` — the only colour form stored (it is written into a <style> block)
pub fn is_hex_color(c: &str) -> bool {
    c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|x| x.is_ascii_hexdigit())
}

/// How vacation days are found (`vacation_rule_json`). A `v: 2` rule names one kind: `category`
/// (the CATEGORIES value, any case), `calendar` ("principal/id") or `title` (a regex over the
/// title, any case). Older rules have no `v`: their `category` meant the category OR the word in
/// the title; they count that way until the user's next save turns them into a `v: 2` rule.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VacationRule {
    #[serde(default)]
    pub v: u8,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub value: String,
}

/// the longest title pattern the settings accept
pub const TITLE_PATTERN_MAX: usize = 200;

impl VacationRule {
    pub fn of(p: &Prefs) -> Self {
        p.vacation_rule_json.as_deref().and_then(|j| serde_json::from_str(j).ok()).unwrap_or(Self { v: 0, kind: "category".into(), value: "Urlaub".into() })
    }
    pub fn legacy(&self) -> bool {
        self.v < 2 && self.kind != "calendar"
    }
    /// A title pattern as the rule uses it; the error is the parser's message.
    pub fn title_regex(pattern: &str) -> Result<regex::Regex, String> {
        if pattern.chars().count() > TITLE_PATTERN_MAX {
            return Err(format!("longer than {TITLE_PATTERN_MAX} characters"));
        }
        regex::RegexBuilder::new(pattern).case_insensitive(true).size_limit(1 << 20).build().map_err(|e| e.to_string())
    }
    /// The test for one event (`key` = "principal/calendar"). A title rule whose pattern does not
    /// parse, and an empty value, match nothing.
    pub fn matcher(&self) -> impl Fn(&str, &[String], &str) -> bool + use<> {
        let (kind, value, legacy) = (self.kind.clone(), self.value.trim().to_string(), self.legacy());
        let re = (kind == "title" && !legacy && !value.is_empty()).then(|| Self::title_regex(&value).ok()).flatten();
        let low = value.to_lowercase();
        move |key: &str, cats: &[String], summary: &str| {
            if value.is_empty() {
                return false;
            }
            let cat = || cats.iter().any(|c| c.trim().to_lowercase() == low);
            match kind.as_str() {
                "calendar" => key == value,
                // before v2: the category OR the word in the title (many clients never write CATEGORIES)
                _ if legacy => cat() || summary.to_lowercase().split(|c: char| !c.is_alphanumeric()).any(|w| w == low),
                "title" => re.as_ref().is_some_and(|r| r.is_match(summary)),
                _ => cat(),
            }
        }
    }
}

#[cfg(test)]
mod pinned;

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(j: &str) -> VacationRule {
        VacationRule::of(&Prefs { vacation_rule_json: Some(j.into()), ..Default::default() })
    }

    #[test]
    fn vacation_rules() {
        let cats = |c: &[&str]| c.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        // no rule yet: the old default, category or title word "Urlaub"
        let m = VacationRule::of(&Prefs::default()).matcher();
        assert!(m("a/b", &cats(&["urlaub"]), "PARIS") && m("a/b", &[], "Urlaub Heimreise") && !m("a/b", &[], "Winterurlaub"));
        // v2 category: the category only, any case
        let m = rule(r#"{"v":2,"kind":"category","value":"Urlaub"}"#).matcher();
        assert!(m("a/b", &cats(&[" URLAUB "]), "PARIS") && !m("a/b", &[], "Urlaub"));
        // v2 title: a regex, any case
        let m = rule(r#"{"v":2,"kind":"title","value":"^urlaub\\b"}"#).matcher();
        assert!(m("a/b", &[], "Urlaub Heimreise") && !m("a/b", &[], "August Urlaub beantragen") && !m("a/b", &cats(&["Urlaub"]), "PARIS"));
        // a pattern that does not parse, an empty value: nothing
        assert!(!rule(r#"{"v":2,"kind":"title","value":"(urlaub"}"#).matcher()("a/b", &[], "(urlaub"));
        assert!(!rule(r#"{"v":2,"kind":"category","value":""}"#).matcher()("a/b", &cats(&[""]), ""));
        // calendar: the key, before and after v2
        assert!(rule(r#"{"kind":"calendar","value":"a/b"}"#).matcher()("a/b", &[], "x"));
        assert!(!rule(r#"{"v":2,"kind":"calendar","value":"a/b"}"#).matcher()("a/c", &[], "x"));
        // the parser's limits
        assert!(VacationRule::title_regex(&"a".repeat(TITLE_PATTERN_MAX + 1)).is_err());
        assert!(VacationRule::title_regex("(?<=x)y").is_err(), "no look-around in the regex crate");
    }
}
