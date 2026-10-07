# Static musl binary on a minimal Alpine, WITH RustiCal's own binary next to it: the
# pod's two containers (rustical, practical) both run this image, so the CalDAV
# server and the UI built on its crates are always the same release. Runs as uid
# 1000 — both processes open the SAME WAL-mode SQLite file and its -wal/-shm siblings.

# renovate: datasource=docker packageName=rust
FROM rust:1.99-alpine@sha256:0cce0a5e0e8ba67b455257a3a02a1d99005f382748789d6464460028810f1627 AS builder
# build-base = C toolchain for the bundled SQLite; perl/make for aws-lc-rs (rustls).
RUN apk add --no-cache build-base perl make
WORKDIR /src

# release (every build) or dist (release tags: dagger-ci.yaml release-build-args);
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
# `cargo test` gates the image (CI, `mise run pipeline`, `mise run image`): same step, AFTER the release build, own
# target/ (dev profile, own cache id). Not a stage: dagger v0.21.10 evaluates stages one after another anyway. Not
# beside the build: the two compiles' memory would add up, and in the 8 GiB CI dind an OOM wipes the whole engine
# cache. CARGO_BUILD_JOBS=2 bounds the cold test compile (at 4 its own memory reached the release build's peak: no margin;
# 2 only slows a cold run). Measured at 4 jobs 2026-10-07, local engine (12 CPUs), a one-line
# change: step 172 s (release 146 + tests 26) vs 150-157 s without, cgroup peak 2.14 GB with and without; both
# targets cold: release 336 s + tests 133 s, peak 3.59 GB in the release build, 5.03 GB in the tests incl. >= 1.46 GB
# page cache the build left (tests' own anon <= 3.57 GB). No `-D warnings` (e2e/cargo.sh adds it locally): a
# toolchain bump with a new lint must not fail CI. No debuginfo: CI needs the verdict and the panic message.
COPY tests ./tests
# the_image_license_is_the_repository_license compares ./LICENSE with ../LICENSE, which is outside this build
# context: the same copy stands in, so that one test is vacuous here and stays real in `mise run test`.
COPY LICENSE ./
COPY LICENSE /
RUN --mount=type=cache,id=cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=cargo-git,target=/usr/local/cargo/git \
    --mount=type=cache,id=practical-target,target=/src/target,sharing=locked \
    --mount=type=cache,id=practical-test-target,target=/test-target,sharing=locked \
    find . -path ./target -prune -o -type f -exec touch {} + \
 && cargo build --profile "$CARGO_PROFILE" --locked \
 && cp "target/$CARGO_PROFILE/practical" /practical \
 && CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=2 cargo test --locked --target-dir /test-target

# renovate: datasource=docker packageName=alpine
FROM alpine:3.24@sha256:294b683cb724975bec92580e1e685676bd4b50bda910ddb8c51d4cabeaec77e6
# Not `scratch` like rustical's image: the shared dagger test harness runs every
# step through `sh -c`, and a shell also makes `kubectl exec` diagnostics possible.
# HTTPS trust (OIDC token exchange, OpenHolidays API, Web Push): the base image's
# ca-certificates-bundle (/etc/ssl/certs/ca-certificates.crt) is all rustls-native-certs reads.
# `apk add ca-certificates` (dropped 2026-10-06) only added 242 per-certificate files to
# /etc/ssl/certs and a 0.25 MB layer; process start CPU and RSS measured the same without it.
LABEL org.opencontainers.image.source="https://git.xarif.de/docker/practical"
LABEL org.opencontainers.image.description="RustiCal (unmodified, /usr/local/bin/rustical) plus its web UI PractiCal: groups, collections, app tokens, calendar, contacts, tasks and change history on RustiCal's own SQLite store"
LABEL org.opencontainers.image.licenses="AGPL-3.0-or-later"
COPY --from=builder /practical /practical
# RustiCal itself, unmodified, from its official image (static binary, AGPL-3.0-or-later;
# source: https://github.com/lennart-k/rustical at the same tag). The tag MUST equal the
# rustical_* crates' tag in Cargo.toml — build.rs and the image test check it, and one
# Renovate group (renovate.json) moves both. Run as `/usr/local/bin/rustical serve`.
COPY --from=ghcr.io/lennart-k/rustical:0.16.4@sha256:9b857e823612516b3371a4f5453e8e392426c4323fad6d38f3be2ad93f2fc07a /usr/local/bin/rustical /usr/local/bin/rustical
# the SQLite path RustiCal's image sets (its Dockerfile); PractiCal reads PRACTICAL__* only
ENV RUSTICAL_DATA_STORE__SQLITE__DB_URL=/var/lib/rustical/db.sqlite3
# PractiCal's allocator (mimalloc, Cargo.toml): return freed memory at once and commit arena memory
# on use — with mimalloc's defaults the process held 66-103 MB instead of ~37 MB (measured 2026-10-06).
# RustiCal's binary does not contain mimalloc: these do nothing for it.
ENV MIMALLOC_PURGE_DELAY=0 MIMALLOC_ARENA_EAGER_COMMIT=0
# AGPL-3.0 §4: the license travels with the programs (one text covers both)
COPY LICENSE /usr/share/licenses/practical/LICENSE
USER 1000:1000
EXPOSE 4001
ENTRYPOINT ["/practical"]
CMD ["serve"]
