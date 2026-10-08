# AGENTS.md — operating rules for agents in docker/practical

PractiCal = Rust (axum + askama, server-rendered) web UI on RustiCal's own SQLite store; one image runs both.
Where code lives: the Rust API doc (`mise run doc`, starts at `build/src/main.rs`) and, for everything else, `docs/repo-overview.md`. Dev commands, CI, releases: `CONTRIBUTING.md`; config/commands/deploy: `docs/`. These rules beat habits and generic defaults.

## Never
- `cargo fmt` — it rewrites every file. House style (long lines) is matched by hand.
- Plain `cargo` in `build/` without a target dir of your own: `build/.cargo/config.toml`'s dir is shared by all
  worktrees and cargo then hands one worktree another's binary as fresh (seen 2026-10-06). Build via `e2e/cargo.sh`.
- Production writes, tests inside the production pod, personal data in tests: synthetic data only (`e2e/seed/`,
  `src/it/seed.rs`, `e2e/bench/gen-db.py`). Cluster access read-only; `mise run dev-db` takes a `VACUUM INTO` copy.
- `sleep`/fixed-delay waiting or polling: wait on a signal (`wait $PID`, `curl --retry-connrefused`, a readiness
  probe, a log line, flock). Long runs: foreground with a timeout, or a tracked background job.
- Synthetic host load (busy loops, stress tools): it froze the host once (load 111).
- `git push`, commits outside your own branch, edits in the main checkout.
- `docker volume prune` / `system prune`; remove only what you created (`docker rm -fv <id>`). colima does not
  share /private/tmp (bind mounts arrive empty): `docker cp`.

## Build, test, gate
- `e2e/cargo.sh <sub> [args]` (= `mise run test` for `test`): toolchain from `build/rust-toolchain.toml` (an
  exported `RUSTUP_TOOLCHAIN` overrides the file; cargo.sh sets it), target dir per checkout (`E2E_TARGET_DIR`,
  default `~/.cache/practical-e2e/target-<hash of path>`; it unsets `CARGO_TARGET_DIR`), inputs touched by content
  hash (cargo trusts mtimes), sccache if installed, `RUSTFLAGS=-D warnings` (a new warning fails every gate step).
- Plain cargo only with `export CARGO_TARGET_DIR=~/.cache/practical-<task>-target` and the toolchain export; delete
  that dir afterwards. Check `df -h ~` before a cold build (1.6-4 GB per target dir).

| command | runs | when |
|---|---|---|
| `mise run gate-quick` | cargo test → API doc (`mise run doc`: rustdoc, `-D warnings`) → docs-nav + site link check (`node site/build.mjs --check`) → server build (`e2e` profile) → functional suite | every commit (~80 s warm; the doc step ~10 s) |
| `mise run a11y` | axe over 77 fixed states (desktop light/dark, phone light) vs `e2e/a11y-allow.txt` + 9 keyboard flows → a11y-check.mjs (~30 s) | part of `gate`; alone after UI/ARIA/focus edits |
| `mise run gate` | quick → chaos → a11y | before most commits; no reference baseline needed |
| `mise run gate-visual -- <ref-dir>` | gate → visual baseline → compare vs `<ref-dir>` → theme-cover | OPT-IN, for a refactoring whose point is that nothing looks different. While surfaces are being designed every state differs on purpose, so the comparison only reports noise and costs a reference baseline of the base commit too (161-262 s each) |
| `mise run bench-compare -- <bench-dir>` | A/B perf bench vs the ref binary, interleaved | changes that can cost time, memory or bytes; exit 0 ok, 1 regression (names metrics), 4 power state changed → rerun |
| `E2E_PROFILE=release <any of the above>` | the shipped profile | release candidates, Cargo profile changes, binary size |

- Image/CI builds use `[profile.e2e]` (`CARGO_PROFILE` in build/Dockerfile); a `v*` tag builds `dist`
  (dagger-ci.yaml). Measured 2026-10-09, `mise run pipeline` after a one-line change: 2m41s with `release`,
  33.9 s with `e2e`; the first build on the new profile is a cold 4m39s.

- References come from the base commit: `mise run e2e-baseline -- <dir>` / `mise run bench` in a worktree of it, or an
  existing `~/.cache/practical-e2e/<…>-<base-sha>`.
