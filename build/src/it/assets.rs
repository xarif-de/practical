//! /assets: the eight embedded files, their types, caching, ETag/304, compression, and the
//! development mode that reads them from disk.

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

#[tokio::test]
async fn embedded_assets_with_etag_and_304() {
    let app = TestApp::new().await;
    let mut a = app.anonymous();
    for (name, mime) in FILES {
        let r = a.send(Method::GET, &format!("/assets/{name}"), &[], Body::empty()).await;
        assert_eq!(r.status, StatusCode::OK, "{name}");
        assert_eq!(r.header("content-type").as_deref(), Some(*mime), "{name}");
        assert_eq!(r.header("cache-control").as_deref(), Some("public, max-age=86400, must-revalidate"), "{name}");
        let on_disk = std::fs::read(format!("{}/assets/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap();
        assert_eq!(r.bytes, on_disk, "{name}: the embedded copy is the file");
        let etag = r.header("etag").unwrap();
        // "…" around 16 base64url characters: 12 bytes of the SHA-256
        assert!(etag.len() == 18 && etag.starts_with('"') && etag.ends_with('"'), "{etag}");
        let again = a.send(Method::GET, &format!("/assets/{name}"), &[("if-none-match", &etag)], Body::empty()).await;
        assert_eq!((again.status, again.bytes.len()), (StatusCode::NOT_MODIFIED, 0), "{name}");
        assert_eq!(again.header("etag"), None, "a 304 carries no headers of its own");
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
async fn gzip_when_the_client_accepts_it() {
    let app = TestApp::new().await;
    let mut a = app.anonymous();
    let plain = a.send(Method::GET, "/assets/app.js", &[], Body::empty()).await;
    let gz = a.send(Method::GET, "/assets/app.js", &[("accept-encoding", "gzip")], Body::empty()).await;
    assert_eq!(gz.header("content-encoding").as_deref(), Some("gzip"));
    assert!(gz.bytes.len() * 3 < plain.bytes.len(), "{} vs {}", gz.bytes.len(), plain.bytes.len());
    assert_eq!(&gz.bytes[..2], &[0x1f, 0x8b]);
    assert_eq!(plain.header("content-encoding"), None);
    // pages too
    let page = a.send(Method::GET, "/login", &[("accept-encoding", "gzip, br")], Body::empty()).await;
    assert_eq!(page.header("content-encoding").as_deref(), Some("gzip"));
    // an icon below the compression threshold or of a non-compressible type stays as it is
    let svg = a.send(Method::GET, "/assets/icon.svg", &[("accept-encoding", "gzip")], Body::empty()).await;
    assert_eq!(svg.status, StatusCode::OK);
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
