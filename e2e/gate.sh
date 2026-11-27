#!/bin/sh
# The checks a change passes, cheapest first; stops at the first failure.
#   gate.sh quick            cargo test → API doc (rustdoc, -D warnings: a broken intra-doc link fails; = mise run doc) →
#                            docs-nav --check → site link check (site/build.mjs --check) → server build (E2E_PROFILE, default e2e) → functional suite
#   gate.sh full             quick → chaos (the seeded UI monkey, e2e/chaos, ~60 s) → a11y (axe over fixed states against
#                            a11y-allow.txt + keyboard flows, e2e/a11y, ~30 s; a11y-check.mjs) → visual (the key screens
#                            against the references in e2e/visual/__screenshots__, seconds). The gate of an ordinary
#                            change; an intended look change re-blesses the references (mise run visual -- --update-snapshots).
# Env as for the suites: E2E_PORT (functional: E2E_PORT … +2×E2E_WORKERS−1), E2E_FIXTURE, E2E_WORKERS (default 4). Runs in
# the VM container (vm.sh): ports are the container's own.
set -eu
[ -n "${E2E_VM_INSIDE:-}" ] || exec "$(dirname "$0")/vm.sh" "$0" "$@" # in the VM container (vm.sh)
unset E2E_SUITE # the functional suite first
E2E="$(cd "$(dirname "$0")" && pwd -P)"
MODE="${1:-}"
case "$MODE" in
  quick|full) ;;
  *) echo "usage: gate.sh quick | full" >&2; exit 2 ;;
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
  echo "== visual (the key screens against e2e/visual/__screenshots__; an intended change: mise run visual -- --update-snapshots)"
  E2E_SUITE=visual npx playwright test
  step "visual"
fi
printf "gate %s passed in %s s:\n$SUMMARY" "$MODE" "$(( $(date +%s) - T0 ))"
