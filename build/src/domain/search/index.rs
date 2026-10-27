//! The per-user corpus: documents per event series, contact and task, cached per collection part and keyed
//! by the collections' synctokens.

use super::*;

#[derive(Clone)]
pub(super) struct EvDoc {
    pub(super) key: String,
    pub(super) uid: String,
    pub(super) summary: String,
    pub(super) color: String,
    pub(super) cal_name: String,
    pub(super) cancelled: bool,
    /// (start, time label), sorted
    pub(super) occ: Vec<(NaiveDateTime, String)>,
    pub(super) doc: Doc,
}

pub(super) struct CtDoc {
    pub(super) hit: ContactHit,
    pub(super) doc: Doc,
}

pub(super) struct TkDoc {
    pub(super) hit: TaskHit,
    pub(super) open: bool,
    pub(super) doc: Doc,
}

pub struct Index {
    stamp: u64,
    /// last use (`IndexCache` drops corpora idle for `IDLE`)
    used: Mutex<Instant>,
    /// what every part depends on: the window's day, the zone, the language
    base: (NaiveDate, chrono_tz::Tz, String),
    /// per calendar ("principal/id"): its synctoken, name and colour when read, and its event documents per object
    cals: HashMap<String, Arc<CalPart>>,
    /// the address books' (key, synctoken, name) the contacts were made of
    ab_stamp: Vec<(String, i64, String)>,
    pub(super) events: Vec<Arc<EvDoc>>,
    pub(super) contacts: Arc<Vec<CtDoc>>,
    pub(super) tasks: Vec<TkDoc>,
    /// every past occurrence, the title the only field (`suggest`): built on the first suggestion,
    /// dropped with the index
    pub(super) past: tokio::sync::OnceCell<Vec<EvDoc>>,
}

/// One calendar's share of the corpus: an event document per object, re-read per object when the
/// synctoken moves (RustiCal's change log names the objects).
pub(super) struct CalPart {
    token: i64,
    pub(super) name: String,
    pub(super) color: String,
    objs: HashMap<String, ObjPart>,
}

#[derive(Clone)]
pub(super) struct ObjPart {
    pub(super) doc: Arc<EvDoc>,
    /// RELATED-TO UIDs, and the titles text the doc's "related" field was made with
    related: Vec<String>,
    related_text: String,
    /// sort keys of the object's first and last occurrence (start, not all-day, title): the corpus
    /// keeps the order a full read gives (by first occurrence); the last one's title is the UID's title
    pub(super) first: (NaiveDateTime, bool, String),
    pub(super) last: (NaiveDateTime, bool, String),
}

/// One corpus per user, rebuilt in part when the fingerprint moves (see the module comment).
#[derive(Default)]
pub struct IndexCache(Mutex<HashMap<String, Arc<Index>>>);

#[cfg(test)]
impl IndexCache {
    pub fn clear(&self) {
        self.0.lock().unwrap().clear();
    }
    /// the address of each event document of `user`'s corpus by its key: unchanged objects keep theirs
    pub fn doc_ptrs(&self, user: &str) -> HashMap<String, usize> {
        self.0.lock().unwrap().get(user).map(|ix| ix.events.iter().map(|e| (e.key.clone(), Arc::as_ptr(e) as usize)).collect()).unwrap_or_default()
    }
    pub fn users(&self) -> usize {
        self.0.lock().unwrap().len()
    }
}

/// a corpus nobody searched for this long is dropped (checked on every search of anyone)
const IDLE: std::time::Duration = std::time::Duration::from_secs(1800);
/// at most this many users' corpora are kept, the least recently used go first
const MAX_CORPORA: usize = 8;

