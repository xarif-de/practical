// Interaction latency of the most common actions: time from the input (mouse click / key press,
// dispatched without Playwright's actionability waits) until the expected result is in the DOM
// (polled per animation frame), median of E2E_LAT_RUNS (default 10) runs each. Not part of the
// gate: `E2E_SUITE=perf npx playwright test` (writes to its own DB copy). Result: one JSON line per
// action on stdout and E2E_LAT_OUT (a JSON file), so two checkouts can be compared.
import fs from 'node:fs';
import { test, expect, api, settle } from '../lib/test.mjs';

const RUNS = Number(process.env.E2E_LAT_RUNS || 10);
const OUT = process.env.E2E_LAT_OUT || '';
const TASKS = '5eed0000-0000-4000-8000-000000000003', PRIVAT = 'alice/5eed0000-0000-4000-8000-000000000002', AB = '5eed0000-0000-4000-8000-000000000011';
const results = {};
const median = a => { const s = [...a].sort((x, y) => x - y), n = s.length; return n % 2 ? s[(n - 1) / 2] : (s[n / 2 - 1] + s[n / 2]) / 2; };

test.describe.configure({ mode: 'serial' });
test.afterAll(() => { if (OUT) fs.writeFileSync(OUT, JSON.stringify(results, null, 1)); });

/** Clicks the centre of `loc` with raw mouse events (no scrolling, no stability wait). */
async function rawClick(page, loc) {
  const b = await loc.boundingBox({ timeout: 10000 });
  if (!b) throw new Error('no box for ' + loc);
  await page.mouse.click(b.x + b.width / 2, b.y + b.height / 2);
}
/** One wheel notch over the month grid (the month view turns its page by wheel, app.js monthWheel). */
async function wheel(page, dy) {
  const b = await page.locator('#viewroot .month').boundingBox();
  await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2);
  await page.mouse.wheel(0, dy);
}
/** `prep(i)` brings the page to the start state, `act(i)` dispatches the input, `done(i)` (in the page) is true once the result shows. */
async function measure(page, name, { prep, act, done, arg = i => i }) {
  const ts = [];
  for (let i = 0; i < RUNS; i++) {
    await prep(i);
    await settle(page, 200);
    const t0 = performance.now();
    await act(i);
    await page.waitForFunction(done, arg(i), { polling: 'raf', timeout: 20000 });
    ts.push(performance.now() - t0);
  }
  results[name] = { median: Math.round(median(ts) * 10) / 10, runs: ts.map(t => Math.round(t * 10) / 10) };
  console.log(JSON.stringify({ name, ...results[name] }));
}

const month = async (page, date = '2026-10-14') => { await page.goto(`/calendar?view=month&date=${date}`); await expect(page.locator('#viewroot .month')).toBeVisible(); };

test('calendar: next / previous / today / view switch / pick a day', async ({ page }) => {
  await measure(page, 'cal-next-month', { prep: () => month(page), act: () => wheel(page, 120), done: () => document.querySelector('#viewroot')?.dataset.date?.startsWith('2026-11') && !!document.querySelector('#viewroot .month .day[data-date="2026-11-18"]') });
  await measure(page, 'cal-prev-month', { prep: () => month(page), act: () => wheel(page, -120), done: () => document.querySelector('#viewroot')?.dataset.date?.startsWith('2026-09') && !!document.querySelector('#viewroot .month .day[data-date="2026-09-16"]') });
  await measure(page, 'cal-today', { prep: () => month(page, '2026-06-10'), act: () => rawClick(page, page.locator('[data-nav=today]')), done: () => !!document.querySelector('#viewroot .month .day[data-date="2026-10-14"]') && document.querySelector('#viewroot').dataset.date.startsWith('2026-10') });
  await measure(page, 'cal-view-week', { prep: () => month(page), act: () => rawClick(page, page.locator('.toolbar [data-view=week]')), done: () => !!document.querySelector('#viewroot .stream .week.focus') });
  await measure(page, 'cal-pick-day', { prep: () => month(page), act: () => rawClick(page, page.locator('.v-month .day[data-date="2026-10-21"] .n')), done: () => !!document.querySelector('#viewroot .v-month .day.sel[data-date="2026-10-21"]') });
});

test('calendar: open the editor, save an edit', async ({ page }) => {
  await month(page);
  const keys = [];
  for (let i = 0; i < RUNS; i++) keys.push((await api(page, 'POST', '/api/events', { principal: 'alice', cal_id: PRIVAT.split('/')[1], summary: `LAT E${i}`, start: `2026-10-${String(5 + 2 * i).padStart(2, '0')}T07:00:00`, end: `2026-10-${String(5 + 2 * i).padStart(2, '0')}T07:30:00`, all_day: false })).id);
  const chip = i => page.locator(`#viewroot .ev[data-key$="/${keys[i]}"]`).first();
  await measure(page, 'editor-open', { prep: async () => { await month(page); }, act: i => rawClick(page, chip(i)), done: i => !document.querySelector('#ed').hidden && document.querySelector('#edh').value === `LAT E${i}`, arg: i => i });
  await measure(page, 'editor-save', {
    prep: async i => { await month(page); await chip(i).click(); await expect(page.locator('#edh')).toHaveValue(`LAT E${i}`); await page.click('#ed-lock'); await page.fill('#edh', `LAT S${i}`); await expect(page.locator('#ed-save')).toBeEnabled(); },
    act: () => rawClick(page, page.locator('#ed-save')),
    done: i => document.querySelector('#ed').hidden && [...document.querySelectorAll('#viewroot .ev')].some(e => e.textContent.includes(`LAT S${i}`)),
  });
});

