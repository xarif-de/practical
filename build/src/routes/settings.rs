//! /settings — groups, collections, app tokens, trash, preferences, import/export. All mutations are POST
//! forms with CSRF (the form field, or X-CSRF-Token for the JSON saves).
//!
//! Every form handler answers through one envelope, [`form`] → [`finish`]: a redirect back to the page
//! section with a flash (or JSON for a fetch). The forms live in the sub-modules (groups, collections,
//! tokens, preferences, transfer = export and import). The two routes that import into a new collection
//! accept a 32 MiB body.

use crate::csrf;
use crate::ctx::{PageCtx, set_flash};
use crate::domain::collections::{self, AbInfo, CalInfo, NewCalendar, Owner, PALETTE};
use crate::domain::events;
use crate::domain::groups::{self, GroupInfo, RemoveOutcome, TrashedGroup};
use crate::domain::history::{self, Actor};
use crate::domain::history_text::Store;
use crate::domain::holidays::{self, Region};
use crate::domain::prefs::{self, Prefs};
use crate::domain::tokens;
use crate::error::{AppError, AppResult};
use crate::state::SharedState;
use askama::Template;
use askama_web::WebTemplate;
use axum::extract::State;
use crate::extract::{Form, Path};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::Router;
use rustical_store::auth::{AppToken, Principal};
use serde::Deserialize;
use std::collections::HashMap;
use tower_sessions::Session;

mod group_forms;
mod collection_forms;
mod onboarding;
mod token_forms;
mod pref_forms;
mod transfer;
use group_forms::*;
use collection_forms::*;
use token_forms::*;
use pref_forms::*;
use transfer::*;

pub(crate) const SESSION_NEW_TOKEN: &str = "new_token";

#[derive(Template, WebTemplate)]
#[template(path = "settings.html")]
pub struct SettingsPage {
    pub ctx: PageCtx,
    pub groups: Vec<GroupInfo>,
    pub individuals: Vec<Principal>,
    pub trashed: Vec<TrashedGroup>,
    pub calendars: Vec<CalInfo>,
    pub addressbooks: Vec<AbInfo>,
    pub deleted_calendars: Vec<CalInfo>,
    pub deleted_addressbooks: Vec<AbInfo>,
    /// connected devices with when they were last seen ([`tokens::devices`]; never-connected tokens left out)
    pub tokens: Vec<(AppToken, tokens::Seen)>,
    pub new_token: Option<String>,
    pub prefs: Prefs,
    pub owners: Vec<Owner>,
    pub palette: Vec<String>,
    pub retention_days: u32,
    pub default_timezone: String,
    /// the regions of public / school holidays (codes, in the user's order), comma-joined for the form
    pub public_regions: String,
    pub school_regions: String,
    /// name and path of every chosen region ("Sachsen", "Deutschland")
    pub region_names: HashMap<String, (String, String)>,
    /// the pickers' data (countries, time zones, chosen regions' names) as JSON for app.js
    pub picker_json: String,
    /// `:root { --hol; --fer }` from the user's holiday colours ("" = the theme's)
    pub holiday_css: String,
    /// the vacation rule as the page edits it: its kind and one value per kind (the other two are
    /// the defaults the page offers when the kind is switched)
    pub vacation_kind: String,
    pub vacation_category: String,
    pub vacation_calendar: String,
    pub vacation_title: String,
    /// the calendars a rule can name: "principal/id" and the name
    pub vacation_calendars: Vec<(String, String)>,
    /// the rule names a calendar the user no longer sees: kept as an option so a save does not change it
    pub vacation_calendar_unknown: bool,
    /// the rule in the overview row: „Urlaub“, „Privat“ (a calendar's name), /^Urlaub/
    pub vacation_summary: String,
    /// reminder strip snap points as the server writes them ("2 min, 1 h, …"), and the built-in lists
    pub alarm_snaps_start: String,
    pub alarm_snaps_end: String,
    pub alarm_snaps_default_start: String,
    pub alarm_snaps_default_end: String,
    /// a sync link needs a key to seal its app password; without one only the one-off import shows
    pub remote_sync: bool,
    /// the user's links as the page renders them, newest-looking first
    pub remote_links: Vec<RemoteLinkRow>,
    /// addresses the user signed in at before, newest first, for the address field to offer back
    pub remote_servers: Vec<String>,
    /// "add a device" › Android: the link that opens DAVx⁵ in its login for this server (`domain::connect`)
    pub davx5_intent: String,
    /// the page is Chrome's fallback of the DAVx⁵ link: the install step says DAVx⁵ is missing
    pub davx5_missing: bool,
    /// the collections as DAVx⁵ lists them (done step on another device; as routes::connect `confirm`)
    pub davx5_calendars: Vec<String>,
    pub davx5_books: Vec<String>,
    /// this server's host as a user types it on the other device ("plan.example.org")
    pub plan_host: String,
    pub stores: &'static [crate::domain::connect::Store],
}

