//! The "Mehr ▾" views: Jahresplaner, Freie Wochenenden, Urlaubsplaner, Countdown-Wand, Jahr in Zahlen —
//! fed with real instances, holidays and birthdays.
//!
//! A year of instances per calendar is kept in [`YearCache`]. Vacation and bridge days count by the user's own
//! region: the first region of the public-holiday list ([`crate::domain::prefs`]).

use crate::ctx::PageCtx;
use crate::domain::collections::{self, CalInfo};
use crate::domain::contacts::{self, Contact};
use crate::domain::events::{self, Instance};
use crate::domain::holidays::{self, Holiday};
use crate::domain::prefs;
use crate::error::AppResult;
use crate::render::{self, L, e};
use crate::routes::calendar::ViewCtx;
use crate::state::SharedState;
use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, Weekday};
use rustical_store::auth::Principal;
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write;

pub struct YearData {
    pub year: i32,
    pub today: NaiveDate,
    pub instances: Vec<Instance>,
    /// public / school holidays of all the user's regions (`Holiday::label` names them)
    pub public: Vec<Holiday>,
    pub school: Vec<Holiday>,
    /// public holidays of the user's own region: what makes a day off (vacation days, bridge days)
    pub own: Vec<Holiday>,
    pub vacations: Vec<Instance>,
    pub birthdays: Vec<Contact>,
    pub vacation_days: Option<i64>,
    pub vacation_rule: String,
    /// the school holidays' regions ("DE-SN, DE-BY") for the legends
    pub school_regions: String,
}

/// One calendar's instances of one year as the five views read them (summary, categories, the event's own
/// colour, start/end, all-day; the calendar's name and colour are taken at use, they change without the
/// synctoken). Strings are kept once per object, not per instance.
pub struct YearPart {
    objs: Vec<YearObj>,
    items: Vec<(NaiveDateTime, NaiveDateTime, bool, u32)>,
}

struct YearObj {
    object_id: String,
    /// shared with the instances made of it (`events::Text`)
    summary: events::Text,
    color: String,
    categories: Vec<String>,
}

type YearKey = (String, String, i64, chrono_tz::Tz, i32);

/// The year views' expansions per (calendar, synctoken, zone, year): a year is outside `InstanceCache`'s
/// windows, so every planner/weekends/vacation/countdown/numbers request and every live-change refetch read
/// and parsed the whole year again (~94 % of a planner request's samples on the bench DB).
/// RustiCal moves the synctoken on every object write, so a write is never answered from before it.
/// Least recently used entries go first above `YEAR_CACHE_BYTES` (estimated heap).
#[derive(Default)]
pub struct YearCache(std::sync::Mutex<(HashMap<YearKey, (std::sync::Arc<YearPart>, usize, u64)>, usize, u64)>);

const YEAR_CACHE_BYTES: usize = 6 << 20;

impl YearPart {
    fn of(instances: &[Instance]) -> YearPart {
        let mut ids: HashMap<&str, u32> = HashMap::new();
        let mut objs = vec![];
        let mut items = Vec::with_capacity(instances.len());
        for i in instances {
            let o = *ids.entry(i.object_id.as_str()).or_insert_with(|| {
                objs.push(YearObj { object_id: i.object_id.clone(), summary: i.summary.clone(), color: i.color.clone(), categories: i.categories.clone() });
                objs.len() as u32 - 1
            });
            // an override (RECURRENCE-ID) may carry another title or colour than its series
            let o = if objs[o as usize].summary == i.summary && objs[o as usize].color == i.color && objs[o as usize].categories == i.categories {
                o
            } else {
                objs.push(YearObj { object_id: i.object_id.clone(), summary: i.summary.clone(), color: i.color.clone(), categories: i.categories.clone() });
                objs.len() as u32 - 1
            };
            items.push((i.start, i.end, i.all_day, o));
        }
        objs.shrink_to_fit();
        YearPart { objs, items }
    }
    /// heap estimate as `events::est_bytes` counts it (16-byte classes + 16 of overhead per allocation)
    fn bytes(&self) -> usize {
        let a = |n: usize| if n == 0 { 0 } else { n.div_ceil(16) * 16 + 16 };
        self.items.len() * std::mem::size_of::<(NaiveDateTime, NaiveDateTime, bool, u32)>()
            + self.objs.iter().map(|o| std::mem::size_of::<YearObj>() + a(o.object_id.len()) + a(o.summary.len()) + a(o.color.len()) + a(o.categories.len() * 24) + o.categories.iter().map(|c| a(c.len())).sum::<usize>()).sum::<usize>()
            + 64
    }
}

