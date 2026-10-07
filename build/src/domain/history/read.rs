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

/// The changes the user may see: those touching their own or their groups' collections (items of
/// other collections left out), oldest first. `before`/`after` of the items are blob hashes.
pub async fn visible(state: &AppState, user: &Principal) -> AppResult<Vec<Change>> {
    let mut by_id: BTreeMap<i64, Change> = BTreeMap::new();
    for r in sqlx::query("SELECT id, at, src, who, device, label, reverts, undone_by, inactive_by, tech FROM practical_history_changes ORDER BY id").fetch_all(&state.db).await? {
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
    for r in sqlx::query("SELECT change_id, store, principal, coll, object_id, uid, kind, act, from_principal, from_coll, from_object, before_hash, after_hash, title, attrs FROM practical_history_items ORDER BY change_id, idx").fetch_all(&state.db).await? {
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
