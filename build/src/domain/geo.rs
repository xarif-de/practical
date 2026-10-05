//! Address suggestions and geocoding behind a small proxy: Photon (komoot, OSM data) for
//! search-as-you-type and coordinates, Nominatim (OSMF) for coordinates → address. The proxy
//! exists because the Nominatim policy demands an identifying User-Agent (a browser cannot set
//! one) and at most one request per second, and because the users' IPs and typed text should
//! not leave the house. Each provider is throttled to one request per second and answers are
//! cached, so typing pauses and button clicks are all the providers ever see.

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const PHOTON: &str = "https://photon.komoot.io";
const NOMINATIM: &str = "https://nominatim.openstreetmap.org";
const USER_AGENT: &str = concat!("PractiCal/", env!("CARGO_PKG_VERSION"), " (+https://git.xarif.de/docker/practical)");
const CACHE_MAX: usize = 1000;
const CACHE_TTL: Duration = Duration::from_secs(7 * 24 * 3600);

#[derive(Default)]
pub struct Geo {
    /// last request per provider — the 1 req/s throttle
    last: Mutex<HashMap<&'static str, Instant>>,
    cache: Mutex<HashMap<String, (Instant, Vec<Place>)>>,
}

/// One address with coordinates, as the editor consumes it: `label` is the text for the
/// location field (no country when it is the instance's own).
#[derive(Debug, Clone, Default, serde::Serialize, PartialEq)]
pub struct Place {
    pub label: String,
    pub name: String,
    pub street: String,
    pub housenumber: String,
    pub postcode: String,
    pub city: String,
    pub country: String,
    pub country_code: String,
    pub lat: f64,
    pub lon: f64,
}

impl Place {
    fn finish(mut self, home_country: &str) -> Self {
        let mut parts: Vec<String> = vec![];
        if !self.name.is_empty() && self.name != self.street && self.name != self.city {
            parts.push(self.name.clone());
        }
        let street = [self.street.as_str(), self.housenumber.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" ");
        if !street.is_empty() {
            parts.push(street);
        }
        let city = [self.postcode.as_str(), self.city.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" ");
        if !city.is_empty() {
            parts.push(city);
        }
        if !self.country.is_empty() && !self.country_code.eq_ignore_ascii_case(home_country) {
            parts.push(self.country.clone());
        }
        self.label = parts.join(", ");
        self
    }
}

impl Geo {
    async fn throttle(&self, provider: &'static str) {
        let mut last = self.last.lock().await;
        if let Some(t) = last.get(provider) {
            let wait = Duration::from_millis(1100).saturating_sub(t.elapsed());
            if !wait.is_zero() {
                tokio::time::sleep(wait).await;
            }
        }
        last.insert(provider, Instant::now());
    }

    async fn cached(&self, key: &str) -> Option<Vec<Place>> {
        let c = self.cache.lock().await;
        c.get(key).filter(|(t, _)| t.elapsed() < CACHE_TTL).map(|(_, v)| v.clone())
    }

    async fn remember(&self, key: String, v: Vec<Place>) {
        let mut c = self.cache.lock().await;
        if c.len() >= CACHE_MAX {
            // drop the oldest quarter rather than scanning on every insert
            let mut ages: Vec<(Instant, String)> = c.iter().map(|(k, (t, _))| (*t, k.clone())).collect();
            ages.sort();
            for (_, k) in ages.into_iter().take(CACHE_MAX / 4) {
                c.remove(&k);
            }
        }
        c.insert(key, (Instant::now(), v));
    }
}

fn s(v: &serde_json::Value, k: &str) -> String {
    v.get(k).and_then(|x| x.as_str()).map(str::trim).unwrap_or("").to_string()
}

