<!-- docs-nav:top -->
<p><a href="https://xarif.gitlab.io/docker/practical/"><b>← Back to gallery and documentation</b></a></p>
<!-- /docs-nav -->

# Performance, release policy, known limits (2026-10)

## History (0.11.1 → 0.13.0)
Goal: faster, smaller, quieter with no functional and no visual change (round 1, 0.12.0; proof: 1132/1132 visual
states identical to the 0.11.1 baseline); round 2 (0.13.0): visible fixes decided one by one, each differing state
mapped. Decisive numbers: production 60 min after 0.12.0 vs before (VictoriaMetrics, container `practical`): CPU
6.0 → 0.38 mcores, working set 22.6 → 15.6 MiB. Local release build on the synthetic big DB (3,920 objects): idle
CPU 0.667 → 0.067 mcores (`PRAGMA data_version`-gated poller, reminder plan rebuilt only after a write or day change),
idle SQL 188 → ~31 statements/min, `/calendar` warm 97 → 1.9 ms (synctoken-keyed caches), app.js + style.css on the
wire 159 KB gzip → 90 KB brotli (build-time oxc/css_min + precompression, `immutable`), binary 26.6 → 21.2 MB
(codegen-units 1), task toggle 138 → 33 ms. Later waves: SQLite session store, input/CSRF/push-target hardening,
sources split per module (largest Rust file 27.9k → 7.1k tok; app.js 104k tok → 32 parts ≤ 10.7k) at bench parity.
Test net: Rust unit + router tests, Playwright functional and chaos suites, a visual baseline of every UI state
(pixels, DOM, computed style), the perf bench (docs/repo-overview.md "e2e/"). Per-item evidence: the commit bodies (`git log`).

## Release policy (2026-10-06)
- plan.xarif.de runs only release images: git tag `vX.Y.Z` → `[profile.dist]` build; kubernetes/rustical Renovate
  follows exact `vX.Y.Z` tags only, every other reference (`main`, `main-<sha8>`, `latest`, `vX.Y`) frozen.
- `main` publishes build versions (`release` profile, `:main` / `main-<sha8>`); not deployed. Until the first release
  tag the deployment stays on its last `main` digest; then both containers move to `vX.Y.Z@sha256:…` once by hand.

## Known limits (accepted)
- Lower-case `VALUE=date` objects stay invisible: caldata rejects them, RustiCal's store hides them before PractiCal sees them.
- Prefetch: preference changes made on ANOTHER device are not on the live stream; for ≤ 30 s (TTL) one prefetched
  navigation can render the old preference (same browser: storage signal drops it).
- Session: the inactivity expiry is refreshed at most once per 60 s, so a session can end up to 60 s early.
- Icons: the very first visit can paint icons a frame late (the sprite is a cached file, fetched after the first
  paint); `<link rel=preload>` would fix Chromium but makes WebKit refetch it on every page.
- 29 Feb birthdays show on 28 Feb in common years (Apple/Google behaviour; `contacts::yearly_in`; German law would
  give 1 March).

<!-- docs-nav:bottom -->
---

### Documentation

| Guide | What's inside |
| --- | --- |
| [Features](features.md) | everything PractiCal does, view by view |
| [Deployment](deployment.md) | production setup: image tags, pod hardening, network, backups |
| [Configuration](configuration.md) | all `PRACTICAL__*` variables, login and SSO, push, holidays, RustiCal settings |
| [Commands](commands.md) | `practical` subcommands, user and group administration with RustiCal's CLI |
| [How it works](architecture.md) | two programs on one database, the image, the RustiCal version contract |
| [Contributing](../CONTRIBUTING.md) | local development, tests and gates, CI and releases |
| [API documentation](https://xarif.gitlab.io/docker/practical/api/practical/index.html) | the Rust code: request flow, every module, invariants, caches (built from `main`) |
| [Repo overview](repo-overview.md) | how the repo is organised: everything outside the Rust API doc |
| [Performance](performance-2026-10.md) | measurements, release policy, known limits |
| [Licensing](licensing.md) | AGPL-3.0, RustiCal, third-party assets |

<p><a href="https://xarif.gitlab.io/docker/practical/"><b>← Back to gallery and documentation</b></a></p>
<!-- /docs-nav -->
