// Settings: every hash page and the dialogs on them (read-only; an autosave is answered in the browser).
import path from 'node:path';
import fs from 'node:fs';
import { shot } from '../lib/visual.mjs';
import { WORK } from '../lib/env.mjs';
import { fakePrefsSave, hash, keepToasts } from './_set.mjs';
import { edReady, edSnap } from './_editor.mjs';

const CAL = 'cal-alice-5eed0000-0000-4000-8000-000000000001';
const CAL_G = 'cal-group.familie-5eed0000-0000-4000-8000-000000000004';
const AB = 'ab-alice-5eed0000-0000-4000-8000-000000000011';
const AB_G = 'ab-group.familie-5eed0000-0000-4000-8000-000000000012';

shot('settings home @main', async (page, snap) => {
  await page.goto('/settings');
  await snap('set-home');
});

shot('settings vacation + title tester', async (page, snap) => {
  await fakePrefsSave(page);
  await page.goto('/settings#vacation');
  await snap('set-vacation');
  await keepToasts(page); // the "saved" toast of the kind switch stays in every following state
  await page.locator('#vacation .seg button[data-v="calendar"]').click();
  await snap('set-vacation-calendar', { segments: 0 });
  await page.locator('#vacation .seg button[data-v="title"]').click();
  await page.locator('#vac-test .vac-note').waitFor();
  await snap('set-vacation-title-empty', { segments: 0 });
  await page.locator('#vac-re').fill('urlaub|messe');
  await page.locator('#vac-test .vac-th').waitFor();
  await snap('set-vacation-title-match');
  await page.locator('#vac-test [data-show="miss"]').click();
  await snap('set-vacation-title-miss', { segments: 0 });
  await page.locator('#vac-re').fill('(?=x)');
  await page.locator('#vac-test .vac-err').waitFor();
  await snap('set-vacation-title-unsupported', { segments: 0 });
  await page.locator('#vac-re').fill('urlaub(');
  await page.locator('#vac-test .vac-err').waitFor();
  await snap('set-vacation-title-invalid', { segments: 0 });
});

shot('settings alarms + snap edit row', async (page, snap) => {
  await page.goto('/settings#alarms');
  await snap('set-alarms');
  await page.locator('#snaps .snp-pt[data-key="start"][data-m="60"]').click();
  await page.locator('#snaps .snp-edit').waitFor();
  await snap('set-alarms-edit', { segments: 0 });
});

shot('settings search history', async (page, snap) => {
  await page.goto('/settings#search');
  await page.locator('#hist-list [data-id]').first().waitFor();
  await snap('set-search');
  await page.locator('#hist-clear').click();
  await page.locator('.ask').waitFor();
  await snap('set-search-clear-ask', { segments: 0 });
  await page.keyboard.press('Escape');
});

shot('settings regions + pickers', async (page, snap) => {
  await page.goto('/settings#regions-public');
  await snap('set-regions-public');
  await hash(page, '#regions-school');
  await snap('set-regions-school');
  await hash(page, '#pick-tz');
  await page.locator('#pick-list button[data-v]').first().waitFor();
  await snap('set-pick-tz');
  await page.locator('#pick-q').fill('berl');
  await snap('set-pick-tz-filtered', { segments: 0 });
  await hash(page, '#pick-country-public');
  await page.locator('#pick-list button[data-v="DE"]').waitFor();
  await snap('set-pick-country-public');
  await hash(page, '#pick-region-public-DE');
  await page.locator('#pick-list .cell').first().waitFor();
  await snap('set-pick-region-public-de');
  await hash(page, '#pick-country-school');
  await page.locator('#pick-list .cell').first().waitFor();
  await snap('set-pick-country-school');
});

shot('settings collections + collection pages', async (page, snap) => {
  await page.goto('/settings#collections');
  await snap('set-collections');
  for (const [h, n] of [[CAL, 'set-cal-own'], [CAL_G, 'set-cal-group'], [AB, 'set-ab-own'], [AB_G, 'set-ab-group'], ['new-calendar', 'set-new-calendar'], ['new-addressbook', 'set-new-addressbook']]) {
    await hash(page, '#' + h);
    await snap(n);
  }
  // a destructive button asks first (askYesNo); "no" leaves everything as it was
  await hash(page, '#' + CAL);
  await page.locator(`#${CAL.replace(/\./g, '\\.')} button[data-confirm]`).click();
  await page.locator('.ask').waitFor();
  await snap('set-cal-trash-ask', { segments: 0 });
  await page.locator('.ask [data-a="0"]').click();
  await page.locator('.ask').waitFor({ state: 'detached' });
});

