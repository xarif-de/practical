// Contacts page, end to end (inventory §2.6, §3 contacts rows). Every write targets contacts this file
// creates (prefix "F3 "); picker choices are restored. Outcomes are read back after a reload and via the API.
import { test, expect, api } from '../lib/test.mjs';

const AB = 'alice/5eed0000-0000-4000-8000-000000000011'; // "Freunde" (fixture)
const ANNA = `${AB}/c-anna@seed`;

const li = (page, name) => page.locator(`.list li[data-name="${name}"]`);
const waitFor = (page, method, re) => page.waitForResponse(r => r.request().method() === method && re.test(new URL(r.url()).pathname));

async function list(page) {
  await page.goto('/contacts');
  await expect(page.locator('.list li[data-key]').first()).toBeVisible();
}
async function table(page) {
  await page.goto('/contacts?view=table');
  await expect(page.locator('#contacts.view-table')).toBeVisible();
}
/** A contact through the API (setup for tests that exercise something else); returns its key. */
async function seedContact(page, given, family, extra = {}) {
  if (!page.url().includes('/contacts')) await list(page);
  const { id } = await api(page, 'POST', '/api/contacts', { principal: 'alice', ab_id: AB.split('/')[1], given, family, full_name: `${given} ${family}`, ...extra });
  return `${AB}/${id}`;
}
async function openContact(page, name) {
  await li(page, name).click();
  await expect(page.locator('#ctform input[name=full_name]')).toHaveValue(name);
}
async function yes(page) {
  await expect(page.locator('.ask')).toBeVisible();
  await page.click('.ask [data-a="1"]');
}

test('create a contact in the editor: name, phone, e-mail, birthday', async ({ page }) => {
  await list(page);
  await expect(page.locator('#ctform.new')).toBeVisible(); // ≥1100: the new-contact form sits beside the list
  await page.fill('#ctform input[name=given]', 'F3 Clara');
  await page.fill('#ctform input[name=family]', 'Neumann');
  await expect(page.locator('#ctform input[name=full_name]')).toHaveValue('F3 Clara Neumann');
  await page.fill('#ctform [data-list=phones] .row input[type=tel]', '+49 170 5550001');
  await page.click('#ctform .addmenu > button');
  await page.click('#ctform [data-card-add=email]');
  await page.fill('#ctform [data-list=emails] .row input[type=email]', 'clara@example.org');
  await page.click('#ctform .addmenu > button');
  await page.click('#ctform [data-card-add=dates]');
  const bd = page.locator('#ctform [data-date=birthday]');
  await bd.locator('[data-p=d]').fill('03');
  await bd.locator('[data-p=m]').fill('11');
  await bd.locator('[data-p=y]').fill('1990');
  const saved = waitFor(page, 'POST', /^\/api\/contacts$/);
  await page.click('#ct-save');
  expect((await saved).ok()).toBe(true);
  // the page reloads onto the new contact (#key)
  await expect(li(page, 'F3 Clara Neumann')).toBeVisible();
  const key = await li(page, 'F3 Clara Neumann').getAttribute('data-key');
  await expect(page.locator('#ctform input[name=full_name]')).toHaveValue('F3 Clara Neumann');
  const c = await api(page, 'GET', `/api/contacts/${key}`);
  expect(c.given).toBe('F3 Clara');
  expect(c.family).toBe('Neumann');
  expect(c.phones.map(p => p.value)).toEqual(['+49 170 5550001']);
  expect(c.emails.map(p => p.value)).toEqual(['clara@example.org']);
  expect(c.birthday).toBe('1990-11-03');
  await expect(li(page, 'F3 Clara Neumann').locator('.bday')).toContainText('03.11.1990');
});

