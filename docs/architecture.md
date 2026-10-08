# How PractiCal works

## Two programs, one database

```
calendar apps ──CalDAV/CardDAV──► rustical serve  (:4000) ─┐
                                                           ├─► /var/lib/rustical/db.sqlite3 (WAL)
browser ───────HTTPS────────────► practical serve (:4001) ─┘
```

- [RustiCal](https://github.com/lennart-k/rustical) is the CalDAV/CardDAV server: sync with phones and desktop
  apps, a minimal frontend, no admin API.
- PractiCal is the web UI. It never talks to RustiCal over HTTP for data: it opens **RustiCal's own SQLite file**
  through RustiCal's store crates (git dependency pinned to the RustiCal release) and keeps its own state in
  `practical_*` tables of the same file. One backup covers everything.
- Both run in one pod as the same uid and share the WAL-mode file; SQLite serialises the writers. PractiCal waits
  for RustiCal's `/ping` at start (RustiCal migrates the schema) and never runs RustiCal's migrations itself.
- Live updates: PractiCal watches the database (`PRAGMA data_version`), so a change made by a phone shows up in open
  browser tabs within seconds (server-sent events), and every change lands in the change history with the device
  that made it.

## One image, both programs

The image carries PractiCal (`/practical`, entrypoint) and RustiCal's binary, unmodified (`/usr/local/bin/rustical`,
copied from RustiCal's official image at the crates' tag). The pod's two containers run `rustical serve` and
`practical serve` from it, so server and UI are always one release. Base: Alpine, uid 1000, static musl binaries.
Published as `registry.gitlab.com/xarif/docker/practical` (linux/amd64): `vX.Y.Z`, `vX.Y`, `vX`, `latest` for
releases; `main`/`main-<sha8>` are untested build versions.

## The RustiCal version contract

The five `rustical_*` crates in `build/Cargo.toml` pin **one** git tag (`build.rs` refuses a mix and exports it as
`RUSTICAL_TAG`), and `build/Dockerfile` copies RustiCal's binary from `ghcr.io/lennart-k/rustical:<the same tag>` —
the crates carry RustiCal's schema expectations, the binary is the server that migrates that schema. Three guards
keep them one release:

1. `build.rs` compares the Dockerfile's tag with the crates' on every local build (the Docker context has no
   Dockerfile).
2. The image test `rustical-binary-is-the-crates-release` compares `rustical --version` with `practical --version`.
3. One Renovate group (`renovate.json`, matched by dependency name — for a git dependency the package name is the
   repository URL) moves all six in one PR.

`practical check-rustical-image <ref>` compares a RustiCal image reference with the crates' tag, for a deployment
that runs RustiCal's own image instead of the one in this image.

## Inside PractiCal

Rust (axum + askama), server-rendered pages, vanilla JavaScript without a framework (Leaflet only for the map dialog, loaded on use); assets are
minified and precompressed at build time and embedded into the binary. Where every module lives, the caches and the
background loops: [MAP.md](MAP.md). Measurements and known limits: [performance-2026-10.md](performance-2026-10.md).
