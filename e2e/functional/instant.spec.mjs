// Behaviour of the client-side speed work (app.js): the live-update stream in a hidden tab, the echo of the
// own save, in-place task updates (DOM equal to a full reload), the calendar filter in the first render.
// Own data only ("F5 …"); bob writes into the shared group calendar.
import { test, expect, api, settle } from '../lib/test.mjs';
import { BASE, POLL_MS } from '../lib/env.mjs';

const GROUP = 'group.familie/5eed0000-0000-4000-8000-000000000004';
const PRIVAT = 'alice/5eed0000-0000-4000-8000-000000000002';
const TASKS = '5eed0000-0000-4000-8000-000000000003';

// every EventSource of the page, to see the live stream open and close
function trackStreams() {
  const ES = window.EventSource;
  window.__es = [];
  window.EventSource = class extends ES { constructor(...a) { super(...a); window.__es.push(this); } };
}
const streams = page => page.evaluate(() => window.__es.filter(e => e.url.endsWith('/api/changes')).map(e => e.readyState));
// a hidden or visible tab, as the browser would report it
const visibility = (page, hidden) => page.evaluate(h => {
  Object.defineProperty(document, 'hidden', { configurable: true, get: () => h });
  Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => h ? 'hidden' : 'visible' });
  document.dispatchEvent(new Event('visibilitychange'));
}, hidden);
// fragment swaps of the calendar (#viewroot's children replaced)
const countSwaps = page => page.evaluate(() => { window.__swaps = 0; new MutationObserver(() => { window.__swaps++; }).observe(document.getElementById('viewroot'), { childList: true }); });
const swaps = page => page.evaluate(() => window.__swaps);

async function bobPage(browser) {
  const ctx = await browser.newContext({ baseURL: BASE });
  const bob = await ctx.newPage();
  await bob.goto('/calendar');
  const csrf = await bob.locator('meta[name=csrf]').getAttribute('content');
  const r = await bob.request.post('/login', { form: { username: 'bob', password: 'devpass', csrf, redirect_uri: '/calendar' }, headers: { Origin: BASE }, maxRedirects: 0 });
  expect(r.status()).toBe(303);
  await bob.goto('/calendar');
  return { bob, ctx };
}
const bobEvent = (bob, summary, start) => api(bob, 'POST', '/api/events', { principal: GROUP.split('/')[0], cal_id: GROUP.split('/')[1], summary, start, end: start.replace(/T(\d\d)/, (m, h) => 'T' + String(+h + 1).padStart(2, '0')) });

test('a hidden tab closes the live stream after the grace period; the return reconnects and shows what changed', async ({ page, browser }) => {
  await page.addInitScript(trackStreams);
  await page.clock.install({ time: new Date('2026-10-14T08:00:00Z') });
  await page.goto('/calendar?view=month&date=2026-11-10');
  await expect.poll(() => streams(page)).toEqual([1]);
  await visibility(page, true);
  await page.clock.runFor(5000);
  expect(await streams(page)).toEqual([1]); // a short absence keeps the stream
  await page.clock.runFor(60000);
  expect(await streams(page)).toEqual([2]); // closed
  // a change while away: no fetch in the hidden tab
  const { bob, ctx } = await bobPage(browser);
  await bobEvent(bob, 'F5 Weg', '2026-11-11T09:00:00');
  await ctx.close();
  const fetched = [];
  page.on('request', r => { if (r.url().includes('fragment=1')) fetched.push(r.url()); });
  await page.clock.runFor(10000);
  expect(fetched).toEqual([]);
  await visibility(page, false);
  await expect.poll(() => streams(page)).toEqual([2, 1]); // a new stream
  await page.clock.runFor(2000); // the views' debounce
  await expect(page.locator('#viewroot .ev', { hasText: 'F5 Weg' })).toBeVisible();
  expect(fetched).toHaveLength(1); // one refresh on return
});

test('the own save redraws once; the echo of the change does not redraw again, a foreign change does', async ({ page, browser }) => {
  await page.goto('/calendar?view=month&date=2026-11-10');
  const key = (await api(page, 'POST', '/api/events', { principal: 'alice', cal_id: PRIVAT.split('/')[1], summary: 'F5 Echo', start: '2026-11-12T09:00:00', end: '2026-11-12T10:00:00' })).id;
  await page.reload();
  await settle(page);
  await page.waitForTimeout(POLL_MS + 1500); // the creation's own live event is through (one poll round + 1.5 s)
  await countSwaps(page);
  await page.locator(`#viewroot .ev[data-key$="/${key}"]`).first().click();
  await page.click('#ed-lock');
  await page.fill('#edh', 'F5 Echo neu');
  const echo = page.waitForResponse(r => r.url().includes('fragment=1'), { timeout: 15000 }).then(() => page.waitForResponse(r => r.url().includes('fragment=1'), { timeout: 15000 }));
  await page.click('#ed-save');
  await expect(page.locator('#viewroot .ev', { hasText: 'F5 Echo neu' })).toBeVisible();
  expect(await swaps(page)).toBe(1);
  await echo; // the change came back over the live stream and was fetched …
  await page.waitForTimeout(300);
  expect(await swaps(page)).toBe(1); // … but the view did not redraw
  // a foreign change still redraws
  const { bob, ctx } = await bobPage(browser);
  await bobEvent(bob, 'F5 Von Bob', '2026-11-13T09:00:00');
  await expect(page.locator('#viewroot .ev', { hasText: 'F5 Von Bob' })).toBeVisible({ timeout: 15000 });
  expect(await swaps(page)).toBe(2);
  // own save and a foreign change in the same poll window: both show
  await page.locator(`#viewroot .ev[data-key$="/${key}"]`).first().click();
  await page.click('#ed-lock');
  await page.fill('#edh', 'F5 Echo zwei');
  await Promise.all([bobEvent(bob, 'F5 Gleichzeitig', '2026-11-14T09:00:00'), page.click('#ed-save')]);
  await ctx.close();
  await expect(page.locator('#viewroot .ev', { hasText: 'F5 Echo zwei' })).toBeVisible();
  await expect(page.locator('#viewroot .ev', { hasText: 'F5 Gleichzeitig' })).toBeVisible({ timeout: 15000 });
});

