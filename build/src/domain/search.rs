//! Universal search (top bar): events, contacts and tasks ranked by relevance, and a "jump to"
//! suggestion when the query looks like a date.
//!
//! Matching (`score_doc`): the query is folded (lower case, diacritics off) and split into words;
//! EVERY word must match some field. A word scores by how it matches — a whole word 1.0, a word
//! prefix .85, run together over word boundaries from a word's start .75 (4+ letters: "dietmarstein" ~
//! "Dietmar Steinbach"), starts of consecutive words .65 (5+ letters, each part 2+: "dietstein"),
//! inside a word .6 (3+ letters), a typo (edit distance 1 from 5 letters on, 2 from 8,
//! against a word's start; a 4-letter word would match too much: "demo" ~ "devops") .5/.38 — times the field's weight; the document's text score is the
//! mean over the words plus a bonus when the whole query starts or equals the main field.
//! Tiers (`search`): exact kinds first; while no clear hit (≥ .85) and fewer than 3 above the floor,
//! loosen — typos/letters left out in the fields that carry them, then everywhere with up to half the
//! words missing (dropped again when some document has every word). The floor (`cutoff`) is relative to
//! the best: clear best hits drop what is far below them, a vague best keeps nearly everything.
//! Ranking adds per kind: events the nearness of the next (else the last) occurrence, upcoming
//! weighing more than past; tasks being open and a near due date.
//!
//! Cost: the searchable corpus of a user (±2 years of occurrences, one document per series; all
//! contacts; all tasks — pre-folded) is built once and kept in `IndexCache`, keyed by a fingerprint
//! of the user's collections' synctokens (one cheap query): any change, also over CalDAV, rebuilds
//! it; nothing else does. A search then only scores in memory.

use crate::domain::collections;
use crate::domain::contacts;
use crate::domain::events::{self, Instance};
use crate::domain::tasks;
use crate::error::AppResult;
use crate::state::AppState;
use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime};
use rustical_store::auth::Principal;
use sqlx::Row;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Debug, serde::Serialize)]
pub struct SearchResult {
    pub jump: Option<JumpTo>,
    /// the category a prefix narrowed the search to ("event" | "contact" | "task")
    pub scope: Option<&'static str>,
    pub events: Vec<EventHit>,
    pub contacts: Vec<ContactHit>,
    /// open tasks first; also used by the event editor's "link a task" search
    pub tasks: Vec<TaskHit>,
    /// matches per category before the limit
    pub totals: Totals,
}

#[derive(Debug, Default, serde::Serialize)]
pub struct Totals {
    pub events: usize,
    pub contacts: usize,
    pub tasks: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TaskHit {
    pub key: String,
    pub uid: String,
    pub summary: String,
    pub due: Option<NaiveDate>,
    pub completed: bool,
    pub cal_name: String,
    pub color: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub why: Vec<Why>,
}

/// Why a hit was found: a field (code, the client names it), its text around the match, whether the
/// row shows that field anyway, and how each query word matched in it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Why {
    pub f: &'static str,
    pub s: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub shown: bool,
    pub hits: Vec<WhyHit>,
}

/// One query word's match: the word, the kind (`Kind::code`) and, for a typo, the word it resembles.
#[derive(Debug, Clone, serde::Serialize)]
pub struct WhyHit {
    pub w: String,
    pub k: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub m: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct JumpTo {
    pub date: NaiveDate,
    pub label: String,
    pub view: &'static str,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct EventHit {
    pub key: String,
    pub uid: String,
    pub summary: String,
    /// the occurrence shown: the next one from today, else the last
    pub date: NaiveDate,
    pub time: String,
    pub color: String,
    pub cal_name: String,
    /// occurrences in the searched window (±2 years); > 1 for a series
    pub count: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub why: Vec<Why>,
}

pub use crate::domain::contacts::ContactHit;

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
    let date = NaiveDate::from_ymd_opt(y, m, d)?;
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

// ---- matching ----

/// Lower case without diacritics (ä→a, ß→ss …): "Müller" and "muller" are the same word.
pub fn fold(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars().flat_map(char::to_lowercase) {
        match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' => o.push('a'),
            'æ' => o.push_str("ae"),
            'ç' | 'č' | 'ć' => o.push('c'),
            'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ě' => o.push('e'),
            'ì' | 'í' | 'î' | 'ï' => o.push('i'),
            'ñ' | 'ń' | 'ň' => o.push('n'),
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' => o.push('o'),
            'œ' => o.push_str("oe"),
            'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ů' => o.push('u'),
            'ý' | 'ÿ' => o.push('y'),
            'ß' => o.push_str("ss"),
            'š' | 'ś' => o.push('s'),
            'ž' | 'ź' | 'ż' => o.push('z'),
            'ř' => o.push('r'),
            'ł' => o.push('l'),
            _ => o.push(c),
        }
    }
    o
}

fn words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_string).collect()
}

/// One searchable text: folded, with its words; `code` names it for "why found", `shown` = the hit
/// displays it already (no reason needed).
#[derive(Debug, Default)]
struct Field {
    code: &'static str,
    shown: bool,
    /// narrows only: a query matching nothing but such fields (a calendar's name) finds nothing
    narrow: bool,
    /// what "why found" quotes (cut around the match, `crop`)
    raw: String,
    text: String,
    words: Vec<String>,
    /// the words run together, and where each starts in it: a query typed without spaces
    compact: String,
    starts: Vec<usize>,
    weight: f32,
}

impl Field {
    fn new(code: &'static str, raw: &str, weight: f32) -> Self {
        Self::matching(code, raw, raw, weight)
    }
    /// matched against `text`, quoted as `raw` (status words: every language matches, the user's shows)
    fn matching(code: &'static str, raw: &str, text: &str, weight: f32) -> Self {
        let text = fold(text);
        let words = words(&text);
        let (mut compact, mut starts) = (String::new(), vec![]);
        for w in &words {
            starts.push(compact.len());
            compact.push_str(w);
        }
        Field { code, shown: false, narrow: false, raw: raw.trim().to_string(), text, words, compact, starts, weight }
    }
    fn shown(mut self) -> Self {
        self.shown = true;
        self
    }
    /// the collection's name: weighs least and only narrows what the other words find
    fn collection(raw: &str) -> Self {
        Field { narrow: true, ..Self::new("coll", raw, 0.15) }
    }
}

/// A date of a document that a date query (a day and a month, the year optional) matches.
#[derive(Debug)]
struct DateField {
    code: &'static str,
    /// None: a date without a year (vCard `--MM-DD`), which every year of the query matches
    y: Option<i32>,
    m: u32,
    d: u32,
    weight: f32,
    raw: String,
}

