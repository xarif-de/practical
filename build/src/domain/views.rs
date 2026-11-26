//! View configurations (`practical_view_config`, settings `#views`): which events the year view marks
//! (colour, optional icon), which count as vacation in the vacation planner (plus the entitlement), which the
//! countdown wall shows (icon).
//!
//! - A [`Rule`] = categories (any of them; none = any category) and calendars (`None` = all of the user's OWN
//!   calendars, their groups' included — a share is the sharer's, picked by name or through their offered rules;
//!   canonical keys `"owner/calendar id"`, so one key names a calendar for its owner and every sharee alike). It needs
//!   a category or a calendar list. A cancelled event (STATUS:CANCELLED) matches no rule.
//! - Sharing: a person who shares calendars with the user offers their rules ([`Marks::share`],
//!   [`Vacation::share`]). Offered rules only ever apply to calendars that person OWNS and the viewer sees
//!   ([`Shared`]): a sharer's rule never marks the viewer's own events, nor calendars the viewer cannot read. The
//!   viewer adopts them one by one ([`Adopt`]; colour and icon overridable for themselves, never written back)
//!   and picks the people whose vacations the planner shows ([`Vacation::people`]).
//! - A user without a row gets [`ViewConfig::from_legacy`]: the former vacation setting
//!   (`practical_user_settings.vacation_rule_json`, `vacation_days`) as one rule in each view.
//! - First match wins: own rules in list order, then adopted rules (people by label, their rule order).

use crate::domain::collections::CalInfo;
use crate::domain::events::Instance;
use crate::domain::prefs::is_hex_color;
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::collections::{BTreeMap, HashMap, HashSet};

/// The icons a rule may carry (ids in `assets/icons.svg`, `i-<id>`); anything else is dropped on save and on read.
pub const ICONS: &[&str] = &["luggage", "palm", "plane", "party", "cake", "star", "heart", "gift", "house", "car", "briefcase", "grad", "sun", "sofa", "flag", "users", "dumbbell", "music", "stethoscope", "pin", "globe"];
/// at most this many rules per view
pub const MAX_RULES: usize = 50;
const NAME_MAX: usize = 60;
const CATEGORY_MAX: usize = 100;
const CATEGORIES_MAX: usize = 30;
const CALS_MAX: usize = 100;
/// a year rule without a colour gets the next of these (the event palette, `collections::PALETTE` minus grey)
const YEAR_COLORS: &[&str] = &["#32ade6", "#30b46c", "#ff9f0a", "#af52de", "#ff2d55", "#0a84ff", "#5856d6", "#a2845e", "#ff3b30"];
/// the countdown icon of a rule without one
pub const COUNTDOWN_ICON: &str = "star";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    /// stable id (8 hex), made on save: what a sharee's [`Adopt`] names
    #[serde(default)]
    pub id: String,
    /// optional label (legend, countdown); empty = the categories, else the calendars
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    /// an event with any of these (any case) is the rule's; none = every category
    pub categories: Vec<String>,
    #[serde(default)]
    pub cals: Option<Vec<String>>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
}

/// A sharee's choice about one offered rule
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Adopt {
    #[serde(default)]
    pub on: bool,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
}

/// The year view's and the countdown wall's configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Marks {
    #[serde(default)]
    pub rules: Vec<Rule>,
    /// offered to the people who see the user's calendars (off unless chosen: rule names and categories can say more
    /// than the events they mark)
    #[serde(default)]
    pub share: bool,
    /// sharer → their rule id → the choice
    #[serde(default)]
    pub adopt: BTreeMap<String, BTreeMap<String, Adopt>>,
}

