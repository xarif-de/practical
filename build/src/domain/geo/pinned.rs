//! Characterisation of the geocoder proxy: labels, the cache in front of the providers, the route.

use super::*;
use crate::testkit::TestApp;

#[test]
fn labels() {
    let p = |name: &str, street: &str, nr: &str, code: &str, city: &str, country: &str, cc: &str| Place { name: name.into(), street: street.into(), housenumber: nr.into(), postcode: code.into(), city: city.into(), country: country.into(), country_code: cc.into(), ..Default::default() }.finish("DE").label;
    assert_eq!(p("", "", "", "", "", "", ""), "");
    assert_eq!(p("Dresden", "", "", "", "Dresden", "Deutschland", "de"), "Dresden", "home country by code, any case");
    assert_eq!(p("Hauptstraße", "Hauptstraße", "5", "", "", "", ""), "Hauptstraße 5", "a name equal to the street is not repeated");
    assert_eq!(p("", "", "", "01067", "", "France", "FR"), "01067, France");
    assert_eq!(enc("a-b_c.d~e/f?"), "a-b_c.d~e%2Ff%3F");
    assert_eq!(parse_photon(&serde_json::json!({}), "DE"), vec![]);
    assert_eq!(parse_photon(&serde_json::json!({ "features": [{ "properties": { "name": "x" } }] }), "DE"), vec![], "no coordinates");
    let town = parse_photon(&serde_json::json!({ "features": [{ "geometry": { "coordinates": [1.0, 2.0] }, "properties": { "name": "Hof", "town": "Kleinstadt", "county": "Kreis" } }] }), "DE");
    assert_eq!(town[0].label, "Hof, Kleinstadt");
    assert!(parse_nominatim(&serde_json::json!({ "lat": "x", "lon": "1" }), "DE").is_none());
    assert_eq!(parse_nominatim(&serde_json::json!({ "lat": "1", "lon": "2", "address": { "pedestrian": "Markt", "village": "Dorf", "country": "Österreich", "country_code": "at" } }), "DE").unwrap().label, "Markt, Dorf, Österreich");
}

#[tokio::test]
async fn cached_answers_through_the_route() {
    let app = TestApp::new().await;
    app.user("alice", "Alice").await;
    let place = Place { label: "Kulturpalast, Dresden".into(), name: "Kulturpalast".into(), city: "Dresden".into(), country_code: "DE".into(), lat: 51.05, lon: 13.74, ..Default::default() };
    // keys as `search` / `reverse` build them: language, home country, the folded query or the rounded position
    app.state.geo.remember("s|de|DE|kulturpalast".into(), vec![place.clone()]).await;
    app.state.geo.remember("r|de|DE|51.0500|13.7400".into(), vec![place.clone()]).await;
    app.state.geo.remember("r|de|DE|0.0000|0.0000".into(), vec![]).await;
    let mut c = app.login("alice").await;
    let r = c.get_json("/api/geo/search?q=%20KulturPalast%20").await.ok().json();
    assert_eq!(r, serde_json::json!([{ "label": "Kulturpalast, Dresden", "name": "Kulturpalast", "street": "", "housenumber": "", "postcode": "", "city": "Dresden", "country": "", "country_code": "DE", "lat": 51.05, "lon": 13.74 }]));
    assert_eq!(c.get_json("/api/geo/reverse?lat=51.05001&lon=13.74002").await.ok().json()["label"], "Kulturpalast, Dresden");
    assert_eq!(c.get_json("/api/geo/reverse?lat=0&lon=0").await.status, axum::http::StatusCode::NOT_FOUND, "the sea: no address");
    // a language without Photon support asks in English (a different key: not cached here)
    assert_eq!(c.get_json(&format!("/api/geo/search?q={}", "x".repeat(201))).await.ok().json(), serde_json::json!([]), "too long: nothing");
    assert!(app.state.geo.cached("s|de|DE|kulturpalast").await.is_some());
    assert!(app.state.geo.cached("s|en|DE|kulturpalast").await.is_none());
}
