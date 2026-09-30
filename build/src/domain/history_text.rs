//! The change history's view of an object's text (`domain::history`): an iCalendar or vCard file as
//! attributes (property name → its lines), the difference of two versions, putting chosen
//! attributes of an older version back into the current one (undo), and readable values.
//!
//! Attribute keys: a master property's name (`SUMMARY`, `ATTENDEE`, …; all lines of a name are one
//! attribute), a sub-component's name (`VALARM`, `VLOCATION`, …; all blocks of a name are one),
//! `RECURRENCE-ID:<value>` for an override of one occurrence, and for vCards the property name
//! without its group prefix (`item1.TEL` → `TEL`). `ACKNOWLEDGED` (an alarm's dismissal) is split off
//! the alarm so that a device dismissing a reminder is a technical change, not an edited reminder.

use crate::domain::{events, ical};
use chrono::{Datelike, NaiveDate, NaiveDateTime, TimeZone};
use std::collections::BTreeMap;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Store {
    Cal,
    Ab,
}

impl Store {
    pub fn tag(self) -> &'static str {
        match self {
            Store::Cal => "cal",
            Store::Ab => "ab",
        }
    }
    pub fn from_tag(s: &str) -> Store {
        if s == "ab" { Store::Ab } else { Store::Cal }
    }
}

/// Pseudo attribute of a move between collections (value: `principal/collection`).
pub const COLL_KEY: &str = "X-HX-COLL";

/// Keys whose change alone is bookkeeping of a client, not an edit: such a change is recorded as
/// "technical" and hidden by default.
pub fn is_tech(key: &str) -> bool {
    matches!(key, "DTSTAMP" | "LAST-MODIFIED" | "SEQUENCE" | "CREATED" | "ACKNOWLEDGED" | "REV" | "PRODID")
        || key.starts_with("X-MOZ-")
        || key.starts_with("X-APPLE-")
}

/// Lines an override is compared without: they change on every write of the series.
fn volatile(name: &str) -> bool {
    matches!(name, "DTSTAMP" | "LAST-MODIFIED" | "SEQUENCE")
}

pub fn prop_name(line: &str) -> String {
    let end = line.find([';', ':']).unwrap_or(line.len());
    line[..end].to_ascii_uppercase()
}

/// A vCard name without its group (`item1.TEL` → `TEL`).
fn vcard_name(line: &str) -> String {
    let n = prop_name(line);
    match n.rfind('.') {
        Some(i) => n[i + 1..].to_string(),
        None => n,
    }
}

/// `;PARAMS:value` of a line (what follows the name); for vCards without the group.
fn rest(line: &str) -> &str {
    let end = line.find([';', ':']).unwrap_or(line.len());
    &line[end..]
}

/// Binary payloads (embedded attachments, photos) would bloat every recorded value.
fn shorten(rest: &str) -> String {
    let up = rest.to_ascii_uppercase();
    let head = split_value(rest).0;
    if up.contains("ENCODING=B") || up.contains("VALUE=BINARY") || rest.len() > 4000 && (up.contains("BASE64") || up.contains("DATA:")) {
        return format!("{head}:…");
    }
    if rest.len() > 4000 { format!("{}…", &rest[..rest.floor_char_boundary(4000)]) } else { rest.to_string() }
}

/// (params incl. the leading ';', value) of `;PARAMS:value`; a ':' inside a quoted parameter
/// (`ALTREP="https://…"`) does not end the parameters.
pub fn split_value(rest: &str) -> (&str, &str) {
    let mut quoted = false;
    for (i, c) in rest.char_indices() {
        match c {
            '"' => quoted = !quoted,
            ':' if !quoted => return (&rest[..i], &rest[i + 1..]),
            _ => {}
        }
    }
    (rest, "")
}

pub fn param<'a>(params: &'a str, name: &str) -> Option<&'a str> {
    params.split(';').skip(1).find_map(|p| {
        let (k, v) = p.split_once('=')?;
        k.eq_ignore_ascii_case(name).then(|| v.trim_matches('"'))
    })
}

/// An iCalendar file as blocks: the calendar's own lines and each component under VCALENDAR.
struct Ics {
    head: Vec<String>,
    blocks: Vec<Vec<String>>,
}

