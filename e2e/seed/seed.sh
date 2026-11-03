#!/bin/sh
# Builds the e2e fixture DB: stage 1 (RustiCal CLI in docker) + stage 2 (PractiCal HTTP, frozen clock,
# blocked egress), then pins ids and timestamps. Usage: seed.sh [out.sqlite3]
# Default out: e2e/.work/fixture.sqlite3 (gitignored). Stage 1 runs on the host (docker run of the RustiCal image),
# then this script again in the VM container (vm.sh): server build + stage 2 (python3 there).
set -eu
E2E="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$E2E/.work/fixture.sqlite3}"
WORK="$E2E/.work/seed"
if [ -z "${E2E_VM_INSIDE:-}" ]; then
  rm -rf "$WORK"; mkdir -p "$WORK" "$(dirname "$OUT")"
  "$E2E/seed/01-rustical.sh" "$WORK"
  exec "$E2E/vm.sh" "$0" "$@"
fi
"$E2E/build-server.sh" "$WORK/practical"
python3 "$E2E/seed/02-practical.py" "$WORK/practical" "$WORK" "$OUT"
rm -f "$WORK"/db.sqlite3-wal "$WORK"/db.sqlite3-shm
