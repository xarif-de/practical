# Performance and efficiency pass, 2026-10 (0.11.1 → 0.12.0)

Goal: faster, smaller, quieter — with **no functional and no visual change**. Every number below is
measured (method in brackets); "local" = macOS arm64 release build on the synthetic big DB
(3,920 calendar objects, 300 contacts, 1,648 history changes), "container" = arm64 musl image
limited to 1 CPU / 192 MiB like the pod.

## Safety net (built first)

| layer | what | run |
|---|---|---|
| Rust | 60 → 210 tests: domain characterisation tests, golden HTML (`build/tests/snapshots`, `UPDATE_SNAPSHOTS=1`), in-process router tests on a RustiCal-migrated temp DB (`src/testkit.rs`, `src/it/`), cache invalidation tests | `mise run test` |
| e2e | 99 Playwright tests, every action of the UI (two-user live update, password login, instant updates) | `mise run e2e-seed`, `mise run e2e` |
| visual | 964 UI states × desktop/phone × light/dark (+ en) = 1132 screenshots, each with a normalised DOM dump and a 58-property computed-style digest; `compare.mjs` = pixel threshold 0 | `mise run e2e-baseline -- <dir>`, `mise run e2e-compare -- <a> <b>` |
| clock | `PRACTICAL__DEV_NOW` freezes server time (dev mode only) | — |

Proof of the pass: 1132/1132 states identical to the 0.11.1 baseline; 38 pages/APIs × 15 mutation
stages byte-identical between e74c076 and 79fedc3 apart from the asset `?v=` hash.
CI runs neither `cargo test` nor e2e — both are local gates.

## Results

Production (container `practical`, VictoriaMetrics, 60 min after the 0.12.0 rollout vs. the 6 h
before it and the same hour one day earlier): CPU **6.0 → 0.38 mcores** (−94 %), working set
**22.6 → 15.6 MiB** (max 22.0 in the first hour, incl. start-up), no panics.


| area | before | after | method |
|---|---|---|---|
| idle CPU, no user | 0.667 mcores | 0.067 mcores | local, `ps` CPU time, 3 × 300 s median |
| idle CPU, 1 push subscriber + 1 SSE tab | 1.733 mcores | 0.300 mcores | same |
| SQL statements / min idle | 188 | ~31 | sqlite trace |
| `/calendar` warm | 97 ms | 1.9 ms | local, median of 3 × 10 |
| week / day view warm | 104 / 98 ms | 3.5 / 4.3 ms | same |
| `/settings` warm | 62 ms | 2.3 ms | same |
| `/history` warm | 255 ms | 89 ms | same |
| `/api/mentions` warm | 144 ms | 2.9 ms | same |
| month view, cold cache | 109.5 ms | 78 ms | container, mimalloc vs musl malloc |
| app.js + style.css on the wire | 159 KB gzip, per-request compression | 90 KB brotli, precompressed, `immutable` | built output / prod curl |
| asset CPU per app.js request | 10.3 ms | 0.9 ms | container |
| style.css source | 175 KB, 1712 rules | 154 KB, 1398 rules | `wc -c`, CSS coverage over 964 states |
| binary | 26.6 MB | 21.2 MB | container build |
| task toggle / save (UI) | 138 / 145 ms | 33 / 51 ms | Playwright click → settled DOM, median 10 × 3 |
| 50 editor keystrokes (main thread) | 1472 ms | 44 ms | same |
| live heap with all caches warm | — | +5.5 MiB | local heap diff |

## What changed

- Idle: change poller gated by `PRAGMA data_version` (1 cheap check per 2 s, full round only after a
  foreign write); reminder plan cached and recomputed on writes or day change; reminders from the
  synctoken-keyed task cache; 30-day push cleanup in the hourly purger; every background loop
  survives a panic in one round (`tasks::guarded`); explicit column lists instead of `SELECT *`
  (a live `DROP COLUMN` had killed the push loop on 2026-10-05); expired sessions purged during writes.
- Requests: synctoken-keyed caches (instances per exact window, picker density, object summaries,
  parsed contacts, note-editor event list), prefs once per request, holidays in memory, `js-*`
  JSON once per language, history page linear instead of O(n²), escaping while writing.
