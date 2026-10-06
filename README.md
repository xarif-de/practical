# PractiCal

**The web UI next to [RustiCal](https://github.com/lennart-k/rustical).** RustiCal is a
CalDAV/CardDAV server with a minimal frontend and no admin API. This sidecar adds what a
family needs on top: groups you own together, calendars and address books for yourself or a
group, app tokens for every device, a trash with a grace period, and (phase 2) the actual
calendar, contacts and tasks views.

**What it does (phase 1 + 2):** groups and memberships · calendars and address books per
person or group · app tokens · trash with restore · calendar views Day (ledger + document),
Week (stream) and Month (tiles with continuous multi-day bars) — in both, scrolling only moves what is on
screen (`?week=<Monday>` / `?month=YYYY-MM`) and a click on a day (number and date label included; also a Year-planner cell) picks it without switching the view; the picked day (`date`) is shared by all views — and the "More" views Year planner,
Free weekends, Vacation planner, Countdown wall, Year in numbers · event editor as a card form
(view mode behind a lock, cards only where data is): reminders on a timeline strip with per-user
presets (before start, before end, at a time; display, sound or mail; repeats), recurrence painted on a week strip (weekdays, day of month, interval, end, exceptions and extra dates),
organizer and attendees as chips (display only, with a jump to the matching address-book entry), links grouped as conference / in this calendar / internet with a join card, embedded files and event/task links, location with coordinates and map actions, status/busy/visibility/
priority as four tiles whose icon follows the value, links, attachments and conference URIs, an event colour, categories, notes ·
contacts as letter panels (fields per entry selectable) or an
inline-editable sheet, sorted by first name (default), last name, birthday (month panels) or e-mail,
ascending/descending (client-side, `localStorage.ctSort`/`ctDesc`; entries lacking the value last), a quick-jump rail, and a card editor: view mode with actions (call, SMS,
WhatsApp, mail, maps, chat, jump to a related contact, vCard download), edit mode behind a lock,
the full vCard field set (structured addresses, URLs, messengers, relationships, extra dates,
preferred flags), address books with a colour · minimal tasks (VTODO) · universal search (see **Search**) · orbit date
picker · public/school holidays for 204 countries (OpenHolidays → Nager.Date) · vacation-day counting ·
browser push reminders (VAPID) · `.ics`/`.vcf` export and import · change history with undo (see **Change history**) · live updates over SSE · German,
English, French and Spanish.

It never talks to RustiCal over HTTP for data. It opens **RustiCal's own SQLite file** through
RustiCal's store crates (`rustical_store_sqlite`, git dependency pinned to the RustiCal release)
and keeps its own state in `practical_*` tables of the same file, so one backup covers everything.

**One image, both programs:** the image also carries RustiCal's own binary, unmodified
(`/usr/local/bin/rustical`, copied from its official image at the crates' tag), so the pod's two
containers run the same image — `rustical serve` and `practical serve` — and the CalDAV server
and the UI built on its crates are always one release. Deployed on xarif.de in the `rustical` pod:
[kubernetes/rustical 🌐](https://git.xarif.de/kubernetes/rustical) → <https://plan.xarif.de>.

## Contents

- [How it works](#how-it-works)
- [Configuration](#configuration)
- [Commands](#commands)
- [The RustiCal version contract](#the-rustical-version-contract)
- [Local development](#local-development)
- [End-to-end safety net](#end-to-end-safety-net)
- [Repository layout](#repository-layout)
- [License](#license)

## How it works

- **One database, two processes.** RustiCal and PractiCal run in the same pod as the same
  uid and share the WAL-mode SQLite file. PractiCal waits for RustiCal's `/ping` before it
  opens the file (RustiCal runs its schema migrations at start) and never runs those
  migrations itself. Its own tables live under the `practical_` prefix with their own
  `practical_migrations` ledger — never `_sqlx_migrations`, which would make RustiCal abort on an
  "unknown migration".
- **Groups are RustiCal `group` principals** (`group.<slug>`) owned by their members: every
  member may add or remove members, create the group's collections and delete the group.
  Deleting moves memberships and collections to the trash (`practical_trashed_groups` + RustiCal's
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
  toolbar sticks under the topbar, scrollbars are hidden everywhere. One shared `style.css` +
  `app.js` serve every view: 90 KB brotli / 104 KB gzip together (measured 2026-10-06, built assets).
- **Assets** (`build/build.rs`, `routes/assets.rs`): JS minified by oxc (parser-based, ES2020, top-level
  names kept), CSS by `src/css_min.rs` (no dependency), each stored as gzip-9 + brotli-11 at BUILD time and served by
  `Accept-Encoding` — no request compresses. `?v=<asset_version>` URLs are `immutable` for a year;
  sw.js, icon.svg and any other version revalidate daily (ETag per coding, 304). `/assets` and
  `/healthz` sit outside the session, authentication, CSRF and compression layers.
- **First paint**: `app.js` sits in `<head>` with `defer blocking="render"`, so a page is painted only
  once app.js has drawn it (task views, sort boxes); the browser keeps the old page meanwhile —
  no empty frame (Chromium; others ignore the attribute). **Tasks** are parsed once per calendar and
  synctoken (`tasks::TaskCache`), caught up via `sync_changes` (dev copy: /tasks 0.93 s → 35 ms,
  task categories 0.65 s → 8 ms, debug build). The lookback persists per browser (cookie `tk_back`;
  a URL `back=` wins).
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
- **Links & attachments** (event card): one paste field detects the kind of a link (Zoom, Teams,
  Meet, Jitsi, Webex, BigBlueButton hosts → conference; file extensions → attachment; else website),
  a search over events and tasks links them as RELATED-TO (resolved to a title and date on load), and
  a drop zone embeds files up to 2 MiB each (6 MiB per save) as base64 ATTACH with FMTTYPE and
  FILENAME; larger files belong in the cloud as a link. Embedded attachments are downloadable
  (`GET /api/events/{key}/attach/{n}`) and can be removed again (the client sends the ones to keep).
  The entries are grouped: a conference gets a join card with provider, meeting id (Zoom `/j/…`,
  Meet code) and a "password in the link" hint, then "in this calendar", then "on the internet".
  FMTTYPE, FILENAME, SIZE and FEATURE are modelled, other parameters ride along untouched.
- **Notes** (event card): the note stays plain text (DESCRIPTION). While typing, an index of
  contacts, categories, events and tasks (`GET /api/mentions`, loaded on the first keystroke) offers
  suggestions even without a prefix; picking one inserts `@Name`, `#Category` or `!Title`. Persons,
  links and categories the note mentions are offered as chips that create the entry in the matching
  card; phone numbers and e-mail addresses become call/write chips. The view renders links, numbers,
  mentions (contact, category search, event) and "- " lists. A changed description drops Outlook's
  `X-ALT-DESC` and `STYLED-DESCRIPTION`, which would otherwise keep showing the old text elsewhere.
- **Categories and rules** (event card, settings tab "Categories"): categories are ordered rows with a
  grip; typing offers the known categories with counts ("existing") or creates a new one, the rest of
  the known ones sit in a cloud below. A rule per category (`practical_category_rules`, JSON per user, see
  `domain::catrules`) puts values on an event: title, all-day, calendar (new events only), colour, location,
  reminders, repeat, status/busy/visibility/priority and links, each as a *template* (prefilled when
  the category is added, editable) or a *lock* (set on open and fixed in this UI; a differing stored value is
  replaced silently, the field shows the forced value). Location and repeat carry one mode per card, all-day/calendar/colour
  and the four status values one per setting, reminders and links one per entry. For one setting a lock beats a
  template, among equals the upper category wins; the category rows show what a rule does, strike
  through what another category overrides and mark a template the event deviates from as "changed". Every card lists the templates
  and locks that touch it in a footer below a rule (name · badge · category · "changed");
  locked settings are greyed out. The rule editor is the same dialog in a rule mode with the same
  cards; its footers carry a chip per setting or entry that switches between "any" (setting disabled,
  not part of the rule), template and lock. Opened from an event ("Rule" in a row) the editor stacks
  over it and returns with the rules re-applied; from the settings tab (`/calendar?rule=<name>&back=…`)
  it returns to the tab. Locks hold in this UI only; other clients change freely.
- **People** (event card) are shown, not edited: RustiCal does no scheduling (RFC 6638), so nobody
  would send an invitation or carry a reply back — the list is a record. Each ORGANIZER/ATTENDEE is a
  chip whose ring colour is the PARTSTAT; a click opens a detail card (role, reply, delegation, RSVP,
  address, mail/copy actions). `GET /api/events/{key}` adds `contacts` to every person: the address-book
  entries with the same e-mail, else a similar name (`contacts::matches`: same family name, given names
  equal or one a prefix of the other; rooms and resources never match). One hit becomes a jump to
  `/contacts#key`, several open a chooser. On save the persons go back unchanged, every parameter
  included (RSVP, DELEGATED-*, MEMBER, SENT-BY, DIR, LANGUAGE, SCHEDULE-*, X-…).
- **Reminders** (event card): the edit mode shows the alarms on a timeline strip (a marker follows
  the pointer and snaps to the user's snap points, each point owning an equal share of the strip;
  settings › Reminders edits them on the same strip: a point opens number + unit / remove, a "+" per gap
  adds the round value between its neighbours, the last start point stays), a row of preset chips
  that add or remove the matching alarm with one click, and one expandable entry per alarm whose
  fields follow the kind (text · sound file · subject, message, recipients) and the trigger (number +
  unit, or date and time). The star in an entry saves its setting — without text, subject and
  acknowledgement — as a preset; presets are per user (`practical_user_settings.alarm_presets_json`,
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
- **Colours** (`domain::colors`, app.js `colorPicker`): one picker everywhere a colour is set (calendar,
  address book, event, task, category rule, holidays). Its circles are the colours already in use
  (`GET /api/colors`: collections, holiday colours, rules with a colour, events/tasks with an own COLOR —
  one scan of all objects), each named by its sources; first comes the fallback where there is one
  ("like the calendar", "standard"). ＋ opens a dialog with a fan deck (14 families × 5 OKLCH tones) or a text
  field (any CSS colour) with three linked sliders (simple/RGB/OKLCH), the distance to the colours in use
  (OKLab; "almost the same as …" offers that one instead) and a light and dark preview. A picked colour
  becomes a source, so the offer grows and shrinks with the data. Settings "Colours" lists every element with
  its colour and changes it in place (`PUT /api/colors/{kind}/{key}`); an event's or task's COLOR is rewritten
  on its master component only (`colors::with_color`), every other line keeps its bytes. A category's colour is
  a setting of its rule: the row rewrites `when.color.value` (template or lock stays as it was).
  Existing events/tasks with the category that still carry the old colour are counted
  (`POST /api/colors/category`, `colors::category_objects`); the user is asked whether they follow — one
  given another colour by hand is left alone.
  The closing list (what the picker offers, one row per colour) recolours every element of that colour at
  once — collections, events/tasks, category rules, holiday colours; read-only ones stay.
  **Farben optimieren** (app.js `optimise`): one button per view whose colours meet — calendar view
  (calendars, events, event categories, holidays) and task list (calendars, tasks, task categories); tasks
  never show in the calendar (`events.rs` expands VEVENTs only), address books have no button. Families =
  union of: event→first coloured category it carries (`/api/colors` sends `cats`), equal titles, then per
  level word stem ("Urlaub"⊂"Urlaubstag"), abbreviation ("RB"~"Rufbereitschaft"), title series
  ("X: …"); two different categories/calendars never merge. One slider, 6 levels: target = most used colour
  (0–1) or the lead's (holiday > category > calendar; holidays at every level, 2–5); families closer than T
  (0, .04, .06, .09, .12, .09; OKLab) are pulled apart — the lighter one gives way; calendars, holidays and
  user-picked colours never move. New colours: the farthest from what stays, tier by tier (Apple system
  colours → Google Calendar event/calendar colours → OKLCH ring → browns), a tier until it cannot keep T.
  Level 5 repaints every other family: farthest-point set, then each family takes the nearest colour of it. Flow chart per family: element groups → action → colour after (SVG lines). Manual: pick a
  colour on the result tile (the picker, `once`), ⊘ leave an element out, drag an element onto another action
  (join) or a gap (own "Trennen" action), leave an action out; all marked ✋, undoable, and a level change
  asks before it drops them. Hints: a picked colour too close to one that cannot move, hard to see on light
  or dark, a calendar colour that changes the other view too. Apply = `POST /api/colors/batch`
  (`colors::apply`: every change, failures counted, rules and prefs written once).
- **Search** (top bar; `domain::search`, `/api/search`, app.js universal search): every field of events
  but recurrence and alarms (title, place, categories, people, links, notes, related titles, status/show
  as/visibility/priority words in every UI language, coordinates, calendar), of contacts (all names,
  affixes, organisation, job title, e-mail, phone digits, messenger, addresses, web, categories, relations,
  notes, address book) and of tasks (title, categories, place, links, notes, related, status/priority/
  progress, coordinates, calendar). Field weights (search.rs, above `index`): what the hit is called 1,
  what describes it next, connections (people, relations, links) lower, status words and coordinates
  least; the calendar / address book name .15 and only narrowing ("foobar zahn" yes, "foobar" alone no).
  A query that is a day (`17.11.`, `17.11.1987`) searches dates instead: occurrences, birthdays (with a
  year only that year), anniversaries, a contact's other dates, a task's due/start/done day. Every hit says
  why it was found (per field: the text around the match, cut on the server, and how each word matched —
  whole word, word start, written together, word starts, inside, like "x" (typo), digits, date); the dialog
  shows all of it, the drop-down only what its row hides (a hidden field, a typo). Ties go to the hit whose
  title/name holds all the query's letters in order.
  Text: folded (case/diacritics off), every query word must match; per word: whole word > word start >
  words typed without spaces (from a word's start over a boundary, 2+ letters of the next word:
  "dietmarstein") > starts of consecutive words (5+ letters, parts 2+, the last 3+: "dietstein") > inside a word >
  typo (OSA distance 1 from 5 letters, 2 from 8; fields weighing ≥ .5) > letters left out (6+ letters in
  order from a word's start over ≤ twice their length: "chrstinbach"; fields weighing ≥ .8: titles,
  names, organisation). Tiers: exact kinds first; while there is no clear hit (≥ .85) and fewer
  than 3 above the floor, loosen — typos (keeping the first letter) and letters left out where allowed,
  then in every field with up to half the words missing (moot when some document has every word). The
  floor is relative to the best hit: clear best → drop below 50 % (60 % with ≥ 10 clear hits), typo-level
  best → keep down to 15 %. Ranking adds the nearness of the next (else last) occurrence for events (one hit per series,
  "n events"), openness and due date for tasks. Prefixes `termin:`/`event:`, `kontakt:`/`contact:`,
  `aufgabe:`/`task:` (+ fr/es) narrow to one kind; a date query adds "jump to". Drop-down: ≤ 5 per kind,
  ≤ 10 in all, split by each kind's share of the matches; "Details / All results" (always there with a query, pinned to the drop-down's foot) / Enter opens a dialog with ≤ 200 per
  kind and kind tabs; ↑↓ Enter Esc, `/` and Ctrl/⌘ K focus, <mark> on matches; an info button explains it.
  A pick keeps the view on screen where it can show the hit: calendar day/week/month/year (else the day
  view), contacts list/table (else the list; `/contacts#key` always opens the list), tasks
  groups/table/board/timeline/deck/history (else the groups list; `/tasks#key`; finished before the
  lookback → `back=all`). Picked contacts/tasks stay picked across view switches. Cost: per user, the
  corpus (±2 years of occurrences, contacts, tasks, pre-folded) is cached in memory (`IndexCache`) under a
  fingerprint of the user's collections' synctokens — rebuilt on any change (also CalDAV), dropped after
  30 min idle; the client debounces (180 ms), aborts stale requests and keeps answers 30 s.
  History (`practical_search_history`, `/api/search/history`): a query is kept when a result is opened from it
  (with that result, so Tab on the grey completion jumps straight there) or on Enter; matching entries
  show at the drop-down's foot (× deletes), recent ones on an empty focus. Settings → Search: retention
  (`practical_user_settings.search_history_days`: 0 = off, 7/30/90 (default)/365 days, -1 = unlimited; ≤ 300
  entries), the list with single deletes, clear all.
- **Change history** (`/history`; `domain::history`, `domain::history_text`, `routes::history`, assets
  `history.js`/`history.css`, tables `practical_history_*`): every closed change of an event, task or contact.
  RustiCal keeps no old versions and its change log names no content or author, so PractiCal keeps its
  own: `heads` = last version seen per object, `blobs` = content-addressed versions, `cursors` = log read
  per collection (first sight = baseline, no change; dev copy 4 263 objects, 4.2 MB). Attribution: a UI
  request runs between `history::begin` and `Guard::commit` (one lock with the 2 s poller, which ingests
  first on each tick) → one change per save/import/bulk recolour; what else moved is a DAV change, one
  per object, device = PRODID of the new version; delete+create of one UID = move (the UI's move keeps
  the UID, as CalDAV MOVE does). Diff = attributes (master property name; VALARM/overrides as blocks;
  ACKNOWLEDGED/DTSTAMP/SEQUENCE/X-MOZ-/X-APPLE- = technical, hidden by default). Undo is itself a
  change (`reverts`, the undone one gets `undone_by`): an edit puts back only the attributes it touched
  (3-way: a value changed later → warning, needs confirm, then overwritten); undoing a create deletes
  and makes later changes `inactive_by` it (undoing that undo reactivates them); a delete restores from
  the stored version (also after RustiCal's trash purge). Values read like the editor (`history_text::Show`): an alarm = when · kind (display/sound/e-mail) · recipients · own text · repeat · sound · proximity; an attendee = name <address> (answer, role, type); vCard values with TYPE and Apple label; several values one per line, the page marks which went/came/stayed; two versions that read the same are shown as written (tests edit every property of the full demo event and a contact). Visible/undoable: collections of the user and
  their groups. Views: line map (default; one line per element, stops = changes, undo arcs) and list
  with details (before/after/today, undo preview, the element's history, day ruler). Kept
  `HISTORY_RETENTION_DAYS` (purge hourly with the trash purger).
- **Templates** (event editor, new events only; `search::suggest`, `/api/events/suggest`, app.js `tpl*`):
  typing a title (≥ 2 letters, 200 ms) lists ≤ 3 past events, one per folded title. The title match only
  has to pass the search's relevance floor (`floor`, tier 1: typos count, a clear match drops them);
  rank = past occurrences of the title (every event/series with it, no time limit) > match > recency.
  Corpus: every occurrence up to tomorrow (`Index::past`, built on the first suggestion, dropped with
  the index; ~0.1 s debug on 3 800 objects). Cancelled events are no template; the newest occurrence's
  event is. Picking one (click, ↑/↓ + Enter; Esc closes the list only) fills every attribute but the
  date: day = the dialog's, times/duration/series pattern/absolute reminders move with it; UNTIL,
  EXDATE/RDATE, attendees' PARTSTAT (→ NEEDS-ACTION), ACKNOWLEDGED stay behind; embedded files are
  fetched and sent as new ones; a calendar no longer visible falls back to the chosen one.
- **vCard editing** keeps what it does not model: on update every property outside the managed set
  (PHOTO, X-…, client items) is carried over verbatim (`preserved_lines`). Parameters are read from
  the raw vCard text (`raw_lines`) because caldata exposes only the first value of a parameter —
  `TYPE=HOME,PREF` and Apple's repeated `;type=…` would otherwise be invisible. Address-book colours
  live in `practical_addressbook_colors` (RustiCal stores none), list-view field choices per user in
  `practical_user_settings.contact_list_fields_json`.
- **Addresses** (event location card): suggestions while typing come from [Photon](https://photon.komoot.io)
  (komoot, OSM data, built for search-as-you-type), the address of a map position from
  [Nominatim](https://nominatim.openstreetmap.org) (OSMF; its policy forbids autocomplete and demands
  an identifying User-Agent, so it only answers the map dialog's button). Both go through the sidecar
  (`/api/geo/search`, `/api/geo/reverse`): it sends the User-Agent, throttles each provider to one
  request per second, caches answers for a week and keeps the users' IPs at home. The browser asks
  only after a 2 s typing pause (the ring in the field counts it down) or on a button. The map
  dialog is Leaflet 1.9.4 (vendored, BSD-2, loaded on first use) with tiles from
  tile.openstreetmap.org, attribution shown as the tile policy requires.
- **Holidays**: a source chain per request — public: `openholidaysapi.org` (36 countries, regions down
  to municipalities) → `date.nager.at` (204 countries, ISO 3166-2 regions) → stale cache; school:
  OpenHolidays → stale cache. 8 s timeout per link, answers cached 30 days in `practical_holidays`.
  Each user keeps two region lists, public and school holidays (default: `HOLIDAYS_COUNTRY`/
  `HOLIDAYS_SUBDIVISION`); an entry is a whole country (`DE`) or a region of any depth (`DE-SN`),
  stored as ISO 3166-2 codes where one exists. Holidays several regions share show once; others
  carry their regions' codes (`Buß- und Bettag (SN)`). The first public region is the user's own:
  vacation days and bridge days count by it. Country/region names in every UI language (all ISO
  countries) and the region tree of countries OpenHolidays lacks come from Unicode CLDR, built in
  (`build/src/data/regions.json` from `tools/gen_regions.py`; Unicode License v3). Upkeep is lazy
  (daily task; at start it only reads saved state): which source covers which country and CLDR's
  latest release are each asked at most weekly (one request with ETag, usually a 304); only a
  newer release is downloaded (~3.5 MB, about yearly) and saved in `practical_holidays`, where it wins over
  the built-in file. A broken answer (error, empty or halved list) never replaces a known one.
- **Import**: into an existing collection (`.ics` merged by UID, `.vcf`), or a new collection from a
  file (settings › Calendars & address books › Import calendar … / Import address book …). A `.vcf`
  names no book: the file name is suggested, contacts and groups are counted, a card without UID
  gets one (as RustiCal's IMPORT), the colour is written after `AddressbookStore::import_addressbook`
  succeeded. For an `.ics` the page prefills name,
  description, colour and zone from the file's X-WR-CALNAME/-CALDESC/-CALCOLOR/-TIMEZONE (as
  RustiCal's own IMPORT; colour also X-APPLE-CALENDAR-COLOR), counts the objects per type and locks
  those types on; the server drops the X-WR-* lines from the objects and creates calendar and
  objects in one transaction (`CalendarStore::import_calendar`, body ≤ 32 MB).
- **Vacation rule** (settings › Vacation, `prefs::VacationRule`, `{"v":2,"kind","value"}`): a category
  (picked from those in use, any case), a calendar, or a title regex (Rust `regex`, any case, ≤ 200
  chars; the page offers building blocks and tests the pattern live against every event title from
  `/api/titles`, translating `\b` to Unicode for JS and refusing look-around/back-references). Rules
  without `v` meant "category OR title word" and count so until the next save; the page shows them as
  the category if one of that name is in use, else as `\bword\b`.
- **Push reminders**: browsers register `assets/sw.js` and subscribe with the VAPID public key;
  a one-minute loop expands the subscribers' events a few days ahead, sends each VALARM once
  (`practical_alarms_sent`) and drops subscriptions the push service reports gone (404/410).
- **i18n**: Fluent `.ftl` files (`build/locales/{de,en,fr,es}.ftl`), embedded at compile time; a
  missing key falls back to English, then German, and a test keeps all files complete. Language =
  user preference → session → `Accept-Language`. app.js gets its `js-*` strings as JSON from the
  page (placeholders `%name`); month/weekday names and date formats come from `Intl`. A new
  language: its `.ftl`, `LANGS`/`NATIVE`/`LOCALE` in `src/i18n.rs`, `LANGS` in `tools/gen_regions.py`.

## Configuration

Environment variables `PRACTICAL__<KEY>`, set on the PractiCal container only.

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
| `HISTORY_RETENTION_DAYS` | `90` | how long the change history keeps changes (and can undo them) |
| `SESSION_HOURS` | `12` | session inactivity limit |
| `VAPID_PRIVATE_KEY_PEM`, `VAPID_SUBJECT` | – | Web Push reminders (`gen-vapid` creates a key); unset = push off |
| `HOLIDAYS_COUNTRY`, `HOLIDAYS_SUBDIVISION`, `HOLIDAYS_ENABLED` | `DE`, –, `true` | holiday region until a user picks one; `false` = no holiday requests |
| `DEFAULT_TIMEZONE`, `DEFAULT_LANG` | `Europe/Berlin`, `de` | fallbacks |

## Commands

```text
practical serve                            # default
practical check-rustical-image <ref>       # exit 0 iff the image tag == the compiled rustical crate tag
practical gen-vapid                        # print a fresh VAPID private key (PEM) + public key
practical --version
```

`/healthz` answers 200 once the database is reachable.

## The RustiCal version contract

The five `rustical_*` crates in `build/Cargo.toml` pin **one** git tag (`build.rs` refuses a mix and
exports it as `RUSTICAL_TAG`), and `build/Dockerfile` copies RustiCal's binary from
`ghcr.io/lennart-k/rustical:<the same tag>` — the crates carry RustiCal's schema expectations, the
binary is the server that migrates that schema. Three guards keep them one release: `build.rs`
compares the Dockerfile's tag with the crates' on every local build (the Docker context has no
Dockerfile), the image test `rustical-binary-is-the-crates-release` compares
`rustical --version` with `practical --version`, and one Renovate group (`renovate.json`,
matched by dependency name — for a git dependency the package name is the repository URL) moves
all six in one PR. The deployment repo only checks that both containers run the same image
(`same_as`). `check-rustical-image <ref>` still compares a RustiCal image reference with the
crates' tag, for a deployment that runs RustiCal's own image.

## Local development

```bash
mise run dev-db                # COPY of the production DB → ~/.cache/practical-dev (cluster read-only), test password
mise run dev                   # http://localhost:4001, login <user>/devpass — the feedback loop below
mise run test                  # cargo test (rust-toolchain.toml = the builder image's Rust version)
mise run deny                  # cargo-deny: advisories, bans, sources
mise run pipeline              # the CI pipeline locally (cargo-deny + image build + tests), never publishes
mise run image                 # the image, loaded into docker as xarif-practical
```

`mise run dev` is the edit-and-see loop: `watchexec` rebuilds and restarts on every change under
`src/`, `templates/` and `locales/` (an incremental debug build takes ~3 s); `PRACTICAL__DEV_ASSETS_DIR`
makes the server read `assets/*` from disk on every request (uncached) and mounts `/dev/reload`, an
SSE stream the page listens to (`body[data-dev]`): a new process boot id or an asset change reloads
the open tab, and `PRACTICAL__DEV_LOGIN` signs `DEV_USER` back in after the restart (sessions are in
memory), so nothing needs a manual refresh or login. Never point `DB` at a live RustiCal file — `dev-db`
takes a `VACUUM INTO` copy through the backup sidecar. Both dev knobs are unset in production; so is
`PRACTICAL__DEV_NOW` (RFC 3339, accepted only with `DEV_ASSETS_DIR`), which freezes every server-side
"now" for reproducible pages (the e2e suite).

## End-to-end safety net

`e2e/` proves that a change keeps function **and** look: a functional Playwright suite and a visual
baseline of every UI state (PNG + normalised DOM + computed-style digest), both against a release
build of the checkout on a synthetic database.

```bash
mise run e2e-seed                       # fixture DB → e2e/.work/fixture.sqlite3 (docker: RustiCal CLI)
mise run e2e                            # functional suite (e2e/functional), serial, one fresh DB copy
mise run e2e-baseline -- <dir>          # visual suite → <dir> (default ~/.cache/practical-e2e/<sha>)
mise run e2e-compare -- <dirA> <dirB>   # pixel/DOM/style diff, report in <dirB>/compare-report; exit 1 on any difference
```

- **Fixture** (`e2e/seed/`, synthetic data only): users alice/bob/carol (password `devpass`, carol
  has nothing = empty states), group `group.familie`, five calendars, two address books, tasks, rules,
  history, holidays (cache rows; no egress). Collection/token ids are fixed UUIDs (`5eed0000-…`),
  clock-derived values fixed: two seeds render the same pages. Regenerate only when the seed changes —
  compare baselines taken on the same fixture (its sha256 is in `<dir>/.e2e-baseline`).
- **Server** (`e2e/global-setup.mjs`): development mode on `127.0.0.1:4101` (`E2E_PORT`) with
  `PRACTICAL__DEV_LOGIN=alice` and `PRACTICAL__DEV_NOW=2026-10-14T08:00:00Z` — the dev-only fixed
  clock (`build/src/clock.rs`), serving the minified assets that build embeds (`E2E_ASSETS=source`: `build/assets`
  as written) — plus a non-dev server on port+1 for the login page; egress blocked
  (`HTTP(S)_PROXY=127.0.0.1:9`). Browser: clock frozen at the same instant, Europe/Berlin, de-DE
  (en-GB for `@main` states), reduced motion, transitions/animations/caret off, `Math.random` seeded,
  service workers blocked, no request leaves 127.0.0.1 (OSM tiles = one gray PNG).
- **Visual projects**: desktop 1440×900 and phone 390×844 × light/dark (de) + `@main` states in en.
  Visual tests are read-only (writes are aborted), so they run in parallel on one DB.
- Pixel baselines depend on the host's fonts and Chromium build (`@playwright/test` pinned in
  `e2e/package-lock.json`): compare baselines from the same machine.

## Repository layout

Standard xarif docker repo: `build/` (Rust crate, `Dockerfile`, `deny.toml`, `templates/`,
`assets/`, `locales/`), `tests/tests.yaml` (image tests), `mise.toml`, and `dagger-ci.yaml`: the whole
CI is one `dagger call … ci` (`base/dagger-modules` docs/ci.md), which `.forgejo/workflows/ci.yml`
only triggers. Every push runs cargo-deny and builds + tests the image; `main` publishes with the
`release` profile (skipped when the build inputs are unchanged), `v*` tags with the `dist` profile.
Deployment follows from the push: Renovate pins the new `:main` digest in `kubernetes/rustical`
(PR, CI, auto-merge, up to 2 h after the publish), ArgoCD syncs it from the gitlab.com mirror, and the
pod restarts with both containers on the one new image.
The Dockerfile keeps cargo's
registry and `target/` in BuildKit cache mounts, so a build recompiles only what changed
(`base/dagger-modules` docs/rust-functions.md). Image:
`registry.gitlab.com/xarif/docker/practical:main` (built and pushed by Forgejo Actions,
pulled from the gitlab.com registry like every xarif image).

**Icons** (`templates/layout.html`, one `<symbol id="i-…">` sprite used via `<use>`): own drawings
plus single SVGs copied unchanged from Lucide (ISC), Tabler (MIT), Phosphor (MIT), Heroicons (MIT),
Iconoir (MIT), Bootstrap Icons (MIT), Material Design Icons (Apache-2.0) and Material Symbols
(Apache-2.0); `i-al-*` are drawn from Lucide `log-in`/`log-out`. `i-sort` dims one arrow via
`--sort-up`/`--sort-down`, `i-star-h` fills via `--ico-fill`.

## License

AGPL-3.0-or-later (`LICENSE`; a copy in `build/LICENSE` goes into the image at
`/usr/share/licenses/practical/LICENSE`, a test keeps both equal). PractiCal links RustiCal's
crates, which are AGPL-3.0-or-later themselves, and runs as a network service: §13 is met by the
"Source code" link in the user menu and on the login page, next to both versions. The image
redistributes RustiCal's binary unmodified; its source is
<https://github.com/lennart-k/rustical> at the tag the UI shows (and `rustical --version` prints).
