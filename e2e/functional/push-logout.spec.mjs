// Logout ends this browser's push subscription (audit3-security #11: a shared device kept showing the
// previous user's reminders). Headless Chromium has no push service: the browser's subscription is a stub.
import { test, expect } from '../lib/test.mjs';
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