#[derive(Deserialize)]
struct PageQuery {
    /// Chrome opened the intent link's fallback: DAVx⁵ is not installed (`domain::connect::davx5_intent`)
    #[serde(default)]
    davx5: String,
}

/// One sync link, as the collection's row and its own page read it. `kind`/`local_id` are what
/// [`SettingsPage::link_for`] matches on: a link belongs to one collection, and both lists are
/// rendered by the same two loops.
pub struct RemoteLinkRow {
    pub id: String,
    pub kind: String,
    /// whose the collection is (the user or a group): ids are unique per owner only
    pub owner: String,
    pub local_id: String,
    pub two_way: bool,
    pub ran_at: String,
    pub err: String,
}

/// The rule as the page edits it: (kind, value). A rule from before v2 ("category or title word")
/// becomes the category when one of that name is in use, else the whole word in the title.
async fn vacation_rule(state: &SharedState, user: &Principal, p: &Prefs) -> AppResult<(String, String)> {
    let r = prefs::VacationRule::of(p);
    if !r.legacy() {
        return Ok((if matches!(r.kind.as_str(), "calendar" | "title") { r.kind } else { "category".into() }, r.value));
    }
    let v = r.value.trim();
    if let Some((name, _)) = events::category_counts(state, user, "event", chrono_tz::UTC).await?.into_iter().find(|(c, _)| c.to_lowercase() == v.to_lowercase()) {
        return Ok(("category".into(), name));
    }
    Ok(("title".into(), if v.is_empty() { String::new() } else { format!(r"\b{}\b", regex::escape(v)) }))
}

