// Settings: every save path (autosave prefs, pickers, forms with redirect + flash, raw imports, exports).
// Shared DB with the other functional specs: own objects carry "F4", changed prefs go back to the seed.
import fs from 'node:fs';
import path from 'node:path';
import { test, expect, settle, loginAs, api } from '../lib/test.mjs';
import { BASE, WORK } from '../lib/env.mjs';
import { savePrefs, settingsForm, nextSave, basic } from './_set.mjs';
import { addCard, answer } from './_editor.mjs';

const go = async (page, h) => { await page.evaluate(x => { location.hash = x; }, h); };
// a real reload: goto() to the same path with another hash only scrolls (same-document navigation)
const reloadAt = async (page, h) => { await page.goto('/settings' + h); await page.reload(); await settle(page); };

test.afterEach(async ({ page }) => { await savePrefs(page); });

test('timezone picker autosaves and survives a reload', async ({ page }) => {
  await reloadAt(page, '#pick-tz');
  await page.locator('#pick-q').fill('london');
  const j = await nextSave(page, '/settings/prefs', () => page.locator('#pick-list button[data-v="Europe/London"]').click());
  expect(j.ok).toBe(true);
  await expect(page.locator('#v-tz')).toHaveText('Europe/London');
  await reloadAt(page, '');
  await expect(page.locator('#v-tz')).toHaveText('Europe/London');
  await expect(page.locator('#home input[name=timezone]')).toHaveValue('Europe/London');
});

test('language choice reloads the page in that language; none again via the seed values', async ({ page }) => {
  await reloadAt(page, '');
  await expect(page.locator('html')).toHaveAttribute('lang', 'de');
  await Promise.all([page.waitForEvent('load'), page.locator('select[name=lang]').selectOption('en')]);
  await expect(page.locator('html')).toHaveAttribute('lang', 'en');
  await expect(page.locator('#home h1')).toHaveText('Settings');
  await expect(page.locator('.toast')).toBeVisible(); // the "saved" toast survives the reload (sessionStorage)
  await savePrefs(page); // lang "" in the DB
  const fresh = await page.context().browser().newContext({ locale: 'de-DE' });
  const p2 = await fresh.newPage();
  await p2.goto(BASE + '/settings');
  await expect(p2.locator('html')).toHaveAttribute('lang', 'de');
  await fresh.close();
});

test('vacation days and rule kinds save; the title tester marks hits', async ({ page }) => {
  await reloadAt(page, '#vacation');
  let j = await nextSave(page, '/settings/prefs', async () => { await page.locator('input[name=vacation_days]').fill('25'); await page.locator('input[name=vacation_days]').blur(); });
  expect(j.ok).toBe(true);
  j = await nextSave(page, '/settings/prefs', () => page.locator('#vacation .seg button[data-v="title"]').click());
  expect(j.ok).toBe(true);
  await page.locator('#vac-re').fill('urlaub');
  await expect(page.locator('#vac-test .vac-list li.match')).toHaveCount(2);
  await expect(page.locator('#vac-test .vac-list mark').first()).toHaveText(/urlaub/i);
  await page.locator('#vac-test [data-show="miss"]').click();
  await expect(page.locator('#vac-test .vac-list li.miss').first()).toBeVisible();
  j = await nextSave(page, '/settings/prefs', () => page.locator('#vac-re').blur());
  expect(j.ok).toBe(true);
  // an invalid pattern blocks the autosave (field validity)
  await page.locator('#vac-re').fill('urlaub(');
  await expect(page.locator('#vac-test .vac-err')).toBeVisible();
  await reloadAt(page, '#vacation');
  await expect(page.locator('input[name=vacation_days]')).toHaveValue('25');
  await expect(page.locator('#vacation .seg button[data-v="title"]')).toHaveAttribute('aria-pressed', 'true');
  await expect(page.locator('#vac-re')).toHaveValue('urlaub');
  await expect(page.locator('#v-vacation')).toHaveText('/urlaub/');
  // the building blocks insert at the cursor
  await page.locator('#vac-tools [data-tool="begin"]').click();
  await expect(page.locator('#vac-re')).toHaveValue('^urlaub');
});

test('reminder snap points: add, edit, remove, reset', async ({ page }) => {
  await reloadAt(page, '#alarms');
  const before = await page.locator('#v-snaps').textContent();
  let j = await nextSave(page, '/settings/prefs', () => page.locator('#snaps .snp-plus[data-add="start"]').first().click());
  expect(j.ok).toBe(true);
  await expect(page.locator('#v-snaps')).not.toHaveText(before);
  await expect(page.locator('#snaps-reset')).toBeVisible();
  // edit the 1 h start point to 2 h
  await page.locator('#snaps .snp-pt[data-key="start"][data-m="60"]').click();
  await page.locator('#snaps .snp-edit input').fill('2');
  await page.locator('#snaps .snp-edit .seg [data-u="60"]').click();
  j = await nextSave(page, '/settings/prefs', () => page.locator('#snaps .snp-edit [data-ok]').click());
  expect(j.ok).toBe(true);
  await expect(page.locator('#snaps .snp-pt[data-key="start"][data-m="120"]')).toBeVisible();
  await reloadAt(page, '#alarms');
  await expect(page.locator('#snaps .snp-pt[data-key="start"][data-m="120"]')).toBeVisible();
  await expect(page.locator('#snaps .snp-pt[data-key="start"][data-m="60"]')).toHaveCount(0);
  // remove an end point
  await page.locator('#snaps .snp-pt[data-key="end"][data-m="10"]').click();
  j = await nextSave(page, '/settings/prefs', () => page.locator('#snaps .snp-edit [data-del]').click());
  expect(j.ok).toBe(true);
  await expect(page.locator('#snaps .snp-pt[data-key="end"][data-m="10"]')).toHaveCount(0);
  j = await nextSave(page, '/settings/prefs', () => page.locator('#snaps-reset').click());
  expect(j.ok).toBe(true);
  await reloadAt(page, '#alarms');
  await expect(page.locator('#snaps-reset')).toBeHidden();
  await expect(page.locator('#snaps .snp-pt[data-key="start"][data-m="60"]')).toBeVisible();
});

