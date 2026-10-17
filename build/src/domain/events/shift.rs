//! Moving an event by whole days (the views' drag and drop, `POST /api/events/{p}/{c}/{id}/move`): every date of the
//! stored object is shifted on its own line, wall-clock times stay; nothing else of the object is rewritten.
//! - A plain event or a whole series ([`shift_ics`] without a target): DTSTART/DTEND, EXDATE/RDATE, RRULE (UNTIL; the
//!   BY-parts that name the start's weekday, month day or month follow the new start, [`shift_rrule`]), every
//!   override's RECURRENCE-ID/DTSTART/DTEND, absolute alarm triggers.
//! - One occurrence of a series (a target): its override is shifted, else one is written — the master without its
//!   rule, RECURRENCE-ID in the master's DTSTART form (caldata refuses another), DTEND = new start + the master's
//!   duration (caldata's own instance rule, event/mod.rs `recurid + duration`).
//! - TZID and floating values move on their own wall clock; UTC values of a series in its rule's zone (its DTSTART's
//!   TZID, else UTC: caldata runs floating and whole-day rules at UTC), a single event's and one occurrence's in the
//!   user's (the time the views show stays across a DST change).

use super::*;
use chrono::Datelike;

/// One date value of a content line, as written.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum When {
    Date(NaiveDate),
    /// floating, or wall clock of the line's TZID
    Local(NaiveDateTime),
    Utc(NaiveDateTime),
}

impl When {
    fn parse(v: &str) -> Option<When> {
        let v = v.trim();
        if let Some(u) = v.strip_suffix(['Z', 'z']) {
            return NaiveDateTime::parse_from_str(u, "%Y%m%dT%H%M%S").ok().map(When::Utc);
        }
        if v.len() == 8 {
            return NaiveDate::parse_from_str(v, "%Y%m%d").ok().map(When::Date);
        }
        NaiveDateTime::parse_from_str(v, "%Y%m%dT%H%M%S").ok().map(When::Local)
    }
    fn fmt(self) -> String {
        match self {
            When::Date(d) => ical::fmt_date(d),
            When::Local(t) => ical::fmt_dt(t),
            When::Utc(t) => format!("{}Z", ical::fmt_dt(t)),
        }
    }
    /// `days` later (earlier when negative); `None` outside `ical::write_date_ok`.
    fn shift(self, days: i64, tz: chrono_tz::Tz) -> Option<When> {
        let d = Duration::days(days);
        let w = match self {
            When::Date(x) => When::Date(x.checked_add_signed(d)?),
            When::Local(x) => When::Local(x.checked_add_signed(d)?),
            When::Utc(x) => When::Utc(ical::to_utc(tz.from_utc_datetime(&x).naive_local().checked_add_signed(d)?, tz).naive_utc()),
        };
        ical::write_date_ok(w.day(tz)).then_some(w)
    }
    /// the calendar day it falls on (UTC: in the user's zone)
    fn day(self, tz: chrono_tz::Tz) -> NaiveDate {
        match self {
            When::Date(d) => d,
            When::Local(t) => t.date(),
            When::Utc(t) => tz.from_utc_datetime(&t).date_naive(),
        }
    }
    /// comparable across forms: a TZID wall clock as UTC (how caldata hands out RECURRENCE-IDs)
    fn instant(self, tzid: Option<chrono_tz::Tz>) -> When {
        match (self, tzid) {
            (When::Local(t), Some(z)) => When::Utc(ical::to_utc(t, z).naive_utc()),
            (w, _) => w,
        }
    }
}

fn unsupported() -> AppError {
    AppError::bad("err-move-unsupported")
}

/// Head (name and parameters) and value of an unfolded line, split at the first `:` outside quotes.
fn head_value(l: &str) -> (&str, &str) {
    let mut quoted = false;
    for (i, c) in l.char_indices() {
        match c {
            '"' => quoted = !quoted,
            ':' if !quoted => return (&l[..i], &l[i + 1..]),
            _ => {}
        }
    }
    (l, "")
}

fn tzid_of(l: &str) -> Option<chrono_tz::Tz> {
    ical::split_line(l).1.iter().find(|(k, _)| k == "TZID").and_then(|(_, v)| ical::parse_tz(v))
}

/// The line with each of its date values moved by `days`: a list (EXDATE, RDATE) item by item, a PERIOD at both
/// ends (a duration end stays).
fn shift_line(l: &str, days: i64, tz: chrono_tz::Tz) -> AppResult<String> {
    let (head, value) = head_value(l);
    let mut items = vec![];
    for item in value.split(',') {
        let mut ends = vec![];
        for (i, p) in item.split('/').enumerate() {
            if i == 1 && p.trim_start().starts_with(['P', '+', '-']) {
                ends.push(p.to_string());
                continue;
            }
            let w = When::parse(p).ok_or_else(unsupported)?;
            ends.push(w.shift(days, tz).ok_or(AppError::bad("err-date-range"))?.fmt());
        }
        items.push(ends.join("/"));
    }
    Ok(format!("{head}:{}", items.join(",")))
}

