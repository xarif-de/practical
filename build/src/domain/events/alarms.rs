//! Reminders: the editor's `Alarm`, per-user presets and snap points, VALARM parse and build.

use super::*;

/// One VALARM as the editor sees it. `idx` is the alarm's position in the stored object: on
/// save the lines the editor does not model (UID, RELATED-TO, binary ATTACH, an EMAIL's
/// attachments, VLOCATION, X-…) are copied from that alarm; a new alarm has none.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Alarm {
    #[serde(default)]
    pub idx: Option<usize>,
    /// DISPLAY (default) | AUDIO | EMAIL
    #[serde(default)]
    pub action: String,
    /// minutes before the anchor (negative = after)
    #[serde(default)]
    pub minutes: i64,
    /// anchor is the end (TRIGGER;RELATED=END) instead of the start
    #[serde(default)]
    pub related_end: bool,
    /// absolute trigger in the user's zone (TRIGGER;VALUE=DATE-TIME); wins over `minutes`
    #[serde(default)]
    pub at: Option<NaiveDateTime>,
    /// DESCRIPTION: the text shown (DISPLAY) or the mail body (EMAIL); empty = the event's title
    #[serde(default)]
    pub description: String,
    /// SUMMARY: the mail subject (EMAIL only); empty = the event's title
    #[serde(default)]
    pub summary: String,
    /// DURATION/REPEAT pair: repeat the alarm `repeat` more times every `duration_minutes`
    #[serde(default)]
    pub repeat: i64,
    #[serde(default)]
    pub duration_minutes: i64,
    /// ATTENDEE mail addresses (EMAIL only)
    #[serde(default)]
    pub attendees: Vec<String>,
    /// ATTACH: the sound's URI (AUDIO only); binary and EMAIL attachments are preserved, not edited
    #[serde(default)]
    pub attach: String,
    /// PROXIMITY (RFC 9074): ARRIVE | DEPART | CONNECT | DISCONNECT, empty = none
    #[serde(default)]
    pub proximity: String,
    /// ACKNOWLEDGED (RFC 9074) in the user's zone: the alarm was dismissed at that time
    #[serde(default)]
    pub acknowledged: Option<NaiveDateTime>,
}

/// Preset of a reminder setting, kept per user (`practical_user_settings.alarm_presets_json`): the
/// same shape as `Alarm` without idx, text, subject and acknowledgement — those belong to one event.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct AlarmPreset {
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub minutes: i64,
    #[serde(default)]
    pub related_end: bool,
    #[serde(default)]
    pub repeat: i64,
    #[serde(default)]
    pub duration_minutes: i64,
    #[serde(default)]
    pub attendees: Vec<String>,
    #[serde(default)]
    pub attach: String,
    #[serde(default)]
    pub proximity: String,
}

pub const PROXIMITY: &[&str] = &["ARRIVE", "DEPART", "CONNECT", "DISCONNECT"];

/// Snap points of the reminder strip in minutes: before the start and before the end.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AlarmSnaps {
    pub start: Vec<i64>,
    pub end: Vec<i64>,
}

impl Default for AlarmSnaps {
    fn default() -> Self {
        Self { start: vec![2, 5, 15, 30, 60, 180, 480, 1440, 2880, 4320, 10080, 20160], end: vec![5, 10, 15, 30, 60] }
    }
}

/// The snap points from a user's setting or the built-in list.
pub fn alarm_snaps(json: Option<&str>) -> AlarmSnaps {
    json.and_then(|j| serde_json::from_str::<AlarmSnaps>(j).ok()).filter(|s| !s.start.is_empty()).unwrap_or_default()
}

