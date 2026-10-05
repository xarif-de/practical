//! Static assets compiled into the binary (eight files; no rust-embed needed).
//! Development (`PRACTICAL__DEV_ASSETS_DIR`): read from that directory on every request, uncached.

use crate::state::SharedState;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use std::sync::OnceLock;

struct Asset {
    name: &'static str,
    mime: &'static str,
    body: &'static [u8],
}

const ASSETS: &[Asset] = &[
    Asset { name: "style.css", mime: "text/css; charset=utf-8", body: include_bytes!("../../assets/style.css") },
    Asset { name: "app.js", mime: "text/javascript; charset=utf-8", body: include_bytes!("../../assets/app.js") },
    // the change history page only (templates/history.html)
    Asset { name: "history.js", mime: "text/javascript; charset=utf-8", body: include_bytes!("../../assets/history.js") },
    Asset { name: "history.css", mime: "text/css; charset=utf-8", body: include_bytes!("../../assets/history.css") },
    Asset { name: "sw.js", mime: "text/javascript; charset=utf-8", body: include_bytes!("../../assets/sw.js") },
    Asset { name: "icon.svg", mime: "image/svg+xml", body: include_bytes!("../../assets/icon.svg") },
    // Leaflet 1.9.4 (BSD-2), vendored; fetched by the browser only when the map dialog opens
    Asset { name: "leaflet.js", mime: "text/javascript; charset=utf-8", body: include_bytes!("../../assets/leaflet.js") },
    Asset { name: "leaflet.css", mime: "text/css; charset=utf-8", body: include_bytes!("../../assets/leaflet.css") },
];

fn etags() -> &'static Vec<String> {
    static E: OnceLock<Vec<String>> = OnceLock::new();
    E.get_or_init(|| {
        use base64ct::Encoding;
        use sha2::{Digest, Sha256};
        ASSETS
            .iter()
            .map(|a| {
                let h = Sha256::digest(a.body);
                format!("\"{}\"", base64ct::Base64UrlUnpadded::encode_string(&h[..12]))
            })
            .collect()
    })
}

pub fn asset_version() -> &'static str {
    // cache-busting query for <link>/<script> tags: changes whenever any asset changes
    static V: OnceLock<String> = OnceLock::new();
    V.get_or_init(|| etags().iter().map(|e| &e[1..5]).collect::<String>())
}

fn finish(mut r: Response, a: &Asset, cache: &'static str, etag: Option<&str>) -> Response {
    let h = r.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static(a.mime));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    if let Some(etag) = etag {
        h.insert(header::ETAG, HeaderValue::from_str(etag).unwrap());
    }
    if a.name == "sw.js" {
        h.insert("Service-Worker-Allowed", HeaderValue::from_static("/"));
    }
    r
}

async fn serve(State(state): State<SharedState>, Path(name): Path<String>, headers: axum::http::HeaderMap) -> Response {
    let Some((i, a)) = ASSETS.iter().enumerate().find(|(_, a)| a.name == name) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if let Some(dir) = &state.config.dev_assets_dir {
        return match tokio::fs::read(std::path::Path::new(dir).join(a.name)).await {
            Ok(body) => finish(body.into_response(), a, "no-store", None),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("{dir}/{}: {e}", a.name)).into_response(),
        };
    }
    let etag = &etags()[i];
    if headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) == Some(etag.as_str()) {
        return StatusCode::NOT_MODIFIED.into_response();
    }
    finish(a.body.into_response(), a, "public, max-age=86400, must-revalidate", Some(etag))
}

pub fn router() -> Router<SharedState> {
    Router::new().route("/assets/{name}", get(serve))
}
