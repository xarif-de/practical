// Top-bar search: drop-down per query kind, history rows, ghost completion, all-results dialog, help popover.
// Read-only: no Enter on a query and no opened result (both record the query, app.js `record`).
import { shot, isPhone } from '../lib/visual.mjs';
import { test } from '../lib/test.mjs';

async function typeQ(page, v) {
  const q = page.locator('#q');
  await q.click();
  await q.fill(v);
  await page.locator('#sres:not([hidden])').waitFor();
  await page.waitForFunction(x => { const b = document.getElementById('sres'); return !b.hidden && (b.querySelector('.r.more') || b.querySelector('.none')) && document.getElementById('q').value === x; }, v);
}

shot('search drop-down', async (page, snap) => {
  await page.goto('/calendar?view=month&date=2026-10-14');
  await page.locator('#q').click();
  await page.locator('#sres .r.hist').first().waitFor();
  await snap('sr-focus-history', { segments: 0 });
  for (const [v, n] of [['zahn', 'sr-zahn'], ['anna', 'sr-anna'], ['14.10.', 'sr-date'], ['termin: review', 'sr-scope-ev'], ['kontakt: dora', 'sr-scope-ct'], ['aufgabe: bericht', 'sr-scope-tk'], ['xyzzyq', 'sr-none'], ['termin', 'sr-many']]) {
    await typeQ(page, v);
    await snap(n, { segments: 0 });
  }
  await typeQ(page, 'termin');
  await page.locator('#q').press('ArrowDown');
  await page.locator('#q').press('ArrowDown');
  await snap('sr-keyboard-pick', { segments: 0 });
});

shot('search ghost completion', async (page, snap) => {
  await page.goto('/calendar?view=month&date=2026-10-14');
  await page.locator('#q').click();
  await page.locator('#sres .r.hist').first().waitFor();
  await page.locator('#q').pressSequentially('zah');
  await page.locator('#q-ghost .t').waitFor();
  await page.locator('#sres .r.more').waitFor();
  await snap('sr-ghost', { segments: 0 });
});

shot('search all-results dialog', async (page, snap) => {
  await page.goto('/calendar?view=month&date=2026-10-14');
  await typeQ(page, 'a');
  await typeQ(page, 'anna');
  await page.locator('#sres .r.more').click();
  await page.locator('.sdlg .sd-tabs button').first().waitFor();
  await page.locator('.sdlg .sd-list .r').first().waitFor();
  await snap('sr-all', { segments: 0 });
  for (const cat of ['event', 'contact', 'task']) {
    const b = page.locator(`.sdlg .sd-tabs [data-cat="${cat}"]`);
    if (await b.isDisabled()) continue;
    await b.click();
    await page.locator(`.sdlg .sd-tabs [data-cat="${cat}"][aria-pressed="true"]`).waitFor();
    await snap(`sr-all-${cat}`, { segments: 0 });
  }
  await page.keyboard.press('Escape');
  await page.locator('.sdlg').waitFor({ state: 'detached' });
});

shot('search help popover', async (page, snap, testInfo) => {
  test.skip(isPhone(testInfo), 'the help button is not shown on a phone (style.css)');
  await page.goto('/calendar?view=month&date=2026-10-14');
  await page.locator('.shelp > button').hover();
  await page.locator('.shelp .pop').waitFor({ state: 'visible' });
  await snap('sr-help', { segments: 0 });
});
