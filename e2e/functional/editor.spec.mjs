// Event/rule editor end to end (inventory §2.3, §3): create, edit, move, delete, series and exceptions,
// every card, rule mode, templates, the unsaved-changes guard. Own data only ("F2 …" in November 2026);
// seeded events are read, never written; the alarm presets are restored.
import { api, expect, settle, test } from '../lib/test.mjs';
import { CAL, addCard, answer, createApi, detail, fillWhen, keyOf, next, one, openEdit, openNew, save, instances } from './_editor.mjs';

test.afterEach(async ({ page }) => {
  expect(page.__dialogs, 'no alert() from the app').toEqual([]);
});

test('create a timed event through the editor', async ({ page }) => {
  await openNew(page, '2026-11-02');
  await expect(page.locator('#edform input[name="date"]')).toHaveValue('2026-11-02');
  await fillWhen(page, { title: 'F2 Termin', from: '14:00', to: '15:30', cal: CAL.privat });
  await save(page);
  const e = await one(page, '2026-11-02', 'F2 Termin');
  expect(e.cal_id).toBe(CAL.privat.split('/')[1]);
  const d = await detail(page, keyOf(e));
  expect([d.start, d.end, d.all_day]).toEqual(['2026-11-02T14:00:00', '2026-11-02T15:30:00', false]);
  await page.reload();
  await expect(page.locator('.v-month .day[data-date="2026-11-02"] .ev', { hasText: 'F2 Termin' })).toContainText('14:00');
});

test('create an all-day event over two days', async ({ page }) => {
  await openNew(page, '2026-11-03');
  await fillWhen(page, { title: 'F2 Ganztag', allDay: true, cal: CAL.privat });
  await page.locator('#edform input[name="end_date"]').fill('2026-11-04');
  await expect(page.locator('#ed-times')).toBeHidden();
  await save(page);
  const e = await one(page, '2026-11-04', 'F2 Ganztag');
  const d = await detail(page, keyOf(e));
  expect(d.all_day).toBe(true);
  expect(d.start.slice(0, 10)).toBe('2026-11-03');
  await page.goto('/calendar?view=month&date=2026-11-03');
  await expect(page.locator('.ev.allday', { hasText: 'F2 Ganztag' }).first()).toBeVisible();
});

test('edit title and end time (resize), then move to another calendar', async ({ page }) => {
  await page.goto('/calendar?view=month&date=2026-11-05');
  const key = await createApi(page, { summary: 'F2 Bearbeiten', start: '2026-11-05T09:00:00', end: '2026-11-05T10:00:00', cal: CAL.privat });
  await openEdit(page, key, '2026-11-05');
  await fillWhen(page, { title: 'F2 Bearbeitet', to: '11:15' });
  await save(page);
  let d = await detail(page, key);
  expect([d.summary, d.start, d.end]).toEqual(['F2 Bearbeitet', '2026-11-05T09:00:00', '2026-11-05T11:15:00']);
  // move: another calendar = a new key, the old one is gone
  await openEdit(page, key, '2026-11-05');
  await page.locator('#ed-cal').selectOption(CAL.work);
  await save(page);
  const moved = await one(page, '2026-11-05', 'F2 Bearbeitet');
  expect(`${moved.principal}/${moved.cal_id}`).toBe(CAL.work);
  const old = await page.request.get(`/api/events/${key}`);
  expect(old.status()).toBe(404);
  d = await detail(page, keyOf(moved));
  expect(d.end).toBe('2026-11-05T11:15:00');
});

test('delete asks first; "no" keeps, "yes" removes', async ({ page }) => {
  await page.goto('/calendar?view=month&date=2026-11-06');
  const key = await createApi(page, { summary: 'F2 Löschen', start: '2026-11-06T09:00:00', end: '2026-11-06T10:00:00' });
  await openEdit(page, key, '2026-11-06', { edit: false });
  await expect(page.locator('#ed-delete')).toBeDisabled(); // view mode: unlock first
  await page.locator('#ed-lock').click();
  await page.locator('#ed-delete').click();
  await answer(page, false);
  await expect(page.locator('#ed')).toBeVisible();
  expect((await page.request.get(`/api/events/${key}`)).status()).toBe(200);
  await page.locator('#ed-delete').click();
  await answer(page, true);
  await expect(page.locator('#ed')).toBeHidden();
  expect((await page.request.get(`/api/events/${key}`)).status()).toBe(404);
  await page.reload();
  await expect(page.locator(`article.lnk[data-key="${key}"]`)).toHaveCount(0);
});

