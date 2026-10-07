//! Tiers, floor and per-kind ranking (`search`), and the title suggestions (`suggest`).

use super::*;

/// Nearness of a day to today, 0..1: upcoming days weigh more and fade slower than past ones.
pub(super) fn near(d: NaiveDate, today: NaiveDate, future_days: f32, past_days: f32) -> f32 {
    let n = (d - today).num_days() as f32;
    if n >= 0.0 { (-n / future_days).exp() } else { 0.7 * (n / past_days).exp() }
}

/// How a document is matched: by its text, or — for a query that is a day (and month) — by its dates.
pub(super) enum Mode<'a> {
    Text(&'a Query),
    Date((Option<i32>, u32, u32)),
}

impl Mode<'_> {
    pub(super) fn score(&self, doc: &Doc) -> Match {
        match self {
            Mode::Text(q) => score_doc(q, doc),
            Mode::Date(p) => score_dates(*p, doc),
        }
    }
}

/// One category's kept matches ranked (`bonus` adds the kind's own points), the first `limit` with their reasons.
pub(super) fn best<'a, D>(docs: &'a [D], kept: Vec<(usize, Match)>, doc: impl Fn(&D) -> &Doc, bonus: impl Fn(&D, &Match) -> f32, q: &Query, limit: usize) -> (usize, Vec<(&'a D, Match, Vec<Why>)>) {
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
pub(super) fn cutoff(all: &mut Vec<(u8, usize, Match)>) {
    let min = floor(&all.iter().map(|x| x.2.score).collect::<Vec<_>>());
    all.retain(|x| x.2.score >= min);
}

/// The lowest score `cutoff` keeps among `scores`.
pub(super) fn floor(scores: &[f32]) -> f32 {
    let top = scores.iter().copied().fold(0.0, f32::max);
    let clear = scores.iter().filter(|&&s| s >= 0.7 * top).count();
    let mut r = if top >= 0.85 { 0.5 } else if top >= 0.6 { 0.35 } else { 0.15 };
    if clear >= 10 {
        r += 0.1;
    }
    r * top
}

pub(super) fn rank(ix: &Index, all: Vec<(u8, usize, Match)>, mode: &Mode, q: &Query, today: NaiveDate, limit: usize, res: &mut SearchResult) {
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
            evs.push(EvDoc { key: k, uid: i.uid.clone(), summary: i.summary.to_string(), color: i.color.clone(), cal_name: i.cal_name.clone(), cancelled: i.cancelled, occ: vec![], doc });
            evs.len() - 1
        });
        evs[at].occ.push((i.start, i.time_label())); // `instances` is sorted by start
    }
    tracing::debug!(occurrences = n, events = evs.len(), ms = t0.elapsed().as_millis() as u64, "past events for suggestions");
    Ok(evs)
}

pub(super) fn suggestions(events: &[EvDoc], q: &Query, now: NaiveDateTime, limit: usize) -> Vec<Suggestion> {
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
