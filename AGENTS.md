# AGENTS.md — operating rules for agents in docker/practical

PractiCal = Rust (axum + askama, server-rendered) web UI on RustiCal's own SQLite store; one image runs both.
Where code lives: the Rust API doc (`mise run doc`, starts at `build/src/main.rs`) and, for everything else, `docs/repo-overview.md`. Dev commands, CI, releases: `CONTRIBUTING.md`; config/commands/deploy: `docs/`. These rules beat habits and generic defaults.

## Never
- `cargo fmt` — it rewrites every file. House style (long lines) is matched by hand.
- cargo, rustc or Playwright natively on the macOS host (MS Defender scans every new host file: CPU tax, disk). Every
  build/test runs in the checkout's VM container (`e2e/vm.sh`; Build, test, gate). Plain cargo: `e2e/vm.sh cargo …`
  in `build/` (target /target of the container, never `build/.cargo/config.toml`'s dir, shared by all worktrees:
  cargo handed one worktree another's binary as fresh, 2026-10-06). `E2E_NATIVE=1` = diagnosis only.
- Wrapping a vm.sh-backed command (mise gate/test/bench/… tasks, cargo.sh, gate.sh) in the heavy lock by hand: vm.sh
  takes it itself; a second flock waits for itself forever.
- Production writes, tests inside the production pod, personal data in tests: synthetic data only (`e2e/seed/`,
  `src/it/seed.rs`, `e2e/bench/gen-db.py`). Cluster access read-only; `mise run dev-db` takes a `VACUUM INTO` copy.
- `sleep`/fixed-delay waiting or polling: wait on a signal (`wait $PID`, `curl --retry-connrefused`, a readiness
  probe, a log line, flock). Long runs: foreground with a timeout, or a tracked background job.
- Synthetic host load (busy loops, stress tools): it froze the host once (load 111).
- `git push`, commits outside your own branch, edits in the main checkout.
- `docker volume prune` / `system prune`; remove only what you created (`docker rm -fv <id>`). colima does not
  share /private/tmp (bind mounts arrive empty): `docker cp`.

## Build, test, gate
- All of it runs in the colima/Docker VM: `e2e/vm.sh <cmd>` (header = reference). Image `practical-e2e-vm:<hash>`
  (e2e/vm/Dockerfile: rust:<channel>-bookworm + toolchain of rust-toolchain.toml, Node 24.7.0, Playwright's Chromium
  + OS deps, fonts-inter, mbx, watchexec; built on first use ~2 min); container `practical-vm-<checkout dir>` (warm,
  kept); volumes `rust-dev-cargo`, `rust-dev-mbx` (fleet layout: mbx store + target `/vol/targets/practical-vm-<dir>`
  = `/target`). Mounted at host paths: checkout, git common dir (ro), `~/.cache/practical-e2e` → reports, baselines,
  bench dirs land there; ref/out dirs elsewhere fail. The mise tasks delegate; cargo.sh, build-server.sh, gate.sh,
  baseline.sh, seed.sh (stage 1 = docker on the host first), bench.mjs re-exec themselves; global-setup.mjs refuses a
  host Playwright. Ports are the container's own (no clash between worktrees, none published). Every vm.sh run holds
  `~/.cache/practical-e2e/heavy.lock` (host flock) → runs of all worktrees queue host-wide.
- `e2e/cargo.sh <sub> [args]` (= `mise run test` for `test`): toolchain from `build/rust-toolchain.toml` (cargo.sh
  exports it as `RUSTUP_TOOLCHAIN`), target `E2E_TARGET_DIR` (VM: /target; native default
  `~/.cache/practical-e2e/target-<hash of path>`), cargo via `E2E_CARGO` (VM: mbx), inputs touched by content hash
  (cargo trusts mtimes), `RUSTFLAGS=-D warnings` (a new warning fails every gate step).

| command | runs | when |
|---|---|---|
| `mise run gate-quick` | cargo test → API doc (`mise run doc`: rustdoc, `-D warnings`) → docs-nav + site link check (`node site/build.mjs --check`) → server build (`e2e` profile) → functional suite | every commit (VM, 2026-11-03: 86 s warm, 117 s after an image rebuild; native was ~80-120 s) |
| `mise run a11y` | axe over 77 fixed states (desktop light/dark, phone light) vs `e2e/a11y-allow.txt` + 9 keyboard flows → a11y-check.mjs (~35 s) | part of `gate`; alone after UI/ARIA/focus edits |
| `mise run gate` | quick → chaos → a11y | before most commits; no reference baseline needed (VM: 163-175 s, 7× green in a row 2026-11-03). A chaos failure keeps its trace: `.work/test-results-<port>/…/trace.zip` |
| `mise run gate-visual -- <ref-dir>` | gate → visual baseline → compare vs `<ref-dir>` → theme-cover | OPT-IN, for a refactoring whose point is that nothing looks different. While surfaces are being designed every state differs on purpose, so the comparison only reports noise and costs a reference baseline of the base commit too (VM: 110 s; native was 161-262 s) |
| `mise run bench-compare -- <bench-dir>` | A/B perf bench vs the ref binary, interleaved | changes that can cost time, memory or bytes; exit 0 ok, 1 regression (names metrics), 4 power state changed → rerun |
| `E2E_PROFILE=release <any of the above>` | the shipped profile | release candidates, Cargo profile changes, binary size |

- Image/CI builds use `[profile.e2e]` (`CARGO_PROFILE` in build/Dockerfile); a `v*` tag builds `dist`
  (dagger-ci.yaml). Measured 2026-10-09, `mise run pipeline` after a one-line change: 2m41s with `release`,
  33.9 s with `e2e`; the first build on the new profile is a cold 4m39s.

- References come from the base commit: `mise run e2e-baseline -- <dir>` / `mise run bench` in a worktree of it, or an
  existing `~/.cache/practical-e2e/<…>-<base-sha>` made in the VM. Everything made natively (all before 2026-11-03)
  is invalid: macOS fonts/raster differ (gate visual refuses a `.e2e-baseline` without `env practical-e2e-vm:…`), a
  native bench dir holds a macOS binary the VM cannot run. VM bench (proc.py Linux): CPU = threads' schedstat ns,
  wakeups = voluntary context switches, instructions null (Colima VM has no PMU: no srv.inst.*/idle.inst), no power
  state (exit 4 never fires). A/A 2026-11-03 (2 runs, host load 4-7): 0 alarms, worst ratios srv.lat 1.052, srv.cpu
  1.032, ui.busy 1.137, ui.cpu4 1.118, idle.cpu 0.70-0.90; thresholds kept. A rise from a reference of 0 counts.
- VM visual repeatability: two baselines of one commit = 0 of 1135 states differ (2026-11-03), needs Chromium's
  `--disable-font-subpixel-positioning` on Linux (playwright.config.mjs; without it 61 phone states varied).
- Visual rule (`gate-visual` only): every state it reports as different must map to one intended change of yours,
  named per state in the commit/report; unmapped difference = failure (default target: 0 states with differences).
- a11y rule: a new axe finding fails (fix it, or add `<rule>⇥<key>⇥<reason>` to e2e/a11y-allow.txt); a line no state
  shows any more is STALE and fails — delete it: the list only shrinks. Icon-only controls need aria-label/title.
- CI (dagger) runs cargo-deny, `cargo test` inside the image build step (build/Dockerfile: after the release build,
  2 jobs, own cache mount, no `-D warnings`) and tests/tests.yaml. e2e, visual, a11y, bench: local gates only.
- A fixed bug gets a test that fails on the old code. A new cache gets a test that writes and reads back.
- Every person in the e2e fixture is marked through the first start (`e2e/seed/02-practical.py` `pin()`,
  `onboarding_json` completed). A user without that record gets the language gate over every page, and no suite can
  click anything: on 2026-10-09 every a11y state and three functional tests failed at once for that reason. A test
  that wants the first start asks for it (`POST /settings/onboarding/reopen`); a confirmation cannot be taken back
  over HTTP, so a test that confirms runs last in its file. A preference can: `POST /settings/prefs` writes every
  field it carries, so a test may hand a zone or a region list back. `/settings/onboarding/confirm` takes JSON, not
  a form.
- An intended bench cost is named in the commit with its ratio from the bench table.

## Parallel work
- One worktree per task: `git worktree add ~/git/xarif.de/docker/practical-wt-<pkg> -b <branch> <base>` (under $HOME: colima mounts only that); the first VM run does `npm ci`.
- Ports: `E2E_PORT=<p>` (default 4101) inside the worktree's container; functional uses p … p+2×E2E_WORKERS−1, bench
  p … p+2, visual p, p+1. Other worktrees have their own container (network namespace): no spacing needed.
  `E2E_FIXTURE=<shared fixture.sqlite3>` (read-only, under ~/.cache/practical-e2e or the checkout) avoids re-seeding.
- With the worktree, remove its VM state: `docker exec practical-vm-<dir> rm -rf /vol/targets/practical-vm-<dir>`,
  then `docker rm -f practical-vm-<dir>`. Never a `rust-dev-*` container: those belong to other tasks.
- Heavy runs one at a time per host, Playwright ≤ 4 workers: vm.sh queues every run behind
  `~/.cache/practical-e2e/heavy.lock` itself ("waiting for … heavy.lock" = another run holds it).
- `mise run dev` = its own throwaway container `practical-vm-<dir>-serve` publishing 127.0.0.1:`DEV_PORT` (4001);
  watchexec polls (colima's virtiofs passes no file events). DB under $HOME.

## Release policy
- plan.xarif.de runs only release images: git tag `vX.Y.Z` → CI builds `[profile.dist]`; kubernetes/rustical's
  Renovate follows exact `vX.Y.Z` tags only.
- Pushes to `main` publish build versions (`e2e` profile, `:main` / `main-<sha8>`): never deployed.
- Dependency updates release themselves: `.forgejo/workflows/release.yml` (hourly, `release-bot`, base/dagger-modules
  `auto-release`, section `release:` of dagger-ci.yaml) tags the next patch when renovate-bot changed `build/` and
  main's CI is green (CONTRIBUTING.md "Build versions and releases"). So keep
  `main` releasable: unfinished work stays on a branch. A release by hand still bumps `build/Cargo.toml` itself.

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
