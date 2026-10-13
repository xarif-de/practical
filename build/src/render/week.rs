//! Week view: the window around a week and its day blocks.

use super::*;

/// Week view ("Stream"): every week of the month, active week tinted, left heat map.
/// Weeks the week view renders before its focus week and after it; the instance window must cover
/// `week_window`, the client extends the stream from there (app.js `mountWeeks`).
pub const WEEK_BACK: i64 = 4;
pub const WEEK_AHEAD: i64 = 5;

/// The outermost lane a multi-day bar takes in `week_blocks` (chrome.css `--ls` 16px each: 4 lanes = 64px).
const MAX_LANE: usize = 3;

/// A multi-day event's stretch within one week of `week_blocks`: day indexes 0-6 and its lane.
struct Run<'a> {
    ev: &'a Instance,
    first: usize,
    last: usize,
    lane: usize,
}

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
    let (mon, today_mon) = (monday_of(start), monday_of(today));
    let idx = by_day(instances, mon, mon + Duration::days(7 * count as i64));
    let mut out = String::with_capacity(count * 4096);
    let add = add_btn(l);
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
        let days: Vec<Vec<&Instance>> = (0..7).map(|i| idx.get(&(w0 + Duration::days(i))).cloned().unwrap_or_default()).collect();
        // a multi-day event's bar stands on its first day of the week only; a line (chrome.css `.ln`) runs down from
        // the bar's right end through its last day of the week. A bar takes lane 1 + the highest lane whose line
        // passes its row, and ends that many lane steps short of the right edge: no line ever crosses a bar (up to
        // MAX_LANE; past it lanes are shared, so a crowded week keeps its bars readable). Its later days name it for
        // screen readers only (`.vh`), which also keeps the density panel's count (calendar.js `heatSync`, one per `li`).
        let mut runs: Vec<Run> = vec![];
        for (di, evs) in days.iter().enumerate() {
            let d = w0 + Duration::days(di as i64);
            let mut items = String::new();
            for i in evs {
                if !i.multi_day() {
                    items.push_str("<li><time>");
                } else if runs.iter().any(|r| std::ptr::eq(r.ev, *i)) {
                    let _ = write!(items, "<li class=\"vh\">{}</li>", E(&i.summary));
                    continue;
                } else {
                    let last = (di..7).take_while(|&j| days[j].iter().any(|x| std::ptr::eq(*x, *i))).last().unwrap_or(di);
                    let k = runs.iter().filter(|r| r.last > di || (r.first < di && r.last == di)).map(|r| r.lane + 1).max().unwrap_or(0).min(MAX_LANE);
                    runs.push(Run { ev: i, first: di, last, lane: k });
                    let _ = write!(items, "<li class=\"mday{}\" style=\"--k:{k}\"><time>", if last > di { " run" } else { "" });
                }
                if i.all_day {
                    items.push_str(&all_day);
                } else {
                    let _ = write!(items, "{}", i.start.format("%H:%M"));
                }
                // the start date only where it is not the day's own (a multi-day bar again on a later week's Monday): app.js
                // reads the nearest data-date, the day block's (−18 % of the week's gzip bytes)
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
            // the lines passing the whole day, and the lanes the day's other entries keep clear (--ln)
            let mut lines = String::new();
            for r in runs.iter().filter(|r| r.first < di && r.last >= di) {
                let _ = write!(lines, "<i class=\"ln{}{}\" style=\"--k:{};--c:{}\"></i>", if r.last == di { " end" } else { "" }, if r.ev.cancelled { " cancelled" } else { "" }, r.lane, E(&r.ev.color));
            }
            let ln = runs.iter().filter(|r| r.first <= di && r.last >= di && r.last > r.first).map(|r| r.lane + 1).max().unwrap_or(0);
            let _ = write!(out, "<section class=\"dayblock{}{}{}\" data-date=\"{d}\"", if d == today { " today" } else { "" }, if evs.is_empty() && marks.is_empty() { " empty" } else { "" }, &hol_class(hol, d));
            if ln > 0 {
                let _ = write!(out, " style=\"--ln:{ln}\"");
            }
            let _ = write!(out, "><div class=\"dl\"><b>{}</b><span>{}</span></div><ul>{items}</ul>{lines}{add}</section>", d.day(), l.dow(d));
        }
        out.push_str("</div>");
    }
    out
}

pub fn week_view(view_date: NaiveDate, today: NaiveDate, instances: &[Instance], l: &L, hol: &Hols) -> String {
    let (start, end) = week_window(view_date);
    let count = ((end - start).num_days() / 7) as usize;
    let stream = week_blocks(start, count, today, instances, l, hol);
    // the panel follows the focus week's month, the same rule the client applies while scrolling
    let focus = monday_of(view_date);
    let (hy, hm) = (focus.year(), focus.month());
    let hw = month_weeks(hy, hm);
    let idx = by_day(instances, hw[0][0], hw[hw.len() - 1][6] + Duration::days(1));
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
    format!("<div class=\"cal v-week on\" data-focus=\"{focus}\"><div class=\"heat\" data-month=\"{hy}-{hm:02}\"><h2>{} <span class=\"hm\">{} {hy}</span></h2><div class=\"grid\">{heat}</div></div><div class=\"stream\">{stream}</div>{}</div>", l.t("v-density"), l.mon(hm), tl_weeks(view_date, today))
}
