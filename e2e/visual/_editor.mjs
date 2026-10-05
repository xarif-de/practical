// Helpers of the editor/colour visual specs (F2).
import { settle } from '../lib/test.mjs';

export const CAL = { work: 'alice/5eed0000-0000-4000-8000-000000000001', privat: 'alice/5eed0000-0000-4000-8000-000000000002' };
export const EV = {
  review: [`${CAL.work}/review@seed`, '2026-10-14'],
  demo: [`${CAL.privat}/demo-event-full@xarif.de`, '2026-10-13'],
  zahnarzt: [`${CAL.privat}/zahnarzt@seed`, '2026-10-14'],
  standup: [`${CAL.work}/standup@seed`, '2026-10-14'],
  urlaub: [`${CAL.privat}/urlaub-ostsee@seed`, '2026-10-26'],
};

/** Waits until the editor dialog is open and its day line has drawn. */
export async function edReady(page) {
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

/** New event: desktop through a month day's add button, phone through the FAB (day = today). */
export async function openNew(page, phone, date = '2026-10-16') {
  if (phone) {
    await page.goto('/calendar?view=month&date=2026-10-14');
    await page.locator('#fab').click();
  } else {
    await page.goto(`/calendar?view=month&date=${date}`);
    const day = page.locator(`.v-month .day[data-date="${date}"]`);
    await day.hover();
    await day.locator('.add').click();
  }
  await edReady(page);
}

export const neutral = page => page.mouse.move(1, 1);

/** The dialog state plus one more screenshot per screenful the dialog box scrolls (`<name>-b<n>`). */
export async function edSnap(page, snap, name, { pointer = true } = {}) {
  if (pointer) await neutral(page);
  await snap(name, { segments: 0 });
  const box = page.locator('#ed .box');
  for (let i = 1; i <= 4; i++) {
    const more = await box.evaluate(b => { if (b.scrollTop + b.clientHeight >= b.scrollHeight - 2) return false; b.scrollTop += b.clientHeight; return true; });
    if (!more) break;
    await snap(`${name}-b${i}`, { segments: 0 });
  }
  await box.evaluate(b => { b.scrollTop = 0; });
}

/** Scrolls the dialog box so `sel` sits at its top (sub-dialog states inside long edit forms). */
export async function edScrollTo(page, sel) {
  await page.locator(sel).first().evaluate(el => {
    const box = el.closest('.box'); box.scrollTop += el.getBoundingClientRect().top - box.getBoundingClientRect().top - 90;
  });
}

/** A dialog state, pointer parked outside (hover-revealed menus such as the add-card menu close). */
export const ds = (page, snap) => async name => { await neutral(page); await snap(name, { segments: 0 }); };

/** After a rAF-driven animation (the location field's countdown ring) parts of the dialog were last rastered
 *  mid-animation: their blurred backdrop differed by ±1 per channel from run to run (measured, 4 runs).
 *  Dropping and restoring the overlay's backdrop filter re-rasters it whole. The ring's last countdown value
 *  (an inline style on the hidden ring, timing-dependent) is set to its end value: invisible either way. */
export async function rasterAgain(page) {
  await page.evaluate(async () => {
    const frame = () => new Promise(r => requestAnimationFrame(() => r()));
    const c = document.querySelector('#ed-ring[hidden] circle'); if (c && c.style.strokeDashoffset) c.style.strokeDashoffset = '50.27';
    const ed = document.getElementById('ed'); ed.style.backdropFilter = 'none'; await frame(); await frame();
    ed.style.backdropFilter = ''; if (!ed.getAttribute('style')) ed.removeAttribute('style'); await frame(); await frame();
  });
}
