// Change history: undo (plain and with the warning question), filters, zoom/view memory, live updates.
import { test, expect } from '../lib/test.mjs';
import { BASE } from '../lib/env.mjs';
import { createEvent } from './_set.mjs';

const doc = async page => (await (await page.request.get('/api/history', { headers: { Accept: 'application/json' } })).json());
const changeOf = async (page, title, act = 'create') => (await doc(page)).changes.filter(c => c.items.some(i => i.title === title && i.act === act)).sort((a, b) => b.id - a.id)[0];

test('undo a change in the list: the event is gone, the change is marked undone', async ({ page }) => {
  await page.goto('/calendar');
  const key = await createEvent(page, 'F4 Undo-Test');
  const c = await changeOf(page, 'F4 Undo-Test');
  expect(c).toBeTruthy();
  await page.goto('/history?view=list');
  await page.locator(`.hxc-list [data-pick="${c.id}"]`).click();
  const card = page.locator(`.hxc-card[data-cid="${c.id}"]`);
  await expect(card.locator('h2')).toHaveText('F4 Undo-Test');
  await card.locator(`[data-undo-now="${c.id}"]`).click();
  await expect(page.locator('.toast')).toContainText(`#${c.id}`);
  await expect(page.locator(`.hxc-list [data-pick="${c.id}"]`)).toHaveAttribute('data-status', 'undone');
  expect((await page.request.get('/api/events/' + key)).status()).toBe(404);
  const after = (await doc(page)).changes.find(x => x.id === c.id);
  expect(after.undoneBy).toBeGreaterThan(c.id);
  // reloaded, the undo is a change of its own that reverts it
  await page.reload();
  await expect(page.locator(`.hxc-list [data-pick="${after.undoneBy}"]`)).toBeVisible();
});

test('undo with a warning (a later edit) asks first in the map; "no" keeps, "yes" undoes', async ({ page }) => {
  await page.goto('/calendar');
  const key = await createEvent(page, 'F4 Warnung');
  const csrf = await page.locator('meta[name=csrf]').getAttribute('content');
  const ev = await (await page.request.get('/api/events/' + key)).json();
  const put = await page.request.put('/api/events/' + key, { data: { ...ev, summary: 'F4 Warnung geändert' }, headers: { 'X-CSRF-Token': csrf, Origin: BASE } });
  expect(put.ok()).toBe(true);
  const c = await changeOf(page, 'F4 Warnung');
  await page.goto('/history');
  await page.locator(`.hxg-stop[data-cid="${c.id}"]`).click();
  await page.locator(`.hxg-pop [data-undo="${c.id}"]`).click();
  await expect(page.locator('.ask p')).toContainText(`#${c.id}`);
  await page.locator('.ask [data-a="0"]').click();
  expect((await page.request.get('/api/events/' + key)).status()).toBe(200);
  await page.locator(`.hxg-pop [data-undo="${c.id}"]`).click();
  await page.locator('.ask [data-a="1"]').click();
  await expect(page.locator('.toast')).toContainText(`#${c.id}`);
  expect((await page.request.get('/api/events/' + key)).status()).toBe(404);
});

test('filters: text, facet, status; chips remove them; reset', async ({ page }) => {
  await page.goto('/history?view=list');
  const count = page.locator('[data-r="count"]');
  await expect(count).toContainText(/\d/);
  const all = await page.locator('.hxc-list .hxc-it').count();
  await page.locator('[data-f="q"]').fill('Zahnarzt');
  await expect.poll(() => page.locator('.hxc-list .hxc-it').count()).toBeLessThan(all);
  await expect(page.locator('.hx-chips .hx-chip')).toHaveCount(1);
  await page.locator('.hx-chips .hx-chip').click();
  await expect(page.locator('.hxc-list .hxc-it')).toHaveCount(all);
  await page.locator('.hx-filter > button').click();
  await page.locator('.hx-fpop input[data-f="srcs"][value="import"]').check();
  await page.locator('.hx-fpop [data-fs="active"]').click();
  await expect(page.locator('.hx-chips .hx-chip')).toHaveCount(2);
  await expect(page.locator('[data-r="badge"]')).toHaveText('2');
  for (const it of await page.locator('.hxc-list .hxc-it').all()) await expect(it).toHaveAttribute('data-src', 'import');
  await page.locator('.hx-fpop [data-reset]').click();
  await expect(page.locator('.hx-chips .hx-chip')).toHaveCount(0);
  await expect(page.locator('.hxc-list .hxc-it')).toHaveCount(all);
});

