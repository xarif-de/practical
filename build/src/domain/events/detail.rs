//! One stored event as the editor reads it (`get_event`), the editor's save input, RELATED-TO links, and the
//! whole-calendar scans behind the pickers (category and title counts, per-object metadata cache).

use super::*;

#[derive(Debug, Clone, serde::Serialize)]
pub struct EventDetail {
    pub principal: String,
    pub cal_id: String,
    pub object_id: String,
    pub uid: String,
    pub summary: String,
    pub location: String,
    pub description: String,
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
    pub all_day: bool,
    pub rrule: Option<String>,
    /// UNTIL of the RRULE as a local date (the rule itself carries it in UTC)
    pub rrule_until: Option<NaiveDate>,
    pub exdates: Vec<String>,
    pub rdates: Vec<String>,
    pub organizer: Option<Person>,
    pub attendees: Vec<Person>,
    pub alarms: Vec<Alarm>,
    pub categories: Vec<String>,
    /// CONFIRMED | TENTATIVE | CANCELLED | ""
    pub status: String,
    /// OPAQUE | TRANSPARENT | ""
    pub transp: String,
    /// PUBLIC | PRIVATE | CONFIDENTIAL | "" | another client's value verbatim (`class_value`; a save keeps it)
    pub class: String,
    /// the strictest CLASS of the RECURRENCE-ID overrides (`overrides_class`): with `class` it makes what sharees see
    pub class_overrides: String,
    pub priority: Option<i64>,
    /// RFC 7986 COLOR (CSS colour name or #rrggbb)
    pub color: String,
    /// GEO as (lat, lon)
    pub geo: Option<(f64, f64)>,
    /// URL, ATTACH (URIs) and CONFERENCE
    pub links: Vec<Link>,
    /// RELATED-TO, resolved to events/tasks where the UID is known
    pub related: Vec<Related>,
    pub etag: String,
    pub has_overrides: bool,
    pub timezone: String,
    /// its calendar takes no PractiCal writes (`collections::is_read_only`): the editor stays in view mode
    pub read_only: bool,
}

/// `read_only`: the caller knows it already (`CalInfo::read_only`; the day view reads a detail per entry), else None
/// and it is looked up (`collections::is_read_only`).
pub async fn get_event(state: &AppState, user: &Principal, principal: &str, cal_id: &str, object_id: &str, tz: chrono_tz::Tz, read_only: Option<bool>) -> AppResult<EventDetail> {
    let auth = collections::ensure_calendar(state, user, principal, cal_id, Need::Read).await?;
    event_detail(state, &auth, object_id, tz, read_only).await
}

