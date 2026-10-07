# MAP — where things live in docker/practical (for agents; rules: /AGENTS.md)

Line 1 of every source file but main.rs states its purpose (Rust `//!`, JS/CSS `// js/x.js —`, templates `{#- … -#}`):
read that before opening more. `rg`/`fd` skip generated/vendored/golden data (`.ignore`; search them with `-u`).

## Data flow
```
browser ─HTTP─► main.rs build_router: TraceLayer → catch_panic → CSP + Cache-Control map_response
  ├─ /assets/{name}, /healthz: merged after the layers below (no session, auth, CSRF, compression)
  └─ CompressionLayer(gzip) → etag::revalidate (weak ETag, 304, brotli of the identity body) → session layer
     (sessions::DbStore) → sessions::activity (expiry extension) → rustical AuthenticationLayer (principal from
     session or HTTP Basic app token) → csrf::origin_guard → router
       ├─ auth::router (/login, /login/oidc*, /logout), routes::dev (/dev/reload, dev only), / → /calendar
       └─ protected: auth::require_user → ctx::localize_errors → error::catch_panic → routes::{calendar,contacts,tasks,history,settings,editor,more,api}
handler: extractors extract::{Path,Query,Json,Form} (validated; a rejection names the field) · csrf::{Token,TokenOrBasic} (after
  path, before body: bad token + bad body = 403) · routes::calendar::PageView(PageCtx, ViewCtx) → domain::* (no HTTP
  types) → RustiCal stores (rustical_store_sqlite: SqliteCalendarStore/AddressbookStore/PrincipalStore, AppState) +
  own tables via sqlx → askama template (templates/, ctx = ctx::PageCtx) | JSON | error::AppError (status + Msg key
  → localized text)
writes in the UI: history::begin(state, Actor) … Guard::commit (one lock with the poller → one attributed change)
live: tasks::spawn_change_poller → state.changes (broadcast ChangeEvent) → /api/changes SSE → app.js refetches fragment
```
Shared DB: RustiCal and PractiCal = same pod, same uid, one WAL SQLite file. PractiCal waits for RustiCal `/ping`
(its migrations run at its start), never runs RustiCal migrations (`create_db_pool(.., migrate=false)`), touches
RustiCal tables only via its store crates (exceptions: read-only queries, e.g. synctokens). Own tables `practical_*`,
ledger `practical_migrations` (never `_sqlx_migrations`: RustiCal aborts on an unknown migration); db.rs `MIGRATIONS`
append-only; `db::check_schema` logs drift.

