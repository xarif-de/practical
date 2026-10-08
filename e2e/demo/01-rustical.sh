#!/bin/sh
# Stage 1 of the README showcase DB (e2e/demo): a fresh, migrated RustiCal store with the demo principals, made by
# RustiCal's own CLI from the image build/Dockerfile pins — e2e/seed/01-rustical.sh with other people.
# Usage: 01-rustical.sh <workdir>  → <workdir>/db.sqlite3 (+ -wal/-shm). Synthetic data; throwaway password.
set -eu
OUT=$1
REPO="$(cd "$(dirname "$0")/../.." && pwd)"
IMG="$(sed -n 's/^COPY --from=\(ghcr\.io\/lennart-k\/rustical:[^ ]*\) .*/\1/p' "$REPO/build/Dockerfile")"
[ -n "$IMG" ] || { echo "no rustical image in build/Dockerfile" >&2; exit 1; }
VOL=practical-demo-seed-$$
CP=practical-demo-seedcp-$$
mkdir -p "$OUT"; rm -f "$OUT"/db.sqlite3*
docker volume create "$VOL" >/dev/null
trap 'docker rm -fv "$CP" >/dev/null 2>&1; docker volume rm "$VOL" >/dev/null 2>&1' EXIT
r() { docker run --rm -u 0 -v "$VOL:/var/lib/rustical" "$IMG" /usr/local/bin/rustical "$@"; }
r principals create emma  -n "Emma Clark"  --for-testing-password-from-arg devpass >/dev/null
r principals create james -n "James Clark" --for-testing-password-from-arg devpass >/dev/null
r principals create group.family -p group -n "Clark Family" >/dev/null
r principals membership assign emma  --to group.family >/dev/null
r principals membership assign james --to group.family >/dev/null
docker create --name "$CP" -v "$VOL:/var/lib/rustical" "$IMG" >/dev/null
for f in db.sqlite3 db.sqlite3-wal db.sqlite3-shm; do docker cp "$CP:/var/lib/rustical/$f" "$OUT/$f" 2>/dev/null || true; done
[ -s "$OUT/db.sqlite3" ] || { echo "stage 1 produced no db.sqlite3" >&2; exit 1; }