impl DateField {
    fn of(code: &'static str, d: NaiveDate, weight: f32) -> Self {
        DateField { code, y: Some(d.year()), m: d.month(), d: d.day(), weight, raw: d.format("%d.%m.%Y").to_string() }
    }
    /// vCard dates: `1987-11-17`, `19871117`, `--11-17`, `--1117` (a time part is ignored)
    fn vcard(code: &'static str, v: &str, weight: f32) -> Option<Self> {
        let v = v.trim().split('T').next()?.replace('-', "");
        let (y, md): (Option<i32>, &str) = match v.len() {
            4 => (None, &v),
            8 => (Some(v[..4].parse().ok()?), &v[4..]),
            _ => return None,
        };
        let (m, d): (u32, u32) = (md.get(..2)?.parse().ok()?, md.get(2..4)?.parse().ok()?);
        NaiveDate::from_ymd_opt(y.unwrap_or(2000), m, d)?;
        let raw = match y { Some(y) => format!("{d:02}.{m:02}.{y}"), None => format!("{d:02}.{m:02}.") };
        Some(DateField { code, y, m, d, weight, raw })
    }
    fn hits(&self, (y, m, d): (Option<i32>, u32, u32)) -> bool {
        self.m == m && self.d == d && (y.is_none() || self.y.is_none() || self.y == y)
    }
}

/// Everything a document is searched by.
#[derive(Debug, Default)]
struct Doc {
    fields: Vec<Field>,
    dates: Vec<DateField>,
    /// phone numbers: their digits, and the numbers as written (for "why found")
    digits: Vec<String>,
    phones: String,
}

/// Optimal string alignment distance (a swap of two letters is one edit), on chars.
fn osa(a: &[char], b: &[char]) -> usize {
    let (n, m) = (a.len(), b.len());
    let mut d = vec![vec![0usize; m + 1]; n + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for j in 0..=m {
        d[0][j] = j;
    }
    for i in 1..=n {
        for j in 1..=m {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut v = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                v = v.min(d[i - 2][j - 2] + 1);
            }
            d[i][j] = v;
        }
    }
    d[n][m]
}

/// `q` = a start (2+ letters) of `ws[0]`, then of `ws[1]` … (`first`: at least two words must take part;
/// the last part 3+ letters: "urlab" is a typo of "urlaub", not "URLaubs-ABrechnung").
fn starts_chain(q: &str, ws: &[String], first: bool) -> bool {
    let Some(w) = ws.first() else { return false };
    let common = q.chars().zip(w.chars()).take_while(|(a, b)| a == b).map(|(a, _)| a.len_utf8()).sum::<usize>();
    // the part taken from this word: 2+ letters, as many as match; the rest must start the next word
    (2..=common).rev().filter(|&l| q.is_char_boundary(l)).any(|l| if l == q.len() { !first && l >= 3 } else { starts_chain(&q[l..], &ws[1..], false) })
}

/// How a query word matched a field, best first.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Word,
    Prefix,
    Joined,
    Chain,
    Inside,
    Typo,
    Gaps,
    Phone,
    Date,
}

impl Kind {
    fn code(self) -> &'static str {
        match self {
            Kind::Word => "word",
            Kind::Prefix => "prefix",
            Kind::Joined => "joined",
            Kind::Chain => "chain",
            Kind::Inside => "inside",
            Kind::Typo => "typo",
            Kind::Gaps => "gaps",
            Kind::Phone => "phone",
            Kind::Date => "date",
        }
    }
}

/// How well one query word matches a field (0 = not at all), how, and for a typo the field's word.
fn word_score(q: &str, qc: &[char], f: &Field, tier: u8) -> (f32, Kind, Option<String>) {
    if f.words.iter().any(|w| w == q) {
        return (1.0, Kind::Word, None);
    }
    if f.words.iter().any(|w| w.starts_with(q)) {
        return (0.85, Kind::Prefix, None);
    }
    let n = qc.len();
    // words typed without the space between them: from a word's start on, past its end
    // (2+ letters of the next word: "christina" is not "Christin A…")
    if n >= 4 && f.starts.iter().zip(&f.words).any(|(&at, w)| q.len() >= w.len() + 2 && f.compact[at..].starts_with(q)) {
        return (0.75, Kind::Joined, None);
    }
    // starts of consecutive words run together ("dietstein" ~ "Dietmar Steinbach")
    if n >= 5 && f.words.len() > 1 && (0..f.words.len() - 1).any(|k| starts_chain(q, &f.words[k..], true)) {
        return (0.65, Kind::Chain, None);
    }
    if n >= 3 && f.text.contains(q) {
        return (0.6, Kind::Inside, None);
    }
    // the fuzzy kinds only where a loose match still says something: long, unimportant texts (a
    // note, a partner, a link) contain almost any letters — there only exact kinds count
    if tier >= 1 && n >= 5 && (tier >= 2 || f.weight >= TYPO_MIN_WEIGHT) {
        // a typo: against the word's start of about the query's length ("kalnder" ~ "kalender")
        let max = if n < 8 { 1 } else { 2 };
        let (mut best, mut like) = (usize::MAX, None);
        for w in &f.words {
            // a typo keeps the first letter ("zugesagt" is no typo of "abgesagt") — and that skips most words cheaply
            if !w.starts_with(qc[0]) || w.len() + 1 < q.len() {
                continue;
            }
            let wc: Vec<char> = w.chars().collect();
            if wc.len() + 1 < n {
                continue;
            }
            for len in [n.saturating_sub(1), n, n + 1] {
                if len == 0 || len > wc.len() {
                    continue;
                }
                let d = osa(qc, &wc[..len]);
                if d < best {
                    (best, like) = (d, Some(w));
                }
            }
            if best == 0 {
                break;
            }
        }
        if best <= max {
            return (if best <= 1 { 0.5 } else { 0.38 }, Kind::Typo, like.cloned());
        }
    }
    // letters left out ("chrstinbach" ~ "Christin Steinbach"): the query's letters in order from a
    // word's start, over at most twice its length; the denser, the better
    if tier >= 1 && n >= 6 && (tier >= 2 || f.weight >= GAPS_MIN_WEIGHT) {
        if let Some(span) = gaps_span(qc, f) {
            return (0.2 + 0.16 * n as f32 / span as f32, Kind::Gaps, None); // .28–.36: below a two-edit typo
        }
    }
    (0.0, Kind::Word, None)
}

/// fields where typos count (place, categories, people, e-mail … — not notes, relations, links, status)
const TYPO_MIN_WEIGHT: f32 = 0.5;
/// fields where letters may be left out (titles, names, organisation)
const GAPS_MIN_WEIGHT: f32 = 0.8;

/// The shortest stretch of `f`'s words run together, from a word's start, holding `q`'s letters in
/// order — if it is at most twice as long as `q`.
fn gaps_span(q: &[char], f: &Field) -> Option<usize> {
    let c: Vec<char> = f.compact.chars().collect();
    let mut best: Option<usize> = None;
    let mut starts = f.starts.iter().map(|&b| f.compact[..b].chars().count());
    for s in starts.by_ref() {
        if c.get(s) != q.first() {
            continue;
        }
        let (mut i, mut end) = (s, None);
        for (k, ch) in q.iter().enumerate() {
            while i < c.len() && c[i] != *ch {
                i += 1;
            }
            if i == c.len() {
                break;
            }
            if k + 1 == q.len() {
                end = Some(i);
            }
            i += 1;
        }
        if let Some(e) = end {
            let span = e - s + 1;
            if span <= 2 * q.len() && best.is_none_or(|b| span < b) {
                best = Some(span);
            }
        }
    }
    best
}

/// All of `q`'s letters and digits in `text` in order, gaps allowed ("chrstinb" in "Christin Steinbach")
fn in_order(q: &str, text: &str) -> bool {
    let mut t = text.chars();
    q.chars().filter(|c| c.is_alphanumeric()).all(|c| t.any(|x| x == c))
}

