// Top-bar search: drop-down, keyboard, history (record, completion, forget), all-results dialog, scopes.
import { test, expect } from '../lib/test.mjs';
import { BASE } from '../lib/env.mjs';

const hist = async page => (await (await page.request.get('/api/search/history')).json()).map(e => e.q);
async function results(page, v) {
  const q = page.locator('#q');
  await q.click();
  await q.fill(v);
  await expect(page.locator('#sres .r.more')).toContainText(/\(\d+\)/);
}

test('typing shows the sections; a scope prefix keeps one; Escape closes', async ({ page }) => {
  await page.goto('/settings');
  await results(page, 'albrecht');
  await expect(page.locator('#sres h5')).toContainText([/Kontakte/]);
  await expect(page.locator('#sres a.r:not(.more):not(.hist)')).toHaveCount(2);
  await expect(page.locator('#sres mark').first()).toHaveText(/albrecht/i);
  await results(page, 'review');
  await expect(page.locator('#sres h5').first()).toContainText('Termine');
  await results(page, 'kontakt: anna'); // the de prefixes (search-scope-*): termin / kontakt / aufgabe
  await expect(page.locator('#sres .sscope')).toBeVisible();
  await expect(page.locator('#sres h5')).toHaveCount(1);
  await page.locator('#q').press('Escape');
  await expect(page.locator('#sres')).toBeHidden();
});

test('a date query offers the jump; picking it opens that day', async ({ page }) => {
  await page.goto('/settings');
  await results(page, '24.10.2026');
  await expect(page.locator('#sres .r.jump')).toBeVisible();
  await page.locator('#sres .r.jump').click();
  await expect(page).toHaveURL(/\/calendar\?view=day&date=2026-10-24/);
});

test('↓ + Enter opens a result and records the query; Tab follows the completion; × forgets it', async ({ page }) => {
  await page.goto('/settings');
  await results(page, 'Radtour');
  const q = page.locator('#q');
  await q.press('ArrowDown');
  await expect(page.locator('#sres .r.on')).toContainText('Radtour');
  await Promise.all([page.waitForURL(/\/calendar\?view=day&date=2026-10-24/), q.press('Enter')]);
  await expect.poll(() => hist(page)).toContain('Radtour');
  // the completion: the start of a remembered query, Tab goes to its result
  await page.goto('/contacts');
  await page.locator('#q').click();
  await page.locator('#q').pressSequentially('Radt');
  await expect(page.locator('#q-ghost')).toHaveText('Radtour');
  await page.locator('#q').press('ArrowRight');
  await expect(page.locator('#q')).toHaveValue('Radtour');
  await page.locator('#q').fill('Radt');
  await page.locator('#q').press('End');
  await expect(page.locator('#q-ghost')).toHaveText('Radtour');
  await Promise.all([page.waitForURL(/\/calendar\?view=day&date=2026-10-24/), page.locator('#q').press('Tab')]);
  // forget it from the drop-down's history rows
  await page.locator('#q').click();
  const row = page.locator('#sres .r.hist', { hasText: 'Radtour' });
  await expect(row).toBeVisible();
  await row.hover();
  await row.locator('[data-hdel]').click();
  await expect(page.locator('#sres .r.hist', { hasText: 'Radtour' })).toHaveCount(0);
  expect(await hist(page)).not.toContain('Radtour');
});

test('Enter without a pick: all-results dialog with tabs, keyboard pick opens the contact', async ({ page }) => {
  await page.goto('/settings');
  await results(page, 'albrecht');
  await page.locator('#q').press('Enter');
  const dlg = page.locator('.sdlg');
  await expect(dlg).toBeVisible();
  await expect(dlg.locator('.sd-tabs [data-cat="all"] .n')).toHaveText('2');
  await expect(dlg.locator('.sd-tabs [data-cat="event"]')).toBeDisabled();
  await dlg.locator('.sd-tabs [data-cat="contact"]').click();
  await expect(dlg.locator('.sd-list .r')).toHaveCount(2);
  await expect(dlg.locator('.sd-list .why').first()).toBeVisible();
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('ArrowDown');
  await expect(dlg.locator('.sd-list .r.on')).toHaveCount(1);
  const name = (await dlg.locator('.sd-list .r.on > b').textContent()).trim();
  await Promise.all([page.waitForURL(/\/contacts#/), page.keyboard.press('Enter')]);
  expect(decodeURIComponent(page.url())).toContain(name.startsWith('Ben') ? 'c-ben@seed' : 'c-anna@seed'); // the contact key: UID = object id
  await expect(page.locator('#detail')).toContainText(name.startsWith('Ben') ? 'BA' : 'AA');
  await expect.poll(() => hist(page)).toContain('albrecht');
  // Escape closes the dialog without a pick
  await results(page, 'termin');
  await page.locator('#sres .r.more').click();
  await expect(page.locator('.sdlg')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.locator('.sdlg')).toHaveCount(0);
});

test('/ and Ctrl+K put the cursor in the search field', async ({ page }) => {
  await page.goto('/tasks');
  await page.locator('body').click({ position: { x: 5, y: 300 } });
  await page.keyboard.press('/');
  await expect(page.locator('#q')).toBeFocused();
  await page.locator('#q').fill('xyz');
  await expect(page.locator('#sres .none')).toBeVisible();
  await page.keyboard.press('Escape'); // 1st: closes the drop-down (keyboard, not locator.press: that would focus again)
  await expect(page.locator('#sres')).toBeHidden();
  await expect(page.locator('#q')).toBeFocused();
  await page.keyboard.press('Escape'); // 2nd: leaves the field
  await expect(page.locator('#q')).not.toBeFocused();
  await page.keyboard.press('Control+k');
  await expect(page.locator('#q')).toBeFocused();
  // Ctrl+K selects what is in the field (Escape in a type=search field emptied it above)
  await page.locator('#q').fill('xyz');
  await page.locator('body').click({ position: { x: 5, y: 300 } });
  await expect(page.locator('#q')).not.toBeFocused();
  await page.keyboard.press('Control+k');
  expect(await page.evaluate(() => { const q = document.getElementById('q'); return [q.selectionStart, q.selectionEnd]; })).toEqual([0, 3]);
});