// the page as the user sees it: DOM with live form values, without the suite's own freeze style
const dom = page => page.evaluate(() => {
  const clone = document.documentElement.cloneNode(true);
  const live = document.querySelectorAll('input,textarea,select'), copies = clone.querySelectorAll('input,textarea,select');
  live.forEach((el, i) => { if (el.type === 'checkbox' || el.type === 'radio') copies[i].setAttribute('data-e2e-checked', String(el.checked)); else copies[i].setAttribute('data-e2e-value', el.value); });
  clone.querySelectorAll('#e2e-freeze').forEach(e => e.remove());
  return { url: location.pathname + location.search + location.hash, html: clone.outerHTML.replace(/></g, '>\n<') };
});
const navigations = page => { const n = { count: 0 }; page.on('framenavigated', f => { if (f === page.mainFrame()) n.count++; }); return n; };

test('tasks: a toggle and a docked save update in place; the page equals a full reload', async ({ page }) => {
  await page.goto('/tasks?view=groups&back=all');
  const keys = [];
  for (const s of ['F5 Haken', 'F5 Speichern']) keys.push(`alice/${TASKS}/` + (await api(page, 'POST', '/api/tasks', { principal: 'alice', cal_id: TASKS, summary: s, due: '2026-10-29' })).id);
  await page.goto('/tasks?view=groups&back=all');
  await expect(page.locator(`#tkmain .tkcard[data-key="${keys[0]}"]`)).toBeVisible();
  const nav = navigations(page);
  const count0 = await page.locator('#task-count').textContent();
  // toggle
  await page.locator(`#tkmain .tkcard[data-key="${keys[0]}"] .tcb`).click();
  await page.click('.ask [data-a="1"]');
  await expect(page.locator(`#tkmain .tkcard.off[data-key="${keys[0]}"]`)).toBeVisible();
  await settle(page);
  expect(nav.count).toBe(0);
  expect(await page.locator('#task-count').textContent()).not.toBe(count0);
  const inPlace = await dom(page);
  await page.reload();
  await settle(page);
  expect(await dom(page)).toEqual(inPlace);
  // docked save: the pane shows the saved task again
  await page.locator(`#tkmain .tkcard[data-key="${keys[1]}"] .ti`).click();
  await expect(page.locator('#edh')).toHaveValue('F5 Speichern');
  await page.click('#ed-lock');
  await page.fill('#edh', 'F5 Gespeichert');
  const n0 = nav.count;
  await page.click('#ed-save');
  await expect(page.locator('#tkmain .tkcard .ti', { hasText: 'F5 Gespeichert' })).toBeVisible();
  await expect(page.locator('#edh')).toHaveValue('F5 Gespeichert');
  await settle(page);
  expect(nav.count).toBe(n0);
  const saved = await dom(page);
  await page.evaluate(k => { sessionStorage.tkSel = k; }, keys[1]); // what the save left for the full reload
  await page.reload();
  await expect(page.locator('#edh')).toHaveValue('F5 Gespeichert');
  await settle(page);
  expect(await dom(page)).toEqual(saved);
});

test('tasks board: a status drop updates in place and equals a full reload', async ({ page }) => {
  await page.goto('/tasks?view=board&back=all');
  const key = `alice/${TASKS}/` + (await api(page, 'POST', '/api/tasks', { principal: 'alice', cal_id: TASKS, summary: 'F5 Ziehen', status: 'NEEDS-ACTION' })).id;
  await page.goto('/tasks?view=board&back=all');
  const nav = navigations(page);
  await page.dragAndDrop(`.tkkc[data-key="${key}"]`, '.tkcol[data-st="IN-PROCESS"]');
  await expect(page.locator(`.tkcol[data-st="IN-PROCESS"] .tkkc[data-key="${key}"]`)).toBeVisible();
  await settle(page);
  expect(nav.count).toBe(0);
  const inPlace = await dom(page);
  await page.reload();
  await settle(page);
  expect(await dom(page)).toEqual(inPlace);
});

test('calendar filter kept in this browser: the calendar page comes filtered in its first render', async ({ page }) => {
  await page.goto('/calendar?view=month&date=2026-10-14');
  await page.click('header .filter:has(#calfilter) > button');
  await page.locator(`#calfilter input[data-cal="${PRIVAT}"]`).uncheck();
  await expect(page.locator(`#viewroot .ev[data-key^="${PRIVAT}/"]`)).toHaveCount(0);
  await page.goto('/tasks?view=groups&back=all');
  const fragments = [];
  page.on('request', r => { if (r.url().includes('fragment=1')) fragments.push(r.url()); });
  await page.locator('nav.nav a[href^="/calendar"]').click();
  await expect(page.locator('#viewroot .month')).toBeVisible();
  await settle(page);
  expect(new URL(page.url()).searchParams.get('cals')).toBeTruthy();
  expect(fragments).toEqual([]); // no second render
  await expect(page.locator(`#viewroot .ev[data-key^="${PRIVAT}/"]`)).toHaveCount(0);
  await expect(page.locator(`#calfilter input[data-cal="${PRIVAT}"]`)).not.toBeChecked();
  await page.evaluate(() => { delete localStorage.calHidden; });
});
