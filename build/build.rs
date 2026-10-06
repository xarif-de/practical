// Exports the RustiCal git tag pinned in Cargo.toml as RUSTICAL_TAG (shown by
// `--version` and in the UI's source note; `check-rustical-image <ref>` compares
// an image reference with it). Parsed from Cargo.toml on purpose: the tag IS the
// contract, and Renovate bumps exactly that line — together with the Dockerfile's
// RustiCal image, which is checked against it below.
//
// Also builds the static assets (assets()): minified and compressed here, once per build, so that
// no request ever compresses (src/routes/assets.rs serves these bytes as they are).
use std::fmt::Write as _;
use std::fs;
use std::io::Write as _;

#[path = "src/css_min.rs"]
mod css_min;
#[path = "src/domain/holidays/names.rs"]
#[allow(dead_code)] // the reader half is for the binary
mod names;

/// name, MIME type, how it is minified. The sources in assets/ stay readable; the binary embeds
/// only the gzip and brotli forms (assets.rs decodes the gzip form once for the rare client that
/// accepts neither).
const ASSETS: &[(&str, &str, Minify)] = &[
    ("style.css", "text/css; charset=utf-8", Minify::Css),
    ("app.js", "text/javascript; charset=utf-8", Minify::Js),
    // the change history page only (templates/history.html)
    ("history.js", "text/javascript; charset=utf-8", Minify::Js),
    ("history.css", "text/css; charset=utf-8", Minify::Css),
    ("sw.js", "text/javascript; charset=utf-8", Minify::Js),
    ("icon.svg", "image/svg+xml", Minify::No),
    // Leaflet 1.9.4 (BSD-2), vendored; fetched by the browser only when the map dialog opens.
    // Shipped as vendored (leaflet.js is upstream's minified build; both keep their licence header).
    ("leaflet.js", "text/javascript; charset=utf-8", Minify::No),
    ("leaflet.css", "text/css; charset=utf-8", Minify::No),
];

#[derive(Clone, Copy, PartialEq)]
enum Minify {
    Js,
    Css,
    No,
}

/// Parser-based (oxc): a syntax error fails the build instead of shipping broken code. Classic
/// scripts, so top-level names (globals other scripts and inline handlers see) are never renamed;
/// target ES2020 = the sources' own language level, so no newer syntax is introduced.
fn minify_js(name: &str, src: &str) -> String {
    use oxc_codegen::{Codegen, CodegenOptions, CommentOptions};
    use oxc_minifier::{CompressOptions, MangleOptions, Minifier, MinifierOptions};
    let alloc = oxc_allocator::Allocator::default();
    let parsed = oxc_parser::Parser::new(&alloc, src, oxc_span::SourceType::script()).parse();
    assert!(parsed.diagnostics.is_empty() && !parsed.fatal_error, "assets/{name}: {:?}", parsed.diagnostics);
    let mut program = parsed.program;
    let target = oxc_compat::EngineTargets::from_target("es2020").expect("es2020");
    let options = MinifierOptions { mangle: Some(MangleOptions::default()), mangle_properties: None, compress: Some(CompressOptions { target, ..CompressOptions::smallest() }) };
    let ret = Minifier::new(options).minify(&alloc, &mut program);
    let comments = CommentOptions { normal: false, jsdoc: false, annotation: false, ..CommentOptions::default() }; // legal (/*! … */) comments stay
    Codegen::new().with_options(CodegenOptions { minify: true, comments, ..CodegenOptions::default() }).with_scoping(ret.scoping).build(&program).code
}

fn assets() {
    println!("cargo:rerun-if-changed=assets");
    let out = std::env::var("OUT_DIR").expect("OUT_DIR");
    // one thread per file: brotli 11 is the slow part (app.js ≈ 1 s), the files are independent
    let lens: Vec<usize> = std::thread::scope(|s| {
        let jobs: Vec<_> = ASSETS.iter().map(|&(name, _, how)| s.spawn(move || build_asset(name, how))).collect();
        jobs.into_iter().map(|j| j.join().expect("asset build panicked")).collect()
    });
    let mut table = String::from("&[\n");
    for (&(name, mime, _), len) in ASSETS.iter().zip(lens) {
        writeln!(table, "    Asset {{ name: {name:?}, mime: {mime:?}, len: {len}, gzip: include_bytes!(concat!(env!(\"OUT_DIR\"), \"/{name}.gz\")), br: include_bytes!(concat!(env!(\"OUT_DIR\"), \"/{name}.br\")) }},").unwrap();
    }
    table.push(']');
    fs::write(format!("{out}/assets.rs"), table).unwrap();
}

