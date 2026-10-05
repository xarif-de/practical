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
        || key.starts_with("X-MICROSOFT-")
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
            let lines = events::unfold(text);
            let group = |l: &str| prop_name(l).rsplit_once('.').map(|(g, _)| g.to_string());
            // Apple writes a custom label as `itemN.X-ABLabel` next to `itemN.TEL`: it is part of that value
            let labels: BTreeMap<String, String> = lines.iter().filter(|l| vcard_name(l) == "X-ABLABEL").filter_map(|l| Some((group(l)?, split_value(rest(l)).1.to_string()))).collect();
            for l in &lines {
                let n = vcard_name(l);
                if l.is_empty() || matches!(n.as_str(), "BEGIN" | "END" | "VERSION" | "UID" | "X-ABLABEL") {
                    continue;
                }
                let r = match group(l).and_then(|g| labels.get(&g)) {
                    Some(lab) => {
                        let (p, v) = split_value(rest(l));
                        format!("{p};X-HX-LABEL=\"{}\":{v}", lab.replace('"', "'"))
                    }
                    None => rest(l).to_string(),
                };
                m.entry(n).or_default().push(shorten(&r));
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
            let group = |l: &str| prop_name(l).rsplit_once('.').map(|(g, _)| g.to_string());
            let mut out: Vec<String> = cur.clone();
            // a grouped value's Apple label (`itemN.X-ABLabel`) goes and comes with it; the old
            // lines get fresh group names, so they cannot take over a label of the current text
            let mut next = out.iter().chain(old.iter()).filter_map(|l| group(l)).filter_map(|g| g.trim_start_matches(|c: char| !c.is_ascii_digit()).parse::<u32>().ok()).max().unwrap_or(0) + 1;
            for k in keys {
                let pos = out.iter().position(|l| &vcard_name(l) == k);
                let gone: Vec<String> = out.iter().filter(|l| &vcard_name(l) == k).filter_map(|l| group(l)).collect();
                out.retain(|l| &vcard_name(l) != k && !(vcard_name(l) == "X-ABLABEL" && group(l).is_some_and(|g| gone.contains(&g))));
                let mut add: Vec<String> = vec![];
                for l in old.iter().filter(|l| &vcard_name(l) == k) {
                    match group(l) {
                        Some(g) => {
                            let fresh = format!("item{next}");
                            next += 1;
                            let rename = |x: &str| format!("{fresh}{}", &x[g.len()..]);
                            add.push(rename(l));
                            add.extend(old.iter().filter(|x| vcard_name(x) == "X-ABLABEL" && group(x).as_deref() == Some(g.as_str())).map(|x| rename(x)));
                        }
                        None => add.push(l.clone()),
                    }
                }
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
    fn tr(&self, key: &str, fallback: &str) -> String {
        let s = (self.t)(key);
        if s == key { fallback.to_string() } else { s }
    }
    /// A code value (STATUS, PARTSTAT, ROLE, …) through `hx-v-<code>`, else as written.
    fn code(&self, v: &str) -> String {
        let v = v.trim();
        self.tr(&format!("hx-v-{}", v.to_ascii_lowercase()), v)
    }
    /// TYPE of a vCard line (and an Apple label of its group, folded in by `attrs`), readable.
    fn kind_of(&self, params: &str) -> Option<String> {
        let mut out: Vec<String> = vec![];
        if let Some(l) = param(params, "X-HX-LABEL") {
            out.push(l.trim_start_matches("_$!<").trim_end_matches(">!$_").to_string());
        }
        for t in param(params, "TYPE").into_iter().flat_map(|t| t.split(',')) {
            let t = t.trim().to_ascii_lowercase();
            if !t.is_empty() && t != "internet" && t != "pref" && t != "voice" {
                out.push(self.tr(&format!("hx-t-{t}"), &t));
            }
        }
        if param(params, "PREF").is_some() || param(params, "TYPE").is_some_and(|t| t.to_ascii_lowercase().contains("pref")) {
            out.push((self.t)("hx-t-pref"));
        }
        (!out.is_empty()).then(|| out.join(", "))
    }
    /// A person line (ATTENDEE/ORGANIZER; vCard RELATED): name, address, and what the invitation says.
    fn person(&self, key: &str, params: &str, v: &str) -> String {
        let addr = v.trim().trim_start_matches("mailto:").trim_start_matches("MAILTO:");
        let mut out = match param(params, "CN") {
            Some(cn) if !addr.is_empty() && !cn.eq_ignore_ascii_case(addr) => format!("{cn} <{addr}>"),
            Some(cn) => cn.to_string(),
            None => addr.to_string(),
        };
        let mut marks = vec![];
        if key == "ATTENDEE" {
            if let Some(ps) = param(params, "PARTSTAT") {
                marks.push(self.code(ps));
            }
            if let Some(r) = param(params, "ROLE").filter(|r| !r.eq_ignore_ascii_case("REQ-PARTICIPANT")) {
                marks.push(self.code(r));
            }
            if let Some(c) = param(params, "CUTYPE").filter(|c| !c.eq_ignore_ascii_case("INDIVIDUAL")) {
                marks.push(self.code(c));
            }
        }
        if !marks.is_empty() {
            out = format!("{out} ({})", marks.join(", "));
        }
        out
    }
    /// One line (`;PARAMS:value`) of `key`, readable.
    fn line(&self, key: &str, line: &str) -> String {
        let (params, v) = split_value(line);
        let text = || ical::unescape(v).trim().to_string();
        let parts = || v.split(';').map(|x| ical::unescape(x).trim().to_string()).filter(|x| !x.is_empty()).collect::<Vec<_>>();
        let typed = |s: String| match self.kind_of(params) {
            Some(k) => format!("{s} ({k})"),
            None => s,
        };
        match key {
            "DTSTART" | "DTEND" | "DUE" | "COMPLETED" | "RECURRENCE-ID" | "ACKNOWLEDGED" | "X-MOZ-LASTACK" | "CREATED" | "LAST-MODIFIED" | "DTSTAMP" => self.when(params, v),
            "EXDATE" | "RDATE" => v.split(',').map(|x| self.when(params, x)).collect::<Vec<_>>().join("\n"),
            "BDAY" | "ANNIVERSARY" | "DEATHDATE" => crate::domain::contacts::fmt_partial(v),
            "X-ABDATE" => typed(crate::domain::contacts::fmt_partial(v)),
            "RRULE" => self.rrule(v),
            "DURATION" => self.amount(self.dur(v).1),
            "ATTENDEE" | "ORGANIZER" => self.person(key, params, v),
            "RELATED" | "X-ABRELATEDNAMES" => typed(text().trim_start_matches("urn:uuid:").to_string()),
            "CATEGORIES" | "RESOURCES" | "NICKNAME" => ical::split_list(v).join("\n"),
            "ADR" => typed(parts().join(", ")),
            "N" => {
                // family;given;additional;prefix;suffix → prefix given additional family suffix
                let f: Vec<String> = v.split(';').map(|x| ical::unescape(x).trim().to_string()).collect();
                let at = |i: usize| f.get(i).cloned().unwrap_or_default();
                [at(3), at(1), at(2), at(0), at(4)].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" ")
            }
            "ORG" => parts().join(" · "),
            "GEO" => v.replace(';', ", "),
            "PRIORITY" => match v.trim().parse::<u8>() {
                Ok(1..=4) => format!("{} ({})", (self.t)("hx-prio-high"), v.trim()),
                Ok(5) => (self.t)("hx-prio-mid"),
                Ok(6..=9) => format!("{} ({})", (self.t)("hx-prio-low"), v.trim()),
                _ => (self.t)("hx-prio-none"),
            },
            "PERCENT-COMPLETE" => format!("{} %", v.trim()),
            "STATUS" | "CLASS" | "TRANSP" | "ACTION" | "PROXIMITY" | "KIND" | "GENDER" => self.code(v),
            "ATTACH" | "IMAGE" | "SOUND" | "PHOTO" | "LOGO" => {
                let name = param(params, "FILENAME").or(param(params, "X-FILENAME")).map(str::to_string);
                let binary = v == "…" || v.starts_with("data:");
                match (name, binary) {
                    (Some(n), _) => n,
                    (None, true) => match param(params, "FMTTYPE") {
                        Some(t) => format!("{} ({t})", (self.t)("hx-file")),
                        None => (self.t)("hx-file"),
                    },
                    (None, false) => text(),
                }
            }
            "CONFERENCE" => match param(params, "LABEL") {
                Some(l) => format!("{l}: {}", text()),
                None => text(),
            },
            "RELATED-TO" => {
                let rel = param(params, "RELTYPE").unwrap_or("PARENT");
                format!("{}: {}", self.code(rel), text())
            }
            "TEL" => typed(text().trim_start_matches("tel:").to_string()),
            "EMAIL" | "URL" | "X-SOCIALPROFILE" => typed(text().trim_start_matches("mailto:").to_string()),
            "IMPP" => {
                let svc = param(params, "X-SERVICE-TYPE").map(str::to_string);
                let s = text();
                typed(match svc {
                    Some(x) => format!("{x}: {s}"),
                    None => s,
                })
            }
            // formatted copies of the note: their text without markup
            "X-ALT-DESC" | "STYLED-DESCRIPTION" => {
                let t = text();
                let mut out = String::new();
                let mut tag = false;
                for c in t.chars() {
                    match c {
                        '<' => tag = true,
                        '>' => tag = false,
                        c if !tag => out.push(c),
                        _ => {}
                    }
                }
                out.trim().to_string()
            }
            _ => text(),
        }
    }
    /// One alarm block (lines of `BEGIN:VALARM` … `END:VALARM`): when, how, and the rest of it.
    fn alarm(&self, block: &str, titles: &[&str]) -> (i64, String) {
        let lines: Vec<&str> = block.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        let get = |n: &str| lines.iter().find(|l| prop_name(l) == n).map(|l| split_value(rest(l)));
        let all = |n: &str| lines.iter().filter(|l| prop_name(l) == n).map(|l| split_value(rest(l))).collect::<Vec<_>>();
        let (when, order) = match get("TRIGGER") {
            Some((p, v)) if v.len() >= 15 && v.contains('T') && !v.starts_with(['-', '+', 'P']) || param(p, "VALUE").is_some_and(|x| x.eq_ignore_ascii_case("DATE-TIME")) => (self.when(p, v), i64::MIN),
            Some((p, v)) => {
                let (neg, m) = self.dur(v);
                let s = self.offset(v);
                let end = param(p, "RELATED").is_some_and(|r| r.eq_ignore_ascii_case("END"));
                (if end { format!("{s} ({})", (self.t)("hx-al-end")) } else { s }, if neg { m } else { -m })
            }
            None => ((self.t)("hx-alarm"), 0),
        };
        let mut out = vec![when];
        let action = get("ACTION").map(|(_, v)| v.trim()).unwrap_or("DISPLAY");
        out.push(self.tr(&format!("hx-v-action-{}", action.to_ascii_lowercase()), action));
        let to: Vec<String> = all("ATTENDEE").iter().map(|(p, v)| self.person("ORGANIZER", p, v)).collect();
        if !to.is_empty() {
            out.push((self.t)("hx-al-to").replace("%who", &to.join(", ")));
        }
        for n in ["SUMMARY", "DESCRIPTION"] {
            if let Some((_, v)) = get(n) {
                let t = ical::unescape(v).trim().to_string();
                if !t.is_empty() && !titles.contains(&t.as_str()) {
                    out.push(format!("„{t}“"));
                }
            }
        }
        if let Some((_, r)) = get("REPEAT") {
            let every = get("DURATION").map(|(_, d)| self.amount(self.dur(d).1)).unwrap_or_default();
            out.push((self.t)("hx-al-repeat").replace("%n", r.trim()).replace("%every", &every));
        }
        for (p, v) in all("ATTACH") {
            out.push(self.line("ATTACH", &format!("{p}:{v}")));
        }
        if let Some((_, v)) = get("PROXIMITY") {
            out.push(self.tr(&format!("hx-v-prox-{}", v.trim().to_ascii_lowercase()), v.trim()));
        }
        // anything else of the alarm (X-…, UID, RELATED-TO): named, so a change of it shows
        for l in &lines {
            let n = prop_name(l);
            if !matches!(n.as_str(), "BEGIN" | "END" | "TRIGGER" | "ACTION" | "ATTENDEE" | "SUMMARY" | "DESCRIPTION" | "REPEAT" | "DURATION" | "ATTACH" | "PROXIMITY" | "UID" | "X-WR-ALARMUID") {
                out.push(format!("{n}: {}", ical::unescape(split_value(rest(l)).1)));
            }
        }
        (order, out.join(" · "))
    }
    /// A recorded value of `key` (see `attrs`), readable; "" stays "". Several values (lines,
    /// alarms, attendees, list items) come one per line, so the page can show which ones changed.
    /// `titles`: the element's titles, which an alarm's text only repeats.
    pub fn value_in(&self, key: &str, raw: &str, colls: &(dyn Fn(&str) -> String + Sync), titles: &[&str]) -> String {
        if raw.is_empty() {
            return String::new();
        }
        if key == COLL_KEY {
            return colls(raw);
        }
        if key == "VALARM" {
            let mut al: Vec<(i64, String)> = raw.split("BEGIN:VALARM").filter(|b| !b.trim().is_empty()).map(|b| self.alarm(b, titles)).collect();
            al.sort();
            return al.into_iter().map(|a| a.1).collect::<Vec<_>>().join("\n");
        }
        if key.starts_with("RECURRENCE-ID:") {
            // an override: its start, end, title, place, status — what makes it differ from the series
            let lines: Vec<&str> = raw.lines().collect();
            let mut out = vec![];
            for n in ["DTSTART", "DTEND", "SUMMARY", "LOCATION", "STATUS", "DESCRIPTION"] {
                if let Some(l) = lines.iter().find(|l| prop_name(l) == n) {
                    let v = self.line(n, rest(l));
                    if !v.is_empty() {
                        out.push(if n == "DTSTART" || n == "SUMMARY" { v } else { format!("{}: {v}", self.label(n)) });
                    }
                }
            }
            return out.join(" · ");
        }
        if raw.starts_with("BEGIN:") {
            // another sub-component (VLOCATION, PARTICIPANT, …): its lines
            return raw.lines().filter(|l| !matches!(prop_name(l).as_str(), "BEGIN" | "END" | "UID")).map(|l| format!("{}: {}", prop_name(l), ical::unescape(split_value(rest(l)).1))).collect::<Vec<_>>().join(" · ");
        }
        let vals: Vec<String> = raw.split('\n').map(|l| self.line(key, l)).filter(|s| !s.is_empty()).collect();
        let s = vals.join("\n");
        if s.chars().count() > 1200 { format!("{}…", s.chars().take(1200).collect::<String>()) } else { s }
    }
    /// `value_in` without titles (the tests' shorthand)
    #[cfg(test)]
    pub fn value(&self, key: &str, raw: &str, colls: &(dyn Fn(&str) -> String + Sync)) -> String {
        self.value_in(key, raw, colls, &[])
    }
    /// The recorded value as written (parameters included), for when the readable ones of two
    /// different versions read the same (a changed parameter the reading leaves out).
    pub fn raw(&self, raw: &str) -> String {
        raw.lines().map(|l| ical::unescape(l.trim_start_matches(':'))).collect::<Vec<_>>().join("\n")
    }
    /// The label of an attribute key.
    pub fn label(&self, key: &str) -> String {
        if key == COLL_KEY {
            return (self.t)("hx-a-coll");
        }
        if let Some(r) = key.strip_prefix("RECURRENCE-ID:") {
            return (self.t)("hx-a-override").replace("%date", &self.when("", r));
        }
        self.tr(&format!("hx-a-{}", key.to_ascii_lowercase()), key)
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
        ("nextcloud", "Nextcloud"), ("sabre", "SabreDAV"), ("rustical-ui", "PractiCal"), ("practical", "PractiCal"), ("fossify", "Fossify"), ("etar", "Etar"),
    ];
    if let Some((_, n)) = known.iter().find(|(k, _)| l.contains(k)) {
        return Some((*n).to_string());
    }
    let parts: Vec<&str> = p.split("//").map(str::trim).filter(|s| !s.is_empty() && *s != "-" && s.len() > 2).collect();
    let name = parts.iter().take(2).copied().collect::<Vec<_>>().join(" ");
    (!name.is_empty()).then(|| name.chars().take(40).collect())
}

