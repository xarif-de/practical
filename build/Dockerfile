# Static musl binary on a minimal Alpine, WITH RustiCal's own binary next to it: the
# pod's two containers (rustical, rustical-ui) both run this image, so the CalDAV
# server and the UI built on its crates are always the same release. Runs as uid
# 1000 — both processes open the SAME WAL-mode SQLite file and its -wal/-shm siblings.

# renovate: datasource=docker packageName=rust
FROM rust:1.99-alpine@sha256:a96ea6d18d4062e38f16cfbadd8b4541d622f2527dd0a5eca1fb36d301da4e88 AS builder
# build-base = C toolchain for the bundled SQLite; perl/make for aws-lc-rs (rustls).
RUN apk add --no-cache build-base perl make
WORKDIR /src

# release (every build) or dist (release tags: CI passes --build-arg=CARGO_PROFILE=dist);
# see the two profiles in Cargo.toml
ARG CARGO_PROFILE=release
# Cached build (base/dagger-modules docs/rust-functions.md): cargo's registry and
# this repo's target/ live in BuildKit cache mounts that persist in the dagger
# engine, so cargo's fingerprints skip every crate whose inputs did not change.
# Replaces a dummy-main.rs dependency layer, which rebuilt ALL dependencies on
# every Cargo.lock change, i.e. on every Renovate bump. Measured on arm64 (8
# CPUs, `release` profile, 2026-10-02): cold 290 s, a CSS change 115 s (only
# this crate). The binary is copied out in the same RUN: a cache mount is not part of
# the layer. The `touch` forces this crate to rebuild every time: cargo judges
# path crates by mtime alone, and a tree with OLDER mtimes than the cache (a
# copied tree, rsync -a) is otherwise served the previous binary — reproduced
# on ai/panelist 2026-10-01. Dependencies live in the registry and stay cached.
COPY Cargo.toml Cargo.lock build.rs askama.toml ./
COPY src ./src
COPY assets ./assets
COPY locales ./locales
COPY templates ./templates
RUN --mount=type=cache,id=cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=cargo-git,target=/usr/local/cargo/git \
    --mount=type=cache,id=rustical-ui-target,target=/src/target,sharing=locked \
    find . -path ./target -prune -o -type f -exec touch {} + \
 && cargo build --profile "$CARGO_PROFILE" --locked \
 && cp "target/$CARGO_PROFILE/rustical-ui" /rustical-ui

# renovate: datasource=docker packageName=alpine
FROM alpine:3.24@sha256:294b683cb724975bec92580e1e685676bd4b50bda910ddb8c51d4cabeaec77e6
# Not `scratch` like rustical's image: the shared dagger test harness runs every
# step through `sh -c`, and a shell also makes `kubectl exec` diagnostics possible.
# ca-certificates: OIDC token exchange, OpenHolidays API and Web Push over HTTPS.
RUN apk add --no-cache ca-certificates
LABEL org.opencontainers.image.source="https://git.xarif.de/docker/rustical-ui"
LABEL org.opencontainers.image.description="RustiCal (unmodified, /usr/local/bin/rustical) plus its web UI rustical-ui: groups, collections, app tokens, calendar, contacts, tasks and change history on RustiCal's own SQLite store"
LABEL org.opencontainers.image.licenses="AGPL-3.0-or-later"
COPY --from=builder /rustical-ui /rustical-ui
# RustiCal itself, unmodified, from its official image (static binary, AGPL-3.0-or-later;
# source: https://github.com/lennart-k/rustical at the same tag). The tag MUST equal the
# rustical_* crates' tag in Cargo.toml — build.rs and the image test check it, and one
# Renovate group (renovate.json) moves both. Run as `/usr/local/bin/rustical serve`.
COPY --from=ghcr.io/lennart-k/rustical:0.16.4@sha256:9b857e823612516b3371a4f5453e8e392426c4323fad6d38f3be2ad93f2fc07a /usr/local/bin/rustical /usr/local/bin/rustical
# the SQLite path RustiCal's image sets (its Dockerfile); rustical-ui reads RUSTICAL_UI__* only
ENV RUSTICAL_DATA_STORE__SQLITE__DB_URL=/var/lib/rustical/db.sqlite3
# AGPL-3.0 §4: the license travels with the programs (one text covers both)
COPY LICENSE /usr/share/licenses/rustical-ui/LICENSE
USER 1000:1000
EXPOSE 4001
ENTRYPOINT ["/rustical-ui"]
CMD ["serve"]
