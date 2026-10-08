//! The change history's view of an object's text (`domain::history`): an iCalendar or vCard file as
//! attributes (property name → its lines), the difference of two versions, putting chosen
//! attributes of an older version back into the current one (undo), and readable values.
//!
//! Attribute keys: a master property's name (`SUMMARY`, `ATTENDEE`, …; all lines of a name are one
//! attribute), a sub-component's name (`VALARM`, `VLOCATION`, …; all blocks of a name are one),
//! `RECURRENCE-ID:<value>` for an override of one occurrence, and for vCards the property name
//! without its group prefix (`item1.TEL` → `TEL`). `ACKNOWLEDGED` (an alarm's dismissal) is split off
//! the alarm so that a device dismissing a reminder is a technical change, not an edited reminder.
//!
//! Readable values for the history page: [`Show`] (show.rs).

use crate::domain::{events, ical};
use chrono::{Datelike, NaiveDate, NaiveDateTime, TimeZone};
use std::collections::BTreeMap;

mod show;
pub use show::*;

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
mod tests;
