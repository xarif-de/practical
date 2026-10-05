//! The server's "now". Production: the system clock, nothing else. Development only
//! (`PRACTICAL__DEV_NOW`, honoured together with `PRACTICAL__DEV_ASSETS_DIR`, config.rs): one FIXED
//! instant for every "today"/timestamp the server derives, so the e2e suite (e2e/) renders the same
//! pages on every run. Fixed, not ticking: an "%H:%M" string never crosses a minute mid-run.

use chrono::{DateTime, Utc};
use std::sync::OnceLock;

static FIXED: OnceLock<DateTime<Utc>> = OnceLock::new();

/// Freezes `now()` for the rest of the process (serve(), once, from `Config::dev_now`).
pub fn fix(at: DateTime<Utc>) {
    let _ = FIXED.set(at);
}

pub fn now() -> DateTime<Utc> {
    resolve(FIXED.get().copied())
}

/// The user's calendar day in `tz` at `now()`.
pub fn today_in(tz: &chrono_tz::Tz) -> chrono::NaiveDate {
    now().with_timezone(tz).date_naive()
}

fn resolve(fixed: Option<DateTime<Utc>>) -> DateTime<Utc> {
    fixed.unwrap_or_else(Utc::now)
}

/// `PRACTICAL__DEV_NOW` (RFC 3339) → the fixed instant; rejected outside development mode.
pub fn parse_dev_now(raw: Option<&str>, dev: bool) -> Result<Option<DateTime<Utc>>, String> {
    let Some(raw) = raw else { return Ok(None) };
    if !dev {
        return Err("PRACTICAL__DEV_NOW is only honoured together with PRACTICAL__DEV_ASSETS_DIR (development)".into());
    }
    DateTime::parse_from_rfc3339(raw).map(|t| Some(t.with_timezone(&Utc))).map_err(|e| format!("PRACTICAL__DEV_NOW: expected RFC 3339, got {raw:?}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_is_the_system_clock_and_set_is_fixed() {
        let before = Utc::now();
        let t = resolve(None);
        assert!(t >= before && t <= Utc::now());
        let at = DateTime::parse_from_rfc3339("2026-10-14T08:00:00Z").unwrap().with_timezone(&Utc);
        assert_eq!(resolve(Some(at)), at);
        assert_eq!(resolve(Some(at)), at, "a fixed clock does not tick");
    }

    #[test]
    fn dev_now_parsing_and_gate() {
        assert_eq!(parse_dev_now(None, false), Ok(None));
        assert_eq!(parse_dev_now(None, true), Ok(None));
        let t = parse_dev_now(Some("2026-10-14T10:00:00+02:00"), true).unwrap().unwrap();
        assert_eq!(t.to_rfc3339(), "2026-10-14T08:00:00+00:00");
        assert_eq!(t.with_timezone(&chrono_tz::Europe::Berlin).date_naive().to_string(), "2026-10-14");
        assert!(parse_dev_now(Some("2026-10-14T08:00:00Z"), false).unwrap_err().contains("DEV_ASSETS_DIR"));
        assert!(parse_dev_now(Some("2026-10-14 08:00"), true).is_err());
    }
}
