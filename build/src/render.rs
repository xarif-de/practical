//! HTML generation for the calendar views — a direct port of the round-10
//! prototype generators (gen.py / gen6.py), so markup and class names match
//! the design-system CSS one to one.

use crate::domain::events::Instance;
use chrono::{Datelike, Duration, NaiveDate};
use std::collections::HashMap;
use std::fmt::Write;

pub const DOW_DE: [&str; 7] = ["Mo", "Di", "Mi", "Do", "Fr", "Sa", "So"];
pub const DOW_EN: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
pub const DOWL_DE: [&str; 7] = ["Montag", "Dienstag", "Mittwoch", "Donnerstag", "Freitag", "Samstag", "Sonntag"];
pub const DOWL_EN: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
pub const MON_DE: [&str; 12] = ["Januar", "Februar", "März", "April", "Mai", "Juni", "Juli", "August", "September", "Oktober", "November", "Dezember"];
pub const MON_EN: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

pub struct L {
    pub de: bool,
}
impl L {
    pub fn new(lang: &str) -> Self {
        Self { de: lang != "en" }
    }
    pub fn dow(&self, d: NaiveDate) -> &'static str {
        let i = d.weekday().num_days_from_monday() as usize;
        if self.de { DOW_DE[i] } else { DOW_EN[i] }
    }
    pub fn dowl(&self, d: NaiveDate) -> &'static str {
        let i = d.weekday().num_days_from_monday() as usize;
        if self.de { DOWL_DE[i] } else { DOWL_EN[i] }
    }
    pub fn mon(&self, m: u32) -> &'static str {
        if self.de { MON_DE[(m - 1) as usize] } else { MON_EN[(m - 1) as usize] }
    }
    pub fn mon3(&self, m: u32) -> String {
        self.mon(m).chars().take(3).collect()
    }
    pub fn t(&self, de: &'static str, en: &'static str) -> &'static str {
        if self.de { de } else { en }
    }
    pub fn long_date(&self, d: NaiveDate) -> String {
        if self.de { format!("{}, {}. {}", self.dowl(d), d.day(), self.mon(d.month())) } else { format!("{}, {} {}", self.dowl(d), self.mon(d.month()), d.day()) }
    }
    pub fn dm(&self, d: NaiveDate) -> String {
        if self.de { format!("{}.{}.", d.day(), d.month()) } else { format!("{}/{}", d.month(), d.day()) }
    }
}

pub fn e(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            c => o.push(c),
        }
    }
    o
}

/// The six weeks shown for a month (Monday first), like calendar.monthdatescalendar.
pub fn month_weeks(year: i32, month: u32) -> Vec<[NaiveDate; 7]> {
    let first = NaiveDate::from_ymd_opt(year, month, 1).unwrap();
    let mut d = first - Duration::days(i64::from(first.weekday().num_days_from_monday()));
    let mut weeks = vec![];
    loop {
        let mut w = [d; 7];
        for (i, slot) in w.iter_mut().enumerate() {
            *slot = d + Duration::days(i as i64);
        }
        weeks.push(w);
        d += Duration::days(7);
        if d.month() != month || d.year() != year {
            break;
        }
    }
    weeks
}

pub fn week_of(d: NaiveDate) -> [NaiveDate; 7] {
    let mon = d - Duration::days(i64::from(d.weekday().num_days_from_monday()));
    let mut w = [mon; 7];
    for (i, s) in w.iter_mut().enumerate() {
        *s = mon + Duration::days(i as i64);
    }
    w
}

pub fn by_day(instances: &[Instance]) -> HashMap<NaiveDate, Vec<&Instance>> {
    let mut m: HashMap<NaiveDate, Vec<&Instance>> = HashMap::new();
    for i in instances {
        let mut d = i.start_date();
        let last = i.last_date();
        while d <= last {
            m.entry(d).or_default().push(i);
            d += Duration::days(1);
        }
    }
    m
}

pub const ADD: &str = "<button class=\"add\" type=\"button\" title=\"+\" aria-label=\"+\">+</button>";

fn ev_html(i: &Instance, role: Option<&str>) -> String {
    let mut cls = "ev".to_string();
    if i.all_day || i.multi_day() {
        cls.push_str(" allday");
    }
    if let Some(r) = role {
        cls.push_str(" span ");
        cls.push_str(r);
    }
    if i.cancelled {
        cls.push_str(" cancelled");
    }
    let label = if i.all_day { i.summary.clone() } else { format!("{} {}", i.summary, i.time_label()) };
    format!("<div class=\"{cls}\" style=\"--c:{}\" data-key=\"{}\" data-date=\"{}\" title=\"{}\">{}</div>", e(&i.color), e(&i.key()), i.start_date(), e(&i.summary), e(&label))
}

