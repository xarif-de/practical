// Prefetch on intent (app.js practical.prefetch / practical.intent): a calendar control fetches what its
// click will show while the pointer rests on it; the click uses that answer once. Changes (own writes, live
// events) drop it, Save-Data and a pending flash keep the server's session untouched. Own data only ("F6 …").
import { test, expect, settle } from '../lib/test.mjs';
import { BASE } from '../lib/env.mjs';

const GROUP = 'group.familie/5eed0000-0000-4000-8000-000000000004';
const PRIVAT = 'alice/5eed0000-0000-4000-8000-000000000002';

// every fragment request of the page: url and whether it was a prefetch
function track(page) {
  const seen = [];
  page.on('request', r => { if (r.url().includes('fragment=1')) seen.push({ url: r.url(), pre: r.headers().purpose === 'prefetch' }); });
  return seen;
}
const prefetchDone = (page, part) => page.waitForResponse(r => r.url().includes('fragment=1') && r.url().includes(part) && r.request().headers().purpose === 'prefetch');
// the page once its live stream is open (prefetching waits for it)
async function open(page, path) {
  const stream = page.waitForResponse(r => r.url().endsWith('/api/changes'));
  await page.goto(path);
  await stream;
  await settle(page);
}
// the text of #viewroot after its next swap (a later live redraw must not hide a stale first one)
const firstSwap = page => page.evaluate(() => { window.__first = null; const root = document.getElementById('viewroot'), mo = new MutationObserver(() => { window.__first = root.textContent; mo.disconnect(); }); mo.observe(root, { childList: true }); });
const ownEvent = (page, summary, start) => page.evaluate(([c, s, t]) => practical.send('/api/events', 'POST', { principal: 'alice', cal_id: c, summary: s, start: t, end: t.replace('T09', 'T10') }).then(r => r.ok), [PRIVAT.split('/')[1], summary, start]);

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

test('hovering "today" prefetches its fragment; the click shows it without another request, as a plain click would', async ({ page }) => {
  await open(page, '/calendar?view=month&date=2026-06-10');
  const seen = track(page);
  const pre = prefetchDone(page, 'date=2026-10-14');
  await page.hover('[data-nav=today]');
  expect((await pre).status()).toBe(200);
  const n = seen.length;
  await page.click('[data-nav=today]');
  await expect(page.locator('#viewroot .month .day[data-date="2026-10-14"]')).toBeVisible();
  await settle(page);
  expect(seen.slice(n).filter(s => s.url.includes('date=2026-10-14'))).toEqual([]); // served from the prefetch
  const viaPrefetch = await page.locator('#viewroot').innerHTML();
  // the same click without a prefetch (stream marked closed: nothing is prefetched or used)
  await open(page, '/calendar?view=month&date=2026-06-10');
  await page.evaluate(() => practical.stream(false));
  const k = seen.length;
  await page.click('[data-nav=today]');
  await expect(page.locator('#viewroot .month .day[data-date="2026-10-14"]')).toBeVisible();
  await settle(page);
  expect(seen.slice(k).map(s => s.pre)).toEqual([false]);
  expect(await page.locator('#viewroot').innerHTML()).toBe(viaPrefetch);
});

test('an own write and a live change from elsewhere drop the prefetched fragment', async ({ page, browser }) => {
  await open(page, '/calendar?view=month&date=2026-11-10');
  const seen = track(page);
  // own write after the prefetch: the click fetches again and shows the new event
  let pre = prefetchDone(page, 'view=week');
  await page.hover('.toolbar [data-view=week]');
  await pre;
  expect(await ownEvent(page, 'F6 Eigen', '2026-11-11T09:00:00')).toBe(true);
  let n = seen.length;
  await firstSwap(page);
  await page.click('.toolbar [data-view=week]');
  await expect(page.locator('#viewroot .stream .ev', { hasText: 'F6 Eigen' }).first()).toBeVisible();
  expect(await page.evaluate(() => window.__first)).toContain('F6 Eigen');
  expect(seen.slice(n).some(s => s.url.includes('view=week'))).toBe(true); // fetched again (the press prefetches anew)
  // a change by bob after the prefetch: the live event drops it
  await open(page, '/calendar?view=month&date=2026-11-10');
  await page.waitForTimeout(3000); // the own write's live event is through
  pre = prefetchDone(page, 'view=week');
  await page.hover('.toolbar [data-view=week]');
  await pre;
  const changed = page.evaluate(() => new Promise(r => document.addEventListener('rustical:change', r, { once: true })));
  const { bob, ctx } = await bobPage(browser);
  const csrf = await bob.locator('meta[name=csrf]').getAttribute('content');
  const r = await bob.request.post('/api/events', { data: { principal: GROUP.split('/')[0], cal_id: GROUP.split('/')[1], summary: 'F6 Von Bob', start: '2026-11-12T09:00:00', end: '2026-11-12T10:00:00' }, headers: { 'X-CSRF-Token': csrf, Origin: BASE, Accept: 'application/json' } });
  expect(r.ok()).toBe(true);
  await ctx.close();
  await changed;
  n = seen.length;
  await firstSwap(page);
  await page.click('.toolbar [data-view=week]');
  await expect(page.locator('#viewroot .stream .ev', { hasText: 'F6 Von Bob' }).first()).toBeVisible();
  expect(await page.evaluate(() => window.__first)).toContain('F6 Von Bob');
  expect(seen.slice(n).some(s => s.url.includes('view=week'))).toBe(true); // fetched again (the press prefetches anew)
});

test('a prefetch leaves a pending flash to the page that shows it; Save-Data prefetches nothing', async ({ page }) => {
  await open(page, '/calendar?view=month&date=2026-06-10');
  // a settings form posted without script: its error flash waits for the next page
  const csrf = await page.locator('meta[name=csrf]').getAttribute('content');
  const r = await page.request.post('/settings/tokens', { form: { name: ' ', csrf }, headers: { Origin: BASE }, maxRedirects: 0 });
  expect(r.status()).toBe(303);
  const pre = prefetchDone(page, 'date=2026-10-14');
  await page.hover('[data-nav=today]');
  expect((await pre).status()).toBe(204);
  await page.goto('/settings');
  await expect(page.locator('.toast.error')).toBeVisible();
  // Save-Data
  await page.addInitScript(() => Object.defineProperty(Navigator.prototype, 'connection', { configurable: true, get: () => ({ saveData: true }) }));
  await open(page, '/calendar?view=month&date=2026-06-10');
  const seen = track(page);
  await page.hover('[data-nav=today]');
  await page.hover('.toolbar [data-view=week]');
  await page.hover('#viewroot .month');
  await page.waitForTimeout(500);
  expect(seen).toEqual([]);
});
