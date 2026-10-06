//! Change history: every closed change of an event, task or contact, whatever made it — a save in
//! this UI (one change, however many fields), an import (one change for all its objects) or a DAV
//! client (read off RustiCal's change log; one change per object) — with the versions before and
//! after, so it can be undone later. Undo is itself a change; the undone one links to it.
//!
//! RustiCal keeps no old versions (`REPLACE INTO calendarobjects`) and its change log names no
//! content and no author, so this module keeps its own copy: `practical_history_heads` holds the last
//! version it saw of every object (`practical_history_blobs`, content-addressed), `practical_history_cursors`
//! how far it has read each collection's log. `ingest` reads what moved since and records it; UI
//! writes run between `begin` and `Guard::commit`, which hold one lock with the poller, so what
//! they wrote is attributed to them and nothing else is.

use crate::domain::history_text::{self as ht, Store};
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use rustical_ical::{AddressObject, CalendarObject};
use rustical_store::auth::Principal;
use rustical_store::{AddressbookWriteStore, CalendarWriteStore};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Default)]
pub struct Hist {
    lock: tokio::sync::Mutex<()>,
}

/// Who a change is attributed to. `src`: ui | import | undo (DAV changes are found, never begun).
pub struct Actor {
    pub src: &'static str,
    pub who: String,
    pub label: Option<String>,
    pub reverts: Option<i64>,
}

impl Actor {
    pub fn ui(user: &Principal) -> Actor {
        Actor { src: "ui", who: user.id.clone(), label: None, reverts: None }
    }
    pub fn import(user: &Principal, label: impl Into<String>) -> Actor {
        Actor { src: "import", who: user.id.clone(), label: Some(label.into()), reverts: None }
    }
}

type ObjKey = (Store, String, String, String);
type CollKey = (Store, String, String);

/// A UI write in progress: what it touched is recorded as its change on `commit`.
pub struct Guard<'a> {
    state: &'a AppState,
    _lock: tokio::sync::MutexGuard<'a, ()>,
    actor: Actor,
    objects: HashSet<ObjKey>,
    colls: HashSet<CollKey>,
}

/// Takes the history lock and first records what devices changed meanwhile, so that it is not
/// attributed to this write. A failing history never fails the write itself.
pub async fn begin(state: &AppState, actor: Actor) -> Guard<'_> {
    let lock = state.history.lock.lock().await;
    if let Err(e) = ingest(state, None).await {
        tracing::warn!("history: {e}");
    }
    Guard { state, _lock: lock, actor, objects: HashSet::new(), colls: HashSet::new() }
}

impl Guard<'_> {
    pub fn object(&mut self, store: Store, principal: &str, coll: &str, object_id: &str) {
        self.objects.insert((store, principal.into(), coll.into(), object_id.into()));
    }
    /// every object that moves in the collection is this write's (imports; a new collection)
    pub fn coll(&mut self, store: Store, principal: &str, coll: &str) {
        self.colls.insert((store, principal.into(), coll.into()));
    }
    /// Records the write; the id of its change, if anything changed.
    pub async fn commit(self) -> Option<i64> {
        match ingest(self.state, Some((&self.actor, &self.objects, &self.colls))).await {
            Ok(id) => id,
            Err(e) => {
                tracing::warn!("history: {e}");
                None
            }
        }
    }
}

/// Polls for DAV changes (the change poller's tick).
pub async fn poll(state: &AppState) {
    let _lock = state.history.lock.lock().await;
    if let Err(e) = ingest(state, None).await {
        tracing::warn!("history: {e}");
    }
}

fn hash(text: &str) -> String {
    use base64ct::Encoding;
    base64ct::Base64UrlUnpadded::encode_string(&Sha256::digest(text.as_bytes())[..18])
}

