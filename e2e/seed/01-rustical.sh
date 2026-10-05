#!/bin/sh
# Stage 1 of the e2e fixture: a FRESH, migrated RustiCal store with principals, a group, memberships and
# app tokens, made by RustiCal's own CLI from the image the Dockerfile pins (same release as the crates).
# Usage: 01-rustical.sh <workdir>  → <workdir>/db.sqlite3 (+ -wal/-shm) and <workdir>/tokens.txt
# Synthetic data only; the passwords and tokens are throwaway values for a local test DB.
set -eu
OUT=$1
REPO="$(cd "$(dirname "$0")/../.." && pwd)"
IMG="$(sed -n 's/^COPY --from=\(ghcr\.io\/lennart-k\/rustical:[^ ]*\) .*/\1/p' "$REPO/build/Dockerfile")"
[ -n "$IMG" ] || { echo "no rustical image in build/Dockerfile" >&2; exit 1; }
VOL=practical-e2e-seed-$$
CP=practical-e2e-seedcp-$$
mkdir -p "$OUT"; rm -f "$OUT"/db.sqlite3*
docker volume create "$VOL" >/dev/null
trap 'docker rm -fv "$CP" >/dev/null 2>&1; docker volume rm "$VOL" >/dev/null 2>&1' EXIT
r() { docker run --rm -u 0 -v "$VOL:/var/lib/rustical" "$IMG" /usr/local/bin/rustical "$@"; }
r principals create alice -n "Alice Beispiel" --for-testing-password-from-arg devpass >/dev/null
r principals create bob   -n "Bob Muster"     --for-testing-password-from-arg devpass >/dev/null
r principals create carol -n "Carol Test"     --for-testing-password-from-arg devpass >/dev/null
r principals create group.familie -p group -n "Familie" >/dev/null
r principals membership assign alice --to group.familie >/dev/null
r principals membership assign bob   --to group.familie >/dev/null
: > "$OUT/tokens.txt"
for name in "Thunderbird Laptop" "DAVx5 Handy" "seed"; do
  printf '%s\t%s\n' "$name" "$(r principals app-token create --name "$name" alice | tail -1)" >> "$OUT/tokens.txt"
done
docker create --name "$CP" -v "$VOL:/var/lib/rustical" "$IMG" >/dev/null
for f in db.sqlite3 db.sqlite3-wal db.sqlite3-shm; do docker cp "$CP:/var/lib/rustical/$f" "$OUT/$f" 2>/dev/null || true; done
[ -s "$OUT/db.sqlite3" ] || { echo "stage 1 produced no db.sqlite3" >&2; exit 1; }
