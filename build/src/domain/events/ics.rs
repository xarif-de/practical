//! Raw access to the stored object — whatever the editor does not model survives a save: component split,
//! VTIMEZONE blocks, unfolding, and building the VEVENT/VCALENDAR text.

use super::*;

/// The master VEVENT's lines outside its VALARMs (unfolded, in order) and its VALARM blocks.
#[derive(Debug, Default)]
pub struct RawMaster {
    pub props: Vec<String>,
    pub alarms: Vec<Vec<String>>,
}

/// Properties `build_vevent` writes itself; every other master line is carried over verbatim.
const MANAGED: &[&str] = &["UID", "DTSTAMP", "SEQUENCE", "LAST-MODIFIED", "SUMMARY", "DTSTART", "DTEND", "DURATION", "LOCATION", "DESCRIPTION", "CATEGORIES", "RRULE", "EXDATE", "RDATE", "ORGANIZER", "ATTENDEE", "STATUS", "TRANSP", "CLASS", "PRIORITY", "URL", "COLOR", "GEO", "ATTACH", "CONFERENCE", "RELATED-TO"];

/// A line `build_vevent` rewrites; binary ATTACHs are the exception and stay verbatim.
pub(super) fn is_managed(line: &str) -> bool {
    let name = prop_name(line);
    if name == "ATTACH" {
        let (_, params, _) = ical::split_line(line);
        return !is_binary_attach(&params);
    }
    MANAGED.contains(&name.as_str())
}

pub(super) const ALARM_MANAGED: &[&str] = &["ACTION", "TRIGGER", "DESCRIPTION", "SUMMARY", "DURATION", "REPEAT", "PROXIMITY", "ACKNOWLEDGED"];

pub fn prop_name(line: &str) -> String {
    line.split([';', ':']).next().unwrap_or("").to_ascii_uppercase()
}

/// Splits a stored object into its master VEVENT and the RECURRENCE-ID override blocks
/// (verbatim, folded again) — those come from other clients and are never rewritten here.
pub fn split_ics(ics: &str) -> (RawMaster, Vec<String>) {
    split_component(ics, "VEVENT")
}

/// The VTIMEZONE blocks of a stored object, verbatim — a carried-over property may still
/// reference one by TZID, and a dangling TZID is an invalid object.
pub fn vtimezone_blocks(ics: &str) -> Vec<String> {
    let mut out = vec![];
    let mut block: Vec<String> = vec![];
    let mut depth = 0;
    for line in unfold(&ics.replace("\r\n", "\n")) {
        if line.starts_with("BEGIN:VTIMEZONE") {
            depth = 1;
            block = vec![line];
            continue;
        }
        if depth == 0 {
            continue;
        }
        if line.starts_with("BEGIN:") {
            depth += 1;
        } else if line.starts_with("END:") {
            depth -= 1;
        }
        block.push(line);
        if depth == 0 {
            out.push(block.iter().map(|l| ical::fold(l)).collect::<Vec<_>>().join("\r\n"));
        }
    }
    out
}

/// Same for any calendar component — `VTODO` uses it to keep whatever the task editor does not model.
pub fn split_component(ics: &str, comp: &str) -> (RawMaster, Vec<String>) {
    let begin = format!("BEGIN:{comp}");
    let end = format!("END:{comp}");
    let unfolded = unfold(&ics.replace("\r\n", "\n"));
    let mut master = RawMaster::default();
    let mut overrides = vec![];
    let mut block: Vec<String> = vec![];
    let mut depth = 0;
    for line in &unfolded {
        if line.starts_with(&begin) && depth == 0 {
            depth = 1;
            block.clear();
            continue;
        }
        if depth == 0 {
            continue;
        }
        if line.starts_with("BEGIN:") {
            depth += 1;
        } else if line.starts_with("END:") {
            depth -= 1;
            if depth == 0 {
                if block.iter().any(|l| l.starts_with("RECURRENCE-ID")) {
                    let folded: Vec<String> = std::iter::once(begin.clone()).chain(block.iter().cloned()).chain(std::iter::once(end.clone())).map(|l| ical::fold(&l)).collect();
                    overrides.push(folded.join("\r\n"));
                } else if master.props.is_empty() && master.alarms.is_empty() {
                    let mut alarm: Option<Vec<String>> = None;
                    for l in &block {
                        if l.starts_with("BEGIN:VALARM") {
                            alarm = Some(vec![]);
                        } else if l.starts_with("END:VALARM") {
                            master.alarms.push(alarm.take().unwrap_or_default());
                        } else if let Some(a) = alarm.as_mut() {
                            a.push(l.clone());
                        } else {
                            master.props.push(l.clone());
                        }
                    }
                }
                continue;
            }
        }
        block.push(line.clone());
    }
    (master, overrides)
}