const DAYS: [&str; 7] = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"];

/// The RRULE of a series whose start moves from `old` to `new` (`days` apart): UNTIL moves along; a BYDAY without
/// ordinals turns by the weekdays passed (WKST with it when the interval groups weeks), and a single ordinal BYDAY,
/// BYMONTHDAY or BYMONTH is read again from the new start (a monthly "3rd Friday" moved a day becomes the new day's
/// "nth Saturday"). Parts that cannot follow a move (BYYEARDAY, BYWEEKNO, BYSETPOS, several ordinals or month days
/// that leave 1-28) are refused rather than guessed: `err-move-unsupported`.
pub(super) fn shift_rrule(value: &str, days: i64, old: NaiveDate, new: NaiveDate, tz: chrono_tz::Tz) -> AppResult<String> {
    let rule = ical::rrule_canon(value);
    let parts: Vec<(&str, &str)> = rule.split(';').filter(|p| !p.is_empty()).map(|p| p.split_once('=').unwrap_or((p, ""))).collect();
    let get = |k: &str| parts.iter().find(|(n, _)| *n == k).map(|(_, v)| *v);
    let freq = get("FREQ").unwrap_or("");
    let turn = |d: &str| DAYS.iter().position(|x| *x == d).map(|i| DAYS[(i as i64 + days).rem_euclid(7) as usize]);
    let dim = |d: NaiveDate| (d.with_day(1).and_then(|f| f.checked_add_months(chrono::Months::new(1))).map(|n| (n - Duration::days(1)).day()).unwrap_or(31)) as i64;
    let interval: i64 = get("INTERVAL").and_then(|v| v.parse().ok()).unwrap_or(1);
    let mut out: Vec<String> = vec![];
    for (k, v) in &parts {
        let nv = match *k {
            "UNTIL" => When::parse(v).ok_or_else(unsupported)?.shift(days, tz).ok_or(AppError::bad("err-date-range"))?.fmt(),
            "BYDAY" => {
                let items: Vec<(&str, &str)> = v.split(',').map(|x| x.split_at(x.len().saturating_sub(2))).collect();
                if items.iter().all(|(o, _)| o.is_empty()) {
                    items.iter().map(|(_, d)| turn(d).map(str::to_string)).collect::<Option<Vec<_>>>().ok_or_else(unsupported)?.join(",")
                } else if let [(o, _)] = items.as_slice()
                    && (freq == "MONTHLY" || (freq == "YEARLY" && get("BYMONTH").is_some()))
                {
                    let n: i64 = o.parse().map_err(|_| unsupported())?;
                    let nth = if n < 0 { -((dim(new) - new.day() as i64) / 7 + 1) } else { (new.day() as i64 - 1) / 7 + 1 };
                    format!("{nth}{}", DAYS[new.weekday().num_days_from_monday() as usize])
                } else {
                    return Err(unsupported());
                }
            }
            "BYMONTHDAY" if freq == "MONTHLY" || freq == "YEARLY" => {
                let ns: Vec<i64> = v.split(',').map(|x| x.parse().map_err(|_| unsupported())).collect::<AppResult<_>>()?;
                if let [n] = ns.as_slice() {
                    if *n < 0 { (-(dim(new) - new.day() as i64 + 1)).to_string() } else { new.day().to_string() }
                } else {
                    let step = new.day() as i64 - old.day() as i64;
                    ns.iter().map(|n| Some(n + step).filter(|m| *n > 0 && (1..=28).contains(m)).map(|m| m.to_string())).collect::<Option<Vec<_>>>().ok_or_else(unsupported)?.join(",")
                }
            }
            "BYMONTH" if freq == "YEARLY" || freq == "MONTHLY" => {
                let step = (new.year() as i64 * 12 + new.month() as i64) - (old.year() as i64 * 12 + old.month() as i64);
                let ms: Vec<i64> = v.split(',').map(|x| x.parse().map_err(|_| unsupported())).collect::<AppResult<_>>()?;
                ms.iter().map(|m| ((m - 1 + step).rem_euclid(12) + 1).to_string()).collect::<Vec<_>>().join(",")
            }
            // a season filter of a daily or weekly rule stays as it is
            "BYMONTH" | "FREQ" | "INTERVAL" | "COUNT" | "BYHOUR" | "BYMINUTE" | "BYSECOND" => v.to_string(),
            "WKST" => turn(v).filter(|_| interval > 1 && get("BYDAY").is_some()).map(str::to_string).unwrap_or_else(|| v.to_string()),
            _ => return Err(unsupported()),
        };
        out.push(format!("{k}={nv}"));
    }
    // the weeks an interval > 1 counts start on WKST (MO by default): it turns with the days, or the grouping changes
    if freq == "WEEKLY" && interval > 1 && get("BYDAY").is_some() && get("WKST").is_none() {
        out.push(format!("WKST={}", turn("MO").unwrap_or("MO")));
    }
    Ok(out.join(";"))
}

/// Everything of one top-level component, unfolded: its lines including BEGIN/END and nested blocks.
type Block = Vec<String>;

