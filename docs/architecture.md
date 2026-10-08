# How PractiCal works

This page explains how PractiCal is built and why. It is for admins who want to understand what they run, and for
developers who are about to change the code. You do not need it to use or deploy PractiCal.

## Two programs, one database

PractiCal runs next to RustiCal. Both programs work on the same database file:

```
calendar apps ──CalDAV/CardDAV──► rustical serve  (:4000) ─┐
                                                           ├─► /var/lib/rustical/db.sqlite3 (WAL)
browser ───────HTTPS────────────► practical serve (:4001) ─┘
```

[RustiCal](https://github.com/lennart-k/rustical) is the CalDAV/CardDAV server. It syncs calendars and contacts
with phones and desktop apps. It has a minimal web frontend and no admin API.

PractiCal is the web UI. It never asks RustiCal for data over HTTP. Instead, it opens **RustiCal's own SQLite
file** through RustiCal's store crates. (A **crate** is a Rust library. PractiCal uses RustiCal's code for reading
and writing its tables, pinned to the RustiCal release.) PractiCal keeps its own data in `practical_*` tables in
the same file. So one backup covers everything.

### Sharing the file safely

Both programs run in one pod, as the same user. The database runs in SQLite's **WAL mode** (write-ahead log), which
lets readers work while another process writes. SQLite makes the writers take turns.

At start, PractiCal waits until RustiCal answers on `/ping`. RustiCal migrates the database schema when it starts.
PractiCal never runs RustiCal's migrations itself.

### Live updates and the change history

PractiCal watches the database for changes with `PRAGMA data_version`, a counter that SQLite raises on every write
from another connection. When a phone changes an event, open browser tabs learn about it within seconds through
server-sent events, a one-way push channel from server to browser.

Every change also lands in the change history, together with the device that made it.

## One image, both programs

The image contains two programs:

- PractiCal at `/practical`. It is the entrypoint.
- RustiCal's binary at `/usr/local/bin/rustical`, unmodified. It is copied from RustiCal's official image, at the
  same version as the crates.

The pod's two containers run `rustical serve` and `practical serve` from this image. So the server and the UI
always come from one release.

The image is based on Alpine Linux and runs as uid 1000. Both programs are static musl binaries. It is published as
`registry.gitlab.com/xarif/docker/practical` for linux/amd64:

- `vX.Y.Z`, `vX.Y`, `vX` and `latest` are releases.
- `main` and `main-<sha8>` are untested build versions.

## The RustiCal version contract

PractiCal reads and writes RustiCal's tables. So it must be built against exactly the RustiCal release whose binary
runs next to it. Two places name that release:

- **The crates**: five `rustical_*` crates in `build/Cargo.toml` pin **one** git tag. They carry RustiCal's
  expectations about the schema. `build.rs` refuses a mix of tags, and it exports the tag as `RUSTICAL_TAG`.
- **The binary**: `build/Dockerfile` copies RustiCal's binary from `ghcr.io/lennart-k/rustical:<the same tag>`. This
  binary is the server that migrates the schema.

Three guards keep the two at one release:

1. `build.rs` compares the Dockerfile's tag with the crates' tag on every local build. (The Docker build context has
   no Dockerfile, so this check runs only locally.)
2. The image test `rustical-binary-is-the-crates-release` compares `rustical --version` with
   `practical --version`.
3. One Renovate group in `renovate.json` moves all six pins in one pull request. It matches by dependency name; for
   a git dependency, the package name is the repository URL.

If you run RustiCal from its own image instead of the one in this image, check it with
`practical check-rustical-image <ref>` ([commands.md](commands.md)). It compares a RustiCal image reference with the
crates' tag.

## Inside PractiCal

PractiCal is written in Rust with the web framework axum and the template engine askama. The server renders the
pages as HTML. The JavaScript in the browser is plain, without a framework. The only library is Leaflet for the map
dialog, and it loads only when you open the map.

At build time, PractiCal's own CSS and JavaScript are minified. All assets are then precompressed (gzip and
brotli) and embedded into the binary.

To go deeper:

- [repo-overview.md](repo-overview.md) gives an overview of the repository.
- The API documentation describes every module, the caches and the background loops. Build it with
  `mise run doc`.
- [performance-2026-10.md](performance-2026-10.md) has measurements and known limits.
