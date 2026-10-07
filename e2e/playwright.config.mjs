// PractiCal e2e. Projects:
//   functional            — e2e/functional, desktop, de; spec files in parallel (E2E_WORKERS, default 4), the tests of
//                           a file in order on one worker, each file on fresh servers + DB copies (lib/test.mjs)
//   visual-<vp>-<scheme>  — e2e/visual, every surface; de; desktop 1440×900 / phone 390×844 × light / dark
//   visual-en-*           — the @main states again in en (desktop light, phone light)
//   chaos                 — e2e/chaos, the seeded UI monkey with network chaos (Tier 1), one worker, desktop, de
//   a11y-<vp>-<scheme>    — e2e/a11y: axe over fixed states (desktop light + dark, phone light) and the keyboard
//                           flows (desktop light); each file on fresh servers like the functional suite
// E2E_SUITE=visual selects the visual projects, E2E_SUITE=chaos the chaos one, E2E_SUITE=a11y the a11y ones, otherwise
// the functional one runs.
// global-setup.mjs builds the server and starts the visual/perf suites' shared pair (the functional workers start
// their own); artifacts of the visual suite go to E2E_OUT.
import { defineConfig } from '@playwright/test';
import { BASE, FIRST_PORT, TZ } from './lib/env.mjs';

const desktop = { viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1, isMobile: false, hasTouch: false };
const phone = { viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, isMobile: true, hasTouch: true };
// software raster, whole tiles: GPU/partial raster antialiased rounded corners differently between runs
// (16–90 px per state, DOM + style identical; with these flags 0 differences over 3 repeat runs)
const launchOptions = { args: ['--disable-gpu', '--disable-partial-raster'] };
const common = { baseURL: BASE, timezoneId: TZ, reducedMotion: 'reduce', serviceWorkers: 'block', browserName: 'chromium', permissions: ['clipboard-read', 'clipboard-write'], launchOptions };

const visual = [];
for (const [vp, dev] of [['desktop', desktop], ['phone', phone]])
  for (const scheme of ['light', 'dark'])
    visual.push({ name: `visual-${vp}-${scheme}`, testDir: './visual', use: { ...common, ...dev, colorScheme: scheme, locale: 'de-DE' } });
// no en-desktop-dark: its checks = en text (en-desktop-light) × dark colours (desktop-dark); en and de share the element
// skeleton and no CSS depends on the language — 0 of 45 mutants were caught by it alone (mutation study, 2026-10-06)
for (const [vp, dev, scheme] of [['desktop', desktop, 'light'], ['phone', phone, 'light']])
  visual.push({ name: `visual-en-${vp}-${scheme}`, testDir: './visual', grep: /@main/, use: { ...common, ...dev, colorScheme: scheme, locale: 'en-GB' } });

// no phone-dark: its contrast pairs are desktop-dark's (one palette per scheme), its layout phone-light's — over the
// full visual suite (E2E_AXE=1, 2026-10-07) it showed no finding the three projects here do not
const a11y = [['desktop', desktop, 'light'], ['desktop', desktop, 'dark'], ['phone', phone, 'light']].map(([vp, dev, scheme]) =>
  ({ name: `a11y-${vp}-${scheme}`, testDir: './a11y', testIgnore: vp === 'desktop' && scheme === 'light' ? undefined : /keys\.spec/, use: { ...common, ...dev, colorScheme: scheme, locale: 'de-DE' } }));

export default defineConfig({
  globalSetup: './global-setup.mjs',
  globalTeardown: './global-teardown.mjs',
  timeout: 60_000,
  expect: { timeout: 10_000 },
  retries: 0, // a flaky test is fixed, never retried away
  forbidOnly: true,
  reporter: [['list'], ['json', { outputFile: `.work/last-run-${FIRST_PORT}.json` }]],
  outputDir: `.work/test-results-${FIRST_PORT}`,
  workers: Number(process.env.E2E_WORKERS || 4),
  // one suite per run: the visual suite's workers share one server (read-only), the functional suite's write to their own
  // perf: interaction latency (e2e/perf), opt-in, never part of the gate
  projects: process.env.E2E_SUITE === 'chaos' ? [{ name: 'chaos', testDir: './chaos', fullyParallel: false, workers: 1, use: { ...common, ...desktop, colorScheme: 'light', locale: 'de-DE' } }]
    : process.env.E2E_SUITE === 'perf' ? [{ name: 'perf', testDir: './perf', fullyParallel: false, workers: 1, timeout: 900_000, use: { ...common, ...desktop, colorScheme: 'light', locale: 'de-DE' } }]
    : process.env.E2E_SUITE === 'visual' ? visual
    : process.env.E2E_SUITE === 'a11y' ? a11y : [
    { name: 'functional', testDir: './functional', fullyParallel: false, use: { ...common, ...desktop, colorScheme: 'light', locale: 'de-DE' } },
  ],
});
