//! The "Mehr ▾" views: Jahresplaner, Freie Wochenenden, Urlaubsplaner, Countdown-Wand, Jahr in Zahlen —
//! fed with real instances, holidays and birthdays.
//!
//! A year of instances per calendar is kept in [`YearCache`]. Vacation and bridge days count by the user's own
//! region: the first region of the public-holiday list ([`crate::domain::prefs`]). Which events the year view marks,
//! which count as vacation and which the countdown wall shows: the view configurations ([`crate::domain::views`]),
//! applied per request (the cache keeps raw instances, so a changed rule shows at once).

use crate::ctx::PageCtx;
use crate::domain::collections::{self, CalInfo};
use crate::domain::contacts::{self, Contact};
use crate::domain::events::{self, Instance};
use crate::domain::holidays::{self, Holiday};
use crate::domain::views::{self, CalKeys};
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
    /// the year view's active rules (own, then adopted) and the instances they mark with the first rule's index
    pub marks: Vec<MarkInfo>,
    pub marked: Vec<Instance>,
    pub mark_of: HashMap<(String, NaiveDateTime), usize>,
    /// the user's vacations, entitlement and rules (their labels, for the planner's foot)
    pub vacations: Vec<Instance>,
    pub vacation_days: Option<i64>,
    pub vacation_rule: String,
    /// the vacations of the people who share them and whom the user chose (`views::Vacation::people`)
    pub others: Vec<OtherVacation>,
    /// the countdown wall's events: (instance, the rule's icon)
    pub countdown: Vec<(Instance, String)>,
    pub birthdays: Vec<Contact>,
    /// the school holidays' regions ("DE-SN, DE-BY") for the legends
    pub school_regions: String,
}

/// A year-view rule as the planner shows it (cell tint, icon, legend)
pub struct MarkInfo {
    pub label: String,
    pub color: String,
    pub icon: Option<String>,
    /// the person who offers it
    pub by: Option<String>,
}

/// Another person's vacations as the planner shows them: only from events of theirs the user sees, counted by their
/// own region's public holidays
pub struct OtherVacation {
    pub label: String,
    pub days: Option<i64>,
    pub vacations: Vec<Instance>,
    pub own: Vec<Holiday>,
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
    /// STATUS:CANCELLED: no view configuration counts it (views::Rule)
    cancelled: bool,
}

/// (calendar + view + path: `state::Authorized::cache_key`, synctoken, zone, year)
type YearKey = (String, i64, chrono_tz::Tz, i32);

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
                objs.push(YearObj { object_id: i.object_id.clone(), summary: i.summary.clone(), color: i.color.clone(), categories: i.categories.clone(), cancelled: i.cancelled });
                objs.len() as u32 - 1
            });
            // an override (RECURRENCE-ID) may carry another title or colour than its series
            let o = if objs[o as usize].summary == i.summary && objs[o as usize].color == i.color && objs[o as usize].categories == i.categories && objs[o as usize].cancelled == i.cancelled {
                o
            } else {
                objs.push(YearObj { object_id: i.object_id.clone(), summary: i.summary.clone(), color: i.color.clone(), categories: i.categories.clone(), cancelled: i.cancelled });
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
        if g.0.keys().any(|o| o.0 == k.0 && o.1 > k.1) {
            return;
        }
        let stale: Vec<YearKey> = g.0.keys().filter(|o| o.0 == k.0 && o.1 < k.1).cloned().collect();
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
        let key: YearKey = (cal.auth.cache_key(), cal.synctoken, tz, year);
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
                cancelled: o.cancelled,
            }
        }));
    }
    // the order `events::instances` gives (each part is sorted, the merge is stable)
    out.sort_by(|a, b| a.start.cmp(&b.start).then(b.all_day.cmp(&a.all_day)).then(a.summary.cmp(&b.summary)));
    Ok(out)
}

