// The client part of bench.mjs: key interactions in Chromium, input → result in the DOM, per side (server) on its
// own browser context, sides interleaved per sample. Each sample: prep (unthrottled; a fresh page load, then quiet
// network + two frames), then the input; its time = event.timeStamp of the first pointerdown/wheel/keydown → `done`
// holds and the result is laid out (arm(); both page clocks, no Playwright round trip inside). `cpu4`: the same
// with Chromium's CPU throttling (CDP Emulation.setCPUThrottlingRate 4, the renderer only) from the input to done.
import { chromium } from '@playwright/test';

const TASK_CAL = '5eed0000-0000-4000-8000-000000000003'; // fixture: alice's "Aufgaben"
const EV_CAL = '5eed0000-0000-4000-8000-000000000002'; // fixture: alice's "Privat"
const NET_QUIET_MS = 150;

async function quiet(page) {
  // no request in flight (the SSE stream excepted) for NET_QUIET_MS, then two frames
  const t0 = Date.now();
  let last = Date.now();
  while (Date.now() - t0 < 5000) {
    if (page.__inflight.size) last = Date.now();
    else if (Date.now() - last >= NET_QUIET_MS) break;
    await new Promise(r => setTimeout(r, 10)); // the in-flight set is filled by request events; this loop reads it
  }
  await page.evaluate(() => new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r))));
}

async function rawClick(page, sel) {
  const b = await page.locator(sel).first().boundingBox({ timeout: 10000 });
  if (!b) throw new Error('no box for ' + sel);
  await page.mouse.click(b.x + b.width / 2, b.y + b.height / 2);
}

/** t0 = the input event's timeStamp; t1 = when `done(arg)` first holds after a DOM change (a MutationObserver checks
 *  it after every change batch; the per-frame poll of waitResult covers a property-only change), after a forced
 *  style + layout: the time includes laying out the result but not the wait for the next frame (vsync, ≤ 16.7 ms,
 *  made the median jump by a frame between runs). */
async function arm(page, done, arg) {
  await page.evaluate(`(${(pred, arg) => {
    window.__bt0 = undefined; window.__bt1 = undefined;
    for (const t of ['pointerdown', 'wheel', 'keydown']) addEventListener(t, e => { if (window.__bt0 === undefined) window.__bt0 = e.timeStamp; }, { capture: true, once: true });
    window.__bcheck = () => { if (window.__bt1 === undefined && window.__bt0 !== undefined && pred(arg)) { void document.documentElement.offsetHeight; window.__bt1 = performance.now(); } return window.__bt1 !== undefined; };
    const mo = new MutationObserver(() => { if (window.__bcheck()) mo.disconnect(); });
    mo.observe(document, { subtree: true, childList: true, attributes: true, characterData: true });
  }})(${done}, ${JSON.stringify(arg)})`);
}