/// Month grid with continuous multi-day bars (lane-aligned per week row).
pub fn month_grid(year: i32, month: u32, today: NaiveDate, instances: &[Instance], l: &L, ev_limit: usize, view_date: NaiveDate) -> String {
    let weeks = month_weeks(year, month);
    let idx = by_day(instances);
    let mut out = String::from("<div class=\"month\" role=\"grid\">");
    for d in 0..7 {
        let _ = write!(out, "<div class=\"dow\">{}</div>", if l.de { DOW_DE[d] } else { DOW_EN[d] });
    }
    for w in &weeks {
        // lanes for multi-day instances in this row
        let mut spans: Vec<&Instance> = vec![];
        for d in w {
            for i in idx.get(d).map(|v| v.as_slice()).unwrap_or(&[]) {
                if i.multi_day() && !spans.iter().any(|s| s.key() == i.key() && s.start == i.start) {
                    spans.push(i);
                }
            }
        }
        spans.sort_by_key(|i| (i.start_date(), std::cmp::Reverse(i.last_date())));
        let mut lanes: Vec<Vec<&Instance>> = vec![];
        let mut lane_of: HashMap<(String, chrono::NaiveDateTime), usize> = HashMap::new();
        for s in &spans {
            let li = lanes.iter().position(|lane| lane.iter().all(|o| o.last_date() < s.start_date() || o.start_date() > s.last_date())).unwrap_or_else(|| {
                lanes.push(vec![]);
                lanes.len() - 1
            });
            lanes[li].push(s);
            lane_of.insert((s.key(), s.start), li);
        }
        for d in w {
            let other = d.month() != month;
            let mut cls = "day".to_string();
            if other {
                cls.push_str(" other");
            }
            if *d == today {
                cls.push_str(" today");
            }
            if *d == view_date && view_date != today {
                cls.push_str(" sel");
            }
            let evs: Vec<&Instance> = idx.get(d).cloned().unwrap_or_default();
            let _ = write!(out, "<div class=\"{cls}\" data-date=\"{d}\"><span class=\"n\">{}</span>{ADD}<div class=\"dots\">", d.day());
            for i in evs.iter().take(4) {
                let _ = write!(out, "<i style=\"--c:{}\"></i>", e(&i.color));
            }
            out.push_str("</div>");
            let mut shown = 0usize;
            // lanes first (multi-day bars), aligned across the row
            let max_lane = evs.iter().filter_map(|i| lane_of.get(&(i.key(), i.start))).max().copied();
            if let Some(ml) = max_lane {
                for li in 0..=ml {
                    let inst = evs.iter().find(|i| lane_of.get(&(i.key(), i.start)) == Some(&li));
                    match inst {
                        Some(i) => {
                            let role = if i.start_date() == *d { "start" } else if i.last_date() == *d { "end" } else { "mid" };
                            out.push_str(&ev_html(i, Some(role)));
                        }
                        None => out.push_str("<div class=\"ev spacer\" aria-hidden=\"true\"></div>"),
                    }
                    shown += 1;
                }
            }
            let singles: Vec<&&Instance> = evs.iter().filter(|i| !i.multi_day()).collect();
            let room = ev_limit.saturating_sub(shown);
            for i in singles.iter().take(room) {
                out.push_str(&ev_html(i, None));
            }
            if singles.len() > room {
                let _ = write!(out, "<div class=\"more\" data-date=\"{d}\">+{} {}</div>", singles.len() - room, l.t("weitere", "more"));
            }
            out.push_str("</div>");
        }
    }
    out.push_str("</div>");
    out
}

