// Helpers of the settings/history/search/shell/login/empty visual specs (fork F4).
import { BASE } from '../lib/env.mjs';

/** Answers the autosave POST /settings/prefs in the browser ({ok:true}, no msg: the page shows its own
 *  localised "saved") — the UI after a save, nothing written. */
export async function fakePrefsSave(page) {
  await page.route(`${BASE}/settings/prefs`, route => route.request().method() === 'POST'
    ? route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ ok: true }) })
    : route.fallback());
}

/** A success toast fades after 3 s (app.js toast): kept on screen for the capture. */
export async function keepToasts(page) {
  await page.evaluate(() => { const st = window.setTimeout; window.setTimeout = (f, ms, ...a) => st(f, ms === 3000 ? 1e9 : ms, ...a); });
}

/** Switches the settings page by hash (same document, app.js `show`). */
export async function hash(page, h) {
  await page.evaluate(x => { location.hash = x; }, h);
  await page.locator(`#settings .page.on${h.startsWith('#pick-') ? '#pick' : h === '#' ? '#home' : CSS_ID(h)}`).waitFor();
}
const CSS_ID = h => '#' + h.slice(1).replace(/\./g, '\\.');
