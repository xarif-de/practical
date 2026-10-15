<!-- docs-nav:top -->
<p><a href="https://xarif.gitlab.io/docker/practical/"><b>← Back to gallery and documentation</b></a></p>
<!-- /docs-nav -->

# Commands

This page is a reference for the command-line tools in the PractiCal image. It is for admins who set up the first
user, manage users and groups, or check the image. Day-to-day work, such as new groups, members, calendars and app
tokens, happens in PractiCal's settings instead.

The image contains two programs:

| Program | Path in the image |
| --- | --- |
| PractiCal (the image's entrypoint) | `/practical` |
| RustiCal | `/usr/local/bin/rustical` |

## PractiCal subcommands

`practical` has four subcommands:

| Command | What it does |
| --- | --- |
| `practical serve` | Starts the web UI. This is the default when you give no subcommand. |
| `practical check-rustical-image <ref>` | Compares a RustiCal image reference with the RustiCal version that PractiCal was built against. Exits with 0 if the tags match. |
| `practical gen-vapid` | Prints a fresh VAPID private key (PEM) and its public key, for [Web Push reminders](configuration.md#web-push-reminders). |
| `practical --version` | Prints the version, for example `practical X.Y.Z (rustical crates vA.B.C)`. |

`check-rustical-image` is useful when you run RustiCal from its own image instead of the one in this image.
[architecture.md](architecture.md#the-rustical-version-contract) explains why the versions must match.

### Health check

`GET /healthz` answers `200 ok` once the database is reachable. If it is not, the answer is `503`. Use it as the
readiness and liveness probe of the `practical` container. It needs no login.

## User and group administration with RustiCal's CLI

Users, groups and memberships are RustiCal **principals**. A principal is RustiCal's word for a user or a group.
RustiCal has no admin API, so you manage principals with RustiCal's command-line tool (CLI). Run it inside the
`rustical` container: it uses the same image and the same database.

> [!tip]
> You need the CLI for the first user. After that, most group work can be done in PractiCal's settings.

The examples below run on Kubernetes. The first line saves the long `kubectl exec` prefix in a variable:

```bash
R="kubectl exec -it deploy/practical -c rustical -- rustical"
$R principals create alice -n "Alice" --password             # a user; prompts for the password
$R principals create group.family -p group -n "Family"       # a group
$R principals membership assign alice --to group.family     # alice may use the group's calendars
$R principals --help                                         # everything else
```

### Passwords

`--password` asks for the password once, with the prompt `Type in a password:`.

> [!warning]
> If you interrupt the prompt, the principal may already exist with an unknown password. Set the password again
> with `--overwrite`.

### App tokens for calendar apps

Calendar apps never use this password. The DAV server accepts only app tokens. You create an app token in one of two
ways:

- In PractiCal: **Settings → Connected devices → Add a device** (DAVx⁵ on Android gets its token by itself; **Manual**
  creates one to copy).
- With the CLI: `rustical principals app-token create --name <device> <id>`.

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