/// Builds the VEVENT lines (without BEGIN/END) for the master event. With `raw` (an update)
/// every stored line the editor does not model is carried over verbatim — properties like
/// ATTENDEE, STATUS, ATTACH, COLOR, EXDATE, X-… and the unmodelled parts of each VALARM.
pub(super) fn build_vevent(uid: &str, sequence: i64, input: &EventInput, tz: chrono_tz::Tz, raw: Option<&RawMaster>) -> AppResult<Vec<String>> {
    if input.summary.trim().is_empty() {
        return Err(AppError::bad("err-summary-required"));
    }
    if input.end < input.start {
        return Err(AppError::bad("err-end-before-start"));
    }
    if !ical::write_date_ok(input.start.date()) || !ical::write_date_ok(input.end.date()) {
        return Err(AppError::bad("err-date-range"));
    }
    validate_common(&input.summary, &input.location, &input.description, &input.categories, &input.alarms, &input.links, &input.related)?;
    if input.attendees.len() > MAX_LIST || input.exdates.len() > MAX_LIST || input.rdates.len() > MAX_LIST {
        return Err(AppError::bad("err-too-long"));
    }
    let mut l = vec![
        format!("UID:{uid}"),
        format!("DTSTAMP:{}", ical::fmt_utc_now()),
        format!("SEQUENCE:{sequence}"),
        format!("SUMMARY:{}", ical::escape(input.summary.trim())),
    ];
    if input.all_day {
        let s = input.start.date();
        let mut e = input.end.date();
        if e <= s {
            e = s.succ_opt().unwrap_or(s);
        }
        l.push(format!("DTSTART;VALUE=DATE:{}", ical::fmt_date(s)));
        l.push(format!("DTEND;VALUE=DATE:{}", ical::fmt_date(e)));
    } else {
        let e = if input.end <= input.start { input.start.checked_add_signed(Duration::hours(1)).unwrap_or(input.start) } else { input.end };
        l.push(format!("DTSTART;TZID={}:{}", tz.name(), ical::fmt_dt(input.start)));
        l.push(format!("DTEND;TZID={}:{}", tz.name(), ical::fmt_dt(e)));
    }
    if !input.location.trim().is_empty() {
        l.push(format!("LOCATION:{}", ical::escape(input.location.trim())));
    }
    if !input.description.trim().is_empty() {
        l.push(format!("DESCRIPTION:{}", ical::escape(input.description.trim())));
    }
    let cats: Vec<String> = input.categories.iter().map(|c| c.trim()).filter(|c| !c.is_empty()).map(ical::escape).collect();
    if !cats.is_empty() {
        l.push(format!("CATEGORIES:{}", cats.join(",")));
    }
    if let Some(r) = &input.rrule
        && !r.trim().is_empty()
    {
        l.push(format!("RRULE:{}", validate_rrule(&normalize_until(r, input.all_day, tz))?));
        l.extend(date_lines("EXDATE", &input.exdates, input.all_day, tz));
        l.extend(date_lines("RDATE", &input.rdates, input.all_day, tz));
    }
    if let Some(o) = &input.organizer
        && let Some(line) = person_line("ORGANIZER", o)
    {
        l.push(line);
    }
    l.extend(input.attendees.iter().filter_map(|a| person_line("ATTENDEE", a)));
    if let Some(v) = one_of(&input.status, &["CONFIRMED", "TENTATIVE", "CANCELLED"]) {
        l.push(format!("STATUS:{v}"));
    }
    if let Some(v) = one_of(&input.transp, &["OPAQUE", "TRANSPARENT"]) {
        l.push(format!("TRANSP:{v}"));
    }
    if let Some(v) = one_of(&input.class, &["PUBLIC", "PRIVATE", "CONFIDENTIAL"]) {
        l.push(format!("CLASS:{v}"));
    }
    if let Some(p) = input.priority.filter(|p| (0..=9).contains(p)) {
        l.push(format!("PRIORITY:{p}"));
    }
    let color = input.color.trim();
    if valid_color(color) {
        l.push(format!("COLOR:{color}"));
    }
    if let Some((lat, lon)) = input.geo.filter(|(a, b)| (-90.0..=90.0).contains(a) && (-180.0..=180.0).contains(b)) {
        l.push(format!("GEO:{lat:.6};{lon:.6}"));
    }
    l.extend(link_lines(&input.links));
    l.extend(embedded_lines(&input.links)?);
    l.extend(related_lines(&input.related));
    l.push(format!("LAST-MODIFIED:{}", ical::fmt_utc_now()));
    if let Some(r) = raw {
        // binary ATTACHs are kept only when the client sent them back with `keep` (else: removed)
        let keep: HashSet<usize> = input.links.iter().filter(|x| x.embedded && x.keep && x.data.is_none()).filter_map(|x| x.bin_index).collect();
        let mut bin = 0usize;
        // a changed DESCRIPTION makes Outlook's X-ALT-DESC (and STYLED-DESCRIPTION) stale: drop them then
        let stored = r.props.iter().find(|x| prop_name(x) == "DESCRIPTION").map(|x| ical::unescape(ical::split_line(x).2.trim())).unwrap_or_default();
        let desc_changed = stored.trim() != input.description.trim();
        for x in r.props.iter().filter(|x| !is_managed(x)) {
            let name = prop_name(x);
            if desc_changed && (name == "X-ALT-DESC" || name == "STYLED-DESCRIPTION") {
                continue;
            }
            if name == "ATTACH" && is_binary_attach(&ical::split_line(x).1) {
                let i = bin;
                bin += 1;
                if !keep.contains(&i) {
                    continue;
                }
            }
            l.push(x.clone());
        }
    }
    for a in &input.alarms {
        l.extend(build_valarm(a, input.summary.trim(), raw, tz));
    }
    Ok(l)
}


