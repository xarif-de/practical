# PractiCal

**The web UI next to [RustiCal](https://github.com/lennart-k/rustical)** (a CalDAV/CardDAV server with a minimal
frontend and no admin API): groups owned together, calendars and address books per person or group, app tokens per
device, a trash with a grace period, calendar views (day, week, month, year planner and more), an event and task
editor that keeps every property it does not model, contacts, tasks, universal search, change history with undo,
public/school holidays, browser push reminders, `.ics`/`.vcf` import and export, live updates — in German, English,
French and Spanish.

It never talks to RustiCal over HTTP for data: it opens **RustiCal's own SQLite file** through RustiCal's store
crates (git dependency pinned to the RustiCal release) and keeps its own state in `practical_*` tables of the same
file, so one backup covers everything. **One image, both programs:** the image also carries RustiCal's binary,
unmodified (`/usr/local/bin/rustical`, from its official image at the crates' tag); the pod's two containers run
`rustical serve` and `practical serve` from it. Deployed on xarif.de in the `rustical` pod:
[kubernetes/rustical 🌐](https://git.xarif.de/kubernetes/rustical) → <https://plan.xarif.de>.

Working on the code: [AGENTS.md](AGENTS.md) (rules, gates) · [docs/MAP.md](docs/MAP.md) (where everything lives,
invariants, caches, loops) · [docs/performance-2026-10.md](docs/performance-2026-10.md) (known limits, perf history).

## Configuration

Environment variables `PRACTICAL__<KEY>`, set on the PractiCal container only.

| Key | Default | Meaning |
| --- | --- | --- |
| `BIND` | `[::]:4001` | listen address |
| `DB_PATH` | `/var/lib/rustical/db.sqlite3` | RustiCal's SQLite file (read-write) |
| `RUSTICAL_URL` | `http://127.0.0.1:4000` | RustiCal's HTTP endpoint (`/ping` wait). Empty = do not wait (development only) |
| `PUBLIC_URL` | `http://localhost:4001` | public base URL (cookie `Secure` if https, OIDC callback host, logout return) |
| `PUBLIC_DAV_URL` | – | RustiCal's public URL, shown to users for client setup |
| `OIDC__ISSUER`, `OIDC__CLIENT_ID`, `OIDC__CLIENT_SECRET`, `OIDC__NAME`, `OIDC__SCOPES` | – | OIDC relying-party settings (PKCE S256, `client_secret_basic`, callback `/login/oidc/callback`). Without `OIDC__ISSUER` there is no SSO |
| `ALLOW_PASSWORD_LOGIN` | `false` | offer RustiCal password login |
| `LOGOUT_URL` | – | IdP logout URL; `{url}` = URL-encoded `PUBLIC_URL` |
| `TRASH_RETENTION_DAYS` | `90` | grace period for deleted groups; RustiCal's `maintenance.trash_retention_days` must be ≥ this |
| `HISTORY_RETENTION_DAYS` | `90` | how long the change history keeps changes (and can undo them) |
| `SESSION_HOURS` | `12` | session inactivity limit |
| `VAPID_PRIVATE_KEY_PEM`, `VAPID_SUBJECT` | – | Web Push reminders (`gen-vapid` creates a key); unset = push off |
| `HOLIDAYS_COUNTRY`, `HOLIDAYS_SUBDIVISION`, `HOLIDAYS_ENABLED` | `DE`, –, `true` | holiday region until a user picks one; `false` = no holiday requests |
| `DEFAULT_TIMEZONE`, `DEFAULT_LANG` | `Europe/Berlin`, `de` | fallbacks |

## Commands

```text
practical serve                            # default
practical check-rustical-image <ref>       # exit 0 iff the image tag == the compiled rustical crate tag
practical gen-vapid                        # print a fresh VAPID private key (PEM) + public key
practical --version
```

`/healthz` answers 200 once the database is reachable.

## The RustiCal version contract

The five `rustical_*` crates in `build/Cargo.toml` pin **one** git tag (`build.rs` refuses a mix and
exports it as `RUSTICAL_TAG`), and `build/Dockerfile` copies RustiCal's binary from
`ghcr.io/lennart-k/rustical:<the same tag>` — the crates carry RustiCal's schema expectations, the
binary is the server that migrates that schema. Three guards keep them one release: `build.rs`
compares the Dockerfile's tag with the crates' on every local build (the Docker context has no
Dockerfile), the image test `rustical-binary-is-the-crates-release` compares
`rustical --version` with `practical --version`, and one Renovate group (`renovate.json`,
matched by dependency name — for a git dependency the package name is the repository URL) moves
all six in one PR. The deployment repo only checks that both containers run the same image
(`same_as`). `check-rustical-image <ref>` still compares a RustiCal image reference with the
crates' tag, for a deployment that runs RustiCal's own image.

## Release and deployment

CI (`dagger-ci.yaml`: one `dagger call … ci`, base/dagger-modules docs/ci.md; `.forgejo/workflows/ci.yml` only
triggers it) runs cargo-deny and builds + tests the image on every push (the build step runs `cargo test`, then `tests/tests.yaml`; e2e stays a local gate). `main` publishes build
versions with the `release` profile (`registry.gitlab.com/xarif/docker/practical:main`, `main-<sha8>`; skipped when the
build inputs are unchanged) — never deployed. A `vX.Y.Z` tag publishes the release image (`dist` profile, `:vX.Y.Z`);
Renovate pins it in kubernetes/rustical (PR, CI, auto-merge for minor/patch, up to 2 h after the publish), ArgoCD
syncs it from the gitlab.com mirror and the pod restarts with both containers on the new image. The Dockerfile keeps
cargo's registry and `target/` in BuildKit cache mounts (base/dagger-modules docs/rust-functions.md).

## Local development

```bash
mise run dev-db                         # COPY of the production DB → ~/.cache/practical-dev (VACUUM INTO, cluster read-only), password devpass
mise run dev                            # http://localhost:4001 as DEV_USER: watchexec rebuild + restart, assets from disk, the tab reloads and re-logs in
mise run test                           # cargo test (e2e/cargo.sh: rust-toolchain.toml, per-checkout target dir)
mise run deny                           # cargo-deny: advisories, bans, sources
mise run pipeline                       # the CI pipeline locally (cargo-deny + image build with cargo test + image tests), never publishes
mise run image                          # the image, loaded into docker as xarif-practical
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
```

Never point `DB` at a live RustiCal file. The dev knobs (`PRACTICAL__DEV_ASSETS_DIR`, `DEV_LOGIN`, `DEV_NOW`,
`DEV_POLL_MS`) are unset in production. Gates, harness and bench in detail: AGENTS.md, docs/MAP.md "e2e/".

## License

AGPL-3.0-or-later (`LICENSE`; a copy in `build/LICENSE` goes into the image at
`/usr/share/licenses/practical/LICENSE`, a test keeps both equal). PractiCal links RustiCal's
crates, which are AGPL-3.0-or-later themselves, and runs as a network service: §13 is met by the
"Source code" link in the user menu and on the login page, next to both versions. The image
redistributes RustiCal's binary unmodified; its source is
<https://github.com/lennart-k/rustical> at the tag the UI shows (and `rustical --version` prints).

**Icons** (`build/assets/icons.svg`, one `<symbol id="i-…">` sprite used via `<use>`): own drawings
plus single SVGs copied unchanged from Lucide (ISC), Tabler (MIT), Phosphor (MIT), Heroicons (MIT),
Iconoir (MIT), Bootstrap Icons (MIT), Material Design Icons (Apache-2.0) and Material Symbols
(Apache-2.0); `i-al-*` are drawn from Lucide `log-in`/`log-out`.
