#!/bin/sh
# The checks a change passes, cheapest first; stops at the first failure.
#   gate.sh quick            cargo test → server build (E2E_PROFILE, default e2e) → functional suite
#   gate.sh full <ref-dir>   quick → visual baseline of this checkout → compare against <ref-dir> (or E2E_REF): a
#                            baseline of the change's base commit, same fixture, same machine → every colour rule
#                            shown by a dark state (theme-cover.mjs)
# Env as for the suites: E2E_PORT (functional: E2E_PORT … +2×E2E_WORKERS−1), E2E_FIXTURE, E2E_WORKERS (default 4),
# E2E_OUT (the new baseline, default ~/.cache/practical-e2e/<sha>[+dirty]). quick has no visual part: no rule
# derives the states a change can reach from the files it touches (a CSS rule, a template, a locale string or a
# Rust value can show on any surface) — whatever can change how a page looks needs `full`.
set -eu
unset E2E_SUITE # the functional suite first; baseline.sh selects the visual one itself
E2E="$(cd "$(dirname "$0")" && pwd -P)"
MODE="${1:-}"
case "$MODE" in
  quick) ;;
  full) REF="${2:-${E2E_REF:-}}"
        [ -n "$REF" ] && [ -f "$REF/.e2e-baseline" ] || { echo "gate full: no reference baseline (${REF:-none}); make one on the base commit: mise run e2e-baseline -- <dir>" >&2; exit 2; } ;;
  *) echo "usage: gate.sh quick | full <reference-baseline-dir>" >&2; exit 2 ;;
esac
T0=$(date +%s); LAST=$T0; SUMMARY=""
step() { NOW=$(date +%s); SUMMARY="$SUMMARY  $1 $((NOW - LAST)) s\n"; LAST=$NOW; }
echo "== cargo test"
"$E2E/cargo.sh" test
step "cargo test"
cd "$E2E"
[ -d node_modules ] || npm ci --no-fund --no-audit
npx playwright install chromium >/dev/null
BIN="$E2E/.work/run-${E2E_PORT:-4101}/practical"
echo "== server build (${E2E_PROFILE:-e2e} profile)"
./build-server.sh "$BIN"
step "server build"
export E2E_SKIP_BUILD=1
echo "== functional suite"
npx playwright test
step "functional"
if [ "$MODE" = full ]; then
  REV="$(git -C "$E2E/.." rev-parse --short HEAD)$( [ -n "$(git -C "$E2E/.." status --porcelain -- build)" ] && echo +dirty || true)"
  OUT="${E2E_OUT:-$HOME/.cache/practical-e2e/$REV}"
  echo "== visual baseline → $OUT"
  E2E_SHOT_REF="$REF" ./baseline.sh "$OUT" # a first screenshot equal to the reference's needs no second (lib/capture.mjs)
  step "visual"
  echo "== compare $REF → $OUT"
  node compare.mjs "$REF" "$OUT"
  step "compare"
  echo "== colour rules shown in dark (theme-cover.mjs)"
  node theme-cover.mjs "$OUT"
  step "theme-cover"
fi
printf "gate %s passed in %s s:\n$SUMMARY" "$MODE" "$(( $(date +%s) - T0 ))"
