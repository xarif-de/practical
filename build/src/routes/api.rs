//! JSON endpoints used by app.js: SSE change stream, event/contact/task CRUD,
//! search, push subscriptions. All mutations need the CSRF header (the origin
//! guard already rejected cross-site requests before we get here).

use crate::csrf;
use crate::domain::contacts::{self, Contact, ContactInput};
use crate::domain::events::{self, EventDetail, EventInput};
use crate::domain::geo;
use crate::domain::history::{self, Actor};
use crate::domain::history_text::Store;
use crate::domain::prefs;
use crate::domain::search;
use crate::domain::tasks::{self, TaskDetail, TaskInput};
use crate::error::{AppError, AppResult};
use crate::routes::calendar::view_ctx;
use crate::state::SharedState;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{get, post};
use axum::Json;
use futures_util::StreamExt;
use futures_util::stream::Stream;
use rustical_store::auth::Principal;
use rustical_store::CalendarReadStore;
use serde::Deserialize;
use std::convert::Infallible;
use tower_sessions::Session;

/// Live updates (plan NFR): one SSE stream per browser tab; events are filtered
/// to the collections the user can see (own principal + groups).
async fn changes(State(state): State<SharedState>, user: Principal) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.changes.subscribe();
    let mine: Vec<String> = user.memberships().into_iter().map(str::to_string).collect();
    let stream = tokio_stream_wrapper(rx).filter_map(move |ev| {
        let mine = mine.clone();
        async move {
            match ev {
                Ok(ev) if mine.iter().any(|m| m == &ev.principal) => Some(Ok(Event::default().event("change").json_data(&ev).unwrap_or_else(|_| Event::default()))),
                _ => None,
            }
        }
    });
    // ends when the server stops: an open stream would hold the graceful shutdown until SIGKILL
    Sse::new(stream.take_until(state.guards.stopping())).keep_alive(KeepAlive::default())
}

/// Broadcast receiver as a stream. `unfold` keeps the pending `recv()` future between polls, so its
/// waker survives; a fresh `recv()` per poll would drop the waiter and the stream only moved on the
/// SSE keep-alive tick (15 s) — measured 2026-09-16 with /dev/reload.
fn tokio_stream_wrapper(rx: tokio::sync::broadcast::Receiver<crate::state::ChangeEvent>) -> impl Stream<Item = Result<crate::state::ChangeEvent, ()>> {
    futures_util::stream::unfold(rx, |mut rx| async move {
        match rx.recv().await {
            Ok(v) => Some((Ok(v), rx)),
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => Some((Err(()), rx)),
            Err(tokio::sync::broadcast::error::RecvError::Closed) => None,
        }
    })
}

/// The handlers taking this have a principal; with a Basic header and no session user it came from a valid
/// app token (`csrf::token_authenticated`).
async fn check_csrf(session: &Session, headers: &HeaderMap) -> AppResult<()> {
    let has_basic = headers.get(axum::http::header::AUTHORIZATION).and_then(|v| v.to_str().ok()).is_some_and(|v| v.starts_with("Basic "));
    if has_basic && session.get::<String>(crate::ctx::SESSION_USER).await?.is_none() {
        return Ok(());
    }
    csrf::check(session, &csrf::header_token(headers)).await
}

#[derive(serde::Serialize)]
pub struct Created {
    pub id: String,
}

// ---- events ----

pub async fn create_event(State(state): State<SharedState>, user: Principal, ctx: crate::ctx::PageCtx, session: Session, headers: HeaderMap, Json(input): Json<EventInput>) -> AppResult<Json<Created>> {
    check_csrf(&session, &headers).await?;
    let vc = view_ctx(&state, &user, &ctx.lang).await?;
    let (p, c) = (input.principal.clone(), input.cal_id.clone());
    let mut g = history::begin(&state, Actor::ui(&user)).await;
    let id = events::create_event(&state, &user, input, vc.tz).await?;
    g.object(Store::Cal, &p, &c, &id);
    g.commit().await;
    Ok(Json(Created { id }))
}

async fn get_event(State(state): State<SharedState>, user: Principal, ctx: crate::ctx::PageCtx, Path((p, c, id)): Path<(String, String, String)>) -> AppResult<Json<EventDetail>> {
    let vc = view_ctx(&state, &user, &ctx.lang).await?;
    let mut d = events::get_event(&state, &user, &p, &c, &id, vc.tz).await?;
    events::resolve_related(&state, &user, &mut d.related, vc.tz).await?;
    if d.organizer.is_some() || !d.attendees.is_empty() {
        let abs = crate::domain::collections::addressbooks_for(&state, &user, false).await?;
        let all = contacts::contacts_for(&state, &abs).await?;
        for person in d.organizer.iter_mut().chain(d.attendees.iter_mut()) {
            person.contacts = contacts::matches(&all, &person.name, &person.email, &person.cutype);
        }
    }
    Ok(Json(d))
}

