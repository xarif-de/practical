// Helpers of the settings/history/search/shell/login functional specs (fork F4).
import { request } from '@playwright/test';
import { BASE } from '../lib/env.mjs';

// alice's preferences as seed/02-practical.py writes them (lang "" = none: Accept-Language decides)
export const SEED_PREFS = {
  lang: '', scheme: 'auto', timezone: 'Europe/Berlin', holiday_color: '', school_color: '',
  public_regions: 'DE-SN,DE-BY', school_regions: 'DE-SN', vacation_days: '30', vacation_kind: 'category', vacation_category: 'Urlaub',
  vacation_calendar: '', vacation_title: '', alarm_snaps_start: '2 min, 5 min, 15 min, 30 min, 1 h, 3 h, 8 h, 1 d, 2 d, 3 d, 1 w, 2 w', alarm_snaps_end: '5 min, 10 min, 15 min, 30 min, 1 h',
  search_history_days: '90',
};

const csrfOf = page => page.locator('meta[name=csrf]').getAttribute('content');

/** POSTs the prefs form (seed values + `over`) as the settings page's autosave does; throws unless ok. */
export async function savePrefs(page, over = {}) {
  if (!page.url().startsWith(BASE)) await page.goto('/settings');
  const csrf = await csrfOf(page);
  const r = await page.request.post('/settings/prefs', { form: { ...SEED_PREFS, ...over, csrf }, headers: { Accept: 'application/json', Origin: BASE } });
  const j = await r.json();
  if (!j.ok) throw new Error(`prefs: ${JSON.stringify(j)}`);
}

/** A settings form POST (urlencoded, JSON answer) — for setup/cleanup only; tests drive the UI. */
export async function settingsForm(page, path, form = {}) {
  if (!page.url().startsWith(BASE)) await page.goto('/settings');
  const csrf = await csrfOf(page);
  const r = await page.request.post(path, { form: { ...form, csrf }, headers: { Accept: 'application/json', Origin: BASE } });
  const j = await r.json().catch(() => ({ ok: r.ok() }));
  if (j.ok === false) throw new Error(`${path}: ${JSON.stringify(j)}`);
  return j;
}


/** The next autosave answer of the settings page (POST to `path`), parsed. */
export async function nextSave(page, path, action) {
  const [r] = await Promise.all([page.waitForResponse(r => r.request().method() === 'POST' && new URL(r.url()).pathname === path), action()]);
  return r.json();
}

/** A request context with HTTP Basic credentials and nothing else (no cookies). */
export async function basic(user, password) {
  return request.newContext({ baseURL: BASE, httpCredentials: { username: user, password, send: 'always' } });
}

/** Creates a timed event through the JSON API as the page's user; returns its key "principal/cal/id". */
export async function createEvent(page, summary, { cal = 'alice/5eed0000-0000-4000-8000-000000000001', start = '2026-10-16T14:00:00', end = '2026-10-16T15:00:00' } = {}) {
  const [principal, cal_id] = cal.split('/');
  const csrf = await csrfOf(page);
  const r = await page.request.post('/api/events', { data: { principal, cal_id, summary, start, end }, headers: { 'X-CSRF-Token': csrf, Origin: BASE } });
  if (!r.ok()) throw new Error(`create event: ${r.status()} ${await r.text()}`);
  const { id } = await r.json();
  return `${cal}/${id}`;
}
