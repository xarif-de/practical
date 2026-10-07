//! HTML generation for the calendar views — a direct port of the round-10
//! prototype generators (gen.py / gen6.py), so markup and class names match
//! the design-system CSS one to one.

use crate::domain::events::{EventDetail, Instance};
use chrono::{Datelike, Duration, NaiveDate};
use std::collections::HashMap;
use std::fmt::Write;

/// Words and dates of the server-rendered views in the page language (Fluent keys:
/// `mon-N`, `dow-N`, `dowl-N`, `date-*`, `v-*`).
pub struct L {
    pub lang: String,
    i18n: std::sync::Arc<crate::i18n::I18n>,
}
impl L {
    pub fn new(lang: &str, i18n: std::sync::Arc<crate::i18n::I18n>) -> Self {
        Self { lang: lang.to_string(), i18n }
    }
    pub fn t(&self, key: &str) -> String {
        self.i18n.t(&self.lang, key)
    }
    /// Message with named arguments (numbers stay numbers, so plural rules apply).
    pub fn ta(&self, key: &str, args: &[(&str, fluent_bundle::FluentValue<'static>)]) -> String {
        let mut a = fluent_bundle::FluentArgs::new();
        for (k, v) in args {
            a.set(k.to_string(), v.clone());
        }
        self.i18n.t_args(&self.lang, key, Some(&a))
    }
    /// Message with the count `n` (its plural form: "1 Termin", "2 Termine").
    pub fn tn(&self, key: &str, n: impl TryInto<i64>) -> String {
        self.ta(key, &[("n", n.try_into().unwrap_or(i64::MAX).into())])
    }
    /// short weekday name, 0 = Monday
    pub fn dow_i(&self, i: usize) -> String {
        self.t(&format!("dow-{}", i + 1))
    }
    pub fn dowl_i(&self, i: usize) -> String {
        self.t(&format!("dowl-{}", i + 1))
    }
    pub fn dow(&self, d: NaiveDate) -> String {
        self.dow_i(d.weekday().num_days_from_monday() as usize)
    }
    pub fn dowl(&self, d: NaiveDate) -> String {
        self.dowl_i(d.weekday().num_days_from_monday() as usize)
    }
    pub fn mon(&self, m: u32) -> String {
        self.t(&format!("mon-{m}"))
    }
    pub fn mon3(&self, m: u32) -> String {
        self.mon(m).chars().take(3).collect()
    }
    pub fn long_date(&self, d: NaiveDate) -> String {
        self.ta("date-long", &[("dow", self.dowl(d).into()), ("day", i64::from(d.day()).into()), ("month", self.mon(d.month()).into())])
    }
    pub fn dm(&self, d: NaiveDate) -> String {
        self.ta("date-dm", &[("d", i64::from(d.day()).into()), ("m", i64::from(d.month()).into())])
    }
    /// a full date as the language writes it (chrono format from `date-fmt`)
    pub fn date(&self, d: NaiveDate) -> String {
        d.format(&self.t("date-fmt")).to_string()
    }
    pub fn week_title(&self, d: NaiveDate) -> String {
        self.ta("date-week", &[("week", i64::from(d.iso_week().week()).into()), ("year", i64::from(d.iso_week().year()).into())])
    }
    pub fn week_short(&self, w: u32) -> String {
        self.ta("date-week-short", &[("week", i64::from(w).into())])
    }
}

/// Named holidays by day (`domain::holidays::marks`); empty when the user switched them off.
pub type Hols = HashMap<NaiveDate, Vec<crate::domain::holidays::Mark>>;

/// Names of holiday marks, one per name: the regions of blocks with the same name are joined
/// ("Herbstferien (HE, SH)"); one that applies in every chosen region needs none.
fn joined<'a>(marks: impl Iterator<Item = &'a crate::domain::holidays::Mark>) -> Vec<String> {
    let mut out: Vec<(String, Option<Vec<String>>)> = vec![]; // None: applies everywhere
    for m in marks {
        match out.iter_mut().find(|(n, _)| *n == m.name) {
            Some((_, t)) => {
                if m.tags.is_empty() {
                    *t = None;
                } else if let Some(t) = t {
                    for x in &m.tags {
                        if !t.contains(x) {
                            t.push(x.clone());
                        }
                    }
                }
            }
            None => out.push((m.name.clone(), Some(m.tags.clone()).filter(|t| !t.is_empty()))),
        }
    }
    out.into_iter().map(|(n, t)| match t { Some(t) => format!("{n} ({})", t.join(", ")), None => n }).collect()
}

/// The holidays of a day as labels: public in red, school holidays (only with `school`) in amber.
fn hol_labels(hol: &Hols, d: NaiveDate, tag: &str, school: bool) -> String {
    let v = hol.get(&d).map(|v| v.as_slice()).unwrap_or(&[]);
    let public = joined(v.iter().filter(|m| !m.school)).into_iter().map(|n| format!("<{tag} class=\"hday\">{}</{tag}>", e(&n)));
    let fer = joined(v.iter().filter(|m| school && m.school)).into_iter().map(|n| format!("<{tag} class=\"hday school\">{}</{tag}>", e(&n)));
    public.chain(fer).collect()
}

/// Classes of a day cell: `hol` (public holiday, red number), `fer` (school holidays, tinted).
fn hol_class(hol: &Hols, d: NaiveDate) -> String {
    let v = hol.get(&d).map(|v| v.as_slice()).unwrap_or(&[]);
    format!("{}{}", if v.iter().any(|m| !m.school) { " hol" } else { "" }, if v.iter().any(|m| m.school) { " fer" } else { "" })
}

/// JSON for a `<script type="application/json">` block: `<` can only occur inside JSON strings,
/// and as `\u003c` it can no longer close the block (a task title `</script>…` from a shared
/// calendar would otherwise inject markup).
pub fn script_json(json: String) -> String {
    json.replace('<', "\\u003c")
}

/// Whether a URI from the data may become an `href`: a `javascript:` or `vbscript:` link runs
/// script in the page, `data:` shows foreign markup. Every other scheme (http(s), mailto, tel,
/// geo, a conference app's own) and a relative reference stays a link. The URL parser drops tab
/// and newline anywhere and C0 controls and spaces around the URL before it reads the scheme, so
/// those are not counted here either. app.js `lkSafe` applies the same rule.
pub fn safe_href(uri: &str) -> bool {
    let s: String = uri.chars().filter(|c| !c.is_ascii_control() && *c != ' ').take(11).collect::<String>().to_ascii_lowercase();
    !["javascript:", "vbscript:", "data:"].iter().any(|p| s.starts_with(p))
}

/// `e` as a `Display`: escapes while it is written, without a String of its own per value.
pub struct E<'a>(pub &'a str);

