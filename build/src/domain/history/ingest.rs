//! Recording: read what moved in RustiCal's change logs since the last cursor, attribute it, store the versions
//! (content-addressed blobs) as changes; retention purge.

use super::*;

pub(super) fn hash(text: &str) -> String {
    use base64ct::Encoding;
    base64ct::Base64UrlUnpadded::encode_string(&Sha256::digest(text.as_bytes())[..18])
}

fn now_utc() -> String {
    crate::clock::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// One object's move found in RustiCal's log.
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
}

impl Rec {
    pub(super) fn text(&self) -> &str {
        self.after.as_deref().or(self.before.as_deref()).unwrap_or("")
    }
    /// (key, before, after) of the attributes; a move adds the collection
    pub(super) fn attrs(&self) -> Vec<(String, String, String)> {
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

pub(super) async fn read_obj(state: &AppState, store: Store, p: &str, c: &str, id: &str) -> AppResult<Option<String>> {
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

pub(super) async fn blob(state: &AppState, h: &str) -> AppResult<Option<String>> {
    Ok(sqlx::query("SELECT body FROM practical_history_blobs WHERE hash = ?").bind(h).fetch_optional(&state.db).await?.map(|r| r.get("body")))
}

/// Everything that moved since the last read, recorded. `actor`: a UI write's attribution — what
/// it touched becomes one change of its own; the rest are DAV changes, one per object.
pub(super) async fn ingest(state: &AppState, actor: Option<(&Actor, &HashSet<ObjKey>, &HashSet<CollKey>)>) -> AppResult<Option<i64>> {
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
    state.history.moved();
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
    if n > 0 {
        state.history.moved();
    }
    Ok(n)
}