/// [`get_event`] in a calendar already resolved for this user (the day view holds its `CalInfo`s: one
/// resolution per page, not one per entry)
pub async fn event_detail(state: &AppState, auth: &crate::state::Authorized, object_id: &str, tz: chrono_tz::Tz, read_only: Option<bool>) -> AppResult<EventDetail> {
    let (principal, cal_id) = (auth.home(), auth.name());
    let obj = state.collections().object(auth, object_id).await?;
    let CalendarInnerData::Event(main, overrides) = obj.get_inner().get_inner() else {
        return Err(AppError::NotFound);
    };
    let cal = CalInfo::bare(auth.clone());
    let i = instance_from(main, &cal, object_id, tz, main.has_rruleset(), alarm_offsets(main));
    let (raw, extra) = split_ics(&obj.get_ics());
    let rrule = main.get_property("RRULE").map(|p| ical::rrule_canon(&p.value));
    let timezone = match &main.dtstart.0 {
        caldata::types::CalDateOrDateTime::DateTime(d) => ical::tz_of(&d.timezone()).map(|t| t.name().to_string()).unwrap_or_else(|| tz.name().to_string()),
        _ => tz.name().to_string(),
    };
    Ok(EventDetail {
        principal: principal.into(),
        cal_id: cal_id.into(),
        object_id: object_id.into(),
        uid: i.uid,
        // the whole texts: an Instance carries cut ones (`MasterText`)
        summary: ical::text(main, "SUMMARY").unwrap_or_else(|| "(ohne Titel)".into()),
        location: ical::text(main, "LOCATION").unwrap_or_default(),
        description: ical::text(main, "DESCRIPTION").unwrap_or_default(),
        start: i.start,
        end: i.end,
        all_day: i.all_day,
        rrule_until: rrule.as_deref().and_then(|r| rrule_until_local(r, tz)),
        rrule,
        exdates: dates_of(&raw.props, "EXDATE", tz),
        rdates: dates_of(&raw.props, "RDATE", tz),
        organizer: persons_of(&raw.props, "ORGANIZER").into_iter().next(),
        attendees: persons_of(&raw.props, "ATTENDEE"),
        alarms: raw.alarms.iter().enumerate().map(|(n, a)| parse_alarm(n, a, tz)).collect(),
        categories: i.categories,
        status: ical::text(main, "STATUS").unwrap_or_default().to_ascii_uppercase(),
        transp: ical::text(main, "TRANSP").unwrap_or_default().to_ascii_uppercase(),
        class: stored_class(main.get_property("CLASS").map(|p| ical::unescape(&p.value)).as_deref()),
        class_overrides: overrides_class(&extra),
        priority: ical::text(main, "PRIORITY").and_then(|p| p.trim().parse().ok()),
        color: ical::text(main, "COLOR").map(|c| c.trim().to_string()).filter(|c| valid_color(c)).unwrap_or_default(),
        geo: ical::text(main, "GEO").and_then(|g| { let (a, b) = g.split_once(';')?; Some((a.trim().parse().ok()?, b.trim().parse().ok()?)) }),
        links: links_of(&raw.props),
        related: related_of(&raw.props),
        etag: obj.get_etag(),
        has_overrides: !overrides.is_empty(),
        timezone,
        // through a share also: read access, or less than full visibility (a sharee changes only what it sees whole)
        read_only: match read_only { Some(r) => r, None => collections::is_read_only(state, auth.owner(), auth.id()).await? } || !auth.allows(Need::Write) || auth.level(&obj) < crate::state::FULL_LEVEL,
    })
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct EventInput {
    pub principal: String,
    pub cal_id: String,
    pub summary: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub description: String,
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
    #[serde(default)]
    pub all_day: bool,
    /// RRULE value without the property name, e.g. FREQ=WEEKLY
    #[serde(default)]
    pub rrule: Option<String>,
    #[serde(default)]
    pub alarms: Vec<Alarm>,
    /// local "YYYY-MM-DD" (all-day) or "YYYY-MM-DDTHH:MM:SS" in the event's zone
    #[serde(default)]
    pub exdates: Vec<String>,
    #[serde(default)]
    pub rdates: Vec<String>,
    #[serde(default)]
    pub organizer: Option<Person>,
    #[serde(default)]
    pub attendees: Vec<Person>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub transp: String,
    #[serde(default)]
    pub class: String,
    #[serde(default)]
    pub priority: Option<i64>,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub geo: Option<(f64, f64)>,
    #[serde(default)]
    pub links: Vec<Link>,
    #[serde(default)]
    pub related: Vec<Related>,
    #[serde(default)]
    pub timezone: Option<String>,
    /// the writer has confirmed that it will see the event only shortened or not at all afterwards (a share)
    #[serde(default)]
    pub confirm_visibility: bool,
    /// the writer has confirmed that its reminders are not kept (a share: `confirm_alarms`)
    #[serde(default)]
    pub confirm_alarms: bool,
}

/// RELATED-TO: a link to another event or task by UID; `target` is resolved for the editor
/// (`resolve_related`), never read back from the client.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Related {
    #[serde(default)]
    pub uid: String,
    /// RELTYPE (PARENT | CHILD | SIBLING), empty = no parameter
    #[serde(default)]
    pub reltype: String,
    #[serde(default)]
    pub extra: Vec<(String, String)>,
    #[serde(default, skip_deserializing, skip_serializing_if = "Option::is_none")]
    pub target: Option<RelatedTarget>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RelatedTarget {
    /// event | task
    pub kind: String,
    pub key: String,
    pub summary: String,
    /// start (event) or due (task) as a local date
    pub date: Option<NaiveDate>,
    pub done: bool,
}

pub(crate) fn related_of(props: &[String]) -> Vec<Related> {
    props.iter().filter(|l| prop_name(l) == "RELATED-TO").map(|l| {
        let (_, params, value) = ical::split_line(l);
        let mut r = Related { uid: value.trim().to_string(), ..Default::default() };
        for (k, v) in params {
            if k == "RELTYPE" { r.reltype = v.to_ascii_uppercase() } else { r.extra.push((k, v)) }
        }
        r
    }).collect()
}

pub(crate) fn related_lines(list: &[Related]) -> Vec<String> {
    list.iter().filter(|r| !r.uid.trim().is_empty() && !r.uid.contains(['\n', '\r', ';', ':'])).map(|r| {
        let mut line = "RELATED-TO".to_string();
        if let Some(t) = one_of(&r.reltype, &["PARENT", "CHILD", "SIBLING"]) {
            line.push_str(&format!(";RELTYPE={t}"));
        }
        for (k, v) in &r.extra {
            line.push_str(&ical::param(k, v));
        }
        format!("{line}:{}", r.uid.trim())
    }).collect()
}

/// CATEGORIES across every visible calendar (events and tasks), most used first. Values a
/// client stamps on every object ("Come From :com.appgenix.bizcal") are not categories and are hidden.
/// `scope` picks the component the counts come from: "task" counts VTODOs, anything else VEVENTs.
/// A category rule belongs to one of the two, so its category list must too. Tasks come from the
/// task cache (`tasks::tasks_for`, hence `tz`); events from the object summaries (`ObjMetaCache`).
pub async fn category_counts(state: &AppState, user: &Principal, scope: &str, tz: chrono_tz::Tz) -> AppResult<Vec<(String, usize)>> {
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut count = |cats: &[String]| {
        for c in cats.iter().map(|c| c.trim()).filter(|c| !c.is_empty() && !c.starts_with("Come From :")) {
            match counts.get_mut(c) {
                Some(n) => *n += 1,
                None => {
                    counts.insert(c.to_string(), 1);
                }
            }
        }
    };
    let cals = visible_calendars(state, user, None).await?;
    if scope == "task" {
        for t in crate::domain::tasks::tasks_for(state, &cals, tz).await? {
            count(&t.categories);
        }
    } else {
        for cal in &cals {
            for o in object_meta(state, cal).await?.objs.iter().filter(|o| o.event) {
                count(&o.categories);
            }
        }
    }
    let mut out: Vec<(String, usize)> = counts.into_iter().collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.to_lowercase().cmp(&b.0.to_lowercase())));
    Ok(out)
}

