//! Browser push reminders: every minute, look one hour ahead for
//! VALARM triggers of the subscribers' events, send once, remember in
//! `practical_alarms_sent` so a restart never repeats a reminder.

use crate::domain::collections;
use crate::domain::events;
use crate::domain::prefs;
use crate::domain::tasks;
use crate::state::{AppState, SharedState};
use chrono::{Duration, NaiveDate, Utc};
use rustical_store::auth::AuthenticationProvider;
use sqlx::Row;
use std::time::Duration as StdDuration;

#[derive(Clone)]
struct Sub {
    endpoint: String,
    p256dh: String,
    auth: String,
}

async fn subscriptions(state: &AppState) -> Result<Vec<(String, Vec<Sub>)>, sqlx::Error> {
    let rows = sqlx::query("SELECT principal, endpoint, p256dh, auth FROM practical_push_subscriptions ORDER BY principal").fetch_all(&state.db).await?;
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

/// Why a delivery failed; `gone` = the push service no longer knows the subscription (404/410), so
/// it is deleted.
#[derive(Debug)]
pub struct Failure {
    pub gone: bool,
    pub msg: String,
}

impl Failure {
    fn other(msg: impl ToString) -> Self {
        Self { gone: false, msg: msg.to_string() }
    }
}

async fn send(state: &AppState, sub: &Sub, payload: &str) -> Result<(), Failure> {
    // also checked on subscribe; rows stored before that check (or by hand) are refused here
    check_subscription(&sub.endpoint, &sub.p256dh, &sub.auth).map_err(Failure::other)?;
    let Some(vapid) = &state.vapid else { return Err(Failure::other("push disabled")) };
    let info = web_push::SubscriptionInfo::new(&sub.endpoint, &sub.p256dh, &sub.auth);
    let mut claims = web_push::Claims::new();
    claims.sub = Some(state.config.vapid_subject.clone());
    let sig = web_push::VapidSignatureBuilder::from_ec(vapid.0.clone(), &info).build(claims).map_err(Failure::other)?;
    let msg = web_push::WebPushMessageBuilder::new(&info).payload(web_push::ContentEncoding::Aes128Gcm, payload.as_bytes()).vapid_signature(sig).ttl(3600).build().map_err(Failure::other)?;
    let client = web_push::ReqwestWebPushClient::from_client(push_client().clone());
    web_push::WebPushClient::send(&client, msg).await.map_err(|e| Failure { gone: matches!(e, web_push::WebPushError::EndpointNotValid(_) | web_push::WebPushError::EndpointNotFound(_)), msg: e.to_string() })
}

// ---- what the server may POST to ----
// The endpoint is user input and the server POSTs to it: without limits it reaches loopback (RustiCal on
// :4000 in the same pod, which no NetworkPolicy filters) and every *.xarif.de route from inside, past
// CrowdSec. So: only the browsers' push services, https on the default port, no IP literals, no
// redirects, and every resolved address public.

/// Hosts of the browsers' push services. Primary sources (2026-10-07): Firebase "Configure your network
/// for FCM" (fcm.googleapis.com); Apple "Sending web push
/// notifications in web apps and browsers" (`https://*.push.apple.com`); Microsoft "Adding WNS traffic to
/// the firewall allowlist" (CloudServiceDNS `*.notify.windows.com`); Mozilla autopush's production host
/// (updates.push.services.mozilla.com). jmt17.google.com: endpoints of some Chromium builds, listed by
/// pushpad/known-push-services (no Google document names it).
const PUSH_HOSTS: [&str; 3] = ["fcm.googleapis.com", "jmt17.google.com", "updates.push.services.mozilla.com"];
const PUSH_HOST_SUFFIXES: [&str; 2] = [".push.apple.com", ".notify.windows.com"];
/// Longest endpoint accepted: a bound only, far above real ones (web-push's FCM example: 188 characters).
const MAX_ENDPOINT: usize = 2048;
/// Subscriptions kept per user: the newest; a new device past this drops the oldest one.
pub const MAX_SUBSCRIPTIONS: i64 = 10;

/// A subscription the server will deliver to, else the Fluent key of the reason. The endpoint must be
/// in the URL parser's normal form (what browsers send), so this check and the HTTP client read the
/// same host; the keys are what the encryption needs: an uncompressed P-256 point (65 bytes) and a
/// 16-byte secret, base64url without padding.
pub fn check_subscription(endpoint: &str, p256dh: &str, auth: &str) -> Result<(), &'static str> {
    check_endpoint(endpoint)?;
    let key = |s: &str, len: usize| <base64ct::Base64UrlUnpadded as base64ct::Encoding>::decode_vec(s).ok().filter(|b| b.len() == len);
    match (key(p256dh, 65), key(auth, 16)) {
        (Some(p), Some(_)) if p[0] == 4 => Ok(()),
        _ => Err("err-push-keys"),
    }
}

