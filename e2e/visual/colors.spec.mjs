// Colour picker family (inventory §2.4): popover (.cpk) from the editor and from the settings' collection
// pages, the new-colour dialog (.cpk-ask) in each tab and slider mode, the settings colours page and the
// optimiser dialog (.cfo-wrap). Read-only: nothing is picked for real (a pick in the editor stays in the form).
import { expect } from '../lib/test.mjs';
import { shot } from '../lib/visual.mjs';
import { EV, ds, openEvent, unlock } from './_editor.mjs';

const ARBEIT = 'alice-5eed0000-0000-4000-8000-000000000001', FREUNDE = 'alice-5eed0000-0000-4000-8000-000000000011';

shot('colour picker: editor popover and new-colour dialog', async (page, snap) => {
  const s = ds(page, snap);
  await openEvent(page, EV.zahnarzt);
  await unlock(page);
  await page.locator('#ed-color-btn').click();
  await page.locator('.cpk .cpk-sw').first().waitFor();
  await s('cpk-editor');
  await page.locator('.cpk .cpk-add').click();
  await page.locator('.cpk-ask .cpk-fanbox').waitFor();
  await s('cpk-new-fan');
  await page.locator('.cpk-ask .cpk-nav button').last().click();
  await s('cpk-new-fan-next');
  await page.locator('.cpk-ask .cpk-dh .seg button[data-t="field"]').click();
  for (const m of ['simple', 'rgb', 'oklch']) {
    await page.locator(`.cpk-ask .cpk-field .seg button[data-m="${m}"]`).click();
    await page.locator(`.cpk-ask .cpk-field .seg button[data-m="${m}"][aria-pressed="true"]`).waitFor();
    await s(`cpk-new-field-${m}`);
  }
  // an invalid text shows its error line
  await page.locator('.cpk-ask .cpk-in input').fill('kein-farbwert');
  await page.locator('.cpk-ask .cpk-err:not([hidden])').waitFor();
  await s('cpk-new-field-bad');
});

shot('colour picker: settings calendar and address book pages', async (page, snap) => {
  const s = ds(page, snap);
  for (const [name, id] of [['cal', `cal-${ARBEIT}`], ['ab', `ab-${FREUNDE}`]]) {
    await page.goto(`/settings#${id}`);
    const btn = page.locator(`section.page.on#${id} button.cpk-btn`);
    await btn.waitFor();
    await btn.click();
    await page.locator('.cpk .cpk-sw').first().waitFor();
    await s(`cpk-settings-${name}`);
  }
});

shot('colours page, a row popover and the optimiser @main', async (page, snap) => {
  const s = ds(page, snap);
  await page.goto('/settings#colors');
  await page.locator('#colors-list .cfo-opt').first().waitFor();
  await page.mouse.move(1, 1);
  await snap('cpk-colors-page');
  await page.locator('#colors-list button.cell.link[data-i]').first().click();
  await page.locator('.cpk .cpk-sw').first().waitFor();
  await s('cpk-colors-row');
  await page.keyboard.press('Escape');
  await page.locator('.cpk').waitFor({ state: 'detached' });
  await page.locator('#colors-list button.cell.link[data-o]').first().click();
  await page.locator('.cpk .cpk-sw').first().waitFor();
  await s('cpk-colors-offer');
  await page.keyboard.press('Escape');
  await page.locator('.cpk').waitFor({ state: 'detached' });
  // the task block exists only with coloured tasks or task-category colours (none in the fixture)
  for (const view of ['event', 'task']) {
    if (!(await page.locator(`#colors-list .cfo-opt[data-opt="${view}"]`).count())) continue;
    await page.locator(`#colors-list .cfo-opt[data-opt="${view}"]`).click();
    await page.locator('.cfo-wrap .cfo-body .cfo-level').waitFor();
    await s(`cpk-cfo-${view}`);
    if (view === 'event') {
      for (const lv of [0, 5]) {
        await page.locator('.cfo-wrap input[data-level]').fill(String(lv)); // the ticks are hidden on phones
        await page.locator(`.cfo-wrap [data-lv="${lv}"][aria-pressed="true"]`).waitFor();
        await s(`cpk-cfo-event-l${lv}`);
      }
    }
    await page.locator('.cfo-wrap .cfo-dh [data-x]').click();
    await page.locator('.cfo-wrap').waitFor({ state: 'detached' });
  }
});

shot('colour optimiser: a group dragged onto another action, the manual result', async (page, snap) => {
  // the drag's drop target, the split line, the manual marks and a hovered result had no state (e2e/theme-cover.mjs);
  // nothing is applied (the dialog is closed with ×)
  await page.goto('/settings#colors');
  await page.locator('#colors-list .cfo-opt[data-opt="event"]').click();
  await page.locator('.cfo-wrap .cfo-body .cfo-level').waitFor();
  const lanes = page.locator('.cfo-wrap .cfo-lane');
  await expect(lanes).toHaveCount(3);
  const centre = async l => { const b = await l.boundingBox(); return [b.x + b.width / 2, b.y + b.height / 2, b]; };
  const [sx, sy] = await centre(lanes.nth(0).locator('.cfo-src'));
  await page.mouse.move(sx, sy);
  await page.mouse.down();
  const [, ty, tb] = await centre(lanes.nth(2));
  await page.mouse.move(sx, ty, { steps: 4 }); // onto the third action: joins it
  await expect(lanes.nth(2)).toHaveClass(/\bdrop\b/);
  await snap('cpk-cfo-drag-join', { segments: 0 });
  await page.mouse.move(sx, tb.y + tb.height + 2, { steps: 2 }); // below its bottom edge: an action of its own
  await page.locator('.cfo-wrap .cfo-sep').waitFor();
  await snap('cpk-cfo-drag-split', { segments: 0 });
  await page.mouse.move(sx, ty, { steps: 2 });
  await page.mouse.up();
  await expect(page.locator('.cfo-wrap .cfo-lane.manual')).not.toHaveCount(0);
  await page.locator('.cfo-wrap .cfo-lane.manual .cfo-res').first().hover();
  await snap('cpk-cfo-moved', { segments: 0 });
  await page.locator('.cfo-wrap .cfo-dh [data-x]').click();
  await page.locator('.cfo-wrap').waitFor({ state: 'detached' });
});