// ---- category rules (per user) and the category index ----

#[derive(serde::Serialize)]
pub struct CategoryInfo { pub name: String, pub count: usize, pub rule: bool }

#[derive(serde::Deserialize, Default)]
pub struct ScopeQ {
    /// "event" (the default) or "task" — category rules are kept apart per component
    #[serde(default)]
    pub scope: Option<String>,
}
fn scope_of(q: &ScopeQ) -> &str {
    match q.scope.as_deref() {
        Some("task") => "task",
        _ => "event",
    }
}

/// Every category of the visible calendars with its count, plus rule-only categories (count 0).
async fn categories(State(state): State<SharedState>, user: Principal, ctx: crate::ctx::PageCtx, Query(q): Query<ScopeQ>) -> AppResult<Json<Vec<CategoryInfo>>> {
    let vc = view_ctx(&state, &user, &ctx.lang).await?;
    let rules = crate::domain::catrules::all(&state.db, &user.id, scope_of(&q)).await?;
    let mut out: Vec<CategoryInfo> = events::category_counts(&state, &user, scope_of(&q), vc.tz).await?.into_iter().map(|(name, count)| CategoryInfo { rule: rules.contains_key(&name), name, count }).collect();
    for name in rules.keys() {
        if !out.iter().any(|c| &c.name == name) {
            out.push(CategoryInfo { name: name.clone(), count: 0, rule: true });
        }
    }
    Ok(Json(out))
}

/// Event titles with their counts: [[title, n], …], most used first.
async fn titles(State(state): State<SharedState>, user: Principal) -> AppResult<Json<Vec<(String, usize)>>> {
    Ok(Json(events::title_counts(&state, &user).await?))
}

/// The colours in use, for the colour picker and the settings page "Farben".
async fn colors_get(State(state): State<SharedState>, user: Principal) -> AppResult<Json<Vec<crate::domain::colors::Source>>> {
    Ok(Json(crate::domain::colors::sources(&state, &user).await?))
}

#[derive(Deserialize)]
struct ColorSet {
    color: String,
}

#[derive(Deserialize)]
struct CategoryRecolor {
    scope: String,
    name: String,
    from: String,
    to: String,
    /// false: only count
    #[serde(default)]
    apply: bool,
}

#[derive(serde::Serialize)]
struct Count {
    count: usize,
}

/// A category's colour changed from `from` to `to`: how many events/tasks with that category still carry
/// `from`, and with `apply` give them `to`.
async fn color_category(State(state): State<SharedState>, user: Principal, session: Session, headers: HeaderMap, Json(c): Json<CategoryRecolor>) -> AppResult<Json<Count>> {
    use crate::domain::colors;
    check_csrf(&session, &headers).await?;
    let scope = if c.scope == "task" { "task" } else { "event" };
    let keys = colors::category_objects(&state, &user, scope, &c.name, &c.from).await?;
    if c.apply {
        let to = c.to.trim().to_lowercase();
        // all of them are one change in the history
        let mut g = history::begin(&state, Actor { label: Some(format!("colors:{}", c.name)), ..Actor::ui(&user) }).await;
        for k in &keys {
            let [p, cal, id] = k.splitn(3, '/').collect::<Vec<_>>()[..] else { continue };
            g.object(Store::Cal, p, cal, id);
            colors::set_object_color(&state, &user, p, cal, id, &to).await?;
        }
        g.commit().await;
    }
    Ok(Json(Count { count: keys.len() }))
}

#[derive(Deserialize)]
struct ColorBatch {
    changes: Vec<crate::domain::colors::Change>,
}

#[derive(serde::Serialize)]
struct BatchDone {
    count: usize,
    failed: usize,
}

