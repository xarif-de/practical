//! /history — the change history (`domain::history`): the line map and the list with details are
//! drawn in the browser (history.js) from one JSON document, which /api/history serves again after
//! an undo or a live change. Values arrive readable (the viewer's language and zone).
//!
//! Routes: `/history`, `/api/history`, `/api/history/{id}/plan` (what an undo would do), `/api/history/{id}/undo`.
//! The document's expensive part is kept per user in [`HistoryCache`].

use crate::csrf;
use crate::ctx::PageCtx;
use crate::domain::collections;
use crate::domain::history::{self, Plan};
use crate::domain::history_text::{self as ht, Show};
use crate::error::{AppError, AppResult};
use crate::routes::calendar::{view_ctx, PageView};
use crate::state::SharedState;
use askama::Template;
use askama_web::WebTemplate;
use axum::extract::State;
use crate::extract::{Json, Path};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use rustical_store::auth::{AuthenticationProvider, Principal};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};

/// The order attributes are listed in, as the editors show them; the rest follow by name.
const ORDER: &[&str] = &[
    "X-HX-COLL", "SUMMARY", "FN", "N", "DTSTART", "DTEND", "DUE", "DURATION", "RRULE", "EXDATE", "RDATE", "LOCATION", "GEO", "DESCRIPTION", "VALARM",
    "CATEGORIES", "COLOR", "STATUS", "PERCENT-COMPLETE", "COMPLETED", "TRANSP", "CLASS", "PRIORITY", "ORGANIZER", "ATTENDEE", "URL", "ATTACH", "CONFERENCE",
    "RELATED-TO", "NICKNAME", "ORG", "TITLE", "ROLE", "TEL", "EMAIL", "IMPP", "ADR", "BDAY", "ANNIVERSARY", "RELATED", "NOTE", "PHOTO",
];

#[derive(Template, WebTemplate)]
#[template(path = "history.html")]
pub struct HistoryPage {
    pub ctx: PageCtx,
    pub data_json: String,
}

/// The browser's document: changes (oldest first) with readable attributes and whether each can be
/// undone now, the elements they belong to, the collections and people they name. Serialized as
/// `Value::to_string` would (keys sorted, compact); the changes and elements come from `HistoryCache`.
/// Fields at their default are left out (history.js `load` puts them back): a change's `dev`, `prodid`,
/// `label`, `reverts`, `undoneBy`, `inactiveBy` when null and `tech` when false, its undo's `why`/`by`
/// when null and `warn` when 0, an element's `deletedBy` when null (~19 % of the raw document).
async fn data(state: &SharedState, user: &Principal, lang: &str) -> AppResult<String> {
    // first: a write after this read moves it, so nothing read below is stored under a later generation
    let generation = state.history.generation();
    let vc = view_ctx(state, user, lang).await?;
    let mut colls: BTreeMap<String, Value> = BTreeMap::new();
    let mut tokens = vec![];
    for deleted in [true, false] {
        for c in collections::calendars_for(state, user, deleted).await? {
            tokens.push(c.synctoken);
            colls.insert(format!("{}/{}", c.principal, c.id), json!({ "name": c.displayname, "color": c.color, "kind": "cal", "deleted": deleted, "group": c.is_group }));
        }
        for a in collections::addressbooks_for(state, user, deleted).await? {
            tokens.push(a.synctoken);
            colls.insert(format!("{}/{}", a.principal, a.id), json!({ "name": a.displayname, "color": a.color, "kind": "ab", "deleted": deleted, "group": a.is_group }));
        }
    }
    let colls = serde_json::to_string(&colls).expect("json");
    // everything the changes and elements are made of: the history tables (generation), the objects now
    // (the collections' synctokens), the collections' names and states, who may see what, language, zone
    let key = format!("{generation}|{lang}|{}|{}|{:?}|{tokens:?}|{colls}", vc.tz, user.id, user.memberships);
    let part = match state.caches.history.get(&user.id, &key) {
        Some(p) => p,
        None => {
            let p = std::sync::Arc::new(changes_and_elements(state, user, lang, vc.tz, &colls).await?);
            state.caches.history.put(&user.id, key, p.clone());
            p
        }
    };
    let mut people: BTreeMap<String, Value> = BTreeMap::new();
    for w in &part.whos {
        let name = state.principals.get_principal(w).await.ok().flatten().and_then(|p| p.displayname).unwrap_or_else(|| w.clone());
        let ini: String = name.split_whitespace().filter_map(|x| x.chars().next()).take(2).collect::<String>();
        let ini = if ini.chars().count() < 2 { name.chars().take(2).collect() } else { ini };
        people.insert(w.clone(), json!({ "name": name, "ini": ini.to_uppercase() }));
    }
    // the page's strings (history.js), placeholders as %name
    let t: BTreeMap<String, Value> = state.i18n.with_prefix_json(lang, "hxj-").into_iter().map(|(k, v)| (k["hxj-".len()..].to_string(), v)).collect();
    let now = crate::clock::now().with_timezone(&vc.tz).format("%Y-%m-%dT%H:%M").to_string();
    Ok(format!(
        "{{\"changes\":{},\"colls\":{colls},\"els\":{},\"me\":{},\"now\":{},\"people\":{},\"retention\":{},\"t\":{}}}",
        part.changes,
        part.els,
        json!(user.id),
        json!(now),
        serde_json::to_string(&people).expect("json"),
        state.config.history_retention_days,
        serde_json::to_string(&t).expect("json")
    ))
}