/// The synctokens, names and colours of the user's collections, the day, the zone and the language (status
/// words are quoted in it): a change of any rebuilds the part of the corpus it touches.
pub(super) fn fingerprint(cals: &[crate::domain::collections::CalInfo], abs: &[crate::domain::collections::AbInfo], today: NaiveDate, tz: chrono_tz::Tz, lang: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (today, tz.name(), lang).hash(&mut h);
    for c in cals {
        // the calendar, its view and its path (`state::Authorized::cache_key`)
        (c.auth.cache_key(), c.synctoken, &c.displayname, &c.color).hash(&mut h);
    }
    for a in abs {
        ("ab", a.auth.cache_key(), a.synctoken, &a.displayname).hash(&mut h);
    }
    h.finish()
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
pub(super) async fn index(state: &AppState, user: &Principal, today: NaiveDate, tz: chrono_tz::Tz, lang: &str) -> AppResult<Arc<Index>> {
    let cals = events::visible_calendars(state, user, None).await?;
    let abs = collections::addressbooks_for(state, user, false).await?;
    let stamp = fingerprint(&cals, &abs, today, tz, lang);
    let old = {
        let mut m = state.search_index.0.lock().unwrap();
        // idle users do not keep a corpus
        m.retain(|_, v| v.used.lock().unwrap().elapsed() < IDLE);
        m.get(&user.id).cloned()
    };
    if let Some(ix) = old.as_ref().filter(|ix| ix.stamp == stamp) {
        *ix.used.lock().unwrap() = Instant::now();
        return Ok(ix.clone());
    }
    let base = (today, tz, lang.to_string());
    // a part of the old corpus is reused only for the same day, zone and language
    let old = old.filter(|o| o.base == base);
    // ±2 years around today: expanding 4 000 objects takes ~40 ms on the real store
    let (from, to) = (today - Duration::days(730), today + Duration::days(730));
    let mut parts: HashMap<String, Arc<CalPart>> = HashMap::new();
    for cal in cals.iter().filter(|c| c.components.iter().any(|c| c == "VEVENT")) {
        // the calendar, its view and its path: a share's part is never another view's
        let ck = cal.auth.cache_key();
        let prev = old.as_ref().and_then(|o| o.cals.get(&ck)).filter(|p| p.name == cal.displayname && p.color == cal.color);
        let part = match prev {
            Some(p) if p.token == cal.synctoken => p.clone(),
            Some(p) => Arc::new(cal_part_update(state, lang, cal, p, from, to, tz).await?),
            None => Arc::new(cal_part(state, lang, cal, from, to, tz).await?),
        };
        parts.insert(ck, part);
    }
    let tasks = tasks::tasks_for(state, &cals, tz).await?;
    // the corpus' order: as a full read sorted by start lists the objects' first occurrences (calendars in
    // their order, then the object id where all else is equal)
    let cal_pos: HashMap<String, usize> = cals.iter().enumerate().map(|(n, c)| (c.auth.cache_key(), n)).collect();
    let mut order: Vec<(&ObjPart, usize, &str)> = parts.iter().flat_map(|(ck, p)| p.objs.iter().map(|(id, o)| (o, cal_pos[ck], id.as_str()))).collect();
    let sort_key = |k: &(NaiveDateTime, bool, String)| (k.0, k.1, k.2.clone());
    // RELATED-TO names the other side by UID: its title is what can be searched (the title of the UID's
    // last occurrence; tasks after events)
    let mut titles: HashMap<String, String> = HashMap::new();
    order.sort_by(|a, b| sort_key(&a.0.last).cmp(&sort_key(&b.0.last)).then(a.1.cmp(&b.1)).then(a.2.cmp(b.2)));
    for (o, _, _) in &order {
        titles.insert(o.doc.uid.clone(), o.last.2.clone());
    }
    titles.extend(tasks.iter().map(|t| (t.uid.clone(), t.summary.clone())));
    let related = |uids: &[String]| uids.iter().filter_map(|u| titles.get(u).cloned()).collect::<Vec<_>>().join(" · ");
    order.sort_by(|a, b| sort_key(&a.0.first).cmp(&sort_key(&b.0.first)).then(a.1.cmp(&b.1)).then(a.2.cmp(b.2)));
    let evs: Vec<Arc<EvDoc>> = order
        .iter()
        .map(|(o, _, _)| {
            let text = related(&o.related);
            if text == o.related_text {
                return o.doc.clone();
            }
            // a related title changed: this document again with the new one
            let mut d = (*o.doc).clone();
            if let Some(f) = d.doc.fields.iter_mut().find(|f| f.code == "related") {
                *f = Field::new("related", &text, 0.3);
            }
            Arc::new(d)
        })
        .collect();

    let ab_stamp: Vec<(String, i64, String)> = abs.iter().map(|a| (format!("{}/{}", a.principal, a.id), a.synctoken, a.displayname.clone())).collect();
    let cts = match old.as_ref().filter(|o| o.ab_stamp == ab_stamp) {
        Some(o) => o.contacts.clone(),
        None => Arc::new(contact_docs(state, &abs).await?),
    };

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
    let ix = Arc::new(Index { stamp, used: Mutex::new(Instant::now()), base, cals: parts, ab_stamp, events: evs, contacts: cts, tasks: tks, past: Default::default() });
    let mut m = state.search_index.0.lock().unwrap();
    m.insert(user.id.clone(), ix.clone());
    while m.len() > MAX_CORPORA {
        let Some(lru) = m.iter().min_by_key(|(_, v)| *v.used.lock().unwrap()).map(|(k, _)| k.clone()) else { break };
        m.remove(&lru);
    }
    Ok(ix)
}

/// A calendar's part read whole: every object's instances in the window and its search extras.
async fn cal_part(state: &AppState, lang: &str, cal: &crate::domain::collections::CalInfo, from: NaiveDate, to: NaiveDate, tz: chrono_tz::Tz) -> AppResult<CalPart> {
    let (inst, mut extras) = events::instances_with_extras(state, std::slice::from_ref(cal), from, to, tz).await?;
    let mut by_obj: HashMap<String, Vec<Instance>> = HashMap::new();
    for i in inst {
        by_obj.entry(i.object_id.clone()).or_default().push(i);
    }
    let mut objs = HashMap::new();
    for (id, insts) in by_obj {
        let x = extras.remove(&format!("{}/{}/{id}", cal.principal, cal.id)).unwrap_or_default();
        objs.insert(id, obj_part(state, lang, &insts, x));
    }
    Ok(CalPart { token: cal.synctoken, name: cal.displayname.clone(), color: cal.color.clone(), objs })
}

/// `prev` brought to the calendar's synctoken now: only the objects the change log names since `prev.token`
/// are read again (a change of name or colour reads the whole calendar: `cal_part`).
async fn cal_part_update(state: &AppState, lang: &str, cal: &crate::domain::collections::CalInfo, prev: &CalPart, from: NaiveDate, to: NaiveDate, tz: chrono_tz::Tz) -> AppResult<CalPart> {
    // the calendar itself (`(owner, id)`), never the path of a share; what is read below is its view
    let changed: Vec<String> = sqlx::query("SELECT DISTINCT object_id FROM calendarobjectchangelog WHERE principal = ? AND cal_id = ? AND synctoken > ?")
        .bind(cal.auth.owner())
        .bind(cal.auth.id())
        .bind(prev.token)
        .fetch_all(&state.db)
        .await?
        .into_iter()
        .map(|r| r.get("object_id"))
        .collect();
    // many objects: one read of the calendar is cheaper than one per object
    if changed.len() > 64 + prev.objs.len() / 4 {
        return cal_part(state, lang, cal, from, to, tz).await;
    }
    let mut objs = prev.objs.clone();
    for id in changed {
        objs.remove(&id);
        let obj = match state.collections().object(&cal.auth, &id).await {
            Ok(o) => o,
            // gone, hidden by a share, or unreadable (a full read skips a broken object too)
            Err(AppError::NotFound | AppError::Store(rustical_store::Error::NotFound | rustical_store::Error::IcalError(_))) => continue,
            Err(e) => return Err(e),
        };
        let mut insts = events::object_instances(&obj, cal, &id, from, to, tz);
        if insts.is_empty() {
            continue;
        }
        insts.sort_by(|a, b| a.start.cmp(&b.start).then(b.all_day.cmp(&a.all_day)).then(a.summary.cmp(&b.summary)));
        let x = events::object_extra(&obj, cal, &id);
        objs.insert(id, obj_part(state, lang, &insts, x));
    }
    // a calendar at the instance limit: a full read decides what is left out (`events::expand`)
    if objs.values().map(|o| o.doc.occ.len()).sum::<usize>() >= events::EXPAND_MAX {
        return cal_part(state, lang, cal, from, to, tz).await;
    }
    Ok(CalPart { token: cal.synctoken, name: cal.displayname.clone(), color: cal.color.clone(), objs })
}

/// The document of one event or series from its instances in the window (sorted by start) and the
/// object's search extras. Its "related" field is made without titles here: `index` fills it in.
fn obj_part(state: &AppState, lang: &str, insts: &[Instance], x: events::SearchExtra) -> ObjPart {
    let i = &insts[0];
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
        Field::new("related", "", 0.3),
        word_field(state, lang, &keys, 0.3),
        Field::new("geo", &geo_text(x.geo), 0.25),
        Field::collection(&i.cal_name).shown(),
    ];
    let occ: Vec<(NaiveDateTime, String)> = insts.iter().map(|i| (i.start, i.time_label())).collect();
    let dates = occ.iter().map(|o| DateField::of("date", o.0.date(), 1.0)).collect();
    let l = &insts[insts.len() - 1];
    ObjPart {
        doc: Arc::new(EvDoc { key: i.key(), uid: i.uid.clone(), summary: i.summary.to_string(), color: i.color.clone(), cal_name: i.cal_name.clone(), cancelled: i.cancelled, occ, doc: Doc { fields, dates, ..Default::default() } }),
        related: x.related,
        related_text: String::new(),
        first: (i.start, !i.all_day, i.summary.to_string()),
        last: (l.start, !l.all_day, l.summary.to_string()),
    }
}

async fn contact_docs(state: &AppState, abs: &[crate::domain::collections::AbInfo]) -> AppResult<Vec<CtDoc>> {
    let mut cts = vec![];
    for c in contacts::contacts_for(state, abs).await? {
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
    Ok(cts)
}