/// The colour optimiser's result in one go: every change it lists; a failing one is counted, not fatal.
async fn colors_batch(State(state): State<SharedState>, user: Principal, session: Session, headers: HeaderMap, Json(b): Json<ColorBatch>) -> AppResult<Json<BatchDone>> {
    check_csrf(&session, &headers).await?;
    if b.changes.len() > 5000 {
        return Err(AppError::bad("err-too-many-changes"));
    }
    let mut g = history::begin(&state, Actor { label: Some("colors:".into()), ..Actor::ui(&user) }).await;
    for ch in &b.changes {
        if let ("event" | "task", [p, cal, id]) = (ch.kind.as_str(), ch.key.split('/').collect::<Vec<_>>().as_slice()) {
            g.object(Store::Cal, p, cal, id);
        }
    }
    let (count, failed) = crate::domain::colors::apply(&state, &user, &b.changes).await?;
    g.commit().await;
    Ok(Json(BatchDone { count, failed }))
}

/// One element's colour: calendar and addressbook `{principal}/{id}`, event and task
/// `{principal}/{calendar}/{object}` ("" = the calendar's again).
async fn color_put(State(state): State<SharedState>, user: Principal, session: Session, headers: HeaderMap, Path((kind, key)): Path<(String, String)>, Json(c): Json<ColorSet>) -> AppResult<axum::http::StatusCode> {
    use crate::domain::colors;
    check_csrf(&session, &headers).await?;
    let parts: Vec<&str> = key.split('/').collect();
    let color = c.color.trim().to_lowercase();
    match (kind.as_str(), parts.as_slice()) {
        ("calendar", [p, id]) => colors::set_calendar_color(&state, &user, p, id, &color).await?,
        ("addressbook", [p, id]) => crate::domain::collections::set_addressbook_color_checked(&state, &user, p, id, &color).await?,
        ("event" | "task", [p, cal, id]) => {
            let mut g = history::begin(&state, Actor::ui(&user)).await;
            g.object(Store::Cal, p, cal, id);
            colors::set_object_color(&state, &user, p, cal, id, &color).await?;
            g.commit().await;
        }
        _ => return Err(AppError::bad("err-colour-target")),
    }
    Ok(axum::http::StatusCode::NO_CONTENT)
}

async fn category_rules_get(State(state): State<SharedState>, user: Principal, Query(q): Query<ScopeQ>) -> AppResult<Json<std::collections::BTreeMap<String, crate::domain::catrules::Rule>>> {
    Ok(Json(crate::domain::catrules::all(&state.db, &user.id, scope_of(&q)).await?))
}

async fn category_rule_put(State(state): State<SharedState>, user: Principal, session: Session, headers: HeaderMap, Path(name): Path<String>, Query(q): Query<ScopeQ>, Json(rule): Json<crate::domain::catrules::Rule>) -> AppResult<Json<crate::domain::catrules::Rule>> {
    check_csrf(&session, &headers).await?;
    crate::domain::catrules::put(&state.db, &user.id, scope_of(&q), &name, &rule).await?;
    Ok(Json(rule))
}

async fn category_rule_delete(State(state): State<SharedState>, user: Principal, session: Session, headers: HeaderMap, Path(name): Path<String>, Query(q): Query<ScopeQ>) -> AppResult<Json<Created>> {
    check_csrf(&session, &headers).await?;
    crate::domain::catrules::delete(&state.db, &user.id, scope_of(&q), &name).await?;
    Ok(Json(Created { id: name }))
}

#[derive(serde::Serialize)]
pub struct Mentions {
    pub contacts: Vec<MentionContact>,
    pub categories: Vec<MentionCategory>,
    pub events: Vec<MentionEvent>,
    pub tasks: Vec<MentionEvent>,
}
#[derive(serde::Serialize)]
pub struct MentionContact { pub name: String, pub email: String, pub key: String }
#[derive(serde::Serialize)]
pub struct MentionCategory { pub name: String, pub count: usize }
#[derive(serde::Serialize, Clone)]
pub struct MentionEvent { pub summary: String, pub key: String, pub uid: String, pub date: Option<chrono::NaiveDate> }

/// The note editor's event list per user: a 15-month expansion of every visible calendar, the bulk of
/// /api/mentions. Valid while the zone, the day and every calendar's synctoken and VEVENT flag are those
/// it was built from (MentionEvent carries nothing else of a calendar). One list per user.
#[derive(Default)]
pub struct MentionCache(std::sync::Mutex<std::collections::HashMap<String, (MentionKey, std::sync::Arc<Vec<MentionEvent>>)>>);

type MentionKey = (chrono_tz::Tz, chrono::NaiveDate, Vec<(String, String, i64, bool)>);