fn check_endpoint(endpoint: &str) -> Result<(), &'static str> {
    if !endpoint.starts_with("https://") {
        return Err("err-push-https");
    }
    if endpoint.len() > MAX_ENDPOINT {
        return Err("err-push-endpoint");
    }
    let Ok(u) = url::Url::parse(endpoint) else { return Err("err-push-endpoint") };
    let known = match u.host() {
        Some(url::Host::Domain(d)) => PUSH_HOSTS.contains(&d) || PUSH_HOST_SUFFIXES.iter().any(|s| d.len() > s.len() && d.ends_with(s)),
        _ => false,
    };
    if !known || u.as_str() != endpoint || u.port().is_some() || !u.username().is_empty() || u.password().is_some() || u.fragment().is_some() {
        return Err("err-push-endpoint");
    }
    Ok(())
}

/// The client for deliveries: no redirects (a push service answers, it never forwards), no proxy,
/// https only, short timeouts, and a resolver that returns public addresses only (a name the check
/// allowed cannot be pointed at loopback or the cluster by DNS). Response bodies: web-push stops
/// reading at 64 KiB (`MAX_RESPONSE_SIZE`).
fn push_client() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| client_builder(PublicOnly).https_only(true).build().expect("push client"))
}

fn client_builder(resolver: impl reqwest::dns::Resolve + 'static) -> reqwest::ClientBuilder {
    reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).no_proxy().connect_timeout(StdDuration::from_secs(5)).timeout(StdDuration::from_secs(10)).dns_resolver(resolver)
}

/// The system resolver without the addresses no push service has.
struct PublicOnly;

