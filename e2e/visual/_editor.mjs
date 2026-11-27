// Editor helpers of the visual check (visual/key.spec.mjs).
import { settle } from '../lib/test.mjs';

const CAL = { work: 'alice/5eed0000-0000-4000-8000-000000000001' };
export const EV = { review: [`${CAL.work}/review@seed`, '2026-10-14'] };

/** Waits until the editor dialog is open and its day line has drawn. */
async function edReady(page) {
  await page.locator('#ed:not([hidden])').waitFor();
  await settle(page);
}

/** Opens an event from the day view's document (both viewports): its title in the entry. */
export async function openEvent(page, [key, date]) {
  await page.goto(`/calendar?view=day&date=${date}`);
  await page.locator(`article.lnk[data-key="${key}"][data-date="${date}"] h3`).click();
  await edReady(page);
}

export async function unlock(page) {
  await page.locator('#ed-lock').click();
  await page.locator('#edform.mode-edit').waitFor();
  await page.locator('#edh').blur();
}