async fn mention_events(state: &SharedState, user: &Principal, cals: &[crate::domain::collections::CalInfo], vc: &crate::routes::calendar::ViewCtx) -> AppResult<std::sync::Arc<Vec<MentionEvent>>> {
    let key: MentionKey = (vc.tz, vc.today, cals.iter().map(|c| (c.principal.clone(), c.id.clone(), c.synctoken, c.components.iter().any(|x| x == "VEVENT"))).collect());
    if let Some((k, v)) = state.caches.mentions.0.lock().unwrap().get(&user.id)
        && *k == key
    {
        return Ok(v.clone());
    }
    let mut seen = std::collections::HashSet::new();
    let events: Vec<MentionEvent> = events::instances(state, cals, vc.today - chrono::Duration::days(90), vc.today + chrono::Duration::days(365), vc.tz).await?.into_iter().filter(|i| seen.insert(i.uid.clone())).take(500).map(|i| MentionEvent { summary: i.summary.clone(), key: i.key(), uid: i.uid.clone(), date: Some(i.start.date()) }).collect();
    // collected in place from the instances' buffer (std's in-place iteration): without the shrink the
    // cache would keep the whole 15-month expansion's allocation (measured +9.5 MiB live heap)
    let mut events = events;
    events.shrink_to_fit();
    let events = std::sync::Arc::new(events);
    state.caches.mentions.0.lock().unwrap().insert(user.id.clone(), (key, events.clone()));
    Ok(events)
}

/// Index for the note editor's suggestions and detection: contacts, categories with counts,
/// events (90 days back, a year ahead, one per UID, at most 500) and open tasks. Loaded on the
/// first keystroke in a note and cached in the page.
async fn mentions(State(state): State<SharedState>, user: Principal, ctx: crate::ctx::PageCtx, Query(q): Query<ScopeQ>) -> AppResult<Json<Mentions>> {
    let vc = view_ctx(&state, &user, &ctx.lang).await?;
    let abs = crate::domain::collections::addressbooks_for(&state, &user, false).await?;
    let contacts = contacts::contacts_for(&state, &abs).await?.into_iter().map(|c| MentionContact { name: c.full_name.clone(), email: c.primary_email(), key: format!("{}/{}/{}", c.principal, c.ab_id, c.object_id) }).filter(|c| !c.name.trim().is_empty()).collect();
    let categories = events::category_counts(&state, &user, scope_of(&q), vc.tz).await?.into_iter().map(|(name, count)| MentionCategory { name, count }).collect();
    let cals = events::visible_calendars(&state, &user, None).await?;
    let events = mention_events(&state, &user, &cals, &vc).await?.as_ref().clone();
    let tasks = tasks::tasks_for(&state, &cals, vc.tz).await?.into_iter().filter(|t| t.open()).map(|t| MentionEvent { summary: t.summary.clone(), key: t.key(), uid: t.uid.clone(), date: t.due.map(|d| d.date()) }).collect();
    Ok(Json(Mentions { contacts, categories, events, tasks }))
}

/// The n-th embedded (base64) ATTACH of an event as a download.
async fn event_attachment(State(state): State<SharedState>, user: Principal, Path((p, c, id, n)): Path<(String, String, String, usize)>) -> AppResult<axum::response::Response> {
    use axum::response::IntoResponse;
    crate::domain::collections::ensure_owner(&user, &p)?;
    let obj = state.calendars.get_object(&p, &c, &id, false).await?;
    let (raw, _) = events::split_ics(&obj.get_ics());
    let (fmt, name, bytes) = events::embedded_attachment(&raw.props, n).ok_or(AppError::NotFound)?;
    let safe: String = name.chars().filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_' | ' ')).collect();
    Ok(([(axum::http::header::CONTENT_TYPE, fmt), (axum::http::header::CONTENT_DISPOSITION, format!("attachment; filename=\"{}\"", if safe.trim().is_empty() { format!("anhang-{}", n + 1) } else { safe })), (axum::http::header::CACHE_CONTROL, "private, max-age=0".into())], bytes).into_response())
}

#[derive(Deserialize)]
struct EventUpdate {
    #[serde(flatten)]
    input: EventInput,
    #[serde(default)]
    etag: Option<String>,
}

