//! Recording: read what moved in RustiCal's change logs since the last cursor, attribute it, store the versions
//! (content-addressed blobs) as changes; retention purge.

use super::*;
use sqlx::SqliteConnection;

pub(super) fn hash(text: &str) -> String {
    use base64ct::Encoding;
    base64ct::Base64UrlUnpadded::encode_string(&Sha256::digest(text.as_bytes())[..18])
}

fn now_utc() -> String {
    crate::clock::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Objects read per query when a whole collection is taken in (first sight): the texts held at once.
const CHUNK: i64 = 256;

/// One object's move found in RustiCal's log: keys and the versions' hashes (their texts are blobs from the moment
/// it is found), so a read of many objects holds no text beyond the one at hand.
pub(super) struct Rec {
    pub(super) store: Store,
    pub(super) principal: String,
    pub(super) coll: String,
    pub(super) object_id: String,
    pub(super) uid: String,
    pub(super) kind: &'static str,
    pub(super) act: &'static str,
    pub(super) from: Option<(String, String, String)>,
    pub(super) before: Option<String>,
    pub(super) after: Option<String>,
    pub(super) at: String,
    /// of the newer version (the older one for a delete)
    pub(super) title: String,
}

/// (key, before, after) of the attributes of a move from text `before` to text `after`; a move adds the collection
fn attrs(r: &Rec, before: Option<&str>, after: Option<&str>) -> Vec<(String, String, String)> {
    let a = |t: Option<&str>| t.map(|t| ht::attrs(r.store, t)).unwrap_or_default();
    let mut d = match r.act {
        // an object that appears or goes: its content as it was (for display and search)
        "create" | "delete" | "restore" => ht::diff(&a(before), &a(after)).into_iter().filter(|(k, _, _)| !ht::is_tech(k)).collect(),
        _ => ht::diff(&a(before), &a(after)),
    };
    if let Some((p, c, _)) = &r.from {
        d.insert(0, (ht::COLL_KEY.into(), format!("{p}/{c}"), format!("{}/{}", r.principal, r.coll)));
    }
    d
}

pub(super) async fn read_obj<'e>(db: impl sqlx::SqliteExecutor<'e>, store: Store, p: &str, c: &str, id: &str) -> AppResult<Option<String>> {
    let row = match store {
        Store::Cal => sqlx::query("SELECT ics AS t FROM calendarobjects WHERE principal = ? AND cal_id = ? AND id = ? AND deleted_at IS NULL"),
        Store::Ab => sqlx::query("SELECT vcf AS t FROM addressobjects WHERE principal = ? AND addressbook_id = ? AND id = ? AND deleted_at IS NULL"),
    }
    .bind(p)
    .bind(c)
    .bind(id)
    .fetch_optional(db)
    .await?;
    Ok(row.map(|r| r.get("t")))
}

async fn last_act(db: &mut SqliteConnection, store: Store, uid: &str) -> AppResult<Option<String>> {
    Ok(sqlx::query("SELECT act FROM practical_history_items WHERE store = ? AND uid = ? ORDER BY change_id DESC LIMIT 1").bind(store.tag()).bind(uid).fetch_optional(db).await?.map(|r| r.get("act")))
}

pub(super) async fn blob<'e>(db: impl sqlx::SqliteExecutor<'e>, h: &str) -> AppResult<Option<String>> {
    Ok(sqlx::query("SELECT body FROM practical_history_blobs WHERE hash = ?").bind(h).fetch_optional(db).await?.map(|r| r.get("body")))
}

/// The version `text` is the last one seen of the object: its blob and head; returns its hash.
async fn put_head(db: &mut SqliteConnection, store: Store, p: &str, c: &str, id: &str, uid: &str, text: &str) -> AppResult<String> {
    let h = hash(text);
    sqlx::query("INSERT OR IGNORE INTO practical_history_blobs (hash, body) VALUES (?, ?)").bind(&h).bind(text).execute(&mut *db).await?;
    sqlx::query("INSERT OR REPLACE INTO practical_history_heads (store, principal, coll, object_id, uid, hash) VALUES (?, ?, ?, ?, ?, ?)").bind(store.tag()).bind(p).bind(c).bind(id).bind(uid).bind(&h).execute(&mut *db).await?;
    Ok(h)
}

/// The collections whose synctoken moved past the cursor: (store, principal, collection, synctoken, cursor — None at first sight).
async fn moved(db: &mut SqliteConnection) -> AppResult<Vec<(Store, String, String, i64, Option<i64>)>> {
    let mut out = vec![];
    for store in [Store::Cal, Store::Ab] {
        let colls_sql = match store {
            Store::Cal => "SELECT principal, id, synctoken FROM calendars",
            Store::Ab => "SELECT principal, id, synctoken FROM addressbooks",
        };
        let seen: HashMap<(String, String), i64> = sqlx::query("SELECT principal, coll, synctoken FROM practical_history_cursors WHERE store = ?")
            .bind(store.tag())
            .fetch_all(&mut *db)
            .await?
            .into_iter()
            .map(|r| ((r.get("principal"), r.get("coll")), r.get("synctoken")))
            .collect();
        for row in sqlx::query(colls_sql).fetch_all(&mut *db).await? {
            let (p, c, tok): (String, String, i64) = (row.get("principal"), row.get("id"), row.get("synctoken"));
            let cur = seen.get(&(p.clone(), c.clone())).copied();
            if !cur.is_some_and(|x| x >= tok) {
                out.push((store, p, c, tok, cur));
            }
        }
    }
    Ok(out)
}

