// PractiCalUploader (PRACTICAL__UPLOADER; on for the login server only, lib/servers.mjs): no tile without the flag; with
// it the setup (name + owner) shows only the config block, an upload makes a read-only calendar whose entry opens in
// view mode only, and disconnecting the device asks which of its calendars go to the trash.
import { test, expect } from '../lib/test.mjs';
import { BASE, LOGIN_BASE } from '../lib/env.mjs';

test('without the flag there is no uploader tile', async ({ page }) => {
  await page.goto(BASE + '/settings#add');
  await expect(page.locator('#add .wiz-tiles a[href="#manual"]')).toBeVisible();
  await expect(page.locator('a[href="#add-uploader"]')).toHaveCount(0);
  await expect(page.locator('#add-uploader')).toHaveCount(0);
});

test('with the flag: set up, upload, entry read-only, disconnect with its calendar', async ({ page, request }) => {
  await page.goto(LOGIN_BASE + '/login');
  await page.locator('form.pwform input[name=username]').fill('alice');
  await page.locator('form.pwform input[name=password]').fill('devpass');
  await page.locator('form.pwform button[type=submit]').click();
  await expect(page.locator('body')).toHaveAttribute('data-user', 'alice');
  await page.goto(LOGIN_BASE + '/settings#add');
  await page.locator('#add a[href="#add-uploader"]').click();
  // the owner: the user or one of their groups
  await expect(page.locator('#add-uploader select[name=owner] option[value="alice"]')).toHaveCount(1);
  await page.locator('#add-uploader input[name=name]').fill('Uploader E2E');
  await page.locator('#add-uploader button[type=submit]').click();
  await expect(page.locator('#up-toml')).toBeVisible();
  const toml = await page.locator('#up-toml').textContent();
  expect(toml).not.toContain('calendar');
  const token = toml.match(/^token = "([^"]+)"/m)[1];
  // one entry tomorrow at 09:00 UTC, uploaded as the uploader does
  const day = new Date(Date.now() + 86400000).toISOString().slice(0, 10), ymd = day.replace(/-/g, '');
  const ics = `BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//e2e//EN\r\nBEGIN:VEVENT\r\nUID:e2e-upl@x\r\nDTSTAMP:20260101T000000Z\r\nDTSTART:${ymd}T090000Z\r\nDTEND:${ymd}T100000Z\r\nSUMMARY:Upload E2E\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n`;
  const r = await request.put(`${LOGIN_BASE}/api/uploader`, {
    headers: { authorization: 'Basic ' + Buffer.from(`alice:${token}`).toString('base64') },
    data: { calendars: [{ key: 'e2e-key', name: 'Arbeit E2E', window: { start: '2026-01-01T00:00:00Z', end: '2028-01-01T00:00:00Z' }, events: [{ uid: 'e2e-upl@x', ics }] }] },
  });
  expect(r.status(), await r.text()).toBe(200);
  const res = await r.json();
  expect(res.calendars[0]).toMatchObject({ key: 'e2e-key', created: 1 });
  const cal = res.calendars[0].calendar;
  await page.goto(`${LOGIN_BASE}/calendar?view=week&date=${day}`);
  await expect(page.locator('#viewroot')).toHaveAttribute('data-ro', new RegExp(`alice/${cal}`));
  await page.locator('#viewroot .ev', { hasText: 'Upload E2E' }).first().click();
  await expect(page.locator('#edform')).toHaveClass(/\bro\b/);
  await expect(page.locator('#edform .ro-hint')).toBeVisible();
  await expect(page.locator('#ed-lock')).toBeHidden();
  await expect(page.locator('#ed-delete')).toBeHidden();
  await expect(page.locator('#edform')).toHaveClass(/\bmode-view\b/);
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