impl SettingsPage {
    pub fn lang_is(&self, l: &str) -> bool {
        self.prefs.lang.as_deref().unwrap_or(&self.ctx.lang) == l
    }
    /// a region list as its row shows it: "Sachsen, Bayern", "3 regions" from three on, "none"
    pub fn regions_text(&self, school: bool) -> String {
        let list: Vec<&str> = if school { &self.school_regions } else { &self.public_regions }.split(',').filter(|c| !c.is_empty()).collect();
        match list.len() {
            0 => self.ctx.t("set-regions-none"),
            1 | 2 => list.iter().map(|c| self.region_names.get(*c).map(|n| n.0.clone()).unwrap_or_else(|| c.to_string())).collect::<Vec<_>>().join(", "),
            n => self.ctx.tn("set-regions-n", "n", n as i64),
        }
    }
    /// the colour pickers' values: the user's colour, else the (light) theme's
    /// "Holidays: OpenHolidays, Nager.Date · School holidays: OpenHolidays"
    pub fn sources_text(&self) -> String {
        // Nothing chosen, nothing to source: naming the data sources of no region read like a
        // failure ("Feiertage: keine Quelle") where in truth the user has simply not picked one.
        if self.public_regions.is_empty() && self.school_regions.is_empty() {
            return String::new();
        }
        // over every chosen country, in chain order
        let mut public = vec![];
        let mut school = vec![];
        for code in self.public_regions.split(',').chain(self.school_regions.split(',')).filter(|c| !c.is_empty()) {
            let (p, s) = holidays::sources(holidays::split(code).0);
            public.extend(p);
            school.extend(s);
        }
        for v in [&mut public, &mut school] {
            v.sort_by_key(|x| usize::from(*x != "OpenHolidays"));
            v.dedup();
        }
        let none = self.ctx.t("set-src-none");
        let list = |v: Vec<&str>| if v.is_empty() { none.clone() } else { v.join(" → ") };
        let mut args = fluent_bundle::FluentArgs::new();
        args.set("public", list(public));
        args.set("school", list(school));
        self.ctx.i18n.t_args(&self.ctx.lang, "set-sources", Some(&args))
    }
    /// A name to suggest on the create page while the first start still asks for this kind: the
    /// very first collection should not make the user invent a word (asked for 2026-10-09). Once
    /// they have one, naming the next is a real decision and the field stays empty.
    ///
    /// The question is the onboarding one — does this user own none? — not `self.calendars`, which
    /// also holds the collections of their groups (bob sees a family calendar and owns nothing).
    pub fn first_name(&self, addressbook: bool) -> String {
        let step = if addressbook { "addressbook" } else { "calendar" };
        if self.ctx.onboarding_open(step) { self.ctx.t(&format!("col-first-{step}")) } else { String::new() }
    }
    /// the time zone as shown in its row: the chosen one or "Default (Europe/Berlin)"
    pub fn tz_name(&self) -> String {
        match &self.prefs.timezone {
            Some(t) => t.replace('_', " "),
            None => self.ctx.t1("set-tz-default", "tz", &self.default_timezone),
        }
    }
    /// the second line of a device's row: when it last connected (hour granularity: RustiCal records a use at
    /// most once per hour), or since when it is connected for a token from before uses were recorded
    pub fn device_seen(&self, t: &AppToken, seen: &tokens::Seen) -> String {
        match seen {
            tokens::Seen::At(at) => {
                let hours = (chrono::Utc::now() - *at).num_hours().max(0);
                // < 2 h is "the last hour": RustiCal writes a use at most once per hour and process, so a device
                // that syncs all the time can read 1 h old
                match hours {
                    0 | 1 => self.ctx.t("devices-seen-hour"),
                    2..=47 => self.ctx.tn("devices-seen-hours", "n", hours),
                    _ => self.ctx.tn("devices-seen-days", "n", hours / 24),
                }
            }
            _ => t.created_at.map(|c| self.ctx.t1("devices-since", "date", c.format("%Y-%m-%d").to_string().as_str())).unwrap_or_default(),
        }
    }
    /// the symbol of a connected device's row
    pub fn device_icon(&self, name: &str) -> &'static str {
        crate::domain::connect::device_icon(name)
    }
    /// the groups with calendars: an iPhone set up by hand needs a CalDAV account for each (#add-ios-more)
    pub fn cal_groups(&self) -> Vec<&GroupInfo> {
        self.groups.iter().filter(|g| g.n_calendars > 0).collect()
    }
    pub fn scheme_is(&self, s: &str) -> bool {
        self.prefs.scheme.as_deref().unwrap_or("auto") == s
    }
    /// The remembered server addresses as the page's JSON island (`#rm-servers`).
    pub fn servers_json(&self) -> String {
        serde_json::to_string(&self.remote_servers).map(crate::render::script_json).unwrap_or_else(|_| "[]".into())
    }
    /// The sync link of one collection, if it has one. A collection has at most one
    /// (`UNIQUE (owner, kind, local_id)`, migration 20), so the first match is the answer.
    pub fn link_for(&self, kind: &str, owner: &str, local_id: &str) -> Option<&RemoteLinkRow> {
        self.remote_links.iter().find(|l| l.kind == kind && l.owner == owner && l.local_id == local_id)
    }
    /// VEVENT/VTODO/VJOURNAL as words ("Termine, Aufgaben")
    pub fn comps(&self, c: &[String]) -> String {
        c.iter().map(|k| match k.as_str() { "VEVENT" => self.ctx.t("set-comp-event"), "VTODO" => self.ctx.t("set-comp-todo"), "VJOURNAL" => self.ctx.t("set-comp-journal"), o => o.to_string() }).collect::<Vec<_>>().join(", ")
    }
}

/// The user's links. No name of its own: a link is shown on the row and the page of the collection
/// it syncs, which is where its name already is. A link whose collection was deleted does not
/// exist — `collections::delete_*` ends it (`remote::unlink_collection`).
async fn remote_rows(state: &SharedState, user: &Principal) -> AppResult<Vec<RemoteLinkRow>> {
    let mut out = vec![];
    for l in crate::domain::remote::links_of(state, user).await? {
        let kind = match l.kind {
            crate::domain::remote::dav::Kind::Cal => "cal",
            crate::domain::remote::dav::Kind::Ab => "ab",
        };
        out.push(RemoteLinkRow {
            id: l.id,
            kind: kind.to_string(),
            owner: l.owner,
            local_id: l.local_id,
            two_way: l.two_way,
            // the stored value is RFC 3339 with microseconds; shown like every other timestamp in
            // the settings (the API tokens' "created"), and left as it is if it cannot be parsed
            ran_at: l
                .ran_at
                .map(|t| {
                    chrono::DateTime::parse_from_rfc3339(&t).map_or(t.clone(), |d| d.naive_local().format("%Y-%m-%d %H:%M").to_string())
                })
                .unwrap_or_default(),
            err: l.err.unwrap_or_default(),
        });
    }
    Ok(out)
}