pub fn agenda(date: NaiveDate, instances: &[Instance], l: &L, title: &str) -> String {
    let idx = by_day(instances);
    let evs = idx.get(&date).cloned().unwrap_or_default();
    let mut out = format!("<section class=\"agenda\"><h2>{}</h2><ul>", e(title));
    if evs.is_empty() {
        let _ = write!(out, "<li><time></time><span class=\"muted\">{}</span></li>", l.t("Keine Termine", "No events"));
    }
    for i in evs {
        let tl = if i.all_day { l.t("ganztags", "all day").to_string() } else { i.time_label() };
        let _ = write!(out, "<li><time>{}</time><span class=\"lnk\" data-key=\"{}\" data-date=\"{}\"><i class=\"sw\" style=\"--c:{}\"></i> {}</span></li>", tl, e(&i.key()), i.start_date(), e(&i.color), e(&i.summary));
    }
    out.push_str("</ul></section>");
    out
}
/// Right rail timeline. Rendered by app.js (endless scrolling, scroll position survives
/// navigation), the server only says which kind and where to centre on first paint.
pub fn tl_weeks(center: NaiveDate, today: NaiveDate) -> String {
    format!("<div class=\"tl\" data-kind=\"weeks\" data-center=\"{center}\" data-today=\"{today}\"></div>")
}

pub fn tl_months(center: NaiveDate, today: NaiveDate) -> String {
    format!("<div class=\"tl\" data-kind=\"months\" data-center=\"{center}\" data-today=\"{today}\"></div>")
}

/// Orbit picker: 17-year outer ring (current year at 6 o'clock), month ring, centre grid slot (JS fills).
pub fn picker(year: i32, month: u32, density: &[usize; 12], l: &L) -> String {
    let (cx, cy, ry, rm) = (400.0, 400.0, 360.0, 270.0);
    let mut out = String::from("<svg class=\"orbit\" viewBox=\"0 0 800 800\">");
    let _ = write!(out, "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{ry}\" class=\"ring\"/><circle cx=\"{cx}\" cy=\"{cy}\" r=\"{rm}\" class=\"ring\"/>");
    for i in 0..17 {
        let a = (45.0 + i as f64 * (270.0 / 16.0) - 90.0_f64).to_radians();
        let (x, y) = (cx + ry * a.cos(), cy + ry * a.sin());
        let yr = year + (8 - i);
        let _ = write!(out, "<g class=\"ynode{}\" data-year=\"{yr}\"><circle cx=\"{x:.0}\" cy=\"{y:.0}\" r=\"{}\"/><text x=\"{x:.0}\" y=\"{:.0}\" text-anchor=\"middle\">{yr}</text></g>", if i == 8 { " cur" } else { "" }, if i == 8 { 20 } else { 15 }, y + 4.0);
    }
    for (ang, label, cls) in [(-30.0, "« 7", "ybtn prev"), (0.0, "↺", "ybtn reset"), (30.0, "7 »", "ybtn next")] {
        let a = (ang - 90.0_f64).to_radians();
        let (x, y) = (cx + ry * a.cos(), cy + ry * a.sin());
        let _ = write!(out, "<g class=\"{cls}\"><circle cx=\"{x:.0}\" cy=\"{y:.0}\" r=\"18\"/><text x=\"{x:.0}\" y=\"{:.0}\" text-anchor=\"middle\">{label}</text></g>", y + 4.0);
    }
    let maxd = density.iter().copied().max().unwrap_or(0).max(1);
    for i in 0..12u32 {
        let a = (i as f64 / 12.0) * std::f64::consts::TAU - std::f64::consts::FRAC_PI_2;
        let (x, y) = (cx + rm * a.cos(), cy + rm * a.sin());
        let cur = i + 1 == month;
        let n = density[i as usize];
        let r = if cur { 22.0 } else { 14.0 + 10.0 * n as f64 / maxd as f64 };
        let _ = write!(out, "<g class=\"mnode{}\" data-month=\"{}\"><circle cx=\"{x:.0}\" cy=\"{y:.0}\" r=\"{r:.0}\"/><text x=\"{x:.0}\" y=\"{:.0}\" text-anchor=\"middle\">{}</text></g>", if cur { " cur" } else { "" }, i + 1, y + 4.0, l.mon3(i + 1));
    }
    out.push_str("</svg>");
    format!("<div class=\"picker\" id=\"picker\" hidden data-year=\"{year}\" data-month=\"{month}\"><div class=\"stage\">{out}<div class=\"centre\"><h2 id=\"pk-title\">{} {year}</h2><div class=\"month mini-grid\" id=\"pk-grid\"></div></div><button class=\"close icon-btn\" id=\"pk-close\" aria-label=\"✕\">✕</button></div></div>", l.mon(month))
}

/// Week view ("Stream"): every week of the month, active week tinted, left heat map.
/// Weeks the week view renders before its focus week and after it; the instance window must cover
/// `week_window`, the client extends the stream from there (app.js `mountWeeks`).
pub const WEEK_BACK: i64 = 4;
pub const WEEK_AHEAD: i64 = 5;

