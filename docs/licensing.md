<!-- docs-nav:top -->
<p><a href="../README.md"><b>← Back to the README</b></a></p>
<!-- /docs-nav -->

# Licensing

PractiCal is AGPL-3.0-or-later (`LICENSE`; a copy in `build/LICENSE` goes into the image at
`/usr/share/licenses/practical/LICENSE`, a test keeps both equal). PractiCal links RustiCal's crates, which are
AGPL-3.0-or-later themselves, and runs as a network service: §13 is met by the "Source code" link in the user menu and
on the login page, next to both versions.

The image redistributes RustiCal's binary unmodified; its source is <https://github.com/lennart-k/rustical> at the
tag the UI shows (and `rustical --version` prints).

## Third-party assets

- **Leaflet** 1.9.4 (BSD-2-Clause, `build/assets/leaflet.{js,css}`: the upstream dist files with a provenance header), loaded only by the map
  dialog. Map tiles: © OpenStreetMap contributors, attributed in the map.
- **Icons** (`build/assets/icons.svg`, one `<symbol id="i-…">` sprite used via `<use>`): own drawings plus single SVGs
  copied unchanged from Lucide (ISC), Tabler (MIT), Phosphor (MIT), Heroicons (MIT), Iconoir (MIT), Bootstrap Icons
  (MIT), Material Design Icons (Apache-2.0) and Material Symbols (Apache-2.0); `i-al-*` are drawn from Lucide
  `log-in`/`log-out`.
- Rust dependencies: `build/Cargo.lock` (CI runs `cargo-deny` for advisories, bans and sources; licenses are not
  checked automatically).

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
| [API documentation](https://practical-758168.gitlab.io/practical/) | the Rust code: request flow, every module, invariants, caches (built from `main`) |
| [Repo overview](repo-overview.md) | how the repo is organised: everything outside the Rust API doc |
| [Performance](performance-2026-10.md) | measurements, release policy, known limits |
| [Licensing](licensing.md) | AGPL-3.0, RustiCal, third-party assets |

<p><a href="../README.md"><b>← Back to the README</b></a></p>
<!-- /docs-nav -->