impl YearCache {
    fn get(&self, k: &YearKey) -> Option<std::sync::Arc<YearPart>> {
        let mut g = self.0.lock().unwrap();
        g.2 += 1;
        let t = g.2;
        g.0.get_mut(k).map(|e| {
            e.2 = t;
            e.0.clone()
        })
    }
    fn put(&self, k: YearKey, v: std::sync::Arc<YearPart>) {
        let size = v.bytes();
        if size > YEAR_CACHE_BYTES / 2 {
            return;
        }
        let mut g = self.0.lock().unwrap();
        let g = &mut *g;
        // a request that read the calendar before a write must not push out the newer token's entries
        if g.0.keys().any(|o| o.0 == k.0 && o.1 == k.1 && o.2 > k.2) {
            return;
        }
        let stale: Vec<YearKey> = g.0.keys().filter(|o| o.0 == k.0 && o.1 == k.1 && o.2 < k.2).cloned().collect();
        for o in stale {
            g.1 -= g.0.remove(&o).unwrap().1;
        }
        while g.1 + size > YEAR_CACHE_BYTES {
            let Some(old) = g.0.iter().min_by_key(|(_, e)| e.2).map(|(o, _)| o.clone()) else { break };
            g.1 -= g.0.remove(&old).unwrap().1;
        }
        g.2 += 1;
        if let Some(prev) = g.0.insert(k, (v, size, g.2)) {
            g.1 -= prev.1;
        }
        g.1 += size;
    }
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.0.lock().unwrap().0.len()
    }
    #[cfg(test)]
    pub fn clear(&self) {
        *self.0.lock().unwrap() = Default::default();
    }
}

/// The year's instances of `cals`, sorted as `events::instances` sorts them. Only what `YearPart` keeps is
/// filled in (location, description, uid and alarms stay empty): the five views read nothing else.
async fn year_instances(state: &SharedState, cals: &[CalInfo], (start, end): (NaiveDate, NaiveDate), year: i32, tz: chrono_tz::Tz) -> AppResult<Vec<Instance>> {
    let mut out = vec![];
    let empty = events::Text::from("");
    for cal in cals.iter().filter(|c| c.components.iter().any(|c| c == "VEVENT")) {
        let key: YearKey = (cal.principal.clone(), cal.id.clone(), cal.synctoken, tz, year);
        let part = match state.caches.years.get(&key) {
            Some(p) => p,
            None => {
                // without the calendar's colour: an instance without its own keeps "" and gets the colour at use
                let bare = CalInfo { displayname: String::new(), color: String::new(), is_group: false, ..cal.clone() };
                let p = std::sync::Arc::new(YearPart::of(&events::instances(state, std::slice::from_ref(&bare), start, end, tz).await?));
                state.caches.years.put(key, p.clone());
                p
            }
        };
        out.extend(part.items.iter().map(|&(start, end, all_day, o)| {
            let o = &part.objs[o as usize];
            Instance {
                principal: cal.principal.clone(),
                cal_id: cal.id.clone(),
                cal_name: cal.displayname.clone(),
                color: if o.color.is_empty() { cal.color.clone() } else { o.color.clone() },
                object_id: o.object_id.clone(),
                uid: String::new(),
                summary: o.summary.clone(),
                location: empty.clone(),
                description: empty.clone(),
                start,
                end,
                all_day,
                recurring: false,
                categories: o.categories.clone(),
                alarm_minutes: None,
                alarms: vec![],
                is_group: cal.is_group,
                cancelled: false,
            }
        }));
    }
    // the order `events::instances` gives (each part is sorted, the merge is stable)
    out.sort_by(|a, b| a.start.cmp(&b.start).then(b.all_day.cmp(&a.all_day)).then(a.summary.cmp(&b.summary)));
    Ok(out)
}

