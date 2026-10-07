// Shared constants of the e2e suite. NOW must equal FROZEN in seed/02-practical.py.
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const E2E = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
export const REPO = path.dirname(E2E);
export const NOW = '2026-10-14T08:00:00Z'; // 10:00 Europe/Berlin, a Wednesday with rich fixture data
export const TODAY = '2026-10-14';
export const TZ = 'Europe/Berlin';
// the dev servers' change-poller period (PRACTICAL__DEV_POLL_MS, development only; production polls every 2 s): the
// live-update tests wait for its rounds
export const POLL_MS = Number(process.env.E2E_POLL_MS || 250);
// E2E_PORT = the first port of a run. The visual and perf suites share one server pair (dev server on E2E_PORT, login
// server on +1; global-setup.mjs). The functional suite gives every Playwright worker a pair of its own on
// E2E_PORT + 2 × its parallel index (lib/servers.mjs, restarted on fresh fixture copies for every spec file):
// a functional run uses E2E_PORT … E2E_PORT + 2 × workers − 1.
export const FIRST_PORT = Number(process.env.E2E_PORT || 4101);
export const PER_WORKER = !process.env.E2E_SUITE || process.env.E2E_SUITE === 'functional' || process.env.E2E_SUITE === 'chaos'; // chaos writes too: fresh servers like the functional suite
export const PORT = FIRST_PORT + (PER_WORKER && process.env.TEST_PARALLEL_INDEX ? 2 * Number(process.env.TEST_PARALLEL_INDEX) : 0);
export const BASE = `http://127.0.0.1:${PORT}`;
// a second server WITHOUT development mode (real login page, password form); its own DB copy
export const LOGIN_PORT = PORT + 1;
export const LOGIN_BASE = `http://127.0.0.1:${LOGIN_PORT}`;
export const WORK = path.join(E2E, '.work');
export const FIXTURE = process.env.E2E_FIXTURE || path.join(WORK, 'fixture.sqlite3');
export const RUN_DIR = path.join(WORK, `run-${PORT}`);
// the server build of the run (global-setup.mjs) + build.rs' minified assets next to it (<BIN>.assets)
export const BIN = path.join(WORK, `run-${FIRST_PORT}`, 'practical');
// where the visual suite writes PNG + DOM + style artifacts (mise e2e-baseline sets it)
export const OUT = process.env.E2E_OUT || path.join(WORK, 'out');