test('weekly series: edit the whole series vs strike one occurrence and add one date', async ({ page }) => {
  await openNew(page, '2026-11-09');
  await fillWhen(page, { title: 'F2 Serie', from: '08:00', to: '08:30', cal: CAL.privat });
  await addCard(page, 'repeat');
  await page.locator('#ed-rr [data-w="freq"] button[data-v="WEEKLY"]').click();
  // end after 4 occurrences
  await page.locator('#ed-rr .pillbtn[data-s="e"]').click();
  await page.locator('.rrpop button[data-v="count"]').click();
  const q = page.locator('.rrpop input, .ask input').first();
  await q.fill('4');
  await q.press('Enter');
  await expect(page.locator('#ed-rr .pillbtn[data-s="e"]')).toContainText('4');
  await save(page);
  const first = await one(page, '2026-11-09', 'F2 Serie');
  const key = keyOf(first);
  let d = await detail(page, key);
  expect(d.rrule).toMatch(/FREQ=WEEKLY/);
  expect(d.rrule).toMatch(/COUNT=4/);
  for (const day of ['2026-11-16', '2026-11-23', '2026-11-30']) expect(await instances(page, day, 'F2 Serie')).toHaveLength(1);
  expect(await instances(page, '2026-12-07', 'F2 Serie')).toHaveLength(0);
  // the whole series: a new title reaches every occurrence
  await openEdit(page, key, '2026-11-16');
  await fillWhen(page, { title: 'F2 Serie neu' });
  await save(page);
  for (const day of ['2026-11-09', '2026-11-16', '2026-11-23', '2026-11-30']) expect(await instances(page, day, 'F2 Serie neu')).toHaveLength(1);
  // one occurrence: struck on the strip (exception mode), and an extra date
  await openEdit(page, key, '2026-11-09');
  await page.locator('#ed-rr [data-w="mode"] button[data-v="ex"]').click();
  await page.locator('#ed-rr b[data-d="2026-11-16"]').click();
  await page.locator('#ed-rr b[data-d="2026-11-19"]').click();
  await save(page);
  d = await detail(page, key);
  expect(d.exdates.map(x => x.slice(0, 10))).toEqual(['2026-11-16']);
  expect(d.rdates.map(x => x.slice(0, 10))).toEqual(['2026-11-19']);
  expect(await instances(page, '2026-11-16', 'F2 Serie neu')).toHaveLength(0);
  expect(await instances(page, '2026-11-19', 'F2 Serie neu')).toHaveLength(1);
  expect(await instances(page, '2026-11-23', 'F2 Serie neu')).toHaveLength(1);
  await page.goto('/calendar?view=day&date=2026-11-09');
  await page.locator(`article.lnk[data-key="${key}"] h3`).click();
  await expect(page.locator('#ed-rrule-text')).not.toBeEmpty();
  await expect(page.locator('#ed-exdates .row')).toHaveCount(1);
});

test('alarms: add, remove, preset star (restored)', async ({ page }) => {
  await page.goto('/calendar?view=month&date=2026-11-10');
  const before = await api(page, 'GET', '/api/alarm-presets');
  const key = await createApi(page, { summary: 'F2 Erinnerung', start: '2026-11-10T09:00:00', end: '2026-11-10T10:00:00' });
  try {
    await openEdit(page, key, '2026-11-10');
    await addCard(page, 'alarm'); // adds one alarm row
    await expect(page.locator('#ed-alarm-card .row.al')).toHaveCount(1);
    await save(page);
    let d = await detail(page, key);
    expect(d.alarms).toHaveLength(1);
    // the star in an alarm's details makes it a preset (PUT /api/alarm-presets) and back
    await openEdit(page, key, '2026-11-10');
    await page.locator('#ed-alarm-card .row.al button[data-al-open]').first().click();
    const star = page.locator('#ed-alarm-card .aldet [data-w="pset"]');
    const sig = p => JSON.stringify([p.action, p.minutes, !!p.related_end]);
    const a = d.alarms[0];
    const had = (before || []).some(p => sig(p) === sig(a));
    let put = page.waitForResponse(r => r.url().endsWith('/api/alarm-presets') && r.request().method() === 'PUT');
    await star.click();
    expect((await put).ok()).toBe(true);
    const after = await api(page, 'GET', '/api/alarm-presets');
    expect(after.some(p => sig(p) === sig(a))).toBe(!had);
    put = page.waitForResponse(r => r.url().endsWith('/api/alarm-presets') && r.request().method() === 'PUT');
    await star.click();
    await put;
    expect((await api(page, 'GET', '/api/alarm-presets')).some(p => sig(p) === sig(a))).toBe(had);
    // remove the alarm
    await page.locator('#ed-alarm-card .row.al [data-rm]').first().click();
    await save(page);
    d = await detail(page, key);
    expect(d.alarms).toHaveLength(0);
  } finally {
    await api(page, 'PUT', '/api/alarm-presets', before);
  }
  expect(await api(page, 'GET', '/api/alarm-presets')).toEqual(before);
});