fn now_utc() -> String {
    crate::clock::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// One object's move found in RustiCal's log.
struct Rec {
    store: Store,
    principal: String,
    coll: String,
    object_id: String,
    uid: String,
    kind: &'static str,
    act: &'static str,
    from: Option<(String, String, String)>,
    before: Option<String>,
    after: Option<String>,
    at: String,
}

impl Rec {
    fn text(&self) -> &str {
        self.after.as_deref().or(self.before.as_deref()).unwrap_or("")
    }
    /// (key, before, after) of the attributes; a move adds the collection
    fn attrs(&self) -> Vec<(String, String, String)> {
        let a = |t: &Option<String>| t.as_deref().map(|t| ht::attrs(self.store, t)).unwrap_or_default();
        let mut d = match self.act {
            // an object that appears or goes: its content as it was (for display and search)
            "create" | "delete" | "restore" => ht::diff(&a(&self.before), &a(&self.after)).into_iter().filter(|(k, _, _)| !ht::is_tech(k)).collect(),
            _ => ht::diff(&a(&self.before), &a(&self.after)),
        };
        if let Some((p, c, _)) = &self.from {
            d.insert(0, (ht::COLL_KEY.into(), format!("{p}/{c}"), format!("{}/{}", self.principal, self.coll)));
        }
        d
    }
}

async fn read_obj(state: &AppState, store: Store, p: &str, c: &str, id: &str) -> AppResult<Option<String>> {
    let row = match store {
        Store::Cal => sqlx::query("SELECT ics AS t FROM calendarobjects WHERE principal = ? AND cal_id = ? AND id = ? AND deleted_at IS NULL"),
        Store::Ab => sqlx::query("SELECT vcf AS t FROM addressobjects WHERE principal = ? AND addressbook_id = ? AND id = ? AND deleted_at IS NULL"),
    }
    .bind(p)
    .bind(c)
    .bind(id)
    .fetch_optional(&state.db)
    .await?;
    Ok(row.map(|r| r.get("t")))
}

async fn last_act(state: &AppState, store: Store, uid: &str) -> AppResult<Option<String>> {
    Ok(sqlx::query("SELECT act FROM practical_history_items WHERE store = ? AND uid = ? ORDER BY change_id DESC LIMIT 1").bind(store.tag()).bind(uid).fetch_optional(&state.db).await?.map(|r| r.get("act")))
}

async fn blob(state: &AppState, h: &str) -> AppResult<Option<String>> {
    Ok(sqlx::query("SELECT body FROM practical_history_blobs WHERE hash = ?").bind(h).fetch_optional(&state.db).await?.map(|r| r.get("body")))
}

/// Everything that moved since the last read, recorded. `actor`: a UI write's attribution — what
/// it touched becomes one change of its own; the rest are DAV changes, one per object.
async fn ingest(state: &AppState, actor: Option<(&Actor, &HashSet<ObjKey>, &HashSet<CollKey>)>) -> AppResult<Option<i64>> {
    let mut recs: Vec<Rec> = vec![];
    let mut heads_put: Vec<(Store, String, String, String, String, String)> = vec![]; // store p c id uid text
    let mut heads_del: Vec<ObjKey> = vec![];
    let mut cursors: Vec<(Store, String, String, i64)> = vec![];
    for store in [Store::Cal, Store::Ab] {
        let (colls_sql, log_sql, all_sql) = match store {
            Store::Cal => (
                "SELECT principal, id, synctoken FROM calendars",
                "SELECT object_id, MAX(created_at) AS at FROM calendarobjectchangelog WHERE principal = ? AND cal_id = ? AND synctoken > ? GROUP BY object_id ORDER BY MAX(synctoken)",
                "SELECT id, ics AS t FROM calendarobjects WHERE principal = ? AND cal_id = ? AND deleted_at IS NULL",
            ),
            Store::Ab => (
                "SELECT principal, id, synctoken FROM addressbooks",
                "SELECT object_id, MAX(created_at) AS at FROM addressobjectchangelog WHERE principal = ? AND addressbook_id = ? AND synctoken > ? GROUP BY object_id ORDER BY MAX(synctoken)",
                "SELECT id, vcf AS t FROM addressobjects WHERE principal = ? AND addressbook_id = ? AND deleted_at IS NULL",
            ),
        };
        let seen: HashMap<(String, String), i64> = sqlx::query("SELECT principal, coll, synctoken FROM practical_history_cursors WHERE store = ?")
            .bind(store.tag())
            .fetch_all(&state.db)
            .await?
            .into_iter()
            .map(|r| ((r.get("principal"), r.get("coll")), r.get("synctoken")))
            .collect();
        for row in sqlx::query(colls_sql).fetch_all(&state.db).await? {
            let (p, c, tok): (String, String, i64) = (row.get("principal"), row.get("id"), row.get("synctoken"));
            let cur = seen.get(&(p.clone(), c.clone())).copied();
            if cur.is_some_and(|x| x >= tok) {
                continue;
            }
            let mine = actor.is_some_and(|(_, _, colls)| colls.contains(&(store, p.clone(), c.clone())));
            if cur.is_none() {
                // first sight: what is there is the starting point, not a change — unless the UI
                // write being recorded made the collection (an import into a new one): then all
                // of it is that write's (RustiCal's import of a new collection logs nothing)
                for o in sqlx::query(all_sql).bind(&p).bind(&c).fetch_all(&state.db).await? {
                    let (id, t): (String, String) = (o.get("id"), o.get("t"));
                    let uid = ht::uid(&t).unwrap_or_else(|| id.clone());
                    if mine {
                        recs.push(Rec { store, principal: p.clone(), coll: c.clone(), object_id: id.clone(), uid: uid.clone(), kind: ht::kind(store, &t), act: "create", from: None, before: None, after: Some(t.clone()), at: now_utc() });
                    }
                    heads_put.push((store, p.clone(), c.clone(), id, uid, t));
                }
                cursors.push((store, p, c, tok));
                continue;
            }
            let heads: HashMap<String, String> = sqlx::query("SELECT object_id, hash FROM practical_history_heads WHERE store = ? AND principal = ? AND coll = ?")
                .bind(store.tag())
                .bind(&p)
                .bind(&c)
                .fetch_all(&state.db)
                .await?
                .into_iter()
                .map(|r| (r.get("object_id"), r.get("hash")))
                .collect();
            for l in sqlx::query(log_sql).bind(&p).bind(&c).bind(cur.unwrap_or(0)).fetch_all(&state.db).await? {
                let (id, at): (String, String) = (l.get("object_id"), l.get::<Option<String>, _>("at").unwrap_or_else(now_utc));
                let now = read_obj(state, store, &p, &c, &id).await?;
                let head = heads.get(&id);
                let before = match head {
                    Some(h) => blob(state, h).await?,
                    None => None,
                };
                let act = match (&before, &now) {
                    (None, Some(_)) => "create",
                    (Some(b), Some(n)) if hash(b) != hash(n) => "update",
                    (Some(_), None) => "delete",
                    _ => continue,
                };
                let text = now.as_deref().or(before.as_deref()).unwrap_or("");
                let uid = ht::uid(text).unwrap_or_else(|| id.clone());
                // back after a recorded delete (trash restore, undo): a restore, not a new object
                let act = if act == "create" && last_act(state, store, &uid).await?.as_deref() == Some("delete") { "restore" } else { act };
                match &now {
                    Some(t) => heads_put.push((store, p.clone(), c.clone(), id.clone(), uid.clone(), t.clone())),
                    None => heads_del.push((store, p.clone(), c.clone(), id.clone())),
                }
                recs.push(Rec { store, principal: p.clone(), coll: c.clone(), object_id: id, uid, kind: ht::kind(store, text), act, from: None, before, after: now, at });
            }
            cursors.push((store, p, c, tok));
        }
    }
    if recs.is_empty() && heads_put.is_empty() && cursors.is_empty() {
        return Ok(None);
    }

    // attribution: what the UI write touched is its change
    let (mut mine, theirs): (Vec<Rec>, Vec<Rec>) = recs.into_iter().partition(|r| {
        actor.is_some_and(|(_, objs, colls)| objs.contains(&(r.store, r.principal.clone(), r.coll.clone(), r.object_id.clone())) || colls.contains(&(r.store, r.principal.clone(), r.coll.clone())))
    });
    let mut theirs = merge_moves(theirs);
    mine = merge_moves(mine);

    let mut tx = state.db.begin_with(rustical_store_sqlite::BEGIN_IMMEDIATE).await?;
    for (store, p, c, id, uid, text) in &heads_put {
        let h = hash(text);
        sqlx::query("INSERT OR IGNORE INTO practical_history_blobs (hash, body) VALUES (?, ?)").bind(&h).bind(text).execute(&mut *tx).await?;
        sqlx::query("INSERT OR REPLACE INTO practical_history_heads (store, principal, coll, object_id, uid, hash) VALUES (?, ?, ?, ?, ?, ?)")
            .bind(store.tag())
            .bind(p)
            .bind(c)
            .bind(id)
            .bind(uid)
            .bind(&h)
            .execute(&mut *tx)
            .await?;
    }
    for (store, p, c, id) in &heads_del {
        sqlx::query("DELETE FROM practical_history_heads WHERE store = ? AND principal = ? AND coll = ? AND object_id = ?").bind(store.tag()).bind(p).bind(c).bind(id).execute(&mut *tx).await?;
    }
    for (store, p, c, tok) in &cursors {
        sqlx::query("INSERT OR REPLACE INTO practical_history_cursors (store, principal, coll, synctoken) VALUES (?, ?, ?, ?)").bind(store.tag()).bind(p).bind(c).bind(tok).execute(&mut *tx).await?;
    }
    // devices first: they were there before the write that is being recorded
    theirs.sort_by(|a, b| a.at.cmp(&b.at));
    for r in &theirs {
        // the new version names the device; a deleted object's text is its creator's
        let dev = r.after.as_deref().and_then(ht::prodid);
        write_change(&mut tx, "dav", None, dev.as_deref(), None, None, &r.at, std::slice::from_ref(r)).await?;
    }
    let mut id = None;
    if let Some((a, _, _)) = actor
        && !mine.is_empty()
    {
        let n = write_change(&mut tx, a.src, Some(&a.who), None, a.label.as_deref(), a.reverts, &now_utc(), &mine).await?;
        if let Some(r) = a.reverts {
            mark_reverted(&mut tx, r, n).await?;
        }
        id = Some(n);
    }
    tx.commit().await?;
    Ok(id)
}

/// A delete and a create of the same UID in one read are a move (a DAV MOVE is both).
fn merge_moves(recs: Vec<Rec>) -> Vec<Rec> {
    let mut out: Vec<Rec> = vec![];
    let mut dels: Vec<Rec> = vec![];
    for r in recs {
        if r.act == "delete" { dels.push(r) } else { out.push(r) }
    }
    for d in dels {
        match out.iter_mut().find(|c| c.act == "create" && c.from.is_none() && c.store == d.store && c.uid == d.uid && (c.principal != d.principal || c.coll != d.coll)) {
            Some(c) => {
                c.act = "move";
                c.from = Some((d.principal, d.coll, d.object_id));
                c.before = d.before;
            }
            None => out.push(d),
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
async fn write_change(tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>, src: &str, who: Option<&str>, prodid: Option<&str>, label: Option<&str>, reverts: Option<i64>, at: &str, recs: &[Rec]) -> AppResult<i64> {
    let items: Vec<(&Rec, Vec<(String, String, String)>)> = recs.iter().map(|r| (r, r.attrs())).collect();
    let tech = items.iter().all(|(r, a)| r.act == "update" && a.iter().all(|(k, _, _)| ht::is_tech(k)));
    let id: i64 = sqlx::query("INSERT INTO practical_history_changes (at, src, who, device, label, reverts, tech) VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING id")
        .bind(at)
        .bind(src)
        .bind(who)
        .bind(prodid)
        .bind(label)
        .bind(reverts)
        .bind(tech)
        .fetch_one(&mut **tx)
        .await?
        .get("id");
    for (i, (r, attrs)) in items.iter().enumerate() {
        let put = |t: &Option<String>| t.as_ref().map(|t| (hash(t), t.clone()));
        let (b, a) = (put(&r.before), put(&r.after));
        for (h, t) in b.iter().chain(a.iter()) {
            sqlx::query("INSERT OR IGNORE INTO practical_history_blobs (hash, body) VALUES (?, ?)").bind(h).bind(t).execute(&mut **tx).await?;
        }
        sqlx::query(
            "INSERT INTO practical_history_items (change_id, idx, store, principal, coll, object_id, uid, kind, act, from_principal, from_coll, from_object, before_hash, after_hash, title, attrs) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(i as i64)
        .bind(r.store.tag())
        .bind(&r.principal)
        .bind(&r.coll)
        .bind(&r.object_id)
        .bind(&r.uid)
        .bind(r.kind)
        .bind(r.act)
        .bind(r.from.as_ref().map(|f| &f.0))
        .bind(r.from.as_ref().map(|f| &f.1))
        .bind(r.from.as_ref().map(|f| &f.2))
        .bind(b.as_ref().map(|x| &x.0))
        .bind(a.as_ref().map(|x| &x.0))
        .bind(ht::title(r.store, r.text()))
        .bind(serde_json::to_string(attrs).unwrap_or_else(|_| "[]".into()))
        .execute(&mut **tx)
        .await?;
    }
    Ok(id)
}

/// `n` undid `r`: `r` is undone; if `r` brought objects into being, every later change of them
/// is inactive now, linked to `n`; and what `r` itself had made inactive counts again.
async fn mark_reverted(tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>, r: i64, n: i64) -> AppResult<()> {
    sqlx::query("UPDATE practical_history_changes SET undone_by = ? WHERE id = ?").bind(n).bind(r).execute(&mut **tx).await?;
    sqlx::query(
        "UPDATE practical_history_changes SET inactive_by = ? WHERE id > ? AND id < ? AND undone_by IS NULL AND inactive_by IS NULL AND id IN (
           SELECT i.change_id FROM practical_history_items i JOIN practical_history_items o ON o.store = i.store AND o.uid = i.uid
           WHERE o.change_id = ? AND o.act IN ('create', 'restore'))",
    )
    .bind(n)
    .bind(r)
    .bind(n)
    .bind(r)
    .execute(&mut **tx)
    .await?;
    sqlx::query("UPDATE practical_history_changes SET inactive_by = NULL WHERE inactive_by = ?").bind(r).execute(&mut **tx).await?;
    Ok(())
}

/// Drops changes older than the retention and the versions nothing refers to any more.
pub async fn purge(state: &AppState) -> AppResult<u64> {
    let cutoff = (crate::clock::now() - chrono::Duration::days(i64::from(state.config.history_retention_days))).format("%Y-%m-%d %H:%M:%S").to_string();
    let _lock = state.history.lock.lock().await;
    let mut tx = state.db.begin_with(rustical_store_sqlite::BEGIN_IMMEDIATE).await?;
    sqlx::query("DELETE FROM practical_history_items WHERE change_id IN (SELECT id FROM practical_history_changes WHERE at < ?)").bind(&cutoff).execute(&mut *tx).await?;
    let n = sqlx::query("DELETE FROM practical_history_changes WHERE at < ?").bind(&cutoff).execute(&mut *tx).await?.rows_affected();
    if n > 0 {
        sqlx::query(
            "DELETE FROM practical_history_blobs WHERE hash NOT IN (SELECT hash FROM practical_history_heads)
               AND hash NOT IN (SELECT before_hash FROM practical_history_items WHERE before_hash IS NOT NULL)
               AND hash NOT IN (SELECT after_hash FROM practical_history_items WHERE after_hash IS NOT NULL)",
        )
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(n)
}

// ---- reading -------------------------------------------------------------------------------------

pub struct Item {
    pub store: Store,
    pub principal: String,
    pub coll: String,
    pub object_id: String,
    pub uid: String,
    pub kind: String,
    pub act: String,
    pub from: Option<(String, String, String)>,
    pub before: Option<String>,
    pub after: Option<String>,
    pub title: String,
    pub attrs: Vec<(String, String, String)>,
}

impl Item {
    /// one element across moves: store and UID
    pub fn el(&self) -> String {
        format!("{}:{}", self.store.tag(), self.uid)
    }
}

pub struct Change {
    pub id: i64,
    pub at: String,
    pub src: String,
    pub who: Option<String>,
    pub device: Option<String>,
    pub label: Option<String>,
    pub reverts: Option<i64>,
    pub undone_by: Option<i64>,
    pub inactive_by: Option<i64>,
    pub tech: bool,
    pub items: Vec<Item>,
}

impl Change {
    pub fn active(&self) -> bool {
        self.undone_by.is_none() && self.inactive_by.is_none()
    }
}

fn row_item(r: &sqlx::sqlite::SqliteRow) -> Item {
    let fp: Option<String> = r.get("from_principal");
    Item {
        store: Store::from_tag(r.get::<String, _>("store").as_str()),
        principal: r.get("principal"),
        coll: r.get("coll"),
        object_id: r.get("object_id"),
        uid: r.get("uid"),
        kind: r.get("kind"),
        act: r.get("act"),
        from: fp.map(|p| (p, r.get("from_coll"), r.get("from_object"))),
        before: r.get("before_hash"),
        after: r.get("after_hash"),
        title: r.get("title"),
        attrs: serde_json::from_str(r.get::<String, _>("attrs").as_str()).unwrap_or_default(),
    }
}

/// The changes the user may see: those touching their own or their groups' collections (items of
/// other collections left out), oldest first. `before`/`after` of the items are blob hashes.
pub async fn visible(state: &AppState, user: &Principal) -> AppResult<Vec<Change>> {
    let mut by_id: BTreeMap<i64, Change> = BTreeMap::new();
    for r in sqlx::query("SELECT * FROM practical_history_changes ORDER BY id").fetch_all(&state.db).await? {
        let id: i64 = r.get("id");
        by_id.insert(id, Change {
            id,
            at: r.get("at"),
            src: r.get("src"),
            who: r.get("who"),
            device: r.get("device"),
            label: r.get("label"),
            reverts: r.get("reverts"),
            undone_by: r.get("undone_by"),
            inactive_by: r.get("inactive_by"),
            tech: r.get::<i64, _>("tech") != 0,
            items: vec![],
        });
    }
    for r in sqlx::query("SELECT * FROM practical_history_items ORDER BY change_id, idx").fetch_all(&state.db).await? {
        let it = row_item(&r);
        if !user.is_principal(&it.principal) && !it.from.as_ref().is_some_and(|f| user.is_principal(&f.0)) {
            continue;
        }
        if let Some(c) = by_id.get_mut(&r.get::<i64, _>("change_id")) {
            c.items.push(it);
        }
    }
    Ok(by_id.into_values().filter(|c| !c.items.is_empty()).collect())
}

/// Elements that exist now: (store, uid) with a head.
pub async fn alive(state: &AppState) -> AppResult<HashSet<(String, String)>> {
    Ok(sqlx::query("SELECT store, uid FROM practical_history_heads").fetch_all(&state.db).await?.into_iter().map(|r| (r.get("store"), r.get("uid"))).collect())
}

// ---- undo ------------------------------------------------------------------------------------------

/// Why an undo is not possible, or what it would do beyond the change itself.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Warn {
    /// conflict: an attribute changed later and gets overwritten; later: later changes go inactive
    pub kind: &'static str,
    pub title: String,
    pub key: Option<String>,
    pub cur: Option<String>,
    pub old: Option<String>,
    pub by: Option<i64>,
    pub ids: Vec<i64>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Skip {
    /// gone | exists | same | coll | access
    pub why: &'static str,
    pub title: String,
    pub by: Option<i64>,
}

enum Op {
    /// (store, principal, coll, object_id)
    Delete(ObjKey),
    Restore(ObjKey, String),
    /// the object now, its text before the change, the attributes to set back (`ht::revert` at undo)
    Put(ObjKey, String, String, Vec<String>),
    Move { at: ObjKey, back: ObjKey, text: String },
}

pub struct Plan {
    /// (element, key, value now, title) of the attributes the change edited; the element gone: key ""
    pub now: Vec<(String, String, String, String)>,
    pub ok: bool,
    /// the undo is blocked for this reason (nothing it could do)
    pub why: Option<&'static str>,
    pub warns: Vec<Warn>,
    pub skips: Vec<Skip>,
    ops: Vec<Op>,
}

async fn coll_alive(state: &AppState, store: Store, p: &str, c: &str) -> AppResult<bool> {
    let sql = match store {
        Store::Cal => "SELECT 1 FROM calendars WHERE principal = ? AND id = ? AND deleted_at IS NULL",
        Store::Ab => "SELECT 1 FROM addressbooks WHERE principal = ? AND id = ? AND deleted_at IS NULL",
    };
    Ok(sqlx::query(sql).bind(p).bind(c).fetch_optional(&state.db).await?.is_some())
}

/// A live object with this UID in the collection (its id).
async fn uid_in(state: &AppState, store: Store, p: &str, c: &str, uid: &str) -> AppResult<Option<String>> {
    let r = sqlx::query("SELECT object_id FROM practical_history_heads WHERE store = ? AND principal = ? AND coll = ? AND uid = ?").bind(store.tag()).bind(p).bind(c).bind(uid).fetch_optional(&state.db).await?;
    Ok(r.map(|r| r.get("object_id")))
}

/// Reads `plan` repeats across the changes of one request (the history page plans every change):
/// each collection, object, blob and parse once, and the later changes of an element by index instead
/// of a scan over all changes per item. Lives for one request: it is a snapshot like the reads it replaces.
#[derive(Default)]
pub struct PlanMemo {
    /// (store, uid) → indexes into `all` of the active changes that touch it, ascending
    later: Option<std::sync::Arc<HashMap<(Store, String), Vec<usize>>>>,
    colls: HashMap<CollKey, bool>,
    objs: HashMap<ObjKey, Option<String>>,
    blobs: HashMap<String, Option<String>>,
    attrs: HashMap<(Store, String), std::sync::Arc<BTreeMap<String, String>>>,
}

impl PlanMemo {
    async fn coll_alive(&mut self, state: &AppState, store: Store, p: &str, c: &str) -> AppResult<bool> {
        let k = (store, p.to_string(), c.to_string());
        if let Some(v) = self.colls.get(&k) {
            return Ok(*v);
        }
        let v = coll_alive(state, store, p, c).await?;
        self.colls.insert(k, v);
        Ok(v)
    }
    async fn read_obj(&mut self, state: &AppState, k: &ObjKey) -> AppResult<Option<String>> {
        if let Some(v) = self.objs.get(k) {
            return Ok(v.clone());
        }
        let v = read_obj(state, k.0, &k.1, &k.2, &k.3).await?;
        self.objs.insert(k.clone(), v.clone());
        Ok(v)
    }
    async fn blob(&mut self, state: &AppState, h: &str) -> AppResult<Option<String>> {
        if let Some(v) = self.blobs.get(h) {
            return Ok(v.clone());
        }
        let v = blob(state, h).await?;
        self.blobs.insert(h.to_string(), v.clone());
        Ok(v)
    }
    /// Every blob the plans of `all` can read, in one query (the history page plans every change).
    pub async fn prefetch_blobs(&mut self, state: &AppState, all: &[Change]) -> AppResult<()> {
        let want: Vec<&str> = all.iter().filter(|c| c.active()).flat_map(|c| c.items.iter()).flat_map(|i| [i.before.as_deref(), i.after.as_deref()]).flatten().filter(|h| !self.blobs.contains_key(*h)).collect::<HashSet<_>>().into_iter().collect();
        if want.is_empty() {
            return Ok(());
        }
        for r in sqlx::query("SELECT hash, body FROM practical_history_blobs WHERE hash IN (SELECT value FROM json_each(?))").bind(serde_json::to_string(&want).unwrap()).fetch_all(&state.db).await? {
            self.blobs.insert(r.get("hash"), Some(r.get("body")));
        }
        for h in want {
            self.blobs.entry(h.to_string()).or_insert(None);
        }
        Ok(())
    }
    /// `ht::attrs` of a text, by a name for it (a blob hash, or "now:" + the object key)
    fn attrs(&mut self, store: Store, name: String, text: &str) -> std::sync::Arc<BTreeMap<String, String>> {
        self.attrs.entry((store, name)).or_insert_with(|| std::sync::Arc::new(ht::attrs(store, text))).clone()
    }
    fn later_index(&mut self, all: &[Change]) -> std::sync::Arc<HashMap<(Store, String), Vec<usize>>> {
        self.later.get_or_insert_with(|| {
            let mut m: HashMap<(Store, String), Vec<usize>> = HashMap::new();
            for (n, x) in all.iter().enumerate().filter(|(_, x)| x.active()) {
                for i in &x.items {
                    let v = m.entry((i.store, i.uid.clone())).or_default();
                    if v.last() != Some(&n) {
                        v.push(n);
                    }
                }
            }
            std::sync::Arc::new(m)
        })
        .clone()
    }
}

/// What undoing change `c` would do now. `all`: every visible change (for "later" and "by").
pub async fn plan(state: &AppState, user: &Principal, c: &Change, all: &[Change], memo: &mut PlanMemo) -> AppResult<Plan> {
    let blocked = |why| Plan { now: vec![], ok: false, why: Some(why), warns: vec![], skips: vec![], ops: vec![] };
    if c.undone_by.is_some() {
        return Ok(blocked("undone"));
    }
    if c.inactive_by.is_some() {
        return Ok(blocked("inactive"));
    }
    let index = memo.later_index(all);
    let later_of = |it: &Item| -> Vec<&Change> { index.get(&(it.store, it.uid.clone())).map(|v| v.iter().map(|n| &all[*n]).filter(|x| x.id > c.id).collect()).unwrap_or_default() };
    let (mut ops, mut warns, mut skips, mut nows) = (vec![], vec![], vec![], vec![]);
    for it in &c.items {
        let title = it.title.clone();
        let skip = |why, by| Skip { why, title: title.clone(), by };
        if !user.is_principal(&it.principal) || it.from.as_ref().is_some_and(|f| !user.is_principal(&f.0)) {
            skips.push(skip("access", None));
            continue;
        }
        if !memo.coll_alive(state, it.store, &it.principal, &it.coll).await? {
            skips.push(skip("coll", None));
            continue;
        }
        let key: ObjKey = (it.store, it.principal.clone(), it.coll.clone(), it.object_id.clone());
        let now = memo.read_obj(state, &key).await?;
        let gone_by = || later_of(it).into_iter().rev().find(|x| x.items.iter().any(|i| i.uid == it.uid && i.act == "delete")).map(|x| x.id);
        match it.act.as_str() {
            "create" | "restore" => {
                if now.is_none() {
                    skips.push(skip("gone", gone_by()));
                    continue;
                }
                let later: Vec<i64> = later_of(it).iter().map(|x| x.id).collect();
                if !later.is_empty() {
                    warns.push(Warn { kind: "later", title: title.clone(), key: None, cur: None, old: None, by: None, ids: later });
                }
                ops.push(Op::Delete(key));
            }
            "delete" => {
                if now.is_some() || uid_in(state, it.store, &it.principal, &it.coll, &it.uid).await?.is_some() {
                    skips.push(skip("exists", None));
                    continue;
                }
                let Some(before) = (match &it.before { Some(h) => memo.blob(state, h).await?, None => None }) else {
                    skips.push(skip("gone", None));
                    continue;
                };
                ops.push(Op::Restore(key, before));
            }
            "move" => {
                let (Some(now), Some((fp, fc, fid))) = (now, it.from.clone()) else {
                    skips.push(skip("gone", gone_by()));
                    continue;
                };
                if !memo.coll_alive(state, it.store, &fp, &fc).await? {
                    skips.push(skip("coll", None));
                    continue;
                }
                if uid_in(state, it.store, &fp, &fc, &it.uid).await?.is_some() {
                    skips.push(skip("exists", None));
                    continue;
                }
                ops.push(Op::Move { at: key, back: (it.store, fp, fc, fid), text: now });
            }
            _ => {
                let Some(now) = now else {
                    nows.push((it.el(), String::new(), String::new(), it.title.clone()));
                    skips.push(skip("gone", gone_by()));
                    continue;
                };
                let (Some(bh), Some(ah)) = (&it.before, &it.after) else { continue };
                let (Some(before), Some(after)) = (memo.blob(state, bh).await?, memo.blob(state, ah).await?) else {
                    skips.push(skip("gone", None));
                    continue;
                };
                let (b, a, n) = (memo.attrs(it.store, bh.clone(), &before), memo.attrs(it.store, ah.clone(), &after), memo.attrs(it.store, format!("now:{}/{}/{}", it.principal, it.coll, it.object_id), &now));
                let mut keys = vec![];
                for (k, old, new) in ht::diff(&b, &a) {
                    let cur = n.get(&k).cloned().unwrap_or_default();
                    nows.push((it.el(), k.clone(), cur.clone(), it.title.clone()));
                    if cur == old || ht::is_tech(&k) && !c.tech {
                        continue;
                    }
                    if cur != new {
                        let by = later_of(it).into_iter().rev().find(|x| x.items.iter().any(|i| i.uid == it.uid && i.attrs.iter().any(|a| a.0 == k))).map(|x| x.id);
                        warns.push(Warn { kind: "conflict", title: title.clone(), key: Some(k.clone()), cur: Some(cur), old: Some(old), by, ids: vec![] });
                    }
                    keys.push(k);
                }
                if keys.is_empty() {
                    skips.push(skip("same", None));
                    continue;
                }
                ops.push(Op::Put(key, now, before, keys));
            }
        }
    }
    let ok = !ops.is_empty();
    Ok(Plan { now: nows, ok, why: (!ok).then(|| skips.first().map(|s| s.why).unwrap_or("same")), warns, skips, ops })
}

async fn put_text(state: &AppState, (store, p, c, id): &ObjKey, text: &str, overwrite: bool) -> AppResult<()> {
    match store {
        Store::Cal => {
            let obj = CalendarObject::import(text, None)?;
            state.calendars.put_object(p, c, id, obj, overwrite).await?;
        }
        Store::Ab => {
            let obj = AddressObject::from_vcf(text.to_string())?;
            AddressbookWriteStore::put_object(&*state.addressbooks, p, c, id, obj, overwrite).await?;
        }
    }
    Ok(())
}

async fn delete_obj(state: &AppState, (store, p, c, id): &ObjKey) -> AppResult<()> {
    match store {
        Store::Cal => state.calendars.delete_object(p, c, id, true).await?,
        Store::Ab => AddressbookWriteStore::delete_object(&*state.addressbooks, p, c, id, true).await?,
    }
    Ok(())
}

/// Undoes change `id` as the user: a new change (src undo) that `id` then links to.
/// Warnings need `confirmed`; without it the plan comes back as a conflict.
pub async fn undo(state: &AppState, user: &Principal, id: i64, confirmed: bool) -> AppResult<Result<i64, Plan>> {
    let all = visible(state, user).await?;
    let Some(c) = all.iter().find(|c| c.id == id) else { return Err(AppError::NotFound) };
    let p = plan(state, user, c, &all, &mut PlanMemo::default()).await?;
    if !p.ok || (!confirmed && (!p.warns.is_empty() || !p.skips.is_empty())) {
        return Ok(Err(p));
    }
    let mut g = begin(state, Actor { src: "undo", who: user.id.clone(), label: None, reverts: Some(id) }).await;
    for op in &p.ops {
        match op {
            Op::Delete(k) => {
                delete_obj(state, k).await?;
                g.object(k.0, &k.1, &k.2, &k.3);
            }
            Op::Restore(k, text) => {
                put_text(state, k, text, true).await?;
                g.object(k.0, &k.1, &k.2, &k.3);
            }
            Op::Put(k, now, before, keys) => {
                put_text(state, k, &ht::revert(k.0, now, before, keys), true).await?;
                g.object(k.0, &k.1, &k.2, &k.3);
            }
            Op::Move { at, back, text } => {
                put_text(state, back, text, true).await?;
                delete_obj(state, at).await?;
                g.object(at.0, &at.1, &at.2, &at.3);
                g.object(back.0, &back.1, &back.2, &back.3);
            }
        }
    }
    match g.commit().await {
        Some(n) => Ok(Ok(n)),
        None => Err(AppError::Conflict("the undo changed nothing".into())),
    }
}

#[cfg(test)]
mod pinned;
