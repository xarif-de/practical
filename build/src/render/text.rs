//! Icons and the sentences for a recurrence rule and a reminder.

use super::*;

pub(super) const BYDAY: [&str; 7] = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"];

pub fn ico(name: &str) -> String {
    format!("<svg class=\"ico\" aria-hidden=\"true\"><use href=\"{}#i-{name}\"/></svg>", crate::routes::assets::icons())
}

/// Same icon, painted in a colour of its own — the calendar's, in the day document.
pub(super) fn ico_c(name: &str, color: &str) -> String {
    format!("<svg class=\"ico\" aria-hidden=\"true\" style=\"color:{}\"><use href=\"{}#i-{name}\"/></svg>", e(color), crate::routes::assets::icons())
}

/// An RRULE in words. Parts the sentence cannot carry (BYSETPOS, several ordinals, …) are left out
/// rather than guessed — the base sentence stays true, the editor shows the whole rule.
pub fn rrule_text(rule: &str, start: NaiveDate, l: &L) -> String {
    let rule = crate::domain::ical::rrule_canon(rule);
    let mut m = std::collections::HashMap::new();
    for part in rule.split(';') {
        if let Some((k, v)) = part.split_once('=') {
            m.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    let get = |k: &str| m.get(k).map(|s| s.as_str()).unwrap_or("");
    let n: i64 = get("INTERVAL").parse().unwrap_or(1);
    let freq = get("FREQ");
    let mut out = match (freq, n) {
        ("DAILY", 1) => l.t("v-daily").to_string(),
        ("WEEKLY", 1) => l.t("v-weekly").to_string(),
        ("MONTHLY", 1) => l.t("v-monthly").to_string(),
        ("YEARLY", 1) => l.t("v-yearly").to_string(),
        ("DAILY", n) => l.ta("v-rr-every-days", &[("n", n.into())]),
        ("WEEKLY", n) => l.ta("v-rr-every-weeks", &[("n", n.into())]),
        ("MONTHLY", n) => l.ta("v-rr-every-months", &[("n", n.into())]),
        ("YEARLY", n) => l.ta("v-rr-every-years", &[("n", n.into())]),
        _ => return l.t("v-recurring").into(),
    };
    let byday = get("BYDAY");
    let names: Vec<String> = byday
        .split(',')
        .filter(|x| !x.is_empty())
        .filter_map(|x| {
            let code = x.trim_start_matches(|c: char| c.is_ascii_digit() || c == '-' || c == '+');
            BYDAY.iter().position(|b| *b == code).map(|p| l.dowl_i(p))
        })
        .collect();
    let ord = byday.split(',').next().unwrap_or("").trim_end_matches(char::is_alphabetic).parse::<i32>().ok();
    if freq == "WEEKLY" && !names.is_empty() {
        let joined = names.join(", ");
        let _ = write!(out, " {}", l.ta("v-rr-on-days", &[("days", joined.into())]));
    } else if matches!(freq, "MONTHLY" | "YEARLY") {
        if let (Some(o), Some(day)) = (ord, names.first()) {
            let which = if o == -1 { l.t("v-last") } else { l.ta("v-rr-ordinal", &[("n", i64::from(o).into())]) };
            let _ = write!(out, " {}", l.ta("v-rr-on-nth", &[("which", which.into()), ("day", day.clone().into())]));
        } else if let Ok(d) = get("BYMONTHDAY").parse::<u32>() {
            let _ = write!(out, " {}", l.ta("v-rr-on-monthday", &[("d", i64::from(d).into())]));
        } else if freq == "YEARLY" {
            let _ = write!(out, " {}", l.ta("v-rr-on-date", &[("d", i64::from(start.day()).into()), ("month", l.mon(start.month()).into())]));
        }
        if freq == "YEARLY" && ord.is_some() {
            if let Ok(mo) = get("BYMONTH").parse::<u32>() {
                let _ = write!(out, " {}", l.ta("v-rr-in-month", &[("month", l.mon(mo).into())]));
            }
        }
    }
    if let Ok(c) = get("COUNT").parse::<u32>() {
        let _ = write!(out, ", {}", l.tn("v-rr-count", c));
    } else if get("UNTIL").len() >= 8 {
        let u = get("UNTIL");
        if let Some(d) = u.get(..8).and_then(|u| NaiveDate::parse_from_str(u, "%Y%m%d").ok()) {
            let _ = write!(out, ", {}", l.ta("v-rr-until", &[("date", l.date(d).into())]));
        }
    }
    out
}

pub fn alarm_label(m: i64, l: &L) -> String {
    if m == 0 {
        return l.t("v-at-start").to_string();
    }
    // the offset counts backwards from the start, so a negative one fires after it
    let (n, after) = (m.abs(), m < 0);
    let amount = if n % 1440 == 0 {
        l.ta("v-n-days", &[("n", (n / 1440).into())])
    } else if n % 60 == 0 {
        l.ta("v-n-hours", &[("n", (n / 60).into())])
    } else {
        l.ta("v-n-minutes", &[("n", n.into())])
    };
    format!("{amount} {}", if after { l.t("v-after") } else { l.t("v-before") })
}
