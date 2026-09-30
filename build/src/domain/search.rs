//! Universal search (top bar): events, contacts and tasks ranked by relevance, and a "jump to"
//! suggestion when the query looks like a date.
//!
//! Matching (`score_doc`): the query is folded (lower case, diacritics off) and split into words;
//! EVERY word must match some field. A word scores by how it matches — a whole word 1.0, a word
//! prefix .85, run together over word boundaries from a word's start .75 (4+ letters: "dietmarstein" ~
//! "Dietmar Steinbach"), starts of consecutive words .65 (5+ letters, each part 2+: "dietstein"),
//! inside a word .6 (3+ letters), a typo (edit distance 1 from 5 letters on, 2 from 8,
//! against a word's start; a 4-letter word would match too much: "demo" ~ "devops") .5/.38 — times the field's weight; the document's text score is the
//! mean over the words plus a bonus when the whole query starts or equals the main field. Typos are
//! only tried when the exact pass finds fewer than 3 hits in all: "serie" must not bring every "server".
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

/// Why a hit was found (the "all results" dialog only): a field (code, the client names it) and its text.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Why {
    pub f: &'static str,
    pub s: String,
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
        if words.len() > 1 {
            for w in &words {
                starts.push(compact.len());
                compact.push_str(w);
            }
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

/// `q` = a start (2+ letters) of `ws[0]`, then of `ws[1]` … (`first`: at least two words must take part).
fn starts_chain(q: &str, ws: &[String], first: bool) -> bool {
    let Some(w) = ws.first() else { return false };
    let common = q.chars().zip(w.chars()).take_while(|(a, b)| a == b).map(|(a, _)| a.len_utf8()).sum::<usize>();
    // the part taken from this word: 2+ letters, as many as match; the rest must start the next word
    (2..=common).rev().filter(|&l| q.is_char_boundary(l)).any(|l| if l == q.len() { !first } else { starts_chain(&q[l..], &ws[1..], false) })
}

/// How well one query word matches a field (0 = not at all).
fn word_score(q: &str, qc: &[char], f: &Field, fuzzy: bool) -> f32 {
    if f.words.iter().any(|w| w == q) {
        return 1.0;
    }
    if f.words.iter().any(|w| w.starts_with(q)) {
        return 0.85;
    }
    let n = qc.len();
    // words typed without the space between them: from a word's start on, past its end
    if n >= 4 && f.starts.iter().zip(&f.words).any(|(&at, w)| q.len() > w.len() && f.compact[at..].starts_with(q)) {
        return 0.75;
    }
    // starts of consecutive words run together ("dietstein" ~ "Dietmar Steinbach")
    if n >= 5 && f.words.len() > 1 && (0..f.words.len() - 1).any(|k| starts_chain(q, &f.words[k..], true)) {
        return 0.65;
    }
    if n >= 3 && f.text.contains(q) {
        return 0.6;
    }
    if fuzzy && n >= 5 {
        // a typo: against the word's start of about the query's length ("kalnder" ~ "kalender")
        let max = if n < 8 { 1 } else { 2 };
        let mut best = usize::MAX;
        for w in &f.words {
            let wc: Vec<char> = w.chars().collect();
            if wc.len() + 1 < n {
                continue;
            }
            for len in [n.saturating_sub(1), n, n + 1] {
                if len == 0 || len > wc.len() {
                    continue;
                }
                best = best.min(osa(qc, &wc[..len]));
            }
            if best == 0 {
                break;
            }
        }
        if best <= max {
            return if best <= 1 { 0.5 } else { 0.38 };
        }
    }
    0.0
}

/// bit of `score_doc`'s mask for a phone number
const PHONE: u64 = 1 << 63;

/// The text score of a document (0 = no match): every query word must match some field. The mask
/// has a bit per field that gave a word its best match (bit 63: a phone number).
fn score_doc(q: &Query, doc: &Doc) -> (f32, u64) {
    if q.words.is_empty() {
        return (0.0, 0);
    }
    let (mut sum, mut mask, mut real) = (0.0, 0u64, false);
    for (w, wc) in q.words.iter().zip(&q.chars) {
        let (mut best, mut at) = (0.0f32, 0u64);
        for (i, f) in doc.fields.iter().enumerate() {
            let s = f.weight * word_score(w, wc, f, q.fuzzy);
            if s > best {
                (best, at) = (s, 1 << i.min(62));
            }
        }
        // a phone number: the query's digits inside the number's digits (a number is an identifier)
        if best < 0.85 && w.len() >= 3 && w.chars().all(|c| c.is_ascii_digit()) && doc.digits.iter().any(|d| d.contains(w.as_str())) {
            (best, at) = (0.85, PHONE);
        }
        if best == 0.0 {
            return (0.0, 0);
        }
        real |= at == PHONE || !doc.fields[at.trailing_zeros() as usize].narrow;
        sum += best;
        mask |= at;
    }
    if !real {
        return (0.0, 0); // only the calendar's / address book's name: that would be all of it
    }
    let mut s = sum / q.words.len() as f32;
    if let Some(main) = doc.fields.first() {
        if main.text == q.phrase {
            s += 0.25;
        } else if main.text.starts_with(&q.phrase) {
            s += 0.15;
        }
    }
    (s, mask)
}

/// The date score of a document for a date query (0 = no match), and the codes/texts it matched.
fn score_dates(pat: (Option<i32>, u32, u32), doc: &Doc) -> (f32, Vec<Why>) {
    let mut best = 0.0f32;
    let mut why = vec![];
    for d in doc.dates.iter().filter(|d| d.hits(pat)) {
        // a birthday with the queried year is that person's, one without is only the same day
        let s = d.weight * if pat.0.is_some() && d.y == pat.0 { 1.0 } else { 0.9 };
        best = best.max(s);
        if !why.iter().any(|w: &Why| w.f == d.code) {
            why.push(Why { f: d.code, s: d.raw.clone() });
        }
    }
    (best, why)
}

/// About 120 characters of `raw` around the first place a query word matches it (all of it when short).
fn crop(raw: &str, q: &Query) -> String {
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
    let at = q.words.iter().filter_map(|w| folded.find(w.as_str())).min().map(|b| from[folded[..b].chars().count()]).unwrap_or(0);
    let a = at.saturating_sub(40).min(chars.len() - MAX);
    let b = a + MAX;
    format!("{}{}{}", if a > 0 { "…" } else { "" }, chars[a..b].iter().collect::<String>(), if b < chars.len() { "…" } else { "" })
}

/// "Why found" for a text match: the matched fields the hit does not show anyway.
fn reasons(doc: &Doc, mask: u64, q: &Query) -> Vec<Why> {
    let mut v: Vec<Why> = doc.fields.iter().enumerate().filter(|(i, f)| mask & (1 << (*i).min(62)) != 0 && !f.shown && !f.raw.is_empty()).map(|(_, f)| Why { f: f.code, s: crop(&f.raw, q) }).collect();
    if mask & PHONE != 0 {
        v.push(Why { f: "phone", s: doc.phones.clone() });
    }
    v
}

struct Query {
    phrase: String,
    words: Vec<String>,
    chars: Vec<Vec<char>>,
    /// also match typos
    fuzzy: bool,
}

impl Query {
    fn new(raw: &str) -> Self {
        let phrase = fold(raw.trim());
        let words = words(&phrase);
        let chars = words.iter().map(|w| w.chars().collect()).collect();
        Query { phrase, words, chars, fuzzy: true }
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
    let ix = Arc::new(Index { stamp, built: Instant::now(), events: evs, contacts: cts, tasks: tks });
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

fn top<T>(mut v: Vec<(f32, T)>, limit: usize) -> (usize, Vec<T>) {
    let total = v.len();
    v.sort_by(|a, b| b.0.total_cmp(&a.0));
    (total, v.into_iter().take(limit).map(|x| x.1).collect())
}

/// How a document is matched: by its text, or — for a query that is a day (and month) — by its dates.
enum Mode<'a> {
    Text(&'a Query),
    Date((Option<i32>, u32, u32)),
}

impl Mode<'_> {
    /// (score 0..~1.25, why) — `why` only when asked for (the dialog)
    fn score(&self, doc: &Doc, why: bool) -> (f32, Vec<Why>) {
        match self {
            Mode::Text(q) => {
                let (s, mask) = score_doc(q, doc);
                (s, if why && s > 0.0 { reasons(doc, mask, q) } else { vec![] })
            }
            Mode::Date(p) => {
                let (s, w) = score_dates(*p, doc);
                (s, if why { w } else { vec![] })
            }
        }
    }
}

fn rank(ix: &Index, mode: &Mode, scope: Option<&'static str>, today: NaiveDate, limit: usize, res: &mut SearchResult) {
    let want = |k: &str| scope.is_none_or(|s| s == k);
    let why = limit > 5;
    if want("event") {
        let mut v = vec![];
        for e in &ix.events {
            let (t, mut w) = mode.score(&e.doc, why);
            if t == 0.0 {
                continue;
            }
            w.retain(|x| x.f != "date"); // the row shows the occurrence's date
            // the occurrence shown: the next one (a date query: the next one on that day), else the last
            let on = |o: &&(NaiveDateTime, String)| match mode { Mode::Date(p) => DateField::of("", o.0.date(), 0.0).hits(*p), _ => true };
            let (start, time) = e.occ.iter().filter(on).find(|o| o.0.date() >= today).or(e.occ.iter().filter(on).last()).cloned().unwrap_or_default();
            let s = 100.0 * t + 30.0 * near(start.date(), today, 60.0, 30.0) - if e.cancelled { 8.0 } else { 0.0 };
            v.push((s, EventHit { key: e.key.clone(), uid: e.uid.clone(), summary: e.summary.clone(), date: start.date(), time, color: e.color.clone(), cal_name: e.cal_name.clone(), count: e.occ.len(), why: w }));
        }
        (res.totals.events, res.events) = top(v, limit);
    }
    if want("contact") {
        let v = ix.contacts.iter().filter_map(|c| { let (t, w) = mode.score(&c.doc, why); (t > 0.0).then(|| (100.0 * t, ContactHit { why: w, ..c.hit.clone() })) }).collect();
        (res.totals.contacts, res.contacts) = top(v, limit);
    }
    if want("task") {
        let v = ix
            .tasks
            .iter()
            .filter_map(|k| {
                let (t, w) = mode.score(&k.doc, why);
                (t > 0.0).then(|| (100.0 * t + if k.open { 12.0 + k.hit.due.map_or(0.0, |d| 8.0 * near(d, today, 30.0, 30.0)) } else { 0.0 }, TaskHit { why: w, ..k.hit.clone() }))
            })
            .collect();
        (res.totals.tasks, res.tasks) = top(v, limit);
    }
}

/// `limit`: hits per category (the drop-down asks for 5, the "all results" dialog for 200, which
/// also gets the reasons). A query that is a day (`17.11.`, `17.11.1987`) searches the dates.
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
        rank(&ix, &Mode::Date((y, m, d)), scope, today, limit, &mut res);
        return Ok(res);
    }
    query.fuzzy = false;
    rank(&ix, &Mode::Text(&query), scope, today, limit, &mut res);
    if res.totals.events + res.totals.contacts + res.totals.tasks < 3 {
        query.fuzzy = true;
        rank(&ix, &Mode::Text(&query), scope, today, limit, &mut res);
    }
    Ok(res)
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
        score_doc(&Query::new(q), &doc(text)).0
    }
    /// the exact pass only (typos are the fallback)
    fn strict(q: &str, text: &str) -> f32 {
        score_doc(&Query { fuzzy: false, ..Query::new(q) }, &doc(text)).0
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
        assert_eq!(strict("dietbach", "Dietmar Steinbach"), 0.0, "no skipped word, starts only");
        let d = Doc { fields: vec![Field::new("t", "Zahnarzt", 1.0), Field::collection("Familie")], ..Default::default() };
        assert!(score_doc(&Query::new("familie zahn"), &d).0 > 0.0 && score_doc(&Query::new("familie"), &d).0 == 0.0, "a collection name only narrows");
        let bday = DateField::vcard("bday", "1987-11-17", 0.95).unwrap();
        assert!(bday.hits((Some(1987), 11, 17)) && bday.hits((None, 11, 17)) && !bday.hits((Some(1988), 11, 17)));
        assert!(DateField::vcard("bday", "--11-17", 1.0).unwrap().hits((Some(2030), 11, 17)), "a date without a year");
        assert_eq!(date_parts("17.11.1987"), Some((Some(1987), Some(11), Some(17))));
        let long = format!("{} Zahnarzt abgesagt {}", "x".repeat(300), "y".repeat(300));
        let c = crop(&long, &Query::new("abgesagt"));
        assert!(c.contains("abgesagt") && c.starts_with('…') && c.ends_with('…') && c.chars().count() <= 122, "{c}");
        assert!(score("kalnder", "Kalender") > 0.0 && score("kalner", "Kalender") == 0.0, "one edit below 8 letters");
        assert_eq!(score("demo", "devops"), 0.0, "no typos below 5 letters");
        assert_eq!(score("kalner", "Termin Zahnarzt"), 0.0);
        assert!(score("0171 234", "x") == 0.0 && score_doc(&Query::new("234"), &Doc { digits: vec!["01712345".into()], ..doc("Anna") }).0 > 0.0, "digits match phone numbers");
    }
}
