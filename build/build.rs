// Exports the RustiCal git tag pinned in Cargo.toml as RUSTICAL_TAG (shown by
// `--version` and in the UI's source note; `check-rustical-image <ref>` compares
// an image reference with it). Parsed from Cargo.toml on purpose: the tag IS the
// contract, and Renovate bumps exactly that line — together with the Dockerfile's
// RustiCal image, which is checked against it below.
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