test('zoom and view are remembered across visits', async ({ page }) => {
  await page.goto('/history');
  await page.locator('[data-r="zoom"] [data-zoom="7"]').click();
  await expect(page.locator('[data-r="zoom"] [data-zoom="7"]')).toHaveAttribute('aria-pressed', 'true');
  await page.reload();
  await expect(page.locator('[data-r="zoom"] [data-zoom="7"]')).toHaveAttribute('aria-pressed', 'true');
  await page.locator('[data-view="list"]').click();
  await expect(page.locator('.hxc-list')).toBeVisible();
  await page.goto('/history');
  await expect(page.locator('[data-view="list"]')).toHaveAttribute('aria-pressed', 'true');
  await expect(page.locator('.hxc-list')).toBeVisible();
  await page.locator('[data-view="map"]').click();
  await expect(page.locator('.hxg-scroll')).toBeVisible();
});

test('live: a change made elsewhere appears without a reload', async ({ page, browser }) => {
  await page.goto('/history?view=list');
  await expect(page.locator('.hxc-list .hxc-it').first()).toBeVisible();
  // bob (another session) changes the shared group calendar
  const ctx = await browser.newContext({ baseURL: BASE });
  const bob = await ctx.newPage();
  await bob.goto('/calendar');
  const csrf = await bob.locator('meta[name=csrf]').getAttribute('content');
  const r = await bob.request.post('/login', { form: { username: 'bob', password: 'devpass', csrf, redirect_uri: '/calendar' }, headers: { Origin: BASE }, maxRedirects: 0 });
  expect(r.status()).toBe(303);
  await bob.goto('/calendar');
  await createEvent(bob, 'F4 Live von Bob', { cal: 'group.familie/5eed0000-0000-4000-8000-000000000004' });
  await ctx.close();
  await expect(page.locator('.hxc-list .hxc-it', { hasText: 'F4 Live von Bob' })).toBeVisible({ timeout: 15000 });
});

test.describe('phone', () => {
  test.use({ viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, isMobile: true, hasTouch: true });
  test('the filter popover opens inside the screen, without a resize first', async ({ page }) => {
    // a phone sends `resize` at random (visual/history.spec.mjs), and a resize used to fit the popover: none here
    await page.addInitScript(() => addEventListener('resize', e => e.stopImmediatePropagation(), true));
    await page.goto('/history');
    await page.locator('.hx-filter > button').click();
    const panel = page.locator('.hx-filter.open .hx-fpop');
    await expect(panel).toBeVisible();
    const b = await panel.boundingBox();
    expect(b.x, 'left edge').toBeGreaterThanOrEqual(8);
    expect(b.x + b.width, 'right edge').toBeLessThanOrEqual(390 - 8);
  });
  test('the closed filter popover is placed for the drawn toolbar, without a resize after load', async ({ page }) => {
    // a slow phone dispatches its one `resize` before the deferred scripts ran: no listener sees it, so the
    // placement history.js makes is the only one. The list view hides the zoom buttons, which moves the button
    // (fitted before that: right edge at 397.75 px of 390 until a resize or an open)
    await page.addInitScript(() => addEventListener('resize', e => e.stopImmediatePropagation(), true));
    await page.goto('/history?view=list');
    await page.locator('.hxc-list').waitFor();
    const b = await page.locator('.hx-fpop').evaluate(p => { const r = p.getBoundingClientRect(); return { left: r.left, right: r.right, visibility: getComputedStyle(p).visibility }; });
    expect(b.visibility, 'closed').toBe('hidden');
    expect(b.left, 'left edge').toBeGreaterThanOrEqual(8);
    expect(b.right, 'right edge').toBeLessThanOrEqual(390 - 8);
  });
});