pub async fn year_data(state: &SharedState, user: &Principal, ctx: &PageCtx, year: i32, cals: &[CalInfo], vc: &ViewCtx) -> AppResult<YearData> {
    let (Some(start), Some(end)) = (NaiveDate::from_ymd_opt(year, 1, 1).filter(|d| crate::domain::ical::year_ok(*d)), NaiveDate::from_ymd_opt(year + 1, 1, 1)) else { return Err(crate::error::AppError::bad("err-date-range")) };
    let instances = year_instances(state, cals, (start, end), year, vc.tz).await?;
    let p = &vc.prefs;
    let (public_regions, school_regions) = (p.regions(&state.config, false), p.regions(&state.config, true));
    let public = if p.holidays_enabled { holidays::for_regions(state, "public", &public_regions, year, &ctx.lang).await? } else { vec![] };
    let school = if p.school_holidays_enabled { holidays::for_regions(state, "school", &school_regions, year, &ctx.lang).await? } else { vec![] };
    let own = match p.own_region(&state.config).filter(|_| p.holidays_enabled) {
        Some(r) => holidays::for_regions(state, "public", &[r], year, &ctx.lang).await?,
        None => vec![],
    };
    let rule = prefs::VacationRule::of(p);
    let is_vacation = rule.matcher();
    let vacations: Vec<Instance> = instances.iter().filter(|i| is_vacation(&format!("{}/{}", i.principal, i.cal_id), &i.categories, &i.summary)).cloned().collect();
    let abs = collections::addressbooks_for(state, user, false).await?;
    let birthdays: Vec<Contact> = contacts::contacts_for(state, &abs).await?.into_iter().filter(|c| !c.birthday.is_empty()).collect();
    Ok(YearData {
        year,
        today: vc.today,
        instances,
        public,
        school,
        own,
        vacations,
        birthdays,
        vacation_days: p.vacation_days,
        vacation_rule: vacation_rule_text(&rule, ctx, cals),
        school_regions: school_regions.join(", "),
    })
}

/// The rule as the planner's legend names it: "Kategorie „Urlaub“", "Kalender „Privat“", "Titel /^Urlaub/"
fn vacation_rule_text(rule: &prefs::VacationRule, ctx: &PageCtx, cals: &[CalInfo]) -> String {
    match rule.kind.as_str() {
        "calendar" => format!("{} „{}“", ctx.t("vacation-kind-calendar"), cals.iter().find(|c| format!("{}/{}", c.principal, c.id) == rule.value).map_or(rule.value.as_str(), |c| c.displayname.as_str())),
        _ if rule.legacy() => format!("{} „{}“", ctx.t("vacation-kind-legacy"), rule.value),
        "title" => format!("{} /{}/", ctx.t("vacation-kind-title"), rule.value),
        _ => format!("{} „{}“", ctx.t("vacation-kind-category"), rule.value),
    }
}

fn in_hol(h: &[Holiday], d: NaiveDate) -> Option<&Holiday> {
    h.iter().find(|x| x.start <= d && d <= x.end)
}

fn workday(d: NaiveDate, public: &[Holiday]) -> bool {
    d.weekday().num_days_from_monday() < 5 && in_hol(public, d).is_none()
}

pub fn planner(y: &YearData, l: &L) -> String {
    let (jan1, next) = (NaiveDate::from_ymd_opt(y.year, 1, 1).unwrap_or_default(), NaiveDate::from_ymd_opt(y.year + 1, 1, 1).unwrap_or_default());
    let idx = render::by_day(&y.instances, jan1, next);
    let vac = render::by_day(&y.vacations, jan1, next);
    let mut cols = String::new();
    for mi in 1..=12u32 {
        let mut rows = String::new();
        for day in 1..=31u32 {
            let Some(d) = NaiveDate::from_ymd_opt(y.year, mi, day) else {
                rows.push_str("<div class=\"c x\"></div>");
                continue;
            };
            let hol = in_hol(&y.public, d);
            let fer = in_hol(&y.school, d);
            let u = vac.get(&d).and_then(|v| v.first().map(|i| i.summary.to_string()));
            let ev = idx.get(&d).and_then(|v| v.iter().find(|i| !y.vacations.iter().any(|x| x.key() == i.key() && x.start == i.start)).map(|i| i.summary.to_string()));
            let txt = hol.map(Holiday::label).or(u.clone()).or(ev).unwrap_or_default();
            let mut cls = String::from("c");
            if d.weekday().num_days_from_monday() >= 5 {
                cls.push_str(" we");
            }
            if hol.is_some() {
                cls.push_str(" hol");
            }
            if fer.is_some() {
                cls.push_str(" fer");
            }
            if d == y.today {
                cls.push_str(" t");
            }
            let kw = if d.weekday() == Weekday::Mon { format!("<i>{}</i>", d.iso_week().week()) } else { String::new() };
            let _ = write!(rows, "<div class=\"{cls}\" data-date=\"{d}\" title=\"{}\"><b>{day}</b><s>{}</s>{kw}<span class=\"{}\">{}</span></div>", e(&txt), l.dow(d).chars().next().unwrap_or(' '), if u.is_some() { "u" } else { "" }, e(&txt));
        }
        let _ = write!(cols, "<div class=\"mcol\"><h3>{}</h3>{rows}</div>", l.mon(mi));
    }
    format!(
        "<div class=\"planner\">{cols}</div><div class=\"leg\"><span><i style=\"background:color-mix(in srgb,var(--fer) 18%,var(--bg))\"></i>{} {}</span><span><i style=\"background:color-mix(in srgb,var(--hol) 22%,var(--bg))\"></i>{}</span><span><i style=\"background:color-mix(in srgb,var(--c6) 35%,var(--bg))\"></i>{}</span><span><i style=\"background:var(--tile-weekend)\"></i>{}</span> <span class=\"sub\">· {} {} · {}</span></div>",
        l.t("v-school-holidays"),
        e(&y.school_regions),
        l.t("v-public-holiday"),
        l.t("v-vacation"),
        l.t("v-weekend"),
        l.t("v-week-abbr"),
        l.t("v-top-right-on-mondays"),
        l.t("v-print-ctrl-p")
    )
}

