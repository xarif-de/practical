// `shot(title, body)`: one visual test; `body(page, snap)` drives the UI and calls `snap(name)` per state: the page,
// settled and the pointer parked, against its reference screenshot (visual/__screenshots__/<project>/<name>.png,
// playwright.config.mjs; `--update-snapshots` re-blesses).
import { BASE } from './env.mjs';
import { expect, settle, test } from './test.mjs';

/** Holds the icon sprite's response until the requesting document has painted once after DOMContentLoaded.
 *  Chromium paints a page whose external `<use>` sprite arrived before its first paint differently from one
 *  whose sprite arrived after it (phone-dark/tk-timeline: 26 px of a dotted lane line one colour unit apart,
 *  DOM + style identical, not undone by a repaint; hiding the tab bar's icons removes the difference). Which
 *  came first depended on host load (the route below disables the HTTP cache: every document fetches the
 *  sprite); held, it is always "after" — the order a 400 ms delay of this response produced in 12 of 12 runs. */
async function spriteAfterFirstPaint(request) {
  const painted = request.frame().evaluate(() => new Promise(r => {
    const go = () => requestAnimationFrame(() => requestAnimationFrame(r));
    if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', go); else go();
  })).catch(() => {}); // the document went away: nothing left to order
  await Promise.race([painted, new Promise(r => setTimeout(r, 5000))]);
}

export function shot(title, body) {
  test(title, async ({ page }, testInfo) => {
    // read-only: the visual projects share one DB across parallel workers, so nothing may be written —
    // a write is aborted here (the app then shows its error, the test fails on the dialog/assert)
    const writes = [];
    await page.route(u => u.href.startsWith(BASE), async route => {
      const r = route.request();
      if (r.method() === 'GET' && new URL(r.url()).pathname === '/assets/icons.svg') await spriteAfterFirstPaint(r);
      if (r.method() === 'GET' || r.method() === 'HEAD') return route.fallback();
      // signing another user in (loginAs: empty states of carol) only changes this context's session
      if (r.method() === 'POST' && new URL(r.url()).pathname === '/login') return route.fallback();
      writes.push(`${r.method()} ${r.url()}`);
      return route.abort('blockedbyclient');
    });
    const names = new Set();
    const snap = async (name, opts = {}) => { // opts: toHaveScreenshot's (mask: what changes on its own, e.g. the version)
      if (names.has(name)) throw new Error(`duplicate state name ${name}`);
      names.add(name);
      await page.mouse.move(1, 1); // no hover state
      await settle(page);
      await expect(page).toHaveScreenshot(`${name}.png`, opts);
    };
    await body(page, snap, testInfo);
    if (page.__dialogs.length) throw new Error(`unexpected dialogs: ${page.__dialogs.join(' | ')}`);
    if (writes.length) throw new Error(`visual tests must not write: ${writes.join(', ')}`);
  });
}
export const isPhone = testInfo => testInfo.project.name.includes('phone');
