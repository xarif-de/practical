//! The history document's time format.

use super::*;

#[test]
fn local_times() {
    let tz: chrono_tz::Tz = "Europe/Berlin".parse().unwrap();
    assert_eq!(local("2026-10-14 08:00:00", tz), "2026-10-14T10:00");
    assert_eq!(local("2026-01-14 08:00:00", tz), "2026-01-14T09:00");
    assert_eq!(local("2026-10-14T08:00:00Z", tz), "2026-10-14T08:00", "another shape is passed on, cut to minutes");
    assert_eq!(local("x", tz), "x");
    assert_eq!(ORDER.len(), ORDER.iter().collect::<std::collections::BTreeSet<_>>().len(), "no key twice");
}
