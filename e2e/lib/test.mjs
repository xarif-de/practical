// The suite's `test`: every page starts deterministic — browser clock frozen at NOW (Date only; timers
// run), Math.random seeded, transitions/animations/caret off, no request leaves 127.0.0.1 (OSM tiles
// get one gray PNG), service workers blocked (config). Logged in as alice via DEV_LOGIN on first visit.
import { test as base, expect } from '@playwright/test';
import zlib from 'node:zlib';
import { BASE, BIN, FIXTURE, NOW, PER_WORKER, PORT, RUN_DIR } from './env.mjs';
import { startPair } from './servers.mjs';
import { clock, mark } from './timing.mjs';

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
  // pending timers by due time and pending animation frames: settle() lets the timers due within its quiet window
  // (the app's debounces, 120–300 ms, start requests and redraws from timers) and frame chains (eased glides,
  // 200–420 ms) run first. Behaviour unchanged: the original functions schedule and run the callbacks.
  const due = new Map(), frames = new Set();
  const st = window.setTimeout, ct = window.clearTimeout, raf = window.requestAnimationFrame, caf = window.cancelAnimationFrame;
  window.setTimeout = function (f, ms, ...a) {
    if (typeof f !== 'function') return st.call(window, f, ms, ...a);
    const id = st.call(window, (...x) => { due.delete(id); return f.apply(window, x); }, ms, ...a);
    due.set(id, performance.now() + Math.max(0, Number(ms) || 0));
    return id;
  };
  window.clearTimeout = id => { due.delete(id); return ct.call(window, id); };
  window.requestAnimationFrame = function (f) {
    const id = raf.call(window, t => { frames.delete(id); return f(t); });
    frames.add(id);
    return id;
  };
  window.cancelAnimationFrame = id => { frames.delete(id); return caf.call(window, id); };
  window.__e2eTimers = { due, frames, sleep: ms => new Promise(r => st.call(window, r, ms)), frame: () => new Promise(r => raf.call(window, r)) };
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
  // phone emulation: the document starts at the 980 px default layout width, the viewport meta then sets 390 →
  // one `resize` per page load, dispatched in the first frame — before the deferred scripts ran when a frame came
  // first (host load), else ~1 ms after DOMContentLoaded. app.js/history.js fit popovers, re-render the history
  // map and extend the week list on resize, so the order decided e.g. `.hx-fpop`'s translate (empty-history-list:
  // 1 of 3 idle runs had the resize before history.js mounted). An early one is held and re-dispatched after
  // DOMContentLoaded: the order of the common case, every time.
  let loaded = false, held = false;
  addEventListener('resize', e => { if (!loaded) { e.stopImmediatePropagation(); held = true; } }, true);
  document.addEventListener('DOMContentLoaded', () => { loaded = true; if (held) setTimeout(() => dispatchEvent(new Event('resize'))); });
  // window.open/alert never leave the page: record instead
  window.__opened = []; window.open = (u, t) => { window.__opened.push(String(u)); return null; };
  // Content-Security-Policy (B19): every violation is reported to the test (the page fixture fails on one)
  document.addEventListener('securitypolicyviolation', e => console.error(`CSP violation: ${e.violatedDirective} blocked ${e.blockedURI || '(inline)'} at ${e.sourceFile}:${e.lineNumber}`), true);
}