/// The stored object's components inside VCALENDAR (`Ok`, in order) between the VCALENDAR's own lines (`Err`).
fn blocks(ics: &str) -> Vec<Result<Block, String>> {
    let mut out = vec![];
    let mut cur: Option<Block> = None;
    let mut depth = 0;
    for l in unfold(&ics.replace("\r\n", "\n")) {
        if l.is_empty() {
            continue;
        }
        if l.starts_with("BEGIN:") {
            depth += 1;
            if depth == 2 {
                cur = Some(vec![]);
            }
        }
        match cur.as_mut() {
            Some(b) => b.push(l.clone()),
            None => out.push(Err(l.clone())),
        }
        if l.starts_with("END:") {
            if depth == 2
                && let Some(b) = cur.take()
            {
                out.push(Ok(b));
            }
            depth -= 1;
        }
    }
    out
}

/// The top-level property of a VEVENT block named `name` (outside its VALARMs).
fn top<'a>(b: &'a Block, name: &str) -> Option<&'a String> {
    let mut depth = 0;
    for l in b {
        if l.starts_with("BEGIN:") {
            depth += 1;
        } else if l.starts_with("END:") {
            depth -= 1;
        } else if depth == 1 && prop_name(l) == name {
            return Some(l);
        }
    }
    None
}

/// A VEVENT block with its dates moved: `rule` = the series' own dates too (master: RRULE, EXDATE, RDATE; override:
/// RECURRENCE-ID). Absolute alarm triggers move along; SEQUENCE, DTSTAMP and LAST-MODIFIED are renewed.
fn shift_block(b: &Block, days: i64, rule: bool, tz: chrono_tz::Tz) -> AppResult<Block> {
    let (old, new) = match top(b, "DTSTART").and_then(|l| When::parse(head_value(l).1)) {
        Some(w) => (w.day(tz), w.shift(days, tz).ok_or(AppError::bad("err-date-range"))?.day(tz)),
        None => return Err(unsupported()),
    };
    let now = ical::fmt_utc_now();
    let mut out = Vec::with_capacity(b.len() + 2);
    let (mut depth, mut seq, mut lm) = (0, false, false);
    for l in b {
        if l.starts_with("BEGIN:") {
            depth += 1;
        }
        let name = prop_name(l);
        let line = match (depth, name.as_str()) {
            (1, "DTSTART" | "DTEND") => shift_line(l, days, tz)?,
            (1, "RECURRENCE-ID" | "EXDATE" | "RDATE") if rule => shift_line(l, days, tz)?,
            (1, "RRULE") if rule => format!("{}:{}", head_value(l).0, shift_rrule(head_value(l).1, days, old, new, tz)?),
            (1, "EXRULE") if rule => return Err(unsupported()),
            (1, "SEQUENCE") => {
                seq = true;
                format!("SEQUENCE:{}", head_value(l).1.trim().parse::<i64>().unwrap_or(0).saturating_add(1))
            }
            (1, "DTSTAMP") => format!("DTSTAMP:{now}"),
            (1, "LAST-MODIFIED") => {
                lm = true;
                format!("LAST-MODIFIED:{now}")
            }
            (2, "TRIGGER") if ical::split_line(l).1.iter().any(|(k, v)| k == "VALUE" && v == "DATE-TIME") => shift_line(l, days, tz)?,
            _ => l.clone(),
        };
        if l.starts_with("END:") {
            if depth == 1 {
                if !seq {
                    out.push("SEQUENCE:1".into());
                }
                if !lm {
                    out.push(format!("LAST-MODIFIED:{now}"));
                }
            }
            depth -= 1;
        }
        out.push(line);
    }
    Ok(out)
}

/// A new override of the master's occurrence `rid` (an instant, `When::instant`) `days` away: the master's lines
/// without rule, RECURRENCE-ID in the form of its DTSTART, DTEND from the master's duration (`dur`).
fn new_override(master: &Block, rid: When, days: i64, dur: Duration, tz: chrono_tz::Tz) -> AppResult<Block> {
    let ds = top(master, "DTSTART").ok_or_else(unsupported)?;
    let (head, value) = head_value(ds);
    let zone = tzid_of(ds);
    // the occurrence in the master's DTSTART form
    let r = match (When::parse(value).ok_or_else(unsupported)?, rid) {
        (When::Local(_), When::Utc(t)) if zone.is_some() => When::Local(zone.map(|z| z.from_utc_datetime(&t).naive_local()).unwrap_or(t)),
        (When::Local(_), When::Local(t)) if zone.is_none() => When::Local(t),
        (When::Utc(_), When::Utc(t)) => When::Utc(t),
        _ => return Err(unsupported()),
    };
    let s = r.shift(days, tz).ok_or(AppError::bad("err-date-range"))?;
    let e = match s {
        When::Date(_) => return Err(unsupported()), // whole days go through `move_date`
        When::Local(t) => When::Local(t.checked_add_signed(dur).ok_or(AppError::bad("err-date-range"))?),
        When::Utc(t) => When::Utc(t.checked_add_signed(dur).ok_or(AppError::bad("err-date-range"))?),
    };
    let tail = &head["DTSTART".len()..]; // the parameters (TZID, VALUE)
    let has_duration = top(master, "DURATION").is_some();
    let mut out = vec![];
    let mut depth = 0;
    for l in master {
        if l.starts_with("BEGIN:") {
            depth += 1;
        }
        let name = prop_name(l);
        if depth == 1 && matches!(name.as_str(), "RRULE" | "RDATE" | "EXDATE" | "EXRULE" | "DTEND" | "RECURRENCE-ID") {
            continue; // never a BEGIN/END line: the depth stays right
        }
        if depth == 1 && name == "DTSTART" {
            out.push(format!("RECURRENCE-ID{tail}:{}", r.fmt()));
            out.push(format!("DTSTART{tail}:{}", s.fmt()));
            if !has_duration {
                out.push(format!("DTEND{tail}:{}", e.fmt()));
            }
        } else if depth == 2 && name == "TRIGGER" && ical::split_line(l).1.iter().any(|(k, v)| k == "VALUE" && v == "DATE-TIME") {
            out.push(shift_line(l, days, tz)?);
        } else if depth == 1 && name == "DTSTAMP" {
            out.push(format!("DTSTAMP:{}", ical::fmt_utc_now()));
        } else {
            out.push(l.clone());
        }
        if l.starts_with("END:") {
            depth -= 1;
        }
    }
    Ok(out)
}