- Assets: oxc JS minifier + `src/css_min.rs` at build time, gzip-9 + brotli-11 embedded, chosen by
  `Accept-Encoding`; `?v=` URLs immutable for a year; `/assets` and `/healthz` before the
  session/auth/CSRF/compression layers; mimalloc (`MIMALLOC_PURGE_DELAY=0`,
  `MIMALLOC_ARENA_EAGER_COMMIT=0`), `codegen-units = 1`; no `ca-certificates` package (in base).
- Client: shared helpers (`window.practical`: `$`, `send`, `esc*`, `ico`, `placeUnder`, …), SSE
  closed 15 s after a tab hides and reopened with one refresh, exponential reconnect backoff,
  identical re-renders skipped, tasks updated in place, editor refresh batched per frame.
- UI code: askama macros (`templates/macros.html`), dead and overridden CSS removed, identical
  components share rules, `--ok`/`--warn`/`--err` defined with the values already in effect.
- Security (found on the way, shipped first): JSON blocks in `<script>` escaped
  (`render::script_json`), event titles/colours/user agents escaped in JS sinks, no `javascript:`/
  `data:` links from event data (`render::safe_href`, `lkSafe`), rule `?back=` and login redirect
  restricted to this site, calendar colours only `#rrggbb`.

## Deliberately not done

- Icon sprite / i18n JSON as separate cached files: first paint would show empty icons.
- Optimistic task state, thin/fat LTO (no gain for the build time), compressing regions.json (no gain
  on the compressed image).
- Prefetch of full-page links (calendar/contacts/tasks/settings): a document navigation can only reuse
  a prefetch through the HTTP cache, which the change stream cannot invalidate (stale page after a
  change; a form POST's redirect target would come from the cache without its flash).

## Round 2 (session expiry, prefetch on intent)

- Session expiry explicit (`sessions::activity`): requests with a page context extend the 12 h
  inactivity window as before, by an expiry-only store write at most once per 60 s (it never
  overwrites a concurrent write, never resurrects a logged-out session); the flash is removed only
  when present. Set-Cookie + store writes in a scripted minute of calendar use: 9 → 0 (≤ 1/min).
- Calendar fragments prefetched on intent (hover 65 ms, focus, press; the month grid under the
  pointer = the wheel's neighbours), used once by the click, dropped on any live change, own write
  (any tab) or closed stream; none on Save-Data. The server answers `Purpose: prefetch` with 204
  while a flash/new app token is pending and never writes session data for it. Click → settled DOM
  with the pointer rested (Playwright, median 10 × 3): 17–29 → 10–20 ms local, 61–64 → 10–19 ms at
  40 ms RTT. Requests in the scripted minute: 9 → 14 (all prefetches, 9 of 9 navigations served).

## Known bugs pinned by tests, not fixed (each changes visible behaviour — needs a decision)

1. Day view count is not pluralised ("1 Termine") — `render/pinned.rs`.
2. Countdown: 29 Feb birthday in a non-leap year shows negative days — `routes/more/pinned.rs`.
3. "Year in numbers" spark bars get shifted arguments (`--h:KW 1%`) — `routes/more/pinned.rs`.
4. Lower-case RRULE values ignored by `rrule_text`; `normalize_until` misses `until=` — `render/pinned.rs`, `domain/events/pinned.rs`.
5. `base64_len("==")` underflows (debug-build panic) — `domain/events/pinned.rs`.
6. History shows `⟦hx-a-…⟧` for unknown X- properties — `domain/history_text/pinned.rs`.
7. `initials()` counts bytes ("Ölaf" → "Ö") — `it/units.rs`.
8. Settings JSON shows the store's English "Not found" — `it/settings.rs`.
9. Session language beats the stored preference (contrary to ctx.rs); "automatic" keeps the old language — `it/settings.rs`.
10. vCard with a bare parameter (`TEL;CELL`) is rejected, the contact skipped.
11. `.ics` import without PRODID reports success but its events never appear (e2e F4).
12. Tasks without STATUS appear in no board column.
13. `T.newTask` read but never defined (app.js).
14. Contact display name renders at 15 px, the intended rule (20 px) loses on specificity.
15. Login error box (`.flash.error`) has no style → blue info tint.
16. Phone: history filter popover off-screen until a resize.
17. Notes: a URL containing an e-mail address or `#tag` renders broken markup.
18. CSS rules that never applied: phone `.ledger { max-height: none }`, phone `.more` hiding,
    `.ed .dayline` height, `.ct-x` grid placement.
19. No Content-Security-Policy (would need the inline theme script hashed or moved).