/// Writes OUT_DIR/<name>{,.gz,.br}; returns the minified size.
fn build_asset(name: &str, how: Minify) -> usize {
    let out = std::env::var("OUT_DIR").expect("OUT_DIR");
    let src = fs::read(format!("assets/{name}")).unwrap_or_else(|e| panic!("assets/{name}: {e}"));
    let body = match how {
        Minify::No => src,
        Minify::Js => minify_js(name, std::str::from_utf8(&src).expect("UTF-8")).into_bytes(),
        Minify::Css => css_min::minify_css(std::str::from_utf8(&src).expect("UTF-8")).into_bytes(),
    };
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    gz.write_all(&body).unwrap();
    let gz = gz.finish().unwrap();
    let mut br = Vec::new();
    let params = brotli::enc::BrotliEncoderParams { quality: 11, lgwin: 22, ..Default::default() };
    brotli::BrotliCompress(&mut &body[..], &mut br, &params).unwrap();
    // assets.rs relies on it: every client gets at most the identity size, a br client the smallest
    assert!(br.len() <= gz.len() && gz.len() < body.len(), "assets/{name}: gzip {} / brotli {} not smaller than {} bytes", gz.len(), br.len(), body.len());
    // the minified file too: what e2e/build-server.sh hands the e2e dev server (and what a reader inspects)
    for (ext, bytes) in [("", &body), (".gz", &gz), (".br", &br)] {
        fs::write(format!("{out}/{name}{ext}"), bytes).unwrap();
    }
    body.len()
}

/// src/data/regions.json (tools/gen_regions.py, 566 KB) as the two tables of names.rs, each
/// brotli-compressed: what the app reads and nothing else (no `source`/`generated`/`langs`, positional
/// fields, a name shared by several languages once). holidays.rs decodes a table when first needed.
fn regions() {
    println!("cargo:rerun-if-changed=src/data/regions.json");
    let out = std::env::var("OUT_DIR").expect("OUT_DIR");
    let src = fs::read_to_string("src/data/regions.json").expect("src/data/regions.json");
    let data: serde_json::Value = serde_json::from_str(&src).expect("src/data/regions.json: JSON");
    let rows = |kind: &str, meta: &dyn Fn(&str, &serde_json::Value) -> String| -> Vec<names::Src> {
        data[kind].as_object().unwrap_or_else(|| panic!("regions.json: no {kind}")).iter().map(|(code, v)| {
            let n = v["n"].as_object().unwrap_or_else(|| panic!("regions.json: {code} has no names"));
            let s = |x: &serde_json::Value| x.as_str().expect("a name is a string").to_string();
            for l in n.keys() {
                assert!(names::LANGS.contains(&l.as_str()), "regions.json: {code} has a name in {l}, not a column of names.rs");
            }
            names::Src { code: code.clone(), meta: meta(code, v), names: names::LANGS.map(|l| n.get(l).map(s)), any: n.values().next().map(s) }
        }).collect()
    };
    let countries = rows("countries", &|_, v| (v["oh"].as_u64().unwrap_or(0) + 2 * v["ng"].as_u64().unwrap_or(0)).to_string());
    let subs = rows("subs", &|code, v| {
        let p = v["p"].as_str().expect("regions.json: a subdivision without parent");
        if p == &code[..2] { String::new() } else { p.to_string() }
    });
    std::thread::scope(|s| {
        for (name, rows) in [("countries", countries), ("subs", subs)] {
            let out = &out;
            s.spawn(move || {
                let text = names::encode(rows);
                let mut br = Vec::new();
                let params = brotli::enc::BrotliEncoderParams { quality: 11, lgwin: 22, ..Default::default() };
                brotli::BrotliCompress(&mut text.as_bytes(), &mut br, &params).unwrap();
                fs::write(format!("{out}/regions-{name}.br"), br).unwrap();
            });
        }
    });
    fs::write(format!("{out}/regions-cldr.txt"), data["cldr"].as_str().expect("regions.json: cldr")).unwrap();
}

fn main() {
    assets();
    regions();
    println!("cargo:rerun-if-changed=Cargo.toml");
    let manifest = fs::read_to_string("Cargo.toml").expect("read Cargo.toml");
    let mut tags = manifest
        .lines()
        .filter(|l| l.contains("github.com/lennart-k/rustical\""))
        .filter_map(|l| l.split("tag = \"").nth(1))
        .filter_map(|l| l.split('"').next())
        .map(|t| t.trim_start_matches('v').to_string())
        .collect::<Vec<_>>();
    tags.sort();
    tags.dedup();
    assert_eq!(tags.len(), 1, "all rustical crates must pin ONE identical tag, found {tags:?}");
    // The image ships RustiCal's own binary (Dockerfile `COPY --from=ghcr.io/lennart-k/rustical:<tag>`):
    // it must be the release the crates are. The Docker build context holds no Dockerfile, so this
    // runs on every local build; in CI the image test compares the two binaries' versions.
    println!("cargo:rerun-if-changed=Dockerfile");
    if let Ok(df) = fs::read_to_string("Dockerfile") {
        let image: Vec<&str> = df.lines().filter(|l| l.trim_start().starts_with("COPY --from=ghcr.io/lennart-k/rustical:")).collect();
        assert_eq!(image.len(), 1, "the Dockerfile must copy ONE rustical image, found {image:?}");
        let tag = image[0].split("rustical:").nth(1).and_then(|t| t.split(['@', ' ']).next()).unwrap_or_default();
        assert_eq!(tag.trim_start_matches('v'), tags[0], "Dockerfile rustical image {tag} ≠ rustical crates v{}", tags[0]);
    }
    println!("cargo:rustc-env=RUSTICAL_TAG={}", tags[0]);
}