/// A whole-day series' occurrence `from` on `to` instead, through the master's EXDATE/RDATE: caldata 0.17.3 never
/// applies a DATE override (its generated RECURRENCE-ID is `CalDate(d, Tz::utc())`, a parsed one `CalDate(d, Tz::Local)`,
/// compared with the derived `PartialEq`: the override was dropped and the occurrence stayed; test
/// `all_day_occurrence_moves_by_exdate_and_rdate`). An RDATE occurrence moves its RDATE; a day the rule makes but an
/// EXDATE takes out comes back instead of becoming an RDATE as well (the EXDATE would hide it).
fn move_date(master: &Block, from: NaiveDate, to: NaiveDate) -> AppResult<Block> {
    let dates = |name: &str| -> (Vec<NaiveDate>, bool) {
        let mut v = vec![];
        let mut all = true;
        for l in master.iter().filter(|l| prop_name(l) == name) {
            for x in head_value(l).1.split(',') {
                match When::parse(x) {
                    Some(When::Date(d)) => v.push(d),
                    _ => all = false,
                }
            }
        }
        (v, all)
    };
    let ((mut ex, ex_ok), (mut rd, rd_ok)) = (dates("EXDATE"), dates("RDATE"));
    if !ex_ok || !rd_ok {
        return Err(unsupported()); // a date-time among them: the lists cannot be written back as dates
    }
    if rd.contains(&from) {
        rd.retain(|d| *d != from);
    } else {
        ex.push(from);
    }
    if ex.contains(&to) {
        ex.retain(|d| *d != to);
    } else {
        rd.push(to);
    }
    let list = |name: &str, v: &[NaiveDate]| (!v.is_empty()).then(|| format!("{name};VALUE=DATE:{}", v.iter().map(|d| ical::fmt_date(*d)).collect::<Vec<_>>().join(",")));
    let mut out: Block = master.iter().filter(|l| !matches!(prop_name(l).as_str(), "EXDATE" | "RDATE")).cloned().collect();
    let end = out.iter().position(|l| l.starts_with("BEGIN:VALARM") || l == "END:VEVENT").unwrap_or(out.len());
    out.splice(end..end, [list("EXDATE", &ex), list("RDATE", &rd)].into_iter().flatten());
    Ok(out)
}