async fn page(State(state): State<SharedState>, user: Principal, session: Session, ctx: PageCtx, crate::extract::Query(q): crate::extract::Query<PageQuery>) -> AppResult<SettingsPage> {
    let new_token: Option<String> = if session.get_value(SESSION_NEW_TOKEN).await?.is_some() { session.remove(SESSION_NEW_TOKEN).await? } else { None }; // no session write without one
    let p = prefs::get(&state.db, &user.id).await?;
    let snaps = events::alarm_snaps(p.alarm_snaps_json.as_deref());
    let (public_regions, school_regions) = (p.regions(false), p.regions(true));
    let mut region_names = HashMap::new();
    for code in public_regions.iter().chain(&school_regions) {
        if !region_names.contains_key(code) {
            region_names.insert(code.clone(), holidays::describe(&state, code, &ctx.lang).await?);
        }
    }
    let cals_all = collections::calendars_for(&state, &user, false).await?;
    let (vac_kind, vac_value) = vacation_rule(&state, &user, &p).await?;
    let vac_summary = match vac_kind.as_str() {
        "title" => format!("/{vac_value}/"),
        "calendar" => format!("„{}“", cals_all.iter().find(|c| format!("{}/{}", c.principal, c.id) == vac_value).map_or(vac_value.as_str(), |c| c.displayname.as_str())),
        _ => format!("„{vac_value}“"),
    };
    let abs_all = collections::addressbooks_for(&state, &user, false).await?;
    let remote_links = remote_rows(&state, &user).await?;
    let davx5_books: Vec<String> = abs_all.iter().map(|a| a.displayname.clone()).collect();
    Ok(SettingsPage {
        remote_sync: crate::domain::remote::sync_enabled(&state),
        remote_servers: crate::domain::remote::servers_of(&state, &user).await?,
        remote_links,
        davx5_intent: crate::domain::connect::davx5_intent(&state.config.public_url),
        davx5_missing: q.davx5 == "missing",
        davx5_calendars: crate::domain::connect::davx5_calendars(cals_all.iter().map(|c| c.displayname.clone()), &davx5_books),
        davx5_books,
        stores: &crate::domain::connect::STORES,
        plan_host: crate::domain::connect::host_of(&state.config.public_url),
        alarm_snaps_start: events::fmt_offsets(&snaps.start),
        alarm_snaps_end: events::fmt_offsets(&snaps.end),
        alarm_snaps_default_start: events::fmt_offsets(&events::AlarmSnaps::default().start),
        alarm_snaps_default_end: events::fmt_offsets(&events::AlarmSnaps::default().end),
        groups: groups::list_for_user(&state, &user).await?,
        individuals: groups::individuals(&state).await?,
        trashed: groups::trashed_for_user(&state, &user).await?,
        calendars: cals_all.clone(),
        addressbooks: abs_all.clone(),
        deleted_calendars: collections::calendars_for(&state, &user, true).await?,
        deleted_addressbooks: collections::addressbooks_for(&state, &user, true).await?,
        tokens: tokens::devices(&state, &user).await?,
        new_token,
        prefs: p.clone(),
        owners: collections::owners(&state, &user).await?,
        palette: PALETTE.iter().map(|s| s.to_string()).collect(),
        retention_days: state.config.trash_retention_days,
        default_timezone: state.config.default_timezone.clone(),
        picker_json: crate::render::script_json(serde_json::json!({ "countries": holidays::countries(&ctx.lang), "zones": timezones(), "names": region_names }).to_string()),
        public_regions: public_regions.join(","),
        school_regions: school_regions.join(","),
        region_names,
        holiday_css: p.holiday_css(),
        vacation_summary: vac_summary,
        vacation_kind: vac_kind.clone(),
        vacation_category: if vac_kind == "category" { vac_value.clone() } else { String::new() },
        vacation_title: if vac_kind == "title" { vac_value.clone() } else if vac_kind == "category" && !vac_value.is_empty() { format!(r"\b{}\b", regex::escape(&vac_value)) } else { r"\bUrlaub\b".into() },
        vacation_calendar_unknown: vac_kind == "calendar" && !vac_value.is_empty() && !cals_all.iter().any(|c| format!("{}/{}", c.principal, c.id) == vac_value),
        vacation_calendar: if vac_kind == "calendar" { vac_value } else { String::new() },
        vacation_calendars: cals_all.iter().filter(|c| c.components.is_empty() || c.components.iter().any(|x| x == "VEVENT")).map(|c| (format!("{}/{}", c.principal, c.id), c.displayname.clone())).collect(),
        ctx,
    })
}

