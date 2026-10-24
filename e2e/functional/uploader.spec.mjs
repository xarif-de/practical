// PractiCalUploader (PRACTICAL__UPLOADER; on for the login server only, lib/servers.mjs): no tile without the flag; with
// it the setup (name + owner, in the shared list-field rows) leads to step 3, which shows the commands and the config
// block and waits for the device's first request before its done step; an upload makes a read-only calendar whose entry opens in
// view mode only (focus on its close button; a category lock rule makes no unsaved change of it), and disconnecting the
// device asks which of its calendars go to the trash. Step 3 drops the block once left, and says the details are gone
// when the device is.
import { test, expect } from '../lib/test.mjs';
import { BASE, LOGIN_BASE } from '../lib/env.mjs';

test('without the flag there is no uploader tile', async ({ page }) => {
  await page.goto(BASE + '/settings#add');
  await expect(page.locator('#add .wiz-tiles a[href="#manual"]')).toBeVisible();
  await expect(page.locator('a[href="#add-uploader"]')).toHaveCount(0);
  await expect(page.locator('#add-uploader')).toHaveCount(0);
});

const login = async page => {
  await page.goto(LOGIN_BASE + '/login');
  await page.locator('form.pwform input[name=username]').fill('alice');
  await page.locator('form.pwform input[name=password]').fill('devpass');
  await page.locator('form.pwform button[type=submit]').click();
  await expect(page.locator('body')).toHaveAttribute('data-user', 'alice');
};

test('with the flag: set up, upload, entry read-only, disconnect with its calendar', async ({ page, request }) => {
  await login(page);
  // a lock rule on the entry's category: it adjusts the read-only entry, which still closes without asking
  const csrf = await page.locator('meta[name=csrf]').getAttribute('content');
  const rule = await page.request.put(`${LOGIN_BASE}/api/category-rules/UplLock?scope=event`, { data: { title: { mode: 'lock', value: 'Gesperrt E2E' } }, headers: { 'X-CSRF-Token': csrf, Origin: LOGIN_BASE } });
  expect(rule.status(), await rule.text()).toBe(200);
  await page.goto(LOGIN_BASE + '/settings#add');
  await page.locator('#add a[href="#add-uploader"]').click();
  // the owner: the user or one of their groups
  await expect(page.locator('#add-uploader select[name=owner] option[value="alice"]')).toHaveCount(1);
  // the form is the settings' list rows (label left, field right), not loose fields
  await expect(page.locator('#add-uploader .group label.cell input[name=name]')).toHaveCount(1);
  await page.locator('#add-uploader input[name=name]').fill('Uploader E2E');
  await page.locator('#add-uploader button[type=submit]').click();
  // step 3: the two commands and the block, each with a copy button; it waits, no "connected" yet
  await expect(page).toHaveURL(/#add-uploader-wait$/);
  await expect(page.locator('#up-toml')).toBeVisible();
  await expect(page.locator('#up-toml')).toHaveCSS('text-align', 'left');
  for (const id of ['upl-set', 'up-toml', 'upl-once']) await expect(page.locator(`#add-uploader-wait button[data-copy="#${id}"]`)).toHaveCount(1);
  await expect(page.locator('#upl-set')).toHaveText('practicaluploader set connection');
  await expect(page.locator('#upl-once')).toHaveText('practicaluploader once');
  await expect(page.locator('#add-uploader-wait [data-live]')).toBeVisible();
  await expect(page.locator('#add-uploader-done')).toBeHidden();
  const toml = await page.locator('#up-toml').textContent();
  expect(toml).not.toContain('calendar');
  const token = toml.match(/^token = "([^"]+)"/m)[1];
  // one entry tomorrow at 09:00 UTC, uploaded as the uploader does
  const day = new Date(Date.now() + 86400000).toISOString().slice(0, 10), ymd = day.replace(/-/g, '');
  const ics = `BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//e2e//EN\r\nBEGIN:VEVENT\r\nUID:e2e-upl@x\r\nDTSTAMP:20260101T000000Z\r\nDTSTART:${ymd}T090000Z\r\nDTEND:${ymd}T100000Z\r\nSUMMARY:Upload E2E\r\nCATEGORIES:UplLock\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n`;
  const r = await request.put(`${LOGIN_BASE}/api/uploader`, {
    headers: { authorization: 'Basic ' + Buffer.from(`alice:${token}`).toString('base64') },
    data: { calendars: [{ key: 'e2e-key', name: 'Arbeit E2E', window: { start: '2026-01-01T00:00:00Z', end: '2028-01-01T00:00:00Z' }, events: [{ uid: 'e2e-upl@x', ics }] }] },
  });
  expect(r.status(), await r.text()).toBe(200);
  // the device's first request ends the wait: the done step names it
  await expect(page.locator('#add-uploader-done h1')).toBeVisible({ timeout: 10_000 });
  await expect(page.locator('#add-uploader-done [data-upl-name]')).toHaveText('Uploader E2E');
  await expect(page.locator('#up-toml')).toHaveText(''); // the step is left: its secret is out of the page
  const res = await r.json();
  expect(res.calendars[0]).toMatchObject({ key: 'e2e-key', created: 1 });
  const cal = res.calendars[0].calendar;
  await page.goto(`${LOGIN_BASE}/calendar?view=week&date=${day}`);
  await expect(page.locator('#viewroot .toolbar')).toHaveAttribute('data-ro', new RegExp(`alice/${cal}`));
  await page.locator('#viewroot .ev', { hasText: 'Upload E2E' }).first().click();
  await expect(page.locator('#edform')).toHaveClass(/\bro\b/);
  await expect(page.locator('#edform .ro-hint')).toBeVisible();
  await expect(page.locator('#ed-lock')).toBeHidden();
  await expect(page.locator('#ed-delete')).toBeHidden();
  await expect(page.locator('#edform')).toHaveClass(/\bmode-view\b/);
  await expect(page.locator('#ed-close')).toBeFocused(); // its unlock button is hidden
  await expect(page.locator('#edh')).toHaveValue('Gesperrt E2E'); // the lock rule shows …
  await page.locator('#ed-close').click(); // … but is no change to discard
  await expect(page.locator('#ed-confirm')).toBeHidden();
  await expect(page.locator('#ed')).toBeHidden();
  await page.locator('#viewroot .ev', { hasText: 'Upload E2E' }).first().click();
  // a new entry cannot pick it
  await expect(page.locator(`#ed-cal option[value="alice/${cal}"]`)).toBeDisabled();
  // its settings page offers no delete, only the hint
  await page.goto(`${LOGIN_BASE}/settings#cal-alice-${cal}`);
  await expect(page.locator(`#cal-alice-${cal} form[action$="/delete"]`)).toHaveCount(0);
  // disconnect: the device's page lists its calendar; ticked, it goes to the trash
  await page.goto(`${LOGIN_BASE}/settings#account`);
  const link = page.locator('#devices a[href^="#upl-del-"]').first();
  const target = await link.getAttribute('href');
  await link.click();
  await page.locator(`${target} input[name="trash-${cal}"]`).check();
  await page.locator(`${target} button[type=submit]`).click();
  await expect(page.locator(`#devices a[href="${target}"]`)).toHaveCount(0);
  const again = await request.put(`${LOGIN_BASE}/api/uploader`, {
    headers: { authorization: 'Basic ' + Buffer.from(`alice:${token}`).toString('base64') }, data: { calendars: [] },
  });
  expect([401, 403]).toContain(again.status());
  await page.goto(`${LOGIN_BASE}/settings#trash`);
  await expect(page.locator('#trash')).toContainText('Arbeit E2E');
});

