// The page shell's cached files and the Content-Security-Policy (package build, 2026-10): icon sprite
// (assets/icons.svg), app.js' strings (/assets/i18n-<lang>.js), colour scheme before the first paint
// (/assets/theme.js), no inline script anywhere (the policy forbids it; lib/test.mjs fails every test
// on a violation), the auto-submitting single sign-on form.
import { test, expect } from '../lib/test.mjs';
import { BASE, LOGIN_BASE } from '../lib/env.mjs';

test('every page answers with the policy; it allows what the app loads and nothing inline', async ({ page }) => {
  for (const p of ['/calendar', '/contacts', '/tasks', '/history', '/settings']) {
    const r = await page.goto(p);
    const csp = r.headers()['content-security-policy'];
    expect(csp, p).toContain("script-src 'self';");
    expect(csp, p).toContain("img-src 'self' data: https://tile.openstreetmap.org;"); // the map dialog's tiles (app.js L.tileLayer), Leaflet's empty-tile GIF
    expect(csp, p).toContain("frame-ancestors 'none'");
    expect(await page.locator('script:not([src]):not([type="application/json"])').count(), p).toBe(0);
  }
  const login = await page.request.get(LOGIN_BASE + '/login');
  expect(login.headers()['content-security-policy']).toContain("form-action 'self'");
});

test('the policy is enforced: an inline script and a foreign image are blocked and reported', async ({ browser }) => {
  // a page of its own: lib/test.mjs' page would (rightly) fail on the violations provoked here
  const page = await browser.newPage({ baseURL: BASE });
  const seen = [];
  page.on('console', m => { if (/Content Security Policy/i.test(m.text())) seen.push(m.text()); });
  await page.goto('/contacts');
  const got = await page.evaluate(() => new Promise(res => {
    const v = [];
    document.addEventListener('securitypolicyviolation', e => { v.push(e.violatedDirective); if (v.length === 2) res(v); });
    const s = document.createElement('script'); s.textContent = 'window.__inline = 1'; document.body.append(s);
    const i = document.createElement('img'); i.src = 'https://example.invalid/x.png'; document.body.append(i);
    setTimeout(() => res(v), 3000);
  }));
  expect(got.sort()).toEqual(['img-src', 'script-src-elem']);
  expect(await page.evaluate(() => window.__inline)).toBeUndefined();
  expect(seen.length).toBeGreaterThan(0);
  await page.close();
});

test('icons come from the sprite file: server-drawn and app.js-drawn ones render', async ({ page }) => {
  await page.goto('/tasks');
  const href = await page.locator('.topbar nav svg.ico use').first().getAttribute('href');
  expect(href).toMatch(/^\/assets\/icons\.svg\?v=[A-Za-z0-9_-]{8}#i-calendar$/);
  expect(await page.locator('meta[name=icons]').getAttribute('content')).toBe(href.split('#')[0]);
  const r = await page.request.get(href.split('#')[0]);
  expect(r.headers()['content-type']).toBe('image/svg+xml');
  await expect.poll(() => page.locator('.topbar nav svg.ico use').first().evaluate(u => u.getBBox().width)).toBeGreaterThan(0);
  // app.js' helper writes the same URL
  const w = await page.evaluate(async () => {
    document.body.insertAdjacentHTML('beforeend', `<span id="e2e-ico">${practical.ico('trash-2')}</span>`);
    for (let i = 0; i < 50; i++) { const b = document.querySelector('#e2e-ico use').getBBox(); if (b.width) return b.width; await new Promise(r => setTimeout(r, 20)); }
    return 0;
  });
  expect(w).toBeGreaterThan(0);
});

test('app.js reads its strings from the language script', async ({ page }) => {
  await page.goto('/calendar');
  expect(await page.locator('script[src^="/assets/i18n-de.js?v="]').count()).toBe(1);
  const t = await page.evaluate(() => ({ n: Object.keys(window.I18N).length, cancel: tr('js-ct-cancel'), missing: tr('js-no-such-key') }));
  expect(t.n).toBeGreaterThan(300);
  expect(t.cancel).toBe('Abbrechen');
  expect(t.missing).toBe('js-no-such-key');
  /* A placeholder whose name is a PREFIX of another must not eat it. Measured: with insertion
     order, "Hole %name … (%n von %of)" came out as "Hole 1ame … (1 von 4)", because `%n` matched
     inside `%name` first. The substitution therefore goes longest name first (core.js `tr`). */
  const ph = await page.evaluate(() => {
    window.I18N['js-ph-probe'] = 'a %name b %n c %of d';
    return tr('js-ph-probe', { n: 1, of: 4, name: 'X' });
  });
  expect(ph).toBe('a X b 1 c 4 d');
});

test('"auto" scheme on a dark system is dark before the first paint', async ({ browser }) => {
  const ctx = await browser.newContext({ baseURL: BASE, colorScheme: 'dark' });
  await ctx.addInitScript(() => requestAnimationFrame(() => { window.__firstFrameScheme = document.documentElement.dataset.scheme; }));
  const page = await ctx.newPage();
  await page.goto('/contacts');
  expect(await page.locator('html').getAttribute('data-pref-scheme')).toBe('auto');
  await expect.poll(() => page.evaluate(() => window.__firstFrameScheme)).toBe('dark');
  await ctx.close();
});

test('a single sign-on form marked data-auto submits itself (app.js; no inline script)', async ({ page }) => {
  // the e2e servers have no identity provider: a page of the same shape, the real app.js
  await page.route(BASE + '/e2e-oidc', r => r.fulfill({ contentType: 'text/html', body: `<!doctype html><html><head><script src="/assets/app.js" defer></script></head><body data-lang="de"><form method="post" action="/e2e-oidc-target" id="oidc-form" data-auto><button>SSO</button></form></body></html>` }));
  let posted = false;
  await page.route(BASE + '/e2e-oidc-target', r => { posted = r.request().method() === 'POST'; return r.fulfill({ contentType: 'text/html', body: '<p>ok</p>' }); });
  await page.goto('/e2e-oidc');
  await expect(page).toHaveURL(BASE + '/e2e-oidc-target');
  expect(posted).toBe(true);
});