/// The cached part of the document: `changes` and `els` serialized, and who made the changes.
pub struct HistPart {
    changes: String,
    els: String,
    whos: Vec<String>,
}

/// The history document's expensive part per user (one entry each: the last key asked for), keyed by
/// everything it is made of (`data`): a revisit and the refetch after a change elsewhere (a calendar the
/// user does not see, a principal's name) skip ~300 statements and the undo plan of every change.
/// Least recently used users go first above `HISTORY_CACHE_BYTES`.
#[derive(Default)]
pub struct HistoryCache(std::sync::Mutex<(HashMap<String, (String, std::sync::Arc<HistPart>, usize, u64)>, usize, u64)>);

const HISTORY_CACHE_BYTES: usize = 6 << 20;

impl HistoryCache {
    fn get(&self, user: &str, key: &str) -> Option<std::sync::Arc<HistPart>> {
        let mut g = self.0.lock().unwrap();
        g.2 += 1;
        let t = g.2;
        g.0.get_mut(user).filter(|e| e.0 == key).map(|e| {
            e.3 = t;
            e.1.clone()
        })
    }
    fn put(&self, user: &str, key: String, v: std::sync::Arc<HistPart>) {
        let size = key.len() + v.changes.len() + v.els.len() + v.whos.iter().map(|w| w.len() + 24).sum::<usize>() + 64;
        let mut g = self.0.lock().unwrap();
        let g = &mut *g;
        if let Some(old) = g.0.remove(user) {
            g.1 -= old.2;
        }
        if size > HISTORY_CACHE_BYTES / 2 {
            return;
        }
        while g.1 + size > HISTORY_CACHE_BYTES {
            let Some(old) = g.0.iter().min_by_key(|(_, e)| e.3).map(|(u, _)| u.clone()) else { break };
            g.1 -= g.0.remove(&old).unwrap().2;
        }
        g.2 += 1;
        g.0.insert(user.to_string(), (key, v, size, g.2));
        g.1 += size;
    }
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.0.lock().unwrap().0.len()
    }
    #[cfg(test)]
    pub fn clear(&self) {
        *self.0.lock().unwrap() = Default::default();
    }
}

