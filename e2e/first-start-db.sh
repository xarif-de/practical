#!/bin/sh
# A database in the state a brand-new user is in: one principal, no collections, no preferences.
# For judging the first start locally (`mise run dev` against it); throwaway password.
# Usage: first-start-db.sh [out.sqlite3]   (default ~/.cache/practical-dev/first-start.sqlite3)
set -eu
OUT="${1:-$HOME/.cache/practical-dev/first-start.sqlite3}"
REPO="$(cd "$(dirname "$0")/.." && pwd)"
IMG="$(sed -n 's/^COPY --from=\(registry\.gitlab\.com\/xarif\/docker\/rustical:[^ ]*\) .*/\1/p' "$REPO/build/Dockerfile")"
[ -n "$IMG" ] || { echo "no rustical image in build/Dockerfile" >&2; exit 1; }
VOL=practical-firststart-$$
CP=practical-firststartcp-$$
mkdir -p "$(dirname "$OUT")"; rm -f "$OUT" "$OUT-wal" "$OUT-shm"
docker volume create "$VOL" >/dev/null
trap 'docker rm -fv "$CP" >/dev/null 2>&1; docker volume rm "$VOL" >/dev/null 2>&1' EXIT
docker run --rm -u 0 -v "$VOL:/var/lib/rustical" "$IMG" \
  /usr/local/bin/rustical principals create thomass -n "Thomas" --for-testing-password-from-arg devpass >/dev/null
docker create --name "$CP" -v "$VOL:/var/lib/rustical" "$IMG" >/dev/null
for f in db.sqlite3 db.sqlite3-wal db.sqlite3-shm; do
  docker cp "$CP:/var/lib/rustical/$f" "$OUT$(echo "$f" | sed 's/db.sqlite3//')" 2>/dev/null || true
done
[ -s "$OUT" ] || { echo "no database produced" >&2; exit 1; }
echo "first-start DB at $OUT — login thomass / devpass"
