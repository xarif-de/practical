//! Matching: folding, per-word match kinds, typo distance, field and document scores, the reasons shown.

use super::*;

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

pub(super) fn words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_string).collect()
}

/// One searchable text: folded, with its words; `code` names it for "why found", `shown` = the hit
/// displays it already (no reason needed).
#[derive(Debug, Default, Clone)]
pub(super) struct Field {
    pub(super) code: &'static str,
    pub(super) shown: bool,
    /// narrows only: a query matching nothing but such fields (a calendar's name) finds nothing
    pub(super) narrow: bool,
    /// what "why found" quotes (cut around the match, `crop`)
    pub(super) raw: String,
    pub(super) text: String,
    pub(super) words: Vec<String>,
    /// the words run together, and where each starts in it: a query typed without spaces
    compact: String,
    starts: Vec<usize>,
    pub(super) weight: f32,
}

impl Field {
    pub(super) fn new(code: &'static str, raw: &str, weight: f32) -> Self {
        Self::matching(code, raw, raw, weight)
    }
    /// matched against `text`, quoted as `raw` (status words: every language matches, the user's shows)
    pub(super) fn matching(code: &'static str, raw: &str, text: &str, weight: f32) -> Self {
        let text = fold(text);
        let words = words(&text);
        let (mut compact, mut starts) = (String::new(), vec![]);
        for w in &words {
            starts.push(compact.len());
            compact.push_str(w);
        }
        Field { code, shown: false, narrow: false, raw: raw.trim().to_string(), text, words, compact, starts, weight }
    }
    pub(super) fn shown(mut self) -> Self {
        self.shown = true;
        self
    }
    /// the collection's name: weighs least and only narrows what the other words find
    pub(super) fn collection(raw: &str) -> Self {
        Field { narrow: true, ..Self::new("coll", raw, 0.15) }
    }
}

/// A date of a document that a date query (a day and a month, the year optional) matches.
#[derive(Debug, Clone)]
pub(super) struct DateField {
    pub(super) code: &'static str,
    /// None: a date without a year (vCard `--MM-DD`), which every year of the query matches
    pub(super) y: Option<i32>,
    pub(super) m: u32,
    pub(super) d: u32,
    pub(super) weight: f32,
    pub(super) raw: String,
}

impl DateField {
    pub(super) fn of(code: &'static str, d: NaiveDate, weight: f32) -> Self {
        DateField { code, y: Some(d.year()), m: d.month(), d: d.day(), weight, raw: d.format("%d.%m.%Y").to_string() }
    }
    /// vCard dates: `1987-11-17`, `19871117`, `--11-17`, `--1117` (a time part is ignored)
    pub(super) fn vcard(code: &'static str, v: &str, weight: f32) -> Option<Self> {
        let v = v.trim().split('T').next()?.replace('-', "");
        if !v.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
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
    pub(super) fn hits(&self, (y, m, d): (Option<i32>, u32, u32)) -> bool {
        self.m == m && self.d == d && (y.is_none() || self.y.is_none() || self.y == y)
    }
}

/// Everything a document is searched by.
#[derive(Debug, Default, Clone)]
pub(super) struct Doc {
    pub(super) fields: Vec<Field>,
    pub(super) dates: Vec<DateField>,
    /// phone numbers: their digits, and the numbers as written (for "why found")
    pub(super) digits: Vec<String>,
    pub(super) phones: String,
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
pub(super) enum Kind {
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
    pub(super) fn code(self) -> &'static str {
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
pub(super) struct Match {
    pub(super) score: f32,
    pub(super) hits: Vec<Hit>,
    /// some query words matched nothing (the loosest tier)
    pub(super) partial: bool,
}

#[derive(Debug)]
pub(super) struct Hit {
    pub(super) at: Place,
    pub(super) kind: Kind,
    /// index into `Query::words`; `usize::MAX` = the whole query (a date)
    pub(super) word: usize,
    pub(super) typo: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Place {
    Field(usize),
    Date(usize),
    Phone,
}

/// The text match of a document (score 0 = none): every query word must match some field; a field
/// that only narrows (a collection's name) cannot carry a match alone. Ties go to the document whose
/// main text holds all the query's letters in order.
pub(super) fn score_doc(q: &Query, doc: &Doc) -> Match {
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
pub(super) fn field_score(q: &Query, f: &Field) -> f32 {
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
pub(super) fn score_dates(pat: (Option<i32>, u32, u32), doc: &Doc) -> Match {
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
pub(super) fn crop(raw: &str, words: &[&str]) -> String {
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
pub(super) fn reasons(doc: &Doc, m: &Match, q: &Query) -> Vec<Why> {
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