/// Every event title with its number of events, most used first (the vacation rule's title test in
/// the settings runs over it). Same objects as `category_counts`.
pub async fn title_counts(state: &AppState, user: &Principal) -> AppResult<Vec<(String, usize)>> {
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for cal in visible_calendars(state, user, None).await? {
        for o in object_meta(state, &cal).await?.objs.iter().filter(|o| o.event) {
            let t = o.summary.as_deref().unwrap_or_default().trim();
            if !t.is_empty() {
                match counts.get_mut(t) {
                    Some(n) => *n += 1,
                    None => {
                        counts.insert(t.to_string(), 1);
                    }
                }
            }
        }
    }
    let mut out: Vec<(String, usize)> = counts.into_iter().collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.to_lowercase().cmp(&b.0.to_lowercase())));
    Ok(out)
}

/// What the whole-calendar scans (`category_counts`, `title_counts`, `colors::sources`) read of one
/// event or task, so they do not parse every object on every call.
#[derive(Debug, Clone)]
pub struct ObjMeta {
    pub id: String,
    pub uid: String,
    /// VEVENT; else VTODO (other components are not kept)
    pub event: bool,
    /// `ical::text` of SUMMARY, COLOR; `ical::list` of CATEGORIES
    pub summary: Option<String>,
    pub color: Option<String>,
    pub categories: Vec<String>,
}

fn meta_of(id: String, obj: &CalendarObject) -> Option<ObjMeta> {
    let uid = obj.get_uid().to_string();
    Some(match obj.get_inner().get_inner() {
        CalendarInnerData::Event(ev, _) => ObjMeta { id, uid, event: true, summary: ical::text(ev, "SUMMARY"), color: ical::text(ev, "COLOR"), categories: ical::list(ev, "CATEGORIES") },
        CalendarInnerData::Todo(t, _) => ObjMeta { id, uid, event: false, summary: ical::text(t, "SUMMARY"), color: ical::text(t, "COLOR"), categories: ical::list(t, "CATEGORIES") },
        _ => return None,
    })
}

/// The objects of one calendar at a synctoken, in the store's read order (`get_objects` walks the
/// (principal, cal_id, uid) index: by UID — `EXPLAIN QUERY PLAN` on RustiCal 0.16.4's schema).
pub struct CalMeta {
    token: i64,
    pub objs: Vec<ObjMeta>,
}