test('step 3 says the details are gone when the device is', async ({ page }) => {
  await login(page);
  await page.goto(LOGIN_BASE + '/settings#add-uploader');
  // the device is gone (deleted, or swept as never connected): its status answers 404
  await page.route('**/settings/uploader/*/status', r => r.fulfill({ status: 404, contentType: 'application/json', body: '{"error":"not found"}' }));
  await page.locator('#add-uploader input[name=name]').fill('Uploader Weg');
  await page.locator('#add-uploader button[type=submit]').click();
  await expect(page).toHaveURL(/#add-uploader-wait$/);
  await expect(page.locator('#add-uploader-wait [data-upl-gone]')).toBeVisible();
  await expect(page.locator('#add-uploader-wait h1')).toHaveText(await page.locator('#add-uploader-wait h1').getAttribute('data-gone')); // the heading says so too
  await expect(page.locator('#add-uploader-wait [data-live]')).toBeHidden();
  await expect(page.locator('#up-toml')).toBeHidden();
  await expect(page.locator('#up-toml')).toHaveText('');
});

// No calendar to write into (only read-only ones, or none): every way to a new event — the + button, a day's + —
// opens one dialog with the way to the calendar settings instead of a form that cannot be saved (editor fill.js
// openEditor; the server says the same, it::uploader). The page here has writable calendars, so the test takes
// the editor's choice away the way read-only calendars do: every option disabled.
test('without a writable calendar a new event asks for a calendar first', async ({ page }) => {
  const dialog = page.locator('.ask[role=alertdialog]');
  const open = async () => { await page.goto(BASE + '/calendar?view=month&date=2026-11-10'); await page.locator('#ed-cal option').evaluateAll(os => os.forEach(o => { o.disabled = true; })); };
  // desktop: a day's + in the month view
  await open();
  await page.locator('.v-month .day .add').first().evaluate(b => b.click());
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText('schreibgeschützt');
  await expect(page.locator('#ed')).toBeHidden(); // no form that cannot be saved
  await expect(dialog.locator('.btn-primary')).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
  // phone: the round + button; the dialog's button leads to the calendar settings, where a calendar is made
  await page.setViewportSize({ width: 390, height: 844 });
  await open();
  await page.locator('#fab').click();
  await expect(dialog).toBeVisible();
  await dialog.locator('.btn-primary').click();
  await expect(page).toHaveURL(/\/settings#collections$/);
  // no calendar at all: the dialog says so
  await page.goto(BASE + '/calendar?view=month&date=2026-11-10');
  await page.locator('#ed-cal option').evaluateAll(os => os.forEach(o => o.remove()));
  await page.locator('#fab').click();
  await expect(dialog).toContainText('noch keinen Kalender');
});
