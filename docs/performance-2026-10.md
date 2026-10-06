# Performance and efficiency pass, 2026-10 (0.11.1 → 0.12.0 → 0.13.0)

Goal: faster, smaller, quieter — round 1 (0.12.0) with **no functional and no visual change**; round 2
(0.13.0) = the items below the user decided one by one (visible changes allowed, each one explained
state by state). Every number is measured (method in brackets); "local" = macOS arm64 release build on
the synthetic big DB (3,920 calendar objects, 300 contacts, 1,648 history changes), "container" = arm64
musl image limited to 1 CPU / 192 MiB like the pod.

## Safety net (built first)

| layer | what | run |
|---|---|---|
| Rust | 60 → 233 tests: domain characterisation tests, golden HTML (`build/tests/snapshots`, `UPDATE_SNAPSHOTS=1`), in-process router tests on a RustiCal-migrated temp DB (`src/testkit.rs`, `src/it/`), cache invalidation tests | `mise run test` |
| e2e | 124 Playwright tests, every action of the UI (two-user live update, password login, instant updates, prefetch, CSP enforced: any violation fails the test) | `mise run e2e-seed`, `mise run e2e` |
| visual | 964 UI states × desktop/phone × light/dark (+ en) = 1134 screenshots, each with a normalised DOM dump and a 58-property computed-style digest; `compare.mjs` = pixel threshold 0 | `mise run e2e-baseline -- <dir>`, `mise run e2e-compare -- <a> <b>` |
| clock | `PRACTICAL__DEV_NOW` freezes server time (dev mode only) | — |

Proof of round 1: 1132/1132 states identical to the 0.11.1 baseline; 38 pages/APIs × 15 mutation
stages byte-identical between e74c076 and 79fedc3 apart from the asset `?v=` hash. Round 2: every
differing state mapped to its item. Determinism: 3 consecutive full runs on one binary = 0 differences
pairwise after two harness fixes (phone emulation's load-time `resize` re-dispatched after
DOMContentLoaded; icon sprite response held until the first paint — both in `e2e/lib`).
CI runs neither `cargo test` nor e2e — both are local gates.

## Results (round 1)

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

## What changed (round 1)

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

## Round 1 "deliberately not done" (N1–N6) → round 2 outcome

| id | item | outcome |
|---|---|---|
| N1 | session expiry / flash write per request | done 157b5ac: `sessions::activity` extends the 12 h inactivity window by an expiry-only store write ≤ 1/60 s (never overwrites a concurrent write, never resurrects a logged-out session); flash removed only when present. Set-Cookie + store writes per scripted minute of calendar use 9 → 0 |
| N2 | icon sprite + i18n JSON as cached files | done 93366a4: `/assets/icons.svg?v=` (`<meta name=icons>`), `/assets/i18n-<lang>.js`, `/assets/theme.js`, immutable. HTML gzip as sent: month 26.3 → 13.5 KB, tasks 33.2 → 20.1, contacts 55.4 → 42.1, settings 27.3 → 14.4, history 16.3 → 3.5, login 13.9 → 1.0; once: icons 6.4 KB br, i18n-de 4.7, theme 0.15. Warm navigation −13.3 KB; FCP ±4…12 ms (faster once bandwidth counts, slower on a bandwidth-free link/first visit). Cold Chromium: 2 frames of empty icons on the first visit (WebKit 1); `rel=preload` dropped (WebKit refetched it on every navigation) |
| N3 | optimistic task state | kept as is (user decision) |
| N4 | prefetch | done 65d6a5c, calendar fragments only: on intent (hover 65 ms, focus, press; month grid under the pointer = both wheel neighbours), Map ≤ 8, TTL 30 s, used once, dropped on any `rustical:change`, own non-GET fetch (any tab, storage signal) or closed stream; never on Save-Data; server answers `Purpose: prefetch` 204 while a flash/new token is pending. Click → settled DOM, pointer rested (median 10 × 3): today 17.3 → 9.6 ms local / 62.3 → 9.9 ms at 40 ms RTT, view switch 20–23 → 17–20 / 61–64 → 17–20, wheel month 27–29 → 16 / 63 → 16–19. Requests per scripted minute 9 → 14 (9/9 navigations from a prefetch). Full-page links stay unprefetched (HTTP-cache reuse cannot be invalidated by the change stream) |
| N5 | LTO / build profile | measured a99721d, no change: `v*` tags build `[profile.dist]` (fat LTO, cgu 1, O3): first requests Σ 646 → 620 ms (−4 %), warm 121 → 116 ms vs `release`, 20.8 MB; O2 (−1.3 MB), thin, Os (+11 % CPU), mimalloc v2 not better. panic stays unwind (`tasks::guarded`), no target-cpu (prod = amd64 Intel); PGO not tried |
| N6 | regions.json | done bf0ec6a + 839a00b: build.rs packs it into two gzip-9 tables (`domain/holidays/names.rs`, decoded on first use): embedded 566 KB → 4.9 + 83 KB; binary ≈ −0.48 MB (−296 KB packing, −182 KB gzip instead of brotli); heap 3.25 MB at once → countries 13 KB / 0.10 ms (calendar views), subdivisions 295 KB / 1.28 ms (pickers only); brotli decoder no runtime dependency. 1,097 region responses byte-identical (one name tie in subs-UG-fr: the old HashMap order varied run to run) |

