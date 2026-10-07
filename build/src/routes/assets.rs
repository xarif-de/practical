//! Static assets compiled into the binary (the files of assets/ and the per-language i18n scripts
//! build.rs generates; no rust-embed needed). build.rs minifies them and stores a gzip and a brotli
//! form, so no request compresses anything: a br client gets the brotli bytes, a gzip client the gzip
//! bytes, any other client the gzip form decoded once. `?v=<asset_version()>` URLs (the <link>/<script>
//! tags of the pages) and `url()`s (`?v=` of the file's own hash: the icon sprite, the i18n scripts) are
//! cached as immutable.
//! Development (`PRACTICAL__DEV_ASSETS_DIR`): read from that directory on every request, uncached
//! (a generated file the directory lacks: the embedded one).

use crate::state::SharedState;
use axum::Router;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use std::sync::OnceLock;

struct Asset {
    name: &'static str,
    mime: &'static str,
    /// size of the minified file (the identity form)
    len: usize,
    /// made by build.rs (i18n-<lang>.js), no file in assets/
    generated: bool,
    gzip: &'static [u8],
    br: &'static [u8],
}

const ASSETS: &[Asset] = include!(concat!(env!("OUT_DIR"), "/assets.rs"));

/// Per asset: 16 base64url characters of the SHA-256 of its gzip form (build.rs output is
/// deterministic). Each encoding gets its own strong ETag (RFC 9110 §8.8.3: a different
/// representation); `asset_version` is built from the bases.
fn etags() -> &'static Vec<String> {
    static E: OnceLock<Vec<String>> = OnceLock::new();
    E.get_or_init(|| {
        use base64ct::Encoding;
        use sha2::{Digest, Sha256};
        ASSETS
            .iter()
            .map(|a| {
                let h = Sha256::digest(a.gzip);
                base64ct::Base64UrlUnpadded::encode_string(&h[..12])
            })
            .collect()
    })
}

pub fn asset_version() -> &'static str {
    // cache-busting query for <link>/<script> tags: changes whenever any asset changes (16 base64url
    // characters of a hash over every asset's ETag base)
    static V: OnceLock<String> = OnceLock::new();
    V.get_or_init(|| {
        use base64ct::Encoding;
        use sha2::{Digest, Sha256};
        let h = Sha256::digest(etags().concat());
        base64ct::Base64UrlUnpadded::encode_string(&h[..12])
    })
}

/// `?v=` of one file: 8 characters of its own ETag base (48 bits), so its URL stays while other files change.
fn own_version(i: usize) -> &'static str {
    &etags()[i][..8]
}

/// `/assets/<name>?v=<own version>`, immutable: for a file a page names many times (the icon sprite in every
/// `<use>`) or that a change of another file must not invalidate (one i18n script per language). None for
/// an unknown name.
pub fn url(name: &str) -> Option<&'static str> {
    static U: OnceLock<Vec<String>> = OnceLock::new();
    let urls = U.get_or_init(|| ASSETS.iter().enumerate().map(|(i, a)| format!("/assets/{}?v={}", a.name, own_version(i))).collect());
    ASSETS.iter().position(|a| a.name == name).map(|i| urls[i].as_str())
}

/// The icon sprite's URL: `<use href="{icons()}#i-x">` (macros.html, render.rs; app.js reads it from <meta name=icons>).
pub fn icons() -> &'static str {
    url("icons.svg").expect("icons.svg is an asset")
}

/// The identity form, decoded from the gzip bytes the first time a client asks for it.
fn identity(i: usize) -> &'static [u8] {
    static RAW: [OnceLock<Vec<u8>>; ASSETS.len()] = [const { OnceLock::new() }; ASSETS.len()];
    RAW[i].get_or_init(|| {
        use std::io::Read;
        let mut v = Vec::with_capacity(ASSETS[i].len);
        flate2::read::GzDecoder::new(ASSETS[i].gzip).read_to_end(&mut v).expect("embedded gzip");
        v
    })
}

#[derive(Clone, Copy, PartialEq)]
enum Coding {
    Br,
    Gzip,
    Identity,
}