- Visual rule (`gate-visual` only): every state it reports as different must map to one intended change of yours,
  named per state in the commit/report; unmapped difference = failure (default target: 0 states with differences).
- a11y rule: a new axe finding fails (fix it, or add `<rule>⇥<key>⇥<reason>` to e2e/a11y-allow.txt); a line no state
  shows any more is STALE and fails — delete it: the list only shrinks. Icon-only controls need aria-label/title.
- CI (dagger) runs cargo-deny, `cargo test` inside the image build step (build/Dockerfile: after the release build,
  2 jobs, own cache mount, no `-D warnings`) and tests/tests.yaml. e2e, visual, a11y, bench: local gates only.
- A fixed bug gets a test that fails on the old code. A new cache gets a test that writes and reads back.
- An intended bench cost is named in the commit with its ratio from the bench table.

## Parallel work
- One worktree per task: `git worktree add ~/git/xarif.de/docker/practical-wt-<pkg> -b <branch> <base>`; `cd e2e && npm ci`.
- Ports: `E2E_PORT=<p>`; functional uses p … p+2×E2E_WORKERS−1 (default 4 workers → p+7), bench p … p+2, visual p, p+1:
  keep 10 ports between agents. `E2E_FIXTURE=<shared fixture.sqlite3>` (read-only) avoids re-seeding.
- Target dir: automatic per checkout (cargo.sh); delete `~/.cache/practical-e2e/target-<hash>` with the worktree.
- Heavy runs (full gate, e2e-baseline, bench) one at a time per host, Playwright ≤ 4 workers. Queue them behind a
  host-wide lock instead of polling:
  `perl -MFcntl=:flock -e 'open(my $f,">","$ENV{HOME}/.cache/practical-e2e/heavy.lock") or die $!; flock($f,LOCK_EX) or die $!; system(@ARGV); exit($?>>8)' -- mise run gate -- <ref>`

## Release policy
- plan.xarif.de runs only release images: git tag `vX.Y.Z` → CI builds `[profile.dist]`; kubernetes/rustical's
  Renovate follows exact `vX.Y.Z` tags only.
- Pushes to `main` publish build versions (`e2e` profile, `:main` / `main-<sha8>`): never deployed.

## Code rules
- Behaviour and look stay unchanged unless the task says otherwise. Desktop UI must never break; phone must not regress.
- UI building blocks are shared: `.seg`, `.field`, `.btn.icon-btn`, `.chip`, `.card`, `.pop`, `.ico`, askama macros
  (`templates/macros.html`), JS helpers (`window.practical`, docs/repo-overview.md "build/assets"). `rg` for an existing class/helper before
  adding one; a variant sits next to its base rule with a comment.
- New JS/CSS part file → list it in `build/src/bundle.rs` `BUNDLES` (order = run order / cascade; a test fails otherwise).
- Locales: every new string in all four `build/locales/{de,en,fr,es}.ftl`; counts are Fluent plurals
  (`{ $n -> [one] … *[other] … }`); keys used by JS start `js-`/`hxj-` (shipped with their plural forms).
- RustiCal tables only through its store crates; own tables `practical_*`, migrations append-only (`db.rs`).
- Caches are keyed so a write is never answered from before it (synctoken + zone/day/language where they matter).
- Comments are assertions with evidence (a measurement with date and method, a test name, an upstream file:line); no
  references to documents outside the repo (round N, plan X). English for code, comments, commits, docs.
- Commits: conventional (`type(scope): subject`), body `## Intent` / `## Key changes` / `## Details`.
- Agent docs (AGENTS.md, docs/repo-overview.md, code comments) are for LLM retrieval: dense, no narrative. Public docs
  (README.md, CONTRIBUTING.md, docs/*.md else) are for humans: short clear sentences, task-oriented headings, tables
  for reference data (Google developer style guide, Diátaxis); their nav blocks come from `mise run docs-nav`; docs/*.md + CONTRIBUTING.md are published as the Pages site (`mise run site`, site/build.mjs): relative links between them, absolute `https://xarif.gitlab.io/docker/practical/…` from README; a link/anchor that does not resolve fails gate-quick. Rust facts live in rustdoc (`//!` module docs, `///` items,
  intra-doc links; `mise run doc` must stay green); a file outside the crate docs that moves or changes purpose →
  update docs/repo-overview.md.
