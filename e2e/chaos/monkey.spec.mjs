// Chaos Tier 1 (audit3-robustness §4): a seeded UI monkey. Per seed, on every page: random clicks, edge-case input,
// keys, selects, wheel, back/forward, resizes, double clicks, reloads — first on a clean network, then with network
// chaos (route interception: delays, dropped requests, 500, 503, 409 on writes, 401). Fails on: page errors and
// unhandled rejections, console errors, CSP violations (lib/test.mjs), a real 5xx or dropped connection, a server
// panic, a native dialog, text "undefined" / "Invalid Date" / "[object Object]", NaN/Infinity in an inline style, a
// dialog off screen, the page wider than the viewport, a login form inside the app shell, a step that hangs.
// Deterministic as far as a browser allows: frozen clock and seeded Math.random (lib/test.mjs), seeded monkey and chaos
// dice; a failure names seed, phase, page, step and the last actions. E2E_SUITE=chaos, one worker; part of `gate full`.
import fs from 'node:fs';
import path from 'node:path';
import { expect, test } from '../lib/test.mjs';
import { BASE, RUN_DIR } from '../lib/env.mjs';

const SEEDS = (process.env.CHAOS_SEEDS || '1,2,3').split(',').map(Number);
const STEPS = Number(process.env.CHAOS_STEPS || 6); // per page and phase
const PAGES = ['/calendar?view=month&date=2026-10-14', '/calendar?view=week&date=2026-10-14', '/calendar?view=day&date=2026-10-14', '/calendar?view=planner&date=2026-10-14', '/tasks', '/tasks?view=board', '/contacts', '/settings', '/history'];
const STRINGS = ['', ' ', 'a', 'Termin', '0', '-1', '99999999999', '1e400', 'NaN', '2026-02-30', '9999-12-31', '0000-01-01', '25:61', '<b>x</b>', '"><img src=x>', "'; DROP TABLE x;--", 'עברית', 'é́́́', '👩‍👩‍👧‍👦', '‮RTL', 'x'.repeat(3000), 'a\nb', '#zzzzzz', 'FREQ=SECONDLY', '@Bob #Tag !Ort', 'https://example.org/?q=1', '%00', '\\', '{{x}}'];
const KEYS = ['Escape', 'Enter', 'Tab', 'Shift+Tab', 'ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Backspace', 'Delete', '/', 'PageDown', 'Home', 'Space', 'Control+z', 'Control+Enter'];
/* known issues outside this suite's reach: { re (finding text), why (finding / issue), until (YYYY-MM-DD: then it fails again) } */
const KNOWN = [];

function rng(seed) { let s = seed >>> 0; return () => { s |= 0; s = (s + 0x6d2b79f5) | 0; let t = Math.imul(s ^ (s >>> 15), 1 | s); t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t; return ((t ^ (t >>> 14)) >>> 0) / 4294967296; }; }