/// A comma-separated list of offsets as typed in the settings ("2 min, 1 h, 1 d, 2 w"; German
/// unit words work too) → ascending, distinct minutes. Errors name the token that did not parse.
pub fn parse_offsets(text: &str) -> Result<Vec<i64>, String> {
    let mut out = vec![];
    for tok in text.split([',', ';', '\n']).map(str::trim).filter(|t| !t.is_empty()) {
        let t = tok.to_lowercase(); // the comma separates entries, so a fraction uses a dot ("1.5 h")
        let split = t.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(t.len());
        let (num, unit) = (t[..split].trim(), t[split..].trim().trim_end_matches('.'));
        let n: f64 = num.parse().map_err(|_| tok.to_string())?;
        let factor = match unit.chars().next() {
            None | Some('m') => 1.0,
            Some('h') | Some('s') => 60.0,
            Some('d') | Some('t') => 1440.0,
            Some('w') => 10080.0,
            _ => return Err(tok.to_string()),
        };
        let minutes = (n * factor).round() as i64;
        if minutes <= 0 || minutes > 366 * 1440 {
            return Err(tok.to_string());
        }
        out.push(minutes);
    }
    out.sort_unstable();
    out.dedup();
    if out.len() > 30 {
        return Err("too many".into());
    }
    Ok(out)
}

/// The inverse of `parse_offsets` for the settings form: "2 min, 1 h, 1 d, 2 w".
pub fn fmt_offsets(list: &[i64]) -> String {
    list.iter().map(|&m| if m % 10080 == 0 { format!("{} w", m / 10080) } else if m % 1440 == 0 { format!("{} d", m / 1440) } else if m % 60 == 0 { format!("{} h", m / 60) } else { format!("{m} min") }).collect::<Vec<_>>().join(", ")
}

/// Rejects a preset list that could not have come from the editor (bounds the stored JSON, too).
pub fn validate_presets(list: &[AlarmPreset]) -> AppResult<()> {
    if list.len() > 50 {
        return Err(AppError::bad("err-too-many-presets"));
    }
    for p in list {
        if one_of(&p.action, &["DISPLAY", "AUDIO", "EMAIL"]).is_none() || (!p.proximity.is_empty() && one_of(&p.proximity, PROXIMITY).is_none()) {
            return Err(AppError::bad("err-invalid-preset"));
        }
        if p.minutes.abs() > 366 * 1440 || p.repeat < 0 || p.repeat > 100 || p.duration_minutes < 0 || p.attach.len() > 1000 || p.attendees.len() > 20 {
            return Err(AppError::bad("err-invalid-preset"));
        }
    }
    Ok(())
}

pub(crate) fn parse_alarm(idx: usize, lines: &[String], tz: chrono_tz::Tz) -> Alarm {
    let mut a = Alarm { idx: Some(idx), action: "DISPLAY".into(), ..Default::default() };
    let mut attach = String::new(); // the first non-binary ATTACH; it is the sound only for AUDIO (ACTION may follow it)
    for l in lines {
        let (name, params, value) = ical::split_line(l);
        let param = |k: &str| params.iter().find(|(n, _)| n == k).map(|(_, v)| v.to_ascii_uppercase());
        match name.as_str() {
            "ACTION" => a.action = value.to_ascii_uppercase(),
            "TRIGGER" => {
                if param("VALUE").as_deref() == Some("DATE-TIME") || (value.ends_with('Z') && !value.starts_with(['P', '-', '+'])) {
                    if let Ok(utc) = NaiveDateTime::parse_from_str(value.trim_end_matches('Z'), "%Y%m%dT%H%M%S") {
                        a.at = Some(chrono::Utc.from_utc_datetime(&utc).with_timezone(&tz).naive_local());
                    }
                } else if let Some(d) = ical::parse_duration(&value) {
                    a.minutes = -d.num_minutes();
                    a.related_end = param("RELATED").as_deref() == Some("END");
                }
            }
            "DESCRIPTION" => a.description = ical::unescape(&value),
            "SUMMARY" => a.summary = ical::unescape(&value),
            "DURATION" => a.duration_minutes = ical::parse_duration(&value).map(|d| d.num_minutes()).unwrap_or(0),
            "REPEAT" => a.repeat = value.trim().parse().unwrap_or(0),
            "ATTENDEE" => a.attendees.push(ical::strip_mailto(value.trim()).to_string()),
            "ATTACH" if attach.is_empty() && !is_binary_attach(&params) => attach = value.trim().to_string(),
            "PROXIMITY" => a.proximity = one_of(&value, PROXIMITY).unwrap_or("").to_string(),
            "ACKNOWLEDGED" => {
                if let Ok(utc) = NaiveDateTime::parse_from_str(value.trim().trim_end_matches('Z'), "%Y%m%dT%H%M%S") {
                    a.acknowledged = Some(chrono::Utc.from_utc_datetime(&utc).with_timezone(&tz).naive_local());
                }
            }
            _ => {}
        }
    }
    if a.action == "AUDIO" {
        a.attach = attach;
    }
    a
}