pub fn monday_of(d: NaiveDate) -> NaiveDate {
    d - Duration::days(i64::from(d.weekday().num_days_from_monday()))
}

/// [start, end) of the weeks `week_view` renders initially.
pub fn week_window(view_date: NaiveDate) -> (NaiveDate, NaiveDate) {
    let mon = monday_of(view_date);
    (mon - Duration::days(7 * WEEK_BACK), mon + Duration::days(7 * (WEEK_AHEAD + 1)))
}

/// `count` week blocks from the Monday of `start`. Each block stands on its own — no month
/// context — so the stream can run on endlessly in both directions.
pub fn week_blocks(start: NaiveDate, count: usize, today: NaiveDate, instances: &[Instance], l: &L) -> String {
    let idx = by_day(instances);
    let (mon, today_mon) = (monday_of(start), monday_of(today));
    let mut out = String::new();
    for wi in 0..count {
        let w0 = mon + Duration::days(7 * wi as i64);
        let w6 = w0 + Duration::days(6);
        let _ = write!(out, "<div class=\"week{}\" data-week=\"{w0}\"><h2 class=\"wk\">KW {} <span>{} – {}</span></h2>", if w0 == today_mon { " now" } else { "" }, w0.iso_week().week(), l.dm(w0), l.dm(w6));
        for i in 0..7 {
            let d = w0 + Duration::days(i);
            let evs = idx.get(&d).cloned().unwrap_or_default();
            let mut items = String::new();
            for i in &evs {
                let tl = if i.all_day { l.t("ganztags", "all day").to_string() } else { i.time_label() };
                let _ = write!(items, "<li><time>{}</time><span class=\"ev{}{}\" style=\"--c:{}\" data-key=\"{}\" data-date=\"{}\">{}</span></li>", tl, if i.all_day || i.multi_day() { " allday" } else { "" }, if i.cancelled { " cancelled" } else { "" }, e(&i.color), e(&i.key()), i.start_date(), e(&i.summary));
            }
            if items.is_empty() {
                items = "<li class=\"none\"><time></time><span>–</span></li>".into();
            }
            let _ = write!(out, "<section class=\"dayblock{}{}\" data-date=\"{d}\"><div class=\"dl\"><b>{}</b><span>{}</span></div><ul>{items}</ul>{ADD}</section>", if d == today { " today" } else { "" }, if evs.is_empty() { " empty" } else { "" }, d.day(), l.dow(d));
        }
        out.push_str("</div>");
    }
    out
}

pub fn week_view(view_date: NaiveDate, today: NaiveDate, instances: &[Instance], l: &L) -> String {
    let (year, month) = (view_date.year(), view_date.month());
    let (start, end) = week_window(view_date);
    let count = ((end - start).num_days() / 7) as usize;
    let stream = week_blocks(start, count, today, instances, l);
    let idx = by_day(instances);
    let mut heat = String::new();
    for w in &month_weeks(year, month) {
        for d in w {
            let n = idx.get(d).map(|v| v.len()).unwrap_or(0);
            let _ = write!(heat, "<i class=\"{}{}\" style=\"--n:{}\" data-date=\"{d}\" title=\"{}\"></i>", if d.month() != month { "o" } else { "" }, if *d == today { " t" } else { "" }, n.min(4), l.dm(*d));
        }
    }
    // the heat panel names its month: the title carries the week now, so nothing else would
    format!("<div class=\"cal v-week on\" data-focus=\"{}\"><div class=\"heat\" data-month=\"{year}-{month:02}\"><h4>{} <span class=\"hm\">{} {year}</span></h4><div class=\"grid\">{heat}</div></div><div class=\"stream\">{stream}</div>{}</div>", monday_of(view_date), l.t("Dichte", "Density"), l.mon(month), tl_weeks(view_date, today))
}