impl std::fmt::Display for E<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut rest = self.0;
        while let Some(at) = rest.find(['&', '<', '>', '"', '\'']) {
            f.write_str(&rest[..at])?;
            f.write_str(match rest.as_bytes()[at] {
                b'&' => "&amp;",
                b'<' => "&lt;",
                b'>' => "&gt;",
                b'"' => "&quot;",
                _ => "&#39;",
            })?;
            rest = &rest[at + 1..];
        }
        f.write_str(rest)
    }
}

/// `e(&i.key())` without building the key: "/" needs no escaping, so the parts escape on their own.
struct EKey<'a>(&'a Instance);

impl std::fmt::Display for EKey<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}/{}", E(&self.0.principal), E(&self.0.cal_id), E(&self.0.object_id))
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

fn ev_html(out: &mut String, i: &Instance, role: Option<&str>) {
    out.push_str("<div class=\"ev");
    if i.all_day || i.multi_day() {
        out.push_str(" allday");
    }
    if let Some(r) = role {
        out.push_str(" span ");
        out.push_str(r);
    }
    if i.cancelled {
        out.push_str(" cancelled");
    }
    // the label is the title, and the start time after a space (digits and a colon: nothing to escape)
    let _ = write!(out, "\" style=\"--c:{}\" data-key=\"{}\" data-date=\"{}\" title=\"{}\">{}", E(&i.color), EKey(i), i.start_date(), E(&i.summary), E(&i.summary));
    if !i.all_day {
        let _ = write!(out, " {}", i.start.format("%H:%M"));
    }
    out.push_str("</div>");
}

/// Month grid with continuous multi-day bars (lane-aligned per week row).
pub fn month_grid(year: i32, month: u32, today: NaiveDate, instances: &[Instance], l: &L, ev_limit: usize, view_date: NaiveDate, hol: &Hols) -> String {
    let weeks = month_weeks(year, month);
    let idx = by_day(instances);
    let mut out = String::from("<div class=\"month\" role=\"grid\">");
    for d in 0..7 {
        let _ = write!(out, "<div class=\"dow\">{}</div>", l.dow_i(d));
    }
    // a school-holiday block is named once in the month: on its first day there that is not a
    // public holiday (a public holiday's own name has that place)
    let mut fer_at: HashMap<NaiveDate, Vec<&crate::domain::holidays::Mark>> = HashMap::new();
    let mut placed: Vec<(NaiveDate, &str, &[String])> = vec![];
    for d in weeks.iter().flatten().filter(|d| d.month() == month) {
        let marks = hol.get(d).map(|v| v.as_slice()).unwrap_or(&[]);
        if marks.iter().any(|m| !m.school) {
            continue;
        }
        for m in marks.iter().filter(|m| m.school) {
            if !placed.iter().any(|(s, n, t)| *s == m.start && *n == m.name && *t == m.tags.as_slice()) {
                placed.push((m.start, &m.name, &m.tags));
                fer_at.entry(*d).or_default().push(m);
            }
        }
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
            cls.push_str(&hol_class(hol, *d));
            let evs: Vec<&Instance> = idx.get(d).cloned().unwrap_or_default();
            // names beside the day number: its public holidays, and a school-holiday block where it is placed
            let fer: String = if other {
                String::new()
            } else {
                let public = joined(hol.get(d).into_iter().flatten().filter(|m| !m.school)).into_iter().map(|n| format!("<b class=\"fname pub\" title=\"{0}\">{0}</b>", e(&n)));
                let school = joined(fer_at.get(d).into_iter().flatten().copied()).into_iter().map(|n| format!("<b class=\"fname\" title=\"{0}\">{0}</b>", e(&n)));
                public.chain(school).collect()
            };
            let num = if fer.is_empty() { format!("<span class=\"n\">{}</span>", d.day()) } else { format!("<span class=\"nline\"><span class=\"n\">{}</span>{fer}</span>", d.day()) };
            let _ = write!(out, "<div class=\"{cls}\" data-date=\"{d}\">{num}{ADD}<div class=\"dots\">");
            for i in evs.iter().take(4) {
                let _ = write!(out, "<i style=\"--c:{}\"></i>", E(&i.color));
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
                            ev_html(&mut out, i, Some(role));
                        }
                        None => out.push_str("<div class=\"ev spacer\" aria-hidden=\"true\"></div>"),
                    }
                    shown += 1;
                }
            }
            let singles: Vec<&&Instance> = evs.iter().filter(|i| !i.multi_day()).collect();
            let room = ev_limit.saturating_sub(shown);
            for i in singles.iter().take(room) {
                ev_html(&mut out, i, None);
            }
            if singles.len() > room {
                let _ = write!(out, "<div class=\"more\" data-date=\"{d}\">+{} {}</div>", singles.len() - room, l.t("v-more"));
            }
            out.push_str("</div>");
        }
    }
    out.push_str("</div>");
    out
}

