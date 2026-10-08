#!/bin/sh
# Builds the README showcase DB (default e2e/.work/demo/demo.sqlite3): stage 1 = RustiCal CLI in docker
# (01-rustical.sh), stage 2 = PractiCal HTTP with frozen clock and blocked egress (02-demo.py).
# Usage: seed.sh <practical-binary> [out.sqlite3]. SEED_PORT = the seeding server's port (default 4111).
set -eu
E2E="$(cd "$(dirname "$0")/.." && pwd)"
BIN=$1
OUT="${2:-$E2E/.work/demo/demo.sqlite3}"
WORK="$E2E/.work/demo/seed"
rm -rf "$WORK"; mkdir -p "$WORK" "$(dirname "$OUT")"
"$E2E/demo/01-rustical.sh" "$WORK"
python3 "$E2E/demo/02-demo.py" "$BIN" "$WORK" "$OUT"
rm -rf "$WORK"