shot('settings import pages (file read in the browser)', async (page, snap) => {
  const dir = path.join(WORK, 'f4-files');
  fs.mkdirSync(dir, { recursive: true });
  const ics = path.join(dir, 'Reise.ics'), vcf = path.join(dir, 'Verein.vcf'), empty = path.join(dir, 'leer.ics');
  fs.writeFileSync(ics, 'BEGIN:VCALENDAR\r\nVERSION:2.0\r\nX-WR-CALNAME:Reiseplan\r\nX-WR-CALDESC:Zug und Hotel\r\nX-APPLE-CALENDAR-COLOR:#34C759FF\r\nX-WR-TIMEZONE:Europe/Berlin\r\nBEGIN:VEVENT\r\nUID:r1@e2e\r\nDTSTART:20261101T080000Z\r\nDTEND:20261101T090000Z\r\nSUMMARY:Zug\r\nEND:VEVENT\r\nBEGIN:VTODO\r\nUID:r2@e2e\r\nSUMMARY:Koffer\r\nEND:VTODO\r\nEND:VCALENDAR\r\n');
  fs.writeFileSync(vcf, 'BEGIN:VCARD\r\nVERSION:4.0\r\nUID:v1@e2e\r\nFN:Vera Verein\r\nEND:VCARD\r\nBEGIN:VCARD\r\nVERSION:4.0\r\nUID:v2@e2e\r\nKIND:group\r\nFN:Vorstand\r\nEND:VCARD\r\n');
  fs.writeFileSync(empty, 'BEGIN:VCALENDAR\r\nVERSION:2.0\r\nEND:VCALENDAR\r\n');
  await page.goto('/settings#import-calendar');
  await snap('set-import-calendar');
  await page.locator('#import-calendar .imp-file').setInputFiles(empty);
  await page.locator('#import-calendar .imp-info .danger').waitFor();
  await snap('set-import-calendar-empty', { segments: 0 });
  await page.locator('#import-calendar .imp-file').setInputFiles(ics);
  await page.locator('#import-calendar .imp-fields').waitFor();
  await page.locator('#import-calendar input[name=name]').blur();
  await snap('set-import-calendar-picked');
  await hash(page, '#import-addressbook');
  await snap('set-import-addressbook');
  await page.locator('#import-addressbook .imp-file').setInputFiles(vcf);
  await page.locator('#import-addressbook .imp-fields').waitFor();
  await page.locator('#import-addressbook input[name=name]').blur();
  await snap('set-import-addressbook-picked');
});

shot('settings categories', async (page, snap) => {
  await page.goto('/settings#categories');
  await page.locator('#cat-list .cell').first().waitFor();
  await snap('set-categories-event');
  await page.locator('#cat-scope button[data-scope="task"]').click();
  await page.locator('#cat-list a', { hasText: 'Haushalt' }).waitFor();
  await snap('set-categories-task');
  // a category's rule opens over the page (GET /editor); last: afterwards the page holds the (hidden) dialog
  await page.locator('#cat-list a', { hasText: 'Haushalt' }).click();
  await edReady(page);
  await edSnap(page, snap, 'set-categories-rule-task');
  await page.keyboard.press('Escape');
  await page.locator('#cat-scope button[data-scope="event"]').click();
  await page.locator('#cat-list a', { hasText: 'Arbeit' }).click();
  await page.locator('#ed-host[data-kind="event"] #ed:not([hidden])').waitFor();
  await edReady(page);
  await edSnap(page, snap, 'set-categories-rule-event');
});

shot('settings groups + typed confirmation', async (page, snap) => {
  await page.goto('/settings#groups');
  await snap('set-groups');
  await page.locator('#groups details.danger-zone summary').first().click();
  await page.locator('.ask .ask-type').waitFor();
  await snap('set-groups-delete-ask', { segments: 0 });
  await page.locator('.ask .ask-type input').fill('Familie');
  await snap('set-groups-delete-typed', { segments: 0 });
  await page.keyboard.press('Escape');
});

shot('settings account', async (page, snap) => {
  await page.goto('/settings#account');
  await snap('set-account');
  await keepToasts(page);
  await page.locator('#account button[data-copy="#dav-cal"]').click();
  await page.locator('.toast').waitFor();
  await snap('set-account-copied-toast', { segments: 0 });
});

shot('settings trash + purge dialog', async (page, snap) => {
  await page.goto('/settings#trash');
  await snap('set-trash');
  await page.locator('#trash details.danger-zone summary').first().click();
  await page.locator('.ask .ask-type').waitFor();
  await snap('set-trash-purge-ask', { segments: 0 });
  await page.keyboard.press('Escape');
});