async fn update_event(State(state): State<SharedState>, user: Principal, ctx: crate::ctx::PageCtx, session: Session, headers: HeaderMap, Path((p, c, id)): Path<(String, String, String)>, Json(u): Json<EventUpdate>) -> AppResult<Json<Created>> {
    check_csrf(&session, &headers).await?;
    let vc = view_ctx(&state, &user, &ctx.lang).await?;
    let mut g = history::begin(&state, Actor::ui(&user)).await;
    g.object(Store::Cal, &p, &c, &id);
    if u.input.principal != p || u.input.cal_id != c {
        // moving to another calendar = create there, delete here (one change: a move)
        let (tp, tc) = (u.input.principal.clone(), u.input.cal_id.clone());
        let target = u.input.clone();
        let uid = state.calendars.get_object(&p, &c, &id, false).await?.get_uid().to_string();
        let new_id = events::create_event_as(&state, &user, target, vc.tz, Some(&uid)).await?;
        g.object(Store::Cal, &tp, &tc, &new_id);
        events::delete_event(&state, &user, &p, &c, &id).await?;
        g.commit().await;
        return Ok(Json(Created { id: new_id }));
    }
    events::update_event(&state, &user, &id, u.input, u.etag.as_deref(), vc.tz).await?;
    g.commit().await;
    Ok(Json(Created { id }))
}

async fn delete_event(State(state): State<SharedState>, user: Principal, session: Session, headers: HeaderMap, Path((p, c, id)): Path<(String, String, String)>) -> AppResult<Json<Created>> {
    check_csrf(&session, &headers).await?;
    let mut g = history::begin(&state, Actor::ui(&user)).await;
    g.object(Store::Cal, &p, &c, &id);
    events::delete_event(&state, &user, &p, &c, &id).await?;
    g.commit().await;
    Ok(Json(Created { id }))
}

// ---- contacts ----

async fn get_contact(State(state): State<SharedState>, user: Principal, Path((p, a, id)): Path<(String, String, String)>) -> AppResult<Json<Contact>> {
    Ok(Json(contacts::get_contact(&state, &user, &p, &a, &id).await?))
}

async fn contact_vcf(State(state): State<SharedState>, user: Principal, Path((p, a, id)): Path<(String, String, String)>) -> AppResult<axum::response::Response> {
    use axum::response::IntoResponse;
    let vcf = contacts::get_vcf(&state, &user, &p, &a, &id).await?;
    Ok(([(axum::http::header::CONTENT_TYPE, "text/vcard; charset=utf-8".to_string()), (axum::http::header::CONTENT_DISPOSITION, format!("attachment; filename=\"{id}.vcf\""))], vcf).into_response())
}

async fn create_contact(State(state): State<SharedState>, user: Principal, session: Session, headers: HeaderMap, Json(input): Json<ContactInput>) -> AppResult<Json<Created>> {
    check_csrf(&session, &headers).await?;
    let (p, a) = (input.principal.clone(), input.ab_id.clone());
    let mut g = history::begin(&state, Actor::ui(&user)).await;
    let id = contacts::create_contact(&state, &user, input).await?;
    g.object(Store::Ab, &p, &a, &id);
    g.commit().await;
    Ok(Json(Created { id }))
}

#[derive(Deserialize)]
struct ContactUpdate {
    #[serde(flatten)]
    input: ContactInput,
    #[serde(default)]
    etag: Option<String>,
}

async fn update_contact(State(state): State<SharedState>, user: Principal, session: Session, headers: HeaderMap, Path((p, a, id)): Path<(String, String, String)>, Json(u): Json<ContactUpdate>) -> AppResult<Json<Created>> {
    check_csrf(&session, &headers).await?;
    let mut g = history::begin(&state, Actor::ui(&user)).await;
    g.object(Store::Ab, &p, &a, &id);
    if u.input.principal != p || u.input.ab_id != a {
        let (tp, ta) = (u.input.principal.clone(), u.input.ab_id.clone());
        let (old, _) = contacts::stored_card(&state, &p, &a, &id).await?;
        let uid = contacts::parse_vcf(&old)?.get_uid().unwrap_or(&id).to_string();
        let new_id = contacts::create_contact_as(&state, &user, u.input, Some(&uid)).await?;
        g.object(Store::Ab, &tp, &ta, &new_id);
        contacts::delete_contact(&state, &user, &p, &a, &id).await?;
        g.commit().await;
        return Ok(Json(Created { id: new_id }));
    }
    contacts::update_contact(&state, &user, &id, u.input, u.etag.as_deref()).await?;
    g.commit().await;
    Ok(Json(Created { id }))
}

async fn delete_contact(State(state): State<SharedState>, user: Principal, session: Session, headers: HeaderMap, Path((p, a, id)): Path<(String, String, String)>) -> AppResult<Json<Created>> {
    check_csrf(&session, &headers).await?;
    let mut g = history::begin(&state, Actor::ui(&user)).await;
    g.object(Store::Ab, &p, &a, &id);
    contacts::delete_contact(&state, &user, &p, &a, &id).await?;
    g.commit().await;
    Ok(Json(Created { id }))
}