impl Default for Marks {
    fn default() -> Self {
        Self { rules: vec![], share: false, adopt: BTreeMap::new() }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Vacation {
    /// vacation days per year (0..=366)
    #[serde(default)]
    pub days: Option<i64>,
    #[serde(default)]
    pub rules: Vec<Rule>,
    /// the days and rules offered to the people who see the user's calendars (off unless chosen)
    #[serde(default)]
    pub share: bool,
    /// sharer → shown in the planner
    #[serde(default)]
    pub people: BTreeMap<String, bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ViewConfig {
    #[serde(default)]
    pub v: u8,
    #[serde(default)]
    pub year: Marks,
    #[serde(default)]
    pub countdown: Marks,
    #[serde(default)]
    pub vacation: Vacation,
}

/// The former vacation setting (`prefs.rs` `VacationRule` until 0.30): `{"v":2,"kind":"category"|"calendar"|"title",
/// "value"}`; no `v` = category or title word. Read only to seed [`ViewConfig::from_legacy`].
#[derive(Deserialize)]
struct LegacyRule {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    value: String,
}

impl ViewConfig {
    /// The default of a user without a row: the former vacation setting as one rule per view (a title rule, which
    /// has no counterpart, and none at all become the category "Urlaub", the former default's category half).
    pub fn from_legacy(days: Option<i64>, rule_json: Option<&str>) -> Self {
        let r: Option<LegacyRule> = rule_json.and_then(|j| serde_json::from_str(j).ok());
        let (categories, cals) = match r {
            Some(r) if r.kind == "calendar" && !r.value.trim().is_empty() => (vec![], Some(vec![r.value.trim().to_string()])),
            Some(r) if r.kind == "category" && !r.value.trim().is_empty() => (vec![r.value.trim().to_string()], None),
            _ => (vec!["Urlaub".to_string()], None),
        };
        let rule = |id: &str, color: Option<&str>, icon: Option<&str>| Rule { id: id.into(), name: String::new(), categories: categories.clone(), cals: cals.clone(), color: color.map(Into::into), icon: icon.map(Into::into) };
        Self {
            v: 1,
            year: Marks { rules: vec![rule("legacy1", Some(YEAR_COLORS[0]), None)], ..Marks::default() },
            countdown: Marks { rules: vec![rule("legacy2", None, Some("luggage"))], ..Marks::default() },
            vacation: Vacation { days: days.filter(|d| (0..=366).contains(d)), rules: vec![rule("legacy3", None, None)], share: false, people: BTreeMap::new() },
        }
    }

    /// Checked and normalised for storage: limits, ids, colours, icons; `Err` = an i18n key.
    pub fn validated(mut self) -> Result<Self, &'static str> {
        self.v = 1;
        let mut ids = HashSet::new();
        for (view, rules) in [("year", &mut self.year.rules), ("countdown", &mut self.countdown.rules), ("vacation", &mut self.vacation.rules)] {
            if rules.len() > MAX_RULES {
                return Err("err-views-too-many");
            }
            for (n, r) in rules.iter_mut().enumerate() {
                r.name = r.name.trim().chars().take(NAME_MAX).collect();
                // trimmed, each once (any case), none empty
                let mut seen = HashSet::new();
                r.categories = r.categories.iter().map(|c| c.trim().to_string()).filter(|c| !c.is_empty() && seen.insert(c.to_lowercase())).collect();
                if r.categories.len() > CATEGORIES_MAX || r.categories.iter().any(|c| c.chars().count() > CATEGORY_MAX) {
                    return Err("err-views-rule");
                }
                if let Some(c) = &mut r.cals {
                    c.retain(|k| key_ok(k));
                    c.dedup();
                    if c.len() > CALS_MAX {
                        return Err("err-views-rule");
                    }
                }
                // a rule that would match every event of every calendar is no rule
                if r.categories.is_empty() && r.cals.as_ref().is_none_or(Vec::is_empty) {
                    return Err("err-views-rule");
                }
                r.color = r.color.take().filter(|c| is_hex_color(c)).map(|c| c.to_lowercase());
                r.icon = r.icon.take().filter(|i| ICONS.contains(&i.as_str()));
                match view {
                    "year" => {
                        r.color.get_or_insert_with(|| YEAR_COLORS[n % YEAR_COLORS.len()].into());
                    }
                    "countdown" => {
                        r.icon.get_or_insert_with(|| COUNTDOWN_ICON.into());
                        r.color = None;
                    }
                    _ => {
                        r.color = None;
                        r.icon = None;
                    }
                }
                if r.id.len() != 8 && !r.id.starts_with("legacy") || !r.id.chars().all(|c| c.is_ascii_alphanumeric()) || !ids.insert(r.id.clone()) {
                    r.id = new_id(&ids);
                    ids.insert(r.id.clone());
                }
            }
        }
        for m in [&mut self.year, &mut self.countdown] {
            for a in m.adopt.values_mut().flat_map(|x| x.values_mut()) {
                a.color = a.color.take().filter(|c| is_hex_color(c)).map(|c| c.to_lowercase());
                a.icon = a.icon.take().filter(|i| ICONS.contains(&i.as_str()));
            }
        }
        self.vacation.days = self.vacation.days.filter(|d| (0..=366).contains(d));
        Ok(self)
    }

    /// What a sharer offers one viewer: nothing of a view not shared, and only rules that can touch a calendar of
    /// the sharer the viewer sees (`visible` = canonical keys of those), their calendar lists cut to it; the sharer's
    /// own choices about others (`adopt`, `people`) never leave.
    pub fn offered(&self, visible: &HashSet<String>) -> Offer {
        let cut = |rules: &[Rule]| -> Vec<Rule> {
            rules
                .iter()
                .filter_map(|r| {
                    let mut r = r.clone();
                    if let Some(c) = &mut r.cals {
                        c.retain(|k| visible.contains(k));
                        if c.is_empty() {
                            return None;
                        }
                    }
                    r.color = r.color.filter(|c| is_hex_color(c));
                    r.icon = r.icon.filter(|i| ICONS.contains(&i.as_str()));
                    Some(r)
                })
                .collect()
        };
        Offer {
            year: if self.year.share { cut(&self.year.rules) } else { vec![] },
            countdown: if self.countdown.share { cut(&self.countdown.rules) } else { vec![] },
            vacation: self.vacation.share.then(|| (self.vacation.days, cut(&self.vacation.rules))),
        }
    }
}

/// What one sharer offers one viewer ([`ViewConfig::offered`])
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Offer {
    pub year: Vec<Rule>,
    pub countdown: Vec<Rule>,
    /// (days, rules) when the sharer shares their vacation
    pub vacation: Option<(Option<i64>, Vec<Rule>)>,
}

fn key_ok(k: &str) -> bool {
    k.len() <= 300 && k.split_once('/').is_some_and(|(a, b)| !a.is_empty() && !b.is_empty())
}

fn new_id(taken: &HashSet<String>) -> String {
    loop {
        let id: String = uuid::Uuid::new_v4().simple().to_string().chars().take(8).collect();
        if !taken.contains(&id) {
            return id;
        }
    }
}

/// The user's configuration: the stored row, else [`ViewConfig::from_legacy`].
pub async fn load(db: &SqlitePool, principal: &str) -> Result<ViewConfig, sqlx::Error> {
    if let Some(j) = sqlx::query_scalar::<_, String>("SELECT json FROM practical_view_config WHERE principal = ?").bind(principal).fetch_optional(db).await? {
        // a row that does not parse (hand edit, a future version) reads as empty, never as an error page
        return Ok(serde_json::from_str(&j).unwrap_or_default());
    }
    let legacy = sqlx::query("SELECT vacation_days, vacation_rule_json FROM practical_user_settings WHERE principal = ?").bind(principal).fetch_optional(db).await?;
    Ok(match legacy {
        Some(r) => ViewConfig::from_legacy(r.get("vacation_days"), r.get::<Option<String>, _>("vacation_rule_json").as_deref()),
        None => ViewConfig::from_legacy(None, None),
    })
}

pub async fn save(db: &SqlitePool, principal: &str, c: &ViewConfig) -> Result<(), sqlx::Error> {
    let j = serde_json::to_string(c).unwrap_or_default();
    sqlx::query("INSERT INTO practical_view_config (principal, json, updated_at) VALUES (?, ?, CURRENT_TIMESTAMP) ON CONFLICT(principal) DO UPDATE SET json = excluded.json, updated_at = CURRENT_TIMESTAMP").bind(principal).bind(j).execute(db).await?;
    Ok(())
}

/// The canonical key of every calendar the user has, by the path its instances carry (`principal/cal_id`): a share
/// arrives under the sharee's home (`dave/<copy>`), the rules name it `owner/id`.
/// home → calendar name → key: an instance is looked up without building its path string
pub struct CalKeys(HashMap<String, HashMap<String, CalKey>>);

pub struct CalKey {
    pub key: String,
    pub owner: String,
    /// the user's own (or their group's), not a share
    pub own: bool,
}

impl CalKeys {
    pub fn of(cals: &[CalInfo]) -> Self {
        let mut m: HashMap<String, HashMap<String, CalKey>> = HashMap::new();
        for c in cals {
            m.entry(c.principal.clone()).or_default().insert(c.id.clone(), CalKey { key: canonical(c), owner: c.owner.clone(), own: !c.view.is_copy() });
        }
        Self(m)
    }
    pub fn of_instance(&self, i: &Instance) -> Option<&CalKey> {
        self.0.get(&i.principal)?.get(&i.cal_id)
    }
    /// (path, canonical key, owner, own)
    #[cfg(test)]
    pub fn of_entries(e: &[(&str, &str, &str, bool)]) -> Self {
        let mut m: HashMap<String, HashMap<String, CalKey>> = HashMap::new();
        for (p, k, o, own) in e {
            let (home, id) = p.split_once('/').unwrap();
            m.entry(home.into()).or_default().insert(id.into(), CalKey { key: k.to_string(), owner: o.to_string(), own: *own });
        }
        Self(m)
    }
}

pub fn canonical(c: &CalInfo) -> String {
    format!("{}/{}", c.owner, c.auth.id())
}

impl Rule {
    /// Whether the instance (its calendar = `key`: canonical, `path`: (home, name) as the instance carries it) is the
    /// rule's: one of the categories (any case, trimmed) when any are set, the calendar when listed. A former calendar
    /// setting named the path, so a listed path counts too. Compared without allocating: it runs per instance and rule.
    pub fn matches(&self, categories: &[String], key: &str, path: (&str, &str)) -> bool {
        let same = |a: &str, b: &str| a.trim().chars().flat_map(char::to_lowercase).eq(b.trim().chars().flat_map(char::to_lowercase));
        (self.categories.is_empty() || self.categories.iter().any(|w| categories.iter().any(|c| same(c, w)))) && self.cals.as_ref().is_none_or(|c| c.iter().any(|k| k == key || k.split_once('/') == Some(path)))
    }
    /// Whether the rule takes the instance: an own rule (`sharer` None) without a calendar list only the user's own
    /// calendars; an offered one (`sharer` = who offers it) only that person's calendars; never a cancelled event.
    pub fn applies(&self, i: &Instance, keys: &CalKeys, sharer: Option<&str>) -> bool {
        let Some(k) = keys.of_instance(i) else { return false };
        if i.cancelled {
            return false;
        }
        let scope = match sharer {
            Some(s) => k.owner == s,
            None => self.cals.is_some() || k.own,
        };
        scope && self.matches(&i.categories, &k.key, (&i.principal, &i.cal_id))
    }
    /// The label: the name, else the categories, else the calendars' names (`names`: canonical key → name)
    pub fn label(&self, names: &HashMap<String, String>) -> String {
        if !self.name.is_empty() {
            return self.name.clone();
        }
        if !self.categories.is_empty() {
            return self.categories.join(", ");
        }
        self.cals.iter().flatten().filter_map(|k| names.get(k)).cloned().collect::<Vec<_>>().join(", ")
    }
}

/// A person who shares calendars with the viewer, with what they offer
#[derive(Debug, Clone, Serialize)]
pub struct Shared {
    pub id: String,
    pub label: String,
    pub offer: Offer,
    /// the person's own region (vacation days count by it), from their preferences
    #[serde(skip)]
    pub own_region: Option<String>,
}

/// The people who share calendars with the user (owners of the user's shares that are persons, not the user), each
/// with what they offer for the calendars of theirs the user sees; sorted by label.
pub async fn shared_with(state: &crate::state::AppState, user: &str, cals: &[CalInfo]) -> crate::error::AppResult<Vec<Shared>> {
    use rustical_store::auth::{AuthenticationProvider, PrincipalType};
    let mut by_owner: BTreeMap<String, (String, HashSet<String>)> = BTreeMap::new();
    for c in cals.iter().filter(|c| c.view.is_copy() && c.owner != user) {
        let e = by_owner.entry(c.owner.clone()).or_insert_with(|| (c.shared_by.clone().unwrap_or_else(|| c.owner.clone()), HashSet::new()));
        e.1.insert(canonical(c));
    }
    let mut out = vec![];
    for (owner, (label, visible)) in by_owner {
        let Some(p) = state.principals().get_principal(&owner).await? else { continue };
        if p.principal_type != PrincipalType::Individual {
            continue; // a group has no settings of its own
        }
        let cfg = load(&state.db, &owner).await?;
        let prefs = crate::domain::prefs::get(&state.db, &owner).await?;
        out.push(Shared { id: owner, label, offer: cfg.offered(&visible), own_region: prefs.own_region() });
    }
    out.sort_by(|a, b| a.label.to_lowercase().cmp(&b.label.to_lowercase()).then(a.id.cmp(&b.id)));
    Ok(out)
}

/// One active marking rule as a view applies it: own, or adopted from `by`
#[derive(Debug, Clone)]
pub struct Active {
    pub rule: Rule,
    pub label: String,
    pub color: Option<String>,
    pub icon: Option<String>,
    /// the sharer (id, label) of an adopted rule
    pub by: Option<(String, String)>,
}

impl Active {
    pub fn matches(&self, i: &Instance, keys: &CalKeys) -> bool {
        self.rule.applies(i, keys, self.by.as_ref().map(|b| b.0.as_str()))
    }
}

/// The rules a view applies, in precedence order: own, then adopted (`offers` = per sharer their rules of this view).
/// Colours and icons are checked again here: they go into markup (a `style`, an `href`), whatever the row holds.
pub fn active(own: &Marks, offers: &[(&Shared, &[Rule])], names: &HashMap<String, String>) -> Vec<Active> {
    let color = |c: &Option<String>| c.clone().filter(|c| is_hex_color(c));
    let icon = |i: &Option<String>| i.clone().filter(|i| ICONS.contains(&i.as_str()));
    let mut out: Vec<Active> = own.rules.iter().map(|r| Active { label: r.label(names), color: color(&r.color), icon: icon(&r.icon), rule: r.clone(), by: None }).collect();
    for (s, rules) in offers {
        for r in rules.iter() {
            let Some(a) = own.adopt.get(&s.id).and_then(|m| m.get(&r.id)).filter(|a| a.on) else { continue };
            out.push(Active { label: r.label(names), color: color(&a.color).or_else(|| color(&r.color)), icon: icon(&a.icon).or_else(|| icon(&r.icon)), rule: r.clone(), by: Some((s.id.clone(), s.label.clone())) });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(cat: &str, cals: Option<&[&str]>) -> Rule {
        Rule { categories: cat.split('|').filter(|c| !c.trim().is_empty()).map(Into::into).collect(), cals: cals.map(|c| c.iter().map(|x| x.to_string()).collect()), ..Default::default() }
    }

    #[test]
    fn every_icon_is_in_the_sprite() {
        let sprite = std::fs::read_to_string(format!("{}/assets/icons.svg", env!("CARGO_MANIFEST_DIR"))).unwrap();
        for i in ICONS.iter().chain([&COUNTDOWN_ICON]) {
            assert!(sprite.contains(&format!("<symbol id=\"i-{i}\"")), "{i}");
        }
    }

    #[test]
    fn legacy_default() {
        let c = ViewConfig::from_legacy(Some(30), Some(r#"{"v":2,"kind":"category","value":"Ferien"}"#));
        assert_eq!((c.year.rules[0].categories.clone(), c.countdown.rules[0].icon.as_deref(), c.vacation.days, c.vacation.share), (vec!["Ferien".to_string()], Some("luggage"), Some(30), false));
        let c = ViewConfig::from_legacy(None, Some(r#"{"v":2,"kind":"calendar","value":"anna/privat"}"#));
        assert_eq!((c.year.rules[0].categories.len(), c.year.rules[0].cals.clone()), (0, Some(vec!["anna/privat".to_string()])));
        // a title rule and nothing at all: the category "Urlaub"; days outside 0..=366 dropped
        for j in [Some(r#"{"v":2,"kind":"title","value":"^Urlaub"}"#), None, Some("not json")] {
            let c = ViewConfig::from_legacy(Some(999), j);
            assert_eq!((c.vacation.rules[0].categories.clone(), c.vacation.days), (vec!["Urlaub".to_string()], None));
        }
        assert!(c_ok(ViewConfig::from_legacy(None, None)));
    }

    fn c_ok(c: ViewConfig) -> bool {
        c.validated().is_ok()
    }

    #[test]
    fn matching() {
        let cats = |c: &[&str]| c.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let r = rule("Urlaub", None);
        assert!(r.matches(&cats(&[" URLAUB "]), "a/b", ("a", "b")) && !r.matches(&cats(&["Urlaubsplanung"]), "a/b", ("a", "b")) && !r.matches(&[], "a/b", ("a", "b")));
        let r = rule("", Some(&["anna/privat"]));
        assert!(r.matches(&[], "anna/privat", ("dave", "copy1")) && r.matches(&[], "x/y", ("anna", "privat")) && !r.matches(&[], "anna/arbeit", ("anna", "arbeit")));
        let r = rule("Urlaub", Some(&["anna/privat"]));
        assert!(r.matches(&cats(&["urlaub"]), "anna/privat", ("anna", "privat")) && !r.matches(&cats(&["urlaub"]), "anna/arbeit", ("anna", "arbeit")) && !r.matches(&[], "anna/privat", ("anna", "privat")));
        // several categories: any of them
        let r = rule("Urlaub|Reise", None);
        assert!(r.matches(&cats(&["reise"]), "a/b", ("a", "b")) && r.matches(&cats(&["Arbeit", "URLAUB"]), "a/b", ("a", "b")) && !r.matches(&cats(&["Arbeit"]), "a/b", ("a", "b")));
        assert_eq!(r.label(&HashMap::new()), "Urlaub, Reise");
    }

    #[test]
    fn own_rules_take_own_calendars_offered_ones_the_sharers_and_none_a_cancelled_event() {
        // alice's own "a", her group's "g", bob's "bp" shared with her (under her home as "copy")
        let keys = CalKeys::of_entries(&[("alice/a", "alice/a", "alice", true), ("alice/g", "group.fam/g", "group.fam", true), ("alice/copy", "bob/bp", "bob", false)]);
        let ev = |cal: &str, cat: &str| { let mut i = crate::testkit::instance(cal, "x", "X", "2026-10-26", "2026-10-27"); i.categories = vec![cat.into()]; i };
        let all = rule("Urlaub", None);
        assert!(all.applies(&ev("a", "Urlaub"), &keys, None) && all.applies(&ev("g", "Urlaub"), &keys, None), "own and group calendars");
        assert!(!all.applies(&ev("copy", "Urlaub"), &keys, None), "a share only when picked by name");
        assert!(rule("Urlaub", Some(&["bob/bp"])).applies(&ev("copy", "Urlaub"), &keys, None));
        assert!(all.applies(&ev("copy", "Urlaub"), &keys, Some("bob")) && !all.applies(&ev("a", "Urlaub"), &keys, Some("bob")), "bob's rule: bob's calendars only");
        let mut gone = ev("a", "Urlaub");
        gone.cancelled = true;
        assert!(!all.applies(&gone, &keys, None));
        assert!(!all.applies(&ev("unknown", "Urlaub"), &keys, None), "a calendar the user does not have");
    }

    #[test]
    fn validation() {
        let mut c = ViewConfig::default();
        c.year.rules = vec![rule("Urlaub", None), Rule { color: Some("red".into()), icon: Some("evil".into()), ..rule("Reise", None) }];
        c.countdown.rules = vec![rule("Geburtstag", None)];
        c.vacation.rules = vec![Rule { color: Some("#123456".into()), ..rule("Urlaub", None) }];
        c.vacation.days = Some(400);
        let v = c.validated().unwrap();
        // colours: given ones checked, missing ones from the palette; icons from the list; ids made and unique
        assert_eq!(v.year.rules.iter().map(|r| r.color.clone().unwrap()).collect::<Vec<_>>(), vec![YEAR_COLORS[0], YEAR_COLORS[1]]);
        assert_eq!((v.year.rules[1].icon.clone(), v.countdown.rules[0].icon.as_deref(), v.vacation.rules[0].color.clone(), v.vacation.days), (None, Some(COUNTDOWN_ICON), None, None));
        let ids: HashSet<_> = [&v.year.rules, &v.countdown.rules, &v.vacation.rules].iter().flat_map(|r| r.iter().map(|r| r.id.clone())).collect();
        assert_eq!(ids.len(), 4);
        assert!(ids.iter().all(|i| i.len() == 8));
        // categories: trimmed, each once whatever the case, empty ones gone
        let mut d = ViewConfig::default();
        d.vacation.rules = vec![Rule { categories: vec![" Urlaub ".into(), "urlaub".into(), "".into(), "Reise".into()], ..Default::default() }];
        assert_eq!(d.validated().unwrap().vacation.rules[0].categories, vec!["Urlaub", "Reise"]);
        // ids survive a second save
        assert_eq!(v.clone().validated().unwrap(), v);
        // no category and no calendar (or none left): refused, never "every event"
        for r in [rule("", None), rule("  ", Some(&[])), rule("", Some(&["nokey"]))] {
            let mut c = ViewConfig::default();
            c.year.rules = vec![r];
            assert_eq!(c.validated(), Err("err-views-rule"));
        }
        let mut c = ViewConfig::default();
        c.countdown.rules = vec![rule("x", None); MAX_RULES + 1];
        assert_eq!(c.validated(), Err("err-views-too-many"));
    }

    #[test]
    fn offers_only_shared_views_and_visible_calendars() {
        let mut c = ViewConfig::default();
        c.year.rules = vec![rule("Urlaub", None), rule("", Some(&["anna/geheim"])), rule("Reise", Some(&["anna/geheim", "anna/privat"]))];
        c.year.adopt.insert("bob".into(), BTreeMap::from([("x".into(), Adopt { on: true, ..Default::default() })]));
        c.year.share = true;
        c.countdown.rules = vec![rule("Geburtstag", None)]; // not shared: the default
        c.vacation = Vacation { days: Some(28), rules: vec![rule("Urlaub", None)], share: true, people: BTreeMap::from([("bob".into(), true)]) };
        let visible: HashSet<String> = ["anna/privat".to_string()].into();
        let o = c.offered(&visible);
        // the rule on a calendar the viewer does not see is gone, a mixed list is cut to the visible calendar
        assert_eq!(o.year.len(), 2);
        assert_eq!(o.year[1].cals, Some(vec!["anna/privat".to_string()]));
        assert!(o.countdown.is_empty());
        assert_eq!(o.vacation.as_ref().map(|v| v.0), Some(Some(28)));
        c.vacation.share = false;
        assert!(c.offered(&visible).vacation.is_none());
    }

    #[test]
    fn adopted_rules_follow_own_ones_with_local_colour() {
        let s = Shared { id: "anna".into(), label: "Anna".into(), offer: Offer::default(), own_region: None };
        let offered = vec![Rule { id: "r1".into(), color: Some("#111111".into()), ..rule("Urlaub", None) }, Rule { id: "r2".into(), ..rule("Reise", None) }];
        let mut own = Marks { rules: vec![rule("Dienst", None)], ..Marks::default() };
        own.adopt.insert("anna".into(), BTreeMap::from([("r1".into(), Adopt { on: true, color: Some("#222222".into()), icon: None }), ("r2".into(), Adopt::default())]));
        let a = active(&own, &[(&s, offered.as_slice())], &HashMap::new());
        assert_eq!(a.iter().map(|a| (a.label.as_str(), a.color.clone(), a.by.is_some())).collect::<Vec<_>>(), vec![("Dienst", None, false), ("Urlaub", Some("#222222".into()), true)]);
    }
}