## Known bugs pinned in round 1 (B1–B19) → outcome

1. Plurals ("1 Termine") — fixed 556077f, 56d4d45, 1496f8c: every count is a Fluent plural, js-/hxj- keys ship their forms (`{"$":var,"one":…,"other":…}`), app.js/history.js pick with `Intl.PluralRules`; hand-made `-1` keys merged.
2. 29 Feb birthday in a common year — fixed 556077f: shown on 28 Feb (`contacts::yearly_in`; Apple/Google behaviour; German law would give 1 March — one line to switch).
3. "Year in numbers" spark arguments — fixed 556077f.
4. Lower-case RRULE / `until=` — fixed 556077f centrally (`ical::rrule_canon`, `events::unfold`, `ical::split_line` param values, case-insensitive TZID/mailto).
5. `base64_len("==")` underflow — fixed 556077f.
6. `⟦hx-a-…⟧` for unknown X- properties — fixed 556077f: the property name as written.
7. `initials()` byte count — fixed 556077f.
8. Store's English "Not found" — fixed 556077f + 56d4d45: `AppError::user_text`, 45 `err-*` keys; app-token/Basic clients keep English.
9. Session language beats the stored preference — fixed 556077f: stored pref, else Accept-Language.
10. vCard bare parameter (`TEL;CELL`) — fixed 556077f (`contacts::bare_params_as_type` before every parse; store-hidden cards read from `addressobjects`); upstream https://github.com/lennart-k/caldata-rs/issues/12.
11. `.ics` without PRODID silently lost — fixed 556077f: `complete_ics` adds PRODID/VERSION/UID/DTSTAMP; the count is what RustiCal reads back.
12. Tasks without STATUS on no board column — fixed 64988b2 (`shown_status`: open, or done with a COMPLETED stamp; STATUS never written by a save) + 29716b7 (editor status tile shows "Offen").
13. `T.newTask` undefined — fixed 86d01eb.
14. Contact name 15 px — fixed 62af209: 20 px/600 like the event title.
15. Login error blue — fixed d0a7352 (12 % `--err` tint) + c448b2b (`role="alert"` on it, `#ed-error`, `#ct-error`).
16. Phone history filter popover off-screen — fixed 9a7b3b7 (`practical.fitPop` on mount + open).
17. Note links broken by e-mail/#tag inside a URL — fixed a408fe4 (`practical.noteHtml`, one pass, escaped once).
18. Four CSS rules that never applied (phone `.ledger` max-height, phone `.more` hiding, `.ed .dayline` 420 px, `.ct-x` grid placement) — pending user decision (before/after screenshots prepared).
19. No Content-Security-Policy — fixed 93366a4: every HTML response `default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: https://tile.openstreetmap.org; connect-src 'self'; worker-src 'self'; object-src 'none'; base-uri 'self'; form-action 'self' <issuer> <logout origin>; frame-ancestors 'none'` (`main.rs content_security_policy`); no inline script left (theme → theme.js, login auto-submit → `data-auto`); `data:` images for Leaflet's empty tile GIF.

## Release policy (2026-10-06)

- plan.xarif.de runs only release images: git tag `vX.Y.Z` → `[profile.dist]` build; kubernetes/rustical
  Renovate follows exact `vX.Y.Z` tags only, every other reference (`main`, `main-<sha8>`, `latest`, `vX.Y`) frozen.
- `main` publishes build versions (`release` profile, `:main` / `main-<sha8>`); not deployed. Until the first
  release tag the deployment stays on its last `main` digest; then both containers move to `vX.Y.Z@sha256:…` once by hand.

## Known limits (accepted)

- Lower-case `VALUE=date` objects stay invisible: caldata rejects them, RustiCal's store hides them before PractiCal sees them.
- Prefetch: preference changes made on ANOTHER device are not on the live stream; for ≤ 30 s (TTL) one prefetched navigation can render the old preference (same browser: storage signal drops it).
- Session: logout up to 60 s earlier than before (expiry refreshed ≤ 1/60 s).
