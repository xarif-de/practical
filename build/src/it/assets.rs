//! /assets: the embedded files (minified + precompressed by build.rs), their types, caching,
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
    ("lettering.svg", "image/svg+xml"),
    ("icons.svg", "image/svg+xml"),
    ("theme.js", "text/javascript; charset=utf-8"),
    ("i18n-de.js", "text/javascript; charset=utf-8"),
    ("i18n-en.js", "text/javascript; charset=utf-8"),
    ("i18n-fr.js", "text/javascript; charset=utf-8"),
    ("i18n-es.js", "text/javascript; charset=utf-8"),
    ("leaflet.js", "text/javascript; charset=utf-8"),
    ("leaflet.css", "text/css; charset=utf-8"),
];

/// build.rs output for `name` (`ext` "", ".gz" or ".br")
fn built(name: &str, ext: &str) -> Vec<u8> {
    std::fs::read(format!("{}/{name}{ext}", env!("OUT_DIR"))).unwrap()
}

/// build/assets, the sources
fn assets_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets")
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
        let on_disk = if name.starts_with("i18n-") { r.bytes.clone() } else { crate::bundle::read(&assets_dir(), name).unwrap() };
        if name.starts_with("leaflet") || name.ends_with(".svg") || name.starts_with("i18n-") {
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
    assert_eq!(v.len(), 16, "12 bytes of a hash over every asset, base64url");
    assert_eq!(v, crate::routes::assets::asset_version());
    let app = TestApp::new().await;
    let page = app.anonymous().get("/login").await;
    // each file by its own version: a change of app.js must not make the browser fetch style.css again
    let own = |n: &str| crate::routes::assets::url(n).unwrap();
    page.ok().has(&format!("<link rel=\"stylesheet\" href=\"{}\">", own("style.css"))).has(&format!("<script src=\"{}\" defer", own("app.js"))).has(&format!("<script src=\"{}\"></script>", own("theme.js")));
    assert!(!page.body.contains(&format!("?v={v}")), "no file by the version of all files");
    assert_ne!(own("style.css").split("?v=").nth(1), own("app.js").split("?v=").nth(1));
    let s = super::seed::seed().await;
    let mut c = s.app.login("alice").await;
    c.get("/history").await.ok().has(&format!("<link rel=\"stylesheet\" href=\"{}\"><script src=\"{}\" defer></script>", own("history.css"), own("history.js")));
    // the map dialog (editor.html) loads Leaflet by its files' own URLs (app.js reads data-leaflet)
    c.get("/calendar").await.ok().has(&format!(" data-leaflet=\"{} {}\"", own("leaflet.js"), own("leaflet.css")));
    for n in ["style.css", "app.js", "theme.js", "history.css", "history.js", "leaflet.js", "leaflet.css"] {
        let r = c.send(Method::GET, own(n), &[("accept-encoding", "br")], Body::empty()).await;
        assert_eq!((r.status, r.header("cache-control").as_deref()), (StatusCode::OK, Some("public, max-age=31536000, immutable")), "{n}");
    }
    // the sprite and the strings by their own version (the language: German, the default)
    let (icons, i18n) = (crate::routes::assets::icons(), crate::routes::assets::url("i18n-de.js").unwrap());
    assert!(icons.starts_with("/assets/icons.svg?v=") && icons.len() == "/assets/icons.svg?v=".len() + 8, "{icons}");
    page.has(&format!("<meta name=\"icons\" content=\"{icons}\">")).has(&format!("<use href=\"{icons}#i-")).has(&format!("<script src=\"{i18n}\" defer></script>"));
    // nothing inline any more: no sprite, no JSON strings block, no script body (the Content-Security-Policy forbids inline script)
    assert!(!page.body.contains("<symbol") && !page.body.contains("id=\"i18n\"") && !page.body.contains("href=\"#i-"));
    assert_eq!(page.body.matches("<script").count(), page.body.matches("<script src=").count(), "{}", page.body);
    assert_eq!(crate::routes::assets::url("nope.js"), None);
}