/// Day view ("Ledger"): dense day column of the month + the selected day as a document.
pub fn day_view(view_date: NaiveDate, today: NaiveDate, instances: &[Instance], l: &L, cal_names: &str) -> String {
    let (year, month) = (view_date.year(), view_date.month());
    let idx = by_day(instances);
    let mut led = String::new();
    let days = month_weeks(year, month).into_iter().flatten().filter(|d| d.month() == month);
    for d in days {
        let evs = idx.get(&d).cloned().unwrap_or_default();
        let names: Vec<String> = evs.iter().map(|i| e(&i.summary)).collect();
        let _ = write!(led, "<a class=\"lrow{}{}{}\" href=\"/calendar?view=day&date={d}\" data-date=\"{d}\"><b>{:02}</b><span>{}</span><i style=\"--n:{}\"></i><em>{}</em>{ADD}</a>", if d == today { " today" } else { "" }, if d == view_date { " sel" } else { "" }, if d.weekday().num_days_from_monday() >= 5 { " we" } else { "" }, d.day(), l.dow(d), evs.len().min(6), names.join(", "));
    }
    let evs = idx.get(&view_date).cloned().unwrap_or_default();
    let mut doc = format!("<section class=\"doc\" data-date=\"{view_date}\"><h2>{} {ADD}</h2><div class=\"sub\">{} {} · {}</div>", e(&l.long_date(view_date)), evs.len(), l.t("Termine", "events"), e(cal_names));
    if evs.is_empty() {
        let _ = write!(doc, "<p class=\"muted\">{}</p>", l.t("Keine Termine an diesem Tag.", "No events on this day."));
    }
    for i in &evs {
        let time = if i.all_day { l.t("ganztags", "all day").to_string() } else if i.end > i.start { format!("{}–{}", i.time_label(), i.end_label()) } else { i.time_label() };
        let mut meta = format!("<span><i class=\"sw\" style=\"--c:{}\"></i> {}</span>", e(&i.color), e(&i.cal_name));
        if let Some(m) = i.alarm_minutes {
            let _ = write!(meta, "<span>⏰ {}</span>", alarm_label(m, l));
        }
        if i.recurring {
            let _ = write!(meta, "<span>{}</span>", l.t("Serie", "recurring"));
        }
        if !i.location.is_empty() {
            let _ = write!(meta, "<span>📍 {}</span>", e(&i.location));
        }
        for c in &i.categories {
            let _ = write!(meta, "<span>{}</span>", e(c));
        }
        let _ = write!(doc, "<article class=\"lnk\" data-key=\"{}\" data-date=\"{}\"><time>{time}</time><div><h3>{}</h3>{}<div class=\"meta\">{meta}</div></div></article>", e(&i.key()), i.start_date(), e(&i.summary), if i.description.is_empty() { String::new() } else { format!("<p>{}</p>", e(&i.description).replace('\n', "<br>")) });
    }
    doc.push_str("</section>");
    format!("<div class=\"cal v-day on\"><div class=\"ledger\">{led}</div>{doc}{}</div>", tl_weeks(view_date, today))
}

pub fn alarm_label(m: i64, l: &L) -> String {
    if m == 0 {
        l.t("zum Termin", "at start").to_string()
    } else if m % 1440 == 0 {
        let d = m / 1440;
        if l.de { format!("{d} Tag{} vorher", if d == 1 { "" } else { "e" }) } else { format!("{d} day{} before", if d == 1 { "" } else { "s" }) }
    } else if m % 60 == 0 {
        let h = m / 60;
        if l.de { format!("{h} Stunde{} vorher", if h == 1 { "" } else { "n" }) } else { format!("{h} hour{} before", if h == 1 { "" } else { "s" }) }
    } else if l.de {
        format!("{m} Minuten vorher")
    } else {
        format!("{m} minutes before")
    }
}

pub fn month_view(view_date: NaiveDate, today: NaiveDate, instances: &[Instance], l: &L, agenda_date: NaiveDate) -> String {
    let grid = month_grid(view_date.year(), view_date.month(), today, instances, l, 3, view_date);
    let title = if agenda_date == today { format!("{}, {}", l.t("Heute", "Today"), l.long_date(agenda_date)) } else { l.long_date(agenda_date) };
    format!("<div class=\"cal v-month on\">{grid}{}{}</div>", tl_months(view_date, today), agenda(agenda_date, instances, l, &title))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weeks_of_september_2026() {
        let w = month_weeks(2026, 9);
        assert_eq!(w.len(), 5);
        assert_eq!(w[0][0], NaiveDate::from_ymd_opt(2026, 8, 31).unwrap());
        assert_eq!(w[4][6], NaiveDate::from_ymd_opt(2026, 10, 4).unwrap());
        assert_eq!(week_of(NaiveDate::from_ymd_opt(2026, 9, 15).unwrap())[0], NaiveDate::from_ymd_opt(2026, 9, 14).unwrap());
    }
}
