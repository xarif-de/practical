// The visual check: the key screens of every page against reference screenshots kept in the repo
// (visual/__screenshots__/<project>/<state>.png). It answers "does anything look different that should not?" in
// seconds; an intended change re-blesses the references (`mise run visual -- --update-snapshots`) and the PNG diff
// is reviewed with the commit. Few states on purpose: the surfaces change often, a state nobody reviews only costs.
// Read-only (lib/visual.mjs aborts writes).
import { expect } from '../lib/test.mjs';
import { LOGIN_BASE } from '../lib/env.mjs';
import { shot } from '../lib/visual.mjs';
import { EV, openEvent, unlock } from './_editor.mjs';

const cal = view => `/calendar?view=${view}&date=2026-10-14`;
async function calendar(page, view, ready) {
  await page.goto(cal(view));
  await expect(page.locator('#viewroot')).toBeVisible();
  await expect(page.locator(ready).first()).toBeVisible();
}

shot('calendar: month, week, day, year', async (page, snap) => {
  await calendar(page, 'month', '.v-month .day.today');
  await snap('cal-month');
  await calendar(page, 'week', '.stream .week.focus');
  await snap('cal-week');
  await calendar(page, 'day', '.lrow.sel[data-date="2026-10-14"]');
  await snap('cal-day');
  await calendar(page, 'planner', '.planner');
  await snap('cal-planner');
});

shot('editor: an event, viewed and edited', async (page, snap) => {
  await openEvent(page, EV.review);
  await snap('ed-view');
  await unlock(page);
  await snap('ed-edit');
});

shot('contacts: list and a contact', async (page, snap) => {
  await page.goto('/contacts');
  await expect(page.locator('.list li[data-key]').first()).toBeVisible();
  await snap('ct-list');
  await page.click('.list li[data-name="Anna Albrecht"]');
  await expect(page.locator('#detail #ctform input[name=full_name]')).toHaveValue('Anna Albrecht');
  await snap('ct-contact');
});

shot('tasks, settings, history, login', async (page, snap) => {
  await page.goto('/tasks?view=groups&back=all');
  await expect(page.locator('#tkmain > *').first()).toBeVisible();
  await snap('tasks');
  await page.goto('/settings');
  await expect(page.locator('#settings')).toBeVisible();
  await snap('settings');
  await page.goto('/history?view=list');
  await page.locator('.hxc-list [data-pick]').first().waitFor();
  await snap('history');
  await page.goto(`${LOGIN_BASE}/login`);
  await page.locator('form.pwform').waitFor();
  await snap('login');
});