async fn changes_and_elements(state: &SharedState, user: &Principal, lang: &str, tz: chrono_tz::Tz, colls_json: &str) -> AppResult<HistPart> {
    let colls: BTreeMap<String, Value> = serde_json::from_str(colls_json).expect("json");
    let t = |k: &str| state.i18n.lookup(lang, k);
    let show = Show { t: &t, lang, tz };
    let coll_name = |k: &str| colls.get(k).and_then(|c| c["name"].as_str()).map(str::to_string).unwrap_or_else(|| k.rsplit('/').next().unwrap_or(k).to_string());
    let all = history::visible(state, user).await?;
    let alive = history::alive(state).await?;
    let mut whos: Vec<String> = vec![];
    let mut els: BTreeMap<String, Value> = BTreeMap::new();
    let mut changes = vec![];
    let mut memo = history::PlanMemo::default();
    memo.prefetch_blobs(state, &all).await?;
    for c in &all {
        if let Some(w) = &c.who
            && !whos.contains(w)
        {
            whos.push(w.clone());
        }
        let plan = history::plan(state, user, c, &all, &mut memo).await?;
        let mut items = vec![];
        for it in &c.items {
            let el = it.el();
            let coll = format!("{}/{}", it.principal, it.coll);
            let exists = alive.contains(&(it.store.tag().to_string(), it.uid.clone()));
            let e = els.entry(el.clone()).or_insert_with(|| json!({ "kind": it.kind, "exists": exists }));
            e["coll"] = json!(coll);
            if !it.title.is_empty() {
                e["title"] = json!(it.title);
            }
            if it.act == "delete" && !exists {
                e["deletedBy"] = json!(c.id);
            }
            let mut shown: Vec<&(String, String, String)> = it.attrs.iter().filter(|(k, _, _)| k != "UID" && (c.tech || !ht::is_tech(k))).collect();
            shown.sort_by_key(|(k, _, _)| (ORDER.iter().position(|o| o == k).unwrap_or(ORDER.len()), k.clone()));
            // an alarm's text that only repeats the title is left out (the editor writes the title there)
            let sums: Vec<String> = it.attrs.iter().filter(|a| a.0 == "SUMMARY").flat_map(|a| [a.1.clone(), a.2.clone()]).map(|r| crate::domain::ical::unescape(ht::split_value(&r).1)).collect();
            let titles: Vec<&str> = std::iter::once(it.title.as_str()).chain(sums.iter().map(String::as_str)).collect();
            let attrs: Vec<Value> = shown
                .into_iter()
                .map(|(k, from, to)| {
                    let (mut f, mut t) = (show.value_in(k, from, &coll_name, &titles), show.value_in(k, to, &coll_name, &titles));
                    // two versions must never read the same: then show them as written
                    if f == t && from != to {
                        (f, t) = (show.raw(from), show.raw(to));
                    }
                    json!([k, show.label(k), f, t])
                })
                .collect();
            items.push(json!({ "el": el, "kind": it.kind, "act": it.act, "coll": coll, "title": it.title, "attrs": attrs }));
        }
        let mut ch = json!({ "id": c.id, "at": local(&c.at, tz), "src": c.src, "who": c.who, "items": items });
        let dev = if c.src == "dav" { ht::device(c.device.as_deref()) } else { None };
        for (k, v) in [("dev", json!(dev)), ("prodid", json!(c.device)), ("label", json!(c.label)), ("reverts", json!(c.reverts)), ("undoneBy", json!(c.undone_by)), ("inactiveBy", json!(c.inactive_by))] {
            if !v.is_null() {
                ch[k] = v;
            }
        }
        if c.tech {
            ch["tech"] = json!(true);
        }
        let mut undo = json!({ "ok": plan.ok });
        let warn = plan.warns.len() + if plan.ok { plan.skips.len() } else { 0 };
        for (k, v) in [("why", json!(plan.why)), ("by", json!(plan.skips.first().and_then(|s| s.by))), ("warn", if warn > 0 { json!(warn) } else { Value::Null })] {
            if !v.is_null() {
                undo[k] = v;
            }
        }
        ch["undo"] = undo;
        changes.push(ch);
    }
    for (el, e) in els.iter_mut() {
        if e.get("title").is_none() {
            e["title"] = json!(el.split_once(':').map(|x| x.1).unwrap_or(el));
        }
    }
    Ok(HistPart { changes: serde_json::to_string(&changes).expect("json"), els: serde_json::to_string(&els).expect("json"), whos })
}