/// The IANA zones a person would pick, grouped by continent/ocean (legacy aliases such as
/// "US/Eastern" or "Etc/GMT+1" left out).
fn timezones() -> Vec<(String, Vec<(String, String)>)> {
    const AREAS: [&str; 10] = ["Africa", "America", "Antarctica", "Arctic", "Asia", "Atlantic", "Australia", "Europe", "Indian", "Pacific"];
    let mut out: Vec<(String, Vec<(String, String)>)> = AREAS.iter().map(|a| (a.to_string(), vec![])).collect();
    for tz in chrono_tz::TZ_VARIANTS {
        let name = tz.name();
        if let Some((area, rest)) = name.split_once('/')
            && let Some(g) = out.iter_mut().find(|(a, _)| a == area)
        {
            g.1.push((name.to_string(), rest.replace('_', " ").replace('/', " / ")));
        }
    }
    out.retain(|(_, v)| !v.is_empty());
    out
}

/// GET /api/holidays/countries — the countries a holiday source covers, in the page language.
async fn api_countries(_user: Principal, ctx: PageCtx) -> axum::Json<Vec<Region>> {
    axum::Json(holidays::countries(&ctx.lang))
}

#[derive(Deserialize)]
struct CountryQuery {
    country: String,
}

/// GET /api/holidays/subdivisions?country=CH — its subdivisions, all levels, parents first.
async fn api_subdivisions(State(state): State<SharedState>, _user: Principal, ctx: PageCtx, crate::extract::Query(q): crate::extract::Query<CountryQuery>) -> AppResult<axum::Json<Vec<Region>>> {
    Ok(axum::Json(holidays::subdivisions(&state, &q.country.to_uppercase(), &ctx.lang).await?))
}

/// Redirect back to the section. A success says something only when the reloaded
/// page cannot show it (a save leaves the same values on screen); store errors
/// become error flashes so the user never lands on a bare error page after a form post.
/// The language gate and the confirmations answer on the page the user is on, not under `/settings`:
/// the gate blocks whatever page was opened first, so it has to come back to that one.
async fn finish_here(state: &SharedState, session: &Session, ctx: &PageCtx, result: AppResult<Option<&str>>, back: &str) -> Response {
    if ctx.wants_json {
        let (ok, msg) = match result {
            Ok(key) => (true, key.map(|k| state.i18n.t(&ctx.lang, k))),
            Err(e) => (false, Some(failed(state, ctx, &e))),
        };
        return axum::Json(serde_json::json!({ "ok": ok, "msg": msg })).into_response();
    }
    match result {
        Ok(None) => {}
        Ok(Some(key)) => set_flash(session, "ok", state.i18n.t(&ctx.lang, key)).await,
        Err(e) => set_flash(session, "error", failed(state, ctx, &e)).await,
    }
    Redirect::to(back).into_response()
}

async fn finish(state: &SharedState, session: &Session, ctx: &PageCtx, result: AppResult<Option<&str>>, anchor: &str) -> Response {
    finish_more(state, session, ctx, result, anchor, serde_json::Map::new()).await
}

/// [`finish`] plus fields for the JSON answer, for a save whose outcome the page cannot predict.
async fn finish_more(
    state: &SharedState,
    session: &Session,
    ctx: &PageCtx,
    result: AppResult<Option<&str>>,
    anchor: &str,
    more: serde_json::Map<String, serde_json::Value>,
) -> Response {
    // a form saved by fetch (settings autosave) gets its message back instead of a flash for the next page
    if ctx.wants_json {
        let (ok, msg) = match result {
            Ok(key) => (true, key.map(|k| state.i18n.t(&ctx.lang, k))),
            Err(e) => (false, Some(failed(state, ctx, &e))),
        };
        let mut o = serde_json::Map::new();
        o.insert("ok".into(), ok.into());
        o.insert("msg".into(), msg.into());
        o.extend(more);
        return axum::Json(serde_json::Value::Object(o)).into_response();
    }
    match result {
        Ok(None) => {}
        Ok(Some(key)) => set_flash(session, "ok", state.i18n.t(&ctx.lang, key)).await,
        Err(e) => set_flash(session, "error", failed(state, ctx, &e)).await,
    }
    Redirect::to(&format!("/settings#{anchor}")).into_response()
}

