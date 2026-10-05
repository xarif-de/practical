// Change history (/history): line map (zoom 90/30/7, stop popover, undo question), filters, list + detail.
import { shot, isPhone } from '../lib/visual.mjs';
import { BASE } from '../lib/env.mjs';

// app.js fits the toolbar popovers into the screen at start and on `resize` — but history.js draws its toolbar
// after app.js started, so whether the filter popover is fitted depends on a resize event arriving (it does at
// random on a phone). One resize after load makes every run the same (the state after a rotation).
async function open(page, url) {
  await page.goto(url);
  await page.locator('#hx .toolbar').waitFor();
  await page.evaluate(() => dispatchEvent(new Event('resize')));
  await page.waitForTimeout(250); // history.js re-renders the map 150 ms after a resize
}
const stop = (page, id) => page.locator(`.hxg-stop[data-cid="${id}"]`);
// the popover covers the map on a phone (a sheet): closed before the next stop is clicked
async function closePop(page) {
  await page.locator('.hxg-pop [data-close]').click();
  await page.locator('.hxg-pop').waitFor({ state: 'hidden' });
}

shot('history map @main', async (page, snap) => {
  await open(page, '/history');
  await stop(page, 11).waitFor();
  await snap('hx-map');
  for (const z of [90, 7]) {
    await page.locator(`[data-r="zoom"] [data-zoom="${z}"]`).click();
    await snap(`hx-map-zoom-${z}`);
  }
});

shot('history map stop popovers', async (page, snap) => {
  await open(page, '/history');
  await stop(page, 9).click();
  await page.locator('.hxg-pop:not([hidden])').waitFor();
  await snap('hx-map-pop-update', { segments: 0 });
  await closePop(page);
  await stop(page, 10).click();
  await page.locator('.hxg-pop .hx-st.undone').waitFor();
  await snap('hx-map-pop-undone', { segments: 0 });
  await closePop(page);
  await stop(page, 1).click();
  await page.locator('.hxg-pop [data-undo="1"]').waitFor();
  await snap('hx-map-pop-bulk', { segments: 0 });
  // undo of #1 warns (later changes): the server answers 409 + plan; answered here from GET /plan, nothing written
  const plan = await (await page.request.get('/api/history/1/plan')).text();
  await page.route(`${BASE}/api/history/1/undo`, r => r.fulfill({ status: 409, contentType: 'application/json', body: plan }));
  await page.locator('.hxg-pop [data-undo="1"]').click();
  await page.locator('.ask').waitFor();
  await snap('hx-undo-ask', { segments: 0 });
  await page.keyboard.press('Escape');
  await page.locator('.ask').waitFor({ state: 'detached' });
});

shot('history filters', async (page, snap) => {
  await open(page, '/history');
  await stop(page, 11).waitFor();
  await page.locator('.hx-filter > button').click();
  await page.locator('.hx-filter.open').waitFor();
  await snap('hx-filter-pop', { segments: 0 });
  await page.locator('.hx-fpop input[data-f="srcs"][value="ui"]').check();
  await page.locator('.hx-fpop [data-fs="active"]').click();
  await page.locator('.hx-chips .hx-chip').nth(1).waitFor();
  await page.mouse.click(5, 300); // closes the popover
  await snap('hx-filter-chips');
  await page.locator('[data-f="q"]').fill('zzzz');
  await page.locator('.hx-empty').waitFor();
  await snap('hx-empty');
});

shot('history list @main', async (page, snap, testInfo) => {
  await open(page, '/history?view=list');
  await page.locator('.hxc-list [data-pick="11"]').waitFor();
  await page.locator('.hxc-card .hxc-pv b').first().waitFor({ state: 'attached' }); // the plan arrived (hidden beside the list on a phone)
  await snap('hx-list');
  for (const id of [9, 1, 10]) {
    await page.locator(`.hxc-list [data-pick="${id}"]`).click();
    await page.locator(`.hxc-card[data-cid="${id}"] .hxc-pv b`).waitFor({ state: 'attached' });
    await snap(`hx-list-detail-${id}`);
    if (isPhone(testInfo)) { await page.locator('.hxc-card [data-back]').click(); await page.locator('.hxc.show-d').waitFor({ state: 'detached' }); }
  }
});