test('links, related event, categories typeahead, location, note mention, status', async ({ page }) => {
  await page.goto('/calendar?view=month&date=2026-11-11');
  const key = await createApi(page, { summary: 'F2 Karten', start: '2026-11-11T09:00:00', end: '2026-11-11T10:00:00' });
  await openEdit(page, key, '2026-11-11');
  // links: an internet link and a related event found by the search
  await addCard(page, 'links');
  await page.locator('#ed-lk-url').fill('example.org/f2-plan');
  await page.locator('#ed-lk-addbtn').click();
  await expect(page.locator('#ed-lk-list')).toContainText('example.org');
  await page.locator('#ed-lk-search').pressSequentially('Projekt-Review');
  await page.locator('.rrpop .lk-hit', { hasText: 'Projekt-Review' }).first().click();
  // categories: typeahead suggestion of an existing one
  await page.locator('#ed-ct-in').pressSequentially('Spo');
  await page.locator('.rrpop .ct-hit:not(.new)', { hasText: 'Sport' }).click();
  await expect(page.locator('#ed-ct-rows .chip')).toHaveText(['Sport']);
  // location typed (committed on leaving the field)
  await addCard(page, 'where');
  await page.locator('#ed-locin').fill('Altmarkt 1, 01067 Dresden');
  await page.locator('#edh').focus();
  // note: an @ mention picked from the suggestions
  await addCard(page, 'note');
  const note = page.locator('textarea[name="description"]');
  await note.pressSequentially('Mit @Ann');
  await page.locator('.rrpop .lk-hit', { hasText: 'Anna Albrecht' }).click();
  await expect(note).toHaveValue(/@Anna Albrecht/);
  await expect(page.locator('#ed-note-found [data-nt]').first()).toBeVisible();
  // status: a tile's popover sets "tentative"
  await addCard(page, 'status');
  await page.locator('#ed-status-tiles .stile[data-sel="status"]').click();
  await page.locator('.rrpop button[data-v="TENTATIVE"]').click();
  await save(page);
  const d = await detail(page, key);
  expect(d.links.map(l => l.uri)).toContain('https://example.org/f2-plan');
  expect(d.related.map(r => r.uid)).toContain('review@seed');
  expect(d.categories).toEqual(['Sport']);
  expect(d.location).toBe('Altmarkt 1, 01067 Dresden');
  expect(d.description).toContain('@Anna Albrecht');
  expect(d.status).toBe('TENTATIVE');
  // the view mode shows them after a reload
  await page.reload();
  await openEdit(page, key, '2026-11-11', { edit: false });
  await expect(page.locator('#ed-addr-lines')).toContainText('Altmarkt 1');
  await expect(page.locator('#ed-status-text')).not.toBeEmpty();
});

test('colour picker sets the event colour; back to the calendar colour', async ({ page }) => {
  await page.goto('/calendar?view=month&date=2026-11-12');
  const key = await createApi(page, { summary: 'F2 Farbe', start: '2026-11-12T09:00:00', end: '2026-11-12T10:00:00' });
  await openEdit(page, key, '2026-11-12');
  await page.locator('#ed-color-btn').click();
  const sw = page.locator('.cpk .cpk-sw:not(.inh):not(.cpk-add)').first();
  const hex = await sw.getAttribute('data-v');
  await sw.click();
  await page.keyboard.press('Escape');
  await expect(page.locator('.cpk')).toHaveCount(0);
  await save(page);
  expect((await detail(page, key)).color.toLowerCase()).toBe(hex.toLowerCase());
  await page.goto('/calendar?view=month&date=2026-11-12');
  const chip = page.locator(`.v-month .ev[data-key="${key}"]`);
  await expect(chip).toBeVisible();
  // the inherit circle: the calendar's colour again (no own colour)
  await openEdit(page, key, '2026-11-12');
  await page.locator('#ed-color-btn').click();
  await page.locator('.cpk .cpk-sw.inh').click();
  await page.keyboard.press('Escape');
  await save(page);
  expect((await detail(page, key)).color).toBe('');
});