/// `CalMeta` per calendar. A moved token is caught up through the store's change log
/// (`sync_changes`, as `tasks::TaskCache` does), so an edit re-reads only what changed. Size: one
/// small record per object of the calendars someone looked at.
#[derive(Default)]
pub struct ObjMetaCache(std::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<CalMeta>>>);

pub async fn object_meta(state: &AppState, cal: &CalInfo) -> AppResult<std::sync::Arc<CalMeta>> {
    // the calendar, its view and its path: a share shows other objects than the calendar itself
    let k = cal.auth.cache_key();
    let cached = state.caches.obj_meta.0.lock().unwrap().get(&k).cloned();
    if let Some(c) = &cached
        && c.token >= cal.synctoken
    {
        // equal, or newer than the caller's calendar row (read before a write): at least as fresh
        return Ok(c.clone());
    }
    // `cal.synctoken` was read before the objects: a write racing this read is applied again next time
    let fresh = match cached {
        Some(c) => {
            let (changed, deleted, _) = state.collections().sync_changes(&cal.auth, c.token).await?;
            let gone: HashSet<&str> = deleted.iter().map(String::as_str).chain(changed.iter().map(|(id, _)| id.as_str())).collect();
            let mut objs: Vec<ObjMeta> = c.objs.iter().filter(|o| !gone.contains(o.id.as_str())).cloned().collect();
            for (id, obj) in changed {
                if let Some(m) = meta_of(id, &obj) {
                    let at = objs.partition_point(|o| o.uid < m.uid);
                    objs.insert(at, m);
                }
            }
            CalMeta { token: cal.synctoken, objs }
        }
        None => {
            let mut objs: Vec<ObjMeta> = state.collections().objects(&cal.auth).await?.into_iter().filter_map(|(id, obj)| meta_of(id, &obj)).collect();
            // collected in place from the parsed objects' buffer: release what the records do not need
            objs.shrink_to_fit();
            CalMeta { token: cal.synctoken, objs }
        }
    };
    let fresh = std::sync::Arc::new(fresh);
    let mut m = state.caches.obj_meta.0.lock().unwrap();
    if m.get(&k).is_none_or(|c| c.token <= fresh.token) {
        m.insert(k, fresh.clone());
    }
    Ok(fresh)
}

/// Fills `target` of every RELATED-TO whose UID is an event or task in a visible calendar. One
/// scan of all objects (≈ 320 ms in a debug build on 3 800 objects), only called when the
/// event has RELATED-TO lines.
pub async fn resolve_related(state: &AppState, user: &Principal, related: &mut [Related], tz: chrono_tz::Tz) -> AppResult<()> {
    let wanted: HashSet<String> = related.iter().map(|r| r.uid.trim().to_string()).filter(|u| !u.is_empty()).collect();
    if wanted.is_empty() {
        return Ok(());
    }
    let mut found: std::collections::HashMap<String, RelatedTarget> = std::collections::HashMap::new();
    for cal in visible_calendars(state, user, None).await? {
        for (id, obj) in state.collections().objects(&cal.auth).await? {
            let uid = obj.get_uid().to_string();
            if !wanted.contains(&uid) || found.contains_key(&uid) {
                continue;
            }
            let key = format!("{}/{}/{}", cal.principal, cal.id, id);
            match obj.get_inner().get_inner() {
                CalendarInnerData::Event(ev, _) => {
                    found.insert(uid, RelatedTarget { kind: "event".into(), key, summary: ical::text(ev, "SUMMARY").unwrap_or_default(), date: Some(ical::local(&ev.dtstart.0, tz).0.date()), done: ev.get_property("STATUS").is_some_and(|s| s.value.eq_ignore_ascii_case("CANCELLED")) });
                }
                CalendarInnerData::Todo(t, _) => {
                    found.insert(uid, RelatedTarget { kind: "task".into(), key, summary: ical::text(t, "SUMMARY").unwrap_or_default(), date: t.due.as_ref().map(|d| ical::local(&d.0, tz).0.date()), done: t.get_property("STATUS").is_some_and(|s| s.value.eq_ignore_ascii_case("COMPLETED")) || t.get_property("COMPLETED").is_some() });
                }
                _ => {}
            }
            if found.len() == wanted.len() {
                break;
            }
        }
    }
    for r in related.iter_mut() {
        r.target = found.get(r.uid.trim()).cloned();
    }
    Ok(())
}
