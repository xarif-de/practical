// Colour picker family end to end (inventory §2.4, §3 "Colours page / picker / optimiser / recolor"):
// a row's popover writes one element, an offer row every element of a colour, a category colour asks to
// recolour its events, the optimiser applies a plan. Own data only ("F2 …"); the optimiser's plan is
// narrowed to own elements (everything else excluded in the dialog), so seeded colours never change.
import { api, expect, settle, test } from '../lib/test.mjs';
import { CAL, answer, createApi, detail } from './_editor.mjs';

test.afterEach(async ({ page }) => {
  expect(page.__dialogs, 'no alert() from the app').toEqual([]);
});

const openColors = async page => {
  await page.goto('/settings#colors');
  await page.locator('#colors-list .cfo-opt').first().waitFor();
  await settle(page);
};
const row = (page, name) => page.locator('#colors-list button.cell.link[data-i]', { hasText: name });

test('a colours-page row recolours its element (PUT /api/colors)', async ({ page }) => {
  await page.goto('/calendar');
  const key = await createApi(page, { summary: 'F2 Zeile', start: '2026-11-19T09:00:00', end: '2026-11-19T10:00:00', color: '#7a3e9d' });
  await openColors(page);
  await row(page, 'F2 Zeile').click();
  const sw = page.locator('.cpk .cpk-sw:not(.cpk-add)').filter({ hasNot: page.locator('[aria-checked="true"]') });
  const target = page.locator('.cpk .cpk-sw[aria-checked="false"]:not(.cpk-add)').first();
  const hex = await target.getAttribute('data-v');
  const put = page.waitForResponse(r => r.url().includes('/api/colors/event/') && r.request().method() === 'PUT');
  await target.click();
  expect((await put).status()).toBe(204);
  expect((await detail(page, key)).color.toLowerCase()).toBe(hex);
  await expect(sw.first()).toBeVisible();
});

test('an offer row recolours every element of that colour', async ({ page }) => {
  await page.goto('/calendar');
  const a = await createApi(page, { summary: 'F2 Angebot A', start: '2026-11-20T09:00:00', end: '2026-11-20T10:00:00', color: '#5d4037' });
  const b = await createApi(page, { summary: 'F2 Angebot B', start: '2026-11-20T11:00:00', end: '2026-11-20T12:00:00', color: '#5d4037' });
  await openColors(page);
  const offer = page.locator('#colors-list button.cell.link[data-o]', { hasText: '#5d4037' });
  await expect(offer).toHaveCount(1);
  await offer.click();
  // ＋ → the new-colour dialog; its text field takes an exact colour
  await page.locator('.cpk .cpk-add').click();
  await page.locator('.cpk-ask .cpk-dh .seg button[data-t="field"]').click();
  await page.locator('.cpk-ask .cpk-in input').fill('#2e7d32');
  await page.locator('.cpk-ask [data-ok]').click();
  await expect(page.locator('.cpk-ask')).toHaveCount(0);
  await expect.poll(async () => [(await detail(page, a)).color, (await detail(page, b)).color]).toEqual(['#2e7d32', '#2e7d32']);
  await page.keyboard.press('Escape');
  await openColors(page);
  await expect(page.locator('#colors-list button.cell.link[data-o]', { hasText: '#5d4037' })).toHaveCount(0);
});

