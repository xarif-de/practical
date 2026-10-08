# Configuration

PractiCal reads environment variables `PRACTICAL__<KEY>` (set on the `practical` container only; an empty value
counts as unset). RustiCal keeps its own configuration (`RUSTICAL_<SECTION>__<KEY>`, see
[RustiCal's docs](https://github.com/lennart-k/rustical/tree/main/docs)).

## PractiCal

| Key | Default | Meaning |
| --- | --- | --- |
| `BIND` | `[::]:4001` | listen address |
| `DB_PATH` | `/var/lib/rustical/db.sqlite3` | RustiCal's SQLite file (read-write) |
| `RUSTICAL_URL` | `http://127.0.0.1:4000` | RustiCal's HTTP endpoint; PractiCal waits for its `/ping` before it starts. Empty = do not wait (development only) |
| `PUBLIC_URL` | `http://localhost:4001` | public base URL (cookie `Secure` if https, OIDC callback host, logout return) |
| `PUBLIC_DAV_URL` | – | RustiCal's public URL, shown to users for client setup |
| `OIDC__ISSUER`, `OIDC__CLIENT_ID`, `OIDC__CLIENT_SECRET`, `OIDC__NAME`, `OIDC__SCOPES` | – | OIDC relying-party settings (below). Without `OIDC__ISSUER` there is no SSO |
| `ALLOW_PASSWORD_LOGIN` | `false` | offer RustiCal password login |
| `LOGOUT_URL` | – | IdP logout URL; `{url}` = URL-encoded `PUBLIC_URL` |
| `TRASH_RETENTION_DAYS` | `90` | grace period for deleted groups; RustiCal's `maintenance.trash_retention_days` must be ≥ this |
| `HISTORY_RETENTION_DAYS` | `90` | how long the change history keeps changes (and can undo them) |
| `SESSION_HOURS` | `12` | session inactivity limit |
| `VAPID_PRIVATE_KEY_PEM`, `VAPID_SUBJECT` | – | Web Push reminders (below); unset = push off |
| `HOLIDAYS_COUNTRY`, `HOLIDAYS_SUBDIVISION`, `HOLIDAYS_ENABLED` | `DE`, –, `true` | holiday region until a user picks one; `false` = no holiday requests |
| `DEFAULT_TIMEZONE`, `DEFAULT_LANG` | `Europe/Berlin`, `de` | fallbacks (`de`, `en`, `fr`, `es`) |

Development-only knobs (`DEV_ASSETS_DIR`, `DEV_LOGIN`, `DEV_NOW`, `DEV_POLL_MS`) stay unset in production:
[CONTRIBUTING.md](../CONTRIBUTING.md).

## Login

- **Password:** `ALLOW_PASSWORD_LOGIN=true`; users are RustiCal principals with a password
  (`rustical principals create <id> --password`). Calendar apps never use it: they get per-device app tokens in
  PractiCal's settings.
- **OIDC (SSO):** register a confidential client at the IdP with the redirect URI `<PUBLIC_URL>/login/oidc/callback`
  (PKCE S256, `client_secret_basic`), then set `OIDC__ISSUER`, `OIDC__CLIENT_ID`, `OIDC__CLIENT_SECRET` and a button
  label `OIDC__NAME`. PractiCal maps the `preferred_username` claim to the principal id and never creates
  principals: a user exists once RustiCal has one — by `rustical principals create`, or by a first SSO login at
  RustiCal's own frontend with `RUSTICAL_OIDC__ALLOW_SIGN_UP=true` and `RUSTICAL_OIDC__CLAIM_USERID=preferred_username`
  (the same IdP, a client of its own). `LOGOUT_URL` ends the IdP
  session too, e.g. `https://auth.example.com/logout?rd={url}`.

## Web Push reminders

`practical gen-vapid` prints a fresh key pair. Put the private key (PEM; `\n` escapes allowed) into
`VAPID_PRIVATE_KEY_PEM` (a Secret), and a contact (`mailto:` or https URL) into `VAPID_SUBJECT`. Users then switch on
reminders per browser in the settings.

## Holidays

Public and school holidays come from [OpenHolidays](https://www.openholidaysapi.org), with
[Nager.Date](https://date.nager.at) as fallback, cached in the database (egress: [deployment.md](deployment.md#network)). `HOLIDAYS_COUNTRY`/`HOLIDAYS_SUBDIVISION` (ISO codes, e.g. `DE` /
`DE-SN`) only preset the region; every user picks their own regions in the settings. Without egress set
`HOLIDAYS_ENABLED=false`.

## RustiCal settings that matter next to PractiCal

| RustiCal variable | Why |
| --- | --- |
| `RUSTICAL_MAINTENANCE__TRASH_RETENTION_DAYS` | default: never purge. Set it ≥ PractiCal's `TRASH_RETENTION_DAYS`, or a restorable group could come back empty |
| `RUSTICAL_HTTP__PAYLOAD_LIMIT_MB` | default 4 MB; vCards with photos or `.ics` files with attachments can exceed it |
| `RUSTICAL_FRONTEND__ALLOW_PASSWORD_LOGIN` | RustiCal's own frontend login; `false` when OIDC is the only login |
| `RUSTICAL_OIDC__*` | RustiCal's own SSO (its frontend; sign-up of new principals) |
| `NO_COLOR=1` | plain log lines (both programs log through tracing) |

Never let Kubernetes inject service-link variables into the `rustical` container (`enableServiceLinks: false`):
RustiCal reads every `RUSTICAL_*` variable as configuration and refuses unknown keys, so a Service named `rustical`
would stop it from starting.