pub fn weekends(y: &YearData, l: &L) -> String {
    let mut d = NaiveDate::from_ymd_opt(y.year, 1, 1).unwrap();
    let idx = render::by_day(&y.instances, d, d + Duration::days(373));
    while d.weekday() != Weekday::Sat {
        d += Duration::days(1);
    }
    let mut cells = String::new();
    let (mut free, mut long_n, mut total) = (0, 0, 0);
    let mut nextfree: Option<NaiveDate> = None;
    while d.year() == y.year {
        let (sa, so) = (d, d + Duration::days(1));
        let mut busy: Vec<String> = vec![];
        for x in [sa, so] {
            for i in idx.get(&x).map(|v| v.as_slice()).unwrap_or(&[]) {
                if !busy.iter().any(|b| **b == *i.summary) {
                    busy.push(i.summary.to_string());
                }
            }
        }
        let longw = in_hol(&y.own, sa - Duration::days(1)).is_some() || in_hol(&y.own, so + Duration::days(1)).is_some();
        let is_free = busy.is_empty();
        total += 1;
        if is_free {
            free += 1;
        }
        if longw {
            long_n += 1;
        }
        let is_next = is_free && nextfree.is_none() && sa >= y.today;
        if is_next {
            nextfree = Some(sa);
        }
        let label = if !busy.is_empty() { busy.iter().take(2).cloned().collect::<Vec<_>>().join(", ") } else if longw { l.t("v-long-weekend").to_string() } else { l.t("v-free").to_string() };
        let _ = write!(cells, "<div class=\"we {}{}{}{}\" data-date=\"{sa}\"><b>{}</b><small>{}</small><span>{}</span></div>", if is_free { "free" } else { "busy" }, if longw { " long" } else { "" }, if so < y.today { " past" } else { "" }, if is_next { " next" } else { "" }, l.dm(sa), l.week_short(sa.iso_week().week()), e(&label));
        d += Duration::days(7);
    }
    format!("<div class=\"kpi\"><div><b>{free}</b><span>{}</span></div><div><b>{}</b><span>{}</span></div><div><b>{long_n}</b><span>{}</span></div></div><div class=\"wes\">{cells}</div>", l.tn("v-free-weekends", free), total - free, l.tn("v-planned", total - free), l.tn("v-extended-by-a-holiday", long_n))
}

