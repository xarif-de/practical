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
  await reloadAt(page, '#account');
  await page.locator('#account form[action="/settings/tokens"] input[name=name]').fill('F4 Token');
  await Promise.all([page.waitForURL(/\/settings#account$/), page.locator('#account form[action="/settings/tokens"] button[type=submit]').click()]);
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

/* The remote address row. Its three parts are all browser behaviour, which is why they are tested
   here and not in Rust: the field has no dropdown marker of its own, Enter in it CONNECTS instead
   of submitting the batch around it, and the arrow is the row's one action so it carries the
   accent. No Nextcloud is needed — what is asserted happens before any request is answered. */
test('the remote address row: no dropdown marker, Enter connects, the arrow is the accent', async ({ page }) => {
  await reloadAt(page, '#remote-cal');
  const field = page.locator('#remote-cal .rm-origin');
  const button = page.locator('#remote-cal .rm-go');
  await expect(field).toBeVisible();

  // the marker is gone, and the placeholder is not clipped by the button beside it
  const clipped = await field.evaluate(el => {
    const before = el.value;
    el.value = el.placeholder;
    const over = el.scrollWidth - el.clientWidth;
    el.value = before;
    return over;
  });
  expect(clipped).toBeLessThanOrEqual(0);
  /* The marker itself: Chromium answers getComputedStyle on that pseudo-element with the INPUT's
     own values (measured: display "block", width 169 px — the field's width), so it cannot be read
     back. What is checkable is that the rule hiding it is in the stylesheet the page loaded. */
  const hides = await page.evaluate(() => [...document.styleSheets]
    .flatMap(sh => { try { return [...sh.cssRules]; } catch { return []; } })
    .some(r => r.selectorText && r.selectorText.includes('calendar-picker-indicator')
      && r.selectorText.includes('rm-in') && r.style.display === 'none'));
  expect(hides).toBe(true);

  // the arrow: accent background, white glyph
  const look = await button.evaluate(el => ({ bg: getComputedStyle(el).backgroundColor, ico: getComputedStyle(el.querySelector('.ico')).color }));
  expect(look.bg).not.toBe('rgba(0, 0, 0, 0)');
  expect(look.bg).toBe(await page.evaluate(() => {
    const p = document.createElement('i');
    p.style.color = 'var(--accent)';
    document.body.appendChild(p);
    const c = getComputedStyle(p).color;
    p.remove();
    return c;
  }));
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
