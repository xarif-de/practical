//! Browser push reminders (plan D12): every minute, look one hour ahead for
//! VALARM triggers of the subscribers' events, send once, remember in
//! `xui_alarms_sent` so a restart never repeats a reminder.

use crate::domain::collections;
use crate::domain::events;
use crate::domain::prefs;
use crate::state::{AppState, SharedState};
use chrono::{Duration, NaiveDate, Utc};
use rustical_store::auth::AuthenticationProvider;
use sqlx::Row;
use std::time::Duration as StdDuration;

struct Sub {
    endpoint: String,
    p256dh: String,
    auth: String,
}

async fn subscriptions(state: &AppState) -> Result<Vec<(String, Vec<Sub>)>, sqlx::Error> {
    let rows = sqlx::query("SELECT principal, endpoint, p256dh, auth FROM xui_push_subscriptions ORDER BY principal").fetch_all(&state.db).await?;
    let mut out: Vec<(String, Vec<Sub>)> = vec![];
    for r in rows {
        let p: String = r.get("principal");
        let s = Sub { endpoint: r.get("endpoint"), p256dh: r.get("p256dh"), auth: r.get("auth") };
        match out.last_mut() {
            Some((lp, v)) if *lp == p => v.push(s),
            _ => out.push((p, vec![s])),
        }
    }
    Ok(out)
}

pub async fn send(state: &AppState, sub: &Sub, payload: &str) -> Result<(), String> {
    let Some(vapid) = &state.vapid else { return Err("push disabled".into()) };
    let info = web_push::SubscriptionInfo::new(&sub.endpoint, &sub.p256dh, &sub.auth);
    let mut claims = web_push::Claims::new();
    claims.sub = Some(state.config.vapid_subject.clone());
    let sig = web_push::VapidSignatureBuilder::from_ec(vapid.0.clone(), &info).build(claims).map_err(|e| e.to_string())?;
    let msg = web_push::WebPushMessageBuilder::new(&info).payload(web_push::ContentEncoding::Aes128Gcm, payload.as_bytes()).vapid_signature(sig).ttl(3600).build().map_err(|e| e.to_string())?;
    let client = web_push::ReqwestWebPushClient::from_client(state.http.clone());
    web_push::WebPushClient::send(&client, msg).await.map_err(|e| e.to_string())
}

async fn tick(state: &SharedState) -> Result<usize, String> {
    let subs = subscriptions(state).await.map_err(|e| e.to_string())?;
    let now = Utc::now();
    let mut sent = 0;
    for (principal, subs) in subs {
        let Some(user) = state.principals.get_principal(&principal).await.map_err(|e| e.to_string())? else { continue };
        let p = prefs::get(&state.db, &principal).await.map_err(|e| e.to_string())?;
        let tz: chrono_tz::Tz = p.timezone.as_deref().and_then(|t| t.parse().ok()).unwrap_or_else(|| state.config.default_timezone.parse().unwrap_or(chrono_tz::Europe::Berlin));
        let today: NaiveDate = now.with_timezone(&tz).date_naive();
        let cals = collections::calendars_for(state, &user, false).await.map_err(|e| e.to_string())?;
        // alarms up to 2 days ahead can only fire within the next hour if the event is within ~3 days
        let inst = events::instances(state, &cals, today - Duration::days(1), today + Duration::days(4), tz).await.map_err(|e| e.to_string())?;
        for i in inst {
            let Some(m) = i.alarm_minutes else { continue };
            let start_utc = crate::domain::ical::to_utc(i.start, tz);
            let trigger = start_utc - Duration::minutes(m);
            // fire window: trigger within [now-2min, now+1min] (the poll runs every minute)
            if trigger < now - Duration::minutes(2) || trigger > now + Duration::minutes(1) {
                continue;
            }
            let key_trigger = trigger.to_rfc3339();
            let already: i64 = sqlx::query("SELECT count(*) AS n FROM xui_alarms_sent WHERE principal = ? AND cal_id = ? AND object_id = ? AND trigger_at = ?")
                .bind(&principal).bind(&i.cal_id).bind(&i.object_id).bind(&key_trigger)
                .fetch_one(&state.db).await.map_err(|e| e.to_string())?.get("n");
            if already > 0 {
                continue;
            }
            let when = if i.all_day { i.start.format("%d.%m.").to_string() } else { i.start.format("%H:%M").to_string() };
            let payload = serde_json::json!({ "title": i.summary, "body": format!("{when} · {}", i.cal_name), "url": format!("/calendar?view=day&date={}", i.start_date()), "tag": i.key() }).to_string();
            for s in &subs {
                match send(state, s, &payload).await {
                    Ok(()) => sent += 1,
                    Err(e) => {
                        tracing::warn!("push to {} failed: {e}", &s.endpoint[..s.endpoint.len().min(40)]);
                        if e.contains("410") || e.contains("404") || e.contains("EndpointNotValid") || e.contains("EndpointNotFound") {
                            let _ = sqlx::query("DELETE FROM xui_push_subscriptions WHERE endpoint = ?").bind(&s.endpoint).execute(&state.db).await;
                        }
                    }
                }
            }
            sqlx::query("INSERT OR IGNORE INTO xui_alarms_sent (principal, cal_id, object_id, trigger_at) VALUES (?, ?, ?, ?)")
                .bind(&principal).bind(&i.cal_id).bind(&i.object_id).bind(&key_trigger)
                .execute(&state.db).await.map_err(|e| e.to_string())?;
        }
    }
    // housekeeping: sent-log older than 30 days
    let _ = sqlx::query("DELETE FROM xui_alarms_sent WHERE sent_at < datetime('now', '-30 days')").execute(&state.db).await;
    Ok(sent)
}

pub fn spawn_reminder_loop(state: SharedState) {
    if state.vapid.is_none() {
        tracing::info!("push reminders disabled (no VAPID key)");
        return;
    }
    tokio::spawn(async move {
        let mut t = tokio::time::interval(StdDuration::from_secs(60));
        t.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            t.tick().await;
            match tick(&state).await {
                Ok(0) => {}
                Ok(n) => tracing::info!("push reminders sent: {n}"),
                Err(e) => tracing::warn!("push reminder loop: {e}"),
            }
        }
    });
}