/// Every symbol the pages and scripts name exists in the sprite (templates, render.rs, app.js, history.js):
/// a missing one would render an empty icon.
#[test]
fn every_named_icon_is_in_the_sprite() {
    let sprite = std::fs::read_to_string(format!("{}/assets/icons.svg", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let have: std::collections::BTreeSet<&str> = sprite.split("<symbol id=\"").skip(1).map(|s| &s[..s.find('"').unwrap()]).collect();
    assert!(have.len() > 100, "{}", have.len());
    let re = regex::Regex::new(r##"['"#]i-([a-z0-9-]+)['"]|ico(?:_c)?\(["']([a-z0-9-]+)["']"##).unwrap();
    let mut missing = vec![];
    let mut dirs: Vec<_> = ["templates", "assets", "src"].map(|d| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(d)).into();
    while let Some(dir) = dirs.pop() {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() { dirs.push(p); continue; }
            if p.extension().is_some_and(|x| x == "svg") { continue; }
            let text = std::fs::read_to_string(&p).unwrap_or_default();
            for c in re.captures_iter(&text) {
                let n = c.get(1).or(c.get(2)).unwrap().as_str();
                // history.js' `ico` takes the symbol id itself ('i-restore'), everything else the name
                let n = if n.starts_with("i-") { n.to_string() } else { format!("i-{n}") };
                if !have.contains(n.as_str()) && !missing.contains(&n) { missing.push(n); }
            }
        }
    }
    assert!(missing.is_empty(), "not in assets/icons.svg: {missing:?}");
}

/// build.rs formats the js-* strings itself (no runtime JSON any more): they must be exactly what the
/// runtime bundles say, for every language.
#[test]
fn i18n_assets_are_the_runtime_strings() {
    let i = crate::i18n::I18n::load();
    for l in crate::i18n::LANGS {
        let js = String::from_utf8(built(&format!("i18n-{l}.js"), "")).unwrap();
        let json = js.strip_prefix("var I18N=").and_then(|j| j.strip_suffix(';')).unwrap_or_else(|| panic!("{l}: {js:.40}"));
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        assert_eq!(v, i.js_json(l), "{l}");
        assert!(v.as_object().unwrap().len() > 300, "{l}");
    }
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
    // a file's own version (`url`) is immutable for that file only
    let icons = crate::routes::assets::icons();
    let own = icons.split("?v=").nth(1).unwrap();
    for (path, want) in [(icons.to_string(), "public, max-age=31536000, immutable"), (format!("/assets/app.js?v={own}"), "public, max-age=86400, must-revalidate")] {
        let r = a.send(Method::GET, &path, &[("accept-encoding", "br")], Body::empty()).await;
        assert_eq!((r.status, r.header("cache-control").as_deref()), (StatusCode::OK, Some(want)), "{path}");
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
    // brotli when the client takes it (every browser over HTTPS), gzip for the others
    let page = a.send(Method::GET, "/login", &[("accept-encoding", "gzip, br")], Body::empty()).await;
    assert_eq!(page.header("content-encoding").as_deref(), Some("br"));
    let page = a.send(Method::GET, "/login", &[("accept-encoding", "gzip")], Body::empty()).await;
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
    // a known name missing on disk is a 500 naming the path …
    let r = a.send(Method::GET, "/assets/app.js", &[], Body::empty()).await;
    assert_eq!(r.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(r.body.contains("app.js"));
    // … unless build.rs made it (no source file): the embedded one, uncached
    let r = a.send(Method::GET, "/assets/i18n-fr.js", &[], Body::empty()).await;
    assert_eq!((r.status, r.header("cache-control").as_deref(), r.bytes), (StatusCode::OK, Some("no-store"), built("i18n-fr.js", "")));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Every file under assets/js and assets/css is a part of exactly one bundle (src/bundle.rs) and every listed part
/// exists: a part missing from the table would never ship.
#[test]
fn bundles_list_every_part_file_once() {
    let mut listed = crate::bundle::files();
    listed.sort();
    let n = listed.len();
    listed.dedup();
    assert_eq!(listed.len(), n, "a part listed twice");
    let (mut on_disk, mut dirs) = (vec![], vec![assets_dir().join("js"), assets_dir().join("css")]);
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() { dirs.push(p) } else { on_disk.push(p.strip_prefix(assets_dir()).unwrap().to_string_lossy().into_owned()) }
        }
    }
    on_disk.sort();
    assert_eq!(listed, on_disk);
    for (name, _) in crate::bundle::BUNDLES {
        assert!(!assets_dir().join(name).exists(), "assets/{name} would shadow its parts in development mode");
    }
}

/// Development mode on the source tree serves app.js and style.css as the joins build.rs minifies: the parts in
/// table order, the editor's fragments inside ONE function.
#[tokio::test]
async fn development_mode_joins_bundles_from_their_parts() {
    let app = TestApp::with(|c| c.dev_assets_dir = Some(assets_dir().to_string_lossy().into_owned())).await;
    let mut a = app.anonymous();
    for name in ["app.js", "style.css"] {
        let r = a.send(Method::GET, &format!("/assets/{name}"), &[], Body::empty()).await;
        assert_eq!((r.status, r.header("cache-control").as_deref()), (StatusCode::OK, Some("no-store")), "{name}");
        assert_eq!(r.bytes, crate::bundle::read(&assets_dir(), name).unwrap(), "{name}");
    }
    let part = |p: &str| std::fs::read_to_string(assets_dir().join(p)).unwrap();
    let js = String::from_utf8(crate::bundle::read(&assets_dir(), "app.js").unwrap()).unwrap();
    // the last part is the onboarding wrap's closing line, so settings.js is checked where it sits
    assert!(js.starts_with(&part("js/core.js")) && js.ends_with(&format!("{}{}\n", part("js/onboarding/guide.js"), "})();")));
    assert!(js.contains(&format!("{}(function () {{\n{}", part("js/settings.js"), part("js/onboarding/steps.js"))));
    // the editor's parts: one function, run at load and callable again (bundle.rs ED_OPEN/ED_END)
    assert!(js.contains(&format!("{}practical.editorInit = function () {{\n{}", part("js/calendar.js"), part("js/editor/setup.js"))));
    assert!(js.contains(&format!("{}}};\npractical.editorInit();\n{}", part("js/editor/wiring.js"), part("js/search.js"))));
    let css = String::from_utf8(crate::bundle::read(&assets_dir(), "style.css").unwrap()).unwrap();
    assert!(css.starts_with(&part("css/base.css")) && css.ends_with(&part("css/color-optimiser.css")));
}

/// The development asset watcher sees a part in a sub-directory change (tasks::asset_stamps).
#[tokio::test]
async fn development_watcher_sees_parts_in_sub_directories() {
    let dir = std::env::temp_dir().join(format!("practical-watch-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir_all(dir.join("js/editor")).unwrap();
    std::fs::write(dir.join("style.css"), "a{}").unwrap();
    std::fs::write(dir.join("js/editor/x.js"), "x;").unwrap();
    let d = dir.to_string_lossy().into_owned();
    let before = crate::tasks::asset_stamps(&d).await;
    assert_eq!(before.len(), 2, "{before:?}");
    assert!(before.iter().any(|(p, _)| p.ends_with("js/editor/x.js")), "{before:?}");
    let f = std::fs::File::options().write(true).open(dir.join("js/editor/x.js")).unwrap();
    f.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1000)).unwrap();
    assert_ne!(crate::tasks::asset_stamps(&d).await, before);
    std::fs::remove_dir_all(&dir).unwrap();
}
