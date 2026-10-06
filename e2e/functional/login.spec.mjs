// Password login on the server WITHOUT development mode (LOGIN_BASE): failure, success, redirect target, logout.
import { test, expect } from '../lib/test.mjs';
import { LOGIN_BASE } from '../lib/env.mjs';

async function login(page, user, password) {
  await page.locator('form.pwform input[name=username]').fill(user);
  await page.locator('form.pwform input[name=password]').fill(password);
  await page.locator('form.pwform button[type=submit]').click();
}

test('wrong password: back to the form with the error', async ({ page }) => {
  await page.goto(LOGIN_BASE + '/login');
  await login(page, 'alice', 'falsch');
  await expect(page).toHaveURL(/\/login\?error=1/);
  await expect(page.locator('.login .flash.error')).toBeVisible();
  // in the error tint (--err), not the blue info tint of a plain .flash
  const [r, g, b] = (await page.locator('.login .flash.error').evaluate(el => getComputedStyle(el).backgroundColor)).match(/[\d.]+/g).map(Number);
  expect(r > b && r > g, `rgb ${r} ${g} ${b}`).toBe(true);
  // and a protected page still sends to the login
  await page.goto(LOGIN_BASE + '/calendar');
  await expect(page).toHaveURL(/\/login\?redirect_uri=%2Fcalendar/);
});

test('right password: the calendar; the redirect target is honoured; logout ends the session', async ({ page }) => {
  await page.goto(LOGIN_BASE + '/tasks?view=board');
  await expect(page).toHaveURL(/\/login\?redirect_uri=/);
  await login(page, 'alice', 'devpass');
  await expect(page).toHaveURL(LOGIN_BASE + '/tasks?view=board');
  await expect(page.locator('body')).toHaveAttribute('data-user', 'alice');
  await expect(page.locator('.filter.user > .avatar')).toHaveText('AB');
  // a logged-in visit of /login goes on to the calendar
  await page.goto(LOGIN_BASE + '/login');
  await expect(page).toHaveURL(LOGIN_BASE + '/calendar');
  await page.locator('.filter.user > .avatar').click();
  await page.locator('.filter.user form[action="/logout"] button').click();
  await expect(page).toHaveURL(LOGIN_BASE + '/login?manual=1');
  await page.goto(LOGIN_BASE + '/settings');
  await expect(page).toHaveURL(/\/login\?redirect_uri=%2Fsettings/);
});

test('another user sees their own data: carol has no calendars', async ({ page }) => {
  await page.goto(LOGIN_BASE + '/login');
  await login(page, 'carol', 'devpass');
  await expect(page).toHaveURL(LOGIN_BASE + '/calendar');
  await expect(page.locator('body')).toHaveAttribute('data-user', 'carol');
  await expect(page.locator('#calfilter input[data-cal]')).toHaveCount(0);
});
