// Shared constants of the e2e suite. NOW must equal FROZEN in seed/02-practical.py.
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const E2E = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
export const REPO = path.dirname(E2E);
export const NOW = '2026-10-14T08:00:00Z'; // 10:00 Europe/Berlin, a Wednesday with rich fixture data
export const TODAY = '2026-10-14';
export const TZ = 'Europe/Berlin';
export const PORT = Number(process.env.E2E_PORT || 4101);
export const BASE = `http://127.0.0.1:${PORT}`;
// a second server WITHOUT development mode (real login page, password form); its own DB copy
export const LOGIN_PORT = PORT + 1;
export const LOGIN_BASE = `http://127.0.0.1:${LOGIN_PORT}`;
export const WORK = path.join(E2E, '.work');
export const FIXTURE = process.env.E2E_FIXTURE || path.join(WORK, 'fixture.sqlite3');
export const RUN_DIR = path.join(WORK, `run-${PORT}`);
// where the visual suite writes PNG + DOM + style artifacts (mise e2e-baseline sets it)
export const OUT = process.env.E2E_OUT || path.join(WORK, 'out');