/// A document's match: its score and, per query word, where and how it matched.
#[derive(Debug, Default)]
struct Match {
    score: f32,
    hits: Vec<Hit>,
    /// some query words matched nothing (the loosest tier)
    partial: bool,
}

#[derive(Debug)]
struct Hit {
    at: Place,
    kind: Kind,
    /// index into `Query::words`; `usize::MAX` = the whole query (a date)
    word: usize,
    typo: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Place {
    Field(usize),
    Date(usize),
    Phone,
}

/// The text match of a document (score 0 = none): every query word must match some field; a field
/// that only narrows (a collection's name) cannot carry a match alone. Ties go to the document whose
/// main text holds all the query's letters in order.
fn score_doc(q: &Query, doc: &Doc) -> Match {
    if q.words.is_empty() {
        return Match::default();
    }
    let (mut sum, mut real, mut hits, mut missing) = (0.0, false, Vec::with_capacity(q.words.len()), 0usize);
    for (wi, (w, wc)) in q.words.iter().zip(&q.chars).enumerate() {
        let mut best = (0.0f32, Place::Phone, Kind::Word, None);
        for (i, f) in doc.fields.iter().enumerate() {
            let (s, k, m) = word_score(w, wc, f, q.tier);
            let s = f.weight * s;
            if s > best.0 {
                best = (s, Place::Field(i), k, m);
            }
        }
        // a phone number: the query's digits inside the number's digits (a number is an identifier)
        if best.0 < 0.85 && w.len() >= 3 && w.chars().all(|c| c.is_ascii_digit()) && doc.digits.iter().any(|d| d.contains(w.as_str())) {
            best = (0.85, Place::Phone, Kind::Phone, None);
        }
        if best.0 == 0.0 {
            // the loosest tier lets up to half of several words go missing
            if q.tier >= 2 && q.words.len() >= 2 {
                missing += 1;
                continue;
            }
            return Match::default();
        }
        real |= match best.1 { Place::Field(i) => !doc.fields[i].narrow, _ => true };
        sum += best.0;
        hits.push(Hit { at: best.1, kind: best.2, word: wi, typo: best.3 });
    }
    if !real {
        return Match::default(); // only the calendar's / address book's name: that would be all of it
    }
    let n = q.words.len();
    if missing * 2 > n {
        return Match::default();
    }
    // a partial match counts its words' mean, scaled by the share found and a penalty
    let mut s = if missing == 0 { sum / n as f32 } else { 0.7 * (sum / (n - missing) as f32) * ((n - missing) as f32 / n as f32) };
    if let Some(main) = doc.fields.first() {
        s += phrase_bonus(q, main);
        if in_order(&q.phrase, &main.text) {
            s += 0.03;
        }
    }
    Match { score: s, hits, partial: missing > 0 }
}

/// The whole query equal to (.25) or the start of (.15) the main field.
fn phrase_bonus(q: &Query, main: &Field) -> f32 {
    if main.text == q.phrase { 0.25 } else if main.text.starts_with(&q.phrase) { 0.15 } else { 0.0 }
}

/// `score_doc` over one field (a title): every word must match it.
fn field_score(q: &Query, f: &Field) -> f32 {
    let mut sum = 0.0;
    for (w, wc) in q.words.iter().zip(&q.chars) {
        let (s, _, _) = word_score(w, wc, f, q.tier);
        if s == 0.0 {
            return 0.0;
        }
        sum += s;
    }
    if q.words.is_empty() { 0.0 } else { f.weight * sum / q.words.len() as f32 + phrase_bonus(q, f) }
}

/// The date match of a document for a date query (score 0 = none): the best-weighted date on that day.
fn score_dates(pat: (Option<i32>, u32, u32), doc: &Doc) -> Match {
    let mut m = Match::default();
    for (i, d) in doc.dates.iter().enumerate().filter(|(_, d)| d.hits(pat)) {
        // a birthday with the queried year is that person's, one without is only the same day
        let s = d.weight * if pat.0.is_some() && d.y == pat.0 { 1.0 } else { 0.9 };
        m.score = m.score.max(s);
        if !m.hits.iter().any(|h| matches!(h.at, Place::Date(j) if doc.dates[j].code == d.code)) {
            m.hits.push(Hit { at: Place::Date(i), kind: Kind::Date, word: usize::MAX, typo: None });
        }
    }
    m
}

/// About 120 characters of `raw` around the first place one of `words` matches it (all of it when short).
fn crop(raw: &str, words: &[&str]) -> String {
    const MAX: usize = 120;
    let chars: Vec<char> = raw.chars().collect();
    if chars.len() <= MAX {
        return raw.to_string();
    }
    // fold char by char, remembering where each folded char came from
    let (mut folded, mut from) = (String::new(), vec![]);
    for (i, c) in chars.iter().enumerate() {
        for x in fold(&c.to_string()).chars() {
            folded.push(x);
            from.push(i);
        }
    }
    let at = words.iter().filter_map(|w| folded.find(w)).min().map(|b| from[folded[..b].chars().count()]).unwrap_or(0);
    let a = at.saturating_sub(40).min(chars.len() - MAX);
    let b = a + MAX;
    format!("{}{}{}", if a > 0 { "…" } else { "" }, chars[a..b].iter().collect::<String>(), if b < chars.len() { "…" } else { "" })
}

/// Why a hit was found: its matches grouped by field, in the order of the query's words.
fn reasons(doc: &Doc, m: &Match, q: &Query) -> Vec<Why> {
    let mut out: Vec<(Place, Why)> = vec![];
    for h in &m.hits {
        let w = if h.word == usize::MAX { q.phrase.clone() } else { q.words[h.word].clone() };
        let hit = WhyHit { w, k: h.kind.code(), m: h.typo.clone() };
        if let Some((_, why)) = out.iter_mut().find(|(p, _)| *p == h.at) {
            why.hits.push(hit);
            continue;
        }
        let why = match h.at {
            Place::Field(i) => {
                let f = &doc.fields[i];
                let mut words: Vec<&str> = q.words.iter().map(String::as_str).collect();
                words.extend(m.hits.iter().filter_map(|x| x.typo.as_deref()));
                Why { f: f.code, s: crop(&f.raw, &words), shown: f.shown, hits: vec![hit] }
            }
            Place::Date(i) => Why { f: doc.dates[i].code, s: doc.dates[i].raw.clone(), shown: false, hits: vec![hit] },
            Place::Phone => Why { f: "phone", s: doc.phones.clone(), shown: false, hits: vec![hit] },
        };
        out.push((h.at, why));
    }
    out.into_iter().map(|(_, w)| w).collect()
}

struct Query {
    phrase: String,
    words: Vec<String>,
    chars: Vec<Vec<char>>,
    /// how loose: 0 exact kinds only; 1 + typos/letters left out in the fields that carry them;
    /// 2 + those in every field, and half the words may be missing (see `search`)
    tier: u8,
}

impl Query {
    fn new(raw: &str) -> Self {
        let phrase = fold(raw.trim());
        let words = words(&phrase);
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

// ---- the per-user corpus ----

struct EvDoc {
    key: String,
    uid: String,
    summary: String,
    color: String,
    cal_name: String,
    cancelled: bool,
    /// (start, time label), sorted
    occ: Vec<(NaiveDateTime, String)>,
    doc: Doc,
}

struct CtDoc {
    hit: ContactHit,
    doc: Doc,
}

struct TkDoc {
    hit: TaskHit,
    open: bool,
    doc: Doc,
}

pub struct Index {
    stamp: u64,
    built: Instant,
    events: Vec<EvDoc>,
    contacts: Vec<CtDoc>,
    tasks: Vec<TkDoc>,
    /// every past occurrence, the title the only field (`suggest`): built on the first suggestion,
    /// dropped with the index
    past: tokio::sync::OnceCell<Vec<EvDoc>>,
}

/// One corpus per user, rebuilt when the fingerprint moves (see the module comment).
#[derive(Default)]
pub struct IndexCache(Mutex<HashMap<String, Arc<Index>>>);

/// The synctokens of the user's collections, the day, the zone and the language (status words are
/// quoted in it): a change of any rebuilds the corpus.
async fn fingerprint(state: &AppState, cals: &[crate::domain::collections::CalInfo], abs: &[crate::domain::collections::AbInfo], today: NaiveDate, tz: chrono_tz::Tz, lang: &str) -> AppResult<u64> {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (today, tz.name(), lang).hash(&mut h);
    let want: std::collections::HashSet<(String, String)> = cals.iter().map(|c| (c.principal.clone(), c.id.clone())).collect();
    for r in sqlx::query("SELECT principal, id, synctoken FROM calendars ORDER BY principal, id").fetch_all(&state.db).await? {
        let k: (String, String) = (r.get("principal"), r.get("id"));
        if want.contains(&k) {
            (k, r.get::<i64, _>("synctoken")).hash(&mut h);
        }
    }
    let want: std::collections::HashSet<(String, String)> = abs.iter().map(|a| (a.principal.clone(), a.id.clone())).collect();
    for r in sqlx::query("SELECT principal, id, synctoken FROM addressbooks ORDER BY principal, id").fetch_all(&state.db).await? {
        let k: (String, String) = (r.get("principal"), r.get("id"));
        if want.contains(&k) {
            ("ab", k, r.get::<i64, _>("synctoken")).hash(&mut h);
        }
    }
    Ok(h.finish())
}

/// A status-like word: quoted in the user's language, matched in every UI language.
fn word_field(state: &AppState, lang: &str, keys: &[String], weight: f32) -> Field {
    let raw = keys.iter().map(|k| state.i18n.t(lang, k)).collect::<Vec<_>>().join(" · ");
    let all = keys.iter().flat_map(|k| crate::i18n::LANGS.iter().map(move |l| (l, k))).map(|(l, k)| state.i18n.t(l, k)).collect::<Vec<_>>().join(" ");
    Field::matching("status", &raw, &all, weight)
}

fn prio_key(p: i64) -> Option<&'static str> {
    match p {
        1..=4 => Some("ed-prio-high"),
        5 => Some("ed-prio-medium"),
        6..=9 => Some("ed-prio-low"),
        _ => None,
    }
}

fn geo_text(g: Option<(f64, f64)>) -> String {
    g.map(|(a, b)| format!("{a:.5}, {b:.5}")).unwrap_or_default()
}

fn link_fields(links: &[events::Link]) -> [Field; 2] {
    let named: Vec<String> = links.iter().map(|l| format!("{} {}", l.label, l.filename).trim().to_string()).filter(|s| !s.is_empty()).collect();
    let uris: Vec<&str> = links.iter().map(|l| l.uri.as_str()).filter(|u| !u.is_empty()).collect();
    [Field::new("link", &named.join(" · "), 0.45), Field::new("link", &uris.join(" · "), 0.35)]
}

/* Field weights — how much a match there says about the document (the word's own match kind is
   multiplied in). What the hit is called weighs most; what describes it next; who and what it is
   connected to less; status words and coordinates least; a date query matches dates only.
   Events:   title 1 · place .75 · categories .7 · people (names .6, e-mails .5) · link names .45,
             addresses .35 · notes .4 · related events/tasks .3 · status/show as/visibility/priority .3
             · coordinates .25 · date query: an occurrence on that day 1.
   Contacts: display name 1 · given/family/nick .9 · organisation .8 · phone digits .85 · e-mail .7 ·
             job title .65 · name affixes (Dr., middle name) .6 · categories .6 · messenger .55 ·
             street/city .5, postcode/region/country .4 · web .45 · notes .35 · related persons .3 (a
             partner's name says little about the contact) · dates: birthday .95, anniversary .8, others .7.
   All:      the calendar's / address book's name .15, and only to narrow: a query matching nothing
             else finds nothing ("foobar zahn" narrows, "foobar" alone would be the whole calendar).
   Tasks:    title 1 · categories .7 · place .6 · link names .45, addresses .35 · notes .4 · related .3
             · status/priority/progress/visibility .3 · coordinates .25 · dates: due .95, start .8, done .7. */
async fn index(state: &AppState, user: &Principal, today: NaiveDate, tz: chrono_tz::Tz, lang: &str) -> AppResult<Arc<Index>> {
    let cals = events::visible_calendars(state, user, None).await?;
    let abs = collections::addressbooks_for(state, user, false).await?;
    let stamp = fingerprint(state, &cals, &abs, today, tz, lang).await?;
    if let Some(ix) = state.search_index.0.lock().unwrap().get(&user.id).filter(|ix| ix.stamp == stamp) {
        return Ok(ix.clone());
    }
    // ±2 years around today: expanding 4 000 objects takes ~40 ms on the real store
    let (from, to) = (today - Duration::days(730), today + Duration::days(730));
    let inst: Vec<Instance> = events::instances(state, &cals, from, to, tz).await?;
    let mut extras = events::search_extras(state, &cals, from, to).await?;
    let tasks = tasks::tasks_for(state, &cals, tz).await?;
    // RELATED-TO names the other side by UID: its title is what can be searched
    let mut titles: HashMap<String, String> = inst.iter().map(|i| (i.uid.clone(), i.summary.clone())).collect();
    titles.extend(tasks.iter().map(|t| (t.uid.clone(), t.summary.clone())));
    let related = |uids: &[String]| uids.iter().filter_map(|u| titles.get(u).cloned()).collect::<Vec<_>>().join(" · ");

    let mut by_key: HashMap<String, usize> = HashMap::new();
    let mut evs: Vec<EvDoc> = vec![];
    for i in &inst {
        let k = i.key();
        let at = match by_key.get(&k) {
            Some(&at) => at,
            None => {
                let x = extras.remove(&k).unwrap_or_default();
                let names: Vec<&str> = x.people.iter().map(|p| p.name.as_str()).filter(|n| !n.is_empty()).collect();
                let mails: Vec<&str> = x.people.iter().map(|p| p.email.as_str()).filter(|n| !n.is_empty()).collect();
                let mut keys: Vec<String> = vec![];
                match x.status.as_str() { "CONFIRMED" => keys.push("ed-st-confirmed".into()), "TENTATIVE" => keys.push("ed-st-tentative".into()), "CANCELLED" => keys.push("ed-st-cancelled".into()), _ => {} }
                match x.transp.as_str() { "OPAQUE" => keys.push("ed-transp-opaque".into()), "TRANSPARENT" => keys.push("ed-transp-transparent".into()), _ => {} }
                match x.class.as_str() { "PUBLIC" => keys.push("ed-class-public".into()), "PRIVATE" => keys.push("ed-class-private".into()), "CONFIDENTIAL" => keys.push("ed-class-confidential".into()), _ => {} }
                keys.extend(x.priority.and_then(prio_key).map(String::from));
                let [link_names, link_uris] = link_fields(&x.links);
                let fields = vec![
                    Field::new("title", &i.summary, 1.0).shown(),
                    Field::new("location", &i.location, 0.75),
                    Field::new("cats", &i.categories.join(", "), 0.7),
                    Field::new("person", &names.join(", "), 0.6),
                    Field::new("email", &mails.join(", "), 0.5),
                    link_names,
                    link_uris,
                    Field::new("note", &i.description.chars().take(2000).collect::<String>(), 0.4),
                    Field::new("related", &related(&x.related), 0.3),
                    word_field(state, lang, &keys, 0.3),
                    Field::new("geo", &geo_text(x.geo), 0.25),
                    Field::collection(&i.cal_name).shown(),
                ];
                evs.push(EvDoc { key: k.clone(), uid: i.uid.clone(), summary: i.summary.clone(), color: i.color.clone(), cal_name: i.cal_name.clone(), cancelled: i.cancelled, occ: vec![], doc: Doc { fields, ..Default::default() } });
                by_key.insert(k, evs.len() - 1);
                evs.len() - 1
            }
        };
        evs[at].occ.push((i.start, i.time_label()));
    }
    for e in &mut evs {
        e.doc.dates = e.occ.iter().map(|o| DateField::of("date", o.0.date(), 1.0)).collect();
    }

    let mut cts = vec![];
    for c in contacts::contacts_for(state, &abs).await? {
        let bday = !c.birthday.is_empty();
        let sub = if bday { c.birthday_label() } else if !c.org.is_empty() { c.org.clone() } else { c.category() };
        let join = |v: &[contacts::Typed]| v.iter().map(|t| t.value.as_str()).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", ");
        let places: Vec<String> = c.addresses.iter().map(|a| format!("{} {}", a.street, a.city).trim().to_string()).filter(|s| !s.is_empty()).collect();
        let regions: Vec<String> = c.addresses.iter().map(|a| format!("{} {} {}", a.code, a.region, a.country).split_whitespace().collect::<Vec<_>>().join(" ")).filter(|s| !s.is_empty()).collect();
        let fields = vec![
            Field::new("name", &c.full_name, 1.0).shown(),
            Field::new("name", &format!("{} {} {}", c.nickname, c.given, c.family), 0.9).shown(),
            Field::new("org", &format!("{} {}", c.org, c.department).trim().to_string(), 0.8),
            Field::new("email", &join(&c.emails), 0.7),
            Field::new("job", &c.title, 0.65),
            Field::new("affix", &format!("{} {} {}", c.prefix, c.additional, c.suffix).split_whitespace().collect::<Vec<_>>().join(" "), 0.6),
            Field::new("cats", &c.categories.join(", "), 0.6),
            Field::new("im", &join(&c.impp), 0.55),
            Field::new("address", &places.join(" · "), 0.5),
            Field::new("address", &regions.join(" · "), 0.4),
            Field::new("web", &join(&c.urls), 0.45),
            Field::new("note", &c.note.chars().take(1000).collect::<String>(), 0.35),
            Field::new("rel", &join(&c.related), 0.3),
            Field::collection(&c.ab_name),
        ];
        let mut dates: Vec<DateField> = [DateField::vcard("bday", &c.birthday, 0.95), DateField::vcard("anniv", &c.anniversary, 0.8)].into_iter().flatten().collect();
        dates.extend(c.dates.iter().filter_map(|d| DateField::vcard("date", &d.date, 0.7).map(|mut f| { if !d.label.is_empty() { f.raw = format!("{}: {}", d.label, f.raw); } f })));
        let digits = c.phones.iter().map(|p| p.value.chars().filter(char::is_ascii_digit).collect::<String>()).filter(|d| d.len() >= 3).collect();
        let doc = Doc { fields, dates, digits, phones: join(&c.phones) };
        cts.push(CtDoc { hit: ContactHit { bday, ..ContactHit::of(&c, sub) }, doc });
    }

