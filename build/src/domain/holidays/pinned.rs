//! Characterisation of the holiday layer with the network blocked: the cache, the merge of several
//! regions into day marks, region descriptions and the sources named per country.

use super::*;
use crate::testkit::{TestApp, date};

async fn put(app: &TestApp, sub: &str, year: i32, key: &str, list: serde_json::Value, fetched: &str) {
    sqlx::query("INSERT OR REPLACE INTO practical_holidays (country, subdivision, year, kind, json, fetched_at) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(&sub[..2]).bind(sub).bind(year).bind(key).bind(list.to_string()).bind(fetched).execute(&app.state.db).await.unwrap();
}

fn h(start: &str, end: &str, name: &str) -> serde_json::Value {
    serde_json::json!({ "start": start, "end": end, "name": name, "kind": "school", "nationwide": false, "source": "openholidays", "regions": [] })
}

#[tokio::test]
async fn cache_fresh_stale_and_off() {
    let app = TestApp::new().await;
    // the seeded rows are fresh: answered from the cache
    let got = get(&app.state, "public", "DE", "DE-SN", 2026, "de").await.unwrap();
    assert_eq!(got.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["Tag der Deutschen Einheit", "Reformationstag", "Buß- und Bettag", "1. Weihnachtsfeiertag"]);
    // a stale row is asked again; the source is unreachable, so the stale answer stays
    put(&app, "DE-BY", 2026, "public2-de", serde_json::json!([{ "start": "2026-11-01", "end": "2026-11-01", "name": "Allerheiligen", "kind": "public" }]), "2000-01-01 00:00:00").await;
    let t = std::time::Instant::now();
    let stale = get(&app.state, "public", "DE", "DE-BY", 2026, "de").await.unwrap();
    assert_eq!(stale.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["Allerheiligen"]);
    assert!(t.elapsed() < std::time::Duration::from_secs(10));
    // nothing cached and nothing reachable: none
    assert!(get(&app.state, "public", "DE", "DE-HE", 2026, "de").await.unwrap().is_empty());
    assert!(get(&app.state, "public", "", "", 2026, "de").await.unwrap().is_empty());
    assert!(get(&app.state, "school", "XX", "", 2026, "de").await.unwrap().is_empty(), "no source covers XX");
    let off = TestApp::with(|c| c.holidays_enabled = false).await;
    assert!(get(&off.state, "public", "DE", "DE-SN", 2026, "de").await.unwrap().is_empty());
}

#[tokio::test]
async fn marks_of_several_regions_and_years() {
    let app = TestApp::new().await;
    put(&app, "DE-BY", 2026, "school2-de", serde_json::json!([h("2026-10-05", "2026-10-17", "Herbstferien"), h("2026-12-24", "2027-01-05", "Weihnachtsferien")]), "2999-01-01 00:00:00").await;
    put(&app, "DE-BY", 2027, "school2-de", serde_json::json!([h("2026-12-24", "2027-01-05", "Weihnachtsferien")]), "2999-01-01 00:00:00").await;
    let mut p = crate::domain::prefs::Prefs::defaults();
    p.school_regions = Some("DE-SN,DE-BY".into());
    crate::domain::prefs::set(&app.state.db, "u", &p).await.unwrap();
    let m = marks(&app.state, &crate::domain::prefs::get(&app.state.db, "u").await.unwrap(), date("2026-10-01"), date("2027-01-03"), "de").await.unwrap();
    let day = |d: &str| m.get(&date(d)).map(|v| v.iter().map(|x| format!("{}{}{}", if x.school { "S:" } else { "P:" }, x.name, if x.tags.is_empty() { String::new() } else { format!(" ({})", x.tags.join(",")) })).collect::<Vec<_>>()).unwrap_or_default();
    assert_eq!(day("2026-10-03"), ["P:Tag der Deutschen Einheit"]);
    assert_eq!(day("2026-10-05"), ["S:Herbstferien"], "both regions: no tag");
    assert_eq!(day("2026-12-21"), ["S:Weihnachtsferien (SN)"]);
    assert_eq!(day("2026-12-25"), ["P:1. Weihnachtsfeiertag", "S:Weihnachtsferien (SN)", "S:Weihnachtsferien (BY)"]);
    assert_eq!(day("2027-01-02"), ["S:Weihnachtsferien (SN)", "S:Weihnachtsferien (BY)"], "a block across New Year counts once");
    assert!(day("2027-01-03").is_empty(), "the end of the window is exclusive");
    assert_eq!(m.get(&date("2026-12-25")).unwrap()[2].start, date("2026-12-24"));
    // switched off
    p.holidays_enabled = false;
    p.school_holidays_enabled = false;
    crate::domain::prefs::set(&app.state.db, "u", &p).await.unwrap();
    assert!(marks(&app.state, &crate::domain::prefs::get(&app.state.db, "u").await.unwrap(), date("2026-10-01"), date("2027-01-03"), "de").await.unwrap().is_empty());
}

#[tokio::test]
async fn regions_named_and_sourced() {
    let app = TestApp::new().await;
    assert_eq!(describe(&app.state, "DE", "de").await.unwrap(), ("Deutschland".to_string(), String::new()));
    assert_eq!(describe(&app.state, "DE-SN", "de").await.unwrap(), ("Sachsen".to_string(), "Deutschland".to_string()));
    assert_eq!(describe(&app.state, "DE-SN", "en").await.unwrap(), ("Saxony".to_string(), "Germany".to_string()));
    // not cached, unreachable: the built-in name
    assert_eq!(describe(&app.state, "DE-BY", "fr").await.unwrap().0, region_name("DE-BY", "fr").unwrap());
    assert_eq!(describe(&app.state, "QQ-1", "de").await.unwrap(), ("QQ-1".to_string(), "QQ".to_string()));
    assert_eq!(sources("DE"), (vec!["OpenHolidays", "Nager.Date"], vec!["OpenHolidays"]));
    assert_eq!(sources("US"), (vec!["Nager.Date"], vec![]));
    assert_eq!(sources("QQ"), (vec![], vec![]));
    assert!(has_school("DE") && !has_school("US"));
    assert_eq!(split("DE"), ("DE", ""));
    assert_eq!(split("D"), ("D", ""));
    assert_eq!(Holiday { start: date("2026-01-01"), end: date("2026-01-01"), name: "N".into(), kind: "public".into(), nationwide: true, source: String::new(), regions: vec![], tags: vec!["SN".into(), "BY".into()] }.label(), "N (SN, BY)");
    assert_eq!(shorten(&["DE-SN".into(), "DE".into()], &["DE-SN".into(), "DE-BY".into()]), ["SN", "DE"]);
    assert_eq!(pick(&[ApiName { language: "FR".into(), text: "f".into() }, ApiName { language: "EN".into(), text: "e".into() }], "de"), "e");
    assert_eq!(pick(&[], "de"), "");
    assert_eq!(cldr_version("release-48-2"), [48, 2]);
    let c = countries("en");
    assert!(c.windows(2).all(|w| fold(&w[0].name) <= fold(&w[1].name)));
    assert!(c.iter().any(|r| r.code == "DE" && r.name == "Germany" && r.school));
}
