// The race behind the seed-2 `goto` findings (_goto.mjs), replayed without dice: the next page's document request is
// held until the page before got a 401 for a request of its own, which sends it to the login (req.js authLost). That
// navigation aborts the goto. A plain page.goto fails with net::ERR_ABORTED; gotoPage lets the login round trip load
// and reaches the page.
import { expect, test } from '../lib/test.mjs';
import { BASE } from '../lib/env.mjs';
import { gotoPage } from './_goto.mjs';

const TARGET = '/calendar?view=day&date=2026-10-14';

test('the next page is reached although the page before leaves for the login during the goto', async ({ page }) => {
  await page.goto('/calendar?view=week&date=2026-10-14');
  let navSeen, authDone;
  const nav = new Promise(r => { navSeen = r; }), auth = new Promise(r => { authDone = r; });
  let held = 0;
  // the goto's document request (the first one only): waits until the old page answered the 401
  await page.route(u => u.href === BASE + TARGET, async route => {
    if (held++) return route.fallback();
    navSeen();
    await auth;
    return route.fallback();
  });
  await page.route(u => u.pathname === '/api/search', async route => {
    await nav;
    await route.fulfill({ status: 401, contentType: 'text/plain', body: 'login required' });
    authDone();
  });
  // a request of the old page that the 401 reaches while the goto is under way (the monkey's debounced search)
  await page.evaluate(() => { window.practical.req('/api/search?q=x'); });
  await gotoPage(page, TARGET);
  await expect(page).toHaveURL(BASE + TARGET);
  expect(held, 'the goto was aborted once and sent again').toBe(2);
});