pub fn vacation(y: &YearData, l: &L) -> String {
    let jan1 = NaiveDate::from_ymd_opt(y.year, 1, 1).unwrap();
    let ndays = if NaiveDate::from_ymd_opt(y.year, 12, 31).unwrap().ordinal() == 366 { 366 } else { 365 };
    let col = |d: NaiveDate| (d - jan1).num_days() + 1;
    let mut months = String::new();
    for mi in 1..=12u32 {
        let first = NaiveDate::from_ymd_opt(y.year, mi, 1).unwrap();
        let last = if mi == 12 { NaiveDate::from_ymd_opt(y.year, 12, 31).unwrap() } else { NaiveDate::from_ymd_opt(y.year, mi + 1, 1).unwrap() - Duration::days(1) };
        let _ = write!(months, "<div class=\"mh\" style=\"grid-column:{} / {}\">{}</div>", col(first), col(last) + 1, l.mon(mi));
    }
    let mut we = String::new();
    for i in 0..ndays {
        let d = jan1 + Duration::days(i);
        if d.weekday().num_days_from_monday() >= 5 {
            let _ = write!(we, "<div class=\"wd\" style=\"grid-column:{}\"></div>", i + 1);
        }
    }
    let tod = if y.today.year() == y.year { format!("<div class=\"todaymark\" style=\"grid-column:{}\"></div>", col(y.today)) } else { String::new() };
    let mut hol = String::new();
    for h in &y.public {
        let _ = write!(hol, "<div class=\"bar hol\" style=\"grid-column:{} / {}\" title=\"{}\"></div>", col(h.start), col(h.end) + 1, e(&h.label()));
    }
    let bars = |items: &[(String, NaiveDate, NaiveDate)], cls: &str| {
        let mut out = String::new();
        for (n, a, b) in items {
            let _ = write!(out, "<div class=\"bar {cls}\" style=\"grid-column:{} / {}\" title=\"{}: {} – {}\">{}</div>", col(*a).max(1), (col(*b) + 1).min(ndays + 1), e(n), l.dm(*a), l.dm(*b), e(n));
        }
        out
    };
    let school: Vec<(String, NaiveDate, NaiveDate)> = y.school.iter().map(|h| (h.label(), h.start, h.end)).collect();
    let vacs: Vec<(String, NaiveDate, NaiveDate)> = y.vacations.iter().map(|i| (i.summary.to_string(), i.start_date(), i.last_date())).collect();
    // workdays used by vacation instances (weekends + public holidays excluded)
    let mut used_days: std::collections::BTreeSet<NaiveDate> = Default::default();
    for (_, a, b) in &vacs {
        let mut d = *a;
        while d <= *b {
            if d.year() == y.year && workday(d, &y.own) {
                used_days.insert(d);
            }
            d += Duration::days(1);
        }
    }
    let used = used_days.len() as i64;
    let remaining = y.vacation_days.map(|v| (v - used).to_string()).unwrap_or_else(|| "–".into());
    // bridge days: a public holiday on Tue → Mon off, Thu → Fri off (the own region's: a day off is one's own)
    let mut bridge = String::new();
    for h in &y.own {
        let d = h.start;
        let (off, span) = match d.weekday() {
            Weekday::Tue => (d - Duration::days(1), 4),
            Weekday::Thu => (d + Duration::days(1), 4),
            _ => continue,
        };
        if off < y.today && d.year() == y.year && y.today.year() == y.year {
            continue;
        }
        let _ = write!(bridge, "<li><b>{} {}</b> — {} <small>1 {} → {} {}</small></li>", l.dow(off), l.dm(off), e(&h.name), l.t("v-day-off"), span, l.t("v-days-in-a-row"));
    }
    if bridge.is_empty() {
        bridge = format!("<li class=\"muted\">{}</li>", l.t("v-no-bridge-days-this-year"));
    }
    format!(
        "<div class=\"gantt\"><div class=\"g\">{months}</div><div class=\"lane\" data-l=\"{}\">{we}{tod}</div><div class=\"lane\" data-l=\"{}\">{hol}</div><div class=\"lane\" data-l=\"{} {}\">{}</div><div class=\"lane\" data-l=\"{}\">{}</div></div>\
<div class=\"grid2\"><div class=\"kpi\"><div><b>{used}</b><span>{}</span></div><div><b>{remaining}</b><span>{} {}</span></div><div><b>{}</b><span>{}</span></div></div>\
<div class=\"ybox brg\"><h2 class=\"yh\">{}</h2><ul>{bridge}</ul></div></div><p class=\"sub\">{} <code>{}</code> · {}</p>",
        l.t("v-weekends"),
        l.t("v-holidays"),
        l.t("v-school-holidays"),
        e(&y.school_regions),
        bars(&school, "f"),
        l.t("v-vacation"),
        bars(&vacs, "u"),
        l.tn("v-vacation-days-planned", used),
        // "–" (no entitlement set) is no number: the general form
        match y.vacation_days { Some(v) => l.tn("v-remaining-of", v - used), None => l.ta("v-remaining-of", &[("n", "–".into())]) },
        y.vacation_days.map(|v| v.to_string()).unwrap_or_else(|| "?".into()),
        vacs.len(),
        l.tn("v-trips", vacs.len()),
        l.t("v-bridge-days"),
        l.t("v-counting-rule"),
        e(&y.vacation_rule),
        l.t("v-entitlement-and-rule-in-the-settings")
    )
}