for (const SEED of SEEDS) test(`seed ${SEED}: ${PAGES.length} pages × ${STEPS} steps clean + ${STEPS} with network chaos`, async ({ page, context }) => {
  test.setTimeout(120_000);
  const R = rng(SEED), NR = rng(SEED * 7919 + 1), pick = (a, r = R) => a[Math.floor(r() * a.length)];
  const findings = []; let chaos = false, cur = '', step = 0, trail = [];
  const note = (kind, msg) => { if (!KNOWN.some(k => k.re.test(`${kind} ${msg}`) && new Date(k.until) > new Date())) findings.push(`[${kind}] ${String(msg).slice(0, 300)}\n    seed ${SEED} ${chaos ? 'chaos' : 'clean'} ${cur} step ${step}: ${trail.slice(-6).join(' → ').slice(0, 400)}`); };
  const injected = new WeakSet(), log = path.join(RUN_DIR, 'server.log'), log0 = fs.existsSync(log) ? fs.statSync(log).size : 0; // this seed's part of the server log
  await context.route(u => u.href.startsWith(BASE), async route => {
    const req = route.request(), u = req.url();
    if (!chaos || req.isNavigationRequest() || /\/assets\/|\/dev\/reload|\/api\/changes/.test(u)) return route.fallback();
    const x = NR();
    if (x < 0.12) { await new Promise(r => setTimeout(r, 300 + Math.floor(NR() * 900))); return route.fallback().catch(() => {}); }
    if (x < 0.17) { injected.add(req); return route.abort('failed'); }
    if (x < 0.21) { injected.add(req); return route.fulfill({ status: 500, contentType: 'text/plain', headers: { 'x-chaos': '1' }, body: 'chaos 500' }); }
    if (x < 0.24) { injected.add(req); return route.fulfill({ status: 503, contentType: 'text/plain', headers: { 'x-chaos': '1' }, body: 'chaos 503' }); }
    if (x < 0.27 && req.method() !== 'GET') { injected.add(req); return route.fulfill({ status: 409, contentType: 'text/plain', headers: { 'x-chaos': '1' }, body: 'chaos 409' }); }
    if (x < 0.29) { injected.add(req); return route.fulfill({ status: 401, contentType: 'text/plain', headers: { 'x-chaos': '1' }, body: 'login required' }); }
    return route.fallback().catch(() => {});
  });
  page.on('pageerror', e => note('pageerror', e.stack || e));
  // "Failed to load resource" is the browser's line for any non-2xx answer: the answers themselves are checked below
  page.on('console', m => { if (m.type() === 'error' && !/Failed to load resource/.test(m.text())) note('console', m.text()); });
  page.on('response', r => { if (r.url().startsWith(BASE) && r.status() >= 500 && !r.headers()['x-chaos']) note('5xx', `${r.status()} ${r.request().method()} ${r.url().slice(BASE.length).split('?')[0]}`); });
  page.on('requestfailed', r => { const t = (r.failure() || {}).errorText || ''; if (r.url().startsWith(BASE) && !injected.has(r) && !/ERR_ABORTED/.test(t)) note('requestfailed', `${t} ${r.method()} ${r.url().slice(BASE.length).split('?')[0]}`); });
  // the app's own questions only: a native one is a finding; leaving a page with unsaved work is allowed (beforeunload)
  page.removeAllListeners('dialog');
  page.on('dialog', d => { if (d.type() === 'beforeunload') d.accept().catch(() => {}); else { note('native-dialog', `${d.type()}: ${d.message()}`); d.dismiss().catch(() => {}); } });
  page.on('popup', p => p.close().catch(() => {}));

  const invariants = () => page.evaluate(() => {
    const bad = [], txt = document.body ? document.body.innerText : '';
    for (const w of ['undefined', 'Invalid Date', '[object Object]']) { const i = txt.indexOf(w); if (i >= 0) bad.push(`text "${w}": …${txt.slice(Math.max(0, i - 40), i + 30).replace(/\s+/g, ' ')}…`); }
    for (const el of document.querySelectorAll('[style]')) { const s = el.getAttribute('style'); if (/NaN|undefined|Infinity/.test(s)) { bad.push(`style ${el.tagName}.${el.className}: ${s.slice(0, 80)}`); break; } }
    for (const el of document.querySelectorAll('#ed:not([hidden]) .box, .ask-box, [role=dialog]:not([hidden]), [role=alertdialog]')) {
      const r = el.getBoundingClientRect(); if (!r.width) continue;
      if (r.left < -2 || r.top < -2 || r.right > innerWidth + 2 || (r.bottom > innerHeight + 2 && r.height < innerHeight)) bad.push(`dialog off screen ${el.className}: ${[r.left, r.top, r.right, r.bottom].map(Math.round)}`);
    }
    if (document.documentElement.scrollWidth > innerWidth + 2) bad.push(`page wider than the viewport: ${document.documentElement.scrollWidth} > ${innerWidth}`);
    if (document.querySelector('.topbar .nav') && document.querySelector('form.pwform, #oidc-form')) bad.push('login form inside the app shell');
    return bad;
  });
  const candidates = () => page.evaluate(() => {
    const sel = 'button, a[href], input, select, textarea, [role=button], [role=option], [tabindex]:not([tabindex="-1"]), summary, label, article, .day, td, [data-key], [data-date], h1#title';
    const out = []; let i = 0;
    for (const el of document.querySelectorAll(sel)) {
      if (el.closest('form[action="/logout"]') || el.matches('a[target=_blank], a[href^="http"], a[href^="mailto"], a[href^="tel"], a[href*="/export"], a[download], input[type=file]')) continue;
      if (el.disabled) continue;
      const r = el.getBoundingClientRect(); if (r.width < 2 || r.height < 2 || r.bottom < 0 || r.top > innerHeight || r.right < 0 || r.left > innerWidth) continue;
      const cs = getComputedStyle(el); if (cs.visibility === 'hidden' || cs.pointerEvents === 'none') continue;
      el.dataset.mk = String(i);
      out.push({ i: i++, tag: el.tagName.toLowerCase(), type: el.type || '', text: (el.getAttribute('aria-label') || el.textContent || el.name || el.id || '').trim().slice(0, 30), x: r.left + r.width / 2, y: r.top + r.height / 2 });
    }
    return out;
  });
  // the step is over when nothing loads — the in-flight set of lib/test.mjs is empty (woken by its own events; 4 s at
  // most: a held SSE-like request must not stall the run) — the document is parsed and two frames were drawn
  const drained = () => new Promise(res => {
    if (!page.__inflight.size) return res();
    const evs = ['requestfinished', 'requestfailed', 'framenavigated'], off = () => { evs.forEach(e => page.off(e, chk)); clearTimeout(t); };
    const chk = () => { if (!page.__inflight.size) { off(); res(); } }, t = setTimeout(() => { off(); res(); }, 4000);
    evs.forEach(e => page.on(e, chk));
  });
  const quiet = async () => {
    await page.waitForLoadState('domcontentloaded', { timeout: 8000 }).catch(() => {});
    await drained();
    await page.evaluate(() => new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)))).catch(() => {});
  };
  async function act(depth = 0) {
    if (depth > 3) return;
    let c = [];
    try { c = await candidates(); } catch { await quiet(); try { c = await candidates(); } catch { c = []; } }
    const x = R(), target = c.length ? pick(c) : null;
    let d;
    try {
      if (x < 0.55 && target) { d = `click ${target.tag} "${target.text}"`; await page.mouse.click(target.x, target.y); }
      else if (x < 0.70) { const ins = c.filter(e => (e.tag === 'input' && /^(text|search|email|url|tel|number|date|time|datetime-local|)$/.test(e.type)) || e.tag === 'textarea'); const t = ins.length ? pick(ins) : null; if (!t) return act(depth + 1); const s = pick(STRINGS); d = `fill ${t.tag}[${t.type}] "${t.text}" ← ${JSON.stringify(s.slice(0, 20))}`; const l = page.locator(`[data-mk="${t.i}"]`); await l.fill(s, { timeout: 800 }).catch(() => l.pressSequentially(s.slice(0, 40), { timeout: 800 })); if (R() < 0.5) await l.press(pick(['Enter', 'Tab']), { timeout: 800 }); }
      else if (x < 0.82) { const k = pick(KEYS); d = `key ${k}`; await page.keyboard.press(k); }
      else if (x < 0.87) { const sels = c.filter(e => e.tag === 'select'); if (!sels.length) return act(depth + 1); const s = pick(sels); const l = page.locator(`[data-mk="${s.i}"]`); const opts = await l.locator('option').evaluateAll(o => o.map(y => y.value)); const v = pick(opts.length ? opts : ['']); d = `select "${s.text}" = ${v}`; await l.selectOption(v, { timeout: 800 }); }
      else if (x < 0.91) { d = 'wheel'; await page.mouse.wheel(0, (R() - 0.4) * 1500); }
      else if (x < 0.93) { d = 'back'; await page.goBack({ timeout: 5000 }).catch(() => {}); }
      else if (x < 0.945) { d = 'forward'; await page.goForward({ timeout: 5000 }).catch(() => {}); }
      else if (x < 0.96) { const vp = pick([{ width: 390, height: 844 }, { width: 1280, height: 800 }, { width: 800, height: 600 }]); d = `resize ${vp.width}`; await page.setViewportSize(vp); }
      else if (x < 0.975 && target) { d = `dblclick ${target.tag} "${target.text}"`; await page.mouse.dblclick(target.x, target.y); }
      else { d = 'reload'; await page.reload({ timeout: 8000 }).catch(() => {}); }
    } catch (e) { d = (d || 'act') + ' ✗ ' + e.message.split('\n')[0].slice(0, 60); }
    trail.push(d);
    if (!page.url().startsWith(BASE)) { trail.push(`(left for ${page.url().slice(0, 40)})`); await page.goto(cur).catch(() => {}); } // back past the first page: not a finding
    await quiet();
    try { for (const b of await invariants()) note('invariant', b); } catch { /* a navigation replaced the document */ }
  }
  let steps = 0;
  for (const phase of ['clean', 'chaos']) {
    chaos = phase === 'chaos';
    for (const p of PAGES) {
      cur = p; trail = [`goto ${p}`];
      await page.setViewportSize({ width: 1280, height: 800 });
      // the page before can still navigate on its own (an injected 401 → the login → back): then once more
      for (let i = 0; i < 2; i++) { try { await page.goto(p, { timeout: 15000 }); break; } catch (e) { if (i || !/interrupted by another navigation/.test(e.message)) note('goto', e.message.split('\n')[0]); await quiet(); } }
      for (step = 0; step < STEPS; step++) {
        let timer;
        const hung = await Promise.race([act().then(() => false), new Promise(r => { timer = setTimeout(() => r(true), 15000); })]);
        clearTimeout(timer);
        if (hung) { note('step-timeout', trail.slice(-1)[0]); await page.goto(p, { timeout: 15000 }).catch(() => {}); }
        steps++;
      }
    }
  }
  chaos = false;
  const text = fs.existsSync(log) ? fs.readFileSync(log).subarray(log0).toString('utf8') : '';
  for (const m of new Set((text.match(/panicked at [^\n]*\n[^\n]*/g) || []).map(s => s.replace(/\x1b\[[0-9;]*m/g, '').replace(/\n/, ' — ').slice(0, 200)))) note('server-panic', m);
  for (const k of KNOWN) if (new Date(k.until) <= new Date()) findings.push(`[allow-list] expired ${k.until}: ${k.why}`);
  console.log(`[chaos] seed ${SEED}: ${steps} steps, ${findings.length} findings`);
  expect(findings, findings.join('\n')).toEqual([]);
});
