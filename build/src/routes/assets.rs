//! Static assets compiled into the binary (three files; no rust-embed needed).

use axum::Router;
use axum::extract::Path;
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
    Asset { name: "sw.js", mime: "text/javascript; charset=utf-8", body: include_bytes!("../../assets/sw.js") },
    Asset { name: "icon.svg", mime: "image/svg+xml", body: include_bytes!("../../assets/icon.svg") },
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

async fn serve(Path(name): Path<String>, headers: axum::http::HeaderMap) -> Response {
    let Some((i, a)) = ASSETS.iter().enumerate().find(|(_, a)| a.name == name) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let etag = &etags()[i];
    if headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) == Some(etag.as_str()) {
        return StatusCode::NOT_MODIFIED.into_response();
    }
    let mut r = a.body.into_response();
    let h = r.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static(a.mime));
    h.insert(header::ETAG, HeaderValue::from_str(etag).unwrap());
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("public, max-age=86400, must-revalidate"));
    if a.name == "sw.js" {
        h.insert("Service-Worker-Allowed", HeaderValue::from_static("/"));
    }
    r
}

pub fn router<S: Clone + Send + Sync + 'static>() -> Router<S> {
    Router::new().route("/assets/{name}", get(serve))
}
