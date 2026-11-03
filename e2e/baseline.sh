#!/bin/sh
# Visual baseline of THIS checkout: every state × projects → PNG + DOM + style artifacts.
# Usage: baseline.sh [outdir]   default ~/.cache/practical-e2e/<git-short-sha>[+dirty]; outdir under ~/.cache/practical-e2e or
# the checkout (runs in the VM container, vm.sh, which sees only those)
# The outdir is replaced only if it holds a previous baseline (marker .e2e-baseline) or is empty.
set -eu
[ -n "${E2E_VM_INSIDE:-}" ] || exec "$(dirname "$0")/vm.sh" "$0" "$@" # in the VM container (vm.sh)
E2E="$(cd "$(dirname "$0")" && pwd)"
REV="$(git -C "$E2E/.." rev-parse --short HEAD)$( [ -n "$(git -C "$E2E/.." status --porcelain -- build)" ] && echo +dirty || true)"
OUT="${1:-$HOME/.cache/practical-e2e/$REV}"
case "$OUT" in /*) ;; *) OUT="$PWD/$OUT" ;; esac
if [ -d "$OUT" ] && [ -n "$(ls -A "$OUT")" ]; then
  [ -f "$OUT/.e2e-baseline" ] || { echo "$OUT is not empty and holds no .e2e-baseline marker — refusing to overwrite" >&2; exit 2; }
  rm -rf "$OUT"
fi
mkdir -p "$OUT"
# env = where it was taken (the VM image, vm.sh; native-Darwin under E2E_NATIVE=1): gate.sh visual compares only equal ones
printf 'rev %s\nfixture-sha256 %s\ncreated %s\nenv %s\n' "$REV" "$(shasum -a 256 "${E2E_FIXTURE:-$E2E/.work/fixture.sqlite3}" | cut -d' ' -f1)" "$(date -u +%FT%TZ)" "${PRACTICAL_E2E_VM:-native-$(uname -s)}" > "$OUT/.e2e-baseline"
cd "$E2E"
[ -d node_modules ] || npm ci --no-fund --no-audit
START=$(date +%s)
E2E_SUITE=visual E2E_OUT="$OUT" npx playwright test
echo "baseline of $REV in $OUT ($(( $(date +%s) - START )) s)"
