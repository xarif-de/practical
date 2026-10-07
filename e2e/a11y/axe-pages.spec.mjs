// axe over contacts, tasks, settings, the history and the login page (a11y/_axe.mjs).
import { expect } from '../lib/test.mjs';
import { LOGIN_BASE } from '../lib/env.mjs';
import { a11y, go, phone } from './_axe.mjs';

a11y('contacts: list, contact, edit, table', async (page, check, testInfo) => {
  await go(page, '/contacts', '.list li[data-key]');
  await check('ct-list');
  await page.click('.list li[data-name="Anna Albrecht"]');
  await expect(page.locator('#ctform input[name=full_name]')).toHaveValue('Anna Albrecht');
  await check('ct-view');
  await page.locator('#ct-lock').click();
  await page.locator('#ctform.mode-edit').waitFor();
  await check('ct-edit');
  if (phone(testInfo)) return; // the phone has no table view
  await page.goto('/contacts');
  await page.click('[data-ctview=table]');
  await page.locator('#contacts.view-table').waitFor();
  await check('ct-table');
});

a11y('tasks: groups, table, board', async (page, check) => {
  for (const v of ['groups', 'table', 'board']) {
    await go(page, `/tasks?view=${v}&back=all`, `#tk-views [data-tkview="${v}"][aria-pressed=true]`);
    await page.locator('#tkmain > *').first().waitFor();
    await check(`tk-${v}`);
  }
});

a11y('settings: home, colours, collections, a calendar', async (page, check) => {
  await go(page, '/settings', '.set');
  await check('set-home');
  for (const [hash, name] of [['#colors', 'set-colors'], ['#collections', 'set-collections'], ['#cal-alice-5eed0000-0000-4000-8000-000000000001', 'set-cal']]) {
    await page.goto('/settings' + hash);
    await page.locator(`section.page${hash}`).waitFor();
    await check(name);
  }
});

a11y('history: map and list, the login page', async (page, check) => {
  await go(page, '/history', '#hx .toolbar');
  await page.locator('.hxg-stop').first().waitFor();
  await check('hx-map');
  await page.locator('[data-view="list"]').click();
  await page.locator('.hxc-list').waitFor();
  await check('hx-list');
  await go(page, `${LOGIN_BASE}/login`, 'form.pwform');
  await check('login');
});
