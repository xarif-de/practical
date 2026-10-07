//! /api/push: VAPID key, subscriptions (list, subscribe, unsubscribe).

use super::*;

#[derive(serde::Serialize)]
pub(super) struct VapidKey {
    pub(super) key: Option<String>,
}

pub(super) async fn push_key(State(state): State<SharedState>) -> Json<VapidKey> {
    Json(VapidKey { key: state.vapid.as_ref().map(|k| k.public().encode_b64().0) })
}

#[derive(Deserialize)]
pub(super) struct SubKeys {
    p256dh: String,
    pub(super) auth: String,
}
#[derive(Deserialize)]
pub(super) struct SubscribeBody {
    endpoint: String,
    pub(super) keys: SubKeys,
    #[serde(default)]
    user_agent: Option<String>,
}

pub(super) async fn push_subscribe(State(state): State<SharedState>, user: Principal, session: Session, headers: HeaderMap, _: csrf::TokenOrBasic, Json(b): Json<SubscribeBody>) -> AppResult<Json<Created>> {
    if state.vapid.is_none() {
        return Err(AppError::bad("err-push-disabled"));
    }
    // the server POSTs to the endpoint: only a browser push service's (domain::push, audit3-security #2/#3)
    crate::domain::push::check_subscription(&b.endpoint, &b.keys.p256dh, &b.keys.auth).map_err(AppError::bad)?;
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO practical_push_subscriptions (id, principal, endpoint, p256dh, auth, user_agent) VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT(endpoint) DO UPDATE SET principal=excluded.principal, p256dh=excluded.p256dh, auth=excluded.auth, user_agent=excluded.user_agent")
        .bind(&id)
        .bind(&user.id)
        .bind(&b.endpoint)
        .bind(&b.keys.p256dh)
        .bind(&b.keys.auth)
        .bind(b.user_agent.or_else(|| headers.get(axum::http::header::USER_AGENT).and_then(|v| v.to_str().ok()).map(str::to_string)).map(|u| u.chars().take(300).collect::<String>()))
        .execute(&state.db)
        .await?;
    // a bounded list per user: past the cap the oldest device goes (more than a few are dead browsers)
    sqlx::query("DELETE FROM practical_push_subscriptions WHERE principal = ? AND endpoint <> ? AND id NOT IN (SELECT id FROM practical_push_subscriptions WHERE principal = ? ORDER BY endpoint = ? DESC, created_at DESC, rowid DESC LIMIT ?)")
        .bind(&user.id)
        .bind(&b.endpoint)
        .bind(&user.id)
        .bind(&b.endpoint)
        .bind(crate::domain::push::MAX_SUBSCRIPTIONS)
        .execute(&state.db)
        .await?;
    crate::domain::push::remember_in_session(&session, &b.endpoint).await;
    Ok(Json(Created { id }))
}

#[derive(Deserialize)]
pub(super) struct UnsubscribeBody {
    endpoint: String,
}

pub(super) async fn push_unsubscribe(State(state): State<SharedState>, user: Principal, _: csrf::TokenOrBasic, Json(b): Json<UnsubscribeBody>) -> AppResult<Json<Created>> {
    sqlx::query("DELETE FROM practical_push_subscriptions WHERE principal = ? AND endpoint = ?").bind(&user.id).bind(&b.endpoint).execute(&state.db).await?;
    Ok(Json(Created { id: String::new() }))
}

#[derive(serde::Serialize)]
pub(super) struct PushSub {
    endpoint: String,
    user_agent: Option<String>,
    created_at: String,
}

pub(super) async fn push_list(State(state): State<SharedState>, user: Principal) -> AppResult<Json<Vec<PushSub>>> {
    use sqlx::Row;
    let rows = sqlx::query("SELECT endpoint, user_agent, created_at FROM practical_push_subscriptions WHERE principal = ? ORDER BY created_at").bind(&user.id).fetch_all(&state.db).await?;
    Ok(Json(rows.iter().map(|r| PushSub { endpoint: r.get("endpoint"), user_agent: r.get("user_agent"), created_at: r.get("created_at") }).collect()))
}
