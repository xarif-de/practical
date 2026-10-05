// Empty states: user carol owns nothing (no calendars, contacts, tasks, history, tokens, groups).
// loginAs posts through page.request — not the browser's network, so the read-only guard is not involved.
import { shot } from '../lib/visual.mjs';
import { loginAs } from '../lib/test.mjs';

const pages = [
  ['/calendar?view=month&date=2026-10-14', 'empty-cal-month', '.month'],
  ['/calendar?view=week&date=2026-10-14', 'empty-cal-week', '#viewroot'],
  ['/calendar?view=day&date=2026-10-14', 'empty-cal-day', '#viewroot'],
  ['/calendar?view=vacation&date=2026-10-14', 'empty-cal-vacation', '#viewroot'],
  ['/calendar?view=countdown&date=2026-10-14', 'empty-cal-countdown', '#viewroot'],
  ['/contacts', 'empty-contacts', '.main'],
  ['/contacts?view=table', 'empty-contacts-table', '.main'],
  ...['groups', 'table', 'board', 'timeline', 'deck', 'history'].map(v => [`/tasks?view=${v}`, `empty-tasks-${v}`, '.main']),
  ['/history', 'empty-history-map', '.hx-empty'],
  ['/history?view=list', 'empty-history-list', '.hx-empty'],
];

shot('empty pages of carol', async (page, snap) => {
  await loginAs(page, 'carol');
  for (const [url, name, sel] of pages) {
    await page.goto(url);
    await page.locator(sel).first().waitFor();
    await snap(name, { segments: name.includes('week') || name.includes('day') ? 0 : 3 });
  }
});

shot('empty settings of carol', async (page, snap) => {
  await loginAs(page, 'carol');
  await page.goto('/settings');
  await snap('empty-set-home');
  for (const h of ['groups', 'trash', 'account', 'search', 'categories', 'collections', 'regions-public']) {
    await page.evaluate(x => { location.hash = x; }, '#' + h);
    await page.locator(`#settings .page.on#${h}`).waitFor();
    if (h === 'search') await page.locator('#hist-list .cell').first().waitFor();
    if (h === 'categories') await page.locator('#cat-list .cell').first().waitFor();
    await snap(`empty-set-${h}`);
  }
});

shot('empty search of carol', async (page, snap) => {
  await loginAs(page, 'carol');
  await page.goto('/contacts');
  await page.locator('#q').fill('zahn');
  await page.locator('#sres .none').waitFor();
  await snap('empty-search', { segments: 0 });
});
