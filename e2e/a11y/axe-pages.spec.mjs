// axe over contacts, tasks, settings, the history and the login page (a11y/_axe.mjs).
import { api, expect, loginAs } from '../lib/test.mjs';
import { BASE, LOGIN_BASE } from '../lib/env.mjs';
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
  await page.click('[data-ctview=table]'); // the table arrives from /contacts/part: the lazily inserted view
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

a11y('settings: home, colours, collections, a calendar, groups and the typed question', async (page, check) => {
  await go(page, '/settings', '.set');
  await check('set-home');
  for (const [hash, name] of [['#colors', 'set-colors'], ['#collections', 'set-collections'], ['#cal-alice-5eed0000-0000-4000-8000-000000000001', 'set-cal']]) {
    await page.goto('/settings' + hash);
    await page.locator(`section.page${hash}`).waitFor();
    await check(name);
  }
  await page.goto('/settings#groups');
  await page.locator('#groups details.danger-zone summary').first().click();
  await page.locator('.ask .ask-type').waitFor();
  await check('set-groups-ask'); // the settings page under it: the groups page
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

/* First start (domain::onboarding): the fixture is through the setup, so the states have to be asked
   for — `reopen` forgets the completion and keeps the confirmations, and the fixture confirmed none.
   bob owns no collections, no zone and no regions, so every point is open for him; alice would have
   only the language left. */
a11y('first start: the language gate, a hint and the closing word', async (page, check) => {
  await loginAs(page, 'bob');
  await go(page, '/settings', '#settings');
  const csrf = await page.locator('meta[name=csrf]').getAttribute('content');
  const post = (path, form = {}) => page.request.post(path, { form: { csrf, ...form }, headers: { Origin: BASE } });
  await post('/settings/onboarding/reopen');

  await go(page, '/calendar', '#onb-gate');
  await check('onb-gate');

  // confirming the language leaves the gate and the first hint takes over, with its arrow
  await page.locator('#onb-gate button[type=submit]').click();
  await page.locator('.onb-tip').waitFor();
  await check('onb-hint');

  // the closing word comes when the LAST point is answered in a tab that was being guided
  for (const step of ['tz', 'regions', 'school']) await api(page, 'POST', '/settings/onboarding/confirm', { step });
  for (const kind of ['calendar', 'addressbook']) {
    await go(page, `/settings#new-${kind}`, `#new-${kind} form button[type=submit]`);
    // the submit navigates: without waiting for it the next `go` cancels it and nothing is created
    await Promise.all([page.waitForLoadState(), page.locator(`#new-${kind} form button[type=submit]`).click()]);
  }
  await page.locator('#onb-done').waitFor();
  await check('onb-done');
});