/// RFC 9110 §12.5.3: a coding is acceptable when listed (or matched by `*`) with q > 0.
pub(crate) fn accepts(headers: &HeaderMap, coding: &str) -> bool {
    let (mut named, mut star) = (None, None);
    for v in headers.get_all(header::ACCEPT_ENCODING).iter().filter_map(|v| v.to_str().ok()) {
        for item in v.split(',') {
            let mut parts = item.split(';');
            let name = parts.next().unwrap_or("").trim();
            let q = parts.find_map(|p| p.trim().strip_prefix("q=").or_else(|| p.trim().strip_prefix("Q="))).map_or(1.0, |q| q.trim().parse::<f32>().unwrap_or(0.0));
            if name.eq_ignore_ascii_case(coding) {
                named = Some(q);
            } else if name == "*" {
                star = Some(q);
            }
        }
    }
    named.or(star).is_some_and(|q| q > 0.0)
}

fn etag_for(i: usize, c: Coding) -> String {
    let base = &etags()[i];
    match c {
        Coding::Br => format!("\"{base}-br\""),
        Coding::Gzip => format!("\"{base}-gz\""),
        Coding::Identity => format!("\"{base}\""),
    }
}

/// If-None-Match (RFC 9110 §13.1.2): weak comparison, a list or `*`.
fn not_modified(headers: &HeaderMap, etag: &str) -> bool {
    headers.get_all(header::IF_NONE_MATCH).iter().filter_map(|v| v.to_str().ok()).flat_map(|v| v.split(',')).any(|t| {
        let t = t.trim();
        t == "*" || t.strip_prefix("W/").unwrap_or(t) == etag
    })
}

fn finish(mut r: Response, a: &Asset, cache: &'static str) -> Response {
    let h = r.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static(a.mime));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    if a.name == "sw.js" {
        h.insert("Service-Worker-Allowed", HeaderValue::from_static("/"));
    }
    r
}

async fn serve(State(state): State<SharedState>, Path(name): Path<String>, RawQuery(query): RawQuery, headers: HeaderMap) -> Response {
    let Some((i, a)) = ASSETS.iter().enumerate().find(|(_, a)| a.name == name) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if let Some(dir) = &state.config.dev_assets_dir {
        return match tokio::fs::read(std::path::Path::new(dir).join(a.name)).await {
            Ok(body) => finish(body.into_response(), a, "no-store"),
            // build.rs made it from locales/ (`mise run dev` rebuilds on a locale edit): no file to read
            Err(e) if a.generated && e.kind() == std::io::ErrorKind::NotFound => finish(identity(i).into_response(), a, "no-store"),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("{dir}/{}: {e}", a.name)).into_response(),
        };
    }
    // The exact current version only: a page of an older (or newer) process asking for its own
    // version gets today's bytes, which must not be pinned for a year under that URL.
    let v = query.as_deref().and_then(|q| q.strip_prefix("v="));
    let versioned = v == Some(asset_version()) || v == Some(own_version(i));
    let cache = if versioned { "public, max-age=31536000, immutable" } else { "public, max-age=86400, must-revalidate" };
    let coding = if accepts(&headers, "br") {
        Coding::Br
    } else if accepts(&headers, "gzip") {
        Coding::Gzip
    } else {
        Coding::Identity
    };
    let etag = etag_for(i, coding);
    let mut r = if not_modified(&headers, &etag) {
        // RFC 9110 §15.4.5: the 304 repeats ETag, Cache-Control and Vary of the 200
        let mut r = StatusCode::NOT_MODIFIED.into_response();
        r.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
        r
    } else {
        let body = match coding {
            Coding::Br => a.br.into_response(),
            Coding::Gzip => a.gzip.into_response(),
            Coding::Identity => identity(i).into_response(),
        };
        let mut r = finish(body, a, cache);
        if coding != Coding::Identity {
            let enc = if coding == Coding::Br { "br" } else { "gzip" };
            r.headers_mut().insert(header::CONTENT_ENCODING, HeaderValue::from_static(enc));
        }
        r
    };
    let h = r.headers_mut();
    h.insert(header::ETAG, HeaderValue::from_str(&etag).unwrap());
    h.insert(header::VARY, HeaderValue::from_static("accept-encoding"));
    r
}

pub fn router() -> Router<SharedState> {
    Router::new().route("/assets/{name}", get(serve))
}
