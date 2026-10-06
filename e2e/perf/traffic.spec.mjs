// Requests a scripted minute of calendar use costs: fragment requests (prefetches apart) and session
// cookies the server sends (each one = a session store write). Not part of the gate:
// `E2E_SUITE=perf npx playwright test -g traffic` (E2E_LAT_SAVEDATA=1: the browser asks for reduced data
// use, app.js prefetches nothing). Result: one JSON line on stdout and E2E_LAT_OUT.
import fs from 'node:fs';
import { test, expect } from '../lib/test.mjs';

if (process.env.E2E_LAT_SAVEDATA === '1') test.beforeEach(({ page }) => page.addInitScript(() => Object.defineProperty(Navigator.prototype, 'connection', { configurable: true, get: () => ({ saveData: true }) })));

test('traffic: a minute of browsing the calendar', async ({ page }) => {
  const live = page.waitForResponse(r => r.url().endsWith('/api/changes'));
  await page.goto('/calendar?view=month&date=2026-10-14');
  await live;
  const n = { navigations: 9, fragments: 0, prefetches: 0, cookies: 0, requests: 0 }; // 6 page turns + 3 view switches
  page.on('request', r => { n.requests++; if (r.url().includes('fragment=1')) { if (r.headers().purpose === 'prefetch') n.prefetches++; else n.fragments++; } });
  page.on('response', async r => { if ((await r.allHeaders())['set-cookie']) n.cookies++; });
  const t0 = Date.now();
  const at = async (sel, ms) => { const b = await page.locator(sel).first().boundingBox(); await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2); await page.waitForTimeout(ms); };
  const month = d => expect(page.locator(`#viewroot .month .day[data-date="${d}"]`)).toBeVisible();
  // the pointer comes to rest on the grid, turns three months on and back again, a pause each
  await at('#viewroot .month', 1500);
  for (const d of ['2026-11-18', '2026-12-16', '2027-01-13']) { await page.mouse.wheel(0, 120); await month(d); await page.waitForTimeout(3000); }
  for (const d of ['2026-12-16', '2026-11-18', '2026-10-14']) { await page.mouse.wheel(0, -120); await month(d); await page.waitForTimeout(3000); }
  // the pointer sweeps over the view switch (no rest on any button), then rests on "week" and clicks
  for (const v of ['planner', 'month', 'week', 'day']) await at(`.toolbar [data-view=${v}]`, 20);
  await at('.toolbar [data-view=week]', 400); await page.mouse.down(); await page.mouse.up();
  await expect(page.locator('#viewroot .stream .week.focus')).toBeVisible();
  await page.waitForTimeout(5000);
  await at('.toolbar [data-view=day]', 400); await page.mouse.down(); await page.mouse.up();
  await expect(page.locator('#viewroot .v-day')).toBeVisible();
  await page.waitForTimeout(5000);
  await at('.toolbar [data-view=month]', 400); await page.mouse.down(); await page.mouse.up();
  await month('2026-10-14');
  await page.waitForTimeout(3000);
  // reads the month, the pointer on the grid; hovers "today" without clicking (the month on screen)
  await at('#viewroot .month', 6000);
  await at('[data-nav=today]', 1000);
  await at('#viewroot .month', 60000 - (Date.now() - t0) > 0 ? 60000 - (Date.now() - t0) : 0);
  n.seconds = Math.round((Date.now() - t0) / 1000);
  console.log(JSON.stringify({ name: 'traffic', ...n }));
  if (process.env.E2E_LAT_OUT) fs.writeFileSync(process.env.E2E_LAT_OUT, JSON.stringify(n));
});
