# rustical-ui

**The web UI next to [RustiCal](https://github.com/lennart-k/rustical).** RustiCal is a
CalDAV/CardDAV server with a minimal frontend and no admin API. This sidecar adds what a
family needs on top: groups you own together, calendars and address books for yourself or a
group, app tokens for every device, a trash with a grace period, and (phase 2) the actual
calendar, contacts and tasks views.

**What it does (phase 1 + 2):** groups and memberships · calendars and address books per
person or group · app tokens · trash with restore · calendar views Day (ledger + document),
Week (stream), Month (tiles with continuous multi-day bars) and the "More" views Year planner,
Free weekends, Vacation planner, Countdown wall, Year in numbers · event editor as a card form
(view mode behind a lock, cards only where data is): reminders on a timeline strip with per-user
presets (before start, before end, at a time; display, sound or mail; repeats), recurrence painted on a week strip (weekdays, day of month, interval, end, exceptions and extra dates),
organizer and attendees as chips (display only, with a jump to the matching address-book entry), location with coordinates and map actions, status/busy/visibility/
priority as four tiles whose icon follows the value, links, attachments and conference URIs, an event colour, categories, notes, presets ·
contacts as letter panels (fields per entry selectable) or an
inline-editable sheet, a quick-jump rail, and a card editor: view mode with actions (call, SMS,
WhatsApp, mail, maps, chat, jump to a related contact, vCard download), edit mode behind a lock,
the full vCard field set (structured addresses, URLs, messengers, relationships, extra dates,
preferred flags), address books with a colour · minimal tasks (VTODO) · universal search incl. "jump to a date" · orbit date
picker · public/school holidays from the OpenHolidays API · vacation-day counting · browser
push reminders (VAPID) · `.ics`/`.vcf` export and import · live updates over SSE · German and
English.

It never talks to RustiCal over HTTP for data. It opens **RustiCal's own SQLite file** through
RustiCal's store crates (`rustical_store_sqlite`, git dependency pinned to the RustiCal release)
and keeps its own state in `xui_*` tables of the same file, so one backup covers everything.

Deployed on xarif.de as a second container in the `rustical` pod:
[kubernetes/rustical 🌐](https://git.xarif.de/kubernetes/rustical) → <https://plan.xarif.de>.

## Contents

- [How it works](#how-it-works)
- [Configuration](#configuration)
- [Commands](#commands)
- [The RustiCal version contract](#the-rustical-version-contract)
- [Local development](#local-development)
- [Repository layout](#repository-layout)

## How it works

- **One database, two processes.** RustiCal and rustical-ui run in the same pod as the same
  uid and share the WAL-mode SQLite file. rustical-ui waits for RustiCal's `/ping` before it
  opens the file (RustiCal runs its schema migrations at start) and never runs those
  migrations itself. Its own tables live under the `xui_` prefix with their own
  `xui_migrations` ledger — never `_sqlx_migrations`, which would make RustiCal abort on an
  "unknown migration".
- **Groups are RustiCal `group` principals** (`group.<slug>`) owned by their members: every
  member may add or remove members, create the group's collections and delete the group.
  Deleting moves memberships and collections to the trash (`xui_trashed_groups` + RustiCal's
  own soft delete); the group is restorable for `TRASH_RETENTION_DAYS`, then a background
  purge removes the principal and the collections for good. The last member leaving trashes
  the group too, so nothing becomes ownerless.
- **Login = RustiCal's accounts.** OIDC through the same IdP as RustiCal (own client; the
  callback only *looks up* the principal — an account is created at RustiCal, never here)
  and, optionally, RustiCal password login. Sessions are in memory (a restart logs everyone
  out). Logout also ends the IdP session via `LOGOUT_URL` (Authelia's portal logout; it has
  no `end_session_endpoint`).
- **App tokens** are created exactly like RustiCal's frontend does (random 64 chars,
  pbkdf2-hashed, shown once as `<id>_<secret>`), so DAV clients use them against the
  `dav` host unchanged. HTTP Basic with an app token also works against this UI's routes.
- **CSRF**: state-changing requests must be same-origin (`Sec-Fetch-Site`/`Origin`) *and*
  carry the session's token; requests authenticated with HTTP Basic skip the origin check
  (no cookie session involved).
- **Live updates**: a 2-second poll of the collections' synctokens (microseconds on SQLite)
  fans changes out to browsers over SSE (`/api/changes`).
- **Recurrence** is caldata's engine (RustiCal's own iCalendar crate). `build/tests/fixtures/
  recurrence-real.json` holds the 60 recurring series of a real family calendar, anonymised,
  with the first 60 instances each as computed by python-dateutil (an independent oracle);
  `cargo test` asserts identical expansion (plan D11).
- **Views** are rendered server-side (`src/render.rs`, a port of the design prototype); the
  browser swaps the fragment (`?fragment=1`) on navigation, re-fetches it on SSE change events,
  and only the event editor, date picker, search, the endless timeline rail and the contact editor
  are JavaScript. The page header stays put: the app frame is the viewport, `.main` scrolls, the
  toolbar sticks under the topbar, scrollbars are hidden everywhere. Per-view CSS/JS budget stays
  below 60 KB gzip (one shared `style.css` + `app.js`).
- **Event editing** keeps what it does not model, too: the stored master VEVENT is split into the
  lines the editor writes (`MANAGED`) and everything else (STATUS, CLASS, ATTACH, COLOR, X-…, RFC 9073
  components), which is copied verbatim on every save; each VALARM keeps its position so its unmodelled
  lines (UID, RELATED-TO, VLOCATION, binary and mail attachments, X-…) travel with the edited alarm, and RECURRENCE-ID
  overrides of other clients stay untouched. `build/tests/fixtures/demo-event-full.ics` carries every
  VEVENT property there is and `unchanged_save_keeps_every_property` proves the round trip. Generated
  series instances get the master's alarms (caldata builds them without VALARMs).
- **Status** (event card): STATUS, TRANSP, CLASS and PRIORITY are four tiles; the icon follows the
  value (check/question/cross, calendar with or without an entry, group/person/agent, arrows) and is grey
  while nothing is set, a click opens the chooser. The hidden selects behind the tiles stay the model, so
  dirty tracking, reset and submit are untouched; the view mode reads the values as one line. TRANSP and
  CLASS have RFC defaults (OPAQUE, PUBLIC), shown as "default" so an unset value stays unset in the file.
  A priority outside 1/5/9 (Outlook writes 2) keeps its number and maps to the RFC bands for the icon.
- **People** (event card) are shown, not edited: RustiCal does no scheduling (RFC 6638), so nobody
  would send an invitation or carry a reply back — the list is a record. Each ORGANIZER/ATTENDEE is a
  chip whose ring colour is the PARTSTAT; a click opens a detail card (role, reply, delegation, RSVP,
  address, mail/copy actions). `GET /api/events/{key}` adds `contacts` to every person: the address-book
  entries with the same e-mail, else a similar name (`contacts::matches`: same family name, given names
  equal or one a prefix of the other; rooms and resources never match). One hit becomes a jump to
  `/contacts#key`, several open a chooser. On save the persons go back unchanged, every parameter
  included (RSVP, DELEGATED-*, MEMBER, SENT-BY, DIR, LANGUAGE, SCHEDULE-*, X-…).
- **Reminders** (event card): the edit mode shows the alarms on a timeline strip (a marker follows
  the pointer and snaps to the user's snap points — settings, "Reminders: snap points" — each
  point owning an equal share of the strip), a row of preset chips
  that add or remove the matching alarm with one click, and one expandable entry per alarm whose
  fields follow the kind (text · sound file · subject, message, recipients) and the trigger (number +
  unit, or date and time). The star in an entry saves its setting — without text, subject and
  acknowledgement — as a preset; presets are per user (`xui_user_settings.alarm_presets_json`,
  `/api/alarm-presets`), the built-in list applies until the user changes it. The editor writes
  `ATTENDEE` (mail alarms) and `ATTACH` (the sound of an audio alarm); `PROXIMITY` and `ACKNOWLEDGED`
  (RFC 9074) are kept as the client wrote them — a dismissed alarm shows struck through — and "after
  start/end" appears only for an alarm that already fires after its anchor.
- **Recurrence** (event card): the rule is painted on a scrollable week strip — one month in full, a
  neighbouring week on each side, separators around the month that move with it. In the pattern mode a
  click on a day toggles its weekday (weekly) or sets the day of month (monthly, yearly); in the exception
  mode a click strikes an occurrence (`EXDATE`) or adds an extra date (`RDATE`). Interval and end are two
  pills; "until" is picked on the strip while the rest of the dialog is dimmed. The strip owns the mouse
  wheel: one gesture moves at most to the neighbouring month. Its preview of occurrences is a small
  client-side expander for what the strip can paint; caldata on the server stays authoritative. Rules with
  parts the strip cannot paint (`BYSETPOS`, several ordinals, `BYHOUR` …) show as a custom rule text and are
  saved as typed.
- **vCard editing** keeps what it does not model: on update every property outside the managed set
  (PHOTO, X-…, client items) is carried over verbatim (`preserved_lines`). Parameters are read from
  the raw vCard text (`raw_lines`) because caldata exposes only the first value of a parameter —
  `TYPE=HOME,PREF` and Apple's repeated `;type=…` would otherwise be invisible. Address-book colours
  live in `xui_addressbook_colors` (RustiCal stores none), list-view field choices per user in
  `xui_user_settings.contact_list_fields_json`.
- **Addresses** (event location card): suggestions while typing come from [Photon](https://photon.komoot.io)
  (komoot, OSM data, built for search-as-you-type), the address of a map position from
  [Nominatim](https://nominatim.openstreetmap.org) (OSMF; its policy forbids autocomplete and demands
  an identifying User-Agent, so it only answers the map dialog's button). Both go through the sidecar
  (`/api/geo/search`, `/api/geo/reverse`): it sends the User-Agent, throttles each provider to one
  request per second, caches answers for a week and keeps the users' IPs at home. The browser asks
  only after a 2 s typing pause (the ring in the field counts it down) or on a button. The map
  dialog is Leaflet 1.9.4 (vendored, BSD-2, loaded on first use) with tiles from
  tile.openstreetmap.org, attribution shown as the tile policy requires.
- **Holidays** come from `openholidaysapi.org` (`HOLIDAYS_COUNTRY`/`HOLIDAYS_SUBDIVISION`, per-user
  region override), cached 30 days in `xui_holidays`; a failed fetch falls back to the stale cache.
- **Push reminders**: browsers register `assets/sw.js` and subscribe with the VAPID public key;
  a one-minute loop expands the subscribers' events a few days ahead, sends each VALARM once
  (`xui_alarms_sent`) and drops subscriptions the push service reports gone (404/410).
- **i18n**: Fluent `.ftl` files (`build/locales/de.ftl`, `en.ftl`), embedded at compile time.
  Language = user preference → session → `Accept-Language`.

## Configuration

Environment variables `RUSTICAL_UI__<KEY>`. Keep them on the rustical-ui container only:
RustiCal reads every `RUSTICAL_*` variable as its own configuration and rejects unknown keys.

| Key | Default | Meaning |
| --- | --- | --- |
| `BIND` | `[::]:4001` | listen address |
| `DB_PATH` | `/var/lib/rustical/db.sqlite3` | RustiCal's SQLite file (read-write) |
| `RUSTICAL_URL` | `http://127.0.0.1:4000` | RustiCal's HTTP endpoint (`/ping` wait). Empty = do not wait (development only) |
| `PUBLIC_URL` | `http://localhost:4001` | public base URL (cookie `Secure` if https, OIDC callback host, logout return) |
| `PUBLIC_DAV_URL` | – | RustiCal's public URL, shown to users for client setup |
| `OIDC__ISSUER`, `OIDC__CLIENT_ID`, `OIDC__CLIENT_SECRET`, `OIDC__NAME`, `OIDC__SCOPES` | – | OIDC relying-party settings (PKCE S256, `client_secret_basic`, callback `/login/oidc/callback`). Without `OIDC__ISSUER` there is no SSO |
| `ALLOW_PASSWORD_LOGIN` | `false` | offer RustiCal password login |
| `LOGOUT_URL` | – | IdP logout URL; `{url}` = URL-encoded `PUBLIC_URL` |
| `TRASH_RETENTION_DAYS` | `90` | grace period for deleted groups; RustiCal's `maintenance.trash_retention_days` must be ≥ this |
| `SESSION_HOURS` | `12` | session inactivity limit |
| `VAPID_PRIVATE_KEY_PEM`, `VAPID_SUBJECT` | – | Web Push reminders (`gen-vapid` creates a key); unset = push off |
| `HOLIDAYS_COUNTRY`, `HOLIDAYS_SUBDIVISION`, `HOLIDAYS_ENABLED` | `DE`, –, `true` | OpenHolidays API defaults (users can override the subdivision) |
| `DEFAULT_TIMEZONE`, `DEFAULT_LANG` | `Europe/Berlin`, `de` | fallbacks |

## Commands

```text
rustical-ui serve                          # default
rustical-ui check-rustical-image <ref>     # exit 0 iff the image tag == the compiled rustical crate tag
rustical-ui gen-vapid                      # print a fresh VAPID private key (PEM) + public key
rustical-ui --version
```

`/healthz` answers 200 once the database is reachable.

## The RustiCal version contract

The five `rustical_*` crates in `build/Cargo.toml` pin **one** git tag; `build.rs` refuses a
mix and exports it as `RUSTICAL_TAG`. That tag must equal the tag of the rustical image the
UI runs next to — the crates carry RustiCal's schema expectations. The deployment repo's
tool check runs `check-rustical-image` with the rendered rustical image reference, so a
Renovate bump of the rustical image alone turns that repo red until this repo has been
bumped to the same tag (Renovate groups the five crates) and published. Order: bump here →
image published → bump the rustical image there → Renovate re-pins this image's digest.

## Local development

```bash
mise run dev-db                # COPY of the production DB → ~/.cache/rustical-ui-dev (cluster read-only), test password
mise run dev                   # http://localhost:4001, login <user>/devpass — the feedback loop below
mise run test                  # cargo test (rust-toolchain.toml → 1.98)
mise run deny                  # cargo-deny: advisories, bans, sources
mise run pipeline              # the CI image build + tests/tests.yaml, loaded into docker
```

`mise run dev` is the edit-and-see loop: `watchexec` rebuilds and restarts on every change under
`src/`, `templates/` and `locales/` (an incremental debug build takes ~3 s); `RUSTICAL_UI__DEV_ASSETS_DIR`
makes the server read `assets/*` from disk on every request (uncached) and mounts `/dev/reload`, an
SSE stream the page listens to (`body[data-dev]`): a new process boot id or an asset change reloads
the open tab, and `RUSTICAL_UI__DEV_LOGIN` signs `DEV_USER` back in after the restart (sessions are in
memory), so nothing needs a manual refresh or login. Never point `DB` at a live RustiCal file — `dev-db`
takes a `VACUUM INTO` copy through the backup sidecar. Both dev knobs are unset in production.

## Repository layout

Standard xarif docker repo: `build/` (Rust crate, `Dockerfile`, `deny.toml`, `templates/`,
`assets/`, `locales/`), `tests/tests.yaml` (image tests), `mise.toml`, the two Forgejo
workflows and `.gitlab-ci.yml` from `base/dagger-ci-basics`. Image:
`registry.gitlab.com/xarif/docker/rustical-ui:main` (built on Forgejo, pulled from the
gitlab.com mirror like every xarif image).
