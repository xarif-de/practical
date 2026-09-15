# Static musl binary on a minimal Alpine. Runs as uid 1000 — the same uid as the
# rustical container in the pod, because both open the SAME WAL-mode SQLite file
# and the -wal/-shm siblings must be writable by both.

# renovate: datasource=docker packageName=rust
FROM rust:1.98-alpine@sha256:1716b3aa042d735f4566d14dc54e8037de9d69556e2d5dd58131d93a613d173d AS builder
# build-base = C toolchain for the bundled SQLite; perl/make for aws-lc-rs (rustls).
RUN apk add --no-cache build-base perl make
WORKDIR /src

# Dependency layer: RustiCal's crates come as git dependencies, so this layer
# (a few minutes) only rebuilds when Cargo.toml/Cargo.lock change.
COPY Cargo.toml Cargo.lock build.rs ./
RUN mkdir -p src assets locales templates \
 && echo 'fn main() {}' > src/main.rs \
 && cargo build --release --locked \
 && rm -rf src

COPY src ./src
COPY assets ./assets
COPY locales ./locales
COPY templates ./templates
COPY askama.toml ./
RUN touch src/main.rs build.rs \
 && cargo build --release --locked \
 && cp target/release/rustical-ui /rustical-ui

# renovate: datasource=docker packageName=alpine
FROM alpine:3.24@sha256:28bd5fe8b56d1bd048e5babf5b10710ebe0bae67db86916198a6eec434943f8b
# Not `scratch` like rustical's image: the shared dagger test harness runs every
# step through `sh -c`, and a shell also makes `kubectl exec` diagnostics possible.
# ca-certificates: OIDC token exchange, OpenHolidays API and Web Push over HTTPS.
RUN apk add --no-cache ca-certificates
LABEL org.opencontainers.image.source="https://git.xarif.de/docker/rustical-ui"
LABEL org.opencontainers.image.description="Web UI next to RustiCal: groups, collections, app tokens, calendar and contacts views on RustiCal's own SQLite store"
LABEL org.opencontainers.image.licenses="AGPL-3.0-or-later"
COPY --from=builder /rustical-ui /rustical-ui
USER 1000:1000
EXPOSE 4001
ENTRYPOINT ["/rustical-ui"]
CMD ["serve"]
