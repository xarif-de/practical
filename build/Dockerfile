# Static musl binary on a minimal Alpine, WITH RustiCal's own binary next to it: the
# pod's two containers (rustical, practical) both run this image, so the CalDAV
# server and the UI built on its crates are always the same release. Runs as uid
# 1000 — both processes open the SAME WAL-mode SQLite file and its -wal/-shm siblings.

# renovate: datasource=docker packageName=rust
FROM rust:1.99-alpine@sha256:b1d5dd5a22a09404d3e798c0cf051b689077360374eb505fa5bb93c479693d87 AS builder
# build-base = C toolchain for the bundled SQLite; perl/make for aws-lc-rs (rustls).
RUN apk add --no-cache build-base perl make
WORKDIR /src

# e2e (every build) or dist (release tags: dagger-ci.yaml release-build-args); see the profiles in Cargo.toml. e2e
# since 2026-10-09: it is `release` plus incremental compilation, and a `main` image is a test artefact that is never
# deployed (CONTRIBUTING.md). `mise run pipeline` after a one-line change: 33.9 s vs 2m41s on release, both warm;
# the first build on a profile is cold once (4m39s).
ARG CARGO_PROFILE=e2e
# Cached build (base/dagger-modules docs/rust-functions.md): registry + target/ in BuildKit cache mounts, so only
# changed crates rebuild (a dummy-main.rs layer rebuilt every dependency on each Cargo.lock bump); arm64 8 CPUs,
# release, 2026-10-02: cold 290 s, a CSS change 115 s. The binary is copied out in the same RUN (a cache mount is no
# layer). `touch`: cargo judges path crates by mtime, and a tree older than the cache got the previous binary
# (ai/panelist 2026-10-01).
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
FROM alpine:3.24@sha256:be2656061b536c1883c4034790e21d0a09c5aaaf51968b89368a3c0371ea2c9a
# Not `scratch` like rustical's image: the shared dagger test harness runs every
# step through `sh -c`, and a shell also makes `kubectl exec` diagnostics possible.
# HTTPS trust (OIDC, holidays, Web Push): the base image's ca-certificates-bundle is all rustls-native-certs reads;
# `apk add ca-certificates` (dropped 2026-10-06) added 242 files + 0.25 MB, start CPU and RSS the same without it.
LABEL org.opencontainers.image.source="https://gitlab.com/xarif/docker/practical"
LABEL org.opencontainers.image.description="RustiCal (xarif fork, modified: github.com/xarif-de/rustical, /usr/local/bin/rustical) plus its web UI PractiCal: groups, collections, app tokens, calendar, contacts, tasks and change history on RustiCal's own SQLite store"
LABEL org.opencontainers.image.licenses="AGPL-3.0-or-later"
COPY --from=builder /practical /practical
# RustiCal itself from the xarif fork's image (static binary, AGPL-3.0-or-later, modified:
# upstream release + patches; source https://github.com/xarif-de/rustical at the same tag,
# XARIF.md there). linux/amd64 + linux/arm64. The tag MUST equal the rustical_* crates' tag
# in Cargo.toml — build.rs and the image test check it, and one Renovate group
# (renovate.json) moves both. Run as `/usr/local/bin/rustical serve`.
COPY --from=registry.gitlab.com/xarif/docker/rustical:v0.16.7-xarif.1@sha256:8f4b16b5e6ff43f734c3b5ef01410792a529b0b075b9fcd53c329632cc550aea /usr/local/bin/rustical /usr/local/bin/rustical
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
