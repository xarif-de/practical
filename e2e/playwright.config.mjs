// PractiCal e2e. Projects:
//   functional            — e2e/functional, desktop, de; spec files in parallel (E2E_WORKERS, default 4), the tests of
//                           a file in order on one worker, each file on fresh servers + DB copies (lib/test.mjs)
//   visual-<vp>-<scheme>  — e2e/visual, every surface; de; desktop 1440×900 / phone 390×844 × light / dark
//   visual-en-*           — the @main states again in en
// E2E_SUITE=visual selects the visual projects, otherwise the functional one runs.
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
for (const [vp, dev, scheme] of [['desktop', desktop, 'light'], ['desktop', desktop, 'dark'], ['phone', phone, 'light']])
  visual.push({ name: `visual-en-${vp}-${scheme}`, testDir: './visual', grep: /@main/, use: { ...common, ...dev, colorScheme: scheme, locale: 'en-GB' } });

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
  projects: process.env.E2E_SUITE === 'perf' ? [{ name: 'perf', testDir: './perf', fullyParallel: false, workers: 1, timeout: 900_000, use: { ...common, ...desktop, colorScheme: 'light', locale: 'de-DE' } }]
    : process.env.E2E_SUITE === 'visual' ? visual : [
    { name: 'functional', testDir: './functional', fullyParallel: false, use: { ...common, ...desktop, colorScheme: 'light', locale: 'de-DE' } },
  ],
});