/// A settings form action: the form's CSRF field, then `op` (an async block does nothing before it is
/// awaited, so nothing of it runs on a bad token), answered by `finish`.
async fn form(state: &SharedState, session: &Session, ctx: &PageCtx, token: &str, anchor: &str, op: impl Future<Output = AppResult<Option<&'static str>>>) -> Response {
    let r = match csrf::check(session, token).await {
        Ok(()) => op.await,
        Err(e) => Err(e),
    };
    finish(state, session, ctx, r, anchor).await
}

/// The user's message for a failed settings action (`AppError::user_message`); a server-side failure is
/// logged with its own text, which the user does not see.
fn failed(state: &SharedState, ctx: &PageCtx, e: &AppError) -> String {
    if e.user_text().is_some_and(|u| u.key == "flash-failed") {
        tracing::error!("{e}");
    }
    e.user_message(&state.i18n, &ctx.lang)
}

#[derive(Deserialize)]
pub(super) struct Csrf {
    pub(super) csrf: String,
}

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/settings", get(page))
        .route("/settings/groups", post(group_create))
        .route("/settings/groups/{gid}/members", post(group_add_member))
        .route("/settings/groups/{gid}/members/{member}/remove", post(group_remove_member))
        .route("/settings/groups/{gid}/leave", post(group_leave))
        .route("/settings/groups/{gid}/delete", post(group_delete))
        .route("/settings/trash/groups/{gid}/restore", post(group_restore))
        .route("/settings/trash/groups/{gid}/purge", post(group_purge))
        .route("/settings/calendars", post(calendar_create))
        .route("/settings/calendars/{owner}/{id}/update", post(calendar_update))
        .route("/settings/calendars/{owner}/{id}/delete", post(calendar_delete))
        .route("/settings/calendars/{owner}/{id}/restore", post(calendar_restore))
        .route("/settings/calendars/{owner}/{id}/purge", post(calendar_purge))
        .route("/settings/addressbooks", post(addressbook_create))
        .route("/settings/addressbooks/{owner}/{id}/update", post(addressbook_update))
        .route("/settings/addressbooks/{owner}/{id}/delete", post(addressbook_delete))
        .route("/settings/addressbooks/{owner}/{id}/restore", post(addressbook_restore))
        .route("/settings/addressbooks/{owner}/{id}/purge", post(addressbook_purge))
        .route("/settings/tokens", post(token_create))
        .route("/settings/tokens/{id}/delete", post(token_delete))
        .route("/settings/prefs", post(prefs_save))
        .route("/settings/onboarding/lang", post(onboarding::lang_save))
        .route("/settings/onboarding/confirm", post(onboarding::confirm))
        .route("/settings/onboarding/complete", post(onboarding::complete))
        .route("/settings/onboarding/reopen", post(onboarding::reopen))
        .route("/api/onboarding", get(onboarding::api_state))
        .route("/settings/holiday-display", post(holiday_display_save))
        .route("/settings/scheme", post(scheme_save))
        .route("/api/holidays/countries", get(api_countries))
        .route("/api/holidays/subdivisions", get(api_subdivisions))
        .route("/settings/columns", post(columns_save))
        .route("/settings/list-fields", post(list_fields_save))
        .route("/settings/task-columns", post(task_columns_save))
        .route("/settings/filter-pin", post(filter_pin_save))
        .route("/settings/export/calendars/{owner}/{id}", get(export_calendar))
        .route("/settings/export/addressbooks/{owner}/{id}", get(export_addressbook))
        .route("/settings/import/calendars", post(import_new_calendar).layer(axum::extract::DefaultBodyLimit::max(32 * 1024 * 1024)))
        .route("/settings/import/calendars/{owner}/{id}", post(import_calendar))
        .route("/settings/import/addressbooks", post(import_new_addressbook).layer(axum::extract::DefaultBodyLimit::max(32 * 1024 * 1024)))
        .route("/settings/import/addressbooks/{owner}/{id}", post(import_addressbook))
        .route("/settings/remote/login", post(remote::login))
        .route("/settings/remote/poll", post(remote::poll))
        .route("/settings/remote/reuse", post(remote::reuse))
        .route("/settings/remote/collections", post(remote::collections_offered))
        .route("/settings/remote/import", post(remote::import))
        .route("/settings/remote/done", post(remote::done))
        .route("/settings/remote/link", post(remote::link))
        .route("/settings/remote/link/{id}", post(remote::edit))
}

mod remote;

#[cfg(test)]
mod pinned;
