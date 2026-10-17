// PractiCalUploader (PRACTICAL__UPLOADER; on for the login server only, lib/servers.mjs): no tile without the flag; with
// it the setup makes a token and a read-only calendar, and an uploaded entry opens in view mode only.
import { test, expect } from '../lib/test.mjs';
import { BASE, LOGIN_BASE } from '../lib/env.mjs';

test('without the flag there is no uploader tile', async ({ page }) => {
  await page.goto(BASE + '/settings#add');
  await expect(page.locator('#add .wiz-tiles a[href="#manual"]')).toBeVisible();
  await expect(page.locator('a[href="#add-uploader"]')).toHaveCount(0);
  await expect(page.locator('#add-uploader')).toHaveCount(0);
});

test('with the flag: set up, upload, and the entry opens read-only', async ({ page, request }) => {
  await page.goto(LOGIN_BASE + '/login');
  await page.locator('form.pwform input[name=username]').fill('alice');
  await page.locator('form.pwform input[name=password]').fill('devpass');
  await page.locator('form.pwform button[type=submit]').click();
  await expect(page.locator('body')).toHaveAttribute('data-user', 'alice');
  await page.goto(LOGIN_BASE + '/settings#add');
  await page.locator('#add a[href="#add-uploader"]').click();
  await page.locator('#add-uploader input[name=calendar]').fill('Arbeit E2E');
  await page.locator('#add-uploader button[type=submit]').click();
  await expect(page.locator('#up-token')).toBeVisible();
  const token = (await page.locator('#up-token').textContent()).trim(), cal = (await page.locator('#up-cal').textContent()).trim();
  await expect(page.locator('#up-toml')).toContainText(`calendar = "${cal}"`);
  // one entry tomorrow at 09:00 UTC, uploaded as the uploader does
  const day = new Date(Date.now() + 86400000).toISOString().slice(0, 10), ymd = day.replace(/-/g, '');
  const ics = `BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//e2e//EN\r\nBEGIN:VEVENT\r\nUID:e2e-upl@x\r\nDTSTAMP:20260101T000000Z\r\nDTSTART:${ymd}T090000Z\r\nDTEND:${ymd}T100000Z\r\nSUMMARY:Upload E2E\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n`;
  const r = await request.put(`${LOGIN_BASE}/api/uploader/calendars/${cal}`, {
    headers: { authorization: 'Basic ' + Buffer.from(`alice:${token}`).toString('base64') },
    data: { window: { start: '2026-01-01T00:00:00Z', end: '2028-01-01T00:00:00Z' }, events: [{ uid: 'e2e-upl@x', ics }] },
  });
  expect(r.status(), await r.text()).toBe(200);
  expect(await r.json()).toEqual({ created: 1, updated: 0, deleted: 0, unchanged: 0 });
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
});