test('holiday regions: add Berlin, make it own, remove; school regions too', async ({ page }) => {
  await reloadAt(page, '#regions-public');
  await page.locator('#regions-public a[href="#pick-country-public"]').click();
  await page.locator('#pick-list button[data-v="DE"]').click();
  await expect(page).toHaveURL(/#pick-region-public-DE$/);
  const j = await nextSave(page, '/settings/prefs', () => page.locator('#pick-list button[data-v="DE-BE"]').click());
  expect(j.ok).toBe(true);
  await expect(page).toHaveURL(/#regions-public$/);
  await expect(page.locator('#regions-public .rlist [data-code="DE-BE"]')).toBeVisible();
  await nextSave(page, '/settings/prefs', () => page.locator('#regions-public .rlist [data-code="DE-BE"] [data-first]').click());
  await expect(page.locator('#regions-public .rlist [data-code]').first()).toHaveAttribute('data-code', 'DE-BE');
  await reloadAt(page, '#regions-public');
  await expect(page.locator('input[name=public_regions]')).toHaveValue('DE-BE,DE-SN,DE-BY');
  await expect(page.locator('#regions-public .rlist [data-code="DE-BE"] .badge')).toBeVisible();
  await nextSave(page, '/settings/prefs', () => page.locator('#regions-public .rlist [data-code="DE-BE"] [data-rm]').click());
  await reloadAt(page, '#regions-school');
  await page.locator('#regions-school a[href="#pick-country-school"]').click();
  await page.locator('#pick-list button[data-v="DE"]').click();
  await nextSave(page, '/settings/prefs', () => page.locator('#pick-list button[data-v="DE-BY"]').click());
  await reloadAt(page, '');
  await expect(page.locator('input[name=public_regions]')).toHaveValue('DE-SN,DE-BY');
  await expect(page.locator('input[name=school_regions]')).toHaveValue('DE-SN,DE-BY');
  await expect(page.locator('#v-regions-school')).toHaveText('Sachsen, Bayern');
});

test('holiday colour reaches the calendar page', async ({ page }) => {
  await savePrefs(page, { holiday_color: '#123456' });
  await page.goto('/calendar?view=month&date=2026-10-14');
  expect(await page.locator('head style').allTextContents()).toContainEqual(expect.stringContaining('--hol:#123456'));
});

test('scheme segment in settings saves the scheme', async ({ page }) => {
  await reloadAt(page, '');
  const j = await nextSave(page, '/settings/prefs', () => page.locator('.seg[data-field=scheme] button[data-v="dark"]').click());
  expect(j.ok).toBe(true);
  await expect(page.locator('html')).toHaveAttribute('data-scheme', 'dark');
  await reloadAt(page, '');
  await expect(page.locator('html')).toHaveAttribute('data-pref-scheme', 'dark');
});

test('app token: created, shown once, works for Basic auth, revoked', async ({ page }) => {
  // made on "add a device" › manual, listed and disconnected under "my connected devices"
  await reloadAt(page, '#manual');
  await page.locator('#manual form[action="/settings/tokens"] input[name=name]').fill('F4 Token');
  await Promise.all([page.waitForURL(/\/settings#manual$/), page.locator('#manual form[action="/settings/tokens"] button[type=submit]').click()]);
  const token = (await page.locator('#new-token').textContent()).trim();
  expect(token).toMatch(/^[0-9a-f]{4}_[A-Za-z0-9]{64}$/);
  const ok = await basic('alice', token);
  expect((await ok.get('/api/history')).status()).toBe(200);
  await reloadAt(page, '#account');
  await expect(page.locator('#new-token')).toHaveCount(0);
  const row = page.locator('#account .cell', { hasText: 'F4 Token' });
  await expect(row).toBeVisible();
  await row.locator('button[data-confirm]').click();
  await page.locator('.ask [data-a="1"]').click();
  await page.waitForURL(/\/settings#account$/);
  await expect(page.locator('#account .cell', { hasText: 'F4 Token' })).toHaveCount(0);
  expect((await ok.get('/api/history')).status()).toBe(401);
  await ok.dispose();
});

test('groups: create, add and remove a member, member leaves, delete → trash → restore → purge', async ({ page, browser }) => {
  const gid = 'group.f4-gruppe';
  await reloadAt(page, '#groups');
  await page.locator('#groups form[action="/settings/groups"] input[name=name]').fill('F4 Gruppe');
  await Promise.all([page.waitForURL(/#groups$/), page.locator('#groups form[action="/settings/groups"] button[type=submit]').click()]);
  const add = page.locator(`form[action="/settings/groups/${gid}/members"]`);
  await expect(add).toBeVisible();
  await add.locator('select[name=member]').selectOption('bob');
  await Promise.all([page.waitForURL(/#groups$/), add.locator('button[type=submit]').click()]);
  const rm = page.locator(`form[action="/settings/groups/${gid}/members/bob/remove"] button`);
  await expect(rm).toBeVisible();
  await rm.click();
  await page.locator('.ask [data-a="1"]').click();
  await page.waitForURL(/#groups$/);
  await expect(page.locator(`form[action="/settings/groups/${gid}/members/bob/remove"]`)).toHaveCount(0);
  // bob again, and bob leaves by himself
  await page.locator(`form[action="/settings/groups/${gid}/members"] select[name=member]`).selectOption('bob');
  await Promise.all([page.waitForURL(/#groups$/), page.locator(`form[action="/settings/groups/${gid}/members"] button[type=submit]`).click()]);
  const bobCtx = await browser.newContext({ baseURL: BASE, locale: 'de-DE' });
  const bob = await bobCtx.newPage();
  await loginAs(bob, 'bob');
  await bob.goto('/settings#groups');
  await bob.locator(`form[action="/settings/groups/${gid}/leave"] button`).click();
  await bob.locator('.ask [data-a="1"]').click();
  await bob.waitForURL(/#groups$/);
  await expect(bob.locator(`form[action="/settings/groups/${gid}/leave"]`)).toHaveCount(0);
  await bobCtx.close();
  await reloadAt(page, '#groups');
  await expect(page.locator(`form[action="/settings/groups/${gid}/members/bob/remove"]`)).toHaveCount(0);
  // delete: typed confirmation, then the trash
  const del = page.locator(`form[action="/settings/groups/${gid}/delete"]`).locator('xpath=ancestor::details[1]').locator('summary');
  await del.click();
  const yes = page.locator('.ask button[type=submit]');
  await expect(yes).toBeDisabled();
  await page.locator('.ask .ask-type input').fill('F4 Gruppe');
  await expect(yes).toBeEnabled();
  await Promise.all([page.waitForURL(/\/settings/), yes.click()]);
  await reloadAt(page, '#trash');
  await expect(page.locator(`form[action="/settings/trash/groups/${gid}/restore"]`)).toBeVisible();
  await Promise.all([page.waitForURL(/\/settings/), page.locator(`form[action="/settings/trash/groups/${gid}/restore"] button`).click()]);
  await reloadAt(page, '#groups');
  await expect(page.locator(`form[action="/settings/groups/${gid}/members"]`)).toBeVisible();
  await settingsForm(page, `/settings/groups/${gid}/delete`, { confirm: 'F4 Gruppe' });
  await reloadAt(page, '#trash');
  await page.locator(`form[action="/settings/trash/groups/${gid}/purge"]`).locator('xpath=ancestor::details[1]').locator('summary').click();
  await page.locator('.ask .ask-type input').fill(gid);
  await Promise.all([page.waitForURL(/\/settings/), page.locator('.ask button[type=submit]').click()]);
  await reloadAt(page, '#trash');
  await expect(page.locator(`form[action^="/settings/trash/groups/${gid}/"]`)).toHaveCount(0);
});

for (const kind of ['calendar', 'addressbook']) {
  test(`${kind}: create, rename + describe (autosave), trash, restore, purge`, async ({ page }) => {
    const pfx = kind === 'calendar' ? 'cal' : 'ab', coll = kind === 'calendar' ? 'calendars' : 'addressbooks';
    await reloadAt(page, `#new-${kind}`);
    const form = page.locator(`#new-${kind} form`);
    await form.locator('input[name=name]').fill(`F4 ${kind}`);
    await Promise.all([page.waitForURL(/\/settings#/), form.locator('button[type=submit]').click()]);
    await reloadAt(page, '#collections');
    const link = page.locator('#collections a', { hasText: `F4 ${kind}` });
    await expect(link).toBeVisible();
    const href = await link.getAttribute('href');
    const [, owner, id] = href.match(new RegExp(`^#${pfx}-(alice)-(.+)$`));
    await link.click();
    const pg = page.locator(`section[id="${pfx}-alice-${id}"]`);
    await expect(pg).toBeVisible();
    // name: data-reload → the page reloads with the toast
    await Promise.all([page.waitForEvent('load'), (async () => { await pg.locator('input[name=name]').fill(`F4 ${kind} neu`); await pg.locator('input[name=name]').blur(); })()]);
    await expect(page.locator('.toast')).toBeVisible();
    await Promise.all([page.waitForEvent('load'), (async () => { await pg.locator('input[name=description]').fill('F4 Beschreibung'); await pg.locator('input[name=description]').blur(); })()]);
    await reloadAt(page, `#${pfx}-${owner}-${id}`);
    await expect(pg.locator('input[name=name]')).toHaveValue(`F4 ${kind} neu`);
    await expect(pg.locator('input[name=description]')).toHaveValue('F4 Beschreibung');
    // trash (asks) → restore → trash → purge
    await pg.locator(`form[action="/settings/${coll}/${owner}/${id}/delete"] button`).click();
    await page.locator('.ask [data-a="1"]').click();
    await page.waitForURL(/\/settings/);
    await reloadAt(page, '#trash');
    await expect(page.locator(`form[action="/settings/${coll}/${owner}/${id}/restore"]`)).toBeVisible();
    await Promise.all([page.waitForURL(/\/settings/), page.locator(`form[action="/settings/${coll}/${owner}/${id}/restore"] button`).click()]);
    await reloadAt(page, '#collections');
    await expect(page.locator('#collections a', { hasText: `F4 ${kind} neu` })).toBeVisible();
    await settingsForm(page, `/settings/${coll}/${owner}/${id}/delete`);
    await reloadAt(page, '#trash');
    await page.locator(`form[action="/settings/${coll}/${owner}/${id}/purge"] button`).click();
    await page.locator('.ask [data-a="1"]').click();
    await page.waitForURL(/\/settings/);
    await reloadAt(page, '#trash');
    await expect(page.locator(`form[action^="/settings/${coll}/${owner}/${id}/"]`)).toHaveCount(0);
    await reloadAt(page, '#collections');
    await expect(page.locator('#collections a', { hasText: `F4 ${kind}` })).toHaveCount(0);
  });
}

test('import: a new calendar from a file, then more into it; a new address book; exports', async ({ page }) => {
  const dir = path.join(WORK, 'f4-files');
  fs.mkdirSync(dir, { recursive: true });
  const f1 = path.join(dir, 'f4-reise.ics'), f2 = path.join(dir, 'f4-mehr.ics'), f3 = path.join(dir, 'f4-verein.vcf');
  const ev = (uid, s, d) => `BEGIN:VEVENT\r\nUID:${uid}\r\nDTSTAMP:20261001T080000Z\r\nDTSTART:${d}T080000Z\r\nDTEND:${d}T090000Z\r\nSUMMARY:${s}\r\nEND:VEVENT\r\n`;
  fs.writeFileSync(f1, `BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//e2e//F4//DE\r\nX-WR-CALNAME:F4 Reiseplan\r\nX-WR-CALCOLOR:#34c759\r\n${ev('f4-zug@e2e', 'F4 Zugfahrt', '20261102')}END:VCALENDAR\r\n`);
  fs.writeFileSync(f2, `BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//e2e//F4//DE\r\n${ev('f4-hotel@e2e', 'F4 Hotel', '20261103')}${ev('f4-museum@e2e', 'F4 Museum', '20261104')}END:VCALENDAR\r\n`);
  fs.writeFileSync(f3, 'BEGIN:VCARD\r\nVERSION:4.0\r\nUID:f4-vera@e2e\r\nFN:F4 Vera Verein\r\nN:Verein;Vera;;;\r\nEND:VCARD\r\n');
  await reloadAt(page, '#import-calendar');
  const form = page.locator('#import-calendar .imp-form');
  await form.locator('.imp-file').setInputFiles(f1);
  await expect(form.locator('input[name=name]')).toHaveValue('F4 Reiseplan');
  await expect(form.locator('input[name=color]')).toHaveValue('#34c759');
  await Promise.all([page.waitForEvent('load'), form.locator('.imp-go').click()]);
  await expect(page).toHaveURL(/#cal-alice-/);
  await expect(page.locator('.toast')).toContainText('F4 Reiseplan');
  const calId = page.url().match(/#cal-alice-(.+)$/)[1];
  // into the existing calendar
  const pg = page.locator(`section[id="cal-alice-${calId}"]`);
  await Promise.all([page.waitForEvent('load'), pg.locator('input[type=file][data-import]').setInputFiles(f2)]);
  await expect(page.locator('.toast')).toBeVisible();
  await expect.poll(async () => (await (await page.request.get('/api/search?q=F4%20Museum')).json()).events.map(e => e.summary)).toContain('F4 Museum');
  // export: the .ics holds all three
  await reloadAt(page, `#cal-alice-${calId}`);
  const [dl] = await Promise.all([page.waitForEvent('download'), pg.locator(`a[href="/settings/export/calendars/alice/${calId}"]`).click()]);
  const ics = fs.readFileSync(await dl.path(), 'utf8');
  for (const uid of ['f4-zug@e2e', 'f4-hotel@e2e', 'f4-museum@e2e']) expect(ics).toContain(`UID:${uid}`);
  // a new address book from a .vcf (its name from the file name), exported as .vcf
  await reloadAt(page, '#import-addressbook');
  const fa = page.locator('#import-addressbook .imp-form');
  await fa.locator('.imp-file').setInputFiles(f3);
  await expect(fa.locator('input[name=name]')).toHaveValue('f4-verein');
  await Promise.all([page.waitForEvent('load'), fa.locator('.imp-go').click()]);
  const abId = page.url().match(/#ab-alice-(.+)$/)[1];
  const [dl2] = await Promise.all([page.waitForEvent('download'), page.locator(`a[href="/settings/export/addressbooks/alice/${abId}"]`).click()]);
  expect(fs.readFileSync(await dl2.path(), 'utf8')).toContain('FN:F4 Vera Verein');
  // cleanup: both into the trash and purged
  for (const p of [`/settings/calendars/alice/${calId}`, `/settings/addressbooks/alice/${abId}`]) { await settingsForm(page, p + '/delete'); await settingsForm(page, p + '/purge'); }
});

// the categories page opens a rule in the editor over itself (GET /editor, js/shell/settings-pages.js): no navigation
const watch = page => { const w = { errors: [], navs: 0, loads: [] }; page.on('pageerror', e => w.errors.push(String(e))); page.on('framenavigated', f => { if (f === page.mainFrame()) w.navs++; }); page.on('request', r => { const u = new URL(r.url()); if (u.pathname === '/editor') w.loads.push(u.search); }); return w; };
const ruleEd = page => page.locator('#ed.rulemode:not([hidden])');
const catRow = (page, n) => page.locator('#cat-list a', { hasText: n });
const unrule = (page, n, scope) => api(page, 'DELETE', `/api/category-rules/${encodeURIComponent(n)}?scope=${scope}`).catch(() => {});

test('categories page: an event category\'s rule opens over the page, saves and deletes, the list follows', async ({ page }) => {
  await reloadAt(page, '#categories');
  const w = watch(page), row = catRow(page, 'Kunde');
  await expect(catRow(page, 'Arbeit').locator('.badge')).toBeVisible();
  await expect(row.locator('.badge')).toHaveCount(0);
  await expect(row).toHaveAttribute('href', '/calendar?rule=Kunde&back=%2Fsettings%23categories'); // the deep link stays the link's target (new tab)
  try {
    await row.click();
    await ruleEd(page).waitFor();
    await expect(page).toHaveURL(/\/settings#categories$/);
    await expect(page.locator('#ed-host')).toHaveAttribute('data-kind', 'event');
    await expect(page.locator('#ed')).not.toHaveClass(/taskmode/);
    await expect(page.locator('#ed-sub')).toContainText('Kunde');
    await expect(page.locator('#ed-close')).toBeFocused();
    await expect(page.locator('#ed-delete')).toBeDisabled(); // no rule yet
    await addCard(page, 'where');
    await page.locator('#ed-locin').fill('F4 Ort, Dresden');
    await page.locator('#edh').focus();
    await page.locator('#ed-save').click();
    await expect(page.locator('#ed')).toBeHidden();
    await expect(row.locator('.badge')).toBeVisible();
    await expect(row).toBeFocused(); // back on its row, redrawn
    expect((await api(page, 'GET', '/api/category-rules?scope=event')).Kunde.where.location).toBe('F4 Ort, Dresden');
    // again: the same dialog (one load), the rule as saved
    await row.click();
    await ruleEd(page).waitFor();
    await expect(page.locator('#ed-locin')).toHaveValue('F4 Ort, Dresden');
    await page.locator('#ed-delete').click();
    await answer(page, true);
    await expect(page.locator('#ed')).toBeHidden();
    await expect(row.locator('.badge')).toHaveCount(0);
    expect((await api(page, 'GET', '/api/category-rules?scope=event')).Kunde).toBeUndefined();
    // by keyboard: Enter on the row, the focus into the dialog, Escape back to the row
    await row.focus();
    await page.keyboard.press('Enter');
    await ruleEd(page).waitFor();
    await expect(page.locator('#ed-close')).toBeFocused();
    await page.keyboard.press('Escape');
    await expect(page.locator('#ed')).toBeHidden();
    await expect(row).toBeFocused();
    await expect(page).toHaveURL(/\/settings#categories$/);
    expect(w.loads).toEqual(['?kind=event']);
    expect(w.navs).toBe(0);
  } finally { await unrule(page, 'Kunde', 'event'); }
  expect(w.errors).toEqual([]);
});

test('categories page: a task category\'s rule, Escape and ×, then an event one: the dialog changes kind', async ({ page }) => {
  await reloadAt(page, '#categories');
  const w = watch(page);
  await page.locator('#cat-scope button[data-scope="task"]').click();
  const row = catRow(page, 'Auto');
  await expect(row).toHaveAttribute('href', '/tasks?rule=Auto&back=%2Fsettings%23categories');
  try {
    await row.click();
    await ruleEd(page).waitFor();
    await expect(page.locator('#ed-host')).toHaveAttribute('data-kind', 'task');
    await expect(page.locator('#ed')).toHaveClass(/taskmode/);
    await expect(page.locator('#ed-sub')).toContainText('Auto');
    await addCard(page, 'where');
    await page.locator('#ed-locin').fill('F4 Werkstatt, Dresden');
    await page.locator('#edh').focus();
    await page.locator('#ed-save').click();
    await expect(page.locator('#ed')).toBeHidden();
    await expect(row.locator('.badge')).toBeVisible();
    expect((await api(page, 'GET', '/api/category-rules?scope=task')).Auto.where.location).toBe('F4 Werkstatt, Dresden');
    // Escape and × close an untouched rule; with a change both ask first, "discard" keeps the stored rule
    await row.click();
    await ruleEd(page).waitFor();
    await page.keyboard.press('Escape');
    await expect(page.locator('#ed')).toBeHidden();
    await row.click();
    await ruleEd(page).waitFor();
    await page.locator('#ed-close').click();
    await expect(page.locator('#ed')).toBeHidden();
    await row.click();
    await ruleEd(page).waitFor();
    await page.locator('#ed-locin').fill('F4 anders');
    await page.locator('#edh').focus();
    await page.keyboard.press('Escape');
    await expect(page.locator('#ed-confirm')).toBeVisible();
    await page.locator('#ed-discard').click();
    await expect(page.locator('#ed')).toBeHidden();
    expect((await api(page, 'GET', '/api/category-rules?scope=task')).Auto.where.location).toBe('F4 Werkstatt, Dresden');
    // the other scope: the task dialog leaves the page, the event one comes
    await page.locator('#cat-scope button[data-scope="event"]').click();
    await catRow(page, 'Kunde').click();
    await ruleEd(page).waitFor();
    await expect(page.locator('#ed-host')).toHaveAttribute('data-kind', 'event');
    await expect(page.locator('#ed, #edform, #geomap, #category-rules-data')).toHaveCount(4); // one of each
    await expect(page.locator('#ed')).not.toHaveClass(/taskmode/);
    await page.keyboard.press('Escape'); // the removed dialog's listeners stay quiet
    await expect(page.locator('#ed')).toBeHidden();
    await page.keyboard.press('Escape');
    expect(w.loads).toEqual(['?kind=task', '?kind=event']);
    expect(w.navs).toBe(0);
  } finally { await unrule(page, 'Auto', 'task'); }
  expect(w.errors).toEqual([]);
});

test('categories page: a new category from the form gets its rule over the page', async ({ page }) => {
  await reloadAt(page, '#categories');
  const w = watch(page);
  try {
    await page.locator('#cat-new-name').fill('F4Kat');
    await page.locator('#cat-new button[type=submit]').click();
    await ruleEd(page).waitFor();
    await expect(page.locator('#ed-sub')).toContainText('F4Kat');
    await expect(page.locator('#ed-delete')).toBeDisabled();
    await addCard(page, 'where');
    await page.locator('#ed-locin').fill('F4 Neu, Dresden');
    await page.locator('#edh').focus();
    await page.locator('#ed-save').click();
    await expect(page.locator('#ed')).toBeHidden();
    await expect(catRow(page, 'F4Kat').locator('.badge')).toBeVisible();
    await expect(page.locator('#cat-new-name')).toHaveValue(''); // saved: the form is done
    await expect(page).toHaveURL(/\/settings#categories$/);
    expect(w.navs).toBe(0);
  } finally { await unrule(page, 'F4Kat', 'event'); }
  expect(w.errors).toEqual([]);
});

test('categories page: a rule kept over a lost session comes back over the page', async ({ page }) => {
  await reloadAt(page, '');
  // what the editor's practical.keep stashes (js/editor/wiring.js) on this path before the login
  await page.evaluate(() => { sessionStorage['practical.stash.ed-task'] = JSON.stringify({ path: '/settings', t: Date.now(), v: { rule: 'F4Kept', r: { where: { mode: 'default', location: 'F4 Keller' } }, ret: null, back: null } }); });
  await page.reload();
  await ruleEd(page).waitFor();
  await expect(page).toHaveURL(/\/settings#categories$/);
  await expect(page.locator('#ed-host')).toHaveAttribute('data-kind', 'task');
  await expect(page.locator('#cat-scope button[data-scope="task"]')).toHaveAttribute('aria-pressed', 'true');
  await expect(page.locator('#ed-sub')).toContainText('F4Kept');
  await expect(page.locator('#ed-locin')).toHaveValue('F4 Keller');
  await page.locator('#ed-close').click();
  await page.locator('#ed-discard').click();
  await expect(page.locator('#ed')).toBeHidden();
  expect((await api(page, 'GET', '/api/category-rules?scope=task')).F4Kept).toBeUndefined();
});

test('search history page: entries listed, one removed, all cleared after the question', async ({ page }) => {
  await reloadAt(page, '');
  const csrf = await page.locator('meta[name=csrf]').getAttribute('content');
  for (const q of ['f4 eins', 'f4 zwei']) await page.request.post('/api/search/history', { data: { q }, headers: { 'X-CSRF-Token': csrf, Origin: BASE } });
  await go(page, '#search');
  const row = page.locator('#hist-list [data-id]', { hasText: 'f4 eins' });
  await expect(row).toBeVisible();
  await row.locator('[data-hrm]').click();
  await expect(page.locator('#hist-list [data-id]', { hasText: 'f4 eins' })).toHaveCount(0);
  await page.locator('#hist-clear').click();
  await page.locator('.ask [data-a="0"]').click();
  await expect(page.locator('#hist-list [data-id]', { hasText: 'f4 zwei' })).toBeVisible();
  await page.locator('#hist-clear').click();
  await page.locator('.ask [data-a="1"]').click();
  await expect(page.locator('#hist-list [data-id]')).toHaveCount(0);
  expect(await (await page.request.get('/api/search/history')).json()).toEqual([]);
  // the seed's two entries back for later specs
  await page.request.post('/api/search/history', { data: { q: 'anna' }, headers: { 'X-CSRF-Token': csrf, Origin: BASE } });
});

test('push without VAPID: the page says unavailable, the API refuses a subscription', async ({ page }) => {
  await reloadAt(page, '#alarms');
  await expect(page.locator('#push-enable')).toHaveCount(0);
  const csrf = await page.locator('meta[name=csrf]').getAttribute('content');
  const r = await page.request.post('/api/push/subscriptions', { data: { endpoint: 'https://push.invalid/x', keys: { p256dh: 'x', auth: 'y' } }, headers: { 'X-CSRF-Token': csrf, Origin: BASE } });
  expect(r.status()).toBe(400);
  expect(await r.text()).toBe('Benachrichtigungen sind auf diesem Server ausgeschaltet.'); // B8: a browser session reads it in its language
});

/* The remote address row. All of this is browser behaviour, which is why it is tested here and not
   in Rust: the field is a combobox over a list of OUR OWN (a <datalist> wears a dropdown triangle
   in Chromium that no other field here has, and it came back on hover even when hidden), Enter in
   it CONNECTS instead of submitting the batch around it, and the arrow is the row's one action so
   it carries the accent. No Nextcloud is needed — all of it happens before a request is answered. */
test('the remote address row: own suggestion list, Enter connects, the arrow is the accent', async ({ page }) => {
  await reloadAt(page, '#remote-cal');
  const field = page.locator('#remote-cal .rm-origin');
  const button = page.locator('#remote-cal .rm-go');
  await expect(field).toBeVisible();

  // no browser dropdown anywhere: no <datalist>, and the field points at no list of the browser's
  expect(await page.locator('datalist').count()).toBe(0);
  expect(await field.getAttribute('list')).toBeNull();
  // it is a combobox over our own listbox instead
  expect(await field.getAttribute('role')).toBe('combobox');
  expect(await field.getAttribute('aria-expanded')).toBe('false');

  // the placeholder is not clipped by the button beside it
  const clipped = await field.evaluate(el => {
    const before = el.value;
    el.value = el.placeholder;
    const over = el.scrollWidth - el.clientWidth;
    el.value = before;
    return over;
  });
  expect(clipped).toBeLessThanOrEqual(0);

  // the arrow: accent background, white glyph
  const look = await button.evaluate(el => ({ bg: getComputedStyle(el).backgroundColor, ico: getComputedStyle(el.querySelector('.ico')).color }));
  const accent = await page.evaluate(() => {
    const p = document.createElement('i');
    p.style.color = 'var(--accent)';
    document.body.appendChild(p);
    const c = getComputedStyle(p).color;
    p.remove();
    return c;
  });
  expect(look.bg).toBe(accent);
  expect(look.ico).toBe('rgb(255, 255, 255)');

  /* Enter must not submit the form it sits in — that form's submit is the import BATCH, so an
     address plus Enter would have started importing nothing at all. */
  const calls = [];
  await page.route('**/settings/remote/**', route => { calls.push(new URL(route.request().url()).pathname); route.abort(); });
  await field.fill('nextcloud.example.com');
  await field.press('Enter');
  await settle(page);
  expect(calls.some(p => p.endsWith('/reuse') || p.endsWith('/login'))).toBe(true);
  expect(calls.some(p => p.endsWith('/import'))).toBe(false);
  await expect(page.locator('.rm-dlg')).not.toBeVisible();
  await page.unroute('**/settings/remote/**');
});

/* The owner of what is taken over is chosen once for the batch, like on "create": the user or one
   of their groups. Every collection request of the batch carries it. No Nextcloud: the sign-in is
   reused and the list is answered here, and the import itself is answered too, so nothing is made. */
test('the remote batch takes an owner, and every request of it carries the one chosen', async ({ page }) => {
  await reloadAt(page, '#remote-cal');
  const bodies = [];
  await page.route('**/settings/remote/reuse', r => r.fulfill({ json: { handle: 'h' } }));
  await page.route('**/settings/remote/collections', r => r.fulfill({ json: {
    calendars: [{ name: 'Eins', href: '/c/1/', writable: true }, { name: 'Zwei', href: '/c/2/', writable: true }], addressbooks: [],
  } }));
  await page.route('**/settings/remote/import', r => { bodies.push(r.request().postDataJSON()); r.fulfill({ status: 500, contentType: 'text/plain', body: 'stop' }); });
  await page.route('**/settings/remote/done', r => r.fulfill({ json: {} }));
  await page.locator('#remote-cal .rm-origin').fill('nextcloud.example.com');
  await page.locator('#remote-cal .rm-go').click();
  const owner = page.locator('#remote-cal .rm-owner');
  await expect(owner).toBeVisible();
  // the user first, as on "create", then the groups
  expect(await owner.inputValue()).toBe('alice');
  await owner.selectOption('group.familie');
  await page.locator('#remote-cal .rm-take').click();
  await expect.poll(() => bodies.length).toBe(2);
  expect(bodies.map(b => b.owner)).toEqual(['group.familie', 'group.familie']);
  await page.unrouteAll({ behavior: 'ignoreErrors' });
});

/* The suggestion list itself, with addresses put in by hand: it opens on focus, narrows as letters
   are typed, and ↑/↓ plus Enter take one. It has to be `fixed` and placed from script — the
   settings group around the field is `overflow: hidden` for its rounded corners, which clipped an
   absolutely positioned list away entirely while it reported itself as open. */
test('the address suggestions open, filter and can be taken with the keyboard', async ({ page }) => {
  /* The addresses come from the page's island, which the fixture has none of (nothing was ever
     signed in at). It is handed in with the answer, in the HEAD: `remote.js` reads it once while
     the bundle runs at the end of the body, so anything appended later is already too late. */
  await page.route('**/settings', async route => {
    const res = await route.fetch();
    const island = '<script type="application/json" id="rm-servers">["https://one.example.org","https://two.example.org"]</script>';
    await route.fulfill({ response: res, body: (await res.text()).replace('</head>', `${island}</head>`) });
  });
  await reloadAt(page, '#remote-cal');

  const field = page.locator('#remote-cal .rm-origin');
  const list = page.locator('#remote-cal .rm-sug');
  await field.click();
  await expect(list).toBeVisible();
  await expect(list.locator('.r')).toHaveCount(2);
  // and it is really on screen, not clipped by the group around it
  expect(await list.evaluate(el => {
    const r = el.getBoundingClientRect();
    return document.elementFromPoint(r.left + r.width / 2, r.top + 14)?.closest('.rm-sug') === el;
  })).toBe(true);

  await field.fill('two');
  await expect(list.locator('.r')).toHaveCount(1);
  await field.press('ArrowDown');
  await expect(list.locator('.r[aria-selected=true]')).toHaveText('https://two.example.org');
  await field.press('Enter');
  await expect(field).toHaveValue('https://two.example.org');
  await expect(list).not.toBeVisible();
  await expect(field).toHaveAttribute('aria-expanded', 'false');
  await page.unroute('**/settings');
});

// "add a device" (routes::connect): the wizard on another device, the app's Login Flow v2, the live jump to the end
const UA_DAVX5 = 'DAVx5/4.5.20-ose (at.bitfire.davdroid)';
test('add a device: explained, installed, one QR code opens DAVx⁵, the confirmation connects, the wizard follows live', async ({ page }) => {
  await reloadAt(page, '#account');
  await page.locator('#account .dev-hero a[href="#add"]').click();
  await page.locator('#add a[href="#add-android-about"]').click();
  await expect(page.locator('#add-android-about .about-list li')).toHaveCount(5); // what happens, before it happens
  await page.locator('#add-android-about a.btn-primary').click();
  // install and set up: a store row picks its code and its short link instead of leaving the page
  await expect(page.locator('#add-android-install .dots span')).toHaveText('Schritt 3 von 5 · Installieren');
  await expect(page.locator('#add-android-install p.miss')).toHaveCount(0); // only as Chrome's fallback
  await page.locator('#add-android-install [data-store="fd"]').click();
  await expect(page).toHaveURL(/#add-android-install$/);
  await expect(page.locator('[data-store-qr="fd"] .short')).toHaveText('f-droid.org/packages/at.bitfire.davdroid');
  await expect(page.locator('[data-store-qr="fd"] [data-qr="fd"] svg')).toBeVisible();
  await expect(page.locator('[data-store-qr="gp"]')).toBeHidden();
  await expect(page.locator('#add-android-install .wiz-strip .dx')).toHaveCount(4); // DAVx⁵'s first start
  await page.locator('#add-android-install .acts a.btn-primary').click();
  await expect(page).toHaveURL(/#add-android$/);
  await expect(page.locator('#add-android [data-qr="connect"] svg')).toBeVisible(); // the code arrived
  await expect(page.locator('#add-android .dx .fld')).toContainText('127.0.0.1'); // the schematic shows this server
  await expect(page.locator('#add-android [data-live]')).toBeVisible();
  // the code's link opens DAVx⁵ through a redirect (a camera app opens https only)
  const go = await fetch(BASE + '/connect/davx5', { redirect: 'manual' });
  expect(go.headers.get('location')).toMatch(/^intent:\/\/127\.0\.0\.1:\d+#Intent;scheme=davx5;package=at\.bitfire\.davdroid;S\.loginFlow=1;.*davx5%3Dmissing/);
  // the app: no browser metadata, no session
  const app = await fetch(BASE + '/index.php/login/v2', { method: 'POST', headers: { 'User-Agent': UA_DAVX5 } }); // Node's fetch: no cookie, no Origin
  expect(app.status).toBe(200);
  const start = await app.json();
  const path = u => new URL(u).pathname;
  // the user confirms in a browser tab of the same session: no app chrome, the setup's trail
  const tab = await page.context().newPage();
  await tab.goto(path(start.login));
  await expect(tab.locator('.connect-page h1')).toHaveText('DAVx⁵ verbinden?');
  await expect(tab.locator('.topbar')).toBeHidden();
  await expect(tab.locator('.connect-page .trail li[aria-current="step"]')).toHaveText('Verbinden');
  await tab.locator('.connect-page input[name=name]').fill('F4 Pixel');
  await tab.locator('.connect-page button[type=submit]').click();
  await expect(tab.locator('.connect-page h1')).toHaveText('Verbunden');
  await expect(tab.locator('.connect-page .next li')).toHaveCount(4); // ✕, account, calendars, done
  await expect(tab.locator('.connect-page .next .dx .fld').first()).toHaveText(/alice/); // DAVx⁵'s account name
  await tab.close();
  await expect(page).toHaveURL(/#add-android$/); // still waiting: the app has not fetched anything yet
  // the app fetches its credentials once
  const poll = () => fetch(BASE + path(start.poll.endpoint), { method: 'POST', body: new URLSearchParams({ token: start.poll.token }) });
  const g = await poll();
  expect(g.status).toBe(200);
  const cred = await g.json();
  expect(cred.loginName).toBe('alice');
  expect((await poll()).status).toBe(404);
  const ok = await basic('alice', cred.appPassword);
  expect((await ok.get('/api/history')).status()).toBe(200);
  await ok.dispose();
  // the wizard notices by itself; Back does not bounce into "done" again
  await expect(page).toHaveURL(/#add-done$/, { timeout: 10000 });
  await expect(page.locator('#add-done [data-device]')).toHaveText('F4 Pixel');
  await expect(page.locator('#add-done .next li')).toHaveCount(4); // on another device: what is left to do on the phone
  await page.goBack();
  await expect(page).toHaveURL(/#add-android-install$/);
  await page.goForward();
  await expect(page).toHaveURL(/#add-done$/);
  // "done" reloads the list: the device is there, and goes again
  await page.locator('#add-done a.btn-primary').click();
  const row = page.locator('#devices .cell', { hasText: 'F4 Pixel' });
  await expect(row).toBeVisible();
  await row.locator('button[data-confirm]').click();
  await page.locator('.ask [data-a="1"]').click();
  await expect(page.locator('#devices .cell', { hasText: 'F4 Pixel' })).toHaveCount(0);
});

// the iPhone path: no app — a profile link's QR code, its page on the phone, the profile, then Settings
const UA_IPHONE = 'Mozilla/5.0 (iPhone; CPU iPhone OS 26_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Mobile/15E148 Safari/604.1';
test('add a device: an iPhone scans a profile link, downloads its profile, and the wizard follows to the install', async ({ page }) => {
  await reloadAt(page, '#add');
  await page.locator('#add a[href="#add-ios-about"]').click();
  await expect(page.locator('#add-ios-about .about-list li')).toHaveCount(3);
  await page.locator('#add-ios-about a.btn-primary').click();
  await expect(page).toHaveURL(/#add-ios$/);
  await expect(page.locator('#add-ios .dots span')).toHaveText('Schritt 3 von 5 · Profil laden');
  await expect(page.locator('#add-ios [data-qr="apple"] svg')).toBeVisible(); // the profile link's code
  await expect(page.locator('#add-ios a[data-apple]')).toBeHidden(); // the download is the device's own button
  await expect(page.locator('#add-ios .ni .wiz-strip .dx')).toHaveCount(4); // Code Scanner, the link's page, Allow, Close
  await expect(page.locator('#add-ios .alt a')).toHaveAttribute('href', '#add-ios-manual'); // a locked iPhone: by hand
  await expect(page.locator('#add-ios [data-live]')).toBeVisible();
  // the code's link: the phone camera opens it in a fresh browser — no session needed
  const r = await page.request.post('/settings/connect/apple', { headers: { 'X-CSRF-Token': await page.locator('meta[name=csrf]').getAttribute('content'), Origin: BASE } });
  expect(r.status()).toBe(200);
  const link = await r.json();
  const phone = await page.context().browser().newContext({ baseURL: BASE, locale: 'de-DE', userAgent: UA_IPHONE });
  const tab = await phone.newPage();
  await tab.goto(new URL(link.url).pathname);
  await expect(tab.locator('.connect-page h1')).toHaveText('PractiCal auf diesem Gerät einrichten');
  await expect(tab.locator('.connect-page .warn')).toHaveCount(0); // Safari installs profiles
  await expect(tab.locator('.connect-page .wiz-strip .dx')).toHaveCount(6); // Allow, Close; then Settings ×4
  const dl = await phone.request.get(await tab.locator('.connect-page a.btn-primary').getAttribute('href'));
  expect(dl.status()).toBe(200);
  expect(dl.headers()['content-type']).toBe('application/x-apple-aspen-config');
  const xml = await dl.text();
  expect(xml).toContain('<string>com.apple.caldav.account</string>');
  expect(xml).toMatch(/caldav-compat\/principal\/alice</);
  const pw = xml.match(/<key>CalDAVPassword<\/key><string>([^<]+)</)[1];
  const ok = await basic('alice', pw);
  expect((await ok.get('/api/history')).status()).toBe(200); // the profile's password is a working app token
  await ok.dispose();
  await phone.close();
  // the wizard of the code that was not scanned notices the download of the other link too: one user, one device
  await expect(page).toHaveURL(/#add-ios-install$/, { timeout: 10000 });
  await expect(page.locator('#add-ios-install [data-device]')).toHaveText('iPhone');
  await expect(page.locator('#add-ios-install .wiz-strip .dx')).toHaveCount(4); // Settings: loaded, install, warning, done
  await page.locator('#add-ios-install .acts a.btn-primary').click();
  await expect(page).toHaveURL(/#add-done$/);
  await expect(page.locator('#add-done .p-android')).toBeHidden(); // no DAVx⁵ steps after an iPhone
  await page.locator('#add-done a.btn-primary').click();
  const row = page.locator('#devices .cell', { hasText: 'iPhone' });
  await expect(row).toBeVisible();
  await row.locator('button[data-confirm]').click();
  await page.locator('.ask [data-a="1"]').click();
  await expect(page.locator('#devices .cell', { hasText: 'iPhone' })).toHaveCount(0);
});

test('add a device: on the iPhone itself one button downloads the profile', async ({ browser }) => {
  const ctx = await browser.newContext({ baseURL: BASE, locale: 'de-DE', userAgent: UA_IPHONE, viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
  const page = await ctx.newPage();
  await loginAs(page, 'bob'); // not alice: a download ends every open wizard of its user, and the test above waits in one
  await page.goto('/settings#add-ios');
  await expect(page.locator('#settings')).toHaveAttribute('data-ios', '1');
  const btn = page.locator('#add-ios a[data-apple]');
  await expect(btn).toHaveAttribute('href', /\/connect\/apple\/[A-Za-z0-9]{32}\/PractiCal\.mobileconfig$/);
  await expect(page.locator('#add-ios [data-qr="apple"]')).toBeHidden(); // no code on the device itself
  await expect(page.locator('#add-ios [data-io-other]')).toBeHidden(); // Safari installs it
  const dl = await page.request.get(await btn.getAttribute('href'));
  expect(dl.status()).toBe(200);
  await expect(page).toHaveURL(/#add-ios-install$/, { timeout: 10000 });
  await expect(page.locator('#add-ios-install [data-device]')).toHaveText('iPhone');
  await page.goto('/settings?done=1#account'); // a new load: the list as the server has it now
  await expect(page.locator('#devices .cell', { hasText: 'iPhone' })).toHaveCount(0); // listed once it connected, not on download
  const pw = (await dl.text()).split('<key>CalDAVPassword</key><string>')[1].split('<')[0];
  const dev = await basic('bob', pw); // the iPhone's first request: RustiCal records the use
  expect((await dev.get('/api/history')).status()).toBe(200);
  await dev.dispose();
  await page.reload(); // goto() to the same URL with the same hash would not load the page again
  const row = page.locator('#devices .cell', { hasText: 'iPhone' });
  await expect(row.locator('.sub')).toHaveText('zuletzt verbunden vor Kurzem'); // tidy up
  await row.locator('button[data-confirm]').click();
  await page.locator('.ask [data-a="1"]').click();
  await expect(page.locator('#devices .cell', { hasText: 'iPhone' })).toHaveCount(0);
  await ctx.close();
});

test('add a device: an iPhone by hand — the way through Settings, the values to copy, contacts and groups', async ({ browser }) => {
  const ctx = await browser.newContext({ baseURL: BASE, locale: 'de-DE', userAgent: UA_IPHONE, viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
  await ctx.grantPermissions(['clipboard-read', 'clipboard-write'], { origin: BASE });
  const page = await ctx.newPage();
  await loginAs(page, 'carol'); // a user of her own: the password made here must not land in another test's list
  await page.goto('/settings#add');
  await page.locator('#add a[href="#add-ios-manual"]').click();
  await expect(page.locator('#add-ios-manual .wiz-strip .dx')).toHaveCount(7); // Home Screen … CalDAV account
  await page.locator('#add-ios-manual .acts a.btn-primary').click();
  await expect(page).toHaveURL(/#add-ios-values$/);
  await expect(page.locator('#add-ios-values [data-qr="apple-manual"]')).toBeHidden(); // the code is for another device
  const pw = page.locator('#iv-p');
  await expect(pw).toHaveText(''); // no password before it is asked for
  await page.locator('#add-ios-values [data-io-pw-copy]').click();
  await expect(pw).toHaveText(/^[0-9a-f]{4}_[A-Za-z0-9]{64}$/); // made now, copied from the hidden element
  await expect(pw).toBeHidden();
  await expect(page.locator('#iv-u')).toHaveText('carol');
  await expect(page.locator('#add-ios-values .io-form .oi button')).toHaveCount(4); // a copy button per value
  const ok = await basic('carol', await pw.textContent());
  expect((await ok.get('/api/history')).status()).toBe(200);
  await ok.dispose();
  await page.locator('#add-ios-values .acts a.btn-primary').click();
  await expect(page.locator('#ic-p')).toHaveText(await pw.textContent()); // the contacts account: the same values
  await page.goBack(); await page.goForward(); // back and on: still the one password
  await expect(page.locator('#ic-p')).toHaveText(await pw.textContent());
  await page.locator('#add-ios-more .acts a.btn-primary').click();
  await expect(page).toHaveURL(/#add-done$/);
  await expect(page.locator('#add-done [data-device]')).toHaveText('iPhone');
  await expect(page.locator('#add-done .p-iosm')).toBeVisible();
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(await pw.textContent());
  // on a computer: a code opens the guide on the iPhone (a profile link's page, no login); the values to type, the password behind the eye
  const desk = await browser.newContext({ baseURL: BASE, locale: 'de-DE' });
  const d = await desk.newPage();
  await loginAs(d, 'carol');
  await d.goto('/settings#add-ios-values');
  await expect(d.locator('#add-ios-values [data-qr="apple-manual"] svg')).toBeVisible();
  await expect(d.locator('#add-ios-values [data-io-pw-copy]')).toBeHidden(); // copying is for the iPhone
  await expect(d.locator('#add-ios-values .io-form [data-copy]').first()).toBeHidden(); // no copy buttons on a computer
  await expect(d.locator('#iv-p')).toHaveText(''); // no password yet
  await d.locator('#add-ios-values [data-io-pw-show]').click();
  await expect(d.locator('#iv-p')).toBeVisible();
  await expect(d.locator('#iv-p')).toHaveText(/^[0-9a-f]{4}_[A-Za-z0-9]{64}$/);
  // a new run that ends on the computer before the phone fetched its password: no Android name, nothing "connected"
  await d.goto('/settings#add');
  await d.locator('#add a[href="#add-ios-manual"]').click();
  await d.locator('#add-ios-manual .acts a.btn-primary').click();
  await d.locator('#add-ios-values .acts a.btn-primary').click();
  await d.locator('#add-ios-more .acts a.btn-primary').click();
  await expect(d).toHaveURL(/#add-done$/);
  await expect(d.locator('#add-done [data-device]')).toHaveText('iPhone');
  await expect(d.locator('#add-done .cell .sub')).toBeHidden();
  await desk.close();
  await page.goto('/settings?done=1#account'); // the used password is a device; the one only shown on the computer is not (yet)
  await expect(page.locator('#devices .cell', { hasText: 'iPhone' })).toHaveCount(1);
  await page.locator('#devices .cell', { hasText: 'iPhone' }).locator('button[data-confirm]').click(); // tidy up
  await page.locator('.ask [data-a="1"]').click();
  await expect(page.locator('#devices .cell', { hasText: 'iPhone' })).toHaveCount(0);
  await ctx.close();
});

test('add a device: an iPhone that refuses the profile goes on by hand, and the computer follows to that end', async ({ page }) => {
  await reloadAt(page, '#add-ios');
  await expect(page.locator('#add-ios [data-qr="apple"] svg')).toBeVisible();
  const r = await page.request.post('/settings/connect/apple', { headers: { 'X-CSRF-Token': await page.locator('meta[name=csrf]').getAttribute('content'), Origin: BASE } });
  const path = new URL((await r.json()).url).pathname;
  const phone = await page.context().browser().newContext({ baseURL: BASE, locale: 'de-DE', userAgent: UA_IPHONE });
  await phone.grantPermissions(['clipboard-read', 'clipboard-write'], { origin: BASE });
  const tab = await phone.newPage();
  await tab.goto(path);
  const dl = await phone.request.get(await tab.locator('.connect-page a.btn-primary').getAttribute('href'));
  expect(dl.status()).toBe(200); // the profile, which a company iPhone then refuses to install
  await expect(page).toHaveURL(/#add-ios-install$/, { timeout: 10000 });
  await tab.locator(`.connect-page a[href="${path}/manual"]`).click(); // "Unable to Install Profile": by hand
  await expect(tab.locator('.connect-page .wiz-strip').first().locator('.dx')).toHaveCount(7);
  await expect(tab.locator('#iv-p')).toHaveText(''); // no password before it is asked for
  await tab.locator('#iv-p').locator('xpath=ancestor::div[1]').locator('[data-io-pw-copy]').click();
  await expect(tab.locator('#iv-p')).toHaveText(/^[0-9a-f]{4}_[A-Za-z0-9]{64}$/);
  const pw = await tab.locator('#iv-p').textContent();
  expect(await tab.evaluate(() => navigator.clipboard.readText())).toBe(pw);
  await expect(tab.locator('#ic-p')).toHaveText(pw); // contacts: the same password, no second one
  const ok = await basic('alice', pw);
  expect((await ok.get('/api/history')).status()).toBe(200);
  await ok.dispose();
  // the computer leaves the install step of a profile that was never installed: the manual path's end
  await expect(page).toHaveURL(/#add-done$/, { timeout: 10000 });
  await expect(page.locator('#add-done .p-iosm')).toBeVisible();
  await expect(page.locator('#add-done [data-device]')).toHaveText('iPhone');
  await tab.locator('[data-io-finish]').click();
  await expect(tab.locator('.ask')).toBeVisible();
  await phone.close();
  await page.locator('#add-done a.btn-primary').click(); // tidy up
  await page.locator('#devices .cell', { hasText: 'iPhone' }).locator('button[data-confirm]').click();
  await page.locator('.ask [data-a="1"]').click();
  await expect(page.locator('#devices .cell', { hasText: 'iPhone' })).toHaveCount(0);
});

test('add a device: on the Android device itself the wizard explains, installs, then connects with one tap', async ({ browser }) => {
  const ctx = await browser.newContext({ baseURL: BASE, locale: 'de-DE', userAgent: 'Mozilla/5.0 (Linux; Android 14; Pixel 9) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0 Mobile Safari/537.36', viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
  const page = await ctx.newPage();
  await loginAs(page, 'alice');
  await page.goto('/settings#add'); // a page of its own context: settle() knows only the fixture's pages
  await expect(page.locator('#settings')).toHaveAttribute('data-android', '1');
  await expect(page.locator('#add .dots span')).toHaveText('Schritt 1 von 5');
  await page.locator('#add a[href="#add-android-about"]').click();
  await page.locator('#add-android-about a.btn-primary').click();
  await expect(page.locator('#add-android-install .wiz-storeqr')).toBeHidden(); // no codes on the device itself
  await expect(page.locator('#add-android-install [data-store="gp"]')).toHaveAttribute('href', /play\.google\.com/);
  await page.locator('#add-android-install .acts a.btn-primary').click();
  await expect(page.locator('#add-android-connect .dots span')).toHaveText('Schritt 4 von 5 · Verbinden');
  const btn = page.locator('#add-android-connect a.btn-primary');
  await expect(btn).toHaveAttribute('href', /^intent:\/\/[^#]+#Intent;scheme=davx5;package=at\.bitfire\.davdroid;S\.loginFlow=1;S\.browser_fallback_url=.+davx5%3Dmissing%23add-android-install;end$/);
  // without DAVx⁵ Chrome opens the fallback: the install page says why it is there
  await page.goto('/settings?davx5=missing#add-android-install');
  await expect(page.locator('#add-android-install p.miss')).toBeVisible();
  // the step meant for another device sends the device itself on
  await page.goto('/settings#add-android');
  await expect(page).toHaveURL(/#add-android-connect$/);
  await ctx.close();
});
