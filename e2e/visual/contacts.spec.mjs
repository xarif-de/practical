// Contacts page (inventory §2.6): list, table, filter/picker popovers, the contact editor in view and
// edit mode, the table's dialogs. Read-only (lib/visual.mjs aborts writes).
import { expect } from '../lib/test.mjs';
import { isPhone, shot } from '../lib/visual.mjs';

const AB = 'alice/5eed0000-0000-4000-8000-000000000011';
const ANNA = `${AB}/c-anna@seed`;
const dialog = { segments: 0 };

async function list(page) {
  await page.goto('/contacts');
  await expect(page.locator('.list li[data-key]').first()).toBeVisible();
}
async function table(page) {
  await list(page);
  await page.click('[data-ctview=table]');
  await expect(page.locator('#contacts.view-table')).toBeVisible();
}
// phones show the contact full-screen; #detail scrolls on its own: its lower half is a state too
async function detailEnd(page, snap, testInfo, name) {
  if (!isPhone(testInfo)) return;
  await page.locator('#detail').evaluate(d => { d.scrollTop = d.scrollHeight; });
  await snap(name, dialog);
  await page.locator('#detail').evaluate(d => { d.scrollTop = 0; });
}
async function openContact(page, name) {
  await page.click(`.list li[data-name="${name}"]`);
  await expect(page.locator('#detail #ctform input[name=full_name]')).toHaveValue(name);
}

shot('contacts list @main', async (page, snap) => {
  await list(page);
  await snap('ct-list');
});

shot('contacts list: sort by birthday, descending', async (page, snap) => {
  await list(page);
  await page.selectOption('#ct-sort', 'bday');
  await page.click('#ct-dir');
  await expect(page.locator('#ct-dir use')).toHaveAttribute('href', '#i-sort-desc');
  await snap('ct-list-sort-bday-desc');
});

shot('contacts list: fields picker and filter popover', async (page, snap) => {
  await list(page);
  await page.click('#colpick > button');
  await expect(page.locator('#colpop [data-for=list]')).toBeVisible();
  await snap('ct-list-fields-pop', dialog);
  await page.click('.toolbar h1'); // outside: closes it
  await page.click('.topbar .filter:has(#abfilter) > button');
  await expect(page.locator('#abfilter')).toBeVisible();
  await snap('ct-filter-pop', dialog);
  // a category off (client-side filter, nothing is saved)
  await page.locator('#abfilter input[data-cat="Freunde"]').uncheck();
  await snap('ct-filter-cat-off', dialog);
});

shot('contacts: rich card in view and edit mode', async (page, snap, testInfo) => {
  await list(page);
  await openContact(page, 'Anna Albrecht');
  await snap('ct-anna-view', isPhone(testInfo) ? dialog : {});
  await detailEnd(page, snap, testInfo, 'ct-anna-view-end');
  await page.click('#ct-lock');
  await expect(page.locator('#ctform.mode-edit')).toBeVisible();
  await snap('ct-anna-edit', isPhone(testInfo) ? dialog : {});
  await detailEnd(page, snap, testInfo, 'ct-anna-edit-end');
  await page.click('#ctform [data-more]');
  await page.click('#ctform .addmenu > button');
  await expect(page.locator('#ctform .addmenu .pop')).toBeVisible();
  await snap('ct-anna-edit-morenames-addmenu', dialog);
  // removing a card asks first (the app's own question), Escape answers "no"
  await page.click('#ctform .card[data-card="note"] [data-card-rm]');
  await expect(page.locator('.ask')).toBeVisible();
  await snap('ct-anna-card-rm-ask', dialog);
  await page.keyboard.press('Escape');
  await expect(page.locator('.ask')).toHaveCount(0);
});

shot('contacts: sparse cards', async (page, snap, testInfo) => {
  await list(page);
  await openContact(page, 'Ben Albrecht'); // birthday without year: "in 6 days", no age
  await snap('ct-ben-view', isPhone(testInfo) ? dialog : {});
  if (isPhone(testInfo)) await page.click('#ct-cancel');
  await openContact(page, 'Hausverwaltung GmbH'); // organisation only
  await snap('ct-org-view', isPhone(testInfo) ? dialog : {});
});

shot('contacts: deep link reveals a contact', async (page, snap, testInfo) => {
  await page.goto(`/contacts#${AB}/c-zora@seed`);
  await expect(page.locator('#detail #ctform input[name=full_name]')).toHaveValue('Zora Zimmermann');
  await snap('ct-deeplink-zora', isPhone(testInfo) ? dialog : {});
});

shot('contacts: letter rail jump', async (page, snap, testInfo) => {
  if (isPhone(testInfo)) return; // the rail exists ≥1100 only
  await list(page);
  await page.click('.ct-rail [data-jump="H"]');
  await expect.poll(() => page.locator('main.main').evaluate(m => m.scrollTop)).toBeGreaterThan(0);
  await snap('ct-rail-h', { segments: 0 });
});

shot('contacts table', async (page, snap) => {
  await table(page);
  await snap('ct-table');
  await page.click('#colpick > button');
  await expect(page.locator('#colpop [data-for=table]')).toBeVisible();
  await snap('ct-table-colpick', dialog);
});

shot('contacts table: dialogs', async (page, snap) => {
  await table(page);
  // a card cell (categories) opens just that card, unlocked, as a dialog
  await page.click(`tr[data-key="${ANNA}"] td[data-c="category"]`);
  await expect(page.locator('#contacts.dlg.solo')).toBeVisible();
  await snap('ct-table-solo-cats', dialog);
  await page.keyboard.press('Escape');
  await expect(page.locator('#contacts.dlg')).toHaveCount(0);
  // + opens the new-contact form as a dialog
  await page.click('#ct-fab');
  await expect(page.locator('#ctform.new')).toBeVisible();
  await snap('ct-table-new', dialog);
});

shot('contacts: new contact form', async (page, snap, testInfo) => {
  await list(page);
  if (isPhone(testInfo)) {
    // phones add from the table view (its + button)
    await page.click('[data-ctview=table]');
    await page.click('#ct-fab');
  }
  await expect(page.locator('#ctform.new')).toBeVisible();
  await page.fill('#ctform input[name=given]', 'Neue');
  await page.fill('#ctform input[name=family]', 'Person');
  await expect(page.locator('#ctform input[name=full_name]')).toHaveValue('Neue Person');
  // measured: after typing, the bottom-right corner of the phone row's native <select> (it sits at
  // y = 241.625 px) was antialiased differently in 3 of 8 runs (1 px, desktop only; a blur and a full
  // repaint did not help). The same select is captured unmasked, untyped, in ct-list.
  await page.locator('#ctform input[name=family]').blur();
  await snap('ct-new-typed', { ...dialog, mask: isPhone(testInfo) ? [] : [page.locator('#ctform [data-list=phones] select.typ')] });
});
