//! /api/events (create, read, update, delete, embedded attachments), /api/mentions, /api/alarm-presets.

use super::*;

pub async fn create_event(State(state): State<SharedState>, user: Principal, PageView(_, vc): PageView, _: csrf::TokenOrBasic, Json(input): Json<EventInput>) -> AppResult<Json<Created>> {
    let (p, c) = (input.principal.clone(), input.cal_id.clone());
    create_object(&state, &user, Store::Cal, &p, &c, events::create_event(&state, &user, input, vc.tz)).await
}

pub(super) async fn get_event(State(state): State<SharedState>, user: Principal, PageView(_, vc): PageView, Path((p, c, id)): Path<(String, String, String)>) -> AppResult<Json<EventDetail>> {
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

pub(super) type MentionKey = (chrono_tz::Tz, chrono::NaiveDate, Vec<(String, String, i64, bool)>);

async fn mention_events(state: &SharedState, user: &Principal, cals: &[crate::domain::collections::CalInfo], vc: &crate::routes::calendar::ViewCtx) -> AppResult<std::sync::Arc<Vec<MentionEvent>>> {
    let key: MentionKey = (vc.tz, vc.today, cals.iter().map(|c| (c.principal.clone(), c.id.clone(), c.synctoken, c.components.iter().any(|x| x == "VEVENT"))).collect());
    if let Some((k, v)) = state.caches.mentions.0.lock().unwrap().get(&user.id)
        && *k == key
    {
        return Ok(v.clone());
    }
    let mut seen = std::collections::HashSet::new();
    let events: Vec<MentionEvent> = events::instances(state, cals, vc.today - chrono::Duration::days(90), vc.today + chrono::Duration::days(365), vc.tz).await?.into_iter().filter(|i| seen.insert(i.uid.clone())).take(500).map(|i| MentionEvent { summary: i.summary.to_string(), key: i.key(), uid: i.uid.clone(), date: Some(i.start.date()) }).collect();
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
pub(super) async fn mentions(State(state): State<SharedState>, user: Principal, PageView(_, vc): PageView, Query(q): Query<ScopeQ>) -> AppResult<Json<Mentions>> {
    let abs = crate::domain::collections::addressbooks_for(&state, &user, false).await?;
    let contacts = contacts::contacts_for(&state, &abs).await?.into_iter().map(|c| MentionContact { name: c.full_name.clone(), email: c.primary_email(), key: format!("{}/{}/{}", c.principal, c.ab_id, c.object_id) }).filter(|c| !c.name.trim().is_empty()).collect();
    let categories = events::category_counts(&state, &user, scope_of(&q), vc.tz).await?.into_iter().map(|(name, count)| MentionCategory { name, count }).collect();
    let cals = events::visible_calendars(&state, &user, None).await?;
    let events = mention_events(&state, &user, &cals, &vc).await?.as_ref().clone();
    let tasks = tasks::tasks_for(&state, &cals, vc.tz).await?.into_iter().filter(|t| t.open()).map(|t| MentionEvent { summary: t.summary.clone(), key: t.key(), uid: t.uid.clone(), date: t.due.map(|d| d.date()) }).collect();
    Ok(Json(Mentions { contacts, categories, events, tasks }))
}

/// The n-th embedded (base64) ATTACH of an event as a download.
pub(super) async fn event_attachment(State(state): State<SharedState>, user: Principal, Path((p, c, id, n)): Path<(String, String, String, usize)>) -> AppResult<axum::response::Response> {
    use axum::response::IntoResponse;
    crate::domain::collections::ensure_owner(&user, &p)?;
    let obj = state.calendars.get_object(&p, &c, &id, false).await?;
    let (raw, _) = events::split_ics(&obj.get_ics());
    let (fmt, name, bytes) = events::embedded_attachment(&raw.props, n).ok_or(AppError::NotFound)?;
    let safe: String = name.chars().filter(|ch| ch.is_alphanumeric() || matches!(ch, '.' | '-' | '_' | ' ')).collect();
    // nosniff + a sandboxing CSP: the bytes are any client's; they are never a script or a page of this origin
    Ok(([(axum::http::header::CONTENT_TYPE, fmt), (axum::http::header::CONTENT_DISPOSITION, crate::routes::attachment(&if safe.trim().is_empty() { format!("anhang-{}", n + 1) } else { safe })), (axum::http::header::CACHE_CONTROL, "private, max-age=0".into()), (axum::http::header::X_CONTENT_TYPE_OPTIONS, "nosniff".into()), (axum::http::header::CONTENT_SECURITY_POLICY, "sandbox; default-src 'none'".into())], bytes).into_response())
}

#[derive(Deserialize)]
pub(super) struct EventUpdate {
    #[serde(flatten)]
    pub(super) input: EventInput,
    #[serde(default)]
    pub(super) etag: Option<String>,
}

pub(super) async fn update_event(State(state): State<SharedState>, user: Principal, PageView(_, vc): PageView, headers: HeaderMap, Path((p, c, id)): Path<(String, String, String)>, _: csrf::TokenOrBasic, Json(u): Json<EventUpdate>) -> AppResult<Json<Created>> {
    let etag = precondition(u.etag.as_deref(), &headers)?;
    let mut g = history::begin(&state, Actor::ui(&user)).await;
    g.object(Store::Cal, &p, &c, &id);
    if u.input.principal != p || u.input.cal_id != c {
        // moving to another calendar = create there, delete here (one change: a move); the source is read
        // only after the owner check, and only in the version the editor showed
        crate::domain::collections::ensure_owner(&user, &p)?;
        let (tp, tc) = (u.input.principal.clone(), u.input.cal_id.clone());
        let target = u.input.clone();
        let old = state.calendars.get_object(&p, &c, &id, false).await?;
        if old.get_etag() != etag {
            return Err(AppError::conflict("err-changed-event"));
        }
        let uid = old.get_uid().to_string();
        let new_id = events::create_event_as(&state, &user, target, vc.tz, Some(&uid)).await?;
        g.object(Store::Cal, &tp, &tc, &new_id);
        crate::domain::collections::delete_object(&state, &user, Store::Cal, &p, &c, &id).await?;
        g.commit().await;
        return Ok(Json(Created { id: new_id }));
    }
    events::update_event(&state, &user, &id, u.input, Some(&etag), vc.tz).await?;
    g.commit().await;
    Ok(Json(Created { id }))
}

#[derive(Deserialize)]
pub(super) struct EventMove {
    /// whole days later (earlier when negative)
    days: i64,
    /// the local start of the one occurrence of a series to move; absent: the whole event
    #[serde(default)]
    at: Option<chrono::NaiveDateTime>,
    #[serde(default)]
    etag: Option<String>,
}

/// The views' drag and drop: the event (or one occurrence of its series) `days` later, its times kept (`events::move_event`).
pub(super) async fn move_event(State(state): State<SharedState>, user: Principal, PageView(_, vc): PageView, Path((p, c, id)): Path<(String, String, String)>, _: csrf::TokenOrBasic, Json(m): Json<EventMove>) -> AppResult<Json<Created>> {
    let mut g = history::begin(&state, Actor::ui(&user)).await;
    g.object(Store::Cal, &p, &c, &id);
    events::move_event(&state, &user, &p, &c, &id, m.days, m.at, m.etag.as_deref(), vc.tz).await?;
    g.commit().await;
    Ok(Json(Created { id }))
}

pub(super) async fn delete_event(State(state): State<SharedState>, user: Principal, Path((p, c, id)): Path<(String, String, String)>, _: csrf::TokenOrBasic) -> AppResult<Json<Created>> {
    delete_object(&state, &user, Store::Cal, &p, &c, id).await
}

/// The user's reminder presets (event editor); `null` = not customised yet, the editor shows its defaults.
pub(super) async fn alarm_presets_get(State(state): State<SharedState>, user: Principal) -> AppResult<Json<Option<Vec<events::AlarmPreset>>>> {
    let p = prefs::get(&state.db, &user.id).await?;
    Ok(Json(p.alarm_presets_json.as_deref().and_then(|j| serde_json::from_str(j).ok())))
}

/// Replaces the user's reminder presets (the star in a reminder's settings toggles one).
pub(super) async fn alarm_presets_put(State(state): State<SharedState>, user: Principal, _: csrf::TokenOrBasic, Json(list): Json<Vec<events::AlarmPreset>>) -> AppResult<Json<Vec<events::AlarmPreset>>> {
    events::validate_presets(&list)?;
    let _lock = state.guards.prefs(&user.id).await; // until prefs::set: no lost update
    let mut p = prefs::get(&state.db, &user.id).await?;
    p.alarm_presets_json = Some(serde_json::to_string(&list).unwrap());
    prefs::set(&state.db, &user.id, &p).await?;
    Ok(Json(list))
}
