#!/bin/sh
# Builds THIS checkout's server with cargo.sh (per-checkout target dir, content-checked freshness, sccache) in the
# E2E_PROFILE cargo profile, default `e2e` (build/Cargo.toml: release + 256 codegen units + incremental; an edit
# rebuilds in ~6-14 s instead of ~100 s). E2E_PROFILE=release builds what ships (same opt-level; e2e differs only
# in codegen units and incremental compilation): use it for a release candidate and for a change to the profiles.
# Also copies build.rs' minified assets (what the binary embeds) to <dest-binary>.assets/: the e2e dev server
# serves THOSE, so the suites test the shipped JS/CSS (lib/servers.mjs, E2E_ASSETS).
# Usage: build-server.sh <dest-binary>
set -eu
case "$1" in /*) DEST=$1 ;; *) DEST="$PWD/$1" ;; esac
mkdir -p "$(dirname "$DEST")"
if ! "$(dirname "$0")/cargo.sh" build --profile "${E2E_PROFILE:-e2e}" --message-format=json-render-diagnostics > "$DEST.cargo.json" 2> "$DEST.cargo.log"; then
  cat "$DEST.cargo.log" >&2; exit 1
fi
# the gate's "no new warnings": cargo's summary line for the practical crate (replayed when nothing was rebuilt)
echo "build-server.sh: ${E2E_PROFILE:-e2e} profile, $(sed -n 's/^warning: `practical` (bin "practical") generated \([0-9]*\) warning.*/\1/p' "$DEST.cargo.log" | tail -1 | grep . || echo 0) warning(s) in practical" >&2
rm -f "$DEST.cargo.log"
# the executable cargo reported for THIS build (not whatever lies in the target dir later)
BIN="$(sed -n 's/.*"executable":"\([^"]*\/practical\)".*/\1/p' "$DEST.cargo.json" | tail -1)"
# the OUT_DIR of THIS build's practical build script (assets.rs lists the files)
OUT="$(sed -n 's/.*"reason":"build-script-executed","package_id":"[^"]*practical[#@][^"]*".*"out_dir":"\([^"]*\)".*/\1/p' "$DEST.cargo.json" | tail -1)"
rm -f "$DEST.cargo.json"
[ -n "$BIN" ] || { echo "cargo reported no practical executable" >&2; exit 1; }
[ -f "$OUT/assets.rs" ] || { echo "cargo reported no practical build-script out_dir ($OUT)" >&2; exit 1; }
rm -rf "$DEST.assets" && mkdir -p "$DEST.assets"
for f in $(sed -n 's/.*name: "\([^"]*\)".*/\1/p' "$OUT/assets.rs"); do cp "$OUT/$f" "$DEST.assets/$f"; done
cp "$BIN" "$DEST.tmp" && mv "$DEST.tmp" "$DEST"
