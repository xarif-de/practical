// The monkey's move to its next page (monkey.spec.mjs). The page before can still navigate on its own while that goto
// runs: a request of its last step answered 401 (an injected one) sends the app to the login (req.js authLost → /login →
// 303 → /calendar), and that navigation aborts ours. Playwright names it "interrupted by another navigation" or
// net::ERR_ABORTED. Trace of a failing seed-2 run (2026-11-03, VM; the same finding natively on 2026-11-02): goto day at
// +0 ms, the old page's debounced /api/search answered 401 at +20 ms, goto ERR_ABORTED at +22 ms; the retry, sent at
// +29 ms while the app's own navigation was still loading, issued no request at all and timed out after 15 s. So:
// after an abort, wait for that navigation's `load`, then go again. goto.spec.mjs replays the race deterministically.
const ABORTED = /interrupted by another navigation|net::ERR_ABORTED/;

/** page.goto(url) that survives a navigation the page starts itself meanwhile (3 attempts). */
export async function gotoPage(page, url, { timeout = 15000 } = {}) {
  let loads = 0;
  const onLoad = () => { loads++; };
  page.on('load', onLoad);
  try {
    for (let i = 0; ; i++) {
      const before = loads;
      try { await page.goto(url, { timeout }); return; } catch (e) {
        if (i === 2 || !ABORTED.test(e.message)) throw e;
        // the page's own navigation took over: let it load (its `load` event, 10 s at most) before going again
        if (loads === before) await page.waitForEvent('load', { timeout: 10000 }).catch(() => {});
      }
    }
  } finally {
    page.off('load', onLoad);
  }
}