/// Photon `/api` → places; identical labels collapse (Photon lists an OSM object once per tag it
/// matched, and a street once per segment — a second "An der Frauenkirche, 01067 Dresden" helps nobody).
pub fn parse_photon(body: &serde_json::Value, home_country: &str) -> Vec<Place> {
    let mut out: Vec<Place> = vec![];
    for f in body.get("features").and_then(|x| x.as_array()).into_iter().flatten() {
        let p = f.get("properties").cloned().unwrap_or_default();
        let coords = f.get("geometry").and_then(|g| g.get("coordinates")).and_then(|c| c.as_array()).cloned().unwrap_or_default();
        let (Some(lon), Some(lat)) = (coords.first().and_then(|x| x.as_f64()), coords.get(1).and_then(|x| x.as_f64())) else { continue };
        let place = Place {
            name: s(&p, "name"),
            street: s(&p, "street"),
            housenumber: s(&p, "housenumber"),
            postcode: s(&p, "postcode"),
            city: [s(&p, "city"), s(&p, "town"), s(&p, "village"), s(&p, "county")].into_iter().find(|x| !x.is_empty()).unwrap_or_default(),
            country: s(&p, "country"),
            country_code: s(&p, "countrycode"),
            lat,
            lon,
            ..Default::default()
        }
        .finish(home_country);
        if place.label.is_empty() || out.iter().any(|o| o.label == place.label) {
            continue;
        }
        out.push(place);
    }
    out
}

/// Nominatim `/reverse?format=jsonv2` → one place, or none when it could not geocode (sea).
pub fn parse_nominatim(body: &serde_json::Value, home_country: &str) -> Option<Place> {
    if body.get("error").is_some() {
        return None;
    }
    let a = body.get("address").cloned().unwrap_or_default();
    let lat: f64 = s(body, "lat").parse().ok()?;
    let lon: f64 = s(body, "lon").parse().ok()?;
    let place = Place {
        name: s(body, "name"),
        street: [s(&a, "road"), s(&a, "pedestrian"), s(&a, "footway"), s(&a, "square")].into_iter().find(|x| !x.is_empty()).unwrap_or_default(),
        housenumber: s(&a, "house_number"),
        postcode: s(&a, "postcode"),
        city: [s(&a, "city"), s(&a, "town"), s(&a, "village"), s(&a, "municipality"), s(&a, "county")].into_iter().find(|x| !x.is_empty()).unwrap_or_default(),
        country: s(&a, "country"),
        country_code: s(&a, "country_code"),
        lat,
        lon,
        ..Default::default()
    }
    .finish(home_country);
    (!place.label.is_empty()).then_some(place)
}

/// RFC 3986 percent-encoding of a query value (reqwest is built without its `query` feature).
fn enc(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    for b in v.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') { out.push(b as char) } else { out.push_str(&format!("%{b:02X}")) }
    }
    out
}

async fn get_json(state: &AppState, url: &str, query: &[(&str, String)]) -> AppResult<serde_json::Value> {
    let url = format!("{url}?{}", query.iter().map(|(k, v)| format!("{k}={}", enc(v))).collect::<Vec<_>>().join("&"));
    let resp = state.http.get(&url).header("Accept", "application/json").header("User-Agent", USER_AGENT).send().await.map_err(|e| AppError::Internal(format!("geocoder: {e}")))?;
    if !resp.status().is_success() {
        return Err(AppError::Internal(format!("geocoder: HTTP {}", resp.status())));
    }
    resp.json().await.map_err(|e| AppError::Internal(format!("geocoder: {e}")))
}