## build/ (crate `practical`; Docker context = build/)
| path | purpose · key items |
|---|---|
| Cargo.toml | deps (policy: RustiCal crate, in RustiCal's graph, or a one-line reason), profiles release/dist/e2e |
| build.rs | RUSTICAL_TAG (one tag for all rustical_* crates; = Dockerfile's rustical image tag), assets: bundle join (src/bundle.rs) → oxc minify (JS, ES2020) / css_min → gzip-9 + brotli-11 → OUT_DIR/assets.rs; i18n-<lang>.js from the .ftl (js-* keys, plural forms); regions.json → 2 gzip name tables |
| Dockerfile | rust-alpine builder (cache mounts, `touch` all = no stale mtime binary) → alpine + /practical + RustiCal's binary (same tag) |
| deny.toml | cargo-deny advisories/bans/sources (`mise run deny`, CI) |
| rust-toolchain.toml | = builder image's Rust; one Renovate group moves both |
| .cargo/config.toml | target-dir outside build/ (dagger uploads build/); SHARED by worktrees → use e2e/cargo.sh |
| askama.toml, LICENSE (copy of /LICENSE, test keeps equal), tools/gen_regions.py (→ src/data/regions.json, CLDR) | |
| locales/{de,en,fr,es}.ftl | Fluent; prefixes: `ed-` editor, `ct-` contacts, `tk-`/`task-` tasks, `hx-`/`hxj-` history (`hxj-`, `js-` ship to the browser), `v-`/`view-` views, `err-` AppError messages, `set-` settings, `js-search-f-*`, `hx-a-*` (dynamic prefixes) |
| templates/ | layout.html (head: theme.js, i18n-<lang>.js, app.js `defer blocking="render"`; top bar, nav, toast) · macros.html (`m::ico`, `pop_btn`, `card_h`, `act`, `add_btn`, `coll_page`, `restore`/`purge`/`trash_empty` …) · calendar.html + calendar_fragment.html (`?fragment=1`) · editor.html (one dialog: event, task, category rule) + editor_islands.html (its JSON islands) + editor_fragment.html (/editor) · contacts, tasks, settings (drill-in per hash), history, login |
| assets/ | js/** + css/** = parts of app.js / style.css (order: src/bundle.rs `BUNDLES`) · history.js/.css (/history only) · sw.js (Web Push) · theme.js (scheme before first paint) · icons.svg (sprite `<symbol id="i-…">`; `i-sort` dims an arrow via `--sort-up`/`--sort-down`, `i-star-h` fills via `--ico-fill`) · icon.svg · leaflet.* (vendored 1.9.4, BSD-2, loaded on first map use) |
| tests/fixtures, tests/snapshots | goldens (`UPDATE_SNAPSHOTS=1 mise run test` rewrites) · recurrence-real.json (60 anonymised real series × 60 instances by python-dateutil) · demo-event-full.ics (every VEVENT property) |

### src/ top level
| file | purpose · key items |
|---|---|
| main.rs | `mod` list, `build_router(_with)`, `content_security_policy`, `serve` (wait_for_rustical, db::open/migrate, AppState, spawn loops, graceful shutdown), CLI (`serve`, `check-rustical-image`, `gen-vapid`, `--version`), mimalloc global allocator |
| state.rs | `AppState` (config, db, stores, i18n, changes, caches, history lock, vapid, guards), `Caches`, `Guards` (shutdown, per-user prefs lock, LoginThrottle) |
| config.rs | `Config::from_env` (`PRACTICAL__*`) |
| db.rs | `open`, `Watch` (PRAGMA data_version stamp per own connection: (generation, version)), `MIGRATIONS`/`migrate`, `check_schema` |
| auth.rs | login = RustiCal's accounts (OIDC: own client at RustiCal's IdP, callback only looks a principal up, never creates one; optional password: dummy argon2 for unknown names, `LoginThrottle`), `require_user`, logout (+ IdP `LOGOUT_URL`: Authelia has no end_session_endpoint), `safe_redirect`, `LOGIN_CSRF` cookie |
| sessions.rs | `DbStore` (practical_sessions: sha256(id), JSON, expiry; write-through memory cache), `activity` (page views extend expiry ≤1/`REFRESH_AFTER`=60 s), `is_prefetch`, `purge` |
| csrf.rs | `origin_guard` (Sec-Fetch-Site/Origin same-origin unless HTTP Basic), per-session token: `Token`, `TokenOrBasic`, `check` |
| ctx.rs | `PageCtx` (lang = stored pref else Accept-Language; user; csrf; flash), `localize_errors`, `set_flash` |
| extract.rs | `Json`/`Form`/`Query`/`Path` + field validators (`day`, `year`, `text`, `color`, `tz`) |
| error.rs | `AppError` → status + `Msg` (`err-*` key), `catch_panic` |
| etag.rs | `revalidate`: weak ETag + 304 for pages/fragments/JSON, brotli (one shot, window sized to body) |
| render.rs + render/{month,week,day,text}.rs | view HTML (L = labels, `script_json` escapes JSON in `<script>`, `safe_href` no javascript:/data:) |
| i18n.rs (+ i18n/pinned.rs, plurals.txt golden) | `LANGS`/`NATIVE`/`LOCALE`, `I18n` (fallback en → de), `negotiate`, `plural_forms`/`plural_json`. New language: .ftl + these 3 + `LANGS` in tools/gen_regions.py |
| bundle.rs | `BUNDLES` = asset part order (source of truth for build.rs and dev mode), `read` |
| css_min.rs | CSS minifier (build.rs `#[path]`; crate copy for tests only) |
| clock.rs | `now()`; `PRACTICAL__DEV_NOW` fixes it (dev only) |
| tasks.rs | background loops (table below), `guarded` (panic in a round logged, loop lives) |
| testkit.rs, it/*.rs | test router on a migrated temp RustiCal DB (`it/seed.rs` synthetic world); it/: api_events, api_more, assets, auth, caches, history, http, pages, poison (tier-0 chaos corpus), sessions, settings, units, validators |

### src/routes/ (each `router()` merged in main.rs)
| file | routes · key items |
|---|---|
| calendar.rs | /calendar (+`?fragment=1`, `?rule=<name>&back=`), /api/{events (window), days, weeks, density}; `ViewCtx`, `PageView`, `render_view` |
| more.rs | "Mehr" views (Jahresplaner, Freie Wochenenden, Urlaubsplaner, Countdown-Wand, Jahr in Zahlen); `YearCache` |
| tasks.rs | /tasks (JSON island `#tasks-data`, app.js draws 6 views); lookback cookie `tk_back` (URL `back=` wins) |
| editor.rs | /editor?kind=event\|task: editor.html + islands alone (editor_fragment.html, `#ed-host`), loaded by the settings' categories; `islands`/`calendars` = what every editor page feeds the dialog; a taken flash goes back |
| contacts.rs | /contacts (letter panels / sheet) |
| history.rs | /history, /api/history(/{id}/plan,/undo); `HistoryCache` |
| settings.rs + settings/{group,collection,token,pref}_forms.rs, transfer.rs | /settings/* POST forms → `finish`/`form` envelope (redirect + flash); export/import |
| api.rs + api/{event,category,contact,task,search,push,geo}_api.rs | /api/changes (SSE), `create_object`/`delete_object` envelopes, JSON CRUD; body limit 8 MiB; `MentionCache` |
| assets.rs | /assets/{name}: embedded br/gzip/identity by Accept-Encoding; `?v=` immutable 1 y, else ETag revalidate daily; dev: from disk, bundles joined |
| dev.rs, pages.rs | /dev/reload SSE (dev only), / redirect |

### src/domain/ (no HTTP types; `<mod>/pinned.rs` = pinned behaviour, `<mod>/tests.rs` = unit tests)
| module | purpose · key items |
|---|---|
| events.rs + events/{detail,props,alarms,ics,write} | `Instance` expansion (caldata rrule; oracle test `recurrence_matches_dateutil_oracle`), `InstanceCache`, `DensityCache`, `ObjMetaCache` (detail.rs); ics.rs `MANAGED` split: unmodelled lines/components copied verbatim, VALARM keeps position, RECURRENCE-ID overrides untouched (test `unchanged_save_keeps_every_property`); props.rs `EMBED_FILE_MAX` 2 MiB/file, 6 MiB/save |
| tasks.rs + tasks/{write,reminders} | `Task`, `TaskDetail` (shaped like EventDetail: shared editor cards), `TaskCache`, `shown_status` (no STATUS → open, or done with COMPLETED) |
| contacts.rs + contacts/{vcard,write} | `Contact`, `ContactCache`, `matches` (same e-mail, else family name + given-name prefix; rooms never), `yearly_in` (29 Feb → 28 Feb); vcard.rs `raw_lines` (params from raw text: caldata keeps only the first), `preserved_lines`, `bare_params_as_type`; write = vCard 3.0 |
| collections.rs | calendars/address books of user + groups, `owners`, CRUD, trash/restore, `delete_object` |
| groups.rs | groups = RustiCal `group.<slug>` principals owned by their members (any member adds/removes members, creates the group's collections, deletes it); trash `practical_trashed_groups` + RustiCal soft delete; last member leaving trashes; `purge_expired` |
| tokens.rs | app tokens as RustiCal's frontend (64 chars, pbkdf2, shown once `<id>_<secret>`) |
| prefs.rs | `practical_user_settings` (`get`/`set` under `Guards::prefs`), `VacationRule` `{"v":2,kind,value}`: category, calendar or title regex (Rust `regex`, ≤ 200 chars, no look-around; the page tests it live via /api/titles); legacy without v = category OR word |
| catrules.rs | `practical_category_rules`: per category template ("Vorlage") or lock ("Vorgabe", UI only); lock > template, upper category wins; applied client-side (js/editor/categories.js) |
| colors.rs | every colour in use (`/api/colors`, one scan), `with_color` (master component only), `category_objects`, `apply` (batch) |
| search.rs + search/{query,score,index,rank} | matching/tiers/floor/ranking spec in search.rs `//!`; `IndexCache` (per user, synctoken fingerprint, dropped after 30 min idle); rank.rs `suggest` (title templates) |
| search_history.rs | `practical_search_history` (kept on result open/Enter; retention `search_history_days`, ≤300) |
| history.rs + history/{ingest,read,undo}, history_text.rs + show.rs | own version store (`practical_history_{heads,blobs,cursors,…}`), `Hist` lock, `begin`/`Guard::commit`, `poll`; undo.rs `plan`/`undo`, `PlanMemo`; attributes/diff/revert, `Show` (readable values) |
| holidays.rs + holidays/{names,cldr,cache,fetch,regions} | source chain + region choice spec in holidays.rs `//!`; `MemCache`; names.rs packed CLDR tables |
| push.rs + push/guard.rs | reminder loop: `Plan` keyed by `db::Watch` stamp + each user's day, `tick`, sent-log `practical_alarms_sent`, 404/410 drop the subscription; guard.rs allowed push hosts, per-subscription isolation |
| transfer.rs | .ics/.vcf export (caldata generator) and import (`complete_ics` adds PRODID/VERSION/UID/DTSTAMP; `IMPORT_MAX_*` 8 MiB, 10 000 objects; new-collection routes 32 MiB body) |
| geo.rs | Photon (search-as-you-type) / Nominatim (reverse only: its policy forbids autocomplete, demands a UA) proxy, 1 req/s per provider, answers cached 7 d |
| ical.rs | fold/unfold, `assemble`, `split_line`, `rrule_canon` (lower-case RRULE), text/list/local helpers |

## Background loops (spawned in main.rs `serve`; every round via `tasks::guarded`)
| loop | period | work |
|---|---|---|
| change poller (tasks.rs) | 2 s (`PRACTICAL__DEV_POLL_MS`, dev) | `db::Watch` stamp; only after a commit by another connection: `history::poll` (ingest) then calendar/address-book synctokens → `ChangeEvent` |
| reminder loop (domain/push.rs) | 60 s from start, first tick at once; VAPID only | rebuild `Plan` only when the stamp moved, a user's day changed or a retry is due (backoff 1…16 min); fire window [now−2 min, now+1 min] |
| purger (tasks.rs) | 1 h, first at start | history past `HISTORY_RETENTION_DAYS`, trashed groups past `TRASH_RETENTION_DAYS`, push sent-log > 30 d, expired sessions |
| region upkeep (holidays::maintain) | 24 h; first round reads saved state only | coverage + CLDR release check weekly (ETag, usually 304) |
| dev asset watcher | 300 ms, dev only | mtimes of assets/** → `dev_reload` → /dev/reload |
Idle (bench statement log, 1 SSE client + 1 push subscriber, 60 s): 31 × `PRAGMA data_version` (poller every 2 s + the reminder tick's), nothing else.

## Caches (all in `AppState`; a write is never answered from before it)
| cache | key | invalidation / bound |
|---|---|---|
| events::InstanceCache | (principal, cal, synctoken, tz, from, to) exact window | LRU > 4 MiB; entry > ¼ not kept |
| events::DensityCache | (principal, cal) → (synctoken, tz, year) | ≤ 6 per cal |
| events::ObjMetaCache, tasks::TaskCache, contacts::ContactCache | (principal, collection) | caught up via store `sync_changes` on a moved synctoken |
| api::MentionCache | user → (tz, today, [(principal, cal, synctoken, has VEVENT)]) | one per user |
| more::YearCache | (principal, cal, synctoken, tz, year) | LRU > 6 MiB |
| history::HistoryCache | user → key of everything the document is made of | LRU > 6 MiB |
| search::IndexCache | user → fingerprint of the user's collections' synctokens | rebuilt in part on change; dropped 30 min idle |
| holidays::MemCache | (country, subdivision, year, kind-lang) | `cache_put` drops key; ≤ 512 keys; freshness by `fetched_at` |
| sessions::DbStore memory | session id | write-through; expired dropped on writes ≤ 1 / 10 min |
| domain::geo::Geo | request string → places | 7 d; throttle 1 req/s per provider |
| history::undo::PlanMemo | — | one request |
Test: it/caches.rs (mutate, read back = uncached answer).

## Assets (shipped bytes = build.rs OUT_DIR)
Bundle order source of truth: `build/src/bundle.rs` `BUNDLES` (order = JS run order / CSS cascade). app.js = js/core.js,
prefetch.js, req.js, IIFE(shell/base, settings-pages, popovers-stream), IIFE(colors/picker, fields, optimise), calendar.js,
`practical.editorInit = function` (editor/setup … wiring, 18 parts; run at load, again after GET /editor), search.js, contacts.js, tasks.js, settings.js. style.css = css/base, frame, grid,
chrome, dialogs, generic, views, icon-buttons, card-form, reminders, recurrence, editor-cards, links-location, tasks,
settings, color-picker, color-optimiser. A Wrap part is a fragment of one function scope (no script alone).
Client contract: `window.practical` (core.js: `$ $$ csrf esc escN escQ pad ymd parseYmd sprite ico keepCals send
placeUnder noteRe noteHtml wheelTouch wheelAxis wheelPages swipeViews slide`; req.js: `req tell progress online keep authLost stash unstash guard writer copy`;
prefetch.js: `prefetch prefetched intent intend stream`; shell: `fitPop leaving`; editor: `editorInit`), `window.{ask,askYesNo,toast,colorPicker,
plural,tr,trs,MONTHS,WEEKDAYS,rusticalCal,rusticalEd,rusticalTasks,rusticalContacts}`; strings `window.I18N` from
/assets/i18n-<lang>.js (placeholders `%name`, plurals via `Intl.PluralRules`). history.js loads after app.js.
No inline script anywhere (CSP `script-src 'self'`); `?v=<hash>` URLs immutable.

## Features → code (non-obvious rules; the spec lives in the module docs named)
| feature | where | rules not evident from the code |
|---|---|---|
| views | render/, routes/calendar.rs, js/calendar.js | server-rendered; client swaps `?fragment=1`, refetches on SSE; `?week=<Monday>`/`?month=YYYY-MM` scroll; `date` shared by all views; tasks never show in the calendar (VEVENTs only); app frame = viewport, `.main` scrolls, toolbar sticky, scrollbars hidden |
| view swipe | core.js `swipeViews` (+ `wheelAxis`/`wheelPages`), calendar/tasks/contacts/history.js | sideways wheel touch or touch swipe = click on the neighbouring view button (calendar: day…year only, no wrap); axis per touch (x iff > 2× vertical after 12 px); a horizontal scroller that can still move takes the whole touch; e2e functional/swipe.spec.mjs |
| view slide | core.js `slide`, frame.css `pc-view` | every switch between a page's views (button, swipe, calendar back/forward among day…year) = same-document view transition of `.main` only (root not captured; its toolbar with the view switch = `pc-bar`, shown at once), transform-only 240 ms; none without the API or with reduced motion (all e2e suites); cost: e2e/perf/slide.spec.mjs |
| event editor | templates/editor.html, js/editor/*, domain/events/* | view mode behind a lock; cards only where data is; generated instances get the master's VALARMs (caldata builds none) |
| status card | js/editor/status.js | TRANSP/CLASS defaults (OPAQUE/PUBLIC) shown as "default", unset stays unset; PRIORITY outside 1/5/9 keeps its number |
| links/attachments | js/editor/links.js, events/props.rs | conference hosts → join card; RELATED-TO resolved to title+date; FMTTYPE/FILENAME/SIZE/FEATURE modelled, other params ride along |
| note | js/editor/note.js, core.js `noteHtml` | plain DESCRIPTION; `@`contact `#`category `!`event mentions (`/api/mentions`); changed DESCRIPTION drops X-ALT-DESC + STYLED-DESCRIPTION |
| people | js/editor/people.js, contacts::matches | display only: RustiCal does no scheduling (RFC 6638); every parameter saved back unchanged |
| reminders | js/editor/reminders.js, events/alarms.rs | presets per user (`alarm_presets_json`, /api/alarm-presets; built-in list until changed); PROXIMITY/ACKNOWLEDGED kept as written |
| recurrence | js/editor/{recurrence,strip}.js | client preview only, caldata authoritative; rules the strip cannot paint (BYSETPOS, several ordinals, BYHOUR…) saved as typed |
| category rules | catrules.rs, js/editor/{categories,rule-editor}.js, routes/editor.rs | locks hold in this UI only; rule editor from an event stacks over it; on /settings#categories over the page: dialog of the scope's kind loaded on first use (GET /editor, `practical.editorInit`, `rusticalEd.rule`), the other kind's removed hidden; list redrawn via `rusticalHost.rulesChanged`; links keep the deep link `/calendar\|/tasks?rule=<name>&back=` |
| colours | colors.rs, js/colors/* | a picked colour becomes a source; category colour = its rule's `when.color.value`; optimiser constants/levels in optimise.js; calendars, holidays, user-picked colours never move |
| search | domain/search/*, js/search.js | client debounce 180 ms, aborts stale, keeps answers 30 s; drop-down ≤5/kind ≤10, dialog ≤200/kind; prefixes `termin:`/`event:`, `kontakt:`/`contact:`, `aufgabe:`/`task:` (+fr/es) |
| title templates | search/rank.rs `suggest`, js/editor/templates.js | new events only; ≥2 letters, 200 ms; ≤3; date stays the dialog's; UNTIL/EXDATE/RDATE/PARTSTAT/ACKNOWLEDGED not copied |
| change history | domain/history*, routes/history.rs, history.js | DAV change = one per object, device = PRODID of the new version; delete+create of one UID = move; undo = a change (`reverts`; undone gets `undone_by`); undo of a create deletes and marks later changes `inactive_by`; 3-way warning when a value moved since; delete restores the stored version (also after RustiCal purge) |
| contacts | routes/contacts.rs, js/contacts.js, contacts/* | sort client-side (`localStorage.ctSort`/`ctDesc`); colours `practical_addressbook_colors`; fields `contact_list_fields_json` |
| tasks | routes/tasks.rs, js/tasks.js, domain/tasks* | minimal VTODO; unmodelled lines (recurrence, attendees, X-) survive |
| holidays | domain/holidays* | first public region = the user's own (vacation/bridge days count by it) |
| import/export | domain/transfer.rs, settings/transfer.rs | .ics merged by UID; new collection prefilled from X-WR-CALNAME/-CALDESC/-CALCOLOR/-TIMEZONE (+X-APPLE-CALENDAR-COLOR), X-WR-* dropped from objects; `.vcf` without UID gets one |
| push | domain/push*, assets/sw.js, js/settings.js | logout ends this browser's subscription |
| addresses/map | domain/geo.rs, js/editor/{location,map}.js | browser asks after a 2 s typing pause or on a button; tiles tile.openstreetmap.org with attribution |
| i18n | i18n.rs, locales/ | missing key → en → de; month/weekday names + date formats from `Intl` |
| dev mode | mise `dev`, routes/{assets,dev}.rs, clock.rs | `PRACTICAL__DEV_ASSETS_DIR` (assets from disk, /dev/reload), `DEV_LOGIN`, `DEV_NOW` (only with DEV_ASSETS_DIR), `DEV_POLL_MS`; all unset in production |

## e2e/ (Playwright + harness; README "Local development" has the commands)
| path | purpose |
|---|---|
| cargo.sh, build-server.sh | per-checkout cargo (see AGENTS.md); server build in E2E_PROFILE (default `e2e`) + copy of the embedded assets |
| gate.sh | quick / full gate |
| seed/ | fixture DB: 01-rustical.sh (RustiCal CLI in docker), 02-practical.py (HTTP, frozen clock); users alice/bob/carol (pw `devpass`; carol empty), group `group.familie`, fixed UUIDs `5eed0000-…` |
| lib/ | axe.mjs (axe-core in the page, finding keys, allow-list; `E2E_AXE=1` + visual suite → `<state>.axe.json` audit), env.mjs (NOW = seed FROZEN), servers.mjs (dev server: DEV_LOGIN=alice, DEV_NOW=2026-10-14T08:00:00Z, DEV_POLL_MS=250, egress blocked; + non-dev login server on port+1), test.mjs (`test`, `settle()`), capture.mjs (PNG + DOM + style digest), visual.mjs, theme-cover.mjs, timing.mjs (`E2E_TIMING=1`) |
| functional/, visual/, chaos/, perf/, a11y/ | suites (`E2E_SUITE`=visual/chaos/perf/a11y, else functional); `_editor.mjs`/`_set.mjs`/`_axe.mjs` = helpers; a11y = axe-cal/axe-pages (axe states) + keys (keyboard flows, desktop light only) |
| a11y-check.mjs (+ a11y-allow.txt: key = contrast colour pair or target without positions/values; only shrinks) | after the a11y suite: NEW / STALE allow-list lines → exit 1 |
| compare.mjs, theme-cover.mjs (+ theme-cover-allow.txt: backlog only shrinks), baseline.sh | visual diff (pixels, DOM, computed style), dark coverage, baseline capture |
| bench/ | bench.mjs (driver, RULES), client.mjs (UI interactions), gen-db.py (big synthetic DB), proc.py (CPU/RSS/wakeups/instructions per PID) |

Visual projects: desktop 1440×900 + phone 390×844 × light/dark (de) + `@main` states in en (desktop/phone light). Browser
frozen at NOW, Europe/Berlin, reduced motion, `Math.random` seeded, service workers blocked, no request leaves
127.0.0.1. Pixel baselines depend on host fonts + Chromium build: compare only baselines of one machine and fixture.

### bench (e2e/bench/bench.mjs)
DB: fixture + alice 3,500 events (60 series), 150 tasks, 300 contacts, ~450 history changes, cached
`~/.cache/practical-e2e/bench-db-<sha of gen-db.py + fixture>.sqlite3`. Servers: production mode, password login,
real clock, retention 100 y, egress blocked, fresh DB copy per phase. Metrics: `srv.lat|cpu|inst.<ep>` (11 endpoints),
`srv.first.*`, `srv.sql.<ep>`, `srv.rss.*`/`srv.peak.burst`, `idle.cpu|inst|wakeups` (60 s, 1 SSE client, VAPID + 1 push
subscriber; login on a start of its own before the measured one), `wire.*`, `dom.*`, `ui.<action>(.cpu4|.busy|.reqs)`,
`size.binary`, `size.src.*`. Verdict: ref and cand alternate per round/sample in one session; alarm ⇔ median per-round
ratio > 1+rel AND median Δ > abs (`RULES`); stored numbers are information. `size.binary` is judged only between two
builds of one non-e2e profile (`E2E_PROFILE=release` against a release-profile ref); on `e2e` (256 codegen units,
module-partitioned) it is information. Power state (pmset) recorded per phase; a change → exit 4.
Calibration 2026-10-07 (901d09e vs itself, M4 Pro, Low Power Mode, host shared with other agents): idle.* ratios
0.93-1.06 in 5 runs (idle.inst 4.8-6.2 Minstr/min; before the separate login start 9.7-12.2 at up to ±103 % noise);
size.binary release vs release 1.000. p90 latency is judged per round (median of the rounds' p90 ratios): pooled,
single 20-75 ms host-load samples in one round alarmed (contacts ×1.37, history ×1.75). ui.editor-open is bimodal
(~9 or ~23 ms per sample, busy time stable): its abs floor is 20 ms (alarmed at ×2.26/Δ 13.9 and ×2.40/Δ 11.9 on
identical code). With these rules 2 consecutive clean runs at load avg 6.3 and 20.5 (start of run); max p90 ratio
1.05. Injected and caught: 20 ms sleep in the tasks handler → srv.lat.tasks.p50 ×11.6, p90 ×11.0; 50 ms busy loop
on every click → 12 ui.* metrics (editor-open ×6.6, task-toggle ×2.6, view-week ×2.3 …); poller every 100 ms
without the `db::Watch` gate → idle.cpu ×95, idle.inst ×188, idle.wakeups ×16; a 300 KB static → release
size.binary ×1.013 (threshold ×1.01: smallest catch ≈ 226 KB), e2e ×1.010 reported only. Earlier (befd301):
app.js + 5 KB → wire ×1.054; 30 MB per burst → srv.rss.burst ×1.12.
Runtime: bench ~2 min, bench-compare ~3 min + build; env BENCH_ROUNDS, BENCH_UI_SAMPLES, BENCH_IDLE_S, BENCH_PHASES
(server,count,idle,client), BENCH_SKIP_BUILD=1.