fn parse_ics(text: &str) -> Ics {
    let mut head = vec![];
    let mut blocks: Vec<Vec<String>> = vec![];
    let mut depth = 0usize;
    let mut cur: Vec<String> = vec![];
    for l in events::unfold(text) {
        if l.is_empty() {
            continue;
        }
        let name = prop_name(&l);
        if name == "BEGIN" {
            depth += 1;
            if depth >= 2 {
                cur.push(l);
            } else {
                head.push(l);
            }
            continue;
        }
        if name == "END" {
            if depth >= 2 {
                cur.push(l);
                if depth == 2 {
                    blocks.push(std::mem::take(&mut cur));
                }
            } else {
                head.push(l);
            }
            depth = depth.saturating_sub(1);
            continue;
        }
        if depth >= 2 { cur.push(l) } else { head.push(l) }
    }
    Ics { head, blocks }
}

fn block_name(b: &[String]) -> String {
    b.first().and_then(|l| l.split_once(':')).map(|(_, v)| v.trim().to_ascii_uppercase()).unwrap_or_default()
}

/// Top-level lines and sub-blocks of one component (without its BEGIN/END).
enum Part {
    Line(String),
    Sub(String, Vec<String>),
}

fn parts(block: &[String]) -> Vec<Part> {
    let inner = if block.len() >= 2 { &block[1..block.len() - 1] } else { &block[..0] };
    let mut out = vec![];
    let mut depth = 0usize;
    let mut sub: Vec<String> = vec![];
    for l in inner {
        let n = prop_name(l);
        if n == "BEGIN" {
            depth += 1;
        }
        if depth > 0 {
            sub.push(l.clone());
        } else {
            out.push(Part::Line(l.clone()));
        }
        if n == "END" && depth > 0 {
            depth -= 1;
            if depth == 0 {
                let name = block_name(&sub);
                out.push(Part::Sub(name, std::mem::take(&mut sub)));
            }
        }
    }
    out
}

fn rid(block: &[String]) -> Option<String> {
    parts(block).into_iter().find_map(|p| match p {
        Part::Line(l) if prop_name(&l) == "RECURRENCE-ID" => Some(split_value(rest(&l)).1.to_string()),
        _ => None,
    })
}

fn is_item(name: &str) -> bool {
    matches!(name, "VEVENT" | "VTODO" | "VJOURNAL")
}

/// The attributes of a file: key → the sorted lines' `;PARAMS:value`, joined by newlines.
pub fn attrs(store: Store, text: &str) -> BTreeMap<String, String> {
    let mut m: BTreeMap<String, Vec<String>> = BTreeMap::new();
    match store {
        Store::Ab => {
            for l in events::unfold(text) {
                let n = vcard_name(&l);
                if l.is_empty() || matches!(n.as_str(), "BEGIN" | "END" | "VERSION" | "UID") {
                    continue;
                }
                m.entry(n).or_default().push(shorten(rest(&l)));
            }
        }
        Store::Cal => {
            let ics = parse_ics(text);
            let mut master_seen = false;
            for b in &ics.blocks {
                if !is_item(&block_name(b)) {
                    continue;
                }
                if let Some(r) = rid(b) {
                    let body: Vec<String> = b.iter().filter(|l| !volatile(&prop_name(l))).map(|l| shorten(l)).collect();
                    m.entry(format!("RECURRENCE-ID:{r}")).or_default().push(body.join("\n"));
                    continue;
                }
                if master_seen {
                    continue;
                }
                master_seen = true;
                for p in parts(b) {
                    match p {
                        // the UID names the object, it is not a value of it
                        Part::Line(l) if prop_name(&l) == "UID" => {}
                        Part::Line(l) => m.entry(prop_name(&l)).or_default().push(shorten(rest(&l))),
                        Part::Sub(name, lines) => {
                            let (ack, keep): (Vec<&String>, Vec<&String>) = lines.iter().partition(|l| name == "VALARM" && matches!(prop_name(l).as_str(), "ACKNOWLEDGED" | "X-MOZ-LASTACK"));
                            for a in ack {
                                m.entry("ACKNOWLEDGED".into()).or_default().push(shorten(rest(a)));
                            }
                            m.entry(name).or_default().push(keep.iter().map(|l| shorten(l)).collect::<Vec<_>>().join("\n"));
                        }
                    }
                }
            }
        }
    }
    m.into_iter()
        .map(|(k, mut v)| {
            v.sort();
            (k, v.join("\n"))
        })
        .collect()
}