/// The stored object text moved by `days` (module doc). `one`: only the occurrence with this RECURRENCE-ID instant
/// (`When::instant`) — its override, or a new one after the master (`dur` = the master's duration).
pub fn shift_ics(ics: &str, days: i64, one: Option<(When, Duration)>, tz: chrono_tz::Tz) -> AppResult<String> {
    let mut parts = blocks(ics);
    let is_event = |b: &Block| b.first().is_some_and(|l| l == "BEGIN:VEVENT");
    let rid_of = |b: &Block| top(b, "RECURRENCE-ID").and_then(|l| When::parse(head_value(l).1).map(|w| w.instant(tzid_of(l))));
    match one {
        None => {
            // a series' values move in the zone its rule runs in (DTSTART's TZID; UTC for a UTC start), or a UTC EXDATE,
            // UNTIL or RECURRENCE-ID would part from the occurrences across a DST change; a single event in the user's
            // zone, whose shown time it keeps (test `utc_series_moves_in_its_own_zone`)
            let master = parts.iter().filter_map(|p| p.as_ref().ok()).find(|b| is_event(b) && rid_of(b).is_none());
            let zone = match master.and_then(|m| top(m, "DTSTART").filter(|_| top(m, "RRULE").or(top(m, "RDATE")).is_some())) {
                // floating and whole-day starts too: caldata runs `Tz::Local` at UTC (types/timezone.rs `Utc.fix()`)
                Some(l) => tzid_of(l).unwrap_or(chrono_tz::UTC),
                None => tz,
            };
            for p in parts.iter_mut() {
                if let Ok(b) = p
                    && is_event(b)
                {
                    *b = shift_block(b, days, true, zone)?;
                }
            }
        }
        Some((rid, dur)) => {
            let own = parts.iter().any(|p| p.as_ref().is_ok_and(|b| is_event(b) && rid_of(b) == Some(rid)));
            let at = parts.iter().position(|p| p.as_ref().is_ok_and(|b| is_event(b) && rid_of(b).is_none())).ok_or(AppError::NotFound)?;
            if let When::Date(d) = rid {
                // another client's override of a whole-day occurrence: caldata never shows it (`move_date`), moving it
                // would change nothing on screen, and an EXDATE would hide it from the clients that do show it
                if own {
                    return Err(unsupported());
                }
                let to = d.checked_add_signed(Duration::days(days)).filter(|t| ical::write_date_ok(*t)).ok_or(AppError::bad("err-date-range"))?;
                let Ok(master) = &mut parts[at] else { return Err(AppError::NotFound) };
                *master = shift_block(&move_date(master, d, to)?, 0, false, tz)?;
            } else if own {
                if let Some(b) = parts.iter_mut().filter_map(|p| p.as_mut().ok()).find(|b| is_event(b) && rid_of(b) == Some(rid)) {
                    *b = shift_block(b, days, false, tz)?;
                }
            } else {
                let Ok(master) = &parts[at] else { return Err(AppError::NotFound) };
                let o = new_override(master, rid, days, dur, tz)?;
                parts.insert(at + 1, Ok(o));
            }
        }
    }
    let mut out = String::with_capacity(ics.len() + 256);
    for p in &parts {
        match p {
            Ok(b) => b.iter().for_each(|l| {
                out.push_str(&ical::fold(l));
                out.push_str("\r\n");
            }),
            Err(l) => {
                out.push_str(&ical::fold(l));
                out.push_str("\r\n");
            }
        }
    }
    Ok(out)
}

