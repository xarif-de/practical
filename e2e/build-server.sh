#!/bin/sh
# Release build of THIS checkout's server into a target dir of its own (E2E_TARGET_DIR, default
# ~/.cache/practical-e2e/target-<hash of the checkout path>, ~2.5 GB each): build/.cargo/config.toml's
# target dir is shared by all worktrees, and cargo judges freshness by the dep-info of whichever
# checkout built last — it then reports ANOTHER worktree's binary as up to date (seen 2026-10-06).
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
rm -f "$DEST.cargo.json"
[ -n "$BIN" ] || { echo "cargo reported no practical executable" >&2; exit 1; }
cp "$BIN" "$DEST.tmp" && mv "$DEST.tmp" "$DEST"