// done(arg): runs in the page (arm), true once the result shows
const INTERACTIONS = [
  {
    name: 'editor-open',
    prep: async (s, i) => { await s.month(); },
    act: async s => rawClick(s.page, `#viewroot .ev[data-key$="/${s.ev}"]`),
    done: () => { const e = document.querySelector('#ed'); return !!e && !e.hidden && /^Bench /.test(document.querySelector('#edh')?.value || ''); },
  },
  {
    name: 'editor-save',
    prep: async (s, i) => {
      await s.month();
      await s.page.locator(`#viewroot .ev[data-key$="/${s.ev}"]`).first().click();
      await s.page.locator('#edh').waitFor();
      await s.page.click('#ed-lock');
      await s.page.fill('#edh', `Bench S${s.saves}`);
      await s.page.locator('#ed-save:not([disabled])').waitFor();
    },
    act: async s => rawClick(s.page, '#ed-save'),
    arg: s => ({ id: s.ev, t: `Bench S${s.saves++}` }),
    done: a => document.querySelector('#ed')?.hidden && (document.querySelector(`#viewroot .ev[data-key$="/${a.id}"]`)?.textContent || '').includes(a.t),
  },
  {
    name: 'view-week',
    prep: async s => { await s.month(); },
    act: async s => rawClick(s.page, '.toolbar [data-view=week]'),
    done: () => !!document.querySelector('#viewroot .stream .week.focus'),
  },
  {
    name: 'next-month',
    prep: async s => { await s.month(); const b = await s.page.locator('#viewroot .month').boundingBox(); await s.page.mouse.move(b.x + b.width / 2, b.y + b.height / 2); },
    act: async s => s.page.mouse.wheel(0, 120),
    done: () => !!document.querySelector('#viewroot')?.dataset.date?.startsWith('2026-11') && !!document.querySelector('#viewroot .month .day[data-date="2026-11-18"]'),
  },
  {
    name: 'task-toggle',
    prep: async s => {
      await s.page.goto('/tasks?view=groups&back=all');
      const k = s.tasks[s.toggles];
      const card = s.page.locator(`#tkmain .tkcard[data-key="${k}"]`);
      await card.scrollIntoViewIfNeeded();
      await card.locator('.tcb').click();
      await s.page.locator('.ask [data-a="1"]').waitFor();
    },
    act: async s => rawClick(s.page, '.ask [data-a="1"]'),
    arg: s => s.tasks[s.toggles++],
    done: k => { const c = document.querySelector(`#tkmain .tkcard[data-key="${k}"]`); return !!c && c.classList.contains('off') && !document.querySelector('.ask'); },
  },
  {
    // list → table on /contacts: with the table in the page a class switch and its layout; with one view per page
    // (perf3 #8) the rows the hover fetched (prep: the pointer rests on the switch until the network is quiet), parsed
    // and laid out. The view is picked in this browser (localStorage, cookie `ctview`): prep resets it to the list.
    name: 'contacts-table',
    prep: async s => {
      await s.page.context().addCookies([{ name: 'ctview', value: 'list', domain: new URL(s.page.url()).hostname, path: '/contacts' }]); // the path app.js sets it for
      await s.page.evaluate(() => { localStorage.ctview = 'list'; });
      await s.page.goto('/contacts');
      await s.page.locator('.list li[data-key]').first().waitFor();
      await s.page.hover('[data-ctview=table]');
    },
    act: async s => rawClick(s.page, '[data-ctview=table]'),
    done: () => !!document.querySelector('#contacts.view-table .sheet tbody tr[data-key]') && document.querySelector('.sheetwrap').getBoundingClientRect().height > 0,
  },
  {
    name: 'search-key',
    prep: async s => {
      await s.month();
      await s.page.locator('#q').fill('zahn');
      await s.page.locator('#sres a.r').first().waitFor();
      await quiet(s.page);
      await s.page.evaluate(() => { document.querySelector('#sres').innerHTML = ''; });
    },
    act: async s => s.page.keyboard.press('a'), // "zahna": a query this page has not asked yet
    // the hits' marks show which query they answer ("Zahn" of the prep's query still matched /zahna/ by its text)
    done: () => [...document.querySelectorAll('#sres a.r mark')].some(m => m.textContent.toLowerCase() === 'zahna'),
  },
];

async function api(page, base, method, path, data) {
  const csrf = await page.locator('meta[name=csrf]').getAttribute('content');
  const r = await page.request.fetch(path, { method, data, headers: { 'X-CSRF-Token': csrf, Origin: base, Accept: 'application/json' } });
  if (!r.ok()) throw new Error(`${method} ${path}: HTTP ${r.status()} ${await r.text()}`);
  return r.json();
}

const DOM_PAGES = [['cal-month', '/calendar?view=month&date=2026-10-14'], ['cal-week', '/calendar?view=week&date=2026-10-14'], ['tasks', '/tasks'], ['contacts', '/contacts'], ['settings', '/settings'], ['history', '/history']];

/** sides: [{ name, base }]; returns { [side]: { [metric]: [[round 0 values], [round 1 values], …] } }: a UI time per
 *  round (= sample index, both sides adjacent), the DOM node counts in one round. Per interaction (normal mode) also
 *  `.busy` (main-thread task time from input to result, CDP Performance.getMetrics TaskDuration: what the page
 *  computed, not what it waited for) and `.reqs` (requests started from input to result). */
