#!/bin/sh
# vm.sh <command> [args…]: runs a command of this checkout in its Linux container in the colima/Docker VM. Every local
# build and test goes through here (the mise tasks; cargo.sh, build-server.sh, gate.sh, baseline.sh and seed.sh
# re-exec themselves through it): never cargo or Playwright on the macOS host — Microsoft Defender scans every new
# host file (CPU), and a target dir is 1.6-4 GB of them.
#   vm.sh --serve <port> <command> [args…]: a throwaway container of the same image that publishes 127.0.0.1:<port>
#   (`mise run dev`); takes no heavy lock.
# Image practical-e2e-vm:<hash of e2e/vm/Dockerfile + toolchain channel + Playwright version>, built here on first use
# and after a change of one of them (former tags no container uses are removed). Container practical-vm-<checkout dir
# name>: kept between runs (warm), `--init` (reaps orphans), recreated when image or mounts change. Same paths as on
# the host, so every printed path and every report is valid there:
#   <checkout> rw · git common dir (worktrees) ro · ~/.cache/practical-e2e rw (baselines, bench dirs, bench DBs).
#   Ref/out dirs elsewhere are not seen: an absolute path argument outside these fails here.
# Fleet cache layout (ai/skills building-rust-projects scripts/rust-dev-container.sh): volumes rust-dev-cargo (cargo
# registry) + rust-dev-mbx (/vol: mbx store /vol/mbx, this checkout's target /vol/targets/<container> = /target);
# cargo runs through mbx (E2E_CARGO=mbx, E2E_TARGET_DIR=/target). Own container names (practical-vm-…, not
# rust-dev-…): a recreate never touches a rust-dev container of the same checkout.
# Heavy lock: a run holds ~/.cache/practical-e2e/heavy.lock (flock on the host: flock does not cross the VM mount) for
# its whole duration, so the runs of all worktrees queue host-wide. Nested calls (PRACTICAL_HEAVY_LOCK=1) and --serve
# do not lock again. Never wrap a vm.sh call in the lock by hand: it would wait for itself.
# Env into the container: E2E_* (except E2E_TARGET_DIR/E2E_NATIVE/E2E_CARGO/E2E_SCCACHE), BENCH_*, CHAOS_*, A11Y_*,
# COMPARE_*, SHOTS, SEED_PORT, DEV_USER, DB; more by name: E2E_VM_ENV="A B". Host RUSTFLAGS/RUSTDOCFLAGS stay out.
# Interrupt: Ctrl-C reaches the command through the TTY; without one, SIGINT/TERM/HUP here stops the command's process
# group in the container.
# E2E_NATIVE=1: the command runs on the host instead (lock still held). Diagnosis only, never routine: it builds
# natively, and its baselines and bench numbers are not comparable with the VM's.
set -eu
[ -z "${E2E_VM_INSIDE:-}" ] || exec "$@" # already in the container (or native)
REPO="$(cd "$(dirname "$0")/.." && pwd -P)"
CACHE="$HOME/.cache/practical-e2e"
SERVE=""
if [ "${1:-}" = --serve ]; then SERVE=$2; shift 2; fi
[ $# -gt 0 ] || { echo "usage: vm.sh [--serve <port>] <command> [args…]" >&2; exit 2; }
mkdir -p "$CACHE"
if [ -z "$SERVE" ] && [ -z "${PRACTICAL_HEAVY_LOCK:-}" ]; then
  export PRACTICAL_HEAVY_LOCK=1
  exec perl -MFcntl=:flock -e '
    open(my $f, ">", "$ENV{HOME}/.cache/practical-e2e/heavy.lock") or die "heavy.lock: $!";
    unless (flock($f, LOCK_EX | LOCK_NB)) {
      print STDERR "vm.sh: waiting for ~/.cache/practical-e2e/heavy.lock (another heavy run; a vm.sh call wrapped in the lock by hand waits for itself)\n";
      flock($f, LOCK_EX) or die "flock: $!";
    }
    my $pid = fork() // die "fork: $!";
    exec(@ARGV) or die "exec: $!" unless $pid;
    $SIG{$_} = do { my $s = $_; sub { kill $s, $pid } } for qw(INT TERM HUP); # → the trap below stops the container side
    waitpid($pid, 0); exit($? & 127 ? 128 + ($? & 127) : $? >> 8);' -- "$0" "$@"
fi
if [ "${E2E_NATIVE:-}" = 1 ]; then
  echo "vm.sh: E2E_NATIVE=1 — running natively on the host (diagnosis only)" >&2
  export E2E_VM_INSIDE=1; exec "$@"
fi
case "$REPO/" in "$HOME"/*) ;; *) echo "vm.sh: $REPO is not under \$HOME: colima would mount it empty" >&2; exit 2 ;; esac
case "$REPO$HOME" in *[[:space:]]*) echo "vm.sh: a path with white space ($REPO)" >&2; exit 2 ;; esac
GITDIR="$(git -C "$REPO" rev-parse --path-format=absolute --git-common-dir)"
for a in "$@"; do
  case "$a" in
    "$REPO"|"$REPO"/*|"$CACHE"|"$CACHE"/*) ;;
    /*) echo "vm.sh: $a is outside the paths the container shares with the host ($REPO, $CACHE): use a dir there" >&2; exit 2 ;;
  esac
done

TOOLCHAIN="$(sed -n 's/^channel = "\(.*\)"/\1/p' "$REPO/build/rust-toolchain.toml")"
PLAYWRIGHT="$(sed -n 's/.*"@playwright\/test": "\([^"]*\)".*/\1/p' "$REPO/e2e/package.json")"
IMAGE="practical-e2e-vm:$({ cat "$REPO/e2e/vm/Dockerfile"; echo "$TOOLCHAIN $PLAYWRIGHT"; } | shasum | cut -c1-12)"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  echo "vm.sh: building $IMAGE (Rust $TOOLCHAIN, Playwright $PLAYWRIGHT; once per change of e2e/vm/Dockerfile)" >&2
  docker build -q -t "$IMAGE" --build-arg TOOLCHAIN="$TOOLCHAIN" --build-arg PLAYWRIGHT="$PLAYWRIGHT" - < "$REPO/e2e/vm/Dockerfile" >/dev/null
  BUILT=1
fi

NAME="practical-vm-$(basename "$REPO" | tr -c 'a-zA-Z0-9_.-\n' '-')"
TZ_HOST="$(readlink /etc/localtime 2>/dev/null | sed -n 's#.*/zoneinfo/##p')"
# the container's mounts + env: no white space in them (checked above), split on purpose below
OPTS="-v $REPO:$REPO -v $CACHE:$CACHE -v rust-dev-cargo:/usr/local/cargo/registry -v rust-dev-mbx:/vol
  -e HOME=$HOME -e TZ=${TZ_HOST:-UTC} -e CARGO_TARGET_DIR=/target -e MBX_CACHE_DIR=/vol/mbx -e MBX_DISPLAY=plain
  -e E2E_VM_INSIDE=1 -e E2E_TARGET_DIR=/target -e E2E_CARGO=mbx -e E2E_SCCACHE=0 -e PRACTICAL_E2E_VM=$IMAGE"
case "$GITDIR/" in "$REPO"/*) ;; *) OPTS="$OPTS -v $GITDIR:$GITDIR:ro" ;; esac
ENVS=""
for k in $(awk 'BEGIN { for (k in ENVIRON) if (k ~ /^(E2E_|BENCH_|CHAOS_|A11Y_|COMPARE_)/ || k ~ /^(SHOTS|SEED_PORT|DEV_USER|DB)$/) print k }') ${E2E_VM_ENV:-}; do
  case "$k" in E2E_TARGET_DIR|E2E_NATIVE|E2E_CARGO|E2E_SCCACHE|E2E_VM_*) ;; *) ENVS="$ENVS -e $k" ;; esac
done
TTY=""; if [ -t 0 ] && [ -t 1 ]; then TTY="-it"; fi
case "$PWD/" in "$REPO"/*) WD=$PWD ;; *) WD=$REPO ;; esac

if [ -n "$SERVE" ]; then
  # dev server: a container of its own with the published port; ~/.cache/practical-dev and DB's dir at their host paths
  DEV="$HOME/.cache/practical-dev"; mkdir -p "$DEV"
  OPTS="$OPTS -v $DEV:$DEV"
  if [ -n "${DB:-}" ]; then
    D="$(cd "$(dirname "$DB")" && pwd -P)"
    case "$D/" in "$HOME/") echo "vm.sh: DB $DB: put it in a directory of its own (e.g. ~/.cache/practical-dev), not \$HOME itself" >&2; exit 2 ;;
      "$HOME"/*) ;; *) echo "vm.sh: DB $DB is not under \$HOME: colima would mount it empty" >&2; exit 2 ;; esac
    case "$D/" in "$REPO"/*|"$DEV"/*|*[[:space:]]*) ;; *) OPTS="$OPTS -v $D:$D" ;; esac
  fi
  # shellcheck disable=SC2086 # split on purpose
  exec docker run --rm --init $TTY --name "$NAME-serve" -p "127.0.0.1:$SERVE:$SERVE" --ulimit nofile=65536:65536 \
    --shm-size=1g $OPTS $ENVS -w "$WD" "$IMAGE" sh -c 'ln -sfn "/vol/targets/$0" /target && exec "$@"' "$NAME" "$@"
fi

SPEC="$(printf '%s\n' "$IMAGE" $OPTS | shasum | cut -c1-12)"
if [ "$(docker inspect -f '{{index .Config.Labels "practical.vm"}}' "$NAME" 2>/dev/null || true)" != "$SPEC" ]; then
  docker rm -f "$NAME" >/dev/null 2>&1 || true # ours only (practical-vm-*): a former image or mount set
  # shm: Chromium's renderers share memory there (docker's 64 MB default crashes tabs)
  # shellcheck disable=SC2086
  docker run -d --init --name "$NAME" --label "practical.vm=$SPEC" --ulimit nofile=65536:65536 --shm-size=2g \
    $OPTS -w "$REPO" "$IMAGE" sleep infinity >/dev/null
  docker exec "$NAME" sh -c 'mkdir -p "/vol/targets/$1" && ln -sfn "/vol/targets/$1" /target && mbx settings set gc.max_total_size "$2" >/dev/null' \
    sh "$NAME" "${MBX_BUDGET:-30GiB}"
  echo "vm.sh: container $NAME created ($IMAGE)" >&2
elif [ "$(docker inspect -f '{{.State.Running}}' "$NAME")" != true ]; then
  docker start "$NAME" >/dev/null
fi
if [ -n "${BUILT:-}" ]; then # former images (one a container of another checkout still uses: docker refuses, kept)
  for old in $(docker image ls --format '{{.Repository}}:{{.Tag}}' practical-e2e-vm); do
    [ "$old" = "$IMAGE" ] || docker image rm "$old" >/dev/null 2>&1 || true
  done
fi

# the command in a process group of its own (setsid), its PGID in a file: an interrupt without TTY stops exactly it
RUN="/tmp/vm-run-$$"
stop() {
  trap - INT TERM HUP
  docker exec "$NAME" sh -c 'p=$(cat "$1" 2>/dev/null) && kill -TERM "-$p"; rm -f "$1"' sh "$RUN" || true
  exit "$1"
}
# shellcheck disable=SC2086
set -- docker exec $TTY $ENVS -w "$WD" "$NAME" setsid -w sh -c 'echo $$ > "$0"; "$@"; r=$?; rm -f "$0"; exit $r' "$RUN" "$@"
if [ -n "$TTY" ]; then exec "$@"; fi
trap 'stop 130' INT; trap 'stop 143' TERM; trap 'stop 129' HUP
"$@" &
CHILD=$!
set +e
wait "$CHILD"; RC=$?
# a trapped signal interrupts `wait` (the trap exits); otherwise RC is the command's exit code
exit "$RC"
