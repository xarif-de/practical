// The shell: scheme button (auto → light → dark → auto, saved per user), avatar menu, copy + toast.
import { test, expect } from '../lib/test.mjs';
import { BASE } from '../lib/env.mjs';

const pref = page => page.locator('html').getAttribute('data-pref-scheme');

// a failed step must not leave light/dark for the specs after this one
test.afterEach(async ({ page }) => {
  if (!page.url().startsWith(BASE)) await page.goto('/contacts');
  const csrf = await page.locator('meta[name=csrf]').getAttribute('content');
  await page.request.post('/settings/scheme', { data: { scheme: 'auto' }, headers: { 'X-CSRF-Token': csrf, Origin: BASE } });
});

test('scheme button steps auto → light → dark → auto and the choice is saved', async ({ page }) => {
  await page.goto('/contacts');
  expect(await pref(page)).toBe('auto');
  const save = () => page.waitForResponse(r => r.url().endsWith('/settings/scheme') && r.request().method() === 'POST');
  for (const want of ['light', 'dark']) {
    const [r] = await Promise.all([save(), page.locator('#scheme').click()]);
    expect(r.status()).toBe(204);
    await expect(page.locator('html')).toHaveAttribute('data-scheme', want);
    await page.reload();
    expect(await pref(page)).toBe(want);
    await expect(page.locator('#scheme')).toHaveAttribute('title', new RegExp(want === 'light' ? 'Hell' : 'Dunkel'));
  }
  await Promise.all([save(), page.locator('#scheme').click()]);
  await page.reload();
  expect(await pref(page)).toBe('auto');
  await expect(page.locator('html')).toHaveAttribute('data-scheme', 'light'); // the project emulates a light system
});

test('avatar menu: user, versions, settings link', async ({ page }) => {
  await page.goto('/tasks');
  await page.locator('.filter.user > .avatar').click();
  const pop = page.locator('.filter.user .pop');
  await expect(pop).toBeVisible();
  await expect(pop.locator('h2')).toHaveText('alice');
  await expect(pop.locator('.src-note')).toContainText(/PractiCal \d+\.\d+\.\d+ · RustiCal \d+\.\d+\.\d+/);
  await pop.locator('a[href="/settings#prefs"]').click();
  await expect(page).toHaveURL(/\/settings#prefs$/);
  await expect(page.locator('#home.page.on')).toBeVisible();
  // a click elsewhere closes the menu
  await page.locator('.filter.user > .avatar').click();
  await expect(page.locator('.filter.user.open')).toHaveCount(1);
  await page.locator('#home h1').click();
  await expect(page.locator('.filter.user.open')).toHaveCount(0);
});

test('copy buttons put the value on the clipboard and say so; the toast closes on ×', async ({ page }) => {
  await page.goto('/settings#account');
  await page.locator('#account button[data-copy="#dav-cal"]').click();
  await expect(page.locator('.toast')).toBeVisible();
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe('https://dav.example.org/caldav/principal/alice');
  await page.locator('.toast .toast-x').click();
  await expect(page.locator('.toast')).toHaveCount(0);
});
