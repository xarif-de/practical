// Change history: objects an import took over (a change each, dated when the object was made) and the older pages the
// views load (/api/history?before=<cursor>, by time). A file of its own: the 100 changes it adds would page the history of history.spec.
import { test, expect } from '../lib/test.mjs';
import { BASE } from '../lib/env.mjs';
import { createEvent } from './_set.mjs';

const CAL = 'alice/5eed0000-0000-4000-8000-000000000001';
const doc = async page => (await (await page.request.get('/api/history', { headers: { Accept: 'application/json' } })).json());

test('an import dates each object it takes over when it was made and shows it as taken over', async ({ page }) => {
  await page.goto('/calendar');
  const csrf = await page.locator('meta[name=csrf]').getAttribute('content');
  const ics = 'BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//e2e//EN\r\nBEGIN:VEVENT\r\nUID:take-1@e2e\r\nDTSTAMP:20261012T000000Z\r\nCREATED:20261001T081500Z\r\nSUMMARY:F4 Übernommen\r\nDTSTART:20261020T100000Z\r\nDTEND:20261020T110000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n';
  const r = await page.request.post(`/settings/import/calendars/${CAL}`, { data: ics, headers: { 'Content-Type': 'text/plain', 'X-CSRF-Token': csrf, Origin: BASE } });
  expect(r.ok(), await r.text()).toBe(true);
  // its own change, at its CREATED (10:15 in Berlin), wherever that falls among the pages
  let d = await doc(page), c;
  for (;;) {
    c = d.changes.find(x => x.items.some(i => i.el === 'cal:take-1@e2e'));
    if (c || !d.more) break;
    d = await (await page.request.get('/api/history?before=' + encodeURIComponent(d.cursor), { headers: { Accept: 'application/json' } })).json();
  }
  expect([c.src, c.at, c.items.length, c.items[0].act, c.items[0].born]).toEqual(['import', '2026-10-01T10:15', 1, 'create', undefined]);
  // a link to the element: older pages until it is there; the list item and the card read "taken over"
  await page.goto(`/history?view=list&el=${encodeURIComponent('cal:take-1@e2e')}`);
  const it = page.locator(`.hxc-list [data-pick="${c.id}"]`);
  await expect(it.locator('.hx-act')).toHaveText('übernommen');
  await it.click();
  const card = page.locator(`.hxc-card[data-cid="${c.id}"]`);
  await expect(card.locator('header .hx-act')).toHaveText('übernommen');
  await expect(card.locator('.hxc-s')).toContainText('übernommen');
  await expect(card.locator('.hxc-meta')).toContainText('1. Oktober');
  await expect(card.locator('.hxc-note')).toHaveText('Übernommen wurde:');
});

test('the list loads older changes at its end, the map those of its window, a link the element it names', async ({ page }) => {
  await page.goto('/calendar');
  const before = await doc(page);
  const oldest = before.changes.find(c => !c.tech); // the oldest in time of the first page
  for (let i = 0; i < 100; i++) await createEvent(page, `F4 Seite ${i}`);
  const first = await doc(page);
  expect(first.more).toBe(true);
  expect(first.changes.length).toBe(100);
  expect(first.changes.some(c => c.id === oldest.id)).toBe(false);
  // the list: the first page, then the next one once its end comes near
  await page.goto('/history?view=list');
  const old = page.locator(`.hxc-list [data-pick="${oldest.id}"]`);
  await expect(page.locator(`.hxc-list [data-pick="${first.changes[99].id}"]`)).toBeVisible();
  await expect(old).toHaveCount(0);
  const btn = page.locator('.hxc-list [data-older]');
  await expect(btn).toHaveText('Ältere Änderungen laden');
  await btn.scrollIntoViewIfNeeded();
  await expect(old).toBeVisible();
  await expect(btn).toHaveCount(0);
  // a filter: no loading by itself, the button asks
  await page.reload();
  await page.locator('[data-f="q"]').fill('nichts trifft das');
  await expect(page.locator('.hxc-list .hx-empty')).toBeVisible();
  await expect(btn).toBeVisible();
  await page.locator('[data-f="q"]').fill('');
  // the map: its 30 days reach before the first page, so the older ones are loaded for it
  await page.goto('/history?view=map');
  await page.locator('[data-r="zoom"] [data-zoom="30"]').click();
  await expect(page.locator(`.hxg-stop[data-cid="${oldest.id}"]`)).toBeAttached();
  // a link to an element only an older page names
  await page.goto(`/history?view=list&el=${encodeURIComponent(oldest.items[0].el)}`);
  await expect(old).toBeVisible();
});
