//! The query: dates ("3. märz 2027", "15.09.") become a jump target, length bounds, the kind prefix ("termin: zahnarzt").

use super::*;

const MONTHS_DE: [&str; 12] = ["januar", "februar", "märz", "april", "mai", "juni", "juli", "august", "september", "oktober", "november", "dezember"];
const MONTHS_EN: [&str; 12] = ["january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november", "december"];
const MONTHS_FR: [&str; 12] = ["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre", "décembre"];
const MONTHS_ES: [&str; 12] = ["enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto", "septiembre", "octubre", "noviembre", "diciembre"];

/// (year, month, day) as far as the query names them: `2027`, `märz`, `3. märz 2027`, `15.09.`,
/// `15.9.2026`, `2026-09-15`, `sep 2027`; None when it is not a date.
pub fn date_parts(q: &str) -> Option<(Option<i32>, Option<u32>, Option<u32>)> {
    let t = q.trim().to_lowercase();
    if t.is_empty() {
        return None;
    }
    if let Ok(d) = NaiveDate::parse_from_str(&t, "%Y-%m-%d") {
        return Some((Some(d.year()), Some(d.month()), Some(d.day())));
    }
    let mut year: Option<i32> = None;
    let mut month: Option<u32> = None;
    let mut day: Option<u32> = None;
    // dd.mm.[yyyy]
    let parts: Vec<&str> = t.split('.').map(str::trim).collect();
    if parts.len() >= 2 && parts.iter().take(2).all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())) {
        day = parts[0].parse().ok();
        month = parts[1].parse().ok();
        if let Some(y) = parts.get(2).filter(|p| !p.is_empty()) {
            year = y.parse::<i32>().ok().map(|y| if y < 100 { 2000 + y } else { y });
        }
    } else {
        for tok in t.split(|c: char| c == ' ' || c == '.').filter(|s| !s.is_empty()) {
            if let Ok(n) = tok.parse::<u32>() {
                if (1900..=2200).contains(&n) {
                    year = Some(n as i32);
                } else if (1..=31).contains(&n) && day.is_none() {
                    day = Some(n);
                }
            } else if let Some(i) = MONTHS_DE.iter().chain(MONTHS_EN.iter()).chain(MONTHS_FR.iter()).chain(MONTHS_ES.iter()).position(|m| m.starts_with(tok) && tok.len() >= 3) {
                month = Some((i % 12) as u32 + 1);
            } else {
                return None;
            }
        }
    }
    if year.is_none() && month.is_none() && day.is_none() {
        return None;
    }
    Some((year, month, day))
}

pub fn parse_date_query(q: &str, today: NaiveDate) -> Option<JumpTo> {
    let (year, month, day) = date_parts(q)?;
    let (y, m, d) = (year.unwrap_or(today.year()), month.unwrap_or(if day.is_some() { today.month() } else { 1 }), day.unwrap_or(1));
    let date = NaiveDate::from_ymd_opt(y, m, d).filter(|d| crate::domain::ical::year_ok(*d))?;
    let view = if day.is_some() { "day" } else if month.is_some() { "month" } else { "planner" };
    let label = match (day, month, year) {
        (Some(_), _, _) => date.format("%d.%m.%Y").to_string(),
        (None, Some(_), _) => format!("{} {}", MONTHS_DE[(m - 1) as usize].to_uppercase(), y),
        _ => y.to_string(),
    };
    Some(JumpTo { date, label: capitalize(&label), view })
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase(),
        None => String::new(),
    }
}

pub(super) struct Query {
    pub(super) phrase: String,
    pub(super) words: Vec<String>,
    pub(super) chars: Vec<Vec<char>>,
    /// how loose: 0 exact kinds only; 1 + typos/letters left out in the fields that carry them;
    /// 2 + those in every field, and half the words may be missing (see `search`)
    pub(super) tier: u8,
}

/// A query's length bound (characters, then words): every hit test runs per query word over the whole corpus,
/// so an unbounded `q` (20 000 characters in one GET) cost CPU in proportion. Nobody types more.
const QUERY_MAX_CHARS: usize = 200;
const QUERY_MAX_WORDS: usize = 12;

impl Query {
    pub(super) fn new(raw: &str) -> Self {
        let raw = raw.trim();
        let raw = raw.char_indices().nth(QUERY_MAX_CHARS).map_or(raw, |(at, _)| &raw[..at]);
        let phrase = fold(raw);
        let mut words = words(&phrase);
        words.truncate(QUERY_MAX_WORDS);
        let chars = words.iter().map(|w| w.chars().collect()).collect();
        Query { phrase, words, chars, tier: 1 }
    }
}

/// `termin: zahnarzt` → ("event", "zahnarzt"); prefixes in every UI language plus English.
pub fn split_scope(q: &str) -> (Option<&'static str>, &str) {
    let t = q.trim_start();
    let Some(i) = t.find(':') else { return (None, q) };
    let scope = match fold(t[..i].trim()).as_str() {
        "termin" | "termine" | "event" | "events" | "evenement" | "evenements" | "evento" | "eventos" => "event",
        "kontakt" | "kontakte" | "contact" | "contacts" | "contacto" | "contactos" | "person" | "personen" => "contact",
        "aufgabe" | "aufgaben" | "task" | "tasks" | "todo" | "tache" | "taches" | "tarea" | "tareas" => "task",
        _ => return (None, q),
    };
    (Some(scope), &t[i + 1..])
}
