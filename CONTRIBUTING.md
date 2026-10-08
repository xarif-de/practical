# Contributing to PractiCal

Rules for every change (what never to do, gates, code style, commits): [AGENTS.md](AGENTS.md). Where code lives,
invariants, caches, loops: [docs/MAP.md](docs/MAP.md). Known limits and performance history:
[docs/performance-2026-10.md](docs/performance-2026-10.md). How the parts fit: [docs/architecture.md](docs/architecture.md).

The source repository is `git.xarif.de/docker/practical`; <https://gitlab.com/xarif/docker/practical> is its public
mirror and the image registry. Tools: [mise](https://mise.jdx.dev) (tasks below), Rust via `build/rust-toolchain.toml`,
Node for the Playwright suites (`cd e2e && npm ci`), docker (colima on macOS) for the RustiCal CLI and image builds.

## Local development

```bash
mise run dev-db                         # COPY of the production DB → ~/.cache/practical-dev (VACUUM INTO, cluster read-only), password devpass
mise run dev                            # http://localhost:4001 as DEV_USER: watchexec rebuild + restart, assets from disk, the tab reloads and re-logs in
mise run test                           # cargo test (e2e/cargo.sh: rust-toolchain.toml, per-checkout target dir)
mise run deny                           # cargo-deny: advisories, bans, sources
mise run pipeline                       # the CI pipeline locally (cargo-deny + image build with cargo test + image tests), never publishes
mise run image                          # the image, loaded into docker as xarif-practical
```

Never point `DB` at a live RustiCal file. The dev knobs (`PRACTICAL__DEV_ASSETS_DIR`, `DEV_LOGIN`, `DEV_NOW`,
`DEV_POLL_MS`) are unset in production; `DEV_NOW` freezes the server clock, `DEV_LOGIN` skips the login.

## Tests and gates

```bash
mise run e2e-seed                       # synthetic fixture DB → e2e/.work/fixture.sqlite3 (docker: RustiCal CLI)
mise run e2e                            # functional Playwright suite (E2E_WORKERS, default 4)
mise run e2e-baseline -- <dir>          # visual baseline of every UI state (PNG + DOM + computed style)
mise run e2e-compare -- <dirA> <dirB>   # pixel/DOM/style diff; exit 1 on any difference
mise run chaos                          # seeded UI monkey + network chaos
mise run a11y                           # axe-core over fixed states + keyboard flows (e2e/a11y, ~30 s)
mise run gate-quick                     # cargo test → server build → functional suite
mise run gate -- <ref-dir>              # gate-quick → chaos → a11y → visual baseline → compare → dark-theme coverage
mise run bench                          # performance bench → ~/.cache/practical-e2e/bench-<sha>
mise run bench-compare -- <bench-dir>   # A/B against a former bench dir; exit 1 on a regression
mise run readme-shots                   # demo DB + the README/gallery screenshots → docs/images/
```

Which gate a change needs, parallel worktrees, ports and the heavy-run lock: [AGENTS.md](AGENTS.md). Harness,
fixtures and bench in detail: [docs/MAP.md](docs/MAP.md) "e2e/". All test data is synthetic.

## CI, releases, images

- CI (`dagger-ci.yaml`: one `dagger call … ci`, base/dagger-modules docs/ci.md; `.forgejo/workflows/ci.yml` only
  triggers it) runs cargo-deny and builds + tests the image on every push (the build step runs `cargo test`, then
  `tests/tests.yaml`). e2e, visual, a11y and bench stay local gates.
- `main` publishes build versions with the `release` profile (`registry.gitlab.com/xarif/docker/practical:main`,
  `main-<sha8>`; skipped when the build inputs are unchanged) — never deployed.
- A `vX.Y.Z` tag publishes the release image (`dist` profile, `:vX.Y.Z`, `:vX.Y`, `:vX`, `:latest`). In the
  reference deployment Renovate pins it in kubernetes/rustical (PR, CI, auto-merge for minor/patch), ArgoCD syncs it
  and the pod restarts with both containers on the new image.
- The Dockerfile keeps cargo's registry and `target/` in BuildKit cache mounts (base/dagger-modules
  docs/rust-functions.md). The RustiCal tag is one release across crates, Dockerfile and image test:
  [docs/architecture.md](docs/architecture.md#the-rustical-version-contract).

## Commits

Conventional commits (`type(scope): subject`) with the body `## Intent` / `## Key changes` / `## Details`
([AGENTS.md](AGENTS.md) "Code rules"). A fixed bug brings a test that fails on the old code.