/// (key, before, after) of every attribute that differs; an absent attribute is "".
pub fn diff(before: &BTreeMap<String, String>, after: &BTreeMap<String, String>) -> Vec<(String, String, String)> {
    let mut keys: Vec<&String> = before.keys().chain(after.keys()).collect();
    keys.sort();
    keys.dedup();
    keys.into_iter()
        .filter_map(|k| {
            let (b, a) = (before.get(k).map(String::as_str).unwrap_or(""), after.get(k).map(String::as_str).unwrap_or(""));
            (b != a).then(|| (k.clone(), b.to_string(), a.to_string()))
        })
        .collect()
}

pub fn title(store: Store, text: &str) -> String {
    let want = if store == Store::Ab { "FN" } else { "SUMMARY" };
    let mut depth = 0;
    for l in events::unfold(text) {
        let n = if store == Store::Ab { vcard_name(&l) } else { prop_name(&l) };
        match n.as_str() {
            "BEGIN" => depth += 1,
            "END" => depth -= 1,
            _ if n == want && (store == Store::Ab || depth == 2) => return ical::unescape(split_value(rest(&l)).1).trim().to_string(),
            _ => {}
        }
    }
    String::new()
}

pub fn uid(text: &str) -> Option<String> {
    events::unfold(text).into_iter().find(|l| prop_name(l) == "UID").map(|l| split_value(rest(&l)).1.trim().to_string()).filter(|s| !s.is_empty())
}

pub fn prodid(text: &str) -> Option<String> {
    events::unfold(text).into_iter().find(|l| prop_name(l) == "PRODID").map(|l| split_value(rest(&l)).1.trim().to_string())
}

/// event | task | contact
pub fn kind(store: Store, text: &str) -> &'static str {
    match store {
        Store::Ab => "contact",
        Store::Cal if text.contains("BEGIN:VTODO") => "task",
        Store::Cal => "event",
    }
}

