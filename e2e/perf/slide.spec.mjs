// What the view-switch slide costs (core.js practical.slide): each switch by its button, with the slide and without it
// (prefers-reduced-motion), alternating per run, at 4x CPU throttling. Per run over the 700 ms after the click: main
// thread busy time (CDP Performance TaskDuration), CPU time of all browser processes (CDP SystemInfo: renderer incl.
// compositor and raster threads, GPU, browser), main-thread frames (rAF) and the longest gap between two, long
// animation frames (≥ 50 ms) and their total. Not part of the gate: `E2E_SUITE=perf npx playwright test -g slide`.
// Result: one JSON line per switch and mode on stdout (medians of E2E_LAT_RUNS, default 10).
import { test, expect, settle } from '../lib/test.mjs';

const RUNS = Number(process.env.E2E_LAT_RUNS || 10), WINDOW = 700;
const median = a => { const s = [...a].sort((x, y) => x - y), n = s.length; return n % 2 ? s[(n - 1) / 2] : (s[n / 2 - 1] + s[n / 2]) / 2; };
const SWITCHES = [
  ['calendar month→week', '/calendar?view=month&date=2026-10-14', '.toolbar [data-view=week]', () => !!document.querySelector('#viewroot .stream .week.focus')],
  ['tasks groups→board', '/tasks?view=groups&back=all', '#tk-views [data-tkview=board]', () => !!document.querySelector('#tkmain .tkboard')],
  ['contacts list→table', '/contacts', '[data-ctview=table]', () => !!document.querySelector('#contacts.view-table')],
  ['history map→list', '/history?view=map', '.toolbar [data-view=list]', () => !!document.querySelector('.hxc-list')],
];

test('slide: cost of one view switch with and without the slide', async ({ page }) => {
  test.setTimeout(600_000);
  const cdp = await page.context().newCDPSession(page);
  await cdp.send('Performance.enable');
  const sys = await page.context().browser().newBrowserCDPSession();
  const cpu = async () => (await sys.send('SystemInfo.getProcessInfo')).processInfo.reduce((a, p) => a + p.cpuTime, 0) * 1000;
  const busy = async () => (await cdp.send('Performance.getMetrics')).metrics.find(m => m.name === 'TaskDuration').value * 1000;
  for (const [name, url, sel, done] of SWITCHES) {
    const out = { slide: [], none: [] };
    for (let i = 0; i < 2 * RUNS; i++) {
      const mode = i % 2 ? 'slide' : 'none';
      await cdp.send('Emulation.setCPUThrottlingRate', { rate: 1 });
      await page.emulateMedia({ reducedMotion: mode === 'slide' ? 'no-preference' : 'reduce' });
      await page.goto(url);
      await expect(page.locator(sel)).toBeVisible();
      await settle(page, 200);
      const b = await page.locator(sel).boundingBox();
      await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2);
      await settle(page, 200);
      await cdp.send('Emulation.setCPUThrottlingRate', { rate: 4 });
      await page.evaluate(w => { const r = window.__r = { frames: [], loaf: [], t0: performance.now() }; const f = t => { r.frames.push(t); if (t - r.t0 < w) requestAnimationFrame(f); }; requestAnimationFrame(f);
        new PerformanceObserver(l => r.loaf.push(...l.getEntries().filter(e => e.startTime >= r.t0).map(e => e.duration))).observe({ type: 'long-animation-frame' }); }, WINDOW);
      const b0 = await busy(), c0 = await cpu();
      await page.mouse.down(); await page.mouse.up();
      await page.waitForFunction(done, null, { polling: 'raf', timeout: 20000 });
      await page.waitForTimeout(WINDOW);
      const ms = await busy() - b0, cms = await cpu() - c0;
      const r = await page.evaluate(() => { const f = window.__r.frames, gaps = f.slice(1).map((t, i) => t - f[i]); return { frames: f.length, maxGap: Math.max(0, ...gaps), loaf: window.__r.loaf }; });
      out[mode].push({ busy: ms, cpu: cms, frames: r.frames, maxGap: r.maxGap, loafN: r.loaf.length, loafMs: r.loaf.reduce((a, x) => a + x, 0) });
    }
    for (const mode of ['none', 'slide']) {
      const m = k => Math.round(median(out[mode].map(x => x[k])) * 10) / 10;
      console.log(JSON.stringify({ name, mode, busyMs: m('busy'), cpuMs: m('cpu'), frames: m('frames'), maxGapMs: m('maxGap'), loafN: m('loafN'), loafMs: m('loafMs'), busyRuns: out[mode].map(x => Math.round(x.busy)), cpuRuns: out[mode].map(x => Math.round(x.cpu)) }));
    }
  }
});
