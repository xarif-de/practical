// Package transport (2026-10): the week view names an entry's date once, on its day block (render.rs week_blocks,
// audit3-perf3 #9); an entry carries its own data-date only on the later days of a multi-day event. The editor
// still opens the clicked event (an existing event opens at its own start: openEditor ignores the date then). Own
// data only ("T9 …" in November 2026).
import { expect, settle, test } from '../lib/test.mjs';
import { createApi } from './_editor.mjs';

test('week entries name their date once, on the day block; a click still opens the event', async ({ page }) => {
  await page.goto('/calendar?view=week&date=2026-11-04'); // the session + CSRF token the API calls use
  await createApi(page, { summary: 'T9 Serie', start: '2026-11-02T10:00:00', end: '2026-11-02T11:00:00', rrule: 'FREQ=DAILY;COUNT=4' });
  await createApi(page, { summary: 'T9 Messe', start: '2026-11-03T00:00:00', end: '2026-11-06T00:00:00', all_day: true });
  await page.goto('/calendar?view=week&date=2026-11-04');
  await settle(page);
  const ev = (day, t) => page.locator(`#viewroot .stream .dayblock[data-date="${day}"] .ev`, { hasText: t });
  const opened = async (day, t, want) => {
    await ev(day, t).click();
    await page.locator('#ed:not([hidden])').waitFor();
    await expect(page.locator('#edh')).toHaveValue(t);
    await expect(page.locator('#edform input[name="date"]')).toHaveValue(want);
    await page.keyboard.press('Escape');
    await expect(page.locator('#ed')).toBeHidden();
  };
  for (const day of ['2026-11-02', '2026-11-03', '2026-11-04', '2026-11-05']) {
    await expect(ev(day, 'T9 Serie')).not.toHaveAttribute('data-date', /./);
    await opened(day, 'T9 Serie', '2026-11-02');
  }
  await expect(ev('2026-11-03', 'T9 Messe')).not.toHaveAttribute('data-date', /./);
  for (const day of ['2026-11-04', '2026-11-05']) {
    await expect(ev(day, 'T9 Messe')).toHaveAttribute('data-date', '2026-11-03');
    await opened(day, 'T9 Messe', '2026-11-03');
  }
  await opened('2026-11-03', 'T9 Messe', '2026-11-03');
});

test.describe('phone', () => {
  test.use({ viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, isMobile: true, hasTouch: true });
  // grid.css hides `.ev` below 720px for the month's dots; the week list showed bare times until chrome.css named it
  test('the week list shows its entries on a phone, a long title cut inside the screen', async ({ page }) => {
    await page.goto('/calendar?view=week&date=2026-11-25');
    await createApi(page, { summary: 'T9 Handy mit einem sehr langen Titel, der nie in eine Zeile eines Telefons passt', start: '2026-11-25T09:00:00', end: '2026-11-25T10:00:00' });
    await page.goto('/calendar?view=week&date=2026-11-25');
    await settle(page);
    const ev = page.locator('#viewroot .stream .dayblock[data-date="2026-11-25"] .ev', { hasText: 'T9 Handy' });
    await expect(ev).toBeVisible();
    const box = await ev.boundingBox();
    expect(box.x + box.width).toBeLessThanOrEqual(390);
    expect(await page.locator('#viewroot .cal').evaluate(e => e.getBoundingClientRect().right)).toBeLessThanOrEqual(390);
  });
});
