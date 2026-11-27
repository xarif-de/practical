// PractiCal e2e. Projects:
//   functional            — e2e/functional, desktop, de; spec files in parallel (E2E_WORKERS, default 4), the tests of
//                           a file in order on one worker, each file on fresh servers + DB copies (lib/test.mjs)
//   visual-desktop, visual-phone — e2e/visual: the key screens against reference screenshots in the repo; de, light
//   chaos                 — e2e/chaos, the seeded UI monkey with network chaos (Tier 1), one worker, desktop, de
//   a11y-<vp>-<scheme>    — e2e/a11y: axe over fixed states (desktop light + dark, phone light) and the keyboard
//                           flows (desktop light); each file on fresh servers like the functional suite
// E2E_SUITE=visual selects the visual projects, E2E_SUITE=chaos the chaos one, E2E_SUITE=a11y the a11y ones, otherwise
// the functional one runs.
// global-setup.mjs builds the server and starts the visual/perf suites' shared pair (the functional workers start
// their own).
import { defineConfig } from '@playwright/test';
import { BASE, FIRST_PORT, TZ } from './lib/env.mjs';

const desktop = { viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1, isMobile: false, hasTouch: false };
const phone = { viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, isMobile: true, hasTouch: true };
// software raster, whole tiles: GPU/partial raster antialiased rounded corners differently between runs
// (16–90 px per state, DOM + style identical; with these flags 0 differences over 3 repeat runs). Linux (the VM, e2e/vm.sh)
// also without subpixel glyph positioning: at deviceScaleFactor 2 the same text line came out with 1-3 levels different
// edge pixels from run to run (two VM baselines: 61 of 1135 states, all phone; --font-render-hinting=none did not help;
// with this flag 0 of 550 phone PNGs over 3 runs and 0 of 1135 states between two full baselines, 2026-11-03)
const launchOptions = { args: ['--disable-gpu', '--disable-partial-raster', ...(process.platform === 'linux' ? ['--disable-font-subpixel-positioning'] : [])] };
const common = { baseURL: BASE, timezoneId: TZ, reducedMotion: 'reduce', serviceWorkers: 'block', browserName: 'chromium', permissions: ['clipboard-read', 'clipboard-write'], launchOptions };

// pixel-exact: two runs in the VM container gave identical screenshots (with the raster flags above); a reference
// taken elsewhere (macOS fonts) differs everywhere — bless in the container only
const visual = [['desktop', desktop], ['phone', phone]].map(([vp, dev]) =>
  ({ name: `visual-${vp}`, testDir: './visual', use: { ...common, ...dev, colorScheme: 'light', locale: 'de-DE' } }));

// no phone-dark: its contrast pairs are desktop-dark's (one palette per scheme), its layout phone-light's — over the
// former full visual suite (E2E_AXE=1, 2026-10-07) it showed no finding the three projects here do not
const a11y = [['desktop', desktop, 'light'], ['desktop', desktop, 'dark'], ['phone', phone, 'light']].map(([vp, dev, scheme]) =>
  ({ name: `a11y-${vp}-${scheme}`, testDir: './a11y', testIgnore: vp === 'desktop' && scheme === 'light' ? undefined : /keys\.spec/, use: { ...common, ...dev, colorScheme: scheme, locale: 'de-DE' } }));

export default defineConfig({
  globalSetup: './global-setup.mjs',
  globalTeardown: './global-teardown.mjs',
  timeout: 60_000,
  expect: { timeout: 10_000, toHaveScreenshot: { maxDiffPixels: 0, animations: 'disabled', caret: 'hide', scale: 'device' } },
  snapshotPathTemplate: '{testDir}/__screenshots__/{projectName}/{arg}{ext}',
  retries: 0, // a flaky test is fixed, never retried away
  forbidOnly: true,
  reporter: [['list'], ['json', { outputFile: `.work/last-run-${FIRST_PORT}.json` }]],
  outputDir: `.work/test-results-${FIRST_PORT}`,
  workers: Number(process.env.E2E_WORKERS || 4),
  // one suite per run: the visual suite's workers share one server (read-only), the functional suite's write to their own
  // perf: interaction latency (e2e/perf), opt-in, never part of the gate
  projects: process.env.E2E_SUITE === 'chaos' ? [{ name: 'chaos', testDir: './chaos', fullyParallel: false, workers: 1, use: { ...common, ...desktop, colorScheme: 'light', locale: 'de-DE', trace: 'retain-on-failure' } }] // a chaos finding is rare: its trace (.work/test-results-<port>/…/trace.zip) is the evidence
    : process.env.E2E_SUITE === 'perf' ? [{ name: 'perf', testDir: './perf', fullyParallel: false, workers: 1, timeout: 900_000, use: { ...common, ...desktop, colorScheme: 'light', locale: 'de-DE' } }]
    : process.env.E2E_SUITE === 'visual' ? visual
    : process.env.E2E_SUITE === 'a11y' ? a11y : [
    { name: 'functional', testDir: './functional', fullyParallel: false, use: { ...common, ...desktop, colorScheme: 'light', locale: 'de-DE' } },
  ],
});