    let mut tks = vec![];
    for t in tasks {
        let mut keys: Vec<String> = vec![];
        match t.status.as_str() { "NEEDS-ACTION" => keys.push("ed-tst-open".into()), "IN-PROCESS" => keys.push("ed-tst-doing".into()), "COMPLETED" => keys.push("ed-tst-done".into()), "CANCELLED" => keys.push("ed-tst-cancelled".into()), _ => {} }
        keys.extend(prio_key(t.priority as i64).map(String::from));
        match t.class.as_str() { "PRIVATE" => keys.push("ed-class-private".into()), "CONFIDENTIAL" => keys.push("ed-class-confidential".into()), _ => {} }
        let mut status = word_field(state, lang, &keys, 0.3);
        if let Some(p) = t.percent.filter(|p| *p > 0) {
            status = Field::matching("status", &format!("{}{}{p} %", status.raw, if status.raw.is_empty() { "" } else { " · " }), &format!("{} {p}", status.text), 0.3);
        }
        let [link_names, link_uris] = link_fields(&t.extra.links);
        let fields = vec![
            Field::new("title", &t.summary, 1.0).shown(),
            Field::new("cats", &t.categories.join(", "), 0.7),
            Field::new("location", &t.location, 0.6),
            link_names,
            link_uris,
            Field::new("note", &t.description.chars().take(2000).collect::<String>(), 0.4),
            Field::new("related", &related(&t.extra.related), 0.3),
            status,
            Field::new("geo", &geo_text(t.extra.geo), 0.25),
            Field::collection(&t.cal_name).shown(),
        ];
        let dates = [t.due.map(|d| DateField::of("due", d.date(), 0.95)), t.start.map(|d| DateField::of("start", d.date(), 0.8)), t.completed_at.map(|d| DateField::of("done", d.date(), 0.7))].into_iter().flatten().collect();
        let open = t.open();
        tks.push(TkDoc { hit: TaskHit { key: t.key(), uid: t.uid.clone(), summary: t.summary.clone(), due: t.due.map(|d| d.date()), completed: t.completed(), cal_name: t.cal_name.clone(), color: t.color.clone(), why: vec![] }, open, doc: Doc { fields, dates, ..Default::default() } });
    }
    let ix = Arc::new(Index { stamp, built: Instant::now(), events: evs, contacts: cts, tasks: tks, past: Default::default() });
    let mut m = state.search_index.0.lock().unwrap();
    m.retain(|_, v| v.built.elapsed().as_secs() < 1800); // idle users do not keep a corpus
    m.insert(user.id.clone(), ix.clone());
    Ok(ix)
}

/// Nearness of a day to today, 0..1: upcoming days weigh more and fade slower than past ones.
fn near(d: NaiveDate, today: NaiveDate, future_days: f32, past_days: f32) -> f32 {
    let n = (d - today).num_days() as f32;
    if n >= 0.0 { (-n / future_days).exp() } else { 0.7 * (n / past_days).exp() }
}

/// How a document is matched: by its text, or — for a query that is a day (and month) — by its dates.
enum Mode<'a> {
    Text(&'a Query),
    Date((Option<i32>, u32, u32)),
}

impl Mode<'_> {
    fn score(&self, doc: &Doc) -> Match {
        match self {
            Mode::Text(q) => score_doc(q, doc),
            Mode::Date(p) => score_dates(*p, doc),
        }
    }
}