test('create a contact from the table view (+ dialog)', async ({ page }) => {
  await table(page);
  await page.click('#ct-fab');
  await expect(page.locator('#contacts.dlg #ctform.new')).toBeVisible();
  await page.fill('#ctform input[name=given]', 'F3 Tabea');
  await page.fill('#ctform input[name=family]', 'Tisch');
  const saved = waitFor(page, 'POST', /^\/api\/contacts$/);
  await page.click('#ct-save');
  expect((await saved).ok()).toBe(true);
  await expect(page.locator('.list li[data-name="F3 Tabea Tisch"]')).toHaveCount(1);
  const key = await page.locator('.list li[data-name="F3 Tabea Tisch"]').getAttribute('data-key');
  expect((await api(page, 'GET', `/api/contacts/${key}`)).full_name).toBe('F3 Tabea Tisch');
});

test('edit a contact: unlock, change, save; reset discards', async ({ page }) => {
  const key = await seedContact(page, 'F3 Emil', 'Edit');
  await list(page);
  await openContact(page, 'F3 Emil Edit');
  await expect(page.locator('#ctform.mode-view')).toBeVisible();
  await page.click('#ct-lock');
  await expect(page.locator('#ctform.mode-edit')).toBeVisible();
  // reset brings the stored values back
  await page.fill('#ctform input[name=nickname]', 'verworfen');
  await expect(page.locator('#ct-save')).toBeEnabled();
  await page.click('#ct-reset');
  await expect(page.locator('#ctform input[name=nickname]')).toHaveValue('');
  await page.click('#ct-lock');
  await page.fill('#ctform input[name=family]', 'Bearbeitet');
  await expect(page.locator('#ctform input[name=full_name]')).toHaveValue('F3 Emil Bearbeitet'); // follows given + family
  await page.click('#ctform .addmenu > button');
  await page.click('#ctform [data-card-add=work]');
  await page.fill('#ctform input[name=org]', 'Firma F3');
  const saved = waitFor(page, 'PUT', /^\/api\/contacts\//);
  await page.click('#ct-save');
  expect((await saved).ok()).toBe(true);
  await expect(li(page, 'F3 Emil Bearbeitet')).toBeVisible();
  const c = await api(page, 'GET', `/api/contacts/${key}`);
  expect([c.family, c.full_name, c.org]).toEqual(['Bearbeitet', 'F3 Emil Bearbeitet', 'Firma F3']);
});

test('delete a contact (asks first; "no" keeps it)', async ({ page }) => {
  const key = await seedContact(page, 'F3 Dieter', 'Delete');
  await list(page);
  await openContact(page, 'F3 Dieter Delete');
  await expect(page.locator('#ct-delete')).toBeDisabled(); // view mode
  await page.click('#ct-lock');
  await page.click('#ct-delete');
  await expect(page.locator('.ask')).toBeVisible();
  await page.click('.ask [data-a="0"]');
  await expect(page.locator('.ask')).toHaveCount(0);
  expect((await api(page, 'GET', `/api/contacts/${key}`)).full_name).toBe('F3 Dieter Delete');
  const gone = waitFor(page, 'DELETE', /^\/api\/contacts\//);
  await page.click('#ct-delete');
  await yes(page);
  expect((await gone).ok()).toBe(true);
  await expect(li(page, 'F3 Dieter Delete')).toHaveCount(0);
  const r = await page.request.get(`/api/contacts/${key}`);
  expect(r.status()).toBe(404);
});

test('table: inline edit of phone and e-mail cells persists', async ({ page }) => {
  const key = await seedContact(page, 'F3 Ines', 'Inline', { phones: [{ kind: 'cell', value: '+49 1' }], emails: [{ kind: 'home', value: 'alt@example.org' }] });
  await table(page);
  const row = page.locator(`.sheet tr[data-key="${key}"]`);
  for (const [f, v] of [['phone', '+49 170 5550002'], ['email', 'ines@example.org']]) {
    const cell = row.locator(`td[data-f="${f}"]`);
    await cell.click();
    await cell.fill(v);
    const put = waitFor(page, 'PUT', /^\/api\/contacts\//);
    await cell.press('Enter');
    expect((await put).ok()).toBe(true);
  }
  await page.reload();
  await expect(row.locator('td[data-f="phone"]')).toHaveText('+49 170 5550002');
  await expect(row.locator('td[data-f="email"]')).toHaveText('ines@example.org');
  const c = await api(page, 'GET', `/api/contacts/${key}`);
  expect(c.phones[0].value).toBe('+49 170 5550002');
  expect(c.emails[0].value).toBe('ines@example.org');
});

test('table: a card cell opens that card alone and saves the contact', async ({ page }) => {
  const key = await seedContact(page, 'F3 Sol', 'Solo');
  await table(page);
  await page.locator(`.sheet tr[data-key="${key}"] td[data-c="category"]`).click();
  await expect(page.locator('#contacts.dlg.solo')).toBeVisible();
  await page.locator('#ctform .card[data-card=cats] .tags input').fill('F3-Kategorie');
  await page.locator('#ctform .card[data-card=cats] .tags input').press('Enter');
  const put = waitFor(page, 'PUT', /^\/api\/contacts\//);
  await page.click('#ct-save');
  expect((await put).ok()).toBe(true);
  await expect.poll(async () => (await api(page, 'GET', `/api/contacts/${key}`)).categories).toEqual(['F3-Kategorie']);
});

test('table column picker persists (POST /settings/columns) and is restored', async ({ page }) => {
  await table(page);
  await page.click('#colpick > button');
  const org = page.locator('#colpop input[data-col=org]');
  const before = await org.isChecked();
  const post = waitFor(page, 'POST', /^\/settings\/columns$/);
  await org.click();
  expect((await post).ok()).toBe(true);
  await page.reload();
  await expect(page.locator('#colpop input[data-col=org]')).toBeChecked({ checked: !before });
  expect(await page.locator('.sheet th[data-c=org]').evaluate(e => e.classList.contains('off'))).toBe(before);
  // restore
  await page.click('#colpick > button');
  const back = waitFor(page, 'POST', /^\/settings\/columns$/);
  await page.locator('#colpop input[data-col=org]').click();
  expect((await back).ok()).toBe(true);
  await page.reload();
  await expect(page.locator('#colpop input[data-col=org]')).toBeChecked({ checked: before });
});

test('list fields picker persists (POST /settings/list-fields) and is restored', async ({ page }) => {
  await list(page);
  await page.click('#colpick > button');
  const note = page.locator('#colpop input[data-lf=note]');
  const before = await note.isChecked();
  const annaNote = li(page, 'Anna Albrecht').locator('[data-lf=note]');
  const post = waitFor(page, 'POST', /^\/settings\/list-fields$/);
  await note.click();
  expect((await post).ok()).toBe(true);
  await page.reload();
  await expect(page.locator('#colpop input[data-lf=note]')).toBeChecked({ checked: !before });
  await (before ? expect(annaNote).toBeHidden() : expect(annaNote).toBeVisible());
  await page.click('#colpick > button');
  const back = waitFor(page, 'POST', /^\/settings\/list-fields$/);
  await page.locator('#colpop input[data-lf=note]').click();
  expect((await back).ok()).toBe(true);
  await page.reload();
  await expect(page.locator('#colpop input[data-lf=note]')).toBeChecked({ checked: before });
  await (before ? expect(annaNote).toBeVisible() : expect(annaNote).toBeHidden());
});

test('sort field/direction and the view switch are kept (localStorage)', async ({ page }) => {
  await list(page);
  const letters = () => page.locator('.panels .lpanel:not(.hide)').evaluateAll(ps => ps.map(p => p.dataset.letter));
  expect((await letters())[0]).toBe('A'); // by given name: Anna
  await page.selectOption('#ct-sort', 'family');
  await page.click('#ct-dir'); // descending
  await page.reload();
  await expect(page.locator('#ct-sort')).toHaveValue('family');
  await expect(page.locator('#ct-dir use')).toHaveAttribute('href', /#i-sort-desc$/); // the sprite file's URL + the symbol
  const desc = await letters();
  expect(desc.indexOf('Z')).toBeLessThan(desc.indexOf('A')); // Zimmermann before Albrecht
  await page.click('[data-ctview=table]');
  await page.goto('/contacts');
  await expect(page.locator('#contacts.view-table')).toBeVisible();
  expect(await page.evaluate(() => localStorage.ctview)).toBe('table');
});

test('filter by address book and by category', async ({ page }) => {
  await list(page);
  await page.click('.topbar .filter:has(#abfilter) > button');
  await page.locator(`#abfilter input[data-ab="${AB}"]`).uncheck();
  await expect(li(page, 'Anna Albrecht')).toBeHidden();
  await expect(li(page, 'Opa Otto')).toBeVisible(); // group address book
  await page.click('#abfilter [data-filter=all]');
  await expect(li(page, 'Anna Albrecht')).toBeVisible();
  await page.locator('#abfilter input[data-cat="Arbeit"]').uncheck(); // Özlem's (first) category
  await expect(li(page, 'Özlem Öztürk')).toBeHidden();
  await expect(li(page, 'Dora Dahl')).toBeVisible();
  await page.click('#abfilter [data-filter=none]');
  await expect(page.locator('.list li[data-key]:visible')).toHaveCount(0);
});

test('vCard download', async ({ page }) => {
  await list(page);
  await openContact(page, 'Anna Albrecht');
  const dl = page.waitForEvent('download');
  await page.click('#ct-vcf');
  const d = await dl;
  const body = await (await d.createReadStream()).toArray().then(b => Buffer.concat(b).toString('utf8'));
  expect(body).toMatch(/^BEGIN:VCARD/);
  expect(body).toContain('UID:c-anna@seed');
  expect(body).toContain('FN:Anna Albrecht');
});

test('deep link /contacts#key opens the contact', async ({ page }) => {
  await page.goto(`/contacts#${AB}/c-dora@seed`);
  await expect(page.locator('#ctform input[name=full_name]')).toHaveValue('Dora Dahl');
  await expect(li(page, 'Dora Dahl')).toHaveClass(/\bon\b/);
});

test('view-mode row actions: chat/WhatsApp open, copy, related goto', async ({ page }) => {
  await list(page);
  await openContact(page, 'Anna Albrecht');
  await page.locator('#ctform [data-list=impp] .row [data-act=im]').click();
  await page.locator('#ctform [data-list=phones] .row').first().locator('[data-act=wa]').click();
  const opened = await page.evaluate(() => window.__opened);
  expect(opened).toEqual(['https://signal.me/#p/+491511234567', 'https://wa.me/491511234567']);
  await page.locator('#ctform [data-list=emails] .row [data-act=copy]').click();
  await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toBe('anna@example.org');
  await page.locator('#ctform [data-list=related] .row [data-act=goto]').click();
  await expect(page.locator('#ctform input[name=full_name]')).toHaveValue('Ben Albrecht');
  expect(page.__dialogs).toEqual([]);
  expect(await page.locator(`.list li[data-key="${ANNA}"].on`).count()).toBe(0);
});

test('the display name is the editor\'s title: 20 px semibold like the event title, in view and edit mode', async ({ page }) => {
  await list(page);
  await openContact(page, 'Anna Albrecht');
  const name = page.locator('#ctform input[name=full_name]');
  const font = () => name.evaluate(el => { const s = getComputedStyle(el); return [s.fontSize, s.fontWeight, s.paddingTop]; });
  expect(await font()).toEqual(['20px', '600', '0px']);
  await page.locator('#ct-lock').click();
  await expect(page.locator('#ctform')).toHaveClass(/mode-edit/);
  expect(await font()).toEqual(['20px', '600', '0px']);
  expect(await name.evaluate(el => getComputedStyle(el).borderBottomStyle)).toBe('solid'); // the edit underline stays
  await expect(page.locator('#ct-error')).toHaveAttribute('role', 'alert'); // a failed save is announced
});
