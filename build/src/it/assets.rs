//! /assets: the eight embedded files (minified + precompressed by build.rs), their types, caching,
//! ETag/304, content coding, the layers they bypass, and the development mode that reads them from disk.

use crate::testkit::TestApp;
use axum::body::Body;
use axum::http::{Method, StatusCode};

const FILES: &[(&str, &str)] = &[
    ("style.css", "text/css; charset=utf-8"),
    ("app.js", "text/javascript; charset=utf-8"),
    ("history.js", "text/javascript; charset=utf-8"),
    ("history.css", "text/css; charset=utf-8"),
    ("sw.js", "text/javascript; charset=utf-8"),
    ("icon.svg", "image/svg+xml"),
    ("leaflet.js", "text/javascript; charset=utf-8"),
    ("leaflet.css", "text/css; charset=utf-8"),
];

/// build.rs output for `name` (`ext` "", ".gz" or ".br")
fn built(name: &str, ext: &str) -> Vec<u8> {
    std::fs::read(format!("{}/{name}{ext}", env!("OUT_DIR"))).unwrap()
}

fn gunzip(b: &[u8]) -> Vec<u8> {
    use std::io::Read;
    let mut v = Vec::new();
    flate2::read::GzDecoder::new(b).read_to_end(&mut v).unwrap();
    v
}