/// "YYYY-MM-DD HH:MM:SS" UTC → "YYYY-MM-DDTHH:MM" in the viewer's zone.
fn local(at: &str, tz: chrono_tz::Tz) -> String {
    chrono::NaiveDateTime::parse_from_str(at, "%Y-%m-%d %H:%M:%S")
        .map(|dt| dt.and_utc().with_timezone(&tz).format("%Y-%m-%dT%H:%M").to_string())
        .unwrap_or_else(|_| at.replace(' ', "T").chars().take(16).collect())
}

async fn page(State(state): State<SharedState>, user: Principal, ctx: PageCtx) -> AppResult<HistoryPage> {
    let d = data(&state, &user, &ctx.lang).await?;
    Ok(HistoryPage { data_json: crate::render::script_json(d), ctx })
}

async fn api_data(State(state): State<SharedState>, user: Principal, ctx: PageCtx) -> AppResult<Response> {
    Ok(([(axum::http::header::CONTENT_TYPE, "application/json")], data(&state, &user, &ctx.lang).await?).into_response())
}

/// What undoing a change would do: warnings (overwritten values, later changes going inactive) and
/// the parts it has to skip, values readable.
fn plan_json(state: &SharedState, lang: &str, tz: chrono_tz::Tz, colls: &HashMap<String, String>, p: &Plan) -> Value {
    let t = |k: &str| state.i18n.lookup(lang, k);
    let show = Show { t: &t, lang, tz };
    let name = |k: &str| colls.get(k).cloned().unwrap_or_else(|| k.to_string());
    json!({
        "ok": p.ok,
        "why": p.why,
        "warns": p.warns.iter().map(|w| json!({
            "kind": w.kind, "title": w.title, "by": w.by, "ids": w.ids,
            "label": w.key.as_deref().map(|k| show.label(k)),
            "cur": w.key.as_deref().zip(w.cur.as_deref()).map(|(k, v)| show.value_in(k, v, &name, &[w.title.as_str()])),
            "old": w.key.as_deref().zip(w.old.as_deref()).map(|(k, v)| show.value_in(k, v, &name, &[w.title.as_str()])),
        })).collect::<Vec<_>>(),
        "skips": p.skips,
        "now": p.now.iter().map(|(el, k, v, title)| json!([el, k, if k.is_empty() { String::new() } else { show.value_in(k, v, &name, &[title.as_str()]) }])).collect::<Vec<_>>(),
    })
}

async fn coll_names(state: &SharedState, user: &Principal) -> AppResult<HashMap<String, String>> {
    let mut m = HashMap::new();
    for c in collections::calendars_for(state, user, false).await? {
        m.insert(format!("{}/{}", c.principal, c.id), c.displayname);
    }
    Ok(m)
}

async fn api_plan(State(state): State<SharedState>, user: Principal, PageView(ctx, vc): PageView, Path(id): Path<i64>) -> AppResult<Json<Value>> {
    let all = history::visible(&state, &user).await?;
    let c = all.iter().find(|x| x.id == id).ok_or(AppError::NotFound)?;
    let p = history::plan(&state, &user, c, &all, &mut history::PlanMemo::default()).await?;
    Ok(Json(plan_json(&state, &ctx.lang, vc.tz, &coll_names(&state, &user).await?, &p)))
}

#[derive(serde::Deserialize, Default)]
struct UndoBody {
    /// the warnings were shown and accepted
    #[serde(default)]
    confirm: bool,
}

/// Undo: `{id}` of the new change, or 409 with the plan when it warns and was not confirmed
/// (or cannot be done at all: then `ok` is false).
async fn api_undo(State(state): State<SharedState>, user: Principal, PageView(ctx, vc): PageView, Path(id): Path<i64>, _: csrf::Token, Json(b): Json<UndoBody>) -> AppResult<Response> {
    match history::undo(&state, &user, id, b.confirm).await? {
        Ok(n) => Ok(Json(json!({ "id": n })).into_response()),
        Err(p) => Ok((StatusCode::CONFLICT, Json(plan_json(&state, &ctx.lang, vc.tz, &coll_names(&state, &user).await?, &p))).into_response()),
    }
}

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/history", get(page))
        .route("/api/history", get(api_data))
        .route("/api/history/{id}/plan", get(api_plan))
        .route("/api/history/{id}/undo", post(api_undo))
}

#[cfg(test)]
mod pinned;