/// `view`: which of the five views it is for — each reads only its own parts (the others stay empty)
pub async fn year_data(state: &SharedState, user: &Principal, ctx: &PageCtx, year: i32, cals: &[CalInfo], vc: &ViewCtx, view: &str) -> AppResult<YearData> {
    let (Some(start), Some(end)) = (NaiveDate::from_ymd_opt(year, 1, 1).filter(|d| crate::domain::ical::year_ok(*d)), NaiveDate::from_ymd_opt(year + 1, 1, 1)) else { return Err(crate::error::AppError::bad("err-date-range")) };
    let instances = year_instances(state, cals, (start, end), year, vc.tz).await?;
    let p = &vc.prefs;
    let (public_regions, school_regions) = (p.regions(false), p.regions(true));
    let public = if p.holidays_enabled { holidays::for_regions(state, "public", &public_regions, year, &ctx.lang).await? } else { vec![] };
    let school = if p.school_holidays_enabled { holidays::for_regions(state, "school", &school_regions, year, &ctx.lang).await? } else { vec![] };
    let own = match p.own_region().filter(|_| p.holidays_enabled) {
        Some(r) => holidays::for_regions(state, "public", &[r], year, &ctx.lang).await?,
        None => vec![],
    };
    // the view configurations, only for the view that shows them: the planner's marks, the vacation planner's own and
    // shared vacations, the countdown's events; rules over all calendars the user has (`cals` may be the filter's subset)
    let (planner, vac, cd) = (view == "planner", view == "vacation", view == "countdown");
    let (mut marks, mut marked, mut mark_of, mut vacations, mut others, mut countdown) = (vec![], vec![], HashMap::new(), vec![], vec![], vec![]);
    let mut cfg = views::ViewConfig::default();
    let mut names: HashMap<String, String> = HashMap::new();
    if planner || vac || cd {
        cfg = views::load(&state.db, &user.id).await?;
        let all = collections::calendars_for(state, user, false).await?;
        let keys = CalKeys::of(&all);
        // by the canonical key, and by the path a former calendar setting stored
        names = all.iter().flat_map(|c| [(views::canonical(c), c.displayname.clone()), (format!("{}/{}", c.principal, c.id), c.displayname.clone())]).collect();
        let people = views::shared_with(state, &user.id, &all).await?;
        if planner {
            let rules = views::active(&cfg.year, &people.iter().map(|s| (s, s.offer.year.as_slice())).collect::<Vec<_>>(), &names);
            marks = rules.iter().map(|a| MarkInfo { label: a.label.clone(), color: a.color.clone().unwrap_or_else(|| "#32ade6".into()), icon: a.icon.clone(), by: a.by.as_ref().map(|b| b.1.clone()) }).collect();
            for i in &instances {
                if let Some(n) = rules.iter().position(|a| a.matches(i, &keys)) {
                    mark_of.insert((i.key(), i.start), n);
                    marked.push(i.clone());
                }
            }
        }
        if vac {
            vacations = instances.iter().filter(|i| cfg.vacation.rules.iter().any(|r| r.applies(i, &keys, None))).cloned().collect();
            for s in people.iter().filter(|s| cfg.vacation.people.get(&s.id).copied().unwrap_or(false)) {
                let Some((days, rules)) = &s.offer.vacation else { continue };
                let v = instances.iter().filter(|i| rules.iter().any(|r| r.applies(i, &keys, Some(&s.id)))).cloned().collect();
                let own = match &s.own_region {
                    Some(r) => holidays::for_regions(state, "public", std::slice::from_ref(r), year, &ctx.lang).await?,
                    None => vec![],
                };
                others.push(OtherVacation { label: s.label.clone(), days: *days, vacations: v, own });
            }
        }
        if cd {
            let rules = views::active(&cfg.countdown, &people.iter().map(|s| (s, s.offer.countdown.as_slice())).collect::<Vec<_>>(), &names);
            // up to a year ahead: next year's instances too (its own cache entries)
            let ahead = match (NaiveDate::from_ymd_opt(year + 1, 1, 1), NaiveDate::from_ymd_opt(year + 2, 1, 1)) {
                (Some(a), Some(b)) if crate::domain::ical::year_ok(a) => year_instances(state, cals, (a, b), year + 1, vc.tz).await?,
                _ => vec![],
            };
            let horizon = vc.today + Duration::days(365);
            countdown = instances.iter().chain(&ahead).filter(|i| i.start_date() <= horizon).filter_map(|i| rules.iter().find(|a| a.matches(i, &keys)).map(|a| (i.clone(), a.icon.clone().unwrap_or_else(|| views::COUNTDOWN_ICON.into())))).collect();
        }
    }
    // birthdays: the countdown's and the numbers' only
    let birthdays: Vec<Contact> = if cd || view == "numbers" {
        let abs = collections::addressbooks_for(state, user, false).await?;
        contacts::contacts_for(state, &abs).await?.into_iter().filter(|c| !c.birthday.is_empty()).collect()
    } else {
        vec![]
    };
    Ok(YearData {
        year,
        today: vc.today,
        instances,
        public,
        school,
        own,
        marks,
        marked,
        mark_of,
        vacations,
        vacation_days: cfg.vacation.days,
        vacation_rule: cfg.vacation.rules.iter().map(|r| r.label(&names)).collect::<Vec<_>>().join(", "),
        others,
        countdown,
        birthdays,
        school_regions: school_regions.join(", "),
    })
}