/// `current` with the attributes `keys` taken from `before` (undo of an edit). Everything else of
/// `current` stays; the master's SEQUENCE grows and its DTSTAMP/LAST-MODIFIED (vCard: REV) are now.
pub fn revert(store: Store, current: &str, before: &str, keys: &[String]) -> String {
    let now = ical::fmt_utc_now();
    match store {
        Store::Ab => {
            let cur = events::unfold(current);
            let old = events::unfold(before);
            let mut out: Vec<String> = cur.clone();
            for k in keys {
                let pos = out.iter().position(|l| &vcard_name(l) == k);
                out.retain(|l| &vcard_name(l) != k);
                let add: Vec<String> = old.iter().filter(|l| &vcard_name(l) == k).cloned().collect();
                let at = pos.unwrap_or_else(|| out.iter().rposition(|l| prop_name(l) == "END").unwrap_or(out.len())).min(out.len());
                out.splice(at..at, add);
            }
            out.retain(|l| vcard_name(l) != "REV");
            let end = out.iter().rposition(|l| prop_name(l) == "END").unwrap_or(out.len());
            out.insert(end, format!("REV:{now}"));
            out.iter().filter(|l| !l.is_empty()).map(|l| ical::fold(l)).collect::<Vec<_>>().join("\r\n") + "\r\n"
        }
        Store::Cal => {
            let mut cur = parse_ics(current);
            let old = parse_ics(before);
            let master_of = |ics: &Ics| ics.blocks.iter().position(|b| is_item(&block_name(b)) && rid(b).is_none());
            let (Some(mi), old_master) = (master_of(&cur), master_of(&old).map(|i| &old.blocks[i])) else {
                return current.to_string();
            };
            let old_parts = old_master.map(|b| parts(b)).unwrap_or_default();
            let mut ps = parts(&cur.blocks[mi]);
            for k in keys {
                if let Some(r) = k.strip_prefix("RECURRENCE-ID:") {
                    cur.blocks.retain(|b| rid(b).as_deref() != Some(r));
                    if let Some(b) = old.blocks.iter().find(|b| rid(b).as_deref() == Some(r)) {
                        cur.blocks.push(b.clone());
                    }
                    continue;
                }
                let sub = if k == "ACKNOWLEDGED" { Some("VALARM".to_string()) } else { old_parts.iter().chain(ps.iter()).find_map(|p| matches!(p, Part::Sub(n, _) if n == k).then(|| k.clone())) };
                match sub {
                    Some(name) => {
                        ps.retain(|p| !matches!(p, Part::Sub(n, _) if *n == name));
                        for p in &old_parts {
                            if let Part::Sub(n, lines) = p
                                && *n == name
                            {
                                ps.push(Part::Sub(n.clone(), lines.clone()));
                            }
                        }
                    }
                    None => {
                        let pos = ps.iter().position(|p| matches!(p, Part::Line(l) if &prop_name(l) == k));
                        ps.retain(|p| !matches!(p, Part::Line(l) if &prop_name(l) == k));
                        let add: Vec<Part> = old_parts.iter().filter_map(|p| match p {
                            Part::Line(l) if &prop_name(l) == k => Some(Part::Line(l.clone())),
                            _ => None,
                        }).collect();
                        let at = pos.unwrap_or_else(|| ps.iter().position(|p| matches!(p, Part::Sub(..))).unwrap_or(ps.len())).min(ps.len());
                        ps.splice(at..at, add);
                    }
                }
            }
            // bookkeeping of the new version
            let seq = ps.iter().find_map(|p| match p {
                Part::Line(l) if prop_name(l) == "SEQUENCE" => split_value(rest(l)).1.trim().parse::<i64>().ok(),
                _ => None,
            });
            ps.retain(|p| !matches!(p, Part::Line(l) if matches!(prop_name(l).as_str(), "SEQUENCE" | "DTSTAMP" | "LAST-MODIFIED")));
            ps.insert(0, Part::Line(format!("DTSTAMP:{now}")));
            ps.push(Part::Line(format!("LAST-MODIFIED:{now}")));
            ps.push(Part::Line(format!("SEQUENCE:{}", seq.unwrap_or(0) + 1)));
            let name = block_name(&cur.blocks[mi]);
            let mut block = vec![format!("BEGIN:{name}")];
            // properties before sub-components, as clients write them
            for p in &ps {
                if let Part::Line(l) = p {
                    block.push(l.clone());
                }
            }
            for p in &ps {
                if let Part::Sub(_, lines) = p {
                    block.extend(lines.iter().cloned());
                }
            }
            block.push(format!("END:{name}"));
            cur.blocks[mi] = block;
            let end = cur.head.iter().rposition(|l| prop_name(l) == "END").unwrap_or(cur.head.len());
            let mut lines: Vec<String> = cur.head[..end].to_vec();
            for b in &cur.blocks {
                lines.extend(b.iter().cloned());
            }
            lines.extend(cur.head[end..].iter().cloned());
            lines.iter().map(|l| ical::fold(l)).collect::<Vec<_>>().join("\r\n") + "\r\n"
        }
    }
}

// ---- readable values ---------------------------------------------------------------------------

/// What a viewer needs to read values: language, zone, and the labels.
pub struct Show<'a> {
    pub t: &'a (dyn Fn(&str) -> String + Sync),
    pub lang: &'a str,
    pub tz: chrono_tz::Tz,
}