// ---- tasks ----

async fn create_task(State(state): State<SharedState>, user: Principal, ctx: crate::ctx::PageCtx, session: Session, headers: HeaderMap, Json(input): Json<TaskInput>) -> AppResult<Json<Created>> {
    check_csrf(&session, &headers).await?;
    let vc = view_ctx(&state, &user, &ctx.lang).await?;
    let (p, c) = (input.principal.clone(), input.cal_id.clone());
    let mut g = history::begin(&state, Actor::ui(&user)).await;
    let id = tasks::create_task(&state, &user, input, vc.tz).await?;
    g.object(Store::Cal, &p, &c, &id);
    g.commit().await;
    Ok(Json(Created { id }))
}

async fn get_task(State(state): State<SharedState>, user: Principal, ctx: crate::ctx::PageCtx, Path((p, c, id)): Path<(String, String, String)>) -> AppResult<Json<TaskDetail>> {
    let vc = view_ctx(&state, &user, &ctx.lang).await?;
    let mut d = tasks::get_task(&state, &user, &p, &c, &id, vc.tz).await?;
    events::resolve_related(&state, &user, &mut d.related, vc.tz).await?;
    Ok(Json(d))
}

#[derive(Deserialize)]
struct TaskUpdate {
    #[serde(flatten)]
    input: TaskInput,
    /// Sent by the tick boxes instead of a full input; `None` means "take the input as it is"
    #[serde(default)]
    completed: Option<bool>,
    #[serde(default)]
    etag: Option<String>,
}

async fn update_task(State(state): State<SharedState>, user: Principal, ctx: crate::ctx::PageCtx, session: Session, headers: HeaderMap, Path((p, c, id)): Path<(String, String, String)>, Json(u): Json<TaskUpdate>) -> AppResult<Json<Created>> {
    check_csrf(&session, &headers).await?;
    let vc = view_ctx(&state, &user, &ctx.lang).await?;
    let mut g = history::begin(&state, Actor::ui(&user)).await;
    g.object(Store::Cal, &p, &c, &id);
    match u.completed {
        Some(done) => tasks::set_completed(&state, &user, &p, &c, &id, done, vc.tz).await?,
        None => {
            if u.input.principal != p || u.input.cal_id != c {
                return Err(AppError::bad("err-task-calendar"));
            }
            tasks::update_task(&state, &user, &id, u.input, u.etag.as_deref(), vc.tz).await?
        }
    }
    g.commit().await;
    Ok(Json(Created { id }))
}

async fn delete_task(State(state): State<SharedState>, user: Principal, session: Session, headers: HeaderMap, Path((p, c, id)): Path<(String, String, String)>) -> AppResult<Json<Created>> {
    check_csrf(&session, &headers).await?;
    let mut g = history::begin(&state, Actor::ui(&user)).await;
    g.object(Store::Cal, &p, &c, &id);
    tasks::delete_task(&state, &user, &p, &c, &id).await?;
    g.commit().await;
    Ok(Json(Created { id }))
}

// ---- search ----

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
    /// "1": the "all results" dialog (200 per category) instead of the drop-down's 5
    #[serde(default)]
    all: Option<String>,
}

async fn api_search(State(state): State<SharedState>, user: Principal, ctx: crate::ctx::PageCtx, Query(q): Query<SearchQuery>) -> AppResult<Json<search::SearchResult>> {
    let vc = view_ctx(&state, &user, &ctx.lang).await?;
    let limit = if q.all.as_deref() == Some("1") { 200 } else { 5 };
    Ok(Json(search::search(&state, &user, &q.q, vc.today, vc.tz, &ctx.lang, limit).await?))
}

#[derive(Deserialize)]
struct SuggestQuery {
    q: String,
}

/// The event editor's title suggestions for a new event (`search::suggest`): three past events.
async fn event_suggest(State(state): State<SharedState>, user: Principal, ctx: crate::ctx::PageCtx, Query(q): Query<SuggestQuery>) -> AppResult<Json<Vec<search::Suggestion>>> {
    let vc = view_ctx(&state, &user, &ctx.lang).await?;
    let now = crate::clock::now().with_timezone(&vc.tz).naive_local();
    Ok(Json(search::suggest(&state, &user, &q.q, now, vc.tz, &ctx.lang, 3).await?))
}

// ---- search history ----