pub fn countdown(y: &YearData, l: &L) -> String {
    struct Item {
        date: NaiveDate,
        label: String,
        icon: &'static str,
        over: bool,
    }
    let mut items = vec![];
    for c in &y.birthdays {
        let Some((by, bm, bd)) = contacts::partial_parts(&c.birthday) else { continue };
        // this year's day (29 February: the 28th in a common year), and the next one from today on
        let (Some(this), Some(nd)) = (contacts::yearly_in(y.today.year(), bm, bd), contacts::next_yearly(bm, bd, y.today)) else { continue };
        let age = |yr: i32| by.map(|b| format!(" · {}", yr - b)).unwrap_or_default();
        let ago = (y.today - this).num_days();
        if ago > 0 && ago <= 30 {
            items.push(Item { date: this, label: format!("{}{}", c.full_name, age(this.year())), icon: "cake", over: true });
        }
        items.push(Item { date: nd, label: format!("{}{}", c.full_name, age(nd.year())), icon: "cake", over: false });
    }
    for v in &y.vacations {
        if v.start_date() >= y.today {
            items.push(Item { date: v.start_date(), label: format!("{} {}", l.t("v-vacation"), v.summary), icon: "luggage", over: false });
        } else {
            let ago = (y.today - v.last_date()).num_days();
            if ago > 0 && ago <= 30 {
                items.push(Item { date: v.last_date(), label: format!("{} {} {}", l.t("v-vacation"), v.summary, l.t("v-over")), icon: "luggage", over: true });
            }
        }
    }
    for h in &y.public {
        if h.start >= y.today {
            items.push(Item { date: h.start, label: h.label(), icon: "party", over: false });
        }
    }
    items.push(Item { date: NaiveDate::from_ymd_opt(y.today.year() + 1, 1, 1).unwrap(), label: l.t("v-new-year").into(), icon: "party", over: false });
    items.sort_by_key(|i| i.date);
    let tile = |i: &Item| {
        if i.over {
            let ago = (y.today - i.date).num_days();
            let pct = 100 - ago * 100 / 30;
            format!("<div class=\"cd over\"><div class=\"ring\" style=\"--p:{pct}\"><b>−{ago}</b><small>{}</small></div><div><div class=\"ic\">{}</div><h3>{}</h3><span>{} {} · {}</span></div></div>", l.tn("v-days", ago), crate::render::ico(i.icon), e(&i.label), l.t("v-was"), l.long_date(i.date), l.t("v-not-congratulated-yet"))
        } else {
            let left = (i.date - y.today).num_days();
            let pct = (100 - left * 100 / 365).clamp(0, 100);
            format!("<div class=\"cd\"><div class=\"ring\" style=\"--p:{pct}\"><b>{left}</b><small>{}</small></div><div><div class=\"ic\">{}</div><h3>{}</h3><span>{}{}</span></div></div>", l.tn("v-days", left), crate::render::ico(i.icon), e(&i.label), l.long_date(i.date), if i.date.year() != y.today.year() { format!(" {}", i.date.year()) } else { String::new() })
        }
    };
    let over: Vec<&Item> = items.iter().filter(|i| i.over).collect();
    let up: Vec<&Item> = items.iter().filter(|i| !i.over).take(12).collect();
    let mut body = String::new();
    if !over.is_empty() {
        let _ = write!(body, "<h2 class=\"yh\">{} <span class=\"sub\">({})</span></h2><div class=\"wall\">{}</div>", l.t("v-just-passed"), l.t("v-up-to-30-days"), over.iter().map(|i| tile(i)).collect::<String>());
    }
    let _ = write!(body, "<h2 class=\"yh\" style=\"margin-top:14px\">{}</h2><div class=\"wall\">{}</div>", l.t("v-coming-up"), up.iter().map(|i| tile(i)).collect::<String>());
    body
}