#[tokio::test]
async fn embedded_assets_with_etag_and_304() {
    let app = TestApp::new().await;
    let mut a = app.anonymous();
    for (name, mime) in FILES {
        let r = a.send(Method::GET, &format!("/assets/{name}"), &[], Body::empty()).await;
        assert_eq!(r.status, StatusCode::OK, "{name}");
        assert_eq!(r.header("content-type").as_deref(), Some(*mime), "{name}");
        assert_eq!(r.header("cache-control").as_deref(), Some("public, max-age=86400, must-revalidate"), "{name}");
        assert_eq!(r.header("content-encoding"), None, "{name}: no Accept-Encoding → identity");
        assert_eq!(r.header("vary").as_deref(), Some("accept-encoding"), "{name}");
        assert_eq!(r.bytes, built(name, ""), "{name}: the identity form is build.rs' minified file");
        let on_disk = std::fs::read(format!("{}/assets/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap();
        if name.starts_with("leaflet") || *name == "icon.svg" {
            assert_eq!(r.bytes, on_disk, "{name}: vendored/tiny files ship as they are");
        } else {
            assert!(r.bytes.len() < on_disk.len() * 9 / 10, "{name}: minified {} of {}", r.bytes.len(), on_disk.len());
        }
        let etag = r.header("etag").unwrap();
        // "…" around 16 base64url characters: 12 bytes of the SHA-256
        assert!(etag.len() == 18 && etag.starts_with('"') && etag.ends_with('"'), "{etag}");
        let again = a.send(Method::GET, &format!("/assets/{name}"), &[("if-none-match", &etag)], Body::empty()).await;
        assert_eq!((again.status, again.bytes.len()), (StatusCode::NOT_MODIFIED, 0), "{name}");
        // RFC 9110 §15.4.5: a 304 repeats ETag, Cache-Control and Vary
        assert_eq!(again.header("etag").as_deref(), Some(etag.as_str()));
        assert_eq!(again.header("cache-control"), r.header("cache-control"));
        assert_eq!(again.header("vary").as_deref(), Some("accept-encoding"));
        let listed = a.send(Method::GET, &format!("/assets/{name}"), &[("if-none-match", &format!("\"x\", W/{etag}"))], Body::empty()).await;
        assert_eq!(listed.status, StatusCode::NOT_MODIFIED, "{name}: a list with a weak match");
        let stale = a.send(Method::GET, &format!("/assets/{name}"), &[("if-none-match", "\"other\"")], Body::empty()).await;
        assert_eq!(stale.status, StatusCode::OK);
        assert_eq!(r.header("service-worker-allowed").as_deref(), if *name == "sw.js" { Some("/") } else { None }, "{name}");
    }
    assert_eq!(a.send(Method::GET, "/assets/nope.js", &[], Body::empty()).await.status, StatusCode::NOT_FOUND);
    assert_eq!(a.send(Method::GET, "/assets/../Cargo.toml", &[], Body::empty()).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn asset_version_is_stable_and_used_in_pages() {
    let v = crate::routes::assets::asset_version();
    assert_eq!(v.len(), 8 * 4, "four ETag characters per asset");
    assert_eq!(v, crate::routes::assets::asset_version());
    let app = TestApp::new().await;
    app.anonymous().get("/login").await.ok().has(&format!("/assets/style.css?v={v}")).has(&format!("/assets/app.js?v={v}"));
}

#[tokio::test]
async fn the_current_version_is_immutable_anything_else_revalidates() {
    let app = TestApp::new().await;
    let mut a = app.anonymous();
    let v = crate::routes::assets::asset_version();
    let cc = async |a: &mut crate::testkit::Client, q: &str| a.send(Method::GET, &format!("/assets/app.js{q}"), &[("accept-encoding", "br")], Body::empty()).await.header("cache-control");
    assert_eq!(cc(&mut a, &format!("?v={v}")).await.as_deref(), Some("public, max-age=31536000, immutable"));
    // a page of another build asks for its own version: today's bytes must not be pinned under it
    for q in ["", "?v=0000", &format!("?v={v}x"), &format!("?x=1&v={v}")] {
        assert_eq!(cc(&mut a, q).await.as_deref(), Some("public, max-age=86400, must-revalidate"), "{q:?}");
    }
    // unversioned by design (service worker script, notification icon)
    for name in ["sw.js", "icon.svg"] {
        let r = a.send(Method::GET, &format!("/assets/{name}"), &[], Body::empty()).await;
        assert_eq!(r.header("cache-control").as_deref(), Some("public, max-age=86400, must-revalidate"));
    }
}

#[tokio::test]
async fn precompressed_by_accept_encoding() {
    let app = TestApp::new().await;
    let mut a = app.anonymous();
    for (name, _) in FILES {
        let get = async |a: &mut crate::testkit::Client, ae: &str| a.send(Method::GET, &format!("/assets/{name}"), &[("accept-encoding", ae)], Body::empty()).await;
        let identity = built(name, "");
        // a browser over HTTPS: brotli, the embedded bytes as they are
        let br = get(&mut a, "gzip, deflate, br, zstd").await;
        assert_eq!((br.header("content-encoding").as_deref(), &br.bytes), (Some("br"), &built(name, ".br")), "{name}");
        assert!(br.bytes.len() < identity.len());
        let gz = get(&mut a, "gzip, deflate").await;
        assert_eq!(gz.header("content-encoding").as_deref(), Some("gzip"), "{name}");
        assert_eq!(gunzip(&gz.bytes), identity, "{name}");
        assert!(br.bytes.len() <= gz.bytes.len() && gz.bytes.len() < identity.len(), "{name}");
        for (ae, want) in [("br;q=0, gzip", Some("gzip")), ("*", Some("br")), ("GZIP;Q=0.5", Some("gzip")), ("identity", None), ("gzip;q=0", None), ("*;q=0, br", Some("br")), ("", None)] {
            assert_eq!(get(&mut a, ae).await.header("content-encoding").as_deref(), want, "{name}: {ae:?}");
        }
        // one strong ETag per representation; a validator of another coding does not match
        let tags = [br.header("etag").unwrap(), gz.header("etag").unwrap(), get(&mut a, "").await.header("etag").unwrap()];
        assert!(tags[0] != tags[1] && tags[1] != tags[2] && tags[0] != tags[2], "{tags:?}");
        let cross = a.send(Method::GET, &format!("/assets/{name}"), &[("accept-encoding", "gzip"), ("if-none-match", &tags[0])], Body::empty()).await;
        assert_eq!(cross.status, StatusCode::OK, "{name}");
        let same = a.send(Method::GET, &format!("/assets/{name}"), &[("accept-encoding", "br"), ("if-none-match", &tags[0])], Body::empty()).await;
        assert_eq!(same.status, StatusCode::NOT_MODIFIED, "{name}");
    }
}

#[tokio::test]
async fn pages_are_still_compressed_per_response() {
    let app = TestApp::new().await;
    let mut a = app.anonymous();
    let page = a.send(Method::GET, "/login", &[("accept-encoding", "gzip, br")], Body::empty()).await;
    assert_eq!(page.header("content-encoding").as_deref(), Some("gzip"));
    assert_eq!(&page.bytes[..2], &[0x1f, 0x8b]);
}

#[tokio::test]
async fn assets_and_healthz_skip_session_auth_and_csrf() {
    let app = TestApp::new().await;
    let mut a = app.anonymous();
    // wrong Basic credentials: the authentication layer is not in the path (no token check at all)
    let bad = ("authorization", "Basic cHJvYmU6d3Jvbmc=");
    for p in ["/assets/app.js", "/healthz"] {
        let r = a.send(Method::GET, p, &[bad, ("cookie", "practical_session=stale")], Body::empty()).await;
        assert_eq!(r.status, StatusCode::OK, "{p}");
        assert_eq!(r.header("set-cookie"), None, "{p}: the session layer is not in the path");
    }
    // GET-only route outside the CSRF guard: other methods are 405, not the guard's 403
    let post = a.send(Method::POST, "/assets/app.js", &[], Body::empty()).await;
    assert_eq!(post.status, StatusCode::METHOD_NOT_ALLOWED);
    // … while the pages keep it
    assert_eq!(a.send(Method::POST, "/logout", &[("origin", "https://evil.example")], Body::empty()).await.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn development_mode_reads_assets_from_disk_uncached() {
    let dir = std::env::temp_dir().join(format!("practical-assets-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("style.css"), "body{color:red}").unwrap();
    let d = dir.to_string_lossy().into_owned();
    let app = TestApp::with(|c| c.dev_assets_dir = Some(d)).await;
    let mut a = app.anonymous();
    let r = a.send(Method::GET, "/assets/style.css", &[], Body::empty()).await;
    assert_eq!((r.status, r.body.as_str()), (StatusCode::OK, "body{color:red}"));
    assert_eq!(r.header("cache-control").as_deref(), Some("no-store"));
    assert_eq!(r.header("etag"), None);
    // a known name missing on disk is a 500 naming the path
    let r = a.send(Method::GET, "/assets/app.js", &[], Body::empty()).await;
    assert_eq!(r.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(r.body.contains("app.js"));
    std::fs::remove_dir_all(&dir).unwrap();
}