async fn history_get(State(state): State<SharedState>, user: Principal) -> AppResult<Json<Vec<crate::domain::search_history::Entry>>> {
    let days = prefs::get(&state.db, &user.id).await?.history_days();
    Ok(Json(crate::domain::search_history::list(&state.db, &user.id, days).await?))
}

#[derive(Deserialize)]
struct HistoryIn {
    q: String,
    #[serde(default)]
    target: Option<serde_json::Value>,
}

async fn history_post(State(state): State<SharedState>, user: Principal, session: Session, headers: HeaderMap, Json(b): Json<HistoryIn>) -> AppResult<Json<bool>> {
    check_csrf(&session, &headers).await?;
    if b.q.chars().count() > 200 || b.target.as_ref().is_some_and(|t| t.to_string().len() > 2000) {
        return Err(AppError::bad("err-too-long"));
    }
    let days = prefs::get(&state.db, &user.id).await?.history_days();
    crate::domain::search_history::record(&state.db, &user.id, &b.q, b.target.as_ref(), days).await?;
    Ok(Json(days != 0))
}

async fn history_clear(State(state): State<SharedState>, user: Principal, session: Session, headers: HeaderMap) -> AppResult<Json<bool>> {
    check_csrf(&session, &headers).await?;
    crate::domain::search_history::clear(&state.db, &user.id).await?;
    Ok(Json(true))
}

async fn history_delete(State(state): State<SharedState>, user: Principal, session: Session, headers: HeaderMap, Path(id): Path<i64>) -> AppResult<Json<bool>> {
    check_csrf(&session, &headers).await?;
    crate::domain::search_history::remove(&state.db, &user.id, id).await?;
    Ok(Json(true))
}

// ---- push (plan D12) ----

#[derive(serde::Serialize)]
struct VapidKey {
    key: Option<String>,
}

async fn push_key(State(state): State<SharedState>) -> Json<VapidKey> {
    Json(VapidKey { key: state.vapid.as_ref().map(|k| k.public().encode_b64().0) })
}

#[derive(Deserialize)]
struct SubKeys {
    p256dh: String,
    auth: String,
}
#[derive(Deserialize)]
struct SubscribeBody {
    endpoint: String,
    keys: SubKeys,
    #[serde(default)]
    user_agent: Option<String>,
}

async fn push_subscribe(State(state): State<SharedState>, user: Principal, session: Session, headers: HeaderMap, Json(b): Json<SubscribeBody>) -> AppResult<Json<Created>> {
    check_csrf(&session, &headers).await?;
    if state.vapid.is_none() {
        return Err(AppError::bad("err-push-disabled"));
    }
    if !b.endpoint.starts_with("https://") {
        return Err(AppError::bad("err-push-https"));
    }
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO practical_push_subscriptions (id, principal, endpoint, p256dh, auth, user_agent) VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT(endpoint) DO UPDATE SET principal=excluded.principal, p256dh=excluded.p256dh, auth=excluded.auth, user_agent=excluded.user_agent")
        .bind(&id)
        .bind(&user.id)
        .bind(&b.endpoint)
        .bind(&b.keys.p256dh)
        .bind(&b.keys.auth)
        .bind(b.user_agent.or_else(|| headers.get(axum::http::header::USER_AGENT).and_then(|v| v.to_str().ok()).map(str::to_string)))
        .execute(&state.db)
        .await?;
    Ok(Json(Created { id }))
}

#[derive(Deserialize)]
struct UnsubscribeBody {
    endpoint: String,
}

async fn push_unsubscribe(State(state): State<SharedState>, user: Principal, session: Session, headers: HeaderMap, Json(b): Json<UnsubscribeBody>) -> AppResult<Json<Created>> {
    check_csrf(&session, &headers).await?;
    sqlx::query("DELETE FROM practical_push_subscriptions WHERE principal = ? AND endpoint = ?").bind(&user.id).bind(&b.endpoint).execute(&state.db).await?;
    Ok(Json(Created { id: String::new() }))
}

#[derive(serde::Serialize)]
struct PushSub {
    endpoint: String,
    user_agent: Option<String>,
    created_at: String,
}

async fn push_list(State(state): State<SharedState>, user: Principal) -> AppResult<Json<Vec<PushSub>>> {
    use sqlx::Row;
    let rows = sqlx::query("SELECT endpoint, user_agent, created_at FROM practical_push_subscriptions WHERE principal = ? ORDER BY created_at").bind(&user.id).fetch_all(&state.db).await?;
    Ok(Json(rows.iter().map(|r| PushSub { endpoint: r.get("endpoint"), user_agent: r.get("user_agent"), created_at: r.get("created_at") }).collect()))
}

