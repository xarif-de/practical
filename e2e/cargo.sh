#!/bin/sh
# cargo for this checkout's build/ — `mise run test` and build-server.sh run it: cargo.sh <subcommand> [args…]
#  - toolchain: build/rust-toolchain.toml (an exported RUSTUP_TOOLCHAIN would silently override the file).
#  - target dir: one per checkout, E2E_TARGET_DIR or ~/.cache/practical-e2e/target-<hash of build/'s path>, given as
#    --target-dir. build/.cargo/config.toml's dir is shared by all worktrees, and cargo then reported ANOTHER
#    worktree's binary as up to date (2026-10-06). CARGO_TARGET_DIR is unset: as a variable it reaches rustc and
#    would key every sccache entry to one checkout.
#  - content, not mtimes: cargo rebuilds what is NEWER than its last build, so a file changed with an old mtime
#    (cp -p, tar, rsync -t, a restored stash) would be skipped. The sha256 of every input file (build/ + LICENSE) is
#    kept per target dir and command; a file whose content differs from the last successful run's is touched first.
#    No record yet (a new target dir, or one from before this script): every input is touched once.
#  - sccache, if installed (E2E_SCCACHE=0: off; a RUSTC_WRAPPER of your own wins): the dependencies' compiler output,
#    keyed by content (sources from rustc's dep-info, env, extern crates), shared by all checkouts; the practical
#    crate itself is a bin, which sccache never caches. Without a reachable daemon rustc runs uncached
#    (SCCACHE_IGNORE_SERVER_IO_ERROR). ~74 % of the Rust crates hit from another checkout (a key that holds the
#    target dir via OUT_DIR misses, and so do the dependents). Cold build of a new checkout with a warm cache
#    (2026-10-06): cargo test 79 → 69 s, e2e server 129 → 95 s; the same dir again ~50 % less; filling the cache
#    costs ~10 % more than no sccache. SCCACHE_CLIENT_SIDE=1 was slower (98 s vs 69 s).
set -eu
REPO="$(cd "$(dirname "$0")/.." && pwd -P)"
cd "$REPO/build"
export RUSTUP_TOOLCHAIN="$(sed -n 's/^channel = "\(.*\)"/\1/p' rust-toolchain.toml)"
T="${E2E_TARGET_DIR:-$HOME/.cache/practical-e2e/target-$(pwd -P | shasum | cut -c1-12)}"
unset CARGO_TARGET_DIR
if [ "${E2E_SCCACHE:-1}" != 0 ] && [ -z "${RUSTC_WRAPPER:-}" ] && command -v sccache >/dev/null 2>&1; then
  export RUSTC_WRAPPER=sccache SCCACHE_IGNORE_SERVER_IO_ERROR=1
fi
SUB=$1; shift
mkdir -p "$T"
REC="$T/.inputs-$(printf '%s\n' "$SUB" "$@" | shasum | cut -c1-12)"
NOW="$REC.new.$$"
trap 'rm -f "$NOW"' EXIT
{ find . -type f -print0 | xargs -0 shasum -a 256; shasum -a 256 ../LICENSE; } | sort -k2 > "$NOW"
if [ -f "$REC" ]; then
  # lines of NOW not in REC: a changed or new file (path after the hash and two spaces)
  awk 'NR == FNR { old[$0] = 1; next } !($0 in old) { print substr($0, 67) }' "$REC" "$NOW" | while IFS= read -r f; do touch "$f"; done
else
  cut -c67- "$NOW" | while IFS= read -r f; do touch "$f"; done
fi
cargo "$SUB" --target-dir "$T" "$@"
mv "$NOW" "$REC"
