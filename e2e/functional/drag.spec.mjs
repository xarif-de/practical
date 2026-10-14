// Drag and drop of an entry in the month grid and the week stream (calendar.js): the day it is held by goes to the
// day it is dropped on, times kept, the rest of a multi-day entry with it (preview); a question before every move, for a
// series "only this one" or "the whole series"; edges scroll (week) or turn the month; resting on the rail jumps.
import { test, expect, settle, api } from '../lib/test.mjs';

const ARBEIT = 'alice/5eed0000-0000-4000-8000-000000000001', PRIVAT = 'alice/5eed0000-0000-4000-8000-000000000002', FAM = 'group.familie/5eed0000-0000-4000-8000-000000000004';
// the tests of this file share one fixture copy (lib/test.mjs): each moves an event of its own
const cal = (view, date = '2026-10-14') => `/calendar?view=${view}&date=${date}`;
const tile = (page, d) => page.locator(`.v-month > .month > .day[data-date="${d}"]`);
const chipOn = (page, d, text) => tile(page, d).locator(':scope > .ev[data-key]', { hasText: text });

async function open(page, url) {
  page.__inflight.clear();
  await page.goto(url);
  await expect(page.locator('#viewroot')).toBeVisible();
  await settle(page);
}
// press on `from` (a locator), move past the threshold, then to the middle of `to` (a locator or {x, y}); the button stays down
async function hold(page, from, to) {
  const a = await from.boundingBox(), x = a.x + Math.min(20, a.width / 2), y = a.y + a.height / 2;
  await page.mouse.move(x, y); await page.mouse.down(); await page.mouse.move(x + 8, y + 8, { steps: 2 });
  const b = to.boundingBox ? await to.boundingBox() : null, t = b ? { x: b.x + b.width / 2, y: b.y + b.height * 0.75 } : to;
  await page.mouse.move(t.x, t.y, { steps: 8 });
}
const marked = page => page.locator('.drop-on').evaluateAll(els => els.map(e => e.dataset.date));
const startOf = async (page, key) => (await api(page, 'GET', `/api/events/${key}`)).start;

test.afterEach(async ({ page }) => {
  expect(page.__dialogs, 'no alert() during the test').toEqual([]);
});

test('month: an event moves to the day it is dropped on, its time kept, after the question', async ({ page }) => {
  await open(page, cal('month'));
  await hold(page, chipOn(page, '2026-10-24', 'Radtour'), tile(page, '2026-10-22'));
  await expect(page.locator('.ev-ghost')).toContainText('Radtour');
  expect(await marked(page)).toEqual(['2026-10-22']);
  await expect(chipOn(page, '2026-10-24', 'Radtour')).toHaveClass(/\bdrop-from\b/);
  await page.mouse.up();
  await expect(page.locator('.ask p')).toHaveText('„Radtour“ von Sa., 24. Okt. auf Do., 22. Okt. verschieben?');
  await page.locator('.ask [data-a="1"]').click();
  await expect(chipOn(page, '2026-10-22', 'Radtour')).toHaveText('Radtour 10:00');
  await expect(chipOn(page, '2026-10-24', 'Radtour')).toHaveCount(0);
  expect(await startOf(page, `${PRIVAT}/radtour@seed`)).toBe('2026-10-22T10:00:00');
  await expect(page.locator('#ed')).toBeHidden(); // the release is no click on the entry
});

test('month: a multi-day bar moves whole, held by any of its days; Escape and the cancel move nothing', async ({ page }) => {
  await open(page, cal('month'));
  // held by its middle day (the 21st), over the 28th: the 27th-29th are marked
  await hold(page, chipOn(page, '2026-10-21', 'Messe Leipzig'), tile(page, '2026-10-28'));
  expect(await marked(page)).toEqual(['2026-10-27', '2026-10-28', '2026-10-29']);
  await expect(page.locator('.ev-ghost small')).toHaveText('→ 27. Okt. – 29. Okt.');
  await page.keyboard.press('Escape');
  await expect(page.locator('.ev-ghost, .drop-on, .drop-from')).toHaveCount(0);
  await page.mouse.up();
  await expect(page.locator('.ask')).toHaveCount(0);
  await expect(page.locator('#ed')).toBeHidden();
  // again, then "no" at the question
  await hold(page, chipOn(page, '2026-10-21', 'Messe Leipzig'), tile(page, '2026-10-28'));
  await page.mouse.up();
  await page.locator('.ask [data-a="0"]').click();
  expect(await startOf(page, `${ARBEIT}/messe@seed`)).toBe('2026-10-20T08:00:00');
  // and "yes"
  await hold(page, chipOn(page, '2026-10-21', 'Messe Leipzig'), tile(page, '2026-10-28'));
  await page.mouse.up();
  await expect(page.locator('.ask p')).toHaveText('„Messe Leipzig“ von 20. Okt. – 22. Okt. auf 27. Okt. – 29. Okt. verschieben?');
  await page.locator('.ask [data-a="1"]').click();
  for (const d of ['2026-10-27', '2026-10-28', '2026-10-29']) await expect(chipOn(page, d, 'Messe Leipzig')).toHaveCount(1);
  await expect(chipOn(page, '2026-10-20', 'Messe Leipzig')).toHaveCount(0);
  expect(await startOf(page, `${ARBEIT}/messe@seed`)).toBe('2026-10-27T08:00:00');
});