test('a category colour asks to recolour its events', async ({ page }) => {
  await page.goto('/calendar');
  await api(page, 'PUT', '/api/category-rules/F2-Farbe?scope=event', { when: { color: { mode: 'default', value: '#8d6e63' } } });
  try {
    const key = await createApi(page, { summary: 'F2 Kategorie', start: '2026-11-24T09:00:00', end: '2026-11-24T10:00:00', color: '#8d6e63', categories: ['F2-Farbe'] });
    await openColors(page);
    await row(page, 'F2-Farbe').click();
    await page.locator('.cpk .cpk-add').click();
    await page.locator('.cpk-ask .cpk-dh .seg button[data-t="field"]').click();
    await page.locator('.cpk-ask .cpk-in input').fill('#00695c');
    await page.locator('.cpk-ask [data-ok]').click();
    await expect(page.locator('.ask p')).toContainText('F2-Farbe');
    await answer(page, true);
    await expect.poll(async () => (await detail(page, key)).color).toBe('#00695c');
    const rules = await api(page, 'GET', '/api/category-rules?scope=event');
    expect(rules['F2-Farbe'].when.color).toEqual({ mode: 'default', value: '#00695c' });
  } finally {
    await api(page, 'DELETE', '/api/category-rules/F2-Farbe?scope=event');
  }
});

test('the optimiser applies its plan (narrowed to own elements) in one batch', async ({ page }) => {
  await page.goto('/calendar');
  const a = await createApi(page, { summary: 'F2 Opt A', start: '2026-11-25T09:00:00', end: '2026-11-25T10:00:00', color: '#c2185b', cal: CAL.privat });
  const b = await createApi(page, { summary: 'F2 Opt B', start: '2026-11-25T11:00:00', end: '2026-11-25T12:00:00', color: '#c4195d', cal: CAL.privat });
  const before = await api(page, 'GET', '/api/colors');
  await openColors(page);
  await page.locator('#colors-list .cfo-opt[data-opt="event"]').click();
  const dlg = page.locator('.cfo-wrap');
  await dlg.locator('.cfo-level').waitFor();
  // exclude every group that is not ours (the dialog redraws after each click)
  for (let i = 0; i < 100; i++) {
    const other = dlg.locator('.cfo-src:not(.excl):not(.ro)').filter({ hasNot: page.locator('b', { hasText: /^F2 / }) }).locator('[data-excl]');
    if (!(await other.count())) break;
    await other.first().click();
  }
  await expect(dlg.locator('.cfo-src:not(.excl):not(.ro) b').first()).toHaveText(/^F2 /);
  const apply = dlg.locator('[data-apply]');
  await expect(apply).toBeEnabled();
  const req = page.waitForRequest(r => r.url().endsWith('/api/colors/batch'));
  await apply.click();
  const body = (await req).postDataJSON();
  expect(body.changes.length).toBeGreaterThan(0);
  // only own elements ("F2 …", also those of the tests before) are in the batch
  for (const c of body.changes) {
    expect(c.kind).toBe('event');
    expect((await detail(page, c.key)).summary).toMatch(/^F2 /);
  }
  expect(body.changes.map(c => c.key)).toEqual(expect.arrayContaining([expect.stringMatching(new RegExp(`^(${a}|${b})$`))]));
  await expect(dlg).toHaveCount(0);
  const [ca, cb] = [(await detail(page, a)).color, (await detail(page, b)).color];
  expect(ca).toBe(cb); // the two near-identical reds became one colour
  // nothing else changed
  const after = await api(page, 'GET', '/api/colors');
  const strip = l => l.filter(s => !/^F2 /.test(s.name)).map(s => JSON.stringify(s)).sort();
  expect(strip(after)).toEqual(strip(before));
});

test('the optimiser dialog: levels, cancel writes nothing', async ({ page }) => {
  await openColors(page);
  let writes = 0;
  page.on('request', r => { if (r.method() !== 'GET') writes++; });
  await page.locator('#colors-list .cfo-opt[data-opt="event"]').click();
  const dlg = page.locator('.cfo-wrap');
  await dlg.locator('.cfo-level').waitFor();
  for (const lv of ['0', '5', '3']) {
    await dlg.locator('input[data-level]').fill(lv);
    await expect(dlg.locator(`[data-lv="${lv}"]`)).toHaveAttribute('aria-pressed', 'true');
  }
  await dlg.locator('.ask-acts [data-x]').click();
  await expect(dlg).toHaveCount(0);
  expect(writes).toBe(0);
});
