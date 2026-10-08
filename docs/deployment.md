# Deployment

The quick start in the [README](../README.md#quick-start-on-kubernetes) applies
[`deploy/kubernetes.yaml`](../deploy/kubernetes.yaml): a PVC, one Deployment with two containers from one image, a
Service and an Ingress. This page lists what a production setup adds. A complete, hardened real-world example (OIDC,
NetworkPolicy, backups, probes, resources) is
[kubernetes/rustical](https://gitlab.com/xarif/kubernetes/rustical/-/blob/main/resources/deployment.yaml).

## Image and versions

- `registry.gitlab.com/xarif/docker/practical`, linux/amd64. Release tags `vX.Y.Z` (also `vX.Y`, `vX`, `latest`);
  pin `vX.Y.Z@sha256:…` in production and let Renovate move it. `main`/`main-<sha8>` are build versions, not
  releases.
- Both containers must run the **same** image: the UI is built on the RustiCal crates of exactly the RustiCal binary
  in the image ([architecture.md](architecture.md#the-rustical-version-contract)).
- Upgrades: RustiCal migrates its schema at start, PractiCal applies its own `practical_*` migrations. Back up
  before an upgrade; a rollback is a plain image change only when no migration came in between.

## Pod

- **One replica.** One SQLite file on a `ReadWriteOnce` volume; `strategy: Recreate` is the simple choice (a
  rolling update also works on one node, SQLite serialises the short overlap).
- **`enableServiceLinks: false`**: RustiCal reads every `RUSTICAL_*` variable as configuration and refuses unknown
  keys.
- **Hardening** (the image runs as uid 1000 and writes only to the data volume and `/tmp`): `runAsNonRoot`,
  `allowPrivilegeEscalation: false`, `capabilities.drop: [ALL]`, `seccompProfile: RuntimeDefault`,
  `readOnlyRootFilesystem: true` plus an `emptyDir` on `/tmp` per container, `automountServiceAccountToken: false`.
- **Volume ownership:** `fsGroup: 1000` makes the volume writable on most storage classes. hostPath-based ones
  (k3s `local-path`) ignore `fsGroup`; there an init container `chown -R 1000:1000 /var/lib/rustical` does it.
- **Probes:** `rustical` → `GET /ping` on 4000, `practical` → `GET /healthz` on 4001.
- **Resources** (measured in production, 2026-10-08, 6 h average with light use): PractiCal ≈ 13 MiB working set,
  0.3 mcores; RustiCal ≈ 4 MiB, < 0.1 mcores. Limits of 192–256 MiB leave room for imports and searches.

## Network

- Two hostnames: the UI (`practical`, port 4001) and DAV for calendar apps (`rustical`, port 4000). TLS terminates at
  the ingress; `PRACTICAL__PUBLIC_URL` must be the https URL (it makes the session cookie `Secure`).
- Egress of PractiCal: the OIDC issuer (if used); holidays from `openholidaysapi.org` with `date.nager.at` as
  fallback, and a weekly region-name check at `api.github.com`/`raw.githubusercontent.com` (CLDR) —
  `HOLIDAYS_ENABLED=false` turns both off; `photon.komoot.io` and `nominatim.openstreetmap.org` (address suggestions and map positions while a user
  enters a location); the browsers' push services (Web Push, if configured). Map tiles load in the browser from
  `tile.openstreetmap.org`.
- Egress of RustiCal: its OIDC issuer (if used) and the DAV-Push servers clients register.

## Backups

Everything — RustiCal's calendars, contacts, principals, tokens and PractiCal's settings, rules, colours and
history — is in the one SQLite file. Take a consistent copy while both run with
`sqlite3 /var/lib/rustical/db.sqlite3 "VACUUM INTO '/tmp/backup.db'"` (e.g. from a sidecar with `sqlite3`, as the
example deployment does for k8up), never by copying the live file.