impl Show<'_> {
    fn date(&self, d: NaiveDate) -> String {
        let wd = (self.t)(&format!("dow-{}", d.weekday().number_from_monday()));
        let (y, m, day) = (d.year(), d.month(), d.day());
        match self.lang {
            "en" => format!("{wd} {m}/{day}/{y}"),
            "de" => format!("{wd} {day}.{m}.{y}"),
            _ => format!("{wd} {day}/{m}/{y}"),
        }
    }
    fn datetime(&self, dt: NaiveDateTime) -> String {
        format!("{} {}", self.date(dt.date()), dt.format("%H:%M"))
    }
    /// An iCalendar DATE / DATE-TIME (floating, TZID or UTC) in the viewer's zone.
    fn when(&self, params: &str, v: &str) -> String {
        let v = v.trim();
        if v.len() == 8
            && let Ok(d) = NaiveDate::parse_from_str(v, "%Y%m%d")
        {
            return self.date(d);
        }
        let Ok(dt) = NaiveDateTime::parse_from_str(v.trim_end_matches('Z'), "%Y%m%dT%H%M%S") else { return v.to_string() };
        let utc = if v.ends_with('Z') {
            Some(chrono::Utc.from_utc_datetime(&dt))
        } else {
            param(params, "TZID").and_then(ical::parse_tz).and_then(|z| z.from_local_datetime(&dt).earliest()).map(|x| x.with_timezone(&chrono::Utc))
        };
        match utc {
            Some(u) => self.datetime(u.with_timezone(&self.tz).naive_local()),
            None => self.datetime(dt),
        }
    }
    /// (negative, minutes) of an iCalendar DURATION
    fn dur(&self, v: &str) -> (bool, i64) {
        let neg = v.starts_with('-');
        let s = v.trim_start_matches(['-', '+']).trim_start_matches('P');
        let (mut mins, mut num) = (0i64, 0i64);
        for c in s.chars() {
            match c {
                '0'..='9' => num = num * 10 + i64::from(c as u8 - b'0'),
                'W' => { mins += num * 10080; num = 0 }
                'D' => { mins += num * 1440; num = 0 }
                'H' => { mins += num * 60; num = 0 }
                'M' => { mins += num; num = 0 }
                'S' => num = 0,
                _ => {}
            }
        }
        (neg, mins)
    }
    /// "2 Std." of a duration's minutes
    fn amount(&self, mins: i64) -> String {
        let (n, unit) = if mins % 10080 == 0 { (mins / 10080, "w") } else if mins % 1440 == 0 { (mins / 1440, "d") } else if mins % 60 == 0 { (mins / 60, "h") } else { (mins, "min") };
        format!("{n} {}", (self.t)(&format!("hx-unit-{unit}")))
    }
    /// an alarm trigger offset: "1 Std. vorher"
    fn offset(&self, v: &str) -> String {
        let (neg, mins) = self.dur(v);
        if mins == 0 {
            return (self.t)("hx-al-at-start");
        }
        (self.t)(if neg { "hx-al-before" } else { "hx-al-after" }).replace("%n", &self.amount(mins))
    }
    fn rrule(&self, v: &str) -> String {
        let mut f = BTreeMap::new();
        for p in v.split(';') {
            if let Some((k, x)) = p.split_once('=') {
                f.insert(k.to_ascii_uppercase(), x.to_string());
            }
        }
        let freq = f.get("FREQ").map(|s| s.to_ascii_lowercase()).unwrap_or_default();
        let every: i64 = f.get("INTERVAL").and_then(|s| s.parse().ok()).unwrap_or(1);
        let mut out = (self.t)(&format!("hx-rr-{freq}"));
        if every > 1 {
            out = (self.t)(&format!("hx-rr-{freq}-n")).replace("%n", &every.to_string());
        }
        if let Some(days) = f.get("BYDAY") {
            let names: Vec<String> = days.split(',').filter_map(|d| {
                let code = d.trim_start_matches(|c: char| c == '-' || c == '+' || c.is_ascii_digit());
                let i = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"].iter().position(|x| *x == code)?;
                Some((self.t)(&format!("dow-{}", i + 1)))
            }).collect();
            if !names.is_empty() {
                out = format!("{out}, {}", names.join(" "));
            }
        }
        if let Some(u) = f.get("UNTIL") {
            out = format!("{out}, {}", (self.t)("hx-rr-until").replace("%date", &self.when("", u)));
        }
        if let Some(c) = f.get("COUNT") {
            out = format!("{out}, {c}×");
        }
        out
    }
    /// One line (`;PARAMS:value`) of `key`, readable.
    fn line(&self, key: &str, line: &str) -> String {
        let (params, v) = split_value(line);
        let text = || ical::unescape(v).trim().to_string();
        let parts = || v.split(';').map(|x| ical::unescape(x).trim().to_string()).filter(|x| !x.is_empty()).collect::<Vec<_>>();
        match key {
            "DTSTART" | "DTEND" | "DUE" | "COMPLETED" | "RECURRENCE-ID" | "ACKNOWLEDGED" | "X-MOZ-LASTACK" => self.when(params, v),
            "EXDATE" | "RDATE" => v.split(',').map(|x| self.when(params, x)).collect::<Vec<_>>().join(", "),
            "BDAY" | "ANNIVERSARY" | "DEATHDATE" => crate::domain::contacts::fmt_partial(v),
            "RRULE" => self.rrule(v),
            "DURATION" => self.amount(self.dur(v).1),
            "ATTENDEE" | "ORGANIZER" => param(params, "CN").map(str::to_string).unwrap_or_else(|| v.trim_start_matches("mailto:").trim_start_matches("MAILTO:").to_string()),
            "CATEGORIES" => ical::split_list(v).join(", "),
            "ADR" | "N" | "ORG" => {
                let mut p = parts();
                if key == "N" {
                    p.reverse();
                }
                p.join(if key == "ADR" { ", " } else { " " })
            }
            "PRIORITY" => match v.trim().parse::<u8>() {
                Ok(1..=4) => (self.t)("hx-prio-high"),
                Ok(5) => (self.t)("hx-prio-mid"),
                Ok(6..=9) => (self.t)("hx-prio-low"),
                _ => "–".into(),
            },
            "PERCENT-COMPLETE" => format!("{} %", v.trim()),
            "STATUS" | "CLASS" | "TRANSP" => {
                let k = format!("hx-v-{}", v.trim().to_ascii_lowercase());
                let s = (self.t)(&k);
                if s == k { text() } else { s }
            }
            "ATTACH" => param(params, "FILENAME").or(param(params, "X-FILENAME")).map(str::to_string).unwrap_or_else(|| if v == "…" { (self.t)("hx-file") } else { text() }),
            "PHOTO" | "LOGO" | "SOUND" => (self.t)("hx-file"),
            "TEL" | "EMAIL" | "URL" | "IMPP" => {
                let ty = param(params, "TYPE").map(|t| t.to_ascii_lowercase());
                match ty {
                    Some(t) if !t.is_empty() => format!("{} ({})", text(), t.replace(',', ", ")),
                    _ => text(),
                }
            }
            _ => text(),
        }
    }
    /// A recorded value of `key` (see `attrs`), readable; "" stays "".
    pub fn value(&self, key: &str, raw: &str, colls: &(dyn Fn(&str) -> String + Sync)) -> String {
        if raw.is_empty() {
            return String::new();
        }
        if key == COLL_KEY {
            return colls(raw);
        }
        if key == "VALARM" {
            // one entry per alarm: its trigger
            return raw.split("BEGIN:VALARM").filter(|b| !b.trim().is_empty()).map(|b| {
                b.lines().find(|l| prop_name(l) == "TRIGGER").map(|l| {
                    let (p, v) = split_value(rest(l));
                    if v.len() >= 15 && v.contains('T') && !v.starts_with(['-', '+', 'P']) { self.when(p, v) } else {
                        let s = self.offset(v);
                        if param(p, "RELATED").is_some_and(|r| r.eq_ignore_ascii_case("END")) { format!("{s} ({})", (self.t)("hx-al-end")) } else { s }
                    }
                }).unwrap_or_else(|| (self.t)("hx-alarm"))
            }).collect::<Vec<_>>().join(", ");
        }
        if key.starts_with("RECURRENCE-ID:") {
            // an override: its start and title
            let lines: Vec<&str> = raw.lines().collect();
            let get = |n: &str| lines.iter().find(|l| prop_name(l) == n).map(|l| rest(l));
            let start = get("DTSTART").map(|r| self.line("DTSTART", r)).unwrap_or_default();
            let sum = get("SUMMARY").map(|r| self.line("SUMMARY", r)).unwrap_or_default();
            return [start, sum].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
        }
        if key.starts_with('V') && raw.starts_with("BEGIN:") {
            return (self.t)("hx-component");
        }
        let vals: Vec<String> = raw.split('\n').map(|l| self.line(key, l)).filter(|s| !s.is_empty()).collect();
        let s = vals.join(if matches!(key, "DESCRIPTION" | "NOTE") { "\n" } else { ", " });
        if s.chars().count() > 600 { format!("{}…", s.chars().take(600).collect::<String>()) } else { s }
    }
    /// The label of an attribute key.
    pub fn label(&self, key: &str) -> String {
        if key == COLL_KEY {
            return (self.t)("hx-a-coll");
        }
        if let Some(r) = key.strip_prefix("RECURRENCE-ID:") {
            return (self.t)("hx-a-override").replace("%date", &self.when("", r));
        }
        let k = format!("hx-a-{}", key.to_ascii_lowercase());
        let s = (self.t)(&k);
        if s == k { key.to_string() } else { s }
    }
}

