<!-- docs-nav:top -->
<p><a href="https://xarif.gitlab.io/docker/practical/"><b>← Back to gallery and documentation</b></a></p>
<!-- /docs-nav -->

# Contributing to PractiCal

This guide gets you from a fresh clone to your first change. It covers the tools you need, the first run, the daily
loop, the tests, CI and releases, and the commit style. It is written for new contributors who know Rust and the web,
but not this project.

> [!important]
> [AGENTS.md](AGENTS.md) holds the strict rules for every change: what never to do, which tests a change needs, the
> code style and the commit format. It is written for AI coding agents, but the rules apply to everyone. Read it
> before your first commit.

## Find your way around

These pages explain the project from different angles:

- [docs/repo-overview.md](docs/repo-overview.md) shows how the repository is organised.
- The [API documentation](https://xarif.gitlab.io/docker/practical/api/practical/index.html) describes every module, its invariants, the caches and the background loops.
  To build it locally, run `mise run doc`.
- [docs/architecture.md](docs/architecture.md) explains how PractiCal and RustiCal fit together.
- [docs/performance-2026-10.md](docs/performance-2026-10.md) has the known limits and the performance history.

The source repository is `git.xarif.de/docker/practical`. <https://gitlab.com/xarif/docker/practical> is its public
mirror and hosts the image registry.

## What you need

| Tool | What it is for |
| --- | --- |
| [mise](https://mise.jdx.dev) | Runs every task on this page (`mise run <task>`). |
| Rust | Builds PractiCal. The version is pinned in `build/rust-toolchain.toml`. |
| Node.js | Runs the Playwright browser tests. Install the packages once with `cd e2e && npm ci`. |
| Docker | Runs RustiCal's CLI for the test data, and builds the image. On macOS, colima works. |
| watchexec (optional) | Rebuilds and restarts the server when you edit a file. Without it, `mise run dev` runs once. |

## First run

1. Install the browser-test packages:

   ```bash
   cd e2e && npm ci
   ```

2. Get a database to work on. You have two options:
   - **Synthetic test data.** This builds a fixture database (a small database with made-up users and events) at
     `e2e/.work/fixture.sqlite3`. Copy it before you use it, because the dev server writes to it:

     ```bash
     mise run e2e-seed
     cp e2e/.work/fixture.sqlite3 ~/practical-dev.sqlite3
     ```

   - **A copy of the production database** (maintainers only, needs read access to the cluster). `mise run dev-db`
     takes a consistent copy with `VACUUM INTO` and saves it in `~/.cache/practical-dev`. The cluster stays
     read-only. On the copy, the password of `DEV_USER` is `devpass`.

3. Start the dev server. With the fixture copy, sign in as the fixture user `alice`:

   ```bash
   DB=~/practical-dev.sqlite3 DEV_USER=alice mise run dev
   ```

   Without `DB`, `mise run dev` uses the copy from `mise run dev-db`.

4. Open <http://localhost:4001>.

> [!warning]
> Never point `DB` at a live RustiCal database. The dev server writes to the file. All test data must be synthetic.

## Daily loop

`mise run dev` is the main feedback loop:

- When you edit a file in `src`, `templates` or `locales`, watchexec rebuilds and restarts the server. This takes
  about 3–10 seconds.
- CSS and JavaScript are served straight from disk, so a change shows up without a rebuild.
- The browser tab reloads itself and signs you in again.

Before you commit, run at least `mise run test` and `mise run gate-quick`. [Tests](#tests) explains which other
checks a change needs.

> [!warning]
> Never run `cargo fmt`. It rewrites every file. The house style uses long lines, which are matched by hand.

If you build in more than one git worktree, build through the mise tasks or `e2e/cargo.sh`. They give every
checkout its own build directory. Plain `cargo` shares one directory between all worktrees and can then hand one
worktree another's binary. [AGENTS.md](AGENTS.md) has the details.

### Development settings

`mise run dev` sets these development-only variables for you. They are never set in production.

| Variable | What it does |
| --- | --- |
| `PRACTICAL__DEV_ASSETS_DIR` | Serves CSS and JavaScript from this directory, uncached, and reloads the tab after a change. Turns on development mode. |
| `PRACTICAL__DEV_LOGIN` | Signs in as this user without a login page. |
| `PRACTICAL__DEV_NOW` | Freezes the server clock at this time (RFC 3339, for example `2026-10-14T08:00:00Z`). |
| `PRACTICAL__DEV_POLL_MS` | Sets how often the server checks the database for changes, in milliseconds (at least 50). |

PractiCal refuses to start if `DEV_LOGIN`, `DEV_NOW` or `DEV_POLL_MS` is set without `DEV_ASSETS_DIR`.

## Tests

PractiCal has several kinds of tests. Each one answers a different question. All of them use synthetic data only.

### Rust tests and supply chain

| Command | What it checks |
| --- | --- |
| `mise run test` | The Rust unit and integration tests (`cargo test` through `e2e/cargo.sh`). |
| `mise run deny` | The dependencies, with cargo-deny: security advisories, banned crate versions and allowed sources. |

### Browser tests

These suites drive a real browser with Playwright against a build of your checkout.

| Command | What it checks |
| --- | --- |
| `mise run e2e-seed` | Builds the synthetic fixture database (`e2e/.work/fixture.sqlite3`) that the suites start from. Uses RustiCal's CLI in Docker. |
| `mise run e2e` | The functional suite: does every feature still work? Runs on `E2E_WORKERS` workers (default 4). |
| `mise run chaos` | A seeded "monkey" that clicks around at random while the network misbehaves. It finds crashes that no scripted test thinks of. |
| `mise run a11y` | Accessibility: axe-core over fixed UI states, plus keyboard flows (about 30 seconds). |

### Visual comparison

A **baseline** is a snapshot of every UI state: a screenshot, the DOM and the computed styles. Comparing two
baselines shows exactly which states look different.

| Command | What it does |
| --- | --- |
| `mise run e2e-baseline -- <dir>` | Takes a baseline of this checkout. |
| `mise run e2e-compare -- <dirA> <dirB>` | Compares two baselines pixel by pixel, and the DOM and styles. Exits with 1 on any difference. |

### Gates: the checks before a commit

A **gate** runs several of the checks above in a row and stops at the first failure.

| Command | What it runs | When to use it |
| --- | --- | --- |
| `mise run gate-quick` | `cargo test`, the API documentation, the docs link check, a server build and the functional suite | Every commit (about 80 seconds when the build is warm). |
| `mise run gate` | `gate-quick`, then the chaos monkey and the accessibility checks | Before most commits. Needs nothing prepared. |
| `mise run gate-visual -- <ref-dir>` | `gate`, then a visual baseline of your checkout, a comparison against `<ref-dir>` and the dark-theme coverage check | Opt-in. Use it for a rewrite that must change no pixel. It also needs a reference baseline of the commit you started from, built in a second worktree (`mise run e2e-baseline -- <dir>`), which takes about as long again. While you are designing a surface, every state differs on purpose and the comparison only produces noise. |

`<ref-dir>` is a baseline of the commit that your change starts from. Every difference that the gate reports must
be one that you intended.

### Performance

| Command | What it does |
| --- | --- |
| `mise run bench` | Measures this checkout on a large synthetic database: server latency, memory, idle CPU, bytes on the wire and UI interaction times. Saves the results in `~/.cache/practical-e2e/bench-<sha>`. |
| `mise run bench-compare -- <bench-dir>` | Compares this checkout with an earlier `bench` result, run interleaved on this machine. Exits with 1 on a regression. |

Run the benchmark when a change can cost time, memory or bytes.

### Screenshots

`mise run readme-shots` builds a demo database and takes the screenshots in `docs/images/`. It also writes their
titles, captions and gallery groups to `e2e/demo/shots.json`.

[AGENTS.md](AGENTS.md) has the full rules for tests: which gate a change needs, how to run several worktrees in
parallel, the ports, and the lock for heavy runs.

## CI and releases

### What CI runs

CI runs on every push. The workflow in `.forgejo/workflows/ci.yml` only triggers it; the pipeline itself is
`dagger-ci.yaml`, one `dagger call … ci` (described in base/dagger-modules `docs/ci.md`). It does two things:

1. It checks the dependencies with cargo-deny.
2. It builds and tests the image. The build step runs `cargo test`, then the image tests in `tests/tests.yaml`.

The browser tests, the visual comparison, the accessibility checks and the benchmark do not run in CI. Run them
locally.

To run the CI pipeline on your machine, use `mise run pipeline`. It never publishes anything. `mise run image`
builds the image and loads it into Docker as `xarif-practical`.

### Build versions and releases

| Trigger | What gets published | Build profile |
| --- | --- | --- |
| A push to `main` | `registry.gitlab.com/xarif/docker/practical:main` and `:main-<sha8>`. Skipped when the build inputs did not change. | `e2e` |
| A `vX.Y.Z` tag | `:vX.Y.Z`, `:vX.Y`, `:vX` and `:latest` | `dist` (optimised for size) |

Builds of `main` are never deployed. Only release tags are.

In the reference deployment, Renovate pins the new release in kubernetes/rustical. It opens a pull request, CI
runs, and minor and patch updates merge automatically. ArgoCD then syncs the change, and the pod restarts with both
containers on the new image.

### Build caching and the RustiCal version

The Dockerfile keeps cargo's registry and the `target/` directory in BuildKit cache mounts (described in
base/dagger-modules `docs/rust-functions.md`).

The RustiCal version must be the same in the crates, the Dockerfile and the image test.
[docs/architecture.md](docs/architecture.md#the-rustical-version-contract) explains why and how this is checked.

## The project site

The documentation is published as a static site with GitLab Pages: the start page (`docs/index.md`) shows the
screenshot gallery and the documentation index, and every page in `docs/` plus this guide becomes an HTML page. The
API documentation sits under `api/`. `site/build.mjs` builds it; the job `pages` in `.gitlab-ci.yml` publishes it
from `main` and from release tags.

To build and preview the site locally:

```bash
mise run site
python3 -m http.server -d e2e/.work/site 4501
```

Then open <http://localhost:4501/>. `mise run site` also builds a fresh API documentation into the site.

Links between the pages are ordinary relative Markdown links (`configuration.md#holidays`), so they work on
gitlab.com too. The build turns them into links between the HTML pages and links to other repository files into
gitlab.com links. It fails on a link or an anchor that does not exist. `mise run gate-quick` runs this check
(`node site/build.mjs --check`) for the docs and the README.

## Commits

Write commits in the [Conventional Commits](https://www.conventionalcommits.org) style: `type(scope): subject`. The
body has three sections:

```text
## Intent
Why the change is needed.

## Key changes
What changed, in a few bullet points.

## Details
Anything a reviewer should know: trade-offs, measurements, follow-ups.
```

A fixed bug comes with a test that fails on the old code. [AGENTS.md](AGENTS.md) ("Code rules") has the remaining
rules.

<!-- docs-nav:bottom -->
---

### Documentation

| Guide | What's inside |
| --- | --- |
| [Features](docs/features.md) | everything PractiCal does, view by view |
| [Deployment](docs/deployment.md) | production setup: image tags, pod hardening, network, backups |
| [Configuration](docs/configuration.md) | all `PRACTICAL__*` variables, login and SSO, push, holidays, RustiCal settings |
| [Commands](docs/commands.md) | `practical` subcommands, user and group administration with RustiCal's CLI |
| [How it works](docs/architecture.md) | two programs on one database, the image, the RustiCal version contract |
| [Contributing](CONTRIBUTING.md) | local development, tests and gates, CI and releases |
| [API documentation](https://xarif.gitlab.io/docker/practical/api/practical/index.html) | the Rust code: request flow, every module, invariants, caches (built from `main`) |
| [Repo overview](docs/repo-overview.md) | how the repo is organised: everything outside the Rust API doc |
| [Performance](docs/performance-2026-10.md) | measurements, release policy, known limits |
| [Licensing](docs/licensing.md) | AGPL-3.0, RustiCal, third-party assets |

<p><a href="https://xarif.gitlab.io/docker/practical/"><b>← Back to gallery and documentation</b></a></p>
<!-- /docs-nav -->
