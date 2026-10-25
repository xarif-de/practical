#!/usr/bin/env bash
# Automatic patch release (policy 2026-10-25, CONTRIBUTING.md "Build versions and releases"):
# when renovate-bot commits that change the image (build/) reached main since the newest vX.Y.Z
# tag and main's CI is green, bump build/Cargo.toml + build/Cargo.lock by one patch, commit,
# tag vX.Y.(Z+1) and push both atomically. Whatever else is on main ships with it; the local
# gates (browser suites, a11y, bench) are caught up by the next local release.
#
# Run in a clone of main with its tags (.forgejo/workflows/release.yml, hourly). Env:
#   PUSH=1        push (otherwise everything up to the push: a dry run)
#   CI_STATE      combined commit status of HEAD; unset = read it from FORGEJO_API/REPOSITORY with TOKEN
# Exit 0 = released, nothing to release, or retry next run (CI pending, main moved meanwhile);
# exit 1 = a stop a human must look at (no tag, version drift, red CI) — the run fails and alerts.
set -euo pipefail

last=$(git tag --merged HEAD --list 'v*' | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' | sort -V | tail -1 || true)
[ -n "$last" ] || { echo "stop: no vX.Y.Z tag reachable from HEAD"; exit 1; }
changes=$(git log --format='- %h %s' --author='^renovate-bot <' "$last..HEAD" -- build/)
[ -n "$changes" ] || { echo "nothing to release: no renovate-bot change under build/ since $last"; exit 0; }

cur=$(awk -F'"' '/^\[/{p=($0=="[package]")} p && /^version = /{print $2; exit}' build/Cargo.toml)
[ "$cur" = "${last#v}" ] || { echo "stop: build/Cargo.toml says $cur, the newest tag is $last"; exit 1; }
IFS=. read -r ma mi pa <<<"$cur"
next="$ma.$mi.$((pa + 1))"
! git rev-parse -q --verify "refs/tags/v$next" >/dev/null || { echo "stop: v$next exists but is not on main"; exit 1; }

state=${CI_STATE:-$(curl -fsS -m 30 -H "Authorization: token $TOKEN" \
  "$FORGEJO_API/repos/$REPOSITORY/commits/$(git rev-parse HEAD)/status" | jq -r .state)}
case "$state" in
  success) ;;
  pending|"") echo "wait: CI of $(git rev-parse --short HEAD) is ${state:-not started}"; exit 0 ;;
  *) echo "stop: CI of $(git rev-parse --short HEAD) is $state"; exit 1 ;;
esac

# the [package] version of build/Cargo.toml and the practical entry of build/Cargo.lock, nothing else
awk -v c="$cur" -v n="$next" '/^\[/{p=($0=="[package]")} p && !d && $0=="version = \"" c "\"" {$0="version = \"" n "\""; d=1} {print}' \
  build/Cargo.toml > build/Cargo.toml.new && mv build/Cargo.toml.new build/Cargo.toml
awk -v c="$cur" -v n="$next" 'prev=="name = \"practical\"" && $0=="version = \"" c "\"" {$0="version = \"" n "\""} {print; prev=$0}' \
  build/Cargo.lock > build/Cargo.lock.new && mv build/Cargo.lock.new build/Cargo.lock
[ "$(git diff --numstat | awk '{a+=$1; d+=$2} END {print a "+" d "-"}')" = "2+2-" ] \
  || { echo "stop: the version bump did not change exactly two lines"; git diff; exit 1; }

msg="Automatic patch release: dependency updates since $last.

$changes"
# env, not -c user.*: GIT_AUTHOR_*/GIT_COMMITTER_* in the environment would win over the config
export GIT_AUTHOR_NAME=release-bot GIT_AUTHOR_EMAIL=release-bot@noreply.git.xarif.de
export GIT_COMMITTER_NAME=$GIT_AUTHOR_NAME GIT_COMMITTER_EMAIL=$GIT_AUTHOR_EMAIL
git commit -q -am "chore(release): PractiCal $next" -m "$msg"
git tag -a "v$next" -m "PractiCal $next" -m "$msg"
echo "release v$next ($(git rev-parse --short HEAD)) over $last:"; echo "$changes"
[ "${PUSH:-0}" = 1 ] || { echo "dry run: not pushed"; exit 0; }
# both or neither; a main that moved meanwhile rejects the push → the next run retries on the new head
if ! git push --atomic origin HEAD:refs/heads/main "refs/tags/v$next"; then
  git fetch -q origin main
  [ "$(git rev-parse origin/main)" != "$(git rev-parse HEAD~1)" ] && { echo "wait: main moved, next run retries"; exit 0; }
  echo "stop: push rejected"; exit 1
fi
