// Logout ends this browser's push subscription (audit3-security #11: a shared device kept showing the
// previous user's reminders). Headless Chromium has no push service: the browser's subscription is a stub.
import { test, expect, settle } from '../lib/test.mjs';
import { LOGIN_BASE } from '../lib/env.mjs';

const ENDPOINT = 'https://fcm.googleapis.com/fcm/send/e2e-logout';

test('logout deletes the push subscription on the server and in the browser, then logs out', async ({ page }) => {
  await page.addInitScript(endpoint => {
    const sub = { endpoint, unsubscribe: () => { localStorage.pushOff = '1'; return Promise.resolve(true); } };
    Object.defineProperty(navigator, 'serviceWorker', { configurable: true, value: { getRegistration: () => Promise.resolve({ pushManager: { getSubscription: () => Promise.resolve(sub) } }) } });
  }, ENDPOINT);
  await page.goto(LOGIN_BASE + '/login');
  await page.locator('form.pwform input[name=username]').fill('carol');
  await page.locator('form.pwform input[name=password]').fill('devpass');
  await page.locator('form.pwform button[type=submit]').click();
  await expect(page).toHaveURL(LOGIN_BASE + '/calendar');
  const del = page.waitForRequest(r => r.url().endsWith('/api/push/subscriptions') && r.method() === 'DELETE');
  await page.locator('.filter.user > .avatar').click();
  await page.locator('.filter.user form[action="/logout"] button').click();
  expect((await del).postDataJSON()).toEqual({ endpoint: ENDPOINT });
  await expect(page).toHaveURL(LOGIN_BASE + '/login?manual=1');
  expect(await page.evaluate(() => localStorage.pushOff)).toBe('1');
});

test('logout without a push subscription is not held up', async ({ page }) => {
  await page.addInitScript(() => {
    Object.defineProperty(navigator, 'serviceWorker', { configurable: true, value: { getRegistration: () => new Promise(() => {}) } }); // never answers
  });
  await page.goto(LOGIN_BASE + '/login');
  await page.locator('form.pwform input[name=username]').fill('carol');
  await page.locator('form.pwform input[name=password]').fill('devpass');
  await page.locator('form.pwform button[type=submit]').click();
  await expect(page).toHaveURL(LOGIN_BASE + '/calendar');
  await page.locator('.filter.user > .avatar').click();
  const t = Date.now();
  await page.locator('.filter.user form[action="/logout"] button').click();
  await expect(page).toHaveURL(LOGIN_BASE + '/login?manual=1');
  expect(Date.now() - t).toBeLessThan(5000);
});

test('a logout in one tab sends the other tabs of the browser to the login at once, their typed work kept', async ({ page, context }) => {
  const login = async p => { await p.locator('form.pwform input[name=username]').fill('alice'); await p.locator('form.pwform input[name=password]').fill('devpass'); await p.locator('form.pwform button[type=submit]').click(); };
  const url = LOGIN_BASE + '/calendar?view=month&date=2026-11-19';
  await page.goto(url); await login(page);
  await expect(page).toHaveURL(url);
  const day = page.locator('.v-month .day[data-date="2026-11-19"]');
  await day.hover(); await day.locator('.add').click();
  await page.locator('#edh').fill('F9 Zweiter Tab');
  await settle(page); // nothing of this tab is on its way: only the other tab's word can send it to the login
  const other = await context.newPage();
  await other.goto(LOGIN_BASE + '/tasks');
  await other.locator('.filter.user > .avatar').click();
  await other.locator('.filter.user form[action="/logout"] button').click();
  await expect(other).toHaveURL(LOGIN_BASE + '/login?manual=1');
  await page.waitForURL(/\/login\?redirect_uri=%2Fcalendar%3Fview%3Dmonth%26date%3D2026-11-19$/);
  await login(page);
  await expect(page).toHaveURL(url);
  await expect(page.locator('#ed:not([hidden]) #edh')).toHaveValue('F9 Zweiter Tab');
});
