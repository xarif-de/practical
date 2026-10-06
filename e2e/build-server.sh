#!/bin/sh
# Release build of THIS checkout's server into a target dir of its own (E2E_TARGET_DIR, default
# ~/.cache/practical-e2e/target-<hash of the checkout path>, ~2.5 GB each): build/.cargo/config.toml's
# target dir is shared by all worktrees, and cargo judges freshness by the dep-info of whichever
# checkout built last — it then reports ANOTHER worktree's binary as up to date (seen 2026-10-06).
# Also copies build.rs' minified assets (what the binary embeds) to <dest-binary>.assets/: the e2e dev
# server serves THOSE, so the suites test the shipped JS/CSS (global-setup.mjs, E2E_ASSETS).
# Usage: build-server.sh <dest-binary>
set -eu
case "$1" in /*) DEST=$1 ;; *) DEST="$PWD/$1" ;; esac
cd "$(dirname "$0")/../build"
export RUSTUP_TOOLCHAIN="$(sed -n 's/^channel = "\(.*\)"/\1/p' rust-toolchain.toml)"
export CARGO_TARGET_DIR="${E2E_TARGET_DIR:-$HOME/.cache/practical-e2e/target-$(pwd -P | shasum | cut -c1-12)}"
mkdir -p "$(dirname "$DEST")"
if ! cargo build --release --message-format=json-render-diagnostics > "$DEST.cargo.json" 2> "$DEST.cargo.log"; then
  cat "$DEST.cargo.log" >&2; exit 1
fi
rm -f "$DEST.cargo.log"
# the executable cargo reported for THIS build (not whatever lies in the shared dir later)
BIN="$(sed -n 's/.*"executable":"\([^"]*\/practical\)".*/\1/p' "$DEST.cargo.json" | tail -1)"
# the OUT_DIR of THIS build's practical build script (assets.rs lists the files)
OUT="$(sed -n 's/.*"reason":"build-script-executed","package_id":"[^"]*practical[#@][^"]*".*"out_dir":"\([^"]*\)".*/\1/p' "$DEST.cargo.json" | tail -1)"
rm -f "$DEST.cargo.json"
[ -n "$BIN" ] || { echo "cargo reported no practical executable" >&2; exit 1; }
[ -f "$OUT/assets.rs" ] || { echo "cargo reported no practical build-script out_dir ($OUT)" >&2; exit 1; }
rm -rf "$DEST.assets" && mkdir -p "$DEST.assets"
for f in $(sed -n 's/.*name: "\([^"]*\)".*/\1/p' "$OUT/assets.rs"); do cp "$OUT/$f" "$DEST.assets/$f"; done
cp "$BIN" "$DEST.tmp" && mv "$DEST.tmp" "$DEST"
