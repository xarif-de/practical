// Sharing in the editor: the visibility slider over the CLASS select, what sharees would see, and the question
// before a write through a share after which its writer sees less (the 422 round trip). erin writes into
// bob's "Team" through group.team (seed: bob → group.team, write); own data only ("F7 …" in November 2026).
import { api, expect, loginAs, settle, test } from '../lib/test.mjs';
import { answer, fillWhen, instances, openNew } from './_editor.mjs';

test.afterEach(async ({ page }) => {
  expect(page.__dialogs, 'no alert() from the app').toEqual([]);
});

const teamCopy = page => page.locator('#ed-cal option[value^="group.team/"]').getAttribute('value');

test('the slider sets CLASS; through a share it tells what sharees see and asks before hiding', async ({ page }) => {
  await loginAs(page, 'erin');
  await openNew(page, '2026-11-02');
  const cal = await teamCopy(page);
  expect(cal, 'erin has bob\'s Team as a copy').toBeTruthy();
  await fillWhen(page, { title: 'F7 Geteilt', from: '09:00', to: '10:00', cal });
  const vis = page.locator('#ed-vis'), out = vis.locator('#ed-vis-out'), hint = vis.locator('#ed-vis-hint');
  // untouched: shown as public, nothing set
  await expect(out).toHaveText('öffentlich');
  await expect(hint).toHaveText('Empfänger sehen: Alles');
  expect(await page.locator('#edform select[name="class"]').inputValue()).toBe('');
  await vis.locator('#ed-vis-range').fill('1');
  await expect(out).toHaveText('privat');
  await expect(hint).toHaveText('Empfänger sehen: Nur die Zeit');
  expect(await page.locator('#edform select[name="class"]').inputValue()).toBe('PRIVATE');
  // "back to editing": nothing saved, the dialog stays with its work
  await page.locator('#ed-save').click();
  await expect(page.locator('.ask')).toContainText('gekürzt');
  await answer(page, false);
  await expect(page.locator('#ed')).toBeVisible();
  await expect(page.locator('#ed-error')).toHaveText('');
  expect(await instances(page, '2026-11-02', 'F7 Geteilt')).toHaveLength(0);
  // "save anyway": written, and erin sees it shortened from now on
  await page.locator('#ed-save').click();
  await answer(page, true);
  await expect(page.locator('#ed')).toBeHidden();
  await settle(page);
  const day = await api(page, 'GET', '/api/events?start=2026-11-02&end=2026-11-03');
  expect(day.filter(e => e.summary === 'F7 Geteilt')).toHaveLength(0);
  expect(day.filter(e => `${e.principal}/${e.cal_id}` === cal), 'still there, as a time').toHaveLength(1);
});

test('an own calendar without shares: no hint, no question', async ({ page }) => {
  await openNew(page, '2026-11-03');
  await fillWhen(page, { title: 'F7 Eigen', from: '09:00', to: '10:00', cal: 'alice/5eed0000-0000-4000-8000-000000000001' });
  await expect(page.locator('#ed-vis-hint')).toBeHidden();
  await page.locator('#ed-vis-range').fill('2');
  await expect(page.locator('#ed-vis-out')).toHaveText('vertraulich');
  await page.locator('#ed-save').click();
  await expect(page.locator('#ed')).toBeHidden();
  await expect(page.locator('.ask')).toHaveCount(0);
  await settle(page);
  const e = (await instances(page, '2026-11-03', 'F7 Eigen'))[0];
  expect((await api(page, 'GET', `/api/events/${e.principal}/${e.cal_id}/${e.object_id}`)).class).toBe('CONFIDENTIAL');
});

test('rule mode: "beliebig" first, the setting off until its chip turns it on; the chip sits under the slider', async ({ page }) => {
  await page.goto('/calendar?rule=Urlaub');
  await page.locator('#ed:not([hidden])').waitFor();
  const range = page.locator('#ed-vis-range');
  await expect(page.locator('#ed-vis-out')).toHaveText('beliebig');
  await expect(range).toBeDisabled();
  const chip = page.locator('.rfoot[data-rfoot="when"] [data-rmode="class"]');
  await expect(chip).toBeVisible();
  await expect(page.locator('.rfoot[data-rfoot="status"] [data-rmode="class"]')).toHaveCount(0);
  await chip.click();
  await page.locator('.rrpop button[data-v="default"]').click();
  await expect(range).toBeEnabled();
  await expect(page.locator('#ed-vis-out')).toHaveText('öffentlich');
  await range.fill('-1');
  await expect(page.locator('#ed-vis-out')).toHaveText('beliebig');
});