export const test = base.extend({
  // functional suite (PER_WORKER): this worker's own server pair (lib/env.mjs ports), started on fresh fixture copies
  // for every spec file — a spec sees the fixture as seeded, whatever ran before it in this worker or another
  _servers: [async ({}, use) => {
    const s = { file: null, pair: null };
    await use(s);
    if (s.pair) await s.pair.stop();
  }, { scope: 'worker' }],
  _fresh: [async ({ _servers: s }, use, testInfo) => {
    if (PER_WORKER && s.file !== testInfo.file) {
      if (s.pair) await s.pair.stop();
      s.pair = null;
      s.pair = await startPair({ bin: BIN, runDir: RUN_DIR, port: PORT, fixture: FIXTURE });
      s.file = testInfo.file;
    }
    await use();
  }, { auto: true }],
  // in-flight request counter for settle() (SSE streams excluded: they never end)
  page: async ({ page }, use) => {
    const inflight = new Set();
    const long = u => /\/api\/changes|\/dev\/reload/.test(u);
    let nav = null; // the main frame's navigation request until its document commits
    page.on('request', r => {
      // a new document in the main frame: what the old one had in flight never reports an end
      if (r.isNavigationRequest() && r.frame() === page.mainFrame()) { inflight.clear(); nav = r; }
      if (!long(r.url())) { inflight.add(r); page.__requests++; }
    });
    // … nor what the old document requested after the navigation began: its icon sprite, seen between a reload's
    // request and its commit, kept settle() waiting for its 15 s limit (twice per run of settings.spec's categories
    // test). At the commit only the navigation itself can be the new document's.
    page.on('framenavigated', f => {
      if (f !== page.mainFrame() || !nav) return;
      for (const r of inflight) if (r !== nav) inflight.delete(r);
      nav = null;
    });
    const done = r => { inflight.delete(r); if (r === nav) nav = null; }; // a navigation without a commit (download)
    page.on('requestfinished', done);
    page.on('requestfailed', done);
    page.__inflight = inflight;
    page.__requests = 0;
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

// ms after the page's last frame for the request events of that frame to reach this process (page fetch() → Playwright
// `request` event: p99 2.3 ms, max 4 ms over 200 fetches, idle host)
const GRACE_MS = 30;
const LIMIT_MS = 15000;
const sleep = ms => new Promise(r => setTimeout(r, ms));

/** Waits until nothing loads: no request in flight; the page's timers due within `quietMs` of that moment have run;
 *  fonts are ready, no animation runs, two frames passed; and no request started meanwhile or within GRACE_MS after
 *  — else again (15 s at most). The timers stand for the fixed 300 ms of silence this waited for before (2026-10):
 *  what starts a request later than a frame after the last one ended, the app starts from a timer. */
export async function settle(page, quietMs = 300) {
  const t0 = Date.now(), c0 = clock();
  let rounds = 0, inflightMs = 0, pageMs = 0;
  for (;;) {
    rounds++;
    const w0 = Date.now();
    while (page.__inflight.size && Date.now() - t0 < LIMIT_MS) await sleep(10);
    inflightMs += Date.now() - w0;
    const seq = page.__requests, p0 = Date.now();
    try {
      await page.evaluate(async ([q, limit]) => {
        const T = window.__e2eTimers, nap = ms => (T ? T.sleep(ms) : new Promise(r => setTimeout(r, ms)));
        const frame = () => (T ? T.frame() : new Promise(r => requestAnimationFrame(r)));
        const now = performance.now(), until = now + q;
        // a frame chain only up to the window's end: Leaflet's tile fade (GridLayer._updateOpacity) asks for frames as
        // long as Date — frozen here — has not moved 200 ms past the tile's load
        while (T && performance.now() - now < limit && ((T.frames.size && performance.now() < until) || [...T.due.values()].some(d => d <= until))) await nap(10);
        await document.fonts.ready;
        for (let i = 0; i < 40 && document.getAnimations().some(a => a.playState === 'running'); i++) await nap(50);
        await frame(); await frame();
      }, [quietMs, Math.max(0, LIMIT_MS - (Date.now() - t0))]);
    } catch (e) {
      // a navigation replaced the document under the evaluation: settle the new one
      if (!/Execution context was destroyed|navigat/i.test(String(e && e.message)) || Date.now() - t0 >= LIMIT_MS) throw e;
      continue;
    }
    pageMs += Date.now() - p0;
    await sleep(GRACE_MS);
    if ((page.__requests === seq && !page.__inflight.size) || Date.now() - t0 >= LIMIT_MS) break;
  }
  mark({ k: 'settle', ms: clock() - c0, rounds, inflightMs, pageMs, timeout: Date.now() - t0 >= LIMIT_MS ? [...page.__inflight].map(r => r.url()) : undefined });
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
