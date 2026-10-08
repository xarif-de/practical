<!-- docs-nav:top -->
<p><a href="https://xarif.gitlab.io/docker/practical/"><b>← Back to gallery and documentation</b></a></p>
<!-- /docs-nav -->

# Deployment

This page explains how to run PractiCal in production on Kubernetes. It is for the person who installs and
maintains the server. Each section covers one topic, so you can also use the page as a checklist.

The quick start in the [README](../README.md#quick-start-on-kubernetes) applies
[`deploy/kubernetes.yaml`](../deploy/kubernetes.yaml). That manifest creates four objects:

- a PersistentVolumeClaim (PVC) for the database,
- one Deployment with two containers that run from the same image,
- a Service,
- an Ingress.

This page describes what a production setup adds on top. For a complete, hardened real-world example with single
sign-on, NetworkPolicy, backups, probes and resource limits, see
[kubernetes/rustical](https://gitlab.com/xarif/kubernetes/rustical/-/blob/main/resources/deployment.yaml).

## Image and versions

The image is `registry.gitlab.com/xarif/docker/practical`, built for linux/amd64. It has these tags:

| Tag | What it is |
| --- | --- |
| `vX.Y.Z` | A release. |
| `vX.Y`, `vX`, `latest` | The newest release of that line. |
| `main`, `main-<sha8>` | A build of the `main` branch. These are build versions, not releases. |

In production, pin an exact release with its digest, for example `vX.Y.Z@sha256:…`. A tool such as Renovate can then
move the pin for you.

Both containers in the pod must run the **same** image. The web UI is built on the code of exactly the RustiCal
release that the image contains. [architecture.md](architecture.md#the-rustical-version-contract) explains why.

## Upgrades

Both programs update the database schema themselves when they start:

- RustiCal migrates its own schema.
- PractiCal applies its own migrations to its `practical_*` tables.

> [!warning]
> Take a [backup](#backups) before every upgrade. A rollback is a plain image change only if no migration came in
> between. Otherwise, restore the backup.

## Pod settings

### Run one replica

Run exactly one replica. All data lives in one SQLite file on a `ReadWriteOnce` volume, which only one node can
mount at a time.

The simple choice is `strategy: Recreate`: the old pod stops before the new one starts. A rolling update also works
on a single node, because SQLite serialises the writes during the short overlap.

### Turn off service links

Set `enableServiceLinks: false` in the pod spec. Otherwise, Kubernetes injects variables for every Service into the
containers. RustiCal reads every variable that starts with `RUSTICAL_` as configuration, and it refuses unknown
keys. A Service named `rustical` would therefore stop RustiCal from starting.

### Harden the pod

The image runs as uid 1000. It writes only to the data volume and to `/tmp`. This lets you lock the pod down with
the following settings:

- `runAsNonRoot: true`
- `allowPrivilegeEscalation: false`
- `capabilities.drop: [ALL]`
- `seccompProfile: RuntimeDefault`
- `readOnlyRootFilesystem: true`, plus an `emptyDir` volume on `/tmp` for each container
- `automountServiceAccountToken: false`

This excerpt shows where each setting goes. It shows one container; the second one gets the same container
settings.

```yaml
spec:
  enableServiceLinks: false
  automountServiceAccountToken: false
  securityContext:
    runAsNonRoot: true
    runAsUser: 1000
    fsGroup: 1000
    seccompProfile: { type: RuntimeDefault }
  containers:
    - name: practical
      securityContext:
        allowPrivilegeEscalation: false
        readOnlyRootFilesystem: true
        capabilities: { drop: [ALL] }
      volumeMounts:
        - { name: data, mountPath: /var/lib/rustical }
        - { name: tmp-practical, mountPath: /tmp }
  volumes:
    - { name: tmp-practical, emptyDir: {} }
```

### Make the volume writable

On most storage classes, `fsGroup: 1000` makes the volume writable for uid 1000.

Storage classes that are based on `hostPath`, such as k3s `local-path`, ignore `fsGroup`. On those, add an init
container that runs `chown -R 1000:1000 /var/lib/rustical`.

### Probes

Each container has its own health endpoint:

| Container | Probe | Port |
| --- | --- | --- |
| `rustical` | `GET /ping` | 4000 |
| `practical` | `GET /healthz` | 4001 |

### Resources

PractiCal and RustiCal need very little. These numbers were measured in production on 2026-10-08, as a 6-hour
average with light use:

| Container | Memory (working set) | CPU |
| --- | --- | --- |
| PractiCal | ≈ 13 MiB | 0.3 mcores |
| RustiCal | ≈ 4 MiB | < 0.1 mcores |

Memory limits of 192–256 MiB leave room for imports and searches.

## Network

### Hostnames and TLS

PractiCal needs two hostnames:

- one for the web UI: the `practical` container, port 4001,
- one for calendar apps (DAV): the `rustical` container, port 4000.

TLS ends at the ingress. Set `PRACTICAL__PUBLIC_URL` to the `https://` address of the web UI. With an `https://`
address, PractiCal marks the session cookie as `Secure`.

### Outgoing connections

If you restrict outgoing traffic (egress), allow these destinations for the `practical` container:

| Destination | Why | How to turn it off |
| --- | --- | --- |
| Your OIDC issuer | Single sign-on, if you use it. | Do not configure OIDC. |
| `openholidaysapi.org`, with `date.nager.at` as fallback | Public and school holidays. | `PRACTICAL__HOLIDAYS_ENABLED=false` |
| `api.github.com`, `raw.githubusercontent.com` | A weekly check of the region names (CLDR data). | `PRACTICAL__HOLIDAYS_ENABLED=false` |
| `photon.komoot.io`, `nominatim.openstreetmap.org` | Address suggestions and map positions while a user enters a location. | – |
| The browsers' push services | Web Push reminders, if configured. | Do not set a VAPID key. |

The browser itself loads the map tiles from `tile.openstreetmap.org`. This traffic does not go through the server.

The `rustical` container connects to its own OIDC issuer (if used), and to the DAV-Push servers that clients
register.

## Backups

Everything is in one SQLite file: RustiCal's calendars, contacts, principals and tokens, and PractiCal's settings,
rules, colours and history. (A **principal** is RustiCal's word for a user or a group.)

> [!warning]
> Never copy the live database file. A copy taken while the programs write can be inconsistent.

Take a consistent copy while both programs run. This command writes a complete snapshot of the database to
`/tmp/backup.db`:

```bash
sqlite3 /var/lib/rustical/db.sqlite3 "VACUUM INTO '/tmp/backup.db'"
```

Run it from a sidecar container that has `sqlite3`. The example deployment does this for its k8up backups.

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
