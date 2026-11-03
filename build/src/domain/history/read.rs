//! Reading: the changes a user may see, as list items.

use super::*;

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

// the columns every reader takes (macros: `concat!` keeps each statement a literal, as `sqlx::query` wants)
macro_rules! change_cols {
    () => {
        "id, at, src, who, device, label, reverts, undone_by, inactive_by, tech"
    };
}
macro_rules! item_cols {
    () => {
        "change_id, idx, store, principal, coll, object_id, uid, kind, act, from_principal, from_coll, from_object, before_hash, after_hash, title, attrs"
    };
}

/// Every change the user may see, whole (the rules: `assemble`), oldest first: the reference the paged readers
/// ([`visible_ids`], [`visible_for`]) are tested against (it::history).
#[cfg(test)]
pub async fn visible(state: &AppState, user: &Principal) -> AppResult<Vec<Change>> {
    let changes = sqlx::query(concat!("SELECT ", change_cols!(), " FROM practical_history_changes ORDER BY id")).fetch_all(&state.db).await?;
    let items = sqlx::query(concat!("SELECT ", item_cols!(), " FROM practical_history_items ORDER BY change_id, idx")).fetch_all(&state.db).await?;
    Ok(assemble(user, changes, items))
}

/// Rows of the changes and of their items (by change, idx) → the changes as the user may see them: those touching
/// their own or their groups' collections (items of other collections left out), oldest first; a change with no
/// such item is left out. `before`/`after` of the items are blob hashes.
fn assemble(user: &Principal, changes: Vec<sqlx::sqlite::SqliteRow>, items: Vec<sqlx::sqlite::SqliteRow>) -> Vec<Change> {
    let mut by_id: BTreeMap<i64, Change> = BTreeMap::new();
    for r in changes {
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
    for r in items {
        let mut it = row_item(&r);
        let (to, from) = (user.is_principal(&it.principal), it.from.as_ref().is_some_and(|f| user.is_principal(&f.0)));
        if !to && !from {
            continue;
        }
        // a move between principals (recorded before moves paired within one principal only): each side
        // reads only its own half
        if it.from.is_some() && !from {
            it.before = None;
            it.attrs.iter_mut().for_each(|(_, old, _)| old.clear());
        }
        if !to {
            it.after = None;
            it.attrs.iter_mut().for_each(|(_, _, new)| new.clear());
        }
        if let Some(c) = by_id.get_mut(&r.get::<i64, _>("change_id")) {
            c.items.push(it);
        }
    }
    by_id.into_values().filter(|c| !c.items.is_empty()).collect()
}

/// The changes the user may see (`assemble`) as (at, id), newest first by time (an import's objects are dated when
/// they were made, so ids are not in time order), without reading them: one pass over the items' keys. The same
/// rule — an item counts when its collection's principal or, for a move, the one it came from is the user or one of
/// their groups (`Principal::is_principal`: id equality), and a change counts with one such item.
pub async fn visible_ids(state: &AppState, user: &Principal) -> AppResult<Vec<(String, i64)>> {
    let me = serde_json::to_string(&std::iter::once(&user.id).chain(&user.memberships).collect::<Vec<_>>()).expect("json");
    let sql = "SELECT DISTINCT c.at AS at, i.change_id AS id FROM practical_history_items i JOIN practical_history_changes c ON c.id = i.change_id
        WHERE i.principal IN (SELECT value FROM json_each(?)) OR i.from_principal IN (SELECT value FROM json_each(?)) ORDER BY c.at DESC, i.change_id DESC";
    Ok(sqlx::query(sql).bind(&me).bind(&me).fetch_all(&state.db).await?.into_iter().map(|r| (r.get("at"), r.get("id"))).collect())
}

/// What [`plan`] of each of the changes `ids` needs, as the user may see them (`assemble`): those changes (the ones the user may
/// see, whole), and every later one (id above the least of `ids`) that touches one of their elements, with only the
/// items of those elements. `plan` reads nothing else of `all`: its index is by element (`PlanMemo::later_index`)
/// and it keeps changes after the planned one only — later by id, the order they were recorded in, also where an
/// import dated a change back (`takes_over`): what was recorded after a change happened after it. A page of the history plans in a read of its own size, not of
/// every change kept (14 000 changes: one read of all and a plan each was 8 s and ~100 MiB, production 2026-11).
pub async fn visible_for(state: &AppState, user: &Principal, ids: &[i64]) -> AppResult<Vec<Change>> {
    let Some(min) = ids.iter().min().copied() else { return Ok(vec![]) };
    let ids_json = serde_json::to_string(ids).expect("json");
    let changes = sqlx::query(concat!("SELECT ", change_cols!(), " FROM practical_history_changes WHERE id IN (SELECT value FROM json_each(?)) ORDER BY id")).bind(&ids_json).fetch_all(&state.db).await?;
    let items = sqlx::query(concat!("SELECT ", item_cols!(), " FROM practical_history_items WHERE change_id IN (SELECT value FROM json_each(?)) ORDER BY change_id, idx")).bind(&ids_json).fetch_all(&state.db).await?;
    let mut out = assemble(user, changes, items);
    // the later changes of their elements: per store through the (store, uid) index
    let own: HashSet<i64> = ids.iter().copied().collect();
    let mut later_items = vec![];
    for store in [Store::Cal, Store::Ab] {
        let uids: Vec<&str> = out.iter().flat_map(|c| &c.items).filter(|i| i.store == store).map(|i| i.uid.as_str()).collect::<HashSet<_>>().into_iter().collect();
        if uids.is_empty() {
            continue;
        }
        let sql = concat!("SELECT ", item_cols!(), " FROM practical_history_items WHERE store = ? AND uid IN (SELECT value FROM json_each(?)) AND change_id > ?");
        later_items.extend(sqlx::query(sql).bind(store.tag()).bind(serde_json::to_string(&uids).expect("json")).bind(min).fetch_all(&state.db).await?.into_iter().filter(|r| !own.contains(&r.get::<i64, _>("change_id"))));
    }
    if later_items.is_empty() {
        return Ok(out);
    }
    later_items.sort_by_key(|r| (r.get::<i64, _>("change_id"), r.get::<i64, _>("idx")));
    let later_ids: Vec<i64> = later_items.iter().map(|r| r.get::<i64, _>("change_id")).collect::<std::collections::BTreeSet<_>>().into_iter().collect();
    let changes = sqlx::query(concat!("SELECT ", change_cols!(), " FROM practical_history_changes WHERE id IN (SELECT value FROM json_each(?)) ORDER BY id")).bind(serde_json::to_string(&later_ids).expect("json")).fetch_all(&state.db).await?;
    out.extend(assemble(user, changes, later_items));
    out.sort_by_key(|c| c.id);
    Ok(out)
}

/// Elements that exist now: (store, uid) with a head.
pub async fn alive(state: &AppState) -> AppResult<HashSet<(String, String)>> {
    Ok(sqlx::query("SELECT store, uid FROM practical_history_heads").fetch_all(&state.db).await?.into_iter().map(|r| (r.get("store"), r.get("uid"))).collect())
}
