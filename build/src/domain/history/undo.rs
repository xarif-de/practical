//! Undo: what undoing a change would do now (`plan`, with warnings and skips), and doing it (`undo`).

use super::*;

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

pub(super) enum Op {
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
    pub(super) colls: HashMap<CollKey, bool>,
    pub(super) objs: HashMap<ObjKey, Option<String>>,
    blobs: HashMap<String, Option<String>>,
    pub(super) attrs: HashMap<(Store, String), std::sync::Arc<BTreeMap<String, String>>>,
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
    pub(super) async fn read_obj(&mut self, state: &AppState, k: &ObjKey) -> AppResult<Option<String>> {
        if let Some(v) = self.objs.get(k) {
            return Ok(v.clone());
        }
        let v = read_obj(&state.db, k.0, &k.1, &k.2, &k.3).await?;
        self.objs.insert(k.clone(), v.clone());
        Ok(v)
    }
    pub(super) async fn blob(&mut self, state: &AppState, h: &str) -> AppResult<Option<String>> {
        if let Some(v) = self.blobs.get(h) {
            return Ok(v.clone());
        }
        let v = blob(&state.db, h).await?;
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
    pub(super) fn attrs(&mut self, store: Store, name: String, text: &str) -> std::sync::Arc<BTreeMap<String, String>> {
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
        None => Err(AppError::conflict("err-undo-nothing")),
    }
}
