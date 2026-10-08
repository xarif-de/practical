#!/bin/sh
# The checks a change passes, cheapest first; stops at the first failure.
#   gate.sh quick            cargo test → API doc (rustdoc, -D warnings: a broken intra-doc link fails; = mise run doc) →
#                            docs-nav --check → site link check (site/build.mjs --check) → server build (E2E_PROFILE, default e2e) → functional suite
#   gate.sh full             quick → chaos (the seeded UI monkey, e2e/chaos, ~60 s) → a11y (axe over fixed states against
#                            a11y-allow.txt + keyboard flows, e2e/a11y, ~30 s; a11y-check.mjs). The gate of an ordinary
#                            change: no reference baseline to build, nothing to compare.
#   gate.sh visual <ref-dir> full → visual baseline of this checkout → compare against <ref-dir> (or E2E_REF): a
#                            baseline of the change's base commit, same fixture, same machine → every colour rule
#                            shown by a dark state (theme-cover.mjs)
#
# The visual part is OPT-IN (decision 2026-10-09). It answers one question — "this rewrite changes no pixel" — and
# only a deliberate refactoring asks it. While surfaces are being designed, every state differs on purpose, so the
# comparison reports hundreds of intended differences and nobody reads them; meanwhile it costs the run itself plus a
# reference baseline of the base commit in a second worktree (161-262 s each, measured 2026-10-08/09). Reach for it
# when the point of the change is that nothing may look different.
# Env as for the suites: E2E_PORT (functional: E2E_PORT … +2×E2E_WORKERS−1), E2E_FIXTURE, E2E_WORKERS (default 4),
# E2E_OUT (the new baseline, default ~/.cache/practical-e2e/<sha>[+dirty]).
set -eu
unset E2E_SUITE # the functional suite first; baseline.sh selects the visual one itself
E2E="$(cd "$(dirname "$0")" && pwd -P)"
MODE="${1:-}"
case "$MODE" in
  quick|full) ;;
  visual) REF="${2:-${E2E_REF:-}}"
        [ -n "$REF" ] && [ -f "$REF/.e2e-baseline" ] || { echo "gate visual: no reference baseline (${REF:-none}); make one on the base commit: mise run e2e-baseline -- <dir>" >&2; exit 2; } ;;
  *) echo "usage: gate.sh quick | full | visual <reference-baseline-dir>" >&2; exit 2 ;;
esac
T0=$(date +%s); LAST=$T0; SUMMARY=""
step() { NOW=$(date +%s); SUMMARY="$SUMMARY  $1 $((NOW - LAST)) s\n"; LAST=$NOW; }
echo "== cargo test"
"$E2E/cargo.sh" test
step "cargo test"
echo "== API doc (rustdoc -D warnings)"
RUSTDOCFLAGS="${RUSTDOCFLAGS:+$RUSTDOCFLAGS }-D warnings" "$E2E/cargo.sh" doc --no-deps --document-private-items
step "doc"
echo "== docs navigation (mise run docs-nav)"
node "$E2E/demo/docs-nav.mjs" --check
step "docs-nav"
echo "== site links (site/build.mjs --check: every internal link and anchor of the Pages site and the README)"
[ "$E2E/../site/node_modules/.package-lock.json" -nt "$E2E/../site/package-lock.json" ] || (cd "$E2E/../site" && npm ci --no-fund --no-audit)
node "$E2E/../site/build.mjs" --check
step "site links"
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
if [ "$MODE" != quick ]; then
  echo "== chaos (seeded UI monkey + network chaos, one worker)"
  E2E_SUITE=chaos npx playwright test
  step "chaos"
  echo "== a11y (axe over fixed states + keyboard flows; allow-list e2e/a11y-allow.txt only shrinks)"
  E2E_SUITE=a11y npx playwright test
  node a11y-check.mjs
  step "a11y"
fi
if [ "$MODE" = visual ]; then
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