test('search: one key stroke to results', async ({ page }) => {
  await measure(page, 'search-keystroke', {
    prep: async () => { await month(page); await page.locator('#q').fill('zahnarz'); await expect(page.locator('#sres a.r').first()).toBeVisible(); await page.locator('#q').fill('zahnar'); await settle(page, 400); await page.evaluate(() => { document.querySelector('#sres').innerHTML = ''; }); },
    act: () => page.keyboard.press('z'),
    done: () => [...document.querySelectorAll('#sres a.r')].some(a => /zahnarzt/i.test(a.textContent)),
  });
});

test('tasks: open, view switch, toggle, save', async ({ page }) => {
  await page.goto('/tasks?view=groups&back=all');
  const keys = [];
  for (let i = 0; i < RUNS * 2; i++) keys.push(`alice/${TASKS}/` + (await api(page, 'POST', '/api/tasks', { principal: 'alice', cal_id: TASKS, summary: `LAT T${i}`, due: '2026-10-28' })).id);
  const groups = async () => { await page.goto('/tasks?view=groups&back=all'); await expect(page.locator('#tkmain .tkcard').first()).toBeVisible(); };
  const card = i => page.locator(`#tkmain .tkcard[data-key="${keys[i]}"]`);
  await measure(page, 'task-open', { prep: async i => { await groups(); await card(i).scrollIntoViewIfNeeded(); }, act: i => rawClick(page, card(i).locator('.ti')), done: i => document.querySelector('#edh').value === `LAT T${i}` });
  await measure(page, 'task-view-board', { prep: groups, act: () => rawClick(page, page.locator('#tk-views [data-tkview=board]')), done: () => !!document.querySelector('#tkmain .tkboard') });
  await measure(page, 'task-toggle', {
    prep: async i => { await groups(); await card(i).locator('.tcb').click(); await expect(page.locator('.ask')).toBeVisible(); },
    act: () => rawClick(page, page.locator('.ask [data-a="1"]')),
    done: k => { const c = document.querySelector(`#tkmain .tkcard[data-key="${k}"]`); return !!c && c.classList.contains('off') && !document.querySelector('.ask'); }, arg: i => keys[i],
  });
  await measure(page, 'task-save', {
    prep: async i => { await groups(); const j = RUNS + i; await card(j).locator('.ti').click(); await expect(page.locator('#edh')).toHaveValue(`LAT T${j}`); await page.click('#ed-lock'); await page.fill('#edh', `LAT U${j}`); await expect(page.locator('#ed-save')).toBeEnabled(); },
    act: () => rawClick(page, page.locator('#ed-save')),
    done: j => [...document.querySelectorAll('#tkmain .tkcard .ti')].some(e => e.textContent === `LAT U${j}`) && document.querySelector('#edh')?.value === `LAT U${j}`, arg: i => RUNS + i,
  });
});

test('contacts: open, save', async ({ page }) => {
  await page.goto('/contacts');
  const keys = [];
  for (let i = 0; i < RUNS; i++) keys.push(`alice/${AB}/` + (await api(page, 'POST', '/api/contacts', { principal: 'alice', ab_id: AB, given: 'LAT', family: `C${i}`, full_name: `LAT C${i}` })).id);
  const list = async () => { await page.goto('/contacts'); await expect(page.locator('.list li[data-key]').first()).toBeVisible(); };
  const li = i => page.locator(`.list li[data-key="${keys[i]}"]`);
  await measure(page, 'contact-open', { prep: async i => { await list(); await li(i).scrollIntoViewIfNeeded(); }, act: i => rawClick(page, li(i)), done: i => document.querySelector('#ctform input[name=full_name]').value === `LAT C${i}` });
  await measure(page, 'contact-save', {
    prep: async i => { await list(); await li(i).click(); await expect(page.locator('#ctform input[name=full_name]')).toHaveValue(`LAT C${i}`); await page.click('#ct-lock'); await page.fill('#ctform input[name=nickname]', `nick${i}`); await expect(page.locator('#ct-save')).toBeEnabled(); },
    act: () => rawClick(page, page.locator('#ct-save')),
    done: i => document.querySelector('#ctform.mode-view input[name=nickname]')?.value === `nick${i}` && !!document.querySelector(`.list li.on`),
  });
});

test('settings: autosave of a preference', async ({ page }) => {
  await measure(page, 'settings-save', {
    prep: async i => { await page.goto('/settings#pick-tz'); await page.reload(); await expect(page.locator('#pick-list button[data-v]').first()).toBeVisible(); await page.locator('#pick-q').fill(i % 2 ? 'london' : 'berlin'); await settle(page, 200); },
    act: i => rawClick(page, page.locator(`#pick-list button[data-v="${i % 2 ? 'Europe/London' : 'Europe/Berlin'}"]`)),
    done: i => document.querySelector('#v-tz').textContent === (i % 2 ? 'Europe/London' : 'Europe/Berlin') && !!document.querySelector('.toast'),
  });
});

test('navigation: calendar with a calendar filter kept in this browser', async ({ page }) => {
  await month(page);
  await page.evaluate(c => { localStorage.calHidden = JSON.stringify([c]); }, PRIVAT);
  await measure(page, 'nav-calendar-filtered', {
    prep: async () => { await page.goto('/tasks?view=groups&back=all'); await expect(page.locator('#tkmain .tkcard').first()).toBeVisible(); },
    act: () => rawClick(page, page.locator('nav.nav a[href^="/calendar"]')),
    done: c => location.pathname === '/calendar' && !!document.querySelector('#viewroot .month .ev') && !document.querySelector(`#viewroot .ev[data-key^="${c}/"]`), arg: () => PRIVAT,
  });
  await page.evaluate(() => { delete localStorage.calHidden; });
});