/// Moves a stored event by `days` (module doc): `at` = the local start (user's zone) of the occurrence to move alone;
/// without it, or for an event without a rule, the whole event moves. `expected_etag` as in `update_event`.
pub async fn move_event(state: &AppState, user: &Principal, p: &str, c: &str, id: &str, days: i64, at: Option<NaiveDateTime>, expected_etag: Option<&str>, tz: chrono_tz::Tz) -> AppResult<()> {
    collections::ensure_owner(user, p)?;
    collections::ensure_writable(state, p, c).await?;
    if days == 0 || days.unsigned_abs() > 10_000 * 366 { // beyond `ical::YEARS` anyway; Duration::days panics far past it
        return Err(AppError::bad("err-date-range"));
    }
    let existing = state.calendars.get_object(p, c, id, false).await?;
    if let Some(e) = expected_etag
        && !e.is_empty()
        && e != existing.get_etag()
    {
        return Err(AppError::conflict("err-changed-event"));
    }
    let CalendarInnerData::Event(main, overrides) = existing.get_inner().get_inner() else {
        return Err(AppError::NotFound);
    };
    let one = match at {
        Some(at) if main.has_rruleset() => {
            // the occurrence that starts at `at` in the user's zone: an override first (moved days away from its
            // RECURRENCE-ID, the window below would not reach it: caldata emits an override only beside its rule
            // instance, event/mod.rs `expand_recurrence`), else as caldata expands the rule around `at`
            let rid_of = |e: &caldata::component::IcalEvent| e.get_property("RECURRENCE-ID").and_then(|r| When::parse(&r.value).map(|w| w.instant(r.params.get_param("TZID").and_then(|z| ical::parse_tz(z)))));
            let rid = match overrides.iter().find(|o| ical::local(&o.dtstart.0, tz).0 == at) {
                Some(o) => rid_of(o),
                None => {
                    let (ws, we) = (ical::to_utc(at, tz) - Duration::days(1), ical::to_utc(at, tz) + Duration::days(1));
                    main.expand_recurrence(Some(ws), Some(we), overrides).iter().find(|e| ical::local(&e.dtstart.0, tz).0 == at).and_then(rid_of)
                }
            }
            .ok_or(AppError::conflict("err-changed-event"))?;
            Some((rid, main.get_duration().unwrap_or_default()))
        }
        _ => None,
    };
    let ics = shift_ics(&existing.get_ics(), days, one, tz)?;
    let obj = CalendarObject::import(&ics, None)?;
    state.calendars.put_object(p, c, id, obj, true).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BER: chrono_tz::Tz = chrono_tz::Europe::Berlin;

    fn cal(body: &str) -> String {
        format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//t//EN\r\n{}\r\n{}\r\nEND:VCALENDAR\r\n", ical::vtimezone("Europe/Berlin").unwrap(), body.trim().replace('\n', "\r\n"))
    }

    /// local starts (Berlin) of every occurrence in 2026-2027, as caldata expands the object
    fn starts(ics: &str) -> Vec<NaiveDateTime> {
        starts_in(ics, BER)
    }

    fn starts_in(ics: &str, zone: chrono_tz::Tz) -> Vec<NaiveDateTime> {
        let obj = CalendarObject::import(ics, None).expect("parses");
        let CalendarInnerData::Event(main, ov) = obj.get_inner().get_inner() else { panic!("no event") };
        let (a, b) = (ical::to_utc(NaiveDate::from_ymd_opt(2026, 1, 1).unwrap().and_hms_opt(0, 0, 0).unwrap(), BER), ical::to_utc(NaiveDate::from_ymd_opt(2028, 1, 1).unwrap().and_hms_opt(0, 0, 0).unwrap(), BER));
        let mut v: Vec<NaiveDateTime> = main.expand_recurrence(Some(a), Some(b), ov).iter().map(|e| ical::local(&e.dtstart.0, zone).0).collect();
        v.sort();
        v
    }

    fn series(rule: &str, start: &str) -> String {
        cal(&format!("BEGIN:VEVENT\nUID:s\nDTSTAMP:20260101T000000Z\nSUMMARY:S\nDTSTART;TZID=Europe/Berlin:{start}\nDTEND;TZID=Europe/Berlin:{}\nRRULE:{rule}\nEND:VEVENT", start.replace("T09", "T10")))
    }

    #[test]
    fn plain_event_moves_its_dates_and_absolute_alarm_only() {
        let ics = cal("BEGIN:VEVENT\nUID:a\nDTSTAMP:20260101T000000Z\nSEQUENCE:4\nSUMMARY:A\nDTSTART;TZID=Europe/Berlin:20261014T090000\nDTEND;TZID=Europe/Berlin:20261014T100000\nX-KEEP:20261014T090000\nBEGIN:VALARM\nACTION:DISPLAY\nTRIGGER;VALUE=DATE-TIME:20261014T060000Z\nEND:VALARM\nBEGIN:VALARM\nACTION:DISPLAY\nTRIGGER:-PT15M\nEND:VALARM\nEND:VEVENT");
        let out = shift_ics(&ics, 2, None, BER).unwrap();
        for want in ["DTSTART;TZID=Europe/Berlin:20261016T090000", "DTEND;TZID=Europe/Berlin:20261016T100000", "TRIGGER;VALUE=DATE-TIME:20261016T060000Z", "TRIGGER:-PT15M", "SEQUENCE:5", "X-KEEP:20261014T090000"] {
            assert!(out.contains(want), "{want} in {out}");
        }
        assert_eq!(starts(&out), vec![NaiveDate::from_ymd_opt(2026, 10, 16).unwrap().and_hms_opt(9, 0, 0).unwrap()]);
    }

    #[test]
    fn utc_values_keep_the_shown_time_across_dst() {
        // 09:00 Berlin in summer time, three days later it is winter time: still 09:00 local, one hour later in UTC
        let ics = cal("BEGIN:VEVENT\nUID:u\nDTSTAMP:20260101T000000Z\nSUMMARY:U\nDTSTART:20261023T070000Z\nDTEND:20261023T080000Z\nEND:VEVENT");
        let out = shift_ics(&ics, 3, None, BER).unwrap();
        assert!(out.contains("DTSTART:20261026T080000Z") && out.contains("DTEND:20261026T090000Z"), "{out}");
    }

    #[test]
    fn all_day_span_moves_whole() {
        let ics = cal("BEGIN:VEVENT\nUID:d\nDTSTAMP:20260101T000000Z\nSUMMARY:D\nDTSTART;VALUE=DATE:20261030\nDTEND;VALUE=DATE:20261102\nEND:VEVENT");
        let out = shift_ics(&ics, -5, None, BER).unwrap();
        assert!(out.contains("DTSTART;VALUE=DATE:20261025") && out.contains("DTEND;VALUE=DATE:20261028"), "{out}");
    }

    /// the moved series is exactly the old one `days` later, per caldata's own expansion
    #[test]
    fn whole_series_moves_exactly() {
        let cases = [
            ("FREQ=WEEKLY;BYDAY=WE", "20261014T090000", 1),
            ("FREQ=WEEKLY;BYDAY=MO,FR;INTERVAL=2;COUNT=12", "20261012T090000", 3),
            ("FREQ=WEEKLY;BYDAY=MO,FR;INTERVAL=2;COUNT=12", "20261012T090000", -4),
            ("FREQ=DAILY;BYDAY=MO,TU,WE,TH,FR;UNTIL=20261231T080000Z", "20261012T090000", 2),
            ("FREQ=DAILY;INTERVAL=3;COUNT=20", "20261012T090000", 10),
            ("FREQ=MONTHLY;BYMONTHDAY=10,20;COUNT=10", "20261010T090000", 5),
            ("FREQ=MONTHLY;COUNT=10", "20261005T090000", 7),
            ("FREQ=YEARLY", "20260214T090000", 1),
        ];
        for (rule, start, days) in cases {
            let ics = series(rule, start);
            let out = shift_ics(&ics, days, None, BER).unwrap_or_else(|e| panic!("{rule}: {e:?}"));
            let want: Vec<NaiveDateTime> = starts(&ics).iter().map(|t| *t + Duration::days(days)).filter(|t| t.date().year() < 2028).collect();
            let got: Vec<NaiveDateTime> = starts(&out).into_iter().filter(|t| t.date().year() < 2028).collect();
            assert_eq!(got, want, "{rule} {days:+}: {out}");
        }
    }

    #[test]
    fn series_dates_follow_the_start() {
        let ics = cal("BEGIN:VEVENT\nUID:s\nDTSTAMP:20260101T000000Z\nSUMMARY:S\nDTSTART;TZID=Europe/Berlin:20261014T090000\nDTEND;TZID=Europe/Berlin:20261014T100000\nRRULE:FREQ=WEEKLY;BYDAY=WE;UNTIL=20261230T080000Z\nEXDATE;TZID=Europe/Berlin:20261028T090000,20261111T090000\nEND:VEVENT\nBEGIN:VEVENT\nUID:s\nDTSTAMP:20260101T000000Z\nRECURRENCE-ID;TZID=Europe/Berlin:20261021T090000\nSUMMARY:moved\nDTSTART;TZID=Europe/Berlin:20261021T110000\nDTEND;TZID=Europe/Berlin:20261021T120000\nEND:VEVENT");
        let out = shift_ics(&ics, 1, None, BER).unwrap();
        for want in ["RRULE:FREQ=WEEKLY;BYDAY=TH;UNTIL=20261231T080000Z", "EXDATE;TZID=Europe/Berlin:20261029T090000,20261112T090000", "RECURRENCE-ID;TZID=Europe/Berlin:20261022T090000", "DTSTART;TZID=Europe/Berlin:20261022T110000"] {
            assert!(out.contains(want), "{want} in {out}");
        }
        let s = starts(&out);
        assert!(s.contains(&NaiveDate::from_ymd_opt(2026, 10, 22).unwrap().and_hms_opt(11, 0, 0).unwrap()) && !s.iter().any(|t| t.date() == NaiveDate::from_ymd_opt(2026, 10, 29).unwrap()), "{s:?}");
    }

    #[test]
    fn ordinal_rules_follow_the_new_day_and_others_are_refused() {
        // 2026-10-16 is the third Friday; a day later the third Saturday, the last Friday a week later the last Saturday
        let out = shift_ics(&series("FREQ=MONTHLY;BYDAY=3FR", "20261016T090000"), 1, None, BER).unwrap();
        assert!(out.contains("RRULE:FREQ=MONTHLY;BYDAY=3SA"), "{out}");
        let out = shift_ics(&series("FREQ=MONTHLY;BYDAY=-1FR", "20261030T090000"), 1, None, BER).unwrap();
        assert!(out.contains("RRULE:FREQ=MONTHLY;BYDAY=-1SA"), "{out}");
        let out = shift_ics(&series("FREQ=MONTHLY;BYMONTHDAY=-1", "20261031T090000"), -1, None, BER).unwrap();
        assert!(out.contains("RRULE:FREQ=MONTHLY;BYMONTHDAY=-2"), "{out}");
        for rule in ["FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1", "FREQ=YEARLY;BYYEARDAY=100", "FREQ=MONTHLY;BYDAY=1MO,3MO", "FREQ=WEEKLY;BYMONTHDAY=3"] {
            assert!(matches!(shift_ics(&series(rule, "20261016T090000"), 1, None, BER), Err(AppError::BadRequest(_))), "{rule}");
        }
    }

    #[test]
    fn one_occurrence_gets_an_override_then_moves_it() {
        let ics = series("FREQ=WEEKLY;BYDAY=WE;COUNT=5", "20261014T090000");
        let rid = When::Utc(NaiveDate::from_ymd_opt(2026, 10, 21).unwrap().and_hms_opt(7, 0, 0).unwrap());
        let out = shift_ics(&ics, 2, Some((rid, Duration::hours(1))), BER).unwrap();
        assert!(out.contains("RECURRENCE-ID;TZID=Europe/Berlin:20261021T090000") && out.contains("DTSTART;TZID=Europe/Berlin:20261023T090000") && out.contains("DTEND;TZID=Europe/Berlin:20261023T100000"), "{out}");
        let at = |d: u32, h: u32| NaiveDate::from_ymd_opt(2026, if d > 31 { 11 } else { 10 }, if d > 31 { d - 31 } else { d }).unwrap().and_hms_opt(h, 0, 0).unwrap();
        assert_eq!(starts(&out), vec![at(14, 9), at(23, 9), at(28, 9), at(35, 9), at(42, 9)]);
        // the same occurrence again: its override moves, no second one
        let again = shift_ics(&out, -1, Some((rid, Duration::hours(1))), BER).unwrap();
        assert_eq!(again.matches("RECURRENCE-ID").count(), 1, "{again}");
        assert_eq!(starts(&again), vec![at(14, 9), at(22, 9), at(28, 9), at(35, 9), at(42, 9)]);
    }

    #[test]
    fn all_day_occurrence_moves_by_exdate_and_rdate() {
        let ics = cal("BEGIN:VEVENT\nUID:w\nDTSTAMP:20260101T000000Z\nSUMMARY:W\nDTSTART;VALUE=DATE:20261014\nDTEND;VALUE=DATE:20261016\nRRULE:FREQ=WEEKLY;COUNT=3\nEND:VEVENT");
        let day = |d: u32| When::Date(NaiveDate::from_ymd_opt(2026, 10, d).unwrap());
        let out = shift_ics(&ics, 3, Some((day(21), Duration::days(2))), BER).unwrap();
        assert!(out.contains("EXDATE;VALUE=DATE:20261021") && out.contains("RDATE;VALUE=DATE:20261024") && !out.contains("RECURRENCE-ID"), "{out}");
        let d = |m: u32, d: u32| NaiveDate::from_ymd_opt(2026, m, d).unwrap().and_hms_opt(0, 0, 0).unwrap();
        assert_eq!(starts(&out), vec![d(10, 14), d(10, 24), d(10, 28)]);
        // the moved one again: its RDATE moves; back onto its own day: the EXDATE goes
        let again = shift_ics(&out, 2, Some((day(24), Duration::days(2))), BER).unwrap();
        assert_eq!(starts(&again), vec![d(10, 14), d(10, 26), d(10, 28)]);
        let back = shift_ics(&again, -5, Some((day(26), Duration::days(2))), BER).unwrap();
        let ev = &back[back.find("BEGIN:VEVENT").unwrap()..]; // the VTIMEZONE has RDATEs of its own
        assert!(!ev.contains("EXDATE") && !ev.contains("RDATE"), "{ev}");
        assert_eq!(starts(&back), starts(&ics));
    }

    /// a UTC series, a UTC EXDATE of a Berlin series seen from New York: across the DST change every value moves in the
    /// rule's zone, so the moved series is the old one `days` later (UTC instants) with its exception kept
    #[test]
    fn utc_series_moves_in_its_own_zone() {
        let utc = chrono_tz::UTC;
        for (body, days, user) in [
            ("DTSTART:20261023T233000Z\nDTEND:20261024T003000Z\nRRULE:FREQ=WEEKLY;BYDAY=FR;COUNT=4", 3, BER),
            ("DTSTART:20261023T070000Z\nDTEND:20261023T080000Z\nRRULE:FREQ=DAILY;COUNT=10\nEXDATE:20261030T070000Z", 3, BER),
            ("DTSTART;TZID=Europe/Berlin:20261102T090000\nDTEND;TZID=Europe/Berlin:20261102T100000\nRRULE:FREQ=DAILY;COUNT=5\nEXDATE:20261103T080000Z", -3, chrono_tz::America::New_York),
            // whole days and floating times run at UTC in caldata: a UTC UNTIL / EXDATE across 29 March stays on them
            ("DTSTART;VALUE=DATE:20260320\nDTEND;VALUE=DATE:20260321\nRRULE:FREQ=DAILY;UNTIL=20260327T000000Z", 5, BER),
            ("DTSTART:20260320T090000\nDTEND:20260320T100000\nRRULE:FREQ=DAILY;COUNT=10\nEXDATE:20260325T090000Z", 5, BER),
        ] {
            let ics = cal(&format!("BEGIN:VEVENT\nUID:z\nDTSTAMP:20260101T000000Z\nSUMMARY:Z\n{body}\nEND:VEVENT"));
            let out = shift_ics(&ics, days, None, user).unwrap();
            let zone = if body.starts_with("DTSTART:") { utc } else { BER };
            let want: Vec<NaiveDateTime> = starts_in(&ics, zone).iter().map(|t| *t + Duration::days(days)).collect();
            assert_eq!(starts_in(&out, zone), want, "{body}: {out}");
        }
    }

    #[test]
    fn whole_day_occurrence_refused_where_it_cannot_move() {
        let base = "BEGIN:VEVENT\nUID:w\nDTSTAMP:20260101T000000Z\nSUMMARY:W\nDTSTART;VALUE=DATE:20261014\nDTEND;VALUE=DATE:20261015\nRRULE:FREQ=WEEKLY;COUNT=3";
        let day = When::Date(NaiveDate::from_ymd_opt(2026, 10, 21).unwrap());
        // another client's override of that day (caldata shows none); a date-time among the RDATEs
        let foreign = cal(&format!("{base}\nEND:VEVENT\nBEGIN:VEVENT\nUID:w\nDTSTAMP:20260101T000000Z\nRECURRENCE-ID;VALUE=DATE:20261021\nSUMMARY:W2\nDTSTART;VALUE=DATE:20261022\nDTEND;VALUE=DATE:20261023\nEND:VEVENT"));
        let mixed = cal(&format!("{base}\nRDATE:20261102T000000Z\nEND:VEVENT"));
        for ics in [foreign, mixed] {
            assert!(matches!(shift_ics(&ics, 1, Some((day, Duration::days(1))), BER), Err(AppError::BadRequest(_))), "{ics}");
        }
    }
}
