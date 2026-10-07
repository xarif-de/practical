//! Validators for the dynamic GET answers (pages, fragments, JSON): a weak ETag over the body and
//! `Cache-Control: private, no-cache`, so the browser keeps the answer and asks again with
//! If-None-Match; an unchanged body is answered 304 without it (a revisit, the live-change refetch of
//! a view nobody changed). The tag is a hash of exactly the bytes that would be sent (and their
//! Content-Type), so a 304 can only stand for those bytes: whatever made the body differ — another
//! user, a write, a zone or language switch, a flash, the CSRF token of a new session — makes the tag
//! differ. The handler still runs; what is saved is the transfer. Weak (RFC 9110 §8.8.1): the same tag
//! for the identity, gzip and brotli forms CompressionLayer makes of the body afterwards.
//! `no-cache`, not `no-store`: no-store would end the back/forward cache.

use axum::body::{Body, HttpBody};
use axum::extract::Request;
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// A body larger than this is passed on unhashed (none of the pages comes near it).
const MAX: u64 = 32 << 20;

/// `W/"<16 base64url characters>"`: 12 bytes of the SHA-256 of the Content-Type and the body.
pub fn tag(content_type: &[u8], body: &[u8]) -> String {
    use base64ct::Encoding;
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(content_type);
    h.update([0]);
    h.update(body);
    format!("W/\"{}\"", base64ct::Base64UrlUnpadded::encode_string(&h.finalize()[..12]))
}

pub async fn revalidate(req: Request, next: Next) -> Response {
    if req.method() != Method::GET {
        return next.run(req).await;
    }
    let inm = req.headers().get_all(header::IF_NONE_MATCH).iter().cloned().collect::<Vec<_>>();
    let res = next.run(req).await;
    let h = res.headers();
    let ct_ok = h.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).is_some_and(|t| t.starts_with("text/html") || t.starts_with("application/json"));
    // a stream (SSE) has no exact size: never waited for
    if res.status() != StatusCode::OK || !ct_ok || h.contains_key(header::CONTENT_ENCODING) || h.contains_key(header::ETAG) || !res.body().size_hint().exact().is_some_and(|n| n <= MAX) {
        return res;
    }
    let (mut parts, body) = res.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, MAX as usize).await else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let etag = tag(parts.headers.get(header::CONTENT_TYPE).map_or(&b""[..], |v| v.as_bytes()), &bytes);
    let h = &mut parts.headers;
    h.insert(header::ETAG, HeaderValue::from_str(&etag).expect("base64url"));
    h.entry(header::CACHE_CONTROL).or_insert(HeaderValue::from_static("private, no-cache"));
    // the 304 repeats it (RFC 9110 §15.4.5); CompressionLayer adds it to a compressed 200 anyway
    h.insert(header::VARY, HeaderValue::from_static("accept-encoding"));
    // weak comparison (RFC 9110 §13.1.2): the tags without their W/
    let strong = &etag[2..];
    let hit = inm.iter().filter_map(|v| v.to_str().ok()).flat_map(|v| v.split(',')).any(|t| {
        let t = t.trim();
        t == "*" || t.strip_prefix("W/").unwrap_or(t) == strong
    });
    if hit {
        parts.status = StatusCode::NOT_MODIFIED;
        parts.headers.remove(header::CONTENT_TYPE);
        parts.headers.remove(header::CONTENT_LENGTH);
        return Response::from_parts(parts, Body::empty());
    }
    Response::from_parts(parts, Body::from(bytes))
}