export async function runClient(sides, samples, log) {
  const browser = await chromium.launch({ args: ['--disable-gpu', '--disable-partial-raster'] });
  const out = Object.fromEntries(sides.map(s => [s.name, {}]));
  const put = (side, id, round, v) => { const m = (out[side][id] ||= []); (m[round] ||= []).push(Math.round(v * 10) / 10); };
  try {
    const S = [];
    for (const side of sides) {
      const ctx = await browser.newContext({ baseURL: side.base, viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1, locale: 'de-DE', timezoneId: 'Europe/Berlin', reducedMotion: 'reduce', serviceWorkers: 'block' });
      const page = await ctx.newPage();
      page.__inflight = new Set(); page.__started = 0;
      page.on('request', r => { if (!r.url().endsWith('/api/changes')) { page.__inflight.add(r); page.__started++; } });
      for (const ev of ['requestfinished', 'requestfailed']) page.on(ev, r => page.__inflight.delete(r));
      page.on('framenavigated', f => { if (f === page.mainFrame()) page.__inflight.clear(); }); // the old document's requests end without an event
      await page.goto('/login');
      await page.fill('input[name=username]', 'alice');
      await page.fill('input[name=password]', 'devpass');
      await Promise.all([page.waitForURL(/\/calendar/), page.click('form.pwform [type=submit]')]);
      const cdp = await ctx.newCDPSession(page);
      await cdp.send('Performance.enable');
      for (const [name, url] of DOM_PAGES) { // before the bench's own objects exist
        await page.goto(url);
        await quiet(page);
        put(side.name, `dom.${name}`, 0, await page.evaluate(() => document.getElementsByTagName('*').length));
      }
      const ev = (await api(page, side.base, 'POST', '/api/events', { principal: 'alice', cal_id: EV_CAL, summary: 'Bench Ganztag', start: '2026-10-22T00:00:00', end: '2026-10-23T00:00:00', all_day: true })).id;
      const tasks = [];
      for (let i = 0; i < 2 * (samples + 1); i++) tasks.push(`alice/${TASK_CAL}/` + (await api(page, side.base, 'POST', '/api/tasks', { principal: 'alice', cal_id: TASK_CAL, summary: `Bench T${i}`, due: '2026-10-28' })).id);
      const s = { name: side.name, page, cdp, ev, tasks, saves: 0, toggles: 0 };
      s.month = async () => { await page.goto('/calendar?view=month&date=2026-10-14'); await page.locator('#viewroot .month').waitFor(); };
      S.push(s);
    }
    const taskMs = async cdp => (await cdp.send('Performance.getMetrics')).metrics.find(m => m.name === 'TaskDuration').value * 1000;
    for (const it of INTERACTIONS) {
      for (let i = -1; i < samples; i++) { // i = -1: a warm-up per side (its first search/editor open ran apart from the rest)
        for (const mode of ['', '.cpu4']) {
          const order = (i + (mode ? 1 : 0)) % 2 ? [...S].reverse() : S;
          for (const s of order) {
            await it.prep(s, i);
            await quiet(s.page);
            const arg = it.arg ? it.arg(s) : null;
            await arm(s.page, it.done, arg);
            if (mode) await s.cdp.send('Emulation.setCPUThrottlingRate', { rate: 4 });
            const busy0 = mode ? 0 : await taskMs(s.cdp), req0 = s.page.__started;
            const t0 = performance.now();
            try {
              await it.act(s);
              await s.page.waitForFunction(() => window.__bcheck(), null, { polling: 'raf', timeout: 20000 });
            } finally {
              if (mode) await s.cdp.send('Emulation.setCPUThrottlingRate', { rate: 1 });
            }
            const node = performance.now() - t0, reqs = s.page.__started - req0, busy = mode ? 0 : await taskMs(s.cdp) - busy0;
            const [b0, b1] = await s.page.evaluate(() => [window.__bt0, window.__bt1]);
            if (i < 0) continue;
            put(s.name, `ui.${it.name}${mode}`, i, b0 !== undefined && b1 !== undefined ? b1 - b0 : node);
            if (!mode) { put(s.name, `ui.${it.name}.busy`, i, busy); put(s.name, `ui.${it.name}.reqs`, i, reqs); }
          }
        }
      }
      const show = (s, id) => out[s.name][id].flat().join('/');
      log(`  ui.${it.name}: ` + S.map(s => `${s.name} ${show(s, `ui.${it.name}`)} | cpu4 ${show(s, `ui.${it.name}.cpu4`)} | busy ${show(s, `ui.${it.name}.busy`)}`).join('  ||  '));
    }
  } finally {
    await browser.close();
  }
  return out;
}