test('location map dialog opens and cancels', async ({ page }) => {
  await page.goto('/calendar?view=month&date=2026-11-13');
  const key = await createApi(page, { summary: 'F2 Karte', start: '2026-11-13T09:00:00', end: '2026-11-13T10:00:00', location: 'Altmarkt, Dresden' });
  await openEdit(page, key, '2026-11-13');
  await page.locator('#ed-locp [data-geo="map"]').click();
  await expect(page.locator('#geomap')).toBeVisible();
  await expect(page.locator('#gm-map .leaflet-container, #gm-map.leaflet-container')).toHaveCount(1);
  // its tiles load under the Content-Security-Policy (img-src names the tile host; lib/test.mjs serves a gray PNG)
  await expect.poll(() => page.locator('#gm-map img.leaflet-tile').evaluateAll(t => t.filter(i => i.complete && i.naturalWidth === 256).length)).toBeGreaterThan(0);
  // zooming unloads tiles: Leaflet gives each its data: GIF (img-src allows data:)
  await page.locator('#gm-map .leaflet-control-zoom-in').click();
  await expect.poll(() => page.locator('#gm-map img.leaflet-tile').evaluateAll(t => t.filter(i => i.complete && i.naturalWidth === 256).length)).toBeGreaterThan(0);
  await page.keyboard.press('Escape'); // the map closes first, the editor stays
  await expect(page.locator('#geomap')).toBeHidden();
  await expect(page.locator('#ed')).toBeVisible();
});