fn in_hol(h: &[Holiday], d: NaiveDate) -> Option<&Holiday> {
    h.iter().find(|x| x.start <= d && d <= x.end)
}

fn workday(d: NaiveDate, public: &[Holiday]) -> bool {
    d.weekday().num_days_from_monday() < 5 && in_hol(public, d).is_none()
}

/// The workdays of `year` that vacations take (weekends and the region's public holidays excluded), each day once
fn used_days(vacs: &[(String, NaiveDate, NaiveDate)], year: i32, public: &[Holiday]) -> i64 {
    let mut days: std::collections::BTreeSet<NaiveDate> = Default::default();
    for (_, a, b) in vacs {
        let mut d = *a;
        while d <= *b {
            if d.year() == year && workday(d, public) {
                days.insert(d);
            }
            d += Duration::days(1);
        }
    }
    days.len() as i64
}

pub fn planner(y: &YearData, l: &L) -> String {
    let (jan1, next) = (NaiveDate::from_ymd_opt(y.year, 1, 1).unwrap_or_default(), NaiveDate::from_ymd_opt(y.year + 1, 1, 1).unwrap_or_default());
    let idx = render::by_day(&y.instances, jan1, next);
    let marked = render::by_day(&y.marked, jan1, next);
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
            // the day's mark: the first rule that marks one of its events (lowest index), with that event's title
            let u = marked.get(&d).and_then(|v| v.iter().filter_map(|i| y.mark_of.get(&(i.key(), i.start)).map(|n| (*n, i))).min_by_key(|(n, _)| *n)).map(|(n, i)| (n, i.summary.to_string()));
            let ev = idx.get(&d).and_then(|v| v.iter().find(|i| !y.mark_of.contains_key(&(i.key(), i.start))).map(|i| i.summary.to_string()));
            let txt = hol.map(Holiday::label).or(u.as_ref().map(|u| u.1.clone())).or(ev).unwrap_or_default();
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
            let span = match u.and_then(|(n, _)| y.marks.get(n)) {
                Some(m) => format!("<span class=\"u\" style=\"--u:{}\">{}{}</span>", m.color, m.icon.as_deref().map(render::ico).unwrap_or_default(), e(&txt)),
                None => format!("<span>{}</span>", e(&txt)),
            };
            let _ = write!(rows, "<div class=\"{cls}\" data-date=\"{d}\" title=\"{}\"><b>{day}</b><s>{}</s>{kw}{span}</div>", e(&txt), l.dow(d).chars().next().unwrap_or(' '));
        }
        let _ = write!(cols, "<div class=\"mcol\"><h3>{}</h3>{rows}</div>", l.mon(mi));
    }
    // one legend entry per rule that marks a day this year: its tint, icon, label and, for an adopted one, whose it is
    let used: std::collections::HashSet<usize> = y.mark_of.values().copied().collect();
    let marks: String = y.marks.iter().enumerate().filter(|(n, _)| used.contains(n)).map(|(_, m)| format!("<span><i style=\"background:color-mix(in srgb,{} 35%,var(--bg))\"></i>{}{}{}</span>", m.color, m.icon.as_deref().map(render::ico).unwrap_or_default(), e(&m.label), m.by.as_deref().map(|b| format!(" <small>({})</small>", e(b))).unwrap_or_default())).collect();
    format!(
        "<div class=\"planner\">{cols}</div><div class=\"leg\"><span><i style=\"background:color-mix(in srgb,var(--fer) 18%,var(--bg))\"></i>{} {}</span><span><i style=\"background:color-mix(in srgb,var(--hol) 22%,var(--bg))\"></i>{}</span>{marks}<span><i style=\"background:var(--tile-weekend)\"></i>{}</span> <span class=\"sub\">· {} {} · {}</span></div>",
        l.t("v-school-holidays"),
        e(&y.school_regions),
        l.t("v-public-holiday"),
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
    let mut hol_seen = std::collections::HashSet::new();
    // a holiday of several of the user's regions is one bar (its days once: the lane stays one row)
    for h in y.public.iter().filter(|h| hol_seen.insert((h.start, h.end))) {
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
    let spans = |v: &[Instance]| -> Vec<(String, NaiveDate, NaiveDate)> { v.iter().map(|i| (i.summary.to_string(), i.start_date(), i.last_date())).collect() };
    let vacs = spans(&y.vacations);
    let used = used_days(&vacs, y.year, &y.own);
    let remaining = y.vacation_days.map(|v| (v - used).to_string()).unwrap_or_else(|| "–".into());
    // the people who share their vacations: a lane each, and their numbers (from the events the user sees)
    let mut other_lanes = String::new();
    let mut other_rows = String::new();
    for o in &y.others {
        let v = spans(&o.vacations);
        let n = used_days(&v, y.year, &o.own);
        let _ = write!(other_lanes, "<div class=\"lane\" data-l=\"{} · {}\">{}</div>", l.t("v-vacation"), e(&o.label), bars(&v, "u o"));
        let rest = match o.days { Some(d) => l.tn("v-remaining-of", d - n), None => l.ta("v-remaining-of", &[("n", "–".into())]) };
        let _ = write!(other_rows, "<li><b>{}</b> — {n} {} · {} {} {}</li>", e(&o.label), l.tn("v-vacation-days-planned", n), o.days.map(|d| (d - n).to_string()).unwrap_or_else(|| "–".into()), rest, o.days.map(|d| d.to_string()).unwrap_or_else(|| "?".into()));
    }
    let others_box = if other_rows.is_empty() { String::new() } else { format!("<div class=\"ybox oth\"><h2 class=\"yh\">{}</h2><ul>{other_rows}</ul><p class=\"sub\">{}</p></div>", l.t("v-shared-vacations"), l.t("v-shared-vacations-hint")) };
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
        "<div class=\"gantt\"><div class=\"g\">{months}</div><div class=\"lane\" data-l=\"{}\">{we}{tod}</div><div class=\"lane\" data-l=\"{}\">{hol}</div><div class=\"lane\" data-l=\"{} {}\">{}</div><div class=\"lane\" data-l=\"{}\">{}</div>{other_lanes}</div>\
<div class=\"grid2\"><div class=\"kpi\"><div><b>{used}</b><span>{}</span></div><div><b>{remaining}</b><span>{} {}</span></div><div><b>{}</b><span>{}</span></div></div>\
<div class=\"ybox brg\"><h2 class=\"yh\">{}</h2><ul>{bridge}</ul></div></div>{others_box}<p class=\"sub\">{} <code>{}</code> · <a href=\"/settings#views-vacation\">{}</a></p>",
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
        icon: String,
        over: bool,
        /// a birthday: a passed one asks whether it was congratulated
        bday: bool,
    }
    let mut items = vec![];
    for c in &y.birthdays {
        let Some((by, bm, bd)) = contacts::partial_parts(&c.birthday) else { continue };
        // this year's day (29 February: the 28th in a common year), and the next one from today on
        let (Some(this), Some(nd)) = (contacts::yearly_in(y.today.year(), bm, bd), contacts::next_yearly(bm, bd, y.today)) else { continue };
        let age = |yr: i32| by.map(|b| format!(" · {}", yr - b)).unwrap_or_default();
        let ago = (y.today - this).num_days();
        if ago > 0 && ago <= 30 {
            items.push(Item { date: this, label: format!("{}{}", c.full_name, age(this.year())), icon: "cake".into(), over: true, bday: true });
        }
        items.push(Item { date: nd, label: format!("{}{}", c.full_name, age(nd.year())), icon: "cake".into(), over: false, bday: true });
    }
    // the configured events (views::Marks of the countdown), titled as they are (the icon names the rule): a series
    // counts once — its next start, or, with none ahead, its last end up to 30 days ago
    let ahead: std::collections::HashSet<String> = y.countdown.iter().filter(|(v, _)| v.start_date() >= y.today).map(|(v, _)| v.key()).collect();
    let mut seen = std::collections::HashSet::new();
    let mut last_over: HashMap<String, usize> = HashMap::new();
    for (v, icon) in &y.countdown {
        if v.start_date() >= y.today {
            if seen.insert(v.key()) {
                items.push(Item { date: v.start_date(), label: v.summary.to_string(), icon: icon.clone(), over: false, bday: false });
            }
        } else if !ahead.contains(&v.key()) {
            let ago = (y.today - v.last_date()).num_days();
            if ago > 0 && ago <= 30 {
                let it = Item { date: v.last_date(), label: format!("{} {}", v.summary, l.t("v-over")), icon: icon.clone(), over: true, bday: false };
                match last_over.get(&v.key()) {
                    Some(&n) => items[n] = it,
                    None => {
                        last_over.insert(v.key(), items.len());
                        items.push(it);
                    }
                }
            }
        }
    }
    for h in &y.public {
        if h.start >= y.today {
            items.push(Item { date: h.start, label: h.label(), icon: "party".into(), over: false, bday: false });
        }
    }
    items.push(Item { date: NaiveDate::from_ymd_opt(y.today.year() + 1, 1, 1).unwrap(), label: l.t("v-new-year").into(), icon: "party".into(), over: false, bday: false });
    items.sort_by_key(|i| i.date);
    let tile = |i: &Item| {
        if i.over {
            let ago = (y.today - i.date).num_days();
            let pct = 100 - ago * 100 / 30;
            format!("<div class=\"cd over\"><div class=\"ring\" style=\"--p:{pct}\"><b>−{ago}</b><small>{}</small></div><div><div class=\"ic\">{}</div><h3>{}</h3><span>{} {}{}</span></div></div>", l.tn("v-days", ago), crate::render::ico(&i.icon), e(&i.label), l.t("v-was"), l.long_date(i.date), if i.bday { format!(" · {}", l.t("v-not-congratulated-yet")) } else { String::new() })
        } else {
            let left = (i.date - y.today).num_days();
            let pct = (100 - left * 100 / 365).clamp(0, 100);
            format!("<div class=\"cd\"><div class=\"ring\" style=\"--p:{pct}\"><b>{left}</b><small>{}</small></div><div><div class=\"ic\">{}</div><h3>{}</h3><span>{}{}</span></div></div>", l.tn("v-days", left), crate::render::ico(&i.icon), e(&i.label), l.long_date(i.date), if i.date.year() != y.today.year() { format!(" {}", i.date.year()) } else { String::new() })
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
    let y = year_data(state, user, ctx, date.year(), cals, vc, view).await?;
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
