//! Small iCalendar/vCard helpers on top of caldata's ContentLine model:
//! text (un)escaping, typed reads of untyped properties, time-zone folding
//! into the user's zone, and line folding for generated objects.

use caldata::component::Component;
use caldata::parser::ContentLine;
use caldata::types::{CalDateOrDateTime, Tz};
use chrono::{NaiveDate, NaiveDateTime, TimeZone};

/// RFC 5545 §3.3.11 TEXT unescape.
pub fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n' | 'N') => out.push('\n'),
                Some(o) => out.push(o),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            c => out.push(c),
        }
    }
    out
}

/// Split a TEXT list value on unescaped commas.
pub fn split_list(s: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut esc = false;
    for c in s.chars() {
        if esc {
            cur.push('\\');
            cur.push(c);
            esc = false;
        } else if c == '\\' {
            esc = true;
        } else if c == ',' {
            out.push(unescape(&cur));
            cur.clear();
        } else {
            cur.push(c);
        }
    }
    if !cur.is_empty() {
        out.push(unescape(&cur));
    }
    out.into_iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

pub fn text<C: Component>(c: &C, name: &str) -> Option<String> {
    c.get_property(name).map(|p| unescape(&p.value)).filter(|s| !s.is_empty())
}

pub fn text_line(line: &ContentLine) -> String {
    unescape(&line.value)
}

pub fn list<C: Component>(c: &C, name: &str) -> Vec<String> {
    c.get_named_properties(name).flat_map(|p| split_list(&p.value)).collect()
}

/// Fold a content line at 75 octets (RFC 5545 §3.1).
pub fn fold(line: &str) -> String {
    let bytes = line.as_bytes();
    if bytes.len() <= 75 {
        return line.to_string();
    }
    let mut out = String::with_capacity(line.len() + line.len() / 70 * 3);
    let mut start = 0;
    let mut first = true;
    while start < bytes.len() {
        let limit = if first { 75 } else { 74 };
        let mut end = (start + limit).min(bytes.len());
        while end < bytes.len() && !line.is_char_boundary(end) {
            end -= 1;
        }
        if !first {
            out.push_str("\r\n ");
        }
        out.push_str(&line[start..end]);
        start = end;
        first = false;
    }
    out
}

/// Local wall-clock time of a date/date-time in the user's zone.
/// Floating ("Local") times are taken as already being in the user's zone.
pub fn local(dt: &CalDateOrDateTime, tz: chrono_tz::Tz) -> (NaiveDateTime, bool) {
    match dt {
        CalDateOrDateTime::Date(d) => (d.naive_date().and_hms_opt(0, 0, 0).unwrap(), true),
        CalDateOrDateTime::DateTime(d) => {
            if d.timezone().is_local() {
                // caldata models floating time as Tz::Local with a UTC offset
                (d.utc().naive_utc(), false)
            } else {
                (d.utc().with_timezone(&tz).naive_local(), false)
            }
        }
    }
}

pub fn local_date(dt: &CalDateOrDateTime, tz: chrono_tz::Tz) -> NaiveDate {
    local(dt, tz).0.date()
}

/// Format a local wall-clock time for an iCalendar DTSTART/DTEND with TZID.
pub fn fmt_dt(dt: NaiveDateTime) -> String {
    dt.format("%Y%m%dT%H%M%S").to_string()
}

pub fn fmt_utc_now() -> String {
    chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string()
}

pub fn fmt_date(d: NaiveDate) -> String {
    d.format("%Y%m%d").to_string()
}

pub fn parse_tz(name: &str) -> Option<chrono_tz::Tz> {
    name.parse().ok()
}

pub fn tz_of(tz: &Tz) -> Option<chrono_tz::Tz> {
    match tz {
        Tz::Olson(t) => Some(*t),
        Tz::Local => None,
    }
}

/// VTIMEZONE block for an Olson id, from the vtimezones-rs table RustiCal itself uses.
pub fn vtimezone(tzid: &str) -> Option<String> {
    let raw = vtimezones_rs::VTIMEZONES.get(tzid).copied()?;
    // the table entries are full VCALENDAR documents; take the VTIMEZONE block only
    let start = raw.find("BEGIN:VTIMEZONE")?;
    let end = raw.find("END:VTIMEZONE")? + "END:VTIMEZONE".len();
    Some(raw[start..end].replace("\r\n", "\n").replace('\n', "\r\n"))
}

/// Wall-clock in `tz` → UTC (first mapping on DST gaps/overlaps).
pub fn to_utc(dt: NaiveDateTime, tz: chrono_tz::Tz) -> chrono::DateTime<chrono::Utc> {
    tz.from_local_datetime(&dt).earliest().unwrap_or_else(|| tz.from_utc_datetime(&dt)).with_timezone(&chrono::Utc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaping_roundtrip() {
        let s = "a, b; c\\d\nline2";
        assert_eq!(unescape(&escape(s)), s);
        assert_eq!(split_list("Reise\\, privat,Arbeit, ,Urlaub"), vec!["Reise, privat", "Arbeit", "Urlaub"]);
    }

    #[test]
    fn folding() {
        let long = "DESCRIPTION:".to_string() + &"ä".repeat(100);
        let folded = fold(&long);
        for l in folded.split("\r\n") {
            assert!(l.len() <= 75, "{}", l.len());
        }
        assert_eq!(folded.replace("\r\n ", ""), long);
    }
}

/// Splits one unfolded content line into (NAME, [(PARAM, value)…], value); quoted parameter
/// values may contain `;`/`:`/`,`. Parameter names are upper-cased, values kept.
pub fn split_line(line: &str) -> (String, Vec<(String, String)>, String) {
    let mut head_end = line.len();
    let mut quoted = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => quoted = !quoted,
            ':' if !quoted => {
                head_end = i;
                break;
            }
            _ => {}
        }
    }
    let (head, value) = (&line[..head_end], line.get(head_end + 1..).unwrap_or(""));
    let mut parts = vec![];
    let mut cur = String::new();
    quoted = false;
    for c in head.chars() {
        match c {
            '"' => quoted = !quoted,
            ';' if !quoted => parts.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    parts.push(cur);
    let name = parts[0].rsplit('.').next().unwrap_or("").to_ascii_uppercase();
    let params = parts[1..].iter().filter_map(|p| p.split_once('=').map(|(k, v)| (k.to_ascii_uppercase(), v.to_string()))).collect();
    (name, params, value.to_string())
}
