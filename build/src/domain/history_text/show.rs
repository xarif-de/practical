//! Readable values: how the history page names an attribute and shows its old and new value.

use super::*;

/// What a viewer needs to read values: language, zone, and the labels.
pub struct Show<'a> {
    /// a message of the viewer's language (`I18n::lookup`): None where no language has the key
    pub t: &'a (dyn Fn(&str) -> Option<String> + Sync),
    pub lang: &'a str,
    pub tz: chrono_tz::Tz,
}

impl Show<'_> {
    pub(super) fn date(&self, d: NaiveDate) -> String {
        let wd = self.s(&format!("dow-{}", d.weekday().number_from_monday()));
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
    pub(super) fn when(&self, params: &str, v: &str) -> String {
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
        format!("{n} {}", self.s(&format!("hx-unit-{unit}")))
    }
    /// an alarm trigger offset: "1 Std. vorher"
    fn offset(&self, v: &str) -> String {
        let (neg, mins) = self.dur(v);
        if mins == 0 {
            return self.s("hx-al-at-start");
        }
        self.s(if neg { "hx-al-before" } else { "hx-al-after" }).replace("%n", &self.amount(mins))
    }
    fn rrule(&self, v: &str) -> String {
        let v = ical::rrule_canon(v);
        let mut f = BTreeMap::new();
        for p in v.split(';') {
            if let Some((k, x)) = p.split_once('=') {
                f.insert(k.to_string(), x.to_string());
            }
        }
        let freq = f.get("FREQ").map(|s| s.to_ascii_lowercase()).unwrap_or_default();
        let every: i64 = f.get("INTERVAL").and_then(|s| s.parse().ok()).unwrap_or(1);
        // a frequency without words (HOURLY, MINUTELY, SECONDLY): the rule as written
        let Some(mut out) = (self.t)(&format!("hx-rr-{freq}")) else { return v };
        if every > 1 {
            out = self.tr(&format!("hx-rr-{freq}-n"), &out).replace("%n", &every.to_string());
        }
        if let Some(days) = f.get("BYDAY") {
            let names: Vec<String> = days.split(',').filter_map(|d| {
                let code = d.trim_start_matches(|c: char| c == '-' || c == '+' || c.is_ascii_digit());
                let i = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"].iter().position(|x| *x == code)?;
                Some(self.s(&format!("dow-{}", i + 1)))
            }).collect();
            if !names.is_empty() {
                out = format!("{out}, {}", names.join(" "));
            }
        }
        if let Some(u) = f.get("UNTIL") {
            out = format!("{out}, {}", self.s("hx-rr-until").replace("%date", &self.when("", u)));
        }
        if let Some(c) = f.get("COUNT") {
            out = format!("{out}, {c}×");
        }
        out
    }
    /// The message `key`, or `fallback` where no language has it (an attribute without a label: any X-
    /// property reads as its own name).
    fn tr(&self, key: &str, fallback: &str) -> String {
        (self.t)(key).unwrap_or_else(|| fallback.to_string())
    }
    /// A message every language has (the i18n tests check the fixed keys); its key should one be missing.
    pub(super) fn s(&self, key: &str) -> String {
        self.tr(key, key)
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
            out.push(self.s("hx-t-pref"));
        }
        (!out.is_empty()).then(|| out.join(", "))
    }
    /// A person line (ATTENDEE/ORGANIZER; vCard RELATED): name, address, and what the invitation says.
    fn person(&self, key: &str, params: &str, v: &str) -> String {
        let addr = ical::strip_mailto(v.trim());
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
    pub(super) fn line(&self, key: &str, line: &str) -> String {
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
                Ok(1..=4) => format!("{} ({})", self.s("hx-prio-high"), v.trim()),
                Ok(5) => self.s("hx-prio-mid"),
                Ok(6..=9) => format!("{} ({})", self.s("hx-prio-low"), v.trim()),
                _ => self.s("hx-prio-none"),
            },
            "PERCENT-COMPLETE" => format!("{} %", v.trim()),
            "STATUS" | "CLASS" | "TRANSP" | "ACTION" | "PROXIMITY" | "KIND" | "GENDER" => self.code(v),
            "ATTACH" | "IMAGE" | "SOUND" | "PHOTO" | "LOGO" => {
                let name = param(params, "FILENAME").or(param(params, "X-FILENAME")).map(str::to_string);
                let binary = v == "…" || v.starts_with("data:");
                match (name, binary) {
                    (Some(n), _) => n,
                    (None, true) => match param(params, "FMTTYPE") {
                        Some(t) => format!("{} ({t})", self.s("hx-file")),
                        None => self.s("hx-file"),
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
            "TEL" => typed(ical::strip_scheme(&text(), "tel:").to_string()),
            "EMAIL" | "URL" | "X-SOCIALPROFILE" => typed(ical::strip_mailto(&text()).to_string()),
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
    pub(super) fn alarm(&self, block: &str, titles: &[&str]) -> (i64, String) {
        let lines: Vec<&str> = block.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        let get = |n: &str| lines.iter().find(|l| prop_name(l) == n).map(|l| split_value(rest(l)));
        let all = |n: &str| lines.iter().filter(|l| prop_name(l) == n).map(|l| split_value(rest(l))).collect::<Vec<_>>();
        let (when, order) = match get("TRIGGER") {
            Some((p, v)) if v.len() >= 15 && v.contains('T') && !v.starts_with(['-', '+', 'P']) || param(p, "VALUE").is_some_and(|x| x.eq_ignore_ascii_case("DATE-TIME")) => (self.when(p, v), i64::MIN),
            Some((p, v)) => {
                let (neg, m) = self.dur(v);
                let s = self.offset(v);
                let end = param(p, "RELATED").is_some_and(|r| r.eq_ignore_ascii_case("END"));
                (if end { format!("{s} ({})", self.s("hx-al-end")) } else { s }, if neg { m } else { -m })
            }
            None => (self.s("hx-alarm"), 0),
        };
        let mut out = vec![when];
        let action = get("ACTION").map(|(_, v)| v.trim()).unwrap_or("DISPLAY");
        out.push(self.tr(&format!("hx-v-action-{}", action.to_ascii_lowercase()), action));
        let to: Vec<String> = all("ATTENDEE").iter().map(|(p, v)| self.person("ORGANIZER", p, v)).collect();
        if !to.is_empty() {
            out.push(self.s("hx-al-to").replace("%who", &to.join(", ")));
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
            out.push(self.s("hx-al-repeat").replace("%n", r.trim()).replace("%every", &every));
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
            return self.s("hx-a-coll");
        }
        if let Some(r) = key.strip_prefix("RECURRENCE-ID:") {
            return self.s("hx-a-override").replace("%date", &self.when("", r));
        }
        self.tr(&format!("hx-a-{}", key.to_ascii_lowercase()), key)
    }
}