/// A device name from a PRODID (`-//Apple Inc.//iOS 26.0//EN` → "Apple iOS").
pub fn device(prodid: Option<&str>) -> Option<String> {
    let p = prodid?.trim();
    let l = p.to_ascii_lowercase();
    let known = [
        ("davx5", "DAVx⁵"), ("bitfire", "DAVx⁵"), ("jtx", "jtx Board"), ("thunderbird", "Thunderbird"), ("mozilla", "Thunderbird"),
        ("evolution", "Evolution"), ("outlook", "Outlook"), ("microsoft", "Outlook"), ("google", "Google"), ("tasks.org", "Tasks.org"),
        ("ios", "Apple iOS"), ("iphone", "Apple iOS"), ("macos", "Apple macOS"), ("mac os", "Apple macOS"), ("apple", "Apple"),
        ("nextcloud", "Nextcloud"), ("sabre", "SabreDAV"), ("rustical-ui", "rustical-ui"), ("fossify", "Fossify"), ("etar", "Etar"),
    ];
    if let Some((_, n)) = known.iter().find(|(k, _)| l.contains(k)) {
        return Some((*n).to_string());
    }
    let parts: Vec<&str> = p.split("//").map(str::trim).filter(|s| !s.is_empty() && *s != "-" && s.len() > 2).collect();
    let name = parts.iter().take(2).copied().collect::<Vec<_>>().join(" ");
    (!name.is_empty()).then(|| name.chars().take(40).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const EV: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Apple Inc.//iOS 26.0//EN\r\nBEGIN:VEVENT\r\nUID:u1\r\nDTSTAMP:20260901T100000Z\r\nSEQUENCE:2\r\nSUMMARY:Zahnreinigung\r\nDTSTART:20261006T080000Z\r\nDESCRIPTION:Bonusheft\r\nATTENDEE;CN=Anna:mailto:a@x\r\nATTENDEE;CN=Ben:mailto:b@x\r\nBEGIN:VALARM\r\nACTION:DISPLAY\r\nTRIGGER:-PT1H\r\nEND:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    fn edit(s: &str, from: &str, to: &str) -> String {
        assert!(s.contains(from), "{from}");
        s.replace(from, to)
    }

    #[test]
    fn attributes_group_lines_and_order_does_not_count() {
        let a = attrs(Store::Cal, EV);
        assert_eq!(a["SUMMARY"], ":Zahnreinigung");
        assert_eq!(a["ATTENDEE"].lines().count(), 2);
        assert!(a["VALARM"].contains("TRIGGER:-PT1H"));
        let swapped = edit(EV, "ATTENDEE;CN=Anna:mailto:a@x\r\nATTENDEE;CN=Ben:mailto:b@x", "ATTENDEE;CN=Ben:mailto:b@x\r\nATTENDEE;CN=Anna:mailto:a@x");
        assert!(diff(&a, &attrs(Store::Cal, &swapped)).is_empty());
    }

    #[test]
    fn a_dismissed_reminder_is_technical() {
        let acked = edit(EV, "TRIGGER:-PT1H\r\n", "TRIGGER:-PT1H\r\nACKNOWLEDGED:20261006T090000Z\r\n");
        let acked = edit(&acked, "DTSTAMP:20260901T100000Z", "DTSTAMP:20261006T090000Z");
        let d = diff(&attrs(Store::Cal, EV), &attrs(Store::Cal, &acked));
        assert_eq!(d.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), ["ACKNOWLEDGED", "DTSTAMP"]);
        assert!(d.iter().all(|x| is_tech(&x.0)));
    }

    #[test]
    fn revert_puts_back_only_the_named_attributes() {
        // before → after changed the note; later the title changed too; undo the note only
        let after = edit(EV, "DESCRIPTION:Bonusheft", "DESCRIPTION:Bonusheft + Karte");
        let current = edit(&after, "SUMMARY:Zahnreinigung", "SUMMARY:Zahnarzt");
        let out = revert(Store::Cal, &current, EV, &["DESCRIPTION".into()]);
        let a = attrs(Store::Cal, &out);
        assert_eq!(a["DESCRIPTION"], ":Bonusheft");
        assert_eq!(a["SUMMARY"], ":Zahnarzt");
        assert_eq!(a["SEQUENCE"], ":3");
        assert!(rustical_ical::CalendarObject::import(&out, None).is_ok(), "{out}");
    }

    #[test]
    fn revert_restores_removed_attributes_and_alarms() {
        let after = edit(EV, "DESCRIPTION:Bonusheft\r\n", "");
        let after = edit(&after, "BEGIN:VALARM\r\nACTION:DISPLAY\r\nTRIGGER:-PT1H\r\nEND:VALARM\r\n", "");
        let out = revert(Store::Cal, &after, EV, &["DESCRIPTION".into(), "VALARM".into()]);
        let a = attrs(Store::Cal, &out);
        assert_eq!(a["DESCRIPTION"], ":Bonusheft");
        assert!(a["VALARM"].contains("TRIGGER:-PT1H"));
        assert!(rustical_ical::CalendarObject::import(&out, None).is_ok(), "{out}");
    }

    #[test]
    fn vcards_revert_by_name_without_group() {
        let v = "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:c1\r\nFN:Jana Weber\r\nitem1.TEL;TYPE=cell:+49 170 1\r\nNOTE:alt\r\nEND:VCARD\r\n";
        let after = edit(v, "item1.TEL;TYPE=cell:+49 170 1", "item1.TEL;TYPE=cell:+49 160 2");
        let current = edit(&after, "NOTE:alt", "NOTE:neu");
        let a = attrs(Store::Ab, &after);
        assert_eq!(a["TEL"], ";TYPE=cell:+49 160 2");
        let out = revert(Store::Ab, &current, v, &["TEL".into()]);
        assert!(out.contains("item1.TEL;TYPE=cell:+49 170 1") && out.contains("NOTE:neu") && out.contains("REV:"), "{out}");
    }

    #[test]
    fn values_read_like_the_editor() {
        let t = |k: &str| match k {
            "dow-2" => "Di".to_string(),
            "hx-unit-h" => "Std.".into(),
            "hx-al-before" => "%n vorher".into(),
            "hx-rr-weekly" => "wöchentlich".into(),
            "hx-rr-until" => "bis %date".into(),
            "dow-3" => "Mi".into(),
            other => other.to_string(),
        };
        let s = Show { t: &t, lang: "de", tz: chrono_tz::Europe::Berlin };
        let none = |x: &str| x.to_string();
        assert_eq!(s.value("DTSTART", ";TZID=Europe/Berlin:20261006T100000", &none), "Di 6.10.2026 10:00");
        assert_eq!(s.value("DTSTART", ":20261006T080000Z", &none), "Di 6.10.2026 10:00");
        assert_eq!(s.value("VALARM", "BEGIN:VALARM\nACTION:DISPLAY\nTRIGGER:-PT1H\nEND:VALARM", &none), "1 Std. vorher");
        assert_eq!(s.value("RRULE", ":FREQ=WEEKLY;BYDAY=WE;UNTIL=20261216T225900Z", &none), "wöchentlich, Mi, bis Mi 16.12.2026 23:59");
        assert_eq!(s.value("ATTENDEE", ";CN=Anna:mailto:a@x\n;CN=Ben:mailto:b@x", &none), "Anna, Ben");
        assert_eq!(device(Some("-//Apple Inc.//iOS 26.0//EN")).as_deref(), Some("Apple iOS"));
        assert_eq!(device(Some("-//Acme Corp//Planner 3//EN")).as_deref(), Some("Acme Corp Planner 3"));
    }
}
