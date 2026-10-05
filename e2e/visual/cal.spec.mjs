// Calendar page: every view, the toolbar popovers, the orbit picker, the rail tip, the phone agenda.
// Read-only (lib/visual.mjs). The editor dialog's own states live in visual/editor specs.
import { expect } from '../lib/test.mjs';
import { isPhone, shot } from '../lib/visual.mjs';

const D = '2026-10-14';
const cal = (view, date = D, extra = '') => `/calendar?view=${view}&date=${date}${extra}`;
// the page is ready when app.js wired the fragment: the rail (SVG) exists on views that have one
async function open(page, url) {
  await page.goto(url);
  await expect(page.locator('#viewroot')).toBeVisible();
}
const pageFilter = page => page.locator('header .filter:not(.user):not(.shelp)');

shot('month @main', async (page, snap) => {
  await open(page, cal('month'));
  await page.mouse.move(0, 0);
  await snap('cal-month');
});

shot('month: picked day, chip hover, rail tip', async (page, snap, testInfo) => {
  await open(page, cal('month'));
  const day = page.locator('.v-month .day[data-date="2026-10-16"]');
  const bb = await day.boundingBox();
  await day.click({ position: { x: bb.width / 2, y: bb.height - 6 } }); // the empty bottom of the cell: no chip, no add button
  await expect(page.locator('.v-month .day[data-date="2026-10-16"]')).toHaveClass(/\bsel\b/);
  await page.mouse.move(0, 0);
  await snap('cal-month-picked');
  if (isPhone(testInfo)) return;
  await page.locator('.v-month .ev[data-key]', { hasText: 'Projekt-Review' }).first().hover();
  await snap('cal-month-chip-hover', { segments: 0 });
  const rail = page.locator('.tl');
  const box = await rail.boundingBox();
  await page.mouse.move(box.x + 32, box.y + box.height / 2);
  await expect(page.locator('body > .tip')).toBeVisible();
  await snap('cal-month-rail-tip', { segments: 0 });
});

shot('month: other months', async (page, snap) => {
  await open(page, cal('month', '2026-11-18'));
  await page.mouse.move(0, 0);
  await snap('cal-month-2026-11');
  await open(page, cal('month', '2026-12-24'));
  await page.mouse.move(0, 0);
  await snap('cal-month-2026-12');
});

shot('month: filter popover and a hidden calendar', async (page, snap) => {
  await open(page, cal('month'));
  await pageFilter(page).locator('> button').click();
  await expect(pageFilter(page)).toHaveClass(/\bopen\b/);
  await snap('cal-filter-pop', { segments: 0 });
  // a calendar switched off: localStorage + GET fragment only
  const done = page.waitForResponse(r => r.url().includes('fragment=1') && r.url().includes('cals='));
  await pageFilter(page).locator('label', { hasText: 'Arbeit' }).locator('input').uncheck();
  await done;
  // close the popover the way a user does (pointer away, focus elsewhere)
  await page.mouse.move(0, 0);
  await page.evaluate(() => { document.activeElement.blur(); document.querySelectorAll('.filter.open').forEach(f => f.classList.remove('open')); });
  await expect(page.locator('.v-month .ev', { hasText: 'Projekt-Review' })).toHaveCount(0);
  await snap('cal-month-filtered');
});

shot('more views popover', async (page, snap) => {
  await open(page, cal('month'));
  await page.locator('.toolbar .more [data-more]').click();
  await expect(page.locator('.toolbar .more')).toHaveClass(/\bopen\b/);
  await snap('cal-more-pop', { segments: 0 });
});

shot('week @main', async (page, snap) => {
  await open(page, cal('week'));
  await expect(page.locator('.stream .week.focus')).toHaveCount(1);
  await page.mouse.move(0, 0);
  // the stream is endless: its length depends on scrolling, so no scrolled segments
  await snap('cal-week', { segments: 0 });
});

shot('day @main', async (page, snap) => {
  await open(page, cal('day'));
  await expect(page.locator('.doc[data-date="2026-10-14"]')).toBeVisible();
  await expect(page.locator('.lrow.sel[data-date="2026-10-14"]')).toHaveCount(1);
  await page.mouse.move(0, 0);
  await snap('cal-day');
});

shot('day without events', async (page, snap) => {
  await open(page, cal('day', '2026-10-18'));
  await expect(page.locator('.doc[data-date="2026-10-18"]')).toBeVisible();
  await page.mouse.move(0, 0);
  await snap('cal-day-empty');
});

shot('year planner @main', async (page, snap) => {
  await open(page, cal('planner'));
  await expect(page.locator('.planner')).toBeVisible();
  await page.mouse.move(0, 0);
  await snap('cal-planner');
  await page.locator('.planner .c[data-date="2026-11-03"]').click();
  await expect(page.locator('.planner .c.sel[data-date="2026-11-03"]')).toHaveCount(1);
  await page.mouse.move(0, 0);
  await snap('cal-planner-picked', { segments: 0 });
});

for (const v of ['weekends', 'vacation', 'countdown', 'numbers']) {
  shot(`more view ${v}`, async (page, snap) => {
    await open(page, cal(v));
    await expect(page.locator(`.toolbar .more [data-more][aria-pressed=true]`)).toBeVisible();
    await page.mouse.move(0, 0);
    await snap(`cal-${v}`);
  });
}

shot('orbit picker', async (page, snap) => {
  await open(page, cal('month'));
  await page.locator('#title').click();
  await expect(page.locator('#picker')).toBeVisible();
  await expect(page.locator('#pk-title')).toHaveText(/2026/);
  await page.mouse.move(0, 0);
  await snap('cal-picker', { segments: 0 });
  await page.locator('#picker .ynode[data-year="2027"]').click();
  await page.locator('#picker .mnode[data-month="3"]').click();
  await expect(page.locator('#pk-title')).toHaveText(/2027/);
  await page.mouse.move(0, 0);
  await snap('cal-picker-2027-03', { segments: 0 });
});
