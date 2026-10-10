//! Characterisation of the per-user settings: defaults, region fallbacks, the stored round trip.

use super::*;
use crate::testkit::{TestApp, config};

#[test]
fn derived_values() {
    let cfg = config("/x");
    let d = Prefs::defaults();
    assert!(d.holidays_enabled && d.school_holidays_enabled && d.lang.is_none());
    assert_eq!(d.history_days(), 90);
    for (v, days) in [(Some(0), 0), (Some(-1), -1), (Some(365), 365), (Some(5), 90), (None, 90)] {
        assert_eq!(Prefs { search_history_days: v, ..Default::default() }.history_days(), days);
    }
    assert_eq!(d.holiday_css(), "");
    assert_eq!(Prefs { school_color: Some("#00ff00".into()), holiday_color: Some("red".into()), ..Default::default() }.holiday_css(), ":root{--fer:#00ff00;}");
    // regions: the lists, else the user's own single region of before — and otherwise NONE. The
    // configured HOLIDAYS_COUNTRY/SUBDIVISION are no fallback here: holidays are nobody's default,
    // and an unasked one could not be told from a choice.
    let empty: [&str; 0] = [];
    assert_eq!(d.regions(false), empty, "a fresh user has no region, whatever the server is configured for");
    assert_eq!(d.regions(true), empty);
    let at = Prefs { holiday_country: Some("AT".into()), ..Default::default() };
    assert_eq!(at.regions(false), ["AT"], "a region the user chose before the lists still counts");
    let whole = Prefs { holiday_country: Some("DE".into()), holiday_subdivision: Some(String::new()), ..Default::default() };
    assert_eq!(whole.regions(false), ["DE"], "the whole country is a choice too");
    let lists = Prefs { public_regions: Some("DE-BY,AT".into()), school_regions: Some(String::new()), ..Default::default() };
    assert_eq!((lists.regions(false), lists.regions(true)), (vec!["DE-BY".to_string(), "AT".into()], vec![]));
    assert_eq!((lists.own_region().as_deref(), lists.home_country(&cfg)), (Some("DE-BY"), "DE".to_string()));
    let only_school = Prefs { public_regions: Some(String::new()), school_regions: Some("FR-75".into()), ..Default::default() };
    assert_eq!((only_school.own_region().as_deref(), only_school.home_country(&cfg)), (Some("FR-75"), "FR".to_string()));
    let none = Prefs { public_regions: Some(String::new()), school_regions: Some(String::new()), ..Default::default() };
    assert_eq!((none.own_region(), none.home_country(&cfg)), (None, "DE".to_string()),
        "without a region the address search still prefers the configured country");
    assert!(is_hex_color("#a1B2c3") && !is_hex_color("#a1b2c") && !is_hex_color("a1b2c3d") && !is_hex_color("#gggggg"));
    // the rule of before v2 and its legacy flag
    assert_eq!(VacationRule::of(&d), VacationRule { v: 0, kind: "category".into(), value: "Urlaub".into() });
    assert!(VacationRule::of(&d).legacy());
    assert!(!VacationRule { v: 0, kind: "calendar".into(), value: String::new() }.legacy());
    assert_eq!(VacationRule::of(&Prefs { vacation_rule_json: Some("broken".into()), ..Default::default() }).value, "Urlaub");
}

#[tokio::test]
async fn stored_round_trip() {
    let app = TestApp::new().await;
    assert_eq!(serde_json::to_value(get(&app.state.db, "nobody").await.unwrap()).unwrap(), serde_json::to_value(Prefs::defaults()).unwrap());
    assert_eq!(get_look(&app.state.db, "nobody").await.unwrap(), Look::default());
    let p = Prefs {
        lang: Some("fr".into()), scheme: Some("dark".into()), timezone: Some("Asia/Tokyo".into()), holidays_enabled: false, school_holidays_enabled: true,
        holiday_country: Some("AT".into()), holiday_subdivision: Some("AT-9".into()), public_regions: Some("AT-9".into()), school_regions: Some(String::new()),
        holiday_color: Some("#111111".into()), school_color: None, vacation_days: Some(25), vacation_rule_json: Some("{}".into()), contact_columns_json: Some("[]".into()),
        contact_list_fields_json: Some("[\"note\"]".into()), alarm_presets_json: Some("[]".into()), alarm_snaps_json: None, task_columns_json: Some("[\"due\"]".into()), search_history_days: Some(-1),
    };
    set(&app.state.db, "u", &p).await.unwrap();
    assert_eq!(serde_json::to_value(get(&app.state.db, "u").await.unwrap()).unwrap(), serde_json::to_value(&p).unwrap());
    assert_eq!(get_look(&app.state.db, "u").await.unwrap(), Look { lang: Some("fr".into()), scheme: Some("dark".into()), onboarding: None });
    // an update replaces every column
    set(&app.state.db, "u", &Prefs::defaults()).await.unwrap();
    assert_eq!(serde_json::to_value(get(&app.state.db, "u").await.unwrap()).unwrap(), serde_json::to_value(Prefs::defaults()).unwrap());
}

/// A column dropped from the live table while a connection still holds the prepared read (how
/// 2026-10-05's hand-applied `DROP COLUMN presets_json` met the running pod): the read goes on. With
/// `SELECT *` the second read panics — the columns after the dropped one have moved, the cached
/// name → index map has not (here: `timezone` decoded from an INTEGER column; in production the
/// last name's index ran past the row).
#[tokio::test]
async fn a_dropped_column_does_not_break_the_cached_read() {
    let app = TestApp::new().await;
    let p = Prefs { timezone: Some("Europe/Vienna".into()), search_history_days: Some(7), ..Prefs::defaults() };
    set(&app.state.db, "alice", &p).await.unwrap();
    // the reads on one connection, so the second meets the statement the first prepared; the
    // schema changes from another (in production: sqlite3 by hand)
    let url = format!("sqlite://{}", app.state.config.db_path);
    let pool = || sqlx::sqlite::SqlitePoolOptions::new().max_connections(1).connect(&url);
    let (one, other) = (pool().await.unwrap(), pool().await.unwrap());
    // a column in the middle, as presets_json was
    let rest: Vec<String> = sqlx::query("SELECT name FROM pragma_table_info('practical_user_settings') WHERE name != 'principal'").fetch_all(&other).await.unwrap().iter().map(|r| r.get("name")).collect();
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("ALTER TABLE practical_user_settings RENAME TO s_old; CREATE TABLE practical_user_settings AS SELECT principal, NULL AS gone_soon, {} FROM s_old; DROP TABLE s_old;", rest.join(", ")))).execute(&other).await.unwrap();
    let read = || async { get(&one, "alice").await.map(|p| (p.timezone, p.search_history_days)) };
    assert_eq!(read().await.unwrap(), (Some("Europe/Vienna".into()), Some(7)));
    sqlx::query("ALTER TABLE practical_user_settings DROP COLUMN gone_soon").execute(&other).await.unwrap();
    assert_eq!(read().await.unwrap(), (Some("Europe/Vienna".into()), Some(7)));
}