#[cfg(test)]
mod pinned;

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
        assert!(out.contains(".TEL;TYPE=cell:+49 170 1") && out.contains("NOTE:neu") && out.contains("REV:"), "{out}");
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
            "hx-v-action-display" => "Anzeige".into(),
            "hx-v-accepted" => "zugesagt".into(),
            other => other.to_string(),
        };
        let s = Show { t: &t, lang: "de", tz: chrono_tz::Europe::Berlin };
        let none = |x: &str| x.to_string();
        assert_eq!(s.value("DTSTART", ";TZID=Europe/Berlin:20261006T100000", &none), "Di 6.10.2026 10:00");
        assert_eq!(s.value("DTSTART", ":20261006T080000Z", &none), "Di 6.10.2026 10:00");
        assert_eq!(s.value("VALARM", "BEGIN:VALARM\nACTION:DISPLAY\nTRIGGER:-PT1H\nEND:VALARM", &none), "1 Std. vorher · Anzeige");
        assert_eq!(s.value("RRULE", ":FREQ=WEEKLY;BYDAY=WE;UNTIL=20261216T225900Z", &none), "wöchentlich, Mi, bis Mi 16.12.2026 23:59");
        assert_eq!(s.value("ATTENDEE", ";CN=Anna:mailto:a@x\n;CN=Ben;PARTSTAT=ACCEPTED:mailto:b@x", &none), "Anna <a@x>\nBen <b@x> (zugesagt)");
        assert_eq!(device(Some("-//Apple Inc.//iOS 26.0//EN")).as_deref(), Some("Apple iOS"));
        assert_eq!(device(Some("-//Acme Corp//Planner 3//EN")).as_deref(), Some("Acme Corp Planner 3"));
    }

    /// Every attribute a version can differ in reads differently before and after: each property
    /// the full demo event carries is edited once (alarm kind, trigger, attendee answer, …) and the
    /// changed attribute must show both a label of its own and two different readable values.
    #[test]
    fn every_edit_of_the_full_event_reads_differently() {
        let i18n = crate::i18n::I18n::load();
        let t = |k: &str| i18n.t("de", k);
        let show = Show { t: &t, lang: "de", tz: chrono_tz::Europe::Berlin };
        let none = |x: &str| x.to_string();
        let base = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/demo-event-full.ics")).unwrap().replace("\r\n", "\n");
        let a = attrs(Store::Cal, &base);
        // every attribute has a label of its own (technical ones included, X- ones of other apps may not)
        for k in a.keys() {
            let l = show.label(k);
            assert!(l != *k || k.starts_with("X-") || k.starts_with("RECURRENCE-ID:"), "no label for {k}");
            assert!(!show.value(k, &a[k], &none).is_empty() || a[k].is_empty(), "no value for {k}");
        }
        let edits: &[(&str, &str)] = &[
            ("ACTION:DISPLAY\nTRIGGER:-PT15M", "ACTION:AUDIO\nTRIGGER:-PT15M"),
            ("ACTION:DISPLAY\nTRIGGER:-PT15M", "ACTION:EMAIL\nTRIGGER:-PT15M"),
            ("TRIGGER:-PT15M", "TRIGGER:-PT20M"),
            ("REPEAT:2", "REPEAT:3"),
            ("PROXIMITY:DEPART", "PROXIMITY:ARRIVE"),
            ("PARTSTAT=ACCEPTED;RSVP=TRUE;CUTYPE=INDIVIDUAL", "PARTSTAT=DECLINED;RSVP=TRUE;CUTYPE=INDIVIDUAL"),
            ("ROLE=OPT-PARTICIPANT", "ROLE=REQ-PARTICIPANT"),
            ("LABEL=Videocall", "LABEL=Telefon"),
            ("RELTYPE=PARENT", "RELTYPE=SIBLING"),
            ("GEO:51.050407;13.737262", "GEO:51.1;13.8"),
            ("EXDATE;TZID=Europe/Berlin:20261006T190000", "EXDATE;TZID=Europe/Berlin:20261013T190000"),
            ("RESOURCES:Beamer,Flipchart", "RESOURCES:Beamer"),
            ("CONTACT:Kasse", "CONTACT:Abendkasse"),
            ("URL:https://www.kulturpalast", "URL:https://www.semperoper"),
            ("ATTACH:https://example.org/programm.pdf", "ATTACH:https://example.org/programm-2.pdf"),
            ("COLOR:tomato", "COLOR:red"),
            ("CLASS:PRIVATE", "CLASS:CONFIDENTIAL"),
            ("TRANSP:OPAQUE", "TRANSP:TRANSPARENT"),
            ("STATUS:CONFIRMED", "STATUS:CANCELLED"),
            ("PRIORITY:5", "PRIORITY:1"),
        ];
        for (from, to) in edits {
            assert!(base.contains(from), "fixture lacks {from:?}");
            let b = attrs(Store::Cal, &base.replacen(from, to, 1));
            let d: Vec<_> = diff(&a, &b).into_iter().filter(|(k, _, _)| !k.starts_with("X-OLD-")).collect();
            assert!(!d.is_empty(), "{from:?}: no difference found");
            for (k, x, y) in d {
                assert_ne!(show.value_in(&k, &x, &none, &["Demo"]), show.value_in(&k, &y, &none, &["Demo"]), "{from:?} → {to:?}: {k} reads the same");
            }
        }
    }

    #[test]
    fn every_edit_of_a_contact_reads_differently() {
        let i18n = crate::i18n::I18n::load();
        let t = |k: &str| i18n.t("de", k);
        let show = Show { t: &t, lang: "de", tz: chrono_tz::Europe::Berlin };
        let none = |x: &str| x.to_string();
        let v = "BEGIN:VCARD\nVERSION:3.0\nUID:c1\nN:Weber;Jana;;Dr.;\nFN:Dr. Jana Weber\nitem1.TEL;TYPE=CELL:+49 170 1\nitem1.X-ABLabel:Dienst\nEMAIL;TYPE=HOME:jana@example.org\nADR;TYPE=HOME:;;Lindenweg 7;Zwickau;;08056;DE\nBDAY:1988-03-14\nitem2.X-ABDATE:2010-06-01\nitem2.X-ABLabel:_$!<Anniversary>!$_\nRELATED;TYPE=spouse:Max\nIMPP;X-SERVICE-TYPE=Signal:signal:+49170\nORG:Praxis;Empfang\nNOTE:alt\nEND:VCARD\n";
        let a = attrs(Store::Ab, v);
        for k in a.keys() {
            assert_ne!(show.label(k), *k, "no label for {k}");
        }
        assert_eq!(show.value("TEL", &a["TEL"], &none), "+49 170 1 (Dienst, mobil)");
        let edits: &[(&str, &str)] = &[
            ("TYPE=CELL:+49 170 1", "TYPE=WORK:+49 170 1"),
            ("X-ABLabel:Dienst", "X-ABLabel:Privat"),
            ("TYPE=HOME:jana", "TYPE=WORK:jana"),
            ("Lindenweg 7", "Lindenweg 9"),
            ("N:Weber;Jana;;Dr.;", "N:Weber;Jana;;Prof.;"),
            ("BDAY:1988-03-14", "BDAY:1988-03-15"),
            ("X-ABDATE:2010-06-01", "X-ABDATE:2011-06-01"),
            ("TYPE=spouse", "TYPE=child"),
            ("X-SERVICE-TYPE=Signal", "X-SERVICE-TYPE=Threema"),
            ("ORG:Praxis;Empfang", "ORG:Praxis;Labor"),
        ];
        for (from, to) in edits {
            assert!(v.contains(from), "fixture lacks {from:?}");
            let b = attrs(Store::Ab, &v.replacen(from, to, 1));
            let d = diff(&a, &b);
            assert!(!d.is_empty(), "{from:?}: no difference found");
            for (k, x, y) in d {
                assert_ne!(show.value(&k, &x, &none), show.value(&k, &y, &none), "{from:?} → {to:?}: {k} reads the same");
            }
        }
        // the label travels with its number when that is undone
        let after = v.replace("item1.TEL;TYPE=CELL:+49 170 1\nitem1.X-ABLabel:Dienst\n", "");
        let back = revert(Store::Ab, &after, v, &["TEL".into()]);
        assert_eq!(attrs(Store::Ab, &back)["TEL"], a["TEL"], "{back}");
    }
}