/// A stored VALARM line `build_valarm` rewrites for this alarm: the managed set, ATTENDEEs of an
/// EMAIL alarm and the (non-binary) sound of an AUDIO alarm; an EMAIL's attachments stay verbatim.
fn alarm_line_managed(line: &str, action: &str) -> bool {
    let name = prop_name(line);
    match name.as_str() {
        "ATTENDEE" => action == "EMAIL",
        "ATTACH" => {
            let (_, params, _) = ical::split_line(line);
            action == "AUDIO" && !is_binary_attach(&params)
        }
        _ => ALARM_MANAGED.contains(&name.as_str()),
    }
}

pub(super) fn fmt_trigger_offset(minutes: i64) -> String {
    if minutes == 0 {
        return "PT0S".into();
    }
    let sign = if minutes > 0 { "-" } else { "" };
    let m = minutes.abs();
    if m % 1440 == 0 { format!("{sign}P{}D", m / 1440) } else if m % 60 == 0 { format!("{sign}PT{}H", m / 60) } else { format!("{sign}PT{m}M") }
}

/// VALARM lines for one alarm: the modelled properties from `a`, the rest from the stored alarm.
pub(crate) fn build_valarm(a: &Alarm, summary: &str, raw: Option<&RawMaster>, tz: chrono_tz::Tz) -> Vec<String> {
    let action = match a.action.to_ascii_uppercase().as_str() { "AUDIO" => "AUDIO", "EMAIL" => "EMAIL", _ => "DISPLAY" };
    let mut l = vec!["BEGIN:VALARM".to_string(), format!("ACTION:{action}")];
    l.push(match a.at {
        Some(at) => format!("TRIGGER;VALUE=DATE-TIME:{}", ical::to_utc(at, tz).format("%Y%m%dT%H%M%SZ")),
        None if a.related_end => format!("TRIGGER;RELATED=END:{}", fmt_trigger_offset(a.minutes)),
        None => format!("TRIGGER:{}", fmt_trigger_offset(a.minutes)),
    });
    let text = if a.description.trim().is_empty() { summary } else { a.description.trim() };
    if action != "AUDIO" {
        l.push(format!("DESCRIPTION:{}", ical::escape(text)));
    }
    let src = a.idx.and_then(|i| raw.and_then(|r| r.alarms.get(i)));
    if action == "EMAIL" {
        let subject = if a.summary.trim().is_empty() { summary } else { a.summary.trim() };
        l.push(format!("SUMMARY:{}", ical::escape(subject)));
        for m in a.attendees.iter().map(|m| m.trim()).filter(|m| m.contains('@')) {
            // the stored line keeps its parameters (CN, …) while the address is unchanged
            let kept = src.and_then(|s| s.iter().find(|x| prop_name(x) == "ATTENDEE" && ical::strip_mailto(ical::split_line(x).2.trim()) == m));
            l.push(kept.cloned().unwrap_or_else(|| format!("ATTENDEE:mailto:{m}")));
        }
    }
    if action == "AUDIO" && !a.attach.trim().is_empty() {
        let uri = a.attach.trim();
        let kept = src.and_then(|s| s.iter().find(|x| prop_name(x) == "ATTACH" && ical::split_line(x).2.trim() == uri));
        l.push(kept.cloned().unwrap_or_else(|| format!("ATTACH:{uri}"))); // a kept line carries its FMTTYPE
    }
    if a.repeat > 0 && a.duration_minutes > 0 {
        l.push(format!("DURATION:{}", fmt_trigger_offset(-a.duration_minutes)));
        l.push(format!("REPEAT:{}", a.repeat));
    }
    if let Some(p) = one_of(&a.proximity, PROXIMITY) {
        l.push(format!("PROXIMITY:{p}"));
    }
    if let Some(ack) = a.acknowledged {
        l.push(format!("ACKNOWLEDGED:{}", ical::to_utc(ack, tz).format("%Y%m%dT%H%M%SZ")));
    }
    if let Some(src) = src {
        l.extend(src.iter().filter(|x| !alarm_line_managed(x, action)).cloned());
    }
    l.push("END:VALARM".into());
    l
}
