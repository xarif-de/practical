// The login page — only reachable without development mode: the second server (LOGIN_BASE, password login,
// no DEV_NOW; the page shows no date). Nothing is posted.
import { shot } from '../lib/visual.mjs';
import { LOGIN_BASE } from '../lib/env.mjs';

const states = [['', 'login-page'], ['?error=1', 'login-error'], ['?manual=1', 'login-manual'], ['?redirect_uri=%2Ftasks', 'login-redirect']];

shot('login page states @main', async (page, snap) => {
  for (const [q, n] of states) {
    await page.goto(`${LOGIN_BASE}/login${q}`);
    await page.locator('form.pwform').waitFor();
    await snap(n);
  }
  // the scheme button works before login for this visit only (no request)
  await page.locator('#scheme').click();
  await page.locator('#scheme').click();
  await page.locator('html[data-pref-scheme="dark"]').waitFor();
  await page.mouse.move(0, 0);
  await snap('login-scheme-dark');
});
