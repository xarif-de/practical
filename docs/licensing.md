<!-- docs-nav:top -->
<p><a href="https://xarif.gitlab.io/docker/practical/"><b>← Back to gallery and documentation</b></a></p>
<!-- /docs-nav -->

# Licensing

© 2026 Thomas Steinbach. PractiCal is AGPL-3.0-or-later (`LICENSE`; a copy in `build/LICENSE` goes into the image at
`/usr/share/licenses/practical/LICENSE`, a test keeps both equal). PractiCal links RustiCal's crates, which are
AGPL-3.0-or-later themselves, and runs as a network service: §13 is met by the "Source code" link in the user menu and
on the login page, next to both versions.

The image redistributes a modified RustiCal binary from the xarif fork. Its source is
<https://github.com/xarif-de/rustical> at the tag the UI shows (`v<upstream>-xarif.<n>`); `XARIF.md` there lists
the changes and their dates. The original is <https://github.com/lennart-k/rustical>. The fork's own web frontend
also links its source (AGPL-3.0 section 13).

## Third-party assets

- **Leaflet** 1.9.4 (BSD-2-Clause, `build/assets/leaflet.{js,css}`: the upstream dist files with a provenance header), loaded only by the map
  dialog. Map tiles: © OpenStreetMap contributors, attributed in the map.
- **Icons** (`build/assets/icons.svg`, one `<symbol id="i-…">` sprite used via `<use>`): own drawings plus single SVGs
  copied unchanged from Lucide (ISC), Tabler (MIT), Phosphor (MIT), Heroicons (MIT), Iconoir (MIT), Bootstrap Icons
  (MIT), Material Design Icons (Apache-2.0) and Material Symbols (Apache-2.0); `i-al-*` are drawn from Lucide
  `log-in`/`log-out`. The store and platform marks `i-android`, `i-apple`, `i-googleplay`, `i-fdroid`,
  `i-appgallery` and `i-github` come from [Simple Icons](https://simpleicons.org/) (CC0-1.0); the marks stay their owners' trademarks
  and only name where an app is found. The Android robot is reproduced from work created and shared by Google and
  used according to the terms of the Creative Commons 3.0 Attribution License.
- **DAVx⁵ logo** (`build/assets/davx5.svg`, in "Add a device" and on the confirmation page DAVx⁵ opens): the
  official icon `davx5_icon_green_bg.svg` from [davx5.com/about/logos](https://www.davx5.com/about/logos), which
  allows its use to refer and link to DAVx⁵ when it is not altered, not shown as one's own product and given
  context (the wizard links it to www.davx5.com). Only its file was compacted (svgo 3, 3 decimals: 25.7 → 10.4 kB;
  rendered at 256 px, no pixel differs by more than 7/255, 2026-10-14). DAVx⁵ is a product of bitfire web
  engineering GmbH; PractiCal is not affiliated with it.
- **Logo lettering** (`build/assets/logo.svg`, `lettering.svg`): outlines of Inter Display ExtraBold from
  [Inter](https://github.com/rsms/inter) 4.1 (SIL Open Font License 1.1); no font file is shipped. The design:
  [brand/README.md](../brand/README.md).
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
| [API documentation](https://xarif.gitlab.io/docker/practical/api/practical/index.html) | the Rust code: request flow, every module, invariants, caches (built from `main`) |
| [Repo overview](repo-overview.md) | how the repo is organised: everything outside the Rust API doc |
| [Performance](performance-2026-10.md) | measurements, release policy, known limits |
| [Licensing](licensing.md) | AGPL-3.0, RustiCal, third-party assets |

<p><a href="https://xarif.gitlab.io/docker/practical/"><b>← Back to gallery and documentation</b></a></p>
<!-- /docs-nav -->
