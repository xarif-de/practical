// axe over the calendar's views, the editor and its questions, the search (a11y/_axe.mjs).
import { DEMO, a11y, go, phone } from './_axe.mjs';

a11y('calendar views', async (page, check) => {
  for (const v of ['month', 'week', 'day', 'planner']) {
    await go(page, `/calendar?view=${v}&date=2026-10-14`, '#viewroot');
    await check(`cal-${v}`);
  }
});

a11y('editor: view, edit, new, the delete question, the colour popover', async (page, check, testInfo) => {
  await go(page, `/calendar?view=day&date=${DEMO[1]}`, '#viewroot');
  await page.locator(`article.lnk[data-key="${DEMO[0]}"] h3`).click();
  await page.locator('#ed:not([hidden])').waitFor();
  await check('ed-view');
  await page.locator('#ed-lock').click();
  await page.locator('#edform.mode-edit').waitFor();
  await check('ed-edit');
  await page.locator('#ed-color-btn').click();
  await page.locator('.cpk').waitFor();
  await check('ed-color-pop');
  await page.keyboard.press('Escape');
  await page.locator('#ed-delete').click();
  await page.locator('.ask').waitFor();
  await check('ed-ask');
  await page.keyboard.press('Escape');
  await page.locator('#ed-close').click();
  if (await page.locator('#ed-confirm:not([hidden])').count()) await page.locator('#ed-discard').click();
  await page.locator('#ed').waitFor({ state: 'hidden' });
  if (phone(testInfo)) await page.locator('#fab').click(); // the desktop's "+" of a day
  else { const h2 = page.locator('.doc h2'); await h2.hover(); await h2.locator('.add').click(); }
  await page.locator('#ed:not([hidden])').waitFor();
  await check('ed-new');
});

a11y('search: drop-down and all results', async (page, check) => {
  await go(page, '/calendar?view=month&date=2026-10-14', '#viewroot');
  await page.locator('#q').fill('anna');
  await page.locator('#sres .r').first().waitFor();
  await check('search-drop');
  await page.locator('#sres .r.more').click();
  await page.locator('.sdlg .sd-list .r').first().waitFor();
  await check('search-all');
});