/// Unfolded content lines, each in its canonical case: property names — and the component name of a
/// BEGIN/END line — are case-insensitive (RFC 5545 §2, RFC 6350 §3.3) and come out upper-case, so every
/// `starts_with("BEGIN:VEVENT")` / `prop_name(l) == …` downstream holds for `begin:vevent` too. Values
/// and parameters stay as written.
pub fn unfold(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for l in raw.split('\n') {
        let l = l.trim_end_matches('\r');
        if (l.starts_with(' ') || l.starts_with('\t')) && !out.is_empty() {
            out.last_mut().unwrap().push_str(&l[1..]);
        } else {
            out.push(l.to_string());
        }
    }
    for l in &mut out {
        canon_name(l);
    }
    out
}

/// Upper-cases the name of one unfolded content line (and the value of BEGIN/END) in place.
fn canon_name(l: &mut String) {
    if l.starts_with([' ', '\t']) {
        return;
    }
    let end = l.find([';', ':']).unwrap_or(l.len());
    if l[..end].bytes().any(|b| b.is_ascii_lowercase()) {
        let up = l[..end].to_ascii_uppercase();
        l.replace_range(..end, &up);
    }
    if (l.starts_with("BEGIN:") || l.starts_with("END:")) && l.bytes().any(|b| b.is_ascii_lowercase()) {
        l.make_ascii_uppercase();
    }
}
