// The suite's `test`: every page starts deterministic — browser clock frozen at NOW (Date only; timers
// run), Math.random seeded, transitions/animations/caret off, no request leaves 127.0.0.1 (OSM tiles
// get one gray PNG), service workers blocked (config). Logged in as alice via DEV_LOGIN on first visit.
import { test as base, expect } from '@playwright/test';
import zlib from 'node:zlib';
import { BASE, NOW } from './env.mjs';

// 256×256 gray tile (#d9d9d9), generated once
const TILE = (() => {
  const w = 256, h = 256, raw = Buffer.alloc((w * 3 + 1) * h, 0xd9);
  for (let y = 0; y < h; y++) raw[y * (w * 3 + 1)] = 0;
  const crcTable = Array.from({ length: 256 }, (_, n) => { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; return c >>> 0; });
  const crc = b => { let c = 0xffffffff; for (const x of b) c = crcTable[(c ^ x) & 0xff] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; };
  const chunk = (t, d) => { const len = Buffer.alloc(4); len.writeUInt32BE(d.length); const td = Buffer.concat([Buffer.from(t), d]); const c = Buffer.alloc(4); c.writeUInt32BE(crc(td)); return Buffer.concat([len, td, c]); };
  const ihdr = Buffer.alloc(13); ihdr.writeUInt32BE(w, 0); ihdr.writeUInt32BE(h, 4); ihdr[8] = 8; ihdr[9] = 2;
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk('IHDR', ihdr), chunk('IDAT', zlib.deflateSync(raw)), chunk('IEND', Buffer.alloc(0))]);
})();

export const FREEZE_CSS = '*,*::before,*::after{transition:none!important;animation:none!important;caret-color:transparent!important}';

function initScript([css]) {
  // mulberry32: the same "random" sequence on every page load (colour dice, Jitsi slug, SSE jitter)
  let s = 0x5eed;
  Math.random = () => { s |= 0; s = (s + 0x6d2b79f5) | 0; let t = Math.imul(s ^ (s >>> 15), 1 | s); t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t; return ((t ^ (t >>> 14)) >>> 0) / 4294967296; };
  const add = () => {
    if (document.getElementById('e2e-freeze')) return;
    const st = document.createElement('style'); st.id = 'e2e-freeze'; st.textContent = css;
    (document.head || document.documentElement).appendChild(st);
  };
  if (document.documentElement) add(); else document.addEventListener('DOMContentLoaded', add);
  document.addEventListener('DOMContentLoaded', add);
  // window.open/alert never leave the page: record instead
  window.__opened = []; window.open = (u, t) => { window.__opened.push(String(u)); return null; };
  // Content-Security-Policy (B19): every violation is reported to the test (the page fixture fails on one)
  document.addEventListener('securitypolicyviolation', e => console.error(`CSP violation: ${e.violatedDirective} blocked ${e.blockedURI || '(inline)'} at ${e.sourceFile}:${e.lineNumber}`), true);
}

export const test = base.extend({
  // in-flight request counter for settle() (SSE streams excluded: they never end)
  page: async ({ page }, use) => {
    const inflight = new Set();
    const long = u => /\/api\/changes|\/dev\/reload/.test(u);
    page.on('request', r => {
      // a new document in the main frame: what the old one had in flight never reports an end
      if (r.isNavigationRequest() && r.frame() === page.mainFrame()) inflight.clear();
      if (!long(r.url())) inflight.add(r);
    });
    page.on('requestfinished', r => inflight.delete(r));
    page.on('requestfailed', r => inflight.delete(r));
    page.__inflight = inflight;
    page.__dialogs = [];
    page.on('dialog', d => { page.__dialogs.push(d.message()); d.dismiss().catch(() => {}); });
    // the server's Content-Security-Policy is enforced (no bypassCSP): a violation event or the browser's own
    // "Refused to … Content Security Policy" console error fails the test
    const csp = [];
    page.on('console', m => { if (m.type() === 'error' && /CSP violation|Content Security Policy/i.test(m.text())) csp.push(m.text()); });
    await page.clock.setFixedTime(new Date(NOW));
    await page.addInitScript(initScript, [FREEZE_CSS]);
    await page.route(u => !u.href.startsWith(BASE) && !u.href.startsWith('http://127.0.0.1:') && !u.href.startsWith('data:') && !u.href.startsWith('blob:'), route => {
      const u = route.request().url();
      if (/tile\.openstreetmap\.org/.test(u)) return route.fulfill({ status: 200, contentType: 'image/png', body: TILE });
      return route.abort('blockedbyclient');
    });
    await use(page);
    expect(csp, 'Content-Security-Policy violations').toEqual([]);
  },
});
export { expect };

/** Waits until nothing loads (no fetch for 300 ms), fonts are ready, no animation runs, two frames passed. */
export async function settle(page, quietMs = 300) {
  const t0 = Date.now();
  let quietSince = Date.now();
  while (Date.now() - t0 < 15000) {
    if (page.__inflight.size) quietSince = Date.now();
    else if (Date.now() - quietSince >= quietMs) break;
    await page.waitForTimeout(50);
  }
  await page.evaluate(async () => {
    await document.fonts.ready;
    for (let i = 0; i < 40 && document.getAnimations().some(a => a.playState === 'running'); i++) await new Promise(r => setTimeout(r, 50));
    await new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)));
  });
}

/** Signs `user` in with the password form route (development mode signed alice in first). */
export async function loginAs(page, user, password = 'devpass') {
  if (!page.url().startsWith(BASE)) await page.goto('/calendar');
  const csrf = await page.locator('meta[name=csrf]').getAttribute('content');
  const r = await page.request.post('/login', { form: { username: user, password, csrf, redirect_uri: '/calendar' }, headers: { Origin: BASE }, maxRedirects: 0 });
  if (r.status() !== 303 && r.status() !== 302) throw new Error(`login as ${user}: HTTP ${r.status()}`);
}

/** JSON API call with the page's session + CSRF token. */
export async function api(page, method, path, data) {
  const csrf = await page.locator('meta[name=csrf]').getAttribute('content');
  const r = await page.request.fetch(path, { method, data, headers: { 'X-CSRF-Token': csrf, Origin: BASE, Accept: 'application/json' } });
  if (!r.ok()) throw new Error(`${method} ${path}: HTTP ${r.status()} ${await r.text()}`);
  const t = await r.text();
  try { return JSON.parse(t); } catch { return t; }
}
