<!-- docs-nav:top -->
<p><a href="https://xarif.gitlab.io/docker/practical/"><b>← Back to gallery and documentation</b></a></p>
<!-- /docs-nav -->

# Configuration

This page lists every setting of PractiCal and explains the ones that need more than one line: login, push
reminders and holidays. It is for the person who deploys PractiCal. Use the tables as a reference, and the sections
below them as step-by-step guides.

## How configuration works

PractiCal reads its settings from environment variables named `PRACTICAL__<KEY>`, with two underscores. Nested keys
use two underscores as well, for example `PRACTICAL__OIDC__ISSUER`.

- Set these variables on the `practical` container only.
- An empty value counts as unset. The one exception is `RUSTICAL_URL`, where an empty value has its own meaning
  (see the table).
- PractiCal refuses to start without a login method. Set `OIDC__ISSUER`, or `ALLOW_PASSWORD_LOGIN=true`, or both.

RustiCal has its own configuration, with variables named `RUSTICAL_<SECTION>__<KEY>`. It is described in
[RustiCal's docs](https://github.com/lennart-k/rustical/tree/main/docs). The RustiCal settings that matter next to
PractiCal are listed [at the end of this page](#rustical-settings-that-matter-next-to-practical).

## PractiCal settings

All keys below are written without the `PRACTICAL__` prefix. For example, `BIND` means the variable
`PRACTICAL__BIND`.

### Server and database

| Key | Default | What it does |
| --- | --- | --- |
| `BIND` | `[::]:4001` | The address and port that PractiCal listens on. |
| `DB_PATH` | `/var/lib/rustical/db.sqlite3` | RustiCal's SQLite file. PractiCal reads and writes it. |
| `RUSTICAL_URL` | `http://127.0.0.1:4000` | RustiCal's HTTP address. At start, PractiCal waits until RustiCal answers on `/ping`. An empty value means "do not wait" (for development only). |
| `PUBLIC_URL` | `http://localhost:4001` | The public address of the web UI. With `https://`, the session cookie is `Secure`. It is also the host for the OIDC callback and the page that users return to after logout. |
| `PUBLIC_DAV_URL` | `https://dav.example.org` | RustiCal's public address. PractiCal shows it to users so that they can set up their calendar apps. |

### Login and sessions

| Key | Default | What it does |
| --- | --- | --- |
| `ALLOW_PASSWORD_LOGIN` | `false` | Offers login with a RustiCal password. |
| `OIDC__ISSUER` | – | The OIDC issuer URL. Without it, there is no single sign-on. |
| `OIDC__CLIENT_ID` | – | The client ID at the identity provider. Required when `OIDC__ISSUER` is set. |
| `OIDC__CLIENT_SECRET` | – | The client secret. |
| `OIDC__NAME` | `SSO` | The label of the login button. |
| `OIDC__SCOPES` | `openid profile groups` | The scopes that PractiCal asks for, separated by spaces. |
| `OIDC__ALLOW_SIGN_UP` | `false` | Creates the RustiCal account at a user's first sign-in. Without it, only people who already have an account can sign in. |
| `OIDC__CLAIM_USERID` | `preferred_username` | The claim that carries the account name: `sub`, `preferred_username` or `email`. Must be the same as RustiCal's `RUSTICAL_OIDC__CLAIM_USERID`. |
| `LOGOUT_URL` | – | The identity provider's logout URL. `{url}` in it is replaced by the URL-encoded `PUBLIC_URL`. |
| `SESSION_HOURS` | `12` | A session ends after this many hours without activity. |

### Data retention

| Key | Default | What it does |
| --- | --- | --- |
| `TRASH_RETENTION_DAYS` | `90` | How long deleted groups stay restorable. RustiCal's `maintenance.trash_retention_days` must be at least this long. |
| `HISTORY_RETENTION_DAYS` | `90` | How long the change history keeps changes, and so how long they can be undone. |

### Push reminders

| Key | Default | What it does |
| --- | --- | --- |
| `VAPID_PRIVATE_KEY_PEM` | – | The private key for Web Push reminders. Without it, push is off. See [Web Push reminders](#web-push-reminders). |
| `VAPID_SUBJECT` | `mailto:admin@example.org` | A contact for the push services: a `mailto:` or `https://` URL. |

### Holidays and defaults

| Key | Default | What it does |
| --- | --- | --- |
| `HOLIDAYS_COUNTRY` | `DE` | The country the address search prefers while a user has named no region. Holiday regions themselves have no default. |
| `REMOTE_ORIGINS` | – | Comma-separated list of servers a user may import from or sync with, for example `https://my.xarif.de`. An allow-list, not free text: see [Importing from, and syncing with, another server](#importing-from-and-syncing-with-another-server). Empty = both features are hidden. |
| `REMOTE_SECRET_KEY` | – | 32 random bytes, base64, sealing a stored app password. Without it a lasting sync cannot be set up; a one-off import still works, because it stores nothing. |
| `HOLIDAYS_ENABLED` | `true` | `false` turns off all holiday requests. See [Holidays](#holidays). |
| `DEFAULT_TIMEZONE` | `Europe/Berlin` | The time zone for users who have not picked one. |
| `DEFAULT_LANG` | `de` | The language for users who have not picked one: `de`, `en`, `fr` or `es`. |

### Development only

Four more keys exist for development: `DEV_ASSETS_DIR`, `DEV_LOGIN`, `DEV_NOW` and `DEV_POLL_MS`. Leave them unset
in production. [CONTRIBUTING.md](../CONTRIBUTING.md) explains them.

## Login

Users sign in to PractiCal in one of two ways: with a password, or with single sign-on (SSO) through OIDC. You can
offer both.

In both cases, a user is a RustiCal **principal**. A principal is RustiCal's word for a user or a group. Calendar
apps never use the login password: each device gets its own app token in PractiCal's settings.

### Password login

1. Set `ALLOW_PASSWORD_LOGIN=true`.
2. Create each user with a password in RustiCal:

   ```bash
   rustical principals create <id> --password
   ```

[commands.md](commands.md#user-and-group-administration-with-rusticals-cli) shows how to run this command in the
pod.

### Single sign-on with OIDC

OIDC (OpenID Connect) lets users sign in with an existing account at your identity provider (IdP).

1. At the IdP, register a confidential client with these settings:
   - Redirect URI: `<PUBLIC_URL>/login/oidc/callback`
   - PKCE: `S256`
   - Client authentication: `client_secret_basic`
2. Set `OIDC__ISSUER`, `OIDC__CLIENT_ID` and `OIDC__CLIENT_SECRET`.
3. Optional: set `OIDC__NAME` as the label of the login button.
4. Optional: set `LOGOUT_URL` so that logging out of PractiCal also ends the IdP session, for example
   `https://auth.example.com/logout?rd={url}`.

PractiCal maps the `preferred_username` claim to the principal ID. `OIDC__CLAIM_USERID` changes which claim
that is; it must match RustiCal's `RUSTICAL_OIDC__CLAIM_USERID`, or the same person ends up with two accounts.

#### Where the account comes from

By default, PractiCal only lets people in who already have a RustiCal principal. There are three ways to get one:

- **Let PractiCal create it** at the user's first sign-in: set `OIDC__ALLOW_SIGN_UP=true`. PractiCal writes the
  same account RustiCal would, in RustiCal's own database. Nothing else is needed, and the user never sees
  RustiCal.
- Let RustiCal create it at the user's first sign-in on RustiCal's own frontend. For this, give RustiCal a client
  of its own at the same IdP, and set `RUSTICAL_OIDC__ALLOW_SIGN_UP=true` and
  `RUSTICAL_OIDC__CLAIM_USERID=preferred_username`.
- Create it by hand with `rustical principals create`.

With sign-up off, a user without an account lands back on the login page with a message and a button to
RustiCal's frontend, where their first sign-in creates the account. `PUBLIC_DAV_URL` is the address behind that
button.

> [!important]
> With `OIDC__ALLOW_SIGN_UP=true`, everybody your identity provider lets through to PractiCal's client gets a
> calendar account. Who may sign in is decided at the identity provider, and that is the only gate — so give
> PractiCal a client of its own there and restrict it to the people who should have a calendar.

Three things PractiCal refuses to create, whatever the IdP sends: an account whose name starts with `group.`
(that is where PractiCal's groups live), one whose name cannot be part of a CalDAV address (`: $ / \ ? # %`,
spaces), and one that would take over an existing group — a group is never an account to sign in to.

## Web Push reminders

Web Push sends reminders as browser notifications, even when no PractiCal tab is open. It needs a VAPID key pair: a
key that identifies your server to the browsers' push services.

1. Generate a key pair. This command prints a fresh private key (PEM) and its public key:

   ```bash
   practical gen-vapid
   ```

2. Put the private key into `VAPID_PRIVATE_KEY_PEM`. Store it in a Secret. You can write line breaks as `\n`.
3. Put a contact into `VAPID_SUBJECT`: a `mailto:` or `https://` URL.

Users then switch reminders on per browser in the settings.

## Holidays

PractiCal gets public and school holidays from [OpenHolidays](https://www.openholidaysapi.org). For public holidays,
[Nager.Date](https://date.nager.at) is the fallback. The holidays are cached in the database. The hosts that
PractiCal contacts are listed in [deployment.md](deployment.md#network).

`HOLIDAYS_COUNTRY` is only the country the address search prefers (ISO code, for example `DE`). Holiday
regions have no server default: until a user names one, no holidays are drawn.
Every user picks their own regions in the settings.

If the server has no internet access, set `HOLIDAYS_ENABLED=false`.

## RustiCal settings that matter next to PractiCal

These RustiCal variables go on the `rustical` container:

| RustiCal variable | Why it matters |
| --- | --- |
| `RUSTICAL_MAINTENANCE__TRASH_RETENTION_DAYS` | By default, RustiCal never purges its trash. If you set it, make it at least PractiCal's `TRASH_RETENTION_DAYS`. Otherwise a restored group could come back empty. |
| `RUSTICAL_HTTP__PAYLOAD_LIMIT_MB` | The default is 4 MB. vCards with photos and `.ics` files with attachments can be larger. |
| `RUSTICAL_FRONTEND__ALLOW_PASSWORD_LOGIN` | Password login on RustiCal's own frontend. Set it to `false` when OIDC is the only login. |
| `RUSTICAL_OIDC__*` | SSO for RustiCal's own frontend, and sign-up of new principals. |
| `NO_COLOR=1` | Plain log lines without colour codes. Both programs log through the `tracing` library. |

> [!warning]
> Do not let Kubernetes inject service-link variables into the `rustical` container: set
> `enableServiceLinks: false`. RustiCal reads every `RUSTICAL_*` variable as configuration and refuses unknown keys.
> A Service named `rustical` would stop it from starting.

## Importing from, and syncing with, another server

PractiCal can copy a calendar or address book from another CalDAV/CardDAV server once, or keep it in
sync. Nextcloud is what this is tested against.

**Which servers may be dialled is configuration, not user input.** `PRACTICAL__REMOTE_ORIGINS` holds
the allowed origins; the settings page offers exactly those and nothing else. Two reasons, either
sufficient:

- a free-text field is a server-side request forgery: the pod would dial whatever it is told, and in
  a cluster a public name can resolve to an internal address (a `*.xarif.de` name resolves to
  Traefik's ClusterIP, a private address);
- PractiCal's own guard refuses a non-public address, so free text could not reach such a server
  anyway, and loosening the guard is exactly what must not happen.

An entry is `scheme://host[:port]` without a path, `https` only (`http://localhost` is accepted in a
development build). A malformed entry fails the start instead of hiding the feature silently.

**Signing in** uses Nextcloud's Login Flow v2: the user's own browser signs in at their Nextcloud,
so a native login, two-factor and an SSO provider all work without PractiCal knowing anything about
them. PractiCal never sees the account password; it receives an app password that the user can
revoke per client in Nextcloud.

**An app password reaches the whole account, including files** — Nextcloud has no calendar-only
scope. A one-off import therefore keeps it in memory only and destroys it at Nextcloud when it is
done. A sync link has to keep it, so it is stored encrypted with `PRACTICAL__REMOTE_SECRET_KEY`: a
copy of the database alone is not enough to use it. The settings page tells the user to switch
filesystem access off for that app password.

**Only the user's own collections are offered.** A shared one is refused rather than warned about,
because Nextcloud withholds `CLASS:PRIVATE` objects from anyone but the owner and reduces
`CLASS:CONFIDENTIAL` ones to a busy slot — a copy would be incomplete without showing it. Calendars
Nextcloud generates from other data (the birthday calendar) and webcal subscriptions are left out
too.

**Two directions or one.** A sync link is either `both` or `pull`. When both sides changed the same
object, Nextcloud wins; the local version stays in the change history and can be brought back from
there. A deletion on either side travels to the other. PractiCal pushes its own changes within
seconds; it asks Nextcloud every `every_min` minutes (5 by default), which is one tiny request that
usually answers "nothing changed".

In the cluster this also needs a NetworkPolicy egress rule to Traefik on 443, because the default
posture denies it.

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