/// Suggestions for a typed address (Photon), biased to `near` when the editor knows a position;
/// `home` is the user's country, left out of the labels of its own addresses.
pub async fn search(state: &AppState, q: &str, lang: &str, near: Option<(f64, f64)>, home: &str) -> AppResult<Vec<Place>> {
    let q = q.trim();
    if q.chars().count() < 3 || q.len() > 200 {
        return Ok(vec![]);
    }
    let lang = if matches!(lang, "de" | "en" | "fr" | "it") { lang } else { "en" };
    let key = format!("s|{lang}|{home}|{}", q.to_lowercase());
    if let Some(v) = state.geo.cached(&key).await {
        return Ok(v);
    }
    let mut query = vec![("q", q.to_string()), ("limit", "6".into()), ("lang", lang.into())];
    if let Some((lat, lon)) = near {
        query.push(("lat", format!("{lat:.4}")));
        query.push(("lon", format!("{lon:.4}")));
    }
    state.geo.throttle("photon").await;
    let body = get_json(state, &format!("{PHOTON}/api/"), &query).await?;
    let places = parse_photon(&body, home);
    state.geo.remember(key, places.clone()).await;
    Ok(places)
}

/// The address at a position (Nominatim), rounded to ~10 m so nearby taps share a cache entry.
pub async fn reverse(state: &AppState, lat: f64, lon: f64, lang: &str, home: &str) -> AppResult<Option<Place>> {
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        return Err(AppError::BadRequest("lat/lon out of range".into()));
    }
    let key = format!("r|{lang}|{home}|{lat:.4}|{lon:.4}");
    if let Some(v) = state.geo.cached(&key).await {
        return Ok(v.into_iter().next());
    }
    let query = [("lat", format!("{lat:.6}")), ("lon", format!("{lon:.6}")), ("format", "jsonv2".into()), ("zoom", "18".into()), ("accept-language", lang.into())];
    state.geo.throttle("nominatim").await;
    let body = get_json(state, &format!("{NOMINATIM}/reverse"), &query).await?;
    let place = parse_nominatim(&body, home);
    state.geo.remember(key, place.clone().into_iter().collect()).await;
    Ok(place)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn photon_features_become_labelled_places() {
        let body: serde_json::Value = serde_json::json!({ "features": [
            { "geometry": { "coordinates": [13.7383838, 51.0509304] }, "properties": { "name": "Kulturpalast", "street": "Schloßstraße", "housenumber": "2", "postcode": "01067", "city": "Dresden", "country": "Deutschland", "countrycode": "DE", "osm_key": "emergency" } },
            { "geometry": { "coordinates": [13.7383838, 51.0509304] }, "properties": { "name": "Kulturpalast", "street": "Schloßstraße", "housenumber": "2", "postcode": "01067", "city": "Dresden", "country": "Deutschland", "countrycode": "DE", "osm_key": "amenity" } },
            { "geometry": { "coordinates": [16.3738, 48.2082] }, "properties": { "name": "Wien", "city": "Wien", "country": "Österreich", "countrycode": "AT" } }
        ] });
        let p = parse_photon(&body, "DE");
        assert_eq!(p.len(), 2, "the duplicate collapses");
        assert_eq!(p[0].label, "Kulturpalast, Schloßstraße 2, 01067 Dresden");
        assert_eq!((p[0].lat, p[0].lon), (51.0509304, 13.7383838));
        assert_eq!(p[1].label, "Wien, Österreich", "foreign country named, own name not doubled");
    }

    #[test]
    fn query_values_are_percent_encoded() {
        assert_eq!(enc("Schloßstraße 2, Dresden"), "Schlo%C3%9Fstra%C3%9Fe%202%2C%20Dresden");
    }

    #[test]
    fn nominatim_reverse_and_sea() {
        let body = serde_json::json!({ "lat": "51.0504070", "lon": "13.7372620", "name": "Tiefgarage Altmarkt", "address": { "amenity": "Tiefgarage Altmarkt", "road": "Wilsdruffer Straße", "city": "Dresden", "postcode": "01067", "country": "Deutschland", "country_code": "de" } });
        let p = parse_nominatim(&body, "DE").unwrap();
        assert_eq!(p.label, "Tiefgarage Altmarkt, Wilsdruffer Straße, 01067 Dresden");
        assert!(parse_nominatim(&serde_json::json!({ "error": "Unable to geocode" }), "DE").is_none());
    }
}