test('rule mode: save and delete a rule for a new category', async ({ page }) => {
  await page.goto('/calendar?rule=F2-Kat&back=%2Fsettings%23categories');
  await page.locator('#ed.rulemode:not([hidden])').waitFor();
  await expect(page.locator('#ed-delete')).toBeDisabled(); // no rule yet
  await addCard(page, 'where');
  await page.locator('#ed-locin').fill('F2 Ort, Dresden');
  await page.locator('#edh').focus();
  await page.locator('#ed-save').click();
  await page.waitForURL(/\/settings#categories/);
  let rules = await api(page, 'GET', '/api/category-rules?scope=event');
  expect(rules['F2-Kat'].where.location).toBe('F2 Ort, Dresden');
  await page.goto('/calendar?rule=F2-Kat&back=%2Fsettings%23categories');
  await page.locator('#ed.rulemode:not([hidden])').waitFor();
  await expect(page.locator('#ed-locin')).toHaveValue('F2 Ort, Dresden');
  await page.locator('#ed-delete').click();
  await answer(page, true);
  await page.waitForURL(/\/settings#categories/);
  rules = await api(page, 'GET', '/api/category-rules?scope=event');
  expect(rules['F2-Kat']).toBeUndefined();
  expect(Object.keys(rules)).toEqual(expect.arrayContaining(['Urlaub', 'Arbeit']));
});

test('template suggestion fills a new event from a past one', async ({ page }) => {
  await openNew(page, '2026-11-17');
  const sug = page.waitForResponse(r => r.url().includes('/api/events/suggest'));
  await page.locator('#edh').pressSequentially('Team');
  await sug;
  await page.locator('.tplpop .lk-hit', { hasText: 'Team-Standup' }).first().click();
  await expect(page.locator('#edh')).toHaveValue('Team-Standup');
  await expect(page.locator('#edform input[name="date"]')).toHaveValue('2026-11-17'); // the day stays the dialog's
  await expect(page.locator('#ed-cal')).toHaveValue(CAL.work);
  await fillWhen(page, { title: 'F2 Vorlage' });
  // a template brings its series along; a one-off copy here
  await page.locator('#ed-rr [data-w="freq"] button[data-v=""]').click();
  await save(page);
  const e = await one(page, '2026-11-17', 'F2 Vorlage');
  const d = await detail(page, keyOf(e));
  expect([d.start.slice(11, 16), d.end.slice(11, 16)]).toEqual(['09:00', '09:15']);
  expect(d.categories).toContain('Arbeit');
  expect(d.alarms.length).toBeGreaterThan(0);
  expect(d.rrule).toBeNull();
});

test('unsaved changes: back keeps editing, discard drops; Esc closes a clean dialog', async ({ page }) => {
  await page.goto('/calendar?view=month&date=2026-11-18');
  const key = await createApi(page, { summary: 'F2 Ungesichert', start: '2026-11-18T09:00:00', end: '2026-11-18T10:00:00' });
  await openEdit(page, key, '2026-11-18', { edit: false });
  await page.keyboard.press('Escape'); // view mode, nothing changed: closes
  await expect(page.locator('#ed')).toBeHidden();
  await openEdit(page, key, '2026-11-18');
  await page.locator('#edh').fill('F2 Geändert');
  await page.keyboard.press('Escape');
  await expect(page.locator('#ed-confirm')).toBeVisible();
  await page.locator('#ed-back').click();
  await expect(page.locator('#ed-confirm')).toBeHidden();
  await expect(page.locator('#edh')).toHaveValue('F2 Geändert');
  await page.locator('#ed-close').click();
  await page.locator('#ed-discard').click();
  await expect(page.locator('#ed')).toBeHidden();
  expect((await detail(page, key)).summary).toBe('F2 Ungesichert');
  // reset puts the stored values back
  await openEdit(page, key, '2026-11-18');
  await page.locator('#edh').fill('F2 Zurück');
  await page.locator('#ed-reset').click();
  await expect(page.locator('#edh')).toHaveValue('F2 Ungesichert');
});

test('view mode of seeded events: day line, people, attachment download, copy', async ({ page }) => {
  await openEdit(page, `${CAL.work}/review@seed`, '2026-10-14', { edit: false });
  // the day line lists the day's timed events (GET /api/events)
  await expect(page.locator('#ed-dayline .blk.me')).toHaveText('Projekt-Review');
  // (other spec files may add events on this day: the seeded ones must be there)
  for (const t of ['Team-Standup', 'Kundentermin', 'Mittagessen mit Bob', 'Zahnarzt Dr. Weiß']) await expect(page.locator('#ed-dayline .blk', { hasText: t })).toHaveCount(1);
  await expect(page.locator('#ed-people .pchip')).toHaveCount(4);
  await page.locator('#ed-people .pchip', { hasText: 'Dora Dahl' }).click();
  await expect(page.locator('#ed-people-detail .pdetail')).toContainText('dora@example.org');
  // the day line toggle sticks (localStorage)
  await page.locator('#ed-tl').click();
  await expect(page.locator('#ed')).toHaveClass(/no-tl/);
  await page.reload();
  await page.locator(`article.lnk[data-key="${CAL.work}/review@seed"] h3`).click();
  await expect(page.locator('#ed')).toHaveClass(/no-tl/);
  await settle(page);
  // the demo event's embedded attachment downloads its bytes
  await openEdit(page, `${CAL.privat}/demo-event-full@xarif.de`, '2026-10-13', { edit: false });
  // (window.open is recorded by lib/test.mjs; the URL it opens is fetched here)
  await page.locator('#ed-lk-list [data-lact="download"]').first().click();
  const [url] = await page.evaluate(() => window.__opened);
  expect(url).toMatch(/^\/api\/events\/alice\/5eed0000-0000-4000-8000-000000000002\/demo-event-full@xarif\.de\/attach\/\d+$/);
  expect(await (await page.request.get(url)).text()).toBe('Hallo Demo!');
  await page.locator('#ed-lk-list [data-lact="open"]').first().click();
  expect((await page.evaluate(() => window.__opened))[1]).toMatch(/^https:\/\//);
  // the day document's copy button
  await page.goto('/calendar?view=day&date=2026-10-14');
  await page.locator(`article.lnk[data-key="${CAL.work}/review@seed"] [data-copytext]`).first().click();
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe('Rathaus, Dr.-Külz-Ring 19, 01067 Dresden');
  // the day document's map buttons open OSM / Google Maps (recorded, nothing leaves)
  await page.locator(`article.lnk[data-key="${CAL.work}/review@seed"] [data-map="osm"]`).click();
  expect(await page.evaluate(() => window.__opened)).toEqual(['https://www.openstreetmap.org/search?query=51.049259%2C13.738144']);
  await expect(page.locator('#ed')).toBeHidden();
});