/// One category's kept matches ranked (`bonus` adds the kind's own points), the first `limit` with their reasons.
fn best<'a, D>(docs: &'a [D], kept: Vec<(usize, Match)>, doc: impl Fn(&D) -> &Doc, bonus: impl Fn(&D, &Match) -> f32, q: &Query, limit: usize) -> (usize, Vec<(&'a D, Match, Vec<Why>)>) {
    let mut v: Vec<(f32, &D, Match)> = kept.into_iter().map(|(i, m)| (100.0 * m.score + bonus(&docs[i], &m), &docs[i], m)).collect();
    let total = v.len();
    v.sort_by(|a, b| b.0.total_cmp(&a.0));
    v.truncate(limit);
    (total, v.into_iter().map(|(_, d, m)| { let w = reasons(doc(d), &m, q); (d, m, w) }).collect())
}

/// Every matching document of the wanted categories: (category 0 events / 1 contacts / 2 tasks, index, match).
fn gather(ix: &Index, mode: &Mode, scope: Option<&'static str>) -> Vec<(u8, usize, Match)> {
    let want = |k: &str| scope.is_none_or(|s| s == k);
    let mut out = vec![];
    let mut add = |cat: u8, docs: &mut dyn Iterator<Item = &Doc>| {
        for (i, d) in docs.enumerate() {
            let m = mode.score(d);
            if m.score > 0.0 {
                out.push((cat, i, m));
            }
        }
    };
    if want("event") {
        add(0, &mut ix.events.iter().map(|e| &e.doc));
    }
    if want("contact") {
        add(1, &mut ix.contacts.iter().map(|c| &c.doc));
    }
    if want("task") {
        add(2, &mut ix.tasks.iter().map(|t| &t.doc));
    }
    out
}

/// The relevance floor, relative to the best match: clear best hits (a whole word or a word's start in
/// a title, ≥ .85) drop what is far below them; a vague best (a typo, letters left out) keeps nearly
/// everything, since that is all there is. Many clear hits raise the floor a little more.
fn cutoff(all: &mut Vec<(u8, usize, Match)>) {
    let min = floor(&all.iter().map(|x| x.2.score).collect::<Vec<_>>());
    all.retain(|x| x.2.score >= min);
}

/// The lowest score `cutoff` keeps among `scores`.
fn floor(scores: &[f32]) -> f32 {
    let top = scores.iter().copied().fold(0.0, f32::max);
    let clear = scores.iter().filter(|&&s| s >= 0.7 * top).count();
    let mut r = if top >= 0.85 { 0.5 } else if top >= 0.6 { 0.35 } else { 0.15 };
    if clear >= 10 {
        r += 0.1;
    }
    r * top
}

fn rank(ix: &Index, all: Vec<(u8, usize, Match)>, mode: &Mode, q: &Query, today: NaiveDate, limit: usize, res: &mut SearchResult) {
    let mut by: [Vec<(usize, Match)>; 3] = Default::default();
    for (cat, i, m) in all {
        by[cat as usize].push((i, m));
    }
    let [evs, cts, tks] = by;
    // the occurrence shown: the next one (a date query: the next one on that day), else the last
    let occ = |e: &EvDoc| {
        let on = |o: &&(NaiveDateTime, String)| match mode { Mode::Date(p) => DateField::of("", o.0.date(), 0.0).hits(*p), _ => true };
        e.occ.iter().filter(on).find(|o| o.0.date() >= today).or(e.occ.iter().filter(on).last()).cloned().unwrap_or_default()
    };
    let (total, hits) = best(&ix.events, evs, |e| &e.doc, |e, _| 30.0 * near(occ(e).0.date(), today, 60.0, 30.0) - if e.cancelled { 8.0 } else { 0.0 }, q, limit);
    res.totals.events = total;
    res.events = hits
        .into_iter()
        .map(|(e, _, mut why)| {
            why.iter_mut().filter(|w| w.f == "date").for_each(|w| w.shown = true); // the row shows the occurrence's day
            let (start, time) = occ(e);
            EventHit { key: e.key.clone(), uid: e.uid.clone(), summary: e.summary.clone(), date: start.date(), time, color: e.color.clone(), cal_name: e.cal_name.clone(), count: e.occ.len(), why }
        })
        .collect();
    let (total, hits) = best(&ix.contacts, cts, |c| &c.doc, |_, _| 0.0, q, limit);
    res.totals.contacts = total;
    res.contacts = hits.into_iter().map(|(c, _, why)| ContactHit { why, ..c.hit.clone() }).collect();
    let (total, hits) = best(&ix.tasks, tks, |k| &k.doc, |k, _| if k.open { 12.0 + k.hit.due.map_or(0.0, |d| 8.0 * near(d, today, 30.0, 30.0)) } else { 0.0 }, q, limit);
    res.totals.tasks = total;
    res.tasks = hits.into_iter().map(|(k, _, why)| TaskHit { why, ..k.hit.clone() }).collect();
}

/// `limit`: hits per category (the drop-down asks for 5, the "all results" dialog for 200); every hit
/// carries why it was found. A query that is a day (`17.11.`, `17.11.1987`) searches the dates.
pub async fn search(state: &AppState, user: &Principal, q: &str, today: NaiveDate, tz: chrono_tz::Tz, lang: &str, limit: usize) -> AppResult<SearchResult> {
    let (scope, rest) = split_scope(q);
    let jump = if scope.is_none() { parse_date_query(rest, today) } else { None };
    let mut query = Query::new(rest);
    let mut res = SearchResult { jump, scope, events: vec![], contacts: vec![], tasks: vec![], totals: Totals::default() };
    if query.phrase.chars().count() < 2 {
        return Ok(res);
    }
    let ix = index(state, user, today, tz, lang).await?;
    if let Some((y, Some(m), Some(d))) = date_parts(rest) {
        let mode = Mode::Date((y, m, d));
        let all = gather(&ix, &mode, scope);
        rank(&ix, all, &mode, &query, today, limit, &mut res);
        return Ok(res);
    }
    // loosen step by step while there is no clear hit and fewer than 3 are left above the floor
    let mut all = vec![];
    for tier in 0..=2 {
        query.tier = tier;
        all = gather(&ix, &Mode::Text(&query), scope);
        if all.iter().any(|x| !x.2.partial) {
            all.retain(|x| !x.2.partial); // documents with every word make those with half of them moot
        }
        cutoff(&mut all);
        let top = all.iter().map(|x| x.2.score).fold(0.0, f32::max);
        if all.len() >= 3 || top >= 0.85 {
            break;
        }
    }
    rank(&ix, all, &Mode::Text(&query), &query, today, limit, &mut res);
    Ok(res)
}

/// A past event offered as the template of a new one while its title is typed (`suggest`).
#[derive(Debug, serde::Serialize)]
pub struct Suggestion {
    /// the event whose attributes the new one takes: the title's most recent past occurrence
    pub key: String,
    pub summary: String,
    /// past occurrences of the title (every event and series with it, within the corpus' 2 years back)
    pub count: usize,
    pub last: NaiveDate,
    pub time: String,
    pub color: String,
    pub cal_name: String,
}

/// Up to `limit` past events whose title matches `q`, one per title (folded words). The title match
/// only has to pass the relevance floor of `search` (`floor`: a clear match drops the typos); among
/// what passes, the title used most often ranks first, then the closer match, then the more recent.
/// Cancelled events and events without a past occurrence are no template.
pub async fn suggest(state: &AppState, user: &Principal, q: &str, now: NaiveDateTime, tz: chrono_tz::Tz, lang: &str, limit: usize) -> AppResult<Vec<Suggestion>> {
    let mut query = Query::new(q);
    if query.phrase.chars().count() < 2 {
        return Ok(vec![]);
    }
    query.tier = 1; // typos and letters left out count in a title (weight 1)
    let ix = index(state, user, now.date(), tz, lang).await?;
    let past = ix.past.get_or_try_init(|| past_events(state, user, now.date(), tz)).await?;
    Ok(suggestions(past, &query, now, limit))
}

/// Every occurrence up to tomorrow, one document per event or series with its title as the only
/// field. Unlike the search corpus it reaches back without a limit: a title used a lot years ago
/// is still a good template.
async fn past_events(state: &AppState, user: &Principal, today: NaiveDate, tz: chrono_tz::Tz) -> AppResult<Vec<EvDoc>> {
    let t0 = Instant::now();
    let cals = events::visible_calendars(state, user, None).await?;
    let (from, to) = (NaiveDate::from_ymd_opt(1900, 1, 1).unwrap(), today + Duration::days(1));
    let mut by_key: HashMap<String, usize> = HashMap::new();
    let mut evs: Vec<EvDoc> = vec![];
    let inst = events::instances(state, &cals, from, to, tz).await?;
    let n = inst.len();
    for i in inst {
        let k = i.key();
        let at = *by_key.entry(k.clone()).or_insert_with(|| {
            let doc = Doc { fields: vec![Field::new("title", &i.summary, 1.0)], ..Default::default() };
            evs.push(EvDoc { key: k, uid: i.uid.clone(), summary: i.summary.clone(), color: i.color.clone(), cal_name: i.cal_name.clone(), cancelled: i.cancelled, occ: vec![], doc });
            evs.len() - 1
        });
        evs[at].occ.push((i.start, i.time_label())); // `instances` is sorted by start
    }
    tracing::debug!(occurrences = n, events = evs.len(), ms = t0.elapsed().as_millis() as u64, "past events for suggestions");
    Ok(evs)
}

fn suggestions(events: &[EvDoc], q: &Query, now: NaiveDateTime, limit: usize) -> Vec<Suggestion> {
    // folded title → (past occurrences, score, index of the event with the latest one, that occurrence)
    let mut groups: HashMap<String, (usize, f32, usize, &(NaiveDateTime, String))> = HashMap::new();
    for (i, e) in events.iter().enumerate().filter(|(_, e)| !e.cancelled) {
        let past = e.occ.partition_point(|o| o.0 < now); // `occ` is sorted
        let (Some(last), Some(title)) = (past.checked_sub(1).map(|p| &e.occ[p]), e.doc.fields.first()) else { continue };
        let score = field_score(q, title);
        if score == 0.0 {
            continue;
        }
        let g = groups.entry(title.words.join(" ")).or_insert((0, score, i, last));
        g.0 += past;
        g.1 = g.1.max(score); // "Zahnarzt" and "Zahnarzt!" share the group, not the phrase bonus
        if last.0 > g.3.0 {
            (g.2, g.3) = (i, last);
        }
    }
    let min = floor(&groups.values().map(|g| g.1).collect::<Vec<_>>());
    let mut v: Vec<_> = groups.into_values().filter(|g| g.1 >= min).collect();
    v.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.total_cmp(&a.1)).then(b.3.0.cmp(&a.3.0)));
    v.into_iter()
        .take(limit)
        .map(|(count, _, i, last)| {
            let e = &events[i];
            Suggestion { key: e.key.clone(), summary: e.summary.clone(), count, last: last.0.date(), time: last.1.clone(), color: e.color.clone(), cal_name: e.cal_name.clone() }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_queries() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 15).unwrap();
        assert_eq!(parse_date_query("2027", today).unwrap().date, NaiveDate::from_ymd_opt(2027, 1, 1).unwrap());
        assert_eq!(parse_date_query("März", today).unwrap().date, NaiveDate::from_ymd_opt(2026, 3, 1).unwrap());
        assert_eq!(parse_date_query("3. März 2027", today).unwrap().date, NaiveDate::from_ymd_opt(2027, 3, 3).unwrap());
        assert_eq!(parse_date_query("15.09.", today).unwrap().date, NaiveDate::from_ymd_opt(2026, 9, 15).unwrap());
        assert_eq!(parse_date_query("1.1.27", today).unwrap().date, NaiveDate::from_ymd_opt(2027, 1, 1).unwrap());
        assert!(parse_date_query("Zahnarzt", today).is_none());
        assert_eq!(parse_date_query("sep 2027", today).unwrap().view, "month");
    }

    #[test]
    fn folding_and_scopes() {
        assert_eq!(fold("Müller Straße ÉTÉ"), "muller strasse ete");
        assert_eq!(split_scope("termin: Zahnarzt"), (Some("event"), " Zahnarzt"));
        assert_eq!(split_scope("Kontakt:anna"), (Some("contact"), "anna"));
        assert_eq!(split_scope("todo: x").0, Some("task"));
        assert_eq!(split_scope("12:30 Treffen"), (None, "12:30 Treffen"));
    }

    fn doc(text: &str) -> Doc {
        Doc { fields: vec![Field::new("t", text, 1.0)], ..Default::default() }
    }
    fn score(q: &str, text: &str) -> f32 {
        score_doc(&Query::new(q), &doc(text)).score
    }
    /// the exact pass only (typos are the fallback)
    fn strict(q: &str, text: &str) -> f32 {
        score_doc(&Query { tier: 0, ..Query::new(q) }, &doc(text)).score
    }

    #[test]
    fn ranking_by_match_kind() {
        // one query against different fields: the whole field, its start, a word, a word's start, inside a word, a typo
        let order = [score("zahnarzt", "Zahnarzt"), score("zahnarzt", "Zahnarzt Weber"), score("zahnarzt", "Termin Zahnarzt"), score("zahnarzt", "Termin Zahnarztpraxis"), score("zahnarzt", "Kinderzahnarzt"), score("zahnartz", "Termin Zahnarzt")];
        assert!(order.windows(2).all(|w| w[0] > w[1]) && order[5] > 0.0, "{order:?}");
        assert_eq!(score("zahnarzt weber", "Zahnarzt"), 0.0, "every word must match");
        assert!(score("muller", "Müller") > 0.9, "diacritics fold");
        assert_eq!(score("xy", "Zahnarzt"), 0.0);
        assert!(score("dietmarstein", "Dietmar Steinbach") > score("steinba", "Dietmar Steinbach") * 0.8, "words run together");
        assert_eq!(score("marstein", "Dietmar Steinbach"), 0.0, "run together only from a word's start");
        assert_eq!(score("dietm", "Dietmar Steinbach") > 0.0, true);
        let chain = score("dietstein", "Dietmar Steinbach");
        assert!(chain > 0.0 && chain < score("dietmarstein", "Dietmar Steinbach"), "starts of consecutive words: {chain}");
        assert!(score("chrstein", "Christin Steinbach") > 0.0);
        assert_eq!(strict("dstein", "Dietmar Steinbach"), 0.0, "a part needs 2 letters");
        assert_eq!(strict("urlab", "Urlaubs-Abrechnung"), 0.0, "the last part needs 3 letters");
        let loose = |q: &str, d: &Doc| score_doc(&Query { tier: 2, ..Query::new(q) }, d).score;
        assert!(loose("zahnarzt weber", &doc("Zahnarzt")) > 0.0 && score("zahnarzt weber", "Zahnarzt") == 0.0, "the loosest tier lets half the words go missing");
        assert_eq!(loose("zahnarzt weber meier", &doc("Zahnarzt")), 0.0, "but not more than half");
        let note = Doc { fields: vec![Field::new("t", "x", 1.0), Field::new("note", "abgesagt", 0.4)], ..Default::default() };
        assert!(loose("abgesagx", &note) > 0.0, "the loosest tier allows typos everywhere");
        assert_eq!(loose("zugesagt", &note), 0.0, "a typo keeps the first letter");
        assert_eq!(strict("dietbach", "Dietmar Steinbach"), 0.0, "no skipped word, starts only");
        let d = Doc { fields: vec![Field::new("t", "Zahnarzt", 1.0), Field::collection("Familie")], ..Default::default() };
        assert!(score_doc(&Query::new("familie zahn"), &d).score > 0.0 && score_doc(&Query::new("familie"), &d).score == 0.0, "a collection name only narrows");
        let bday = DateField::vcard("bday", "1987-11-17", 0.95).unwrap();
        assert!(bday.hits((Some(1987), 11, 17)) && bday.hits((None, 11, 17)) && !bday.hits((Some(1988), 11, 17)));
        assert!(DateField::vcard("bday", "--11-17", 1.0).unwrap().hits((Some(2030), 11, 17)), "a date without a year");
        assert_eq!(date_parts("17.11.1987"), Some((Some(1987), Some(11), Some(17))));
        let long = format!("{} Zahnarzt abgesagt {}", "x".repeat(300), "y".repeat(300));
        let c = crop(&long, &["abgesagt"]);
        assert!(c.contains("abgesagt") && c.starts_with('…') && c.ends_with('…') && c.chars().count() <= 122, "{c}");
        assert!(score("chrstinb", "Christin Steinbach") > score("chrstinb", "Christine Ackermann"), "letters in order break the tie");
        assert!(score("chrstinbach", "Christin Steinbach") > 0.0, "letters left out");
        assert!(score("chrstinb", "Christin Steinbach") > score("chrstinb", "Christian Berger"), "a typo of a whole word beats letters left out");
        assert_eq!(strict("christina", "Christin Altersvorsorge"), 0.0, "one letter of the next word is no joined match");
        assert_eq!(score("chrstinbach", "Christine Ackermann"), 0.0);
        let note = Doc { fields: vec![Field::new("t", "Einkauf", 1.0), Field::new("note", "Christin Steinbach kommt", 0.4)], ..Default::default() };
        assert_eq!(score_doc(&Query::new("chrstinbach"), &note).score, 0.0, "no fuzzy kinds in a note");
        assert_eq!(score_doc(&Query::new("zugesagt"), &Doc { fields: vec![Field::new("t", "x", 1.0), Field::new("note", "abgesagt", 0.4)], ..Default::default() }).score, 0.0, "no typos in a note");
        let m = score_doc(&Query::new("chrstinb"), &doc("Christin Steinbach"));
        assert_eq!((m.hits[0].kind, m.hits[0].typo.as_deref()), (Kind::Typo, Some("christin")));
        assert!(score("kalnder", "Kalender") > 0.0, "one edit below 8 letters");
        let m = score_doc(&Query::new("kalner"), &doc("Kalender"));
        assert!(m.score > 0.0 && m.hits[0].kind == Kind::Gaps, "two edits on 6 letters are no typo, but letters left out");
        assert_eq!(score("demo", "devops"), 0.0, "no typos below 5 letters");
        assert_eq!(score("kalner", "Termin Zahnarzt"), 0.0);
        assert!(score("0171 234", "x") == 0.0 && score_doc(&Query::new("234"), &Doc { digits: vec!["01712345".into()], ..doc("Anna") }).score > 0.0, "digits match phone numbers");
    }

    fn ev(title: &str, days_ago: &[i64], cancelled: bool) -> EvDoc {
        let now = NaiveDate::from_ymd_opt(2026, 9, 15).unwrap().and_hms_opt(12, 0, 0).unwrap();
        let mut occ: Vec<(NaiveDateTime, String)> = days_ago.iter().map(|d| (now - Duration::days(*d), format!("t{d}"))).collect();
        occ.sort();
        EvDoc { key: format!("k/{title}/{}", days_ago.first().unwrap_or(&0)), uid: String::new(), summary: title.into(), color: String::new(), cal_name: String::new(), cancelled, occ, doc: Doc { fields: vec![Field::new("title", title, 1.0)], ..Default::default() } }
    }

    fn suggest_titles(evs: &[EvDoc], q: &str) -> Vec<(String, usize)> {
        let now = NaiveDate::from_ymd_opt(2026, 9, 15).unwrap().and_hms_opt(12, 0, 0).unwrap();
        let mut query = Query::new(q);
        query.tier = 1;
        suggestions(evs, &query, now, 3).into_iter().map(|s| (s.summary, s.count)).collect()
    }

    #[test]
    fn suggestions_rank_by_frequency_before_the_exact_title() {
        let evs = vec![
            ev("Zahnarzt", &[40], false),
            ev("Zahnarzt Kontrolle", &[10, 100, 200], false),
            ev("zahnarzt kontrolle", &[300], false), // same title, another event: counted together
            ev("Zahnarztpraxis", &[5, 6], false),
            ev("Zahnreinigung", &[1, 2, 3, 4, 5], false), // no match for "zahnarzt"
            ev("Zahnarzt Notdienst", &[-3], false),     // only in the future: no template
            ev("Zahnarzt abgesagt", &[1, 2, 3, 4, 5, 6], true), // cancelled
        ];
        assert_eq!(suggest_titles(&evs, "zahnarzt"), vec![("Zahnarzt Kontrolle".into(), 4), ("Zahnarztpraxis".into(), 2), ("Zahnarzt".into(), 1)]);
    }

    #[test]
    fn suggestions_take_the_latest_occurrence_and_drop_typos_beside_clear_matches() {
        let evs = vec![ev("Yoga", &[30], false), ev("Yoga", &[2, 9], false), ev("Yogo Kurs", &[1, 2, 3, 4], false), ev("Kalender pflegen", &[7], false)];
        let now = NaiveDate::from_ymd_opt(2026, 9, 15).unwrap().and_hms_opt(12, 0, 0).unwrap();
        let mut q = Query::new("yoga");
        q.tier = 1;
        let s = suggestions(&evs, &q, now, 3);
        assert_eq!(s.len(), 1, "a typo (.5) falls below a whole word's floor");
        assert_eq!((s[0].count, s[0].key.as_str(), s[0].time.as_str()), (3, "k/Yoga/2", "t2"));
        assert_eq!(suggest_titles(&evs, "kalnder"), vec![("Kalender pflegen".into(), 1)], "a typo alone is still offered");
    }
}
