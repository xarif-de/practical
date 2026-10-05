#!/bin/sh
# Builds the e2e fixture DB: stage 1 (RustiCal CLI in docker) + stage 2 (PractiCal HTTP, frozen clock,
# blocked egress), then pins ids and timestamps. Usage: seed.sh [out.sqlite3]
# Default out: e2e/.work/fixture.sqlite3 (gitignored). Needs docker (colima), python3, cargo.
set -eu
E2E="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$E2E/.work/fixture.sqlite3}"
WORK="$E2E/.work/seed"
rm -rf "$WORK"; mkdir -p "$WORK" "$(dirname "$OUT")"
"$E2E/build-server.sh" "$WORK/practical"
"$E2E/seed/01-rustical.sh" "$WORK"
python3 "$E2E/seed/02-practical.py" "$WORK/practical" "$WORK" "$OUT"
rm -f "$WORK"/db.sqlite3-wal "$WORK"/db.sqlite3-shm
