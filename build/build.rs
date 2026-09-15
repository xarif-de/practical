// Exports the RustiCal git tag pinned in Cargo.toml as RUSTICAL_TAG so the
// binary can compare itself against the rustical image it runs next to
// (`rustical-ui check-rustical-image <image-ref>`, used by the deployment
// repo's tool_check). Parsed from Cargo.toml on purpose: the tag IS the
// contract, and Renovate bumps exactly that line.
use std::fs;

fn main() {
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
    println!("cargo:rustc-env=RUSTICAL_TAG={}", tags[0]);
}