#[derive(Deserialize)]
struct GeoSearch {
    q: String,
    lat: Option<f64>,
    lon: Option<f64>,
}

#[derive(Deserialize)]
struct GeoReverse {
    lat: f64,
    lon: f64,
}

/// Address suggestions (Photon) — the browser asks only after a typing pause.
async fn geo_search(State(state): State<SharedState>, user: Principal, ctx: crate::ctx::PageCtx, Query(q): Query<GeoSearch>) -> AppResult<Json<Vec<geo::Place>>> {
    let near = match (q.lat, q.lon) { (Some(a), Some(b)) => Some((a, b)), _ => None };
    let home = crate::domain::prefs::get(&state.db, &user.id).await?.home_country(&state.config);
    Ok(Json(geo::search(&state, &q.q, &ctx.lang, near, &home).await?))
}

/// Address at a position (Nominatim) — one click on the map dialog's button.
async fn geo_reverse(State(state): State<SharedState>, user: Principal, ctx: crate::ctx::PageCtx, Query(q): Query<GeoReverse>) -> AppResult<Json<geo::Place>> {
    let home = crate::domain::prefs::get(&state.db, &user.id).await?.home_country(&state.config);
    geo::reverse(&state, q.lat, q.lon, &ctx.lang, &home).await?.map(Json).ok_or(AppError::NotFound)
}

/// The user's reminder presets (event editor); `null` = not customised yet, the editor shows its defaults.
async fn alarm_presets_get(State(state): State<SharedState>, user: Principal) -> AppResult<Json<Option<Vec<events::AlarmPreset>>>> {
    let p = prefs::get(&state.db, &user.id).await?;
    Ok(Json(p.alarm_presets_json.as_deref().and_then(|j| serde_json::from_str(j).ok())))
}

/// Replaces the user's reminder presets (the star in a reminder's settings toggles one).
async fn alarm_presets_put(State(state): State<SharedState>, user: Principal, session: Session, headers: HeaderMap, Json(list): Json<Vec<events::AlarmPreset>>) -> AppResult<Json<Vec<events::AlarmPreset>>> {
    check_csrf(&session, &headers).await?;
    events::validate_presets(&list)?;
    let mut p = prefs::get(&state.db, &user.id).await?;
    p.alarm_presets_json = Some(serde_json::to_string(&list).unwrap());
    prefs::set(&state.db, &user.id, &p).await?;
    Ok(Json(list))
}

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/api/alarm-presets", get(alarm_presets_get).put(alarm_presets_put))
        .route("/api/geo/search", get(geo_search))
        .route("/api/geo/reverse", get(geo_reverse))
        .route("/api/changes", get(changes))
        .route("/api/events/suggest", get(event_suggest))
        .route("/api/events/{p}/{c}/{id}", get(get_event).put(update_event).delete(delete_event))
        .route("/api/events/{p}/{c}/{id}/attach/{n}", get(event_attachment))
        .route("/api/contacts", post(create_contact))
        .route("/api/contacts/{p}/{a}/{id}", get(get_contact).put(update_contact).delete(delete_contact))
        .route("/api/contacts/{p}/{a}/{id}/vcf", get(contact_vcf))
        .route("/api/tasks", post(create_task))
        .route("/api/tasks/{p}/{c}/{id}", get(get_task).put(update_task).delete(delete_task))
        .route("/api/search", get(api_search))
        .route("/api/search/history", get(history_get).post(history_post).delete(history_clear))
        .route("/api/search/history/{id}", axum::routing::delete(history_delete))
        .route("/api/mentions", get(mentions))
        .route("/api/categories", get(categories))
        .route("/api/titles", get(titles))
        .route("/api/colors", get(colors_get))
        .route("/api/colors/category", post(color_category))
        .route("/api/colors/batch", post(colors_batch))
        .route("/api/colors/{kind}/{*key}", axum::routing::put(color_put))
        .route("/api/category-rules", get(category_rules_get))
        .route("/api/category-rules/{name}", axum::routing::put(category_rule_put).delete(category_rule_delete))
        .route("/api/push/key", get(push_key))
        .route("/api/push/subscriptions", get(push_list).post(push_subscribe).delete(push_unsubscribe))
        .layer(axum::extract::DefaultBodyLimit::max(8 * 1024 * 1024)) // embedded files ride in the JSON body as base64
}