impl reqwest::dns::Resolve for PublicOnly {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_string();
        Box::pin(async move {
            let addrs: Vec<std::net::SocketAddr> = tokio::net::lookup_host((host.as_str(), 0)).await?.filter(|a| public_ip(a.ip())).collect();
            if addrs.is_empty() {
                return Err(format!("{host}: no public address").into());
            }
            Ok(Box::new(addrs.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

/// Globally routable unicast (the parts of std's unstable `is_global` that matter here).
fn public_ip(ip: std::net::IpAddr) -> bool {
    use std::net::{IpAddr, Ipv4Addr};
    let v4 = |a: Ipv4Addr| {
        let [o0, o1, o2, _] = a.octets();
        !(a.is_private() || a.is_loopback() || a.is_link_local() || a.is_unspecified() || a.is_broadcast() || a.is_multicast() || a.is_documentation()
            || o0 == 0                                    // "this network"
            || (o0 == 100 && (64..128).contains(&o1))     // shared address space (CGNAT)
            || (o0 == 192 && o1 == 0 && o2 == 0)          // IETF protocol assignments
            || (o0 == 198 && (18..20).contains(&o1))      // benchmarking
            || o0 >= 240)                                 // reserved
    };
    match ip {
        IpAddr::V4(a) => v4(a),
        IpAddr::V6(a) => {
            let s = a.segments();
            if let Some(m) = a.to_ipv4_mapped() {
                return v4(m);
            }
            // NAT64 (64:ff9b::/96) and 6to4 (2002::/16) carry an IPv4 address
            if s[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
                return v4(Ipv4Addr::new((s[6] >> 8) as u8, s[6] as u8, (s[7] >> 8) as u8, s[7] as u8));
            }
            if s[0] == 0x2002 {
                return v4(Ipv4Addr::new((s[1] >> 8) as u8, s[1] as u8, (s[2] >> 8) as u8, s[2] as u8));
            }
            !(a.is_loopback() || a.is_unspecified() || a.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00                  // unique local
                || (s[0] & 0xffc0) == 0xfe80                  // link local
                || (s[0] == 0x2001 && s[1] == 0x0db8)         // documentation
                || (s[0] == 0x2001 && s[1] == 0)              // Teredo
                || s[..6] == [0, 0, 0, 0, 0, 0])              // IPv4-compatible (deprecated)
        }
    }
}

/// An endpoint for the log: its host (the rest is the subscription's secret), char-safe.
fn endpoint_host(endpoint: &str) -> String {
    match url::Url::parse(endpoint).ok().and_then(|u| u.host_str().map(str::to_string)) {
        Some(h) => h,
        None => endpoint.chars().take(40).collect(),
    }
}

/// Session key: the endpoints subscribed in this session, for `forget_on_logout`.
const SESSION_ENDPOINTS: &str = "push_endpoints";

/// After a subscribe: remember the endpoint in the session (newest `MAX_SUBSCRIPTIONS`).
pub async fn remember_in_session(session: &tower_sessions::Session, endpoint: &str) {
    let mut list: Vec<String> = session.get(SESSION_ENDPOINTS).await.ok().flatten().unwrap_or_default();
    list.retain(|e| e != endpoint);
    list.push(endpoint.to_string());
    let skip = list.len().saturating_sub(MAX_SUBSCRIPTIONS as usize);
    let _ = session.insert(SESSION_ENDPOINTS, &list[skip..]).await;
}

/// Logout hook (`auth::post_logout`, BEFORE `session.flush()`): deletes the session user's push
/// subscriptions made in this session, so a shared device stops showing the previous user's reminders.
/// (app.js also deletes the browser's own subscription before it submits the logout form; this covers a
/// logout without that script.) Never fails the logout: errors go to the log. Returns the rows deleted.
pub async fn forget_on_logout(state: &AppState, session: &tower_sessions::Session) -> u64 {
    let user: Option<String> = session.get(crate::ctx::SESSION_USER).await.ok().flatten();
    let list: Vec<String> = session.get(SESSION_ENDPOINTS).await.ok().flatten().unwrap_or_default();
    let Some(user) = user else { return 0 };
    let mut n = 0;
    for e in &list {
        match sqlx::query("DELETE FROM practical_push_subscriptions WHERE principal = ? AND endpoint = ?").bind(&user).bind(e).execute(&state.db).await {
            Ok(r) => n += r.rows_affected(),
            Err(err) => tracing::warn!("push: forgetting {} at logout: {err}", endpoint_host(e)),
        }
    }
    n
}

/// One reminder ready to go out — an event's or a task's; from here on they are the same thing.
struct Fire {
    cal_id: String,
    object_id: String,
    trigger: chrono::DateTime<Utc>,
    title: String,
    body: String,
    url: String,
    tag: String,
}

/// One subscriber's reminders that can fire while `today` (in `tz`) lasts.
struct Planned {
    principal: String,
    subs: Vec<Sub>,
    tz: chrono_tz::Tz,
    today: NaiveDate,
    fires: Vec<Fire>,
}

/// What the reminder loop keeps between rounds. The reminders a round can fire depend on the
/// database and on each subscriber's day — not on the minute — so they are computed again only when
/// `db::Watch` saw a commit or a subscriber's day changed; every other round only checks the fire
/// window. (Before: every minute a full parse of every task calendar plus a 6-day expansion per
/// subscriber, ≈ 80 % of the pod's idle CPU — perf-baseline.md §1.)
#[derive(Default)]
pub struct Plan {
    stamp: Option<(u64, i64)>,
    users: Vec<Planned>,
    /// after a failed build: when to build again, and the wait (minutes) that led there
    retry_at: Option<(chrono::DateTime<Utc>, i64)>,
    /// rounds that computed the reminders (not only checked the window)
    pub builds: usize,
}

/// A reminder a round recorded as sent: principal, tag, trigger (RFC 3339), payload.
#[derive(Debug, Clone, PartialEq)]
pub struct Fired {
    pub principal: String,
    pub tag: String,
    pub trigger: String,
    pub payload: String,
}

/// Every subscriber's reminders anchored around their `today` — what each minute's round of old
/// computed afresh, unchanged. One subscriber's failure (an error, or a panic on an object their
/// calendars hold) costs only that subscriber's reminders — or with per-calendar isolation only that
/// calendar's — the others are planned; the error is returned after all of them.
async fn build(state: &AppState, now: chrono::DateTime<Utc>, out: &mut Vec<Planned>) -> Result<(), String> {
    let subs = subscriptions(state).await.map_err(|e| e.to_string())?;
    let mut failed = None;
    for (principal, subs) in subs {
        // a stored row the subscribe check would refuse (stored before it existed, or by hand) is skipped
        let (subs, bad): (Vec<Sub>, Vec<Sub>) = subs.into_iter().partition(|s| check_subscription(&s.endpoint, &s.p256dh, &s.auth).is_ok());
        for s in &bad {
            tracing::warn!("push: {principal}'s subscription at {} is not deliverable, skipped", endpoint_host(&s.endpoint));
        }
        if subs.is_empty() {
            continue;
        }
        match crate::tasks::guarded("push reminder plan", plan_for(state, now, principal.clone(), subs)).await {
            Some(Ok((p, None))) => out.extend(p),
            Some(Ok((p, Some(e)))) => {
                out.extend(p);
                failed = Some(e);
            }
            Some(Err(e)) => failed = Some(format!("{principal}: {e}")),
            None => failed = Some(format!("{principal}: panicked")),
        }
    }
    failed.map_or(Ok(()), Err)
}

/// Runs `f` on all of `cals` at once (the order the views use); if that fails, on each calendar alone so
/// one calendar's poison object costs only that calendar. Returns what succeeded and the last error.
async fn per_calendar<T, Fut>(cals: &[collections::CalInfo], f: impl Fn(Vec<collections::CalInfo>) -> Fut) -> (Vec<T>, Option<String>)
where
    Fut: std::future::Future<Output = crate::error::AppResult<Vec<T>>>,
{
    if let Some(Ok(v)) = crate::tasks::guarded("push reminder plan", f(cals.to_vec())).await {
        return (v, None);
    }
    let (mut out, mut failed) = (vec![], None);
    for cal in cals {
        match crate::tasks::guarded("push reminder plan", f(vec![cal.clone()])).await {
            Some(Ok(v)) => out.extend(v),
            Some(Err(e)) => failed = Some(format!("{}/{}: {e}", cal.principal, cal.id)),
            None => failed = Some(format!("{}/{}: panicked", cal.principal, cal.id)),
        }
    }
    (out, failed)
}

/// One subscriber's reminders; the error of a calendar that could not be read rides along.
async fn plan_for(state: &AppState, now: chrono::DateTime<Utc>, principal: String, subs: Vec<Sub>) -> Result<(Option<Planned>, Option<String>), String> {
    let Some(user) = state.principals.get_principal(&principal).await.map_err(|e| e.to_string())? else { return Ok((None, None)) };
    let p = prefs::get(&state.db, &principal).await.map_err(|e| e.to_string())?;
    let tz: chrono_tz::Tz = p.timezone.as_deref().and_then(|t| t.parse().ok()).unwrap_or_else(|| state.config.default_timezone.parse().unwrap_or(chrono_tz::Europe::Berlin));
    let today: NaiveDate = now.with_timezone(&tz).date_naive();
    let cals = collections::calendars_for(state, &user, false).await.map_err(|e| e.to_string())?;
    // a reminder can only fire in the next minutes if its anchor is within a few days
    let from = today - Duration::days(1);
    let to = today + Duration::days(4);
    let mut fires: Vec<Fire> = vec![];
    let (instances, failed_ev) = per_calendar(&cals, |c| async move { events::instances(state, &c, from, to, tz).await }).await;
    for i in instances {
        let start_utc = crate::domain::ical::to_utc(i.start, tz);
        let when = if i.all_day { i.start.format("%d.%m.").to_string() } else { i.start.format("%H:%M").to_string() };
        for m in i.alarms.iter().copied() {
            fires.push(Fire {
                cal_id: i.cal_id.clone(),
                object_id: i.object_id.clone(),
                trigger: start_utc - Duration::minutes(m),
                title: i.summary.to_string(),
                body: format!("{when} · {}", i.cal_name),
                url: format!("/calendar?view=day&date={}", i.start_date()),
                tag: i.key(),
            });
        }
    }
    // the same for tasks: their reminders used to be stored and shown but never fired
    let (reminders, failed_t) = per_calendar(&cals, |c| async move { tasks::reminders(state, &c, tz, from, to).await }).await;
    for r in reminders {
        let when = match r.due {
            Some(d) if r.due_all_day => d.format("%d.%m.").to_string(),
            Some(d) => d.format("%d.%m. %H:%M").to_string(),
            None => String::new(),
        };
        fires.push(Fire {
            cal_id: r.cal_id.clone(),
            object_id: r.object_id.clone(),
            trigger: crate::domain::ical::to_utc(r.at, tz),
            title: r.summary.clone(),
            body: if when.is_empty() { r.cal_name.clone() } else { format!("{when} · {}", r.cal_name) },
            url: "/tasks".to_string(),
            tag: r.key(),
        });
    }
    Ok((Some(Planned { principal, subs, tz, today, fires }), failed_t.or(failed_ev)))
}

/// After a build that failed (in part), the next one waits 1, 2, 4 … `MAX_RETRY_MINUTES` minutes —
/// unless the database moves first: a transient error is retried in the next round as before, a
/// persistent one (a poison object) no longer costs a full build every minute.
const MAX_RETRY_MINUTES: i64 = 16;

/// One minute's round at `now`; `stamp` = `db::Watch::stamp` taken before it. Returns the reminders
/// recorded as sent and how many deliveries succeeded; a build that failed (in part) is logged and the
/// reminders it did plan go out (an `Err` = the sent-log could not be read or written).
async fn tick(state: &SharedState, plan: &mut Plan, stamp: Option<(u64, i64)>, now: chrono::DateTime<Utc>) -> Result<(Vec<Fired>, usize), String> {
    tick_with(state, plan, stamp, now, |s, payload| async move { send(state, &s, &payload).await }).await
}

/// `tick` with the delivery as a parameter (tests record it).
async fn tick_with<Fut>(state: &SharedState, plan: &mut Plan, stamp: Option<(u64, i64)>, now: chrono::DateTime<Utc>, deliver: impl Fn(Sub, String) -> Fut) -> Result<(Vec<Fired>, usize), String>
where
    Fut: std::future::Future<Output = Result<(), Failure>>,
{
    let current = stamp.is_some() && plan.stamp == stamp && plan.users.iter().all(|u| now.with_timezone(&u.tz).date_naive() == u.today) && plan.retry_at.is_none_or(|(at, _)| now < at);
    if !current {
        let backoff = plan.retry_at.map_or(1, |(_, m)| (m * 2).min(MAX_RETRY_MINUTES));
        plan.stamp = None;
        plan.users.clear();
        plan.builds += 1;
        plan.retry_at = None;
        match build(state, now, &mut plan.users).await {
            Ok(()) => plan.stamp = stamp,
            Err(e) => {
                // what was planned fires; the rest waits for the retry or the next write
                tracing::warn!("push reminder loop: {e} (next try in {backoff} min or after a write)");
                plan.stamp = stamp;
                plan.retry_at = Some((now + Duration::minutes(backoff), backoff));
            }
        }
    }
    let mut fired = vec![];
    let mut sent = 0;
    for u in &plan.users {
        let (principal, subs) = (&u.principal, &u.subs);
        for fire in &u.fires {
            // fire window: trigger within [now-2min, now+1min] (the poll runs every minute)
            if fire.trigger < now - Duration::minutes(2) || fire.trigger > now + Duration::minutes(1) {
                continue;
            }
            let key_trigger = fire.trigger.to_rfc3339();
            let already: i64 = sqlx::query("SELECT count(*) AS n FROM practical_alarms_sent WHERE principal = ? AND cal_id = ? AND object_id = ? AND trigger_at = ?")
                .bind(principal).bind(&fire.cal_id).bind(&fire.object_id).bind(&key_trigger)
                .fetch_one(&state.db).await.map_err(|e| e.to_string())?.get("n");
            if already > 0 {
                continue;
            }
            let payload = serde_json::json!({ "title": fire.title, "body": fire.body, "url": fire.url, "tag": fire.tag }).to_string();
            // each subscription on its own: one that errs or panics does not keep the others from theirs
            for s in subs {
                match crate::tasks::guarded("push delivery", deliver(s.clone(), payload.clone())).await {
                    Some(Ok(())) => sent += 1,
                    Some(Err(e)) => {
                        tracing::warn!("push to {} failed: {}", endpoint_host(&s.endpoint), e.msg);
                        if e.gone {
                            let _ = sqlx::query("DELETE FROM practical_push_subscriptions WHERE endpoint = ?").bind(&s.endpoint).execute(&state.db).await;
                        }
                    }
                    None => {}
                }
            }
            sqlx::query("INSERT OR IGNORE INTO practical_alarms_sent (principal, cal_id, object_id, trigger_at) VALUES (?, ?, ?, ?)")
                .bind(principal).bind(&fire.cal_id).bind(&fire.object_id).bind(&key_trigger)
                .execute(&state.db).await.map_err(|e| e.to_string())?;
            fired.push(Fired { principal: principal.clone(), tag: fire.tag.clone(), trigger: key_trigger, payload });
        }
    }
    Ok((fired, sent))
}

/// Housekeeping of the sent-log (the hourly purger, `tasks::spawn_trash_purger`): a row only guards
/// its trigger's fire window of minutes, 30 days is a wide margin.
pub async fn purge_sent(state: &AppState) -> Result<u64, sqlx::Error> {
    Ok(sqlx::query("DELETE FROM practical_alarms_sent WHERE sent_at < datetime('now', '-30 days')").execute(&state.db).await?.rows_affected())
}

pub fn spawn_reminder_loop(state: SharedState) {
    if state.vapid.is_none() {
        tracing::info!("push reminders disabled (no VAPID key)");
        return;
    }
    tokio::spawn(async move {
        let mut t = tokio::time::interval(StdDuration::from_secs(60));
        t.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut plan = Plan::default();
        let mut watch = crate::db::Watch::new(state.db.clone());
        loop {
            t.tick().await;
            // the stamp BEFORE the reads: a commit racing them moves it, so the next round rebuilds
            let stamp = watch.stamp().await;
            match crate::tasks::guarded("push reminder loop", tick(&state, &mut plan, stamp, crate::clock::now())).await {
                Some(Ok((_, 0))) => {}
                Some(Ok((_, n))) => tracing::info!("push reminders sent: {n}"),
                Some(Err(e)) => tracing::warn!("push reminder loop: {e}"),
                // half-built after a panic: start over
                None => plan = Plan::default(),
            }
        }
    });
}

#[cfg(test)]
pub(crate) mod guard;
#[cfg(test)]
mod pinned;
