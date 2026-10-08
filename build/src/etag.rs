//! Validators for the dynamic GET answers (pages, fragments, JSON): a weak ETag over the body (with main.rs'
//! `Cache-Control: private, no-cache`), so the browser keeps the answer and asks again with
//! If-None-Match; an unchanged body is answered 304 without it (a revisit, the live-change refetch of
//! a view nobody changed). The tag is a hash of exactly the bytes that would be sent (and their
//! Content-Type), so a 304 can only stand for those bytes: whatever made the body differ — another
//! user, a write, a zone or language switch, a flash, the CSRF token of a new session — makes the tag
//! differ. The handler still runs; what is saved is the transfer. Weak (RFC 9110 §8.8.1): the same tag
//! for the identity, gzip and brotli forms of the body.
//! `no-cache`, not `no-store`: no-store would end the back/forward cache.
//! Brotli is made here, of the buffered body in one go, the window no larger than the body: tower-http's
//! streaming encoder reserves brotli's default 4 MiB window per response. Gzip (a client without br) is
//! left to CompressionLayer.

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

/// Middleware for GET answers (HTML or JSON, status 200, exact size up to `MAX`, not yet encoded or tagged): a
/// weak ETag over the body, 304 when `If-None-Match` names it, else the body brotli-encoded in one shot when
/// the client accepts `br` and the body has at least `MIN_BR` bytes. Anything else passes unchanged.
pub async fn revalidate(req: Request, next: Next) -> Response {
    if req.method() != Method::GET {
        return next.run(req).await;
    }
    let inm = req.headers().get_all(header::IF_NONE_MATCH).iter().cloned().collect::<Vec<_>>();
    let br = crate::routes::assets::accepts(req.headers(), "br");
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
        // RFC 9110 §15.4.5: the 304 repeats Cache-Control. The 200 gets it from main.rs' response map (every page
        // and JSON answer), which keys on the Content-Type this 304 no longer carries.
        parts.headers.entry(header::CACHE_CONTROL).or_insert(HeaderValue::from_static("private, no-cache"));
        parts.headers.remove(header::CONTENT_TYPE);
        parts.headers.remove(header::CONTENT_LENGTH);
        return Response::from_parts(parts, Body::empty());
    }
    if br && bytes.len() >= MIN_BR {
        let body = brotli(&bytes);
        parts.headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static("br"));
        parts.headers.remove(header::CONTENT_LENGTH);
        return Response::from_parts(parts, Body::from(body));
    }
    Response::from_parts(parts, Body::from(bytes))
}

/// Smaller bodies go out as they are (CompressionLayer's own floor is 32 bytes; a brotli stream of a few
/// bytes is no smaller).
const MIN_BR: usize = 64;

/// Quality 4, the window the smallest power of two that holds the body (16 KiB … 4 MiB). Measured 2026-10-07 on
/// the bench DB (e2e build; RSS after the bench's request loop + burst, macOS, 3 runs each): gzip only 156-160 MB,
/// br 3 173-177, br 4 179-194, br 5 318-405, br 4 with the window capped at 256 KiB 347-357 (befd301, gzip
/// only: 183-205; tower-http's streaming br 5: 259-282). Bytes and CPU on the 15 main pages/JSON (1.37 MB
/// raw; release, best of 9): gzip 6 333.0 KB / 34.8 ms, br 4 300.6 KB / 20.6 ms (−10 %, −41 %), br 5 279.9 KB /
/// 36.9 ms — br 5's extra 6 % of bytes cost 1.7-2.2× the memory.
pub fn brotli(body: &[u8]) -> Vec<u8> {
    let lgwin = (usize::BITS - body.len().saturating_sub(1).leading_zeros()).clamp(14, 22) as i32;
    let params = brotli::enc::BrotliEncoderParams { quality: 4, lgwin, size_hint: body.len(), ..Default::default() };
    let mut out = Vec::with_capacity(body.len() / 6);
    brotli::BrotliCompress(&mut &body[..], &mut out, &params).expect("in memory");
    out
}
