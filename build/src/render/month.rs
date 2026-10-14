//! Month view: week rows, the grid with event chips, the agenda, the timeline and the date picker.

use super::*;

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


/// The instances by each day of [from, to) they touch — only the days the caller looks up: one entry per
/// covered day of an all-day event running until 9999 took a month view to 505 MiB.
pub fn by_day(instances: &[Instance], from: NaiveDate, to: NaiveDate) -> HashMap<NaiveDate, Vec<&Instance>> {
    let mut m: HashMap<NaiveDate, Vec<&Instance>> = HashMap::new();
    for i in instances {
        let mut d = i.start_date().max(from);
        let last = i.last_date().min(to.pred_opt().unwrap_or(to));
        while d <= last && d < to {
            m.entry(d).or_default().push(i);
            d += Duration::days(1);
        }
    }
    m
}

/// The "+" of a day (a new event on it): the sign stays "+", its accessible name says what it does.
pub fn add_btn(l: &L) -> String {
    format!("<button class=\"add\" type=\"button\" title=\"+\" aria-label=\"{}\">+</button>", e(&l.t("ed-title-new")))
}

/// What calendar.js's drag and drop needs beyond key and start day: a multi-day entry's last day (the preview covers
/// it), and a series occurrence's local start (`/move` moves that one alone when asked to; `events::move_event`).
pub(super) fn drag_attrs(out: &mut String, i: &Instance) {
    if i.multi_day() {
        let _ = write!(out, " data-last=\"{}\"", i.last_date());
    }
    if i.recurring {
        let _ = write!(out, " data-at=\"{}\"", i.start.format("%Y-%m-%dT%H:%M:%S"));
    }
}

pub(super) fn ev_html(out: &mut String, i: &Instance, role: Option<&str>) {
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
    let _ = write!(out, "\" style=\"--c:{}\" data-key=\"{}\" data-date=\"{}\"", E(&i.color), EKey(i), i.start_date());
    drag_attrs(out, i);
    let _ = write!(out, " title=\"{}\">{}", E(&i.summary), E(&i.summary));
    if !i.all_day {
        let _ = write!(out, " {}", i.start.format("%H:%M"));
    }
    out.push_str("</div>");
}

/// Month grid with continuous multi-day bars (lane-aligned per week row).
pub fn month_grid(year: i32, month: u32, today: NaiveDate, instances: &[Instance], l: &L, ev_limit: usize, view_date: NaiveDate, hol: &Hols) -> String {
    let weeks = month_weeks(year, month);
    let idx = by_day(instances, weeks[0][0], weeks[weeks.len() - 1][6] + Duration::days(1));
    let more = e(&l.t("v-more")); // the word after "+N", here and for calendar.js dayFit
    let mut out = format!("<div class=\"month\" data-plus=\"{more}\">");
    let add = add_btn(l);
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
            let _ = write!(out, "<div class=\"{cls}\" data-date=\"{d}\">{num}{add}<div class=\"dots\">");
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
            // every single is here; the "+N" (N ≥ 2) takes the last slot when they do not all fit, the singles past it
            // are hidden (`.xt`). This guess counts `ev_limit` slots; calendar.js dayFit redoes it from the tile's real
            // height (its "+N" line is smaller than a chip, so it may say "+1"), and hovering the tile, focus in it or a click on the "+N" unfolds it (dayOpen)
            let singles: Vec<&&Instance> = evs.iter().filter(|i| !i.multi_day()).collect();
            let room = ev_limit.saturating_sub(shown);
            // (no room at all for a lone single: it is shown, there is nothing its "+1" would stand in for)
            let fit = if singles.len() > room && singles.len() - room.saturating_sub(1) > 1 { room.saturating_sub(1) } else { singles.len() };
            for (n, i) in singles.iter().enumerate() {
                let at = out.len();
                ev_html(&mut out, i, None);
                if n >= fit {
                    out.insert_str(at + "<div class=\"ev".len(), " xt");
                }
            }
            if fit < singles.len() {
                let _ = write!(out, "<button class=\"dmore\" type=\"button\" aria-expanded=\"false\">+{} {more}</button>", singles.len() - fit);
            }
            out.push_str("</div>");
        }
    }
    out.push_str("</div>");
    out
}

pub fn agenda(date: NaiveDate, instances: &[Instance], l: &L, title: &str, hol: &Hols) -> String {
    let idx = by_day(instances, date, date + Duration::days(1));
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

/// `view_date` picks the month, `picked` the day marked in it.
pub fn month_view(view_date: NaiveDate, picked: NaiveDate, today: NaiveDate, instances: &[Instance], l: &L, agenda_date: NaiveDate, hol: &Hols) -> String {
    let grid = month_grid(view_date.year(), view_date.month(), today, instances, l, 3, picked, hol);
    let title = if agenda_date == today { format!("{}, {}", l.t("v-today"), l.long_date(agenda_date)) } else { l.long_date(agenda_date) };
    format!("<div class=\"cal v-month on\">{grid}{}{}</div>", tl_months(view_date, today), agenda(agenda_date, instances, l, &title, hol))
}