pub fn agenda(date: NaiveDate, instances: &[Instance], l: &L, title: &str, hol: &Hols) -> String {
    let idx = by_day(instances);
    let evs = idx.get(&date).cloned().unwrap_or_default();
    let mut out = format!("<section class=\"agenda\"><h2>{}</h2>{}<ul>", e(title), hol_labels(hol, date, "div", true));
    if evs.is_empty() {
        let _ = write!(out, "<li><time></time><span class=\"muted\">{}</span></li>", l.t("v-no-events"));
    }
    let all_day = if evs.iter().any(|i| i.all_day) { l.t("v-all-day") } else { String::new() };
    for i in evs {
        let tl = if i.all_day { all_day.clone() } else { i.time_label() };
        let _ = write!(out, "<li><time>{}</time><span class=\"lnk\" data-key=\"{}\" data-date=\"{}\"><i class=\"sw\" style=\"--c:{}\"></i> {}</span></li>", tl, EKey(i), i.start_date(), E(&i.color), E(&i.summary));
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
pub fn week_blocks(start: NaiveDate, count: usize, today: NaiveDate, instances: &[Instance], l: &L, hol: &Hols) -> String {
    let idx = by_day(instances);
    let (mon, today_mon) = (monday_of(start), monday_of(today));
    let mut out = String::with_capacity(count * 4096);
    let all_day = if instances.iter().any(|i| i.all_day) { l.t("v-all-day") } else { String::new() };
    for wi in 0..count {
        let w0 = mon + Duration::days(7 * wi as i64);
        let w6 = w0 + Duration::days(6);
        // the week's school holidays are named in its head; the days themselves are only tinted
        let mut blocks: Vec<&crate::domain::holidays::Mark> = vec![];
        for m in (0..7).filter_map(|i| hol.get(&(w0 + Duration::days(i)))).flatten().filter(|m| m.school) {
            if !blocks.iter().any(|b| b.start == m.start && b.name == m.name && b.tags == m.tags) {
                blocks.push(m);
            }
        }
        let fer: String = joined(blocks.into_iter()).iter().map(|n| format!(" <b class=\"fname\">{}</b>", e(n))).collect();
        let _ = write!(out, "<div class=\"week{}\" data-week=\"{w0}\"><h2 class=\"wk\">{} <span>{} – {}</span>{fer}</h2>", if w0 == today_mon { " now" } else { "" }, l.week_short(w0.iso_week().week()), l.dm(w0), l.dm(w6));
        for i in 0..7 {
            let d = w0 + Duration::days(i);
            let evs = idx.get(&d).cloned().unwrap_or_default();
            let mut items = String::new();
            for i in &evs {
                let _ = write!(items, "<li><time>");
                if i.all_day {
                    items.push_str(&all_day);
                } else {
                    let _ = write!(items, "{}", i.start.format("%H:%M"));
                }
                // the start date only where it is not the day's own (a multi-day event's later days): app.js
                // reads the nearest data-date, the day block's (−18 % of the week's gzip bytes, audit3-perf3 #9)
                let _ = write!(items, "</time><span class=\"ev{}{}\" style=\"--c:{}\" data-key=\"{}\"", if i.all_day || i.multi_day() { " allday" } else { "" }, if i.cancelled { " cancelled" } else { "" }, E(&i.color), EKey(i));
                if i.start_date() != d {
                    let _ = write!(items, " data-date=\"{}\"", i.start_date());
                }
                let _ = write!(items, ">{}</span></li>", E(&i.summary));
            }
            let marks = hol_labels(hol, d, "span", false);
            if items.is_empty() && marks.is_empty() {
                items = "<li class=\"none\"><time></time><span>–</span></li>".into();
            }
            if !marks.is_empty() {
                items = format!("<li class=\"hols\"><time></time><span>{marks}</span></li>{items}");
            }
            let _ = write!(out, "<section class=\"dayblock{}{}{}\" data-date=\"{d}\"><div class=\"dl\"><b>{}</b><span>{}</span></div><ul>{items}</ul>{ADD}</section>", if d == today { " today" } else { "" }, if evs.is_empty() && marks.is_empty() { " empty" } else { "" }, &hol_class(hol, d), d.day(), l.dow(d));
        }
        out.push_str("</div>");
    }
    out
}

pub fn week_view(view_date: NaiveDate, today: NaiveDate, instances: &[Instance], l: &L, hol: &Hols) -> String {
    let (start, end) = week_window(view_date);
    let count = ((end - start).num_days() / 7) as usize;
    let stream = week_blocks(start, count, today, instances, l, hol);
    let idx = by_day(instances);
    // the panel follows the focus week's month, the same rule the client applies while scrolling
    let focus = monday_of(view_date);
    let (hy, hm) = (focus.year(), focus.month());
    let mut heat = String::new();
    for w in &month_weeks(hy, hm) {
        let _ = write!(heat, "<div class=\"hw{}\" data-week=\"{}\"><b>{}</b>", if w[0] == focus { " focus" } else { "" }, w[0], w[0].iso_week().week());
        for d in w {
            let n = idx.get(d).map(|v| v.len()).unwrap_or(0);
            let _ = write!(heat, "<i class=\"{}{}\" style=\"--n:{}\" data-date=\"{d}\" title=\"{}\"></i>", if d.month() != hm { "o" } else { "" }, if *d == today { " t" } else { "" }, n.min(4), l.dm(*d));
        }
        heat.push_str("</div>");
    }
    // the heat panel names its month: the title carries the week now, so nothing else would
    format!("<div class=\"cal v-week on\" data-focus=\"{focus}\"><div class=\"heat\" data-month=\"{hy}-{hm:02}\"><h4>{} <span class=\"hm\">{} {hy}</span></h4><div class=\"grid\">{heat}</div></div><div class=\"stream\">{stream}</div>{}</div>", l.t("v-density"), l.mon(hm), tl_weeks(view_date, today))
}

const BYDAY: [&str; 7] = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"];

pub fn ico(name: &str) -> String {
    format!("<svg class=\"ico\" aria-hidden=\"true\"><use href=\"{}#i-{name}\"/></svg>", crate::routes::assets::icons())
}

/// Same icon, painted in a colour of its own — the calendar's, in the day document.
fn ico_c(name: &str, color: &str) -> String {
    format!("<svg class=\"ico\" aria-hidden=\"true\" style=\"color:{}\"><use href=\"{}#i-{name}\"/></svg>", e(color), crate::routes::assets::icons())
}

/// An RRULE in words. Parts the sentence cannot carry (BYSETPOS, several ordinals, …) are left out
/// rather than guessed — the base sentence stays true, the editor shows the whole rule.
pub fn rrule_text(rule: &str, start: NaiveDate, l: &L) -> String {
    let rule = crate::domain::ical::rrule_canon(rule);
    let mut m = std::collections::HashMap::new();
    for part in rule.split(';') {
        if let Some((k, v)) = part.split_once('=') {
            m.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    let get = |k: &str| m.get(k).map(|s| s.as_str()).unwrap_or("");
    let n: i64 = get("INTERVAL").parse().unwrap_or(1);
    let freq = get("FREQ");
    let mut out = match (freq, n) {
        ("DAILY", 1) => l.t("v-daily").to_string(),
        ("WEEKLY", 1) => l.t("v-weekly").to_string(),
        ("MONTHLY", 1) => l.t("v-monthly").to_string(),
        ("YEARLY", 1) => l.t("v-yearly").to_string(),
        ("DAILY", n) => l.ta("v-rr-every-days", &[("n", n.into())]),
        ("WEEKLY", n) => l.ta("v-rr-every-weeks", &[("n", n.into())]),
        ("MONTHLY", n) => l.ta("v-rr-every-months", &[("n", n.into())]),
        ("YEARLY", n) => l.ta("v-rr-every-years", &[("n", n.into())]),
        _ => return l.t("v-recurring").into(),
    };
    let byday = get("BYDAY");
    let names: Vec<String> = byday
        .split(',')
        .filter(|x| !x.is_empty())
        .filter_map(|x| {
            let code = x.trim_start_matches(|c: char| c.is_ascii_digit() || c == '-' || c == '+');
            BYDAY.iter().position(|b| *b == code).map(|p| l.dowl_i(p))
        })
        .collect();
    let ord = byday.split(',').next().unwrap_or("").trim_end_matches(char::is_alphabetic).parse::<i32>().ok();
    if freq == "WEEKLY" && !names.is_empty() {
        let joined = names.join(", ");
        let _ = write!(out, " {}", l.ta("v-rr-on-days", &[("days", joined.into())]));
    } else if matches!(freq, "MONTHLY" | "YEARLY") {
        if let (Some(o), Some(day)) = (ord, names.first()) {
            let which = if o == -1 { l.t("v-last") } else { l.ta("v-rr-ordinal", &[("n", i64::from(o).into())]) };
            let _ = write!(out, " {}", l.ta("v-rr-on-nth", &[("which", which.into()), ("day", day.clone().into())]));
        } else if let Ok(d) = get("BYMONTHDAY").parse::<u32>() {
            let _ = write!(out, " {}", l.ta("v-rr-on-monthday", &[("d", i64::from(d).into())]));
        } else if freq == "YEARLY" {
            let _ = write!(out, " {}", l.ta("v-rr-on-date", &[("d", i64::from(start.day()).into()), ("month", l.mon(start.month()).into())]));
        }
        if freq == "YEARLY" && ord.is_some() {
            if let Ok(mo) = get("BYMONTH").parse::<u32>() {
                let _ = write!(out, " {}", l.ta("v-rr-in-month", &[("month", l.mon(mo).into())]));
            }
        }
    }
    if let Ok(c) = get("COUNT").parse::<u32>() {
        let _ = write!(out, ", {}", l.tn("v-rr-count", c));
    } else if get("UNTIL").len() >= 8 {
        let u = get("UNTIL");
        if let Ok(d) = NaiveDate::parse_from_str(&u[..8], "%Y%m%d") {
            let _ = write!(out, ", {}", l.ta("v-rr-until", &[("date", l.date(d).into())]));
        }
    }
    out
}

/// The rows under an entry of the day document: one per kind of detail, icon in the left column.
/// Values that carry no information are left out — a default visibility on every entry is noise.
fn day_facts(i: &Instance, d: Option<&EventDetail>, l: &L) -> String {
    let mut rows = String::new();
    // the icon comes in ready-made: the calendar's carries the event's colour
    let mut row = |icon: String, inner: String, cls: &str| {
        let _ = write!(rows, "{icon}<div class=\"fv{cls}\">{inner}</div>");
    };
    // the calendar leads the list
    row(ico_c("calendar", &i.color), format!("<span>{}</span>", e(&i.cal_name)), "");
    let geo = d.and_then(|x| x.geo);
    if !i.location.is_empty() || geo.is_some() {
        let mut v = String::new();
        if !i.location.is_empty() {
            let _ = write!(v, "<button type=\"button\" class=\"cp\" data-copytext=\"{0}\" title=\"{1}\">{2}{0}</button>", e(&i.location), l.t("v-copy-address"), ico("copy"));
        }
        if let Some((lat, lon)) = geo {
            let c = format!("{lat:.5}, {lon:.5}");
            let _ = write!(v, "<button type=\"button\" class=\"cp\" data-copytext=\"{c}\" title=\"{}\">{}{c}</button>", l.t("v-copy-coordinates"), ico("copy"));
        }
        // the map query: coordinates when we have them, the address otherwise
        let q = geo.map(|(a, b)| format!("{a:.6},{b:.6}")).unwrap_or_else(|| i.location.clone());
        let _ = write!(v, "<div class=\"maps\"><button type=\"button\" class=\"mb\" data-map=\"osm\" data-q=\"{0}\">{1}OpenStreetMap</button><button type=\"button\" class=\"mb\" data-map=\"gmaps\" data-q=\"{0}\">{1}Google Maps</button></div>", e(&q), ico("map"));
        row(ico("pin"), v, " loc");
    }
    if !i.alarms.is_empty() {
        row(ico("bell"), i.alarms.iter().map(|m| format!("<span>{}</span>", e(&alarm_label(*m, l)))).collect::<String>(), "");
    }
    if let Some(r) = d.and_then(|x| x.rrule.as_deref()) {
        row(ico("repeat"), format!("<span>{}</span>", e(&rrule_text(r, i.start_date(), l))), "");
    } else if i.recurring {
        row(ico("repeat"), format!("<span>{}</span>", l.t("v-recurring")), "");
    }
    if !i.categories.is_empty() {
        row(ico("tag"), i.categories.iter().map(|c| format!("<span>{}</span>", e(c))).collect::<String>(), "");
    }
    let mut marks = String::new();
    let mut mark = |icon: &str, text: String| {
        let _ = write!(marks, "<span class=\"st\">{}{}</span>", ico(icon), e(&text));
    };
    match d.map(|x| x.status.as_str()).unwrap_or(if i.cancelled { "CANCELLED" } else { "" }) {
        "CANCELLED" => mark("thumbs-down", l.t("v-cancelled")),
        "TENTATIVE" => mark("thumbs-mid", l.t("v-tentative")),
        _ => {}
    }
    if d.map(|x| x.transp.as_str()) == Some("TRANSPARENT") {
        mark("free", l.t("v-shown-free"));
    }
    match d.map(|x| x.class.as_str()).unwrap_or("") {
        "PRIVATE" => mark("lock", l.t("v-private")),
        "CONFIDENTIAL" => mark("agent", l.t("v-confidential")),
        _ => {}
    }
    if let Some(p) = d.and_then(|x| x.priority).filter(|p| (1..=9).contains(p)) {
        let (icon, text) = match p {
            1..=4 => ("prio-high", l.t("v-high-priority")),
            5 => ("prio-mid", l.t("v-medium-priority")),
            _ => ("prio-low", l.t("v-low-priority")),
        };
        mark(icon, text);
    }
    if !marks.is_empty() {
        row(ico("flag"), marks, "");
    }
    if let Some(links) = d.map(|x| x.links.as_slice()).filter(|x| !x.is_empty()) {
        let mut v = String::new();
        for (n, k) in links.iter().enumerate() {
            let icon = match k.kind.as_str() { "conference" => "users", "attach" => "download", _ => "globe" };
            let label = if !k.label.is_empty() { k.label.clone() } else if !k.filename.is_empty() { k.filename.clone() } else if !k.uri.is_empty() { short_uri(&k.uri) } else { l.t("v-attachment").to_string() };
            // an embedded file has no URI of its own — it is served from the event
            let href = if k.embedded { format!("/api/events/{}/{}/{}/attach/{}", i.principal, i.cal_id, i.object_id, k.bin_index.unwrap_or(n)) } else { k.uri.clone() };
            // a script URI keeps its row and title, but is no link
            let href_attr = if safe_href(&href) { format!(" href=\"{}\"", e(&href)) } else { String::new() };
            let _ = write!(v, "<a class=\"lk\"{href_attr} target=\"_blank\" rel=\"noopener\" title=\"{}\">{}{}</a>", e(&href), ico(icon), e(&label));
        }
        row(ico("ext"), v, "");
    }
    if rows.is_empty() { String::new() } else { format!("<div class=\"facts\">{rows}</div>") }
}

/// A URI as a label: host plus a hint of the path, without scheme noise.
fn short_uri(u: &str) -> String {
    let rest = u.split_once("//").map(|(_, r)| r).unwrap_or(u);
    let rest = rest.trim_end_matches('/');
    if rest.chars().count() <= 42 { rest.to_string() } else { rest.chars().take(41).collect::<String>() + "…" }
}

/// Day view ("Ledger"): dense day column of the month + the selected day as a document.
/// Days the day view's ledger holds before and after the selected day; the client extends it from
/// there (app.js `mountLedger`), so the window only has to fill the column.
pub const DAY_BACK: i64 = 30;
pub const DAY_AHEAD: i64 = 30;

/// [start, end) of the ledger's initial days.
pub fn day_window(view_date: NaiveDate) -> (NaiveDate, NaiveDate) {
    (view_date - Duration::days(DAY_BACK), view_date + Duration::days(DAY_AHEAD + 1))
}

/// `count` ledger rows from `start`. The selection is the client's business once the ledger runs
/// endlessly, so it is only marked on the rows the page itself brings.
pub fn day_rows(start: NaiveDate, count: usize, today: NaiveDate, sel: Option<NaiveDate>, instances: &[Instance], l: &L, hol: &Hols) -> String {
    let idx = by_day(instances);
    let mut out = String::new();
    for i in 0..count {
        let d = start + Duration::days(i as i64);
        let evs = idx.get(&d).cloned().unwrap_or_default();
        // a public holiday leads the row's text, a school-holiday block only on its first day
        // (the others are tinted); the entries follow
        let marks = hol.get(&d).map(|v| v.as_slice()).unwrap_or(&[]);
        let names: Vec<String> = joined(marks.iter().filter(|m| !m.school)).into_iter().chain(joined(marks.iter().filter(|m| m.school && m.start == d))).map(|n| e(&n)).chain(evs.iter().map(|i| e(&i.summary))).collect();
        let _ = write!(out, "<a class=\"lrow{}{}{}{}\" href=\"/calendar?view=day&date={d}\" data-date=\"{d}\"><b>{:02}</b><span>{}</span><i style=\"--n:{}\"></i><em>{}</em>{ADD}</a>", if d == today { " today" } else { "" }, if sel == Some(d) { " sel" } else { "" }, if d.weekday().num_days_from_monday() >= 5 { " we" } else { "" }, &hol_class(hol, d), d.day(), l.dow(d), evs.len().min(6), names.join(", "));
    }
    out
}

pub fn day_view(view_date: NaiveDate, today: NaiveDate, instances: &[Instance], l: &L, cal_names: &str, details: &HashMap<String, EventDetail>, hol: &Hols) -> String {
    let idx = by_day(instances);
    let (ds, de) = day_window(view_date);
    let led = day_rows(ds, (de - ds).num_days() as usize, today, Some(view_date), instances, l, hol);
    let evs = idx.get(&view_date).cloned().unwrap_or_default();
    let mut doc = format!("<section class=\"doc\" data-date=\"{view_date}\"><h2>{} {ADD}</h2><div class=\"sub\">{} · {}</div>{}", e(&l.long_date(view_date)), l.tn("v-n-events", evs.len()), e(cal_names), { let m = hol_labels(hol, view_date, "span", true); if m.is_empty() { m } else { format!("<div class=\"hols\">{m}</div>") } });
    if evs.is_empty() {
        let _ = write!(doc, "<p class=\"muted\">{}</p>", l.t("v-no-events-on-this-day"));
    }
    for i in &evs {
        let time = if i.all_day { l.t("v-all-day").to_string() } else if i.end > i.start { format!("{}–{}", i.time_label(), i.end_label()) } else { i.time_label() };
        let _ = write!(doc, "<article class=\"lnk\" data-key=\"{}\" data-date=\"{}\"><time>{time}</time><div><h3>{}</h3>{}{}</div></article>",
            e(&i.key()), i.start_date(), e(&i.summary),
            if i.description.is_empty() { String::new() } else { format!("<p>{}</p>", e(&i.description)) },
            day_facts(i, details.get(&i.key()), l));
    }
    doc.push_str("</section>");
    // the ledger's head names the month of its topmost row — an endless day list needs it (app.js `mountLedger`)
    format!("<div class=\"cal v-day on\"><div class=\"ledger\"><div class=\"lhd\">{} {}</div><div class=\"lrows\">{led}</div></div>{doc}{}</div>", l.mon(view_date.month()), view_date.year(), tl_weeks(view_date, today))
}

pub fn alarm_label(m: i64, l: &L) -> String {
    if m == 0 {
        return l.t("v-at-start").to_string();
    }
    // the offset counts backwards from the start, so a negative one fires after it
    let (n, after) = (m.abs(), m < 0);
    let amount = if n % 1440 == 0 {
        l.ta("v-n-days", &[("n", (n / 1440).into())])
    } else if n % 60 == 0 {
        l.ta("v-n-hours", &[("n", (n / 60).into())])
    } else {
        l.ta("v-n-minutes", &[("n", n.into())])
    };
    format!("{amount} {}", if after { l.t("v-after") } else { l.t("v-before") })
}

/// `view_date` picks the month, `picked` the day marked in it.
pub fn month_view(view_date: NaiveDate, picked: NaiveDate, today: NaiveDate, instances: &[Instance], l: &L, agenda_date: NaiveDate, hol: &Hols) -> String {
    let grid = month_grid(view_date.year(), view_date.month(), today, instances, l, 3, picked, hol);
    let title = if agenda_date == today { format!("{}, {}", l.t("v-today"), l.long_date(agenda_date)) } else { l.long_date(agenda_date) };
    format!("<div class=\"cal v-month on\">{grid}{}{}</div>", tl_months(view_date, today), agenda(agenda_date, instances, l, &title, hol))
}

#[cfg(test)]
mod pinned;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_json_cannot_close_its_block() {
        let j = script_json(serde_json::json!({ "title": "</script><script>alert(1)</script>" }).to_string());
        assert!(!j.contains('<'));
        let back: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(back["title"], "</script><script>alert(1)</script>");
    }

    #[test]
    fn script_uris_are_no_hrefs() {
        for ok in ["https://example.org/a?b=1&c=2", "http://x", "mailto:a@b.de", "tel:+4912345", "geo:50.1,8.6", "zoommtg://zoom.us/join?confno=1", "sip:a@b", "relative/path", "", "Javascript-guide.html"] {
            assert!(safe_href(ok), "{ok}");
        }
        for bad in ["javascript:alert(1)", "JavaScript:alert(1)", " javascript:alert(1)", "\u{1}javascript:x", "java\tscript:x", "jav\nascript:x", "vbscript:msgbox(1)", "data:text/html,<script>alert(1)</script>", "DATA:text/html;base64,PHNjcmlwdD4="] {
            assert!(!safe_href(bad), "{bad:?}");
        }
    }

    /// Every text of an event comes from any CalDAV client or a shared group calendar: each view
    /// prints it as text, and a script URI among its links is no link.
    #[test]
    fn foreign_event_data_stays_text() {
        use crate::domain::events::Link;
        let x = "<img src=x onerror=alert(1)>\"'&";
        let d = NaiveDate::from_ymd_opt(2026, 9, 15).unwrap();
        let i = Instance {
            principal: "p".into(), cal_id: "c".into(), cal_name: x.into(), color: "#112233".into(), object_id: "o\"><xss>.ics".into(), uid: x.into(),
            summary: x.into(), location: x.into(), description: x.into(), start: d.and_hms_opt(9, 0, 0).unwrap(), end: d.and_hms_opt(10, 0, 0).unwrap(),
            all_day: false, recurring: false, categories: vec![x.into()], alarm_minutes: None, alarms: vec![], is_group: true, cancelled: false,
        };
        let link = |uri: &str| Link { kind: "url".into(), uri: uri.into(), label: x.into(), ..Default::default() };
        let det = EventDetail {
            principal: "p".into(), cal_id: "c".into(), object_id: i.object_id.clone(), uid: x.into(), summary: x.into(), location: x.into(), description: x.into(),
            start: i.start, end: i.end, all_day: false, rrule: None, rrule_until: None, exdates: vec![], rdates: vec![], organizer: None, attendees: vec![], alarms: vec![],
            categories: vec![x.into()], status: String::new(), transp: String::new(), class: String::new(), priority: None, color: String::new(), geo: None,
            links: vec![link("javascript:alert(1)"), link(" JAVA\tSCRIPT:alert(1)"), link("data:text/html,<script>alert(1)</script>"), link("https://example.org/?a=1&b=2")],
            related: vec![], etag: String::new(), has_overrides: false, timezone: String::new(),
        };
        let l = L::new("de", crate::i18n::I18n::load());
        let details = HashMap::from([(i.key(), det)]);
        let insts = [i];
        for html in [month_view(d, d, d, &insts, &l, d, &HashMap::new()), week_view(d, d, &insts, &l, &HashMap::new()), day_view(d, d, &insts, &l, x, &details, &HashMap::new())] {
            assert!(!html.contains("<img") && !html.contains("<xss") && !html.contains("<script"), "{html}");
            for (at, _) in html.match_indices("href=\"") {
                let v = &html[at + 6..];
                assert!(v.starts_with('/') || v.starts_with('#') || v.starts_with("https://example.org/"), "{}", &v[..v.len().min(40)]);
            }
        }
        let day = day_view(d, d, &insts, &l, x, &details, &HashMap::new());
        assert!(day.contains("&lt;img src=x onerror=alert(1)&gt;&quot;&#39;&amp;"));
        // the three script links are kept as rows without href, the web link stays one
        assert_eq!(day.matches("<a class=\"lk\" target=").count(), 3);
        assert!(day.contains("<a class=\"lk\" href=\"https://example.org/?a=1&amp;b=2\" target=\"_blank\""));
    }

    #[test]
    fn weeks_of_september_2026() {
        let w = month_weeks(2026, 9);
        assert_eq!(w.len(), 5);
        assert_eq!(w[0][0], NaiveDate::from_ymd_opt(2026, 8, 31).unwrap());
        assert_eq!(w[4][6], NaiveDate::from_ymd_opt(2026, 10, 4).unwrap());
        assert_eq!(week_of(NaiveDate::from_ymd_opt(2026, 9, 15).unwrap())[0], NaiveDate::from_ymd_opt(2026, 9, 14).unwrap());
    }

    #[test]
    fn same_named_blocks_share_one_label() {
        use crate::domain::holidays::Mark;
        let d = NaiveDate::from_ymd_opt(2026, 10, 5).unwrap();
        let m = |name: &str, tags: &[&str]| Mark { name: name.into(), tags: tags.iter().map(|t| t.to_string()).collect(), school: true, start: d };
        let v = [m("Herbstferien", &["HE"]), m("Herbstferien", &["SH"]), m("Herbstferien", &["RP", "SL"]), m("Brückentag", &["SN"])];
        assert_eq!(joined(v.iter()), ["Herbstferien (HE, SH, RP, SL)", "Brückentag (SN)"]);
        // one of them applies everywhere: the name alone
        let v = [m("Herbstferien", &["HE"]), m("Herbstferien", &[])];
        assert_eq!(joined(v.iter()), ["Herbstferien"]);
    }

    #[test]
    fn month_names_a_school_block_on_its_first_day_that_is_no_public_holiday() {
        use crate::domain::holidays::Mark;
        let d = |m, day| NaiveDate::from_ymd_opt(2026, m, day).unwrap();
        let mut hol: Hols = HashMap::new();
        // Weihnachtsferien from 25.12., which is also a public holiday; the block goes on into January
        for day in 25..=31 {
            hol.entry(d(12, day)).or_default().push(Mark { name: "Weihnachtsferien".into(), tags: vec![], school: true, start: d(12, 25) });
        }
        hol.entry(d(12, 25)).or_default().push(Mark { name: "1. Weihnachtsfeiertag".into(), tags: vec![], school: false, start: d(12, 25) });
        let l = L::new("de", crate::i18n::I18n::load());
        let html = month_grid(2026, 12, d(9, 1), &[], &l, 3, d(12, 1), &hol);
        let tile = |day: u32| { let k = format!("data-date=\"{}\"", d(12, day)); let i = html.find(&k).unwrap(); html[i..].split("<div class=\"day").next().unwrap().to_string() };
        assert!(tile(25).contains("fname pub\"") && tile(25).contains(">1. Weihnachtsfeiertag<") && !tile(25).contains(">Weihnachtsferien<"));
        assert!(tile(26).contains("class=\"fname\"") && tile(26).contains(">Weihnachtsferien<"));
        assert!(!tile(27).contains("fname"));
        assert!(html.contains("day hol fer\" data-date=\"2026-12-25"), "a public holiday that is also a school-holiday day is both, `hol` wins in the CSS");
    }
}
