//! Day view: the window, the rows and the facts shown per event.

use super::*;

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
        let q = geo.map(|(a, b)| format!("{a:.6},{b:.6}")).unwrap_or_else(|| i.location.to_string());
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
    // every CLASS but PUBLIC and CONFIDENTIAL is private to sharees (events::shown_class)
    match d.map(|x| x.class.as_str()).unwrap_or("") {
        "" | "PUBLIC" => {}
        "CONFIDENTIAL" => mark("agent", l.t("v-confidential")),
        _ => mark("lock", l.t("v-private")),
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
pub(super) fn short_uri(u: &str) -> String {
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
    let idx = by_day(instances, start, start + Duration::days(count as i64));
    let mut out = String::new();
    let add = add_btn(l);
    for i in 0..count {
        let d = start + Duration::days(i as i64);
        let evs = idx.get(&d).cloned().unwrap_or_default();
        // a public holiday leads the row's text, a school-holiday block only on its first day
        // (the others are tinted); the entries follow
        let marks = hol.get(&d).map(|v| v.as_slice()).unwrap_or(&[]);
        let names: Vec<String> = joined(marks.iter().filter(|m| !m.school)).into_iter().chain(joined(marks.iter().filter(|m| m.school && m.start == d))).map(|n| e(&n)).chain(evs.iter().map(|i| e(&i.summary))).collect();
        let _ = write!(out, "<a class=\"lrow{}{}{}{}\" href=\"/calendar?view=day&date={d}\" data-date=\"{d}\"><b>{:02}</b><span>{}</span><i style=\"--n:{}\"></i><em>{}</em>{add}</a>", if d == today { " today" } else { "" }, if sel == Some(d) { " sel" } else { "" }, if d.weekday().num_days_from_monday() >= 5 { " we" } else { "" }, &hol_class(hol, d), d.day(), l.dow(d), evs.len().min(6), names.join(", "));
    }
    out
}

pub fn day_view(view_date: NaiveDate, today: NaiveDate, instances: &[Instance], l: &L, cal_names: &str, details: &HashMap<String, EventDetail>, hol: &Hols) -> String {
    let idx = by_day(instances, view_date, view_date + Duration::days(1));
    let (ds, de) = day_window(view_date);
    let led = day_rows(ds, (de - ds).num_days() as usize, today, Some(view_date), instances, l, hol);
    let evs = idx.get(&view_date).cloned().unwrap_or_default();
    // the notes up to `NOTES_PER_RESPONSE` (`events::limit_notes`); the entries all stay
    let mut notes_left = crate::domain::events::NOTES_PER_RESPONSE;
    let add = add_btn(l);
    let mut doc = format!("<section class=\"doc\" data-date=\"{view_date}\"><h2>{} {add}</h2><div class=\"sub\">{} · {}</div>{}", e(&l.long_date(view_date)), l.tn("v-n-events", evs.len()), e(cal_names), { let m = hol_labels(hol, view_date, "span", true); if m.is_empty() { m } else { format!("<div class=\"hols\">{m}</div>") } });
    if evs.is_empty() {
        let _ = write!(doc, "<p class=\"muted\">{}</p>", l.t("v-no-events-on-this-day"));
    }
    for i in &evs {
        let time = if i.all_day { l.t("v-all-day").to_string() } else if i.end > i.start { format!("{}–{}", i.time_label(), i.end_label()) } else { i.time_label() };
        let _ = write!(doc, "<article class=\"lnk\" data-key=\"{}\" data-date=\"{}\"><time>{time}</time><div><h3>{}</h3>{}{}</div></article>",
            e(&i.key()), i.start_date(), e(&i.summary),
            if i.description.is_empty() || i.description.len() > notes_left { String::new() } else { notes_left -= i.description.len(); format!("<p>{}</p>", e(&i.description)) },
            day_facts(i, details.get(&i.key()), l));
    }
    doc.push_str("</section>");
    // the ledger's head names the month of its topmost row — an endless day list needs it (app.js `mountLedger`)
    format!("<div class=\"cal v-day on\"><div class=\"ledger\"><div class=\"lhd\">{} {}</div><div class=\"lrows\">{led}</div></div>{doc}{}</div>", l.mon(view_date.month()), view_date.year(), tl_weeks(view_date, today))
}
