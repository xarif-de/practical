// `shot(title, body)`: one visual test; `body(page, snap)` drives the UI and calls `snap(name)` per state.
// Tag a title with @main to also capture it in the en projects.
import { capture } from './capture.mjs';
import { BASE } from './env.mjs';
import { test } from './test.mjs';

export function shot(title, body, opts = {}) {
  test(title, async ({ page }, testInfo) => {
    // read-only: the visual projects share one DB across parallel workers, so nothing may be written —
    // a write is aborted here (the app then shows its error, the test fails on the dialog/assert)
    const writes = [];
    await page.route(u => u.href.startsWith(BASE), route => {
      const r = route.request();
      if (r.method() === 'GET' || r.method() === 'HEAD') return route.fallback();
      // signing another user in (loginAs: empty states of carol) only changes this context's session
      if (r.method() === 'POST' && new URL(r.url()).pathname === '/login') return route.fallback();
      writes.push(`${r.method()} ${r.url()}`);
      return route.abort('blockedbyclient');
    });
    const names = new Set();
    const snap = async (name, o) => {
      if (names.has(name)) throw new Error(`duplicate state name ${name}`);
      names.add(name);
      await capture(page, testInfo, name, { ...opts, ...o });
    };
    await body(page, snap, testInfo);
    if (page.__dialogs.length) throw new Error(`unexpected dialogs: ${page.__dialogs.join(' | ')}`);
    if (writes.length) throw new Error(`visual tests must not write: ${writes.join(', ')}`);
  });
}
export const isPhone = testInfo => testInfo.project.name.includes('phone');
