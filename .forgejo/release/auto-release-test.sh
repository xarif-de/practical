#!/usr/bin/env bash
# Behavioural test of auto-release.sh with throwaway repos (no network): `mise run test-auto-release`.
set -u
S=$(cd "$(dirname "$0")" && pwd)/auto-release.sh
T=$(mktemp -d); trap 'rm -rf "$T"' EXIT
fail=0
export GIT_AUTHOR_NAME=dev GIT_AUTHOR_EMAIL=dev@x GIT_COMMITTER_NAME=dev GIT_COMMITTER_EMAIL=dev@x
ok() { echo "OK: $1"; }; no() { echo "FAIL: $1"; sed 's/^/    /' "$T/out"; fail=1; }
run() { (cd "$T/w" && env "$@" bash "$S") > "$T/out" 2>&1; echo $?; }
setup() { # main at v0.25.0 in a bare origin, working clone in $T/w
  rm -rf "$T"/*; git init -q --bare "$T/o.git"; git init -q -b main "$T/w"; cd "$T/w"
  mkdir -p build e2e
  printf '[package]\nname = "practical"\nversion = "0.25.0"\n\n[dependencies]\nfoo = { version = "0.25.0" }\n' > build/Cargo.toml
  printf '[[package]]\nname = "foo"\nversion = "0.25.0"\n\n[[package]]\nname = "practical"\nversion = "0.25.0"\n' > build/Cargo.lock
  echo 1 > e2e/package.json; git add -A; git commit -qm "chore(release): PractiCal 0.25.0"; git tag -a v0.25.0 -m v0.25.0
  git remote add origin "$T/o.git"; git push -q origin main v0.25.0; cd - >/dev/null
}
bot() { (cd "$T/w" && echo "$2" >> "$1" && git add -A && GIT_AUTHOR_NAME=renovate-bot GIT_AUTHOR_EMAIL=renovate-bot@xarif.de git commit -qm "fix(deps): $3" && git push -q origin main); }

echo "== nothing since the tag"; setup
[ "$(run CI_STATE=success PUSH=1)" = 0 ] && grep -q '^nothing to release' "$T/out" && ok "no release" || no "no release"

echo "== a renovate change outside build/ only"; bot e2e/package.json 2 "pixelmatch"
[ "$(run CI_STATE=success PUSH=1)" = 0 ] && grep -q '^nothing to release' "$T/out" && ok "e2e-only ignored" || no "e2e-only ignored"

echo "== a human commit under build/ alone does not release"; (cd "$T/w" && echo x >> build/src.rs && git add -A && git commit -qm "feat: x" && git push -q origin main)
[ "$(run CI_STATE=success PUSH=1)" = 0 ] && grep -q '^nothing to release' "$T/out" && ok "human-only ignored" || no "human-only ignored"

echo "== renovate under build/, CI pending → wait"; bot build/Cargo.lock "# bump" "tokio 1.54.1"
[ "$(run CI_STATE=pending PUSH=1)" = 0 ] && grep -q '^wait: CI' "$T/out" && ! git -C "$T/o.git" rev-parse -q --verify refs/tags/v0.25.1 >/dev/null && ok "pending waits, no tag" || no "pending waits"

echo "== CI red → stop"
[ "$(run CI_STATE=failure PUSH=1)" = 1 ] && grep -q '^stop: CI' "$T/out" && ok "red CI stops" || no "red CI stops"

echo "== dry run"
[ "$(run CI_STATE=success)" = 0 ] && grep -q 'dry run' "$T/out" && ! git -C "$T/o.git" rev-parse -q --verify refs/tags/v0.25.1 >/dev/null && ok "dry run pushes nothing" || no "dry run"
(cd "$T/w" && git tag -d v0.25.1 >/dev/null && git reset -q --hard origin/main)

echo "== release"
[ "$(run CI_STATE=success PUSH=1)" = 0 ] || no "release exit"
o() { git -C "$T/o.git" "$@"; }
[ "$(o show main:build/Cargo.toml | grep -c '^version = "0.25.1"')" = 1 ] && o show main:build/Cargo.toml | grep -q 'foo = { version = "0.25.0" }' && ok "Cargo.toml: package bumped, dependency line untouched" || no "Cargo.toml"
o show main:build/Cargo.lock | awk 'p=="name = \"practical\""{print} {p=$0}' | grep -q '0.25.1' && o show main:build/Cargo.lock | awk 'p=="name = \"foo\""{print} {p=$0}' | grep -q '0.25.0' && ok "Cargo.lock: only practical bumped" || no "Cargo.lock"
[ "$(o rev-parse v0.25.1^{commit})" = "$(o rev-parse main)" ] && [ "$(o cat-file -t v0.25.1)" = tag ] && ok "annotated tag v0.25.1 on main" || no "tag"
o log -1 --format=%B main | grep -q 'tokio 1.54.1' && [ "$(o log -1 --format=%an main)" = release-bot ] && ok "commit by release-bot lists the change" || no "commit message"

echo "== rerun: nothing new"
(cd "$T/w" && git fetch -q --tags origin && git reset -q --hard origin/main)
[ "$(run CI_STATE=success PUSH=1)" = 0 ] && grep -q '^nothing to release' "$T/out" && ok "idempotent" || no "idempotent"

echo "== main moved between clone and push → retry next run, nothing pushed"
bot build/Cargo.lock "# b2" "serde 1.0.300"; (cd "$T/w" && git reset -q --hard HEAD) ; C2=$T/c2; git clone -q "$T/o.git" "$C2"; (cd "$C2" && echo y >> README && git add -A && git commit -qm "docs: y" && git push -q origin main)
[ "$(run CI_STATE=success PUSH=1)" = 0 ] && grep -q '^wait: main moved' "$T/out" && ! git -C "$T/o.git" rev-parse -q --verify refs/tags/v0.25.2 >/dev/null && ok "moved main: atomic push refused, no tag" || no "moved main"

echo "== version drift → stop"; setup; (cd "$T/w" && sed -i.b 's/^version = "0.25.0"$/version = "0.26.0"/' build/Cargo.toml && rm build/Cargo.toml.b && git commit -qam "chore: bump" && git push -q origin main); bot build/Cargo.lock "# c" "x"
[ "$(run CI_STATE=success PUSH=1)" = 1 ] && grep -q '^stop: build/Cargo.toml says 0.26.0' "$T/out" && ok "drift stops" || no "drift"
exit $fail