/// Everything that moved since the last read, recorded. `actor`: a UI write's attribution — what
/// it touched becomes one change of its own; the rest are DAV changes, one per object.
/// Reading and recording run in one write transaction, object by object (a whole collection in chunks of CHUNK):
/// a read of N objects keeps N small records (keys, hashes, title) and one object's texts and diff at a time.
/// Before, every text was held twice and every diff at once until the end: an import of 35 000 objects
/// took +198 MiB RSS (w3-data report).
pub(super) async fn ingest(state: &AppState, actor: Option<(&Actor, &HashSet<ObjKey>, &HashSet<CollKey>)>) -> AppResult<Option<i64>> {
    // the poller's tick mostly finds nothing: that look takes no write lock
    if moved(&mut *state.db.acquire().await?).await?.is_empty() {
        return Ok(None);
    }
    let mut tx = state.db.begin_with(rustical_store_sqlite::BEGIN_IMMEDIATE).await?;
    let mut recs: Vec<Rec> = vec![];
    for (store, p, c, tok, cur) in moved(&mut tx).await? {
        let Some(cur) = cur else {
            // first sight: what is there is the starting point, not a change — unless the UI
            // write being recorded made the collection (an import into a new one): then all
            // of it is that write's (RustiCal's import of a new collection logs nothing)
            let mine = actor.is_some_and(|(_, _, colls)| colls.contains(&(store, p.clone(), c.clone())));
            let all_sql = match store {
                Store::Cal => "SELECT id, ics AS t FROM calendarobjects WHERE principal = ? AND cal_id = ? AND deleted_at IS NULL AND id > ? ORDER BY id LIMIT ?",
                Store::Ab => "SELECT id, vcf AS t FROM addressobjects WHERE principal = ? AND addressbook_id = ? AND deleted_at IS NULL AND id > ? ORDER BY id LIMIT ?",
            };
            let mut after_id = String::new();
            loop {
                let rows = sqlx::query(all_sql).bind(&p).bind(&c).bind(&after_id).bind(CHUNK).fetch_all(&mut *tx).await?;
                let full = rows.len() as i64 == CHUNK;
                for o in rows {
                    let (id, t): (String, String) = (o.get("id"), o.get("t"));
                    let uid = ht::uid(&t).unwrap_or_else(|| id.clone());
                    let h = put_head(&mut tx, store, &p, &c, &id, &uid, &t).await?;
                    if mine {
                        recs.push(Rec { store, principal: p.clone(), coll: c.clone(), object_id: id.clone(), uid, kind: ht::kind(store, &t), act: "create", from: None, before: None, after: Some(h), at: now_utc(), title: ht::title(store, &t) });
                    }
                    after_id = id;
                }
                if !full {
                    break;
                }
            }
            put_cursor(&mut tx, store, &p, &c, tok).await?;
            continue;
        };
        let log_sql = match store {
            Store::Cal => "SELECT object_id, MAX(created_at) AS at FROM calendarobjectchangelog WHERE principal = ? AND cal_id = ? AND synctoken > ? GROUP BY object_id ORDER BY MAX(synctoken)",
            Store::Ab => "SELECT object_id, MAX(created_at) AS at FROM addressobjectchangelog WHERE principal = ? AND addressbook_id = ? AND synctoken > ? GROUP BY object_id ORDER BY MAX(synctoken)",
        };
        let heads: HashMap<String, String> = sqlx::query("SELECT object_id, hash FROM practical_history_heads WHERE store = ? AND principal = ? AND coll = ?")
            .bind(store.tag())
            .bind(&p)
            .bind(&c)
            .fetch_all(&mut *tx)
            .await?
            .into_iter()
            .map(|r| (r.get("object_id"), r.get("hash")))
            .collect();
        for l in sqlx::query(log_sql).bind(&p).bind(&c).bind(cur).fetch_all(&mut *tx).await? {
            let (id, at): (String, String) = (l.get("object_id"), l.get::<Option<String>, _>("at").unwrap_or_else(now_utc));
            let now = read_obj(&mut *tx, store, &p, &c, &id).await?;
            let head = heads.get(&id);
            let before = match head {
                Some(h) => blob(&mut *tx, h).await?,
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
            let act = if act == "create" && last_act(&mut tx, store, &uid).await?.as_deref() == Some("delete") { "restore" } else { act };
            let (kind, title) = (ht::kind(store, text), ht::title(store, text));
            let after = match &now {
                Some(t) => Some(put_head(&mut tx, store, &p, &c, &id, &uid, t).await?),
                None => {
                    sqlx::query("DELETE FROM practical_history_heads WHERE store = ? AND principal = ? AND coll = ? AND object_id = ?").bind(store.tag()).bind(&p).bind(&c).bind(&id).execute(&mut *tx).await?;
                    None
                }
            };
            // a blob's hash is its key: the head's
            let before = before.and(head.cloned());
            recs.push(Rec { store, principal: p.clone(), coll: c.clone(), object_id: id, uid, kind, act, from: None, before, after, at, title });
        }
        put_cursor(&mut tx, store, &p, &c, tok).await?;
    }

    // attribution: what the UI write touched is its change
    let (mut mine, theirs): (Vec<Rec>, Vec<Rec>) = recs.into_iter().partition(|r| {
        actor.is_some_and(|(_, objs, colls)| objs.contains(&(r.store, r.principal.clone(), r.coll.clone(), r.object_id.clone())) || colls.contains(&(r.store, r.principal.clone(), r.coll.clone())))
    });
    let mut theirs = merge_moves(theirs);
    mine = merge_moves(mine);
    // devices first: they were there before the write that is being recorded
    theirs.sort_by(|a, b| a.at.cmp(&b.at));
    for r in &theirs {
        write_change(&mut tx, "dav", None, true, None, None, &r.at, std::slice::from_ref(r)).await?;
    }
    let mut id = None;
    if let Some((a, _, _)) = actor
        && !mine.is_empty()
    {
        let n = write_change(&mut tx, a.src, Some(&a.who), false, a.label.as_deref(), a.reverts, &now_utc(), &mine).await?;
        if let Some(r) = a.reverts {
            mark_reverted(&mut tx, r, n).await?;
        }
        id = Some(n);
    }
    tx.commit().await?;
    state.history.moved();
    Ok(id)
}

async fn put_cursor(db: &mut SqliteConnection, store: Store, p: &str, c: &str, tok: i64) -> AppResult<()> {
    sqlx::query("INSERT OR REPLACE INTO practical_history_cursors (store, principal, coll, synctoken) VALUES (?, ?, ?, ?)").bind(store.tag()).bind(p).bind(c).bind(tok).execute(db).await?;
    Ok(())
}

/// A delete and a create of the same UID in one read are a move (a DAV MOVE is both).
fn merge_moves(recs: Vec<Rec>) -> Vec<Rec> {
    let mut out: Vec<Rec> = vec![];
    let mut dels: Vec<Rec> = vec![];
    for r in recs {
        if r.act == "delete" { dels.push(r) } else { out.push(r) }
    }
    for d in dels {
        // only within one principal: across principals a pairing would hand the source's whole text to the
        // target's readers (`read::visible` shows a move's `before`), so there it stays a delete and a create
        match out.iter_mut().find(|c| c.act == "create" && c.from.is_none() && c.store == d.store && c.uid == d.uid && c.principal == d.principal && c.coll != d.coll) {
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

/// One change of `recs` (never empty). The versions are read back from their blobs one item at a time; the change's
/// row goes in with its first item: `device_of_new` (a DAV change, one object) takes the device from that item's new
/// version (a deleted object's text is its creator's: none), and a change of one item knows its `tech` flag by then.
#[allow(clippy::too_many_arguments)]
async fn write_change(tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>, src: &str, who: Option<&str>, device_of_new: bool, label: Option<&str>, reverts: Option<i64>, at: &str, recs: &[Rec]) -> AppResult<i64> {
    let mut id = None;
    let mut tech = true;
    for (i, r) in recs.iter().enumerate() {
        let before = match &r.before {
            Some(h) => blob(&mut **tx, h).await?,
            None => None,
        };
        let after = match &r.after {
            Some(h) => blob(&mut **tx, h).await?,
            None => None,
        };
        let attrs = attrs(r, before.as_deref(), after.as_deref());
        tech &= r.act == "update" && attrs.iter().all(|(k, _, _)| ht::is_tech(k));
        let cid = match id {
            Some(x) => x,
            None => {
                let device = if device_of_new { after.as_deref().and_then(ht::prodid) } else { None };
                let x: i64 = sqlx::query("INSERT INTO practical_history_changes (at, src, who, device, label, reverts, tech) VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING id")
                    .bind(at)
                    .bind(src)
                    .bind(who)
                    .bind(device)
                    .bind(label)
                    .bind(reverts)
                    .bind(recs.len() == 1 && tech)
                    .fetch_one(&mut **tx)
                    .await?
                    .get("id");
                id = Some(x);
                x
            }
        };
        sqlx::query(
            "INSERT INTO practical_history_items (change_id, idx, store, principal, coll, object_id, uid, kind, act, from_principal, from_coll, from_object, before_hash, after_hash, title, attrs) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(cid)
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
        .bind(&r.before)
        .bind(&r.after)
        .bind(&r.title)
        .bind(serde_json::to_string(&attrs).unwrap_or_else(|_| "[]".into()))
        .execute(&mut **tx)
        .await?;
    }
    let id = id.ok_or_else(|| AppError::Internal("history: a change without items".into()))?;
    if recs.len() > 1 && tech {
        sqlx::query("UPDATE practical_history_changes SET tech = 1 WHERE id = ?").bind(id).execute(&mut **tx).await?;
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
    if n > 0 {
        state.history.moved();
    }
    Ok(n)
}
