// Round-2 fixes of the "bugs" package: one test per user-visible fix (B1..B11 of the bug list in
// `git show 901d09e:docs/performance-2026-10.md`, "Known bugs"). Shared DB with the other functional specs: own objects carry "F9", prefs go back to the seed.
import fs from 'node:fs';
import path from 'node:path';
import { test, expect, api } from '../lib/test.mjs';
import { BASE, WORK } from '../lib/env.mjs';
import { savePrefs, settingsForm, createEvent } from './_set.mjs';

const FREUNDE = '5eed0000-0000-4000-8000-000000000011'; // alice's "Freunde" (fixture)
const plusDay = d => new Date(Date.parse(d + 'T00:00:00Z') + 864e5).toISOString().slice(0, 10);

test.afterEach(async ({ page }) => { await savePrefs(page); });

test('B1: a day with one event says "1 Termin", not "1 Termine"', async ({ page }) => {
  await page.goto('/calendar');
  let day = null;
  for (let d = '2027-05-03'; !day; d = plusDay(d)) if (!(await api(page, 'GET', `/api/events?start=${d}&end=${plusDay(d)}`)).length) day = d;
  const key = await createEvent(page, 'F9 Einzeltermin', { start: `${day}T14:00:00`, end: `${day}T15:00:00` });
  try {
    await page.goto(`/calendar?view=day&date=${day}`);
    await expect(page.locator(`.doc[data-date="${day}"] > .sub`)).toHaveText(/^1 Termin · /);
  } finally {
    await api(page, 'DELETE', `/api/events/${key}`);
  }
});

test('B2: a 29 February birthday counts to 28 February 2027, never negative', async ({ page }) => {
  await page.goto('/contacts');
  const { id } = await api(page, 'POST', '/api/contacts', { principal: 'alice', ab_id: FREUNDE, given: 'F9 Lea', family: 'Schalttag', birthday: '--02-29' });
  try {
    await page.goto('/calendar?view=countdown');
    const tile = page.locator('.cd', { has: page.locator('h3', { hasText: 'F9 Lea Schalttag' }) });
    await expect(tile.locator('.ring b')).toHaveText('137');
    await expect(tile.locator('h3 + span')).toHaveText('Sonntag, 28. Februar 2027');
    for (const b of await page.locator('.cd:not(.over) .ring b').allTextContents()) expect(Number(b)).toBeGreaterThanOrEqual(0);
    // the contact card counts the same days (app.js dateInfo)
    await page.goto(`/contacts#alice/${FREUNDE}/${id}`);
    await expect(page.locator('#ctform [data-date=birthday] .dateinfo')).toHaveText('in 137 Tagen');
  } finally {
    await api(page, 'DELETE', `/api/contacts/alice/${FREUNDE}/${id}`);
  }
});

test('B1: counts drawn by app.js are plurals too ("in 1 Tag" on a contact card)', async ({ page }) => {
  await page.goto(`/contacts#alice/${FREUNDE}/c-oezil@seed`); // birthday 15 October, the frozen today is the 14th
  await expect(page.locator('#ctform [data-date=birthday] .dateinfo')).toHaveText('in 1 Tag · wird 34');
});

test('B3: "Jahr in Zahlen" spark bars carry their share, the title week and count', async ({ page }) => {
  await page.goto('/calendar?view=numbers&date=2026-10-14');
  const bars = await page.locator('.spark i').evaluateAll(l => l.map(i => [i.getAttribute('style'), i.getAttribute('title')]));
  expect(bars.length).toBe(53);
  for (const [style, title] of bars) { expect(style).toMatch(/^--h:\d+%$/); expect(title).toMatch(/^KW \d+: \d+$/); }
  expect(bars.some(([s]) => s === '--h:100%')).toBe(true);
});

test('B8: saving a collection that does not exist answers in German', async ({ page }) => {
  await page.goto('/settings');
  await expect(settingsForm(page, '/settings/calendars/alice/f9-missing/update', { name: 'x' })).rejects.toThrow('"msg":"Nicht gefunden."');
});

test("B8: PractiCal's own error texts reach the browser in its language", async ({ page }) => {
  await page.goto('/calendar');
  const csrf = await page.locator('meta[name=csrf]').getAttribute('content');
  const r = await page.request.post('/api/events', { data: { principal: 'alice', cal_id: '5eed0000-0000-4000-8000-000000000001', summary: '  ', start: '2026-10-16T14:00:00', end: '2026-10-16T15:00:00' }, headers: { 'X-CSRF-Token': csrf, Origin: BASE } });
  expect([r.status(), await r.text()]).toEqual([400, 'Bitte einen Titel angeben.']);
});

test('B9: the stored language wins; "automatic" follows the browser at once', async ({ page }) => {
  await page.goto('/settings');
  await savePrefs(page, { lang: 'en' });
  await page.reload();
  await expect(page.locator('html')).toHaveAttribute('lang', 'en');
  await savePrefs(page); // lang "" = automatic; this browser asks for de-DE
  await page.reload();
  await expect(page.locator('html')).toHaveAttribute('lang', 'de');
});

test('B10 + B11: a vCard 2.1 card and an .ics without PRODID/VERSION/DTSTAMP import and show', async ({ page }) => {
  const dir = path.join(WORK, 'f9-files');
  fs.mkdirSync(dir, { recursive: true });
  const vcf = path.join(dir, 'f9-alt.vcf'), ics = path.join(dir, 'f9-lose.ics');
  fs.writeFileSync(vcf, 'BEGIN:VCARD\r\nVERSION:2.1\r\nUID:f9-otto@e2e\r\nN:Alt;F9 Otto;;;\r\nFN:F9 Otto Alt\r\nTEL;CELL;VOICE:0151 2223334\r\nEMAIL;INTERNET:otto@example.org\r\nEND:VCARD\r\n');
  fs.writeFileSync(ics, 'BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:f9-lose@e2e\r\nDTSTART:20261105T080000Z\r\nDTEND:20261105T090000Z\r\nSUMMARY:F9 Ohne Kopf\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n');
  const made = [];
  try {
    await page.goto('/settings#import-addressbook');
    const fa = page.locator('#import-addressbook .imp-form');
    await fa.locator('.imp-file').setInputFiles(vcf);
    await Promise.all([page.waitForEvent('load'), fa.locator('.imp-go').click()]);
    made.push(`/settings/addressbooks/alice/${page.url().match(/#ab-alice-(.+)$/)[1]}`);
    await page.goto('/contacts?view=table');
    await expect(page.locator('tr', { hasText: 'F9 Otto Alt' }).locator('td[data-c=phone]')).toHaveText('0151 2223334');
    await page.goto('/settings#import-calendar');
    const fc = page.locator('#import-calendar .imp-form');
    await fc.locator('.imp-file').setInputFiles(ics);
    await Promise.all([page.waitForEvent('load'), fc.locator('.imp-go').click()]);
    made.push(`/settings/calendars/alice/${page.url().match(/#cal-alice-(.+)$/)[1]}`);
    await expect(page.locator('.toast')).toBeVisible();
    await expect.poll(async () => (await api(page, 'GET', '/api/search?q=F9%20Ohne%20Kopf')).events.map(e => e.summary)).toContain('F9 Ohne Kopf');
    await page.goto('/calendar?view=day&date=2026-11-05');
    await expect(page.locator('.doc')).toContainText('F9 Ohne Kopf');
  } finally {
    await page.goto('/settings');
    for (const p of made) { await settingsForm(page, p + '/delete'); await settingsForm(page, p + '/purge'); }
  }
});