test('month: a series asks for this occurrence or all of them', async ({ page }) => {
  await open(page, cal('month'));
  // the Wednesday standup alone to Thursday
  await hold(page, chipOn(page, '2026-10-14', 'Team-Standup'), tile(page, '2026-10-15'));
  await page.mouse.up();
  await expect(page.locator('.ask p')).toHaveText('„Team-Standup“ ist ein Serientermin. Von Mi., 14. Okt. auf Do., 15. Okt. verschieben?');
  await expect(page.locator('.ask .btn-primary')).toBeFocused();
  await page.locator('.ask [data-a="one"]').click();
  await expect(chipOn(page, '2026-10-15', 'Team-Standup')).toHaveText('Team-Standup 09:00');
  await expect(chipOn(page, '2026-10-14', 'Team-Standup')).toHaveCount(0);
  await expect(chipOn(page, '2026-10-21', 'Team-Standup')).toHaveCount(1);
  // the whole series a day later: Mondays/Wednesdays/Fridays become Tuesdays/Thursdays/Saturdays
  await hold(page, chipOn(page, '2026-10-21', 'Team-Standup'), tile(page, '2026-10-22'));
  await page.mouse.up();
  await page.locator('.ask [data-a="all"]').click();
  await expect(chipOn(page, '2026-10-22', 'Team-Standup')).toHaveCount(1);
  for (const d of ['2026-10-19', '2026-10-21', '2026-10-23']) await expect(chipOn(page, d, 'Team-Standup')).toHaveCount(0);
  for (const d of ['2026-10-20', '2026-10-24']) await expect(chipOn(page, d, 'Team-Standup')).toHaveCount(1);
  await expect(chipOn(page, '2026-10-16', 'Team-Standup')).toHaveCount(1); // the moved Thursday one went along (15th → 16th)
  expect((await api(page, 'GET', `/api/events/${ARBEIT}/standup@seed`)).rrule).toContain('BYDAY=TU,TH,SA');
});

test('month: the bottom edge turns the month while the pointer stays, the rail jumps; the drop lands there', async ({ page }) => {
  await open(page, cal('month'));
  const vh = page.viewportSize().height;
  await hold(page, chipOn(page, '2026-10-15', 'Spätschicht'), { x: 700, y: vh - 4 });
  await expect(page.locator('#viewroot')).toHaveAttribute('data-drag-edge', 'bottom');
  await expect(page.locator('#title')).toHaveText('November 2026');
  await page.mouse.move(700, 450, { steps: 3 });
  await expect(page.locator('#viewroot')).not.toHaveAttribute('data-drag-edge', /./);
  const title = await page.locator('#title').textContent();
  expect(['November 2026', 'Dezember 2026']).toContain(title); // a second turn may have begun before the pointer left
  // the rail: resting on a month far below
  const rail = await page.locator('.tl').boundingBox();
  await page.mouse.move(rail.x + 32, rail.y + rail.height - 30, { steps: 4 });
  await expect(page.locator('#title')).not.toHaveText(title);
  const there = await page.locator('#title').textContent();
  await page.mouse.move(700, 450, { steps: 3 });
  const target = await page.locator('.v-month > .month > .day:not(.other)').nth(9).getAttribute('data-date');
  await page.locator(`.v-month > .month > .day[data-date="${target}"]`).hover();
  await page.mouse.up();
  await page.locator('.ask [data-a="1"]').click();
  await expect(page.locator('#title')).toHaveText(there);
  await expect(chipOn(page, target, 'Spätschicht')).toHaveCount(1);
  expect(await startOf(page, `${ARBEIT}/spaet@seed`)).toBe(`${target}T19:00:00`);
});

test('week: an entry moves to another day block; the stream scrolls at the bottom edge', async ({ page }) => {
  await open(page, cal('week', '2026-10-21')); // its week in the middle of the window
  const entry = page.locator('.dayblock[data-date="2026-10-21"] .ev[data-key]', { hasText: 'Elternabend' });
  await hold(page, entry, page.locator('.dayblock[data-date="2026-10-23"]'));
  expect(await marked(page)).toEqual(['2026-10-23']);
  const box = await page.evaluate(() => document.querySelector('.main').scrollTop);
  await page.mouse.move(700, page.viewportSize().height - 4, { steps: 3 });
  await expect.poll(() => page.evaluate(() => document.querySelector('.main').scrollTop)).toBeGreaterThan(box + 200);
  await page.mouse.move(700, 400, { steps: 3 });
  const target = await page.locator('.drop-on').first().getAttribute('data-date');
  expect(target > '2026-10-25').toBe(true);
  await page.mouse.up();
  await page.locator('.ask [data-a="1"]').click();
  await expect(page.locator(`.dayblock[data-date="${target}"] .ev[data-key]`, { hasText: 'Elternabend' })).toHaveCount(1);
  expect(await startOf(page, `${FAM}/elternabend@seed`)).toBe(`${target}T19:30:00`); // the group's calendar is hers to change
});

test('a press without travel is still the click that opens the event', async ({ page }) => {
  await open(page, cal('month'));
  const c = chipOn(page, '2026-10-25', 'Familienfest'), b = await c.boundingBox();
  await page.mouse.move(b.x + 10, b.y + b.height / 2); await page.mouse.down(); await page.mouse.move(b.x + 12, b.y + b.height / 2 + 1); await page.mouse.up();
  await expect(page.locator('#ed')).toBeVisible();
  await expect(page.locator('#edh')).toHaveValue('Familienfest');
});
