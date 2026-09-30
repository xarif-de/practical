//! Universal search (top bar): events, contacts and tasks ranked by relevance, and a "jump to"
//! suggestion when the query looks like a date.
//!
//! Matching (`score_doc`): the query is folded (lower case, diacritics off) and split into words;
//! EVERY word must match some field. A word scores by how it matches — a whole word 1.0, a word
//! prefix .85, run together over word boundaries from a word's start .75 (4+ letters: "dietmarstein" ~
//! "Dietmar Steinbach"), inside a word .6 (3+ letters), a typo (edit distance 1 from 5 letters on, 2 from 8,
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
}

pub use crate::domain::contacts::ContactHit;

const MONTHS_DE: [&str; 12] = ["januar", "februar", "märz", "april", "mai", "juni", "juli", "august", "september", "oktober", "november", "dezember"];
const MONTHS_EN: [&str; 12] = ["january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november", "december"];
const MONTHS_FR: [&str; 12] = ["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre", "décembre"];
const MONTHS_ES: [&str; 12] = ["enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto", "septiembre", "octubre", "noviembre", "diciembre"];

/// `2027`, `märz`, `3. märz 2027`, `15.09.`, `15.9.2026`, `2026-09-15`, `sep 2027`.
pub fn parse_date_query(q: &str, today: NaiveDate) -> Option<JumpTo> {
    let t = q.trim().to_lowercase();
    if t.is_empty() {
        return None;
    }
    if let Ok(d) = NaiveDate::parse_from_str(&t, "%Y-%m-%d") {
        return Some(JumpTo { date: d, label: d.format("%d.%m.%Y").to_string(), view: "day" });
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
                if n >= 1900 && n <= 2200 {
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

/// One searchable text: folded, with its words.
#[derive(Debug, Default)]
struct Field {
    text: String,
    words: Vec<String>,
    /// the words run together, and where each starts in it: a query typed without spaces
    compact: String,
    starts: Vec<usize>,
    weight: f32,
}

impl Field {
    fn new(raw: &str, weight: f32) -> Self {
        let text = fold(raw);
        let words = words(&text);
        let (mut compact, mut starts) = (String::new(), vec![]);
        if words.len() > 1 {
            for w in &words {
                starts.push(compact.len());
                compact.push_str(w);
            }
        }
        Field { text, words, compact, starts, weight }
    }
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

/// The text score of a document (0 = no match): every query word must match some field.
fn score_doc(q: &Query, fields: &[Field], digits: &[String]) -> f32 {
    if q.words.is_empty() {
        return 0.0;
    }
    let mut sum = 0.0;
    for (w, wc) in q.words.iter().zip(&q.chars) {
        let mut best = fields.iter().map(|f| f.weight * word_score(w, wc, f, q.fuzzy)).fold(0.0, f32::max);
        // a phone number: the query's digits inside the number's digits
        if best == 0.0 && w.len() >= 3 && w.chars().all(|c| c.is_ascii_digit()) && digits.iter().any(|d| d.contains(w.as_str())) {
            best = 0.9;
        }
        if best == 0.0 {
            return 0.0;
        }
        sum += best;
    }
    let mut s = sum / q.words.len() as f32;
    if let Some(main) = fields.first() {
        if main.text == q.phrase {
            s += 0.25;
        } else if main.text.starts_with(&q.phrase) {
            s += 0.15;
        }
    }
    s
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
    fields: Vec<Field>,
}

struct CtDoc {
    hit: ContactHit,
    fields: Vec<Field>,
    digits: Vec<String>,
}

struct TkDoc {
    hit: TaskHit,
    open: bool,
    fields: Vec<Field>,
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

/// The synctokens of the user's collections, the day and the zone: a change of any rebuilds the corpus.
async fn fingerprint(state: &AppState, cals: &[crate::domain::collections::CalInfo], abs: &[crate::domain::collections::AbInfo], today: NaiveDate, tz: chrono_tz::Tz) -> AppResult<u64> {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (today, tz.name()).hash(&mut h);
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

async fn index(state: &AppState, user: &Principal, today: NaiveDate, tz: chrono_tz::Tz) -> AppResult<Arc<Index>> {
    let cals = events::visible_calendars(state, user, None).await?;
    let abs = collections::addressbooks_for(state, user, false).await?;
    let stamp = fingerprint(state, &cals, &abs, today, tz).await?;
    if let Some(ix) = state.search_index.0.lock().unwrap().get(&user.id).filter(|ix| ix.stamp == stamp) {
        return Ok(ix.clone());
    }
    // ±2 years around today: expanding 4 000 objects takes ~40 ms on the real store
    let inst: Vec<Instance> = events::instances(state, &cals, today - Duration::days(730), today + Duration::days(730), tz).await?;
    let mut by_key: HashMap<String, usize> = HashMap::new();
    let mut evs: Vec<EvDoc> = vec![];
    for i in &inst {
        let k = i.key();
        let at = *by_key.entry(k.clone()).or_insert_with(|| {
            evs.push(EvDoc {
                key: k.clone(),
                uid: i.uid.clone(),
                summary: i.summary.clone(),
                color: i.color.clone(),
                cal_name: i.cal_name.clone(),
                cancelled: i.cancelled,
                occ: vec![],
                fields: vec![Field::new(&i.summary, 1.0), Field::new(&i.location, 0.75), Field::new(&i.categories.join(" "), 0.7), Field::new(&i.description.chars().take(2000).collect::<String>(), 0.4)],
            });
            evs.len() - 1
        });
        evs[at].occ.push((i.start, i.time_label()));
    }
    let mut cts = vec![];
    for c in contacts::contacts_for(state, &abs).await? {
        let bday = !c.birthday.is_empty();
        let sub = if bday { c.birthday_label() } else if !c.org.is_empty() { c.org.clone() } else { c.category() };
        let mails: Vec<&str> = c.emails.iter().map(|e| e.value.as_str()).collect();
        let places: Vec<String> = c.addresses.iter().map(|a| format!("{} {}", a.street, a.city)).collect();
        let fields = vec![
            Field::new(&c.full_name, 1.0),
            Field::new(&format!("{} {} {}", c.nickname, c.given, c.family), 0.9),
            Field::new(&format!("{} {}", c.org, c.department), 0.8),
            Field::new(&mails.join(" "), 0.7),
            Field::new(&c.categories.join(" "), 0.6),
            Field::new(&places.join(" "), 0.5),
            Field::new(&c.note.chars().take(1000).collect::<String>(), 0.35),
        ];
        let digits = c.phones.iter().map(|p| p.value.chars().filter(char::is_ascii_digit).collect::<String>()).filter(|d| d.len() >= 3).collect();
        cts.push(CtDoc { hit: ContactHit { bday, ..ContactHit::of(&c, sub) }, fields, digits });
    }
    let mut tks = vec![];
    for t in tasks::tasks_for(state, &cals, tz).await? {
        let fields = vec![Field::new(&t.summary, 1.0), Field::new(&t.categories.join(" "), 0.7), Field::new(&t.location, 0.6), Field::new(&t.description.chars().take(2000).collect::<String>(), 0.4)];
        let open = t.open();
        tks.push(TkDoc { hit: TaskHit { key: t.key(), uid: t.uid.clone(), summary: t.summary.clone(), due: t.due.map(|d| d.date()), completed: t.completed(), cal_name: t.cal_name.clone(), color: t.color.clone() }, open, fields });
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

fn rank(ix: &Index, query: &Query, scope: Option<&'static str>, today: NaiveDate, limit: usize, res: &mut SearchResult) {
    let want = |k: &str| scope.is_none_or(|s| s == k);
    if want("event") {
        let mut v = vec![];
        for e in &ix.events {
            let t = score_doc(query, &e.fields, &[]);
            if t == 0.0 {
                continue;
            }
            let (start, time) = e.occ.iter().find(|o| o.0.date() >= today).or(e.occ.last()).cloned().unwrap_or_default();
            let s = 100.0 * t + 30.0 * near(start.date(), today, 60.0, 30.0) - if e.cancelled { 8.0 } else { 0.0 };
            v.push((s, EventHit { key: e.key.clone(), uid: e.uid.clone(), summary: e.summary.clone(), date: start.date(), time, color: e.color.clone(), cal_name: e.cal_name.clone(), count: e.occ.len() }));
        }
        (res.totals.events, res.events) = top(v, limit);
    }
    if want("contact") {
        let v = ix.contacts.iter().filter_map(|c| { let t = score_doc(query, &c.fields, &c.digits); (t > 0.0).then(|| (100.0 * t, c.hit.clone())) }).collect();
        (res.totals.contacts, res.contacts) = top(v, limit);
    }
    if want("task") {
        let v = ix
            .tasks
            .iter()
            .filter_map(|k| {
                let t = score_doc(query, &k.fields, &[]);
                (t > 0.0).then(|| (100.0 * t + if k.open { 12.0 + k.hit.due.map_or(0.0, |d| 8.0 * near(d, today, 30.0, 30.0)) } else { 0.0 }, k.hit.clone()))
            })
            .collect();
        (res.totals.tasks, res.tasks) = top(v, limit);
    }
}

/// `limit`: hits per category (the drop-down asks for 5, the "all results" dialog for 200).
pub async fn search(state: &AppState, user: &Principal, q: &str, today: NaiveDate, tz: chrono_tz::Tz, limit: usize) -> AppResult<SearchResult> {
    let (scope, rest) = split_scope(q);
    let jump = if scope.is_none() { parse_date_query(rest, today) } else { None };
    let query = Query::new(rest);
    let mut res = SearchResult { jump, scope, events: vec![], contacts: vec![], tasks: vec![], totals: Totals::default() };
    if query.phrase.chars().count() < 2 {
        return Ok(res);
    }
    let ix = index(state, user, today, tz).await?;
    let mut query = query;
    query.fuzzy = false;
    rank(&ix, &query, scope, today, limit, &mut res);
    if res.totals.events + res.totals.contacts + res.totals.tasks < 3 {
        query.fuzzy = true;
        rank(&ix, &query, scope, today, limit, &mut res);
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

    fn score(q: &str, text: &str) -> f32 {
        score_doc(&Query::new(q), &[Field::new(text, 1.0)], &[])
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
        assert!(score("kalnder", "Kalender") > 0.0 && score("kalner", "Kalender") == 0.0, "one edit below 8 letters");
        assert_eq!(score("demo", "devops"), 0.0, "no typos below 5 letters");
        assert_eq!(score("kalner", "Termin Zahnarzt"), 0.0);
        assert!(score("0171 234", "x") == 0.0 && score_doc(&Query::new("234"), &[Field::new("Anna", 1.0)], &["01712345".into()]) > 0.0, "digits match phone numbers");
    }
}