pub fn numbers(y: &YearData, l: &L) -> String {
    let mut percal: BTreeMap<String, (usize, String)> = BTreeMap::new();
    let mut per_week: BTreeMap<u32, usize> = BTreeMap::new();
    let mut evenings: std::collections::HashSet<NaiveDate> = Default::default();
    let mut weekend_days: HashMap<NaiveDate, usize> = HashMap::new();
    for i in &y.instances {
        if i.start.date().year() != y.year {
            continue;
        }
        percal.entry(i.cal_name.clone()).or_insert((0, i.color.clone())).0 += 1;
        *per_week.entry(i.start.date().iso_week().week()).or_default() += 1;
        if !i.all_day && i.start.time() >= chrono::NaiveTime::from_hms_opt(18, 0, 0).unwrap() {
            evenings.insert(i.start.date());
        }
        if i.start.date().weekday().num_days_from_monday() >= 5 {
            *weekend_days.entry(i.start.date()).or_default() += 1;
        }
    }
    let mx = percal.values().map(|v| v.0).max().unwrap_or(1).max(1);
    let mut sorted: Vec<(&String, &(usize, String))> = percal.iter().collect();
    sorted.sort_by_key(|(_, v)| std::cmp::Reverse(v.0));
    let bars: String = sorted.iter().map(|(k, (n, c))| format!("<div class=\"row\"><span>{}</span><div class=\"b\" style=\"--w:{}%;--c:{}\"></div><b>{n}</b></div>", e(k), n * 100 / mx, e(c))).collect();
    let wmax = per_week.values().copied().max().unwrap_or(1).max(1);
    let spark: String = (1..=53u32).map(|w| { let n = per_week.get(&w).copied().unwrap_or(0); format!("<i style=\"--h:{}%\" title=\"{}: {n}\"></i>", n * 100 / wmax, l.week_short(w)) }).collect();
    let mut top: Vec<(u32, usize)> = per_week.iter().map(|(k, v)| (*k, *v)).collect();
    top.sort_by_key(|(_, v)| std::cmp::Reverse(*v));
    let top: String = top.iter().take(5).map(|(k, v)| format!("<li><b>{}</b> — {}</li>", l.week_short(*k), l.tn("v-n-events", *v))).collect();
    let total: usize = percal.values().map(|v| v.0).sum();
    let wes = weekend_days.values().filter(|n| **n >= 2).count();
    format!(
        "<div class=\"kpis\"><div><b>{total}</b><span>{}</span></div><div><b>{}</b><span>{}</span></div><div><b>{wes}</b><span>{}</span></div><div><b>{}</b><span>{}</span></div></div>\
<div class=\"stats\"><div class=\"ybox\"><h2 class=\"yh\">{}</h2>{bars}</div><div class=\"ybox\"><h2 class=\"yh\">{}</h2><div class=\"spark\">{spark}</div><div class=\"sub\">{} → {}</div></div><div class=\"ybox topw\"><h2 class=\"yh\">{}</h2><ol>{top}</ol></div></div>",
        l.tn("v-events-this-year", total),
        evenings.len(),
        l.tn("v-evenings-booked-from-6-pm", evenings.len()),
        l.tn("v-busy-weekend-days", wes),
        y.birthdays.len(),
        l.tn("v-birthdays-in-the-address-books", y.birthdays.len()),
        l.t("v-events-per-calendar"),
        l.t("v-load-per-week"),
        l.week_short(1),
        l.week_short(53),
        l.t("v-busiest-weeks")
    )
}

pub async fn render(state: &SharedState, user: &Principal, ctx: &PageCtx, view: &str, date: NaiveDate, cals: &[CalInfo], vc: &ViewCtx) -> AppResult<(String, String)> {
    let y = year_data(state, user, ctx, date.year(), cals, vc).await?;
    let l = &vc.l;
    let (label, _) = crate::routes::calendar::view_label(view, l);
    let body = match view {
        "planner" => planner(&y, l),
        "weekends" => weekends(&y, l),
        "vacation" => vacation(&y, l),
        "countdown" => countdown(&y, l),
        _ => numbers(&y, l),
    };
    let title = if view == "countdown" { label.clone() } else { date.year().to_string() };
    Ok((title, format!("<div class=\"cal v-{view} on\"><div class=\"mv\"><h2 class=\"mvh\">{} <span class=\"sub\">{}</span></h2>{body}</div>{}</div>", e(&label), y.year, render::tl_months(date, vc.today))))
}

#[cfg(test)]
mod pinned;
