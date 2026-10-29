// Sharing in the editor: the visibility slider over the CLASS select, what sharees would see, and the question
// before a write through a share after which its writer sees less (the 422 round trip). erin writes into
// bob's "Team" through group.team (seed: bob → group.team, write); own data only ("F7 …" in November 2026).
import { api, expect, loginAs, settle, test } from '../lib/test.mjs';
import { BASE } from '../lib/env.mjs';
import { addCard, answer, createApi, fillWhen, instances, keyOf, openEdit, openNew, save } from './_editor.mjs';

test.afterEach(async ({ page }) => {
  expect(page.__dialogs, 'no alert() from the app').toEqual([]);
});

const teamCopy = page => page.locator('#ed-cal option[value^="group.team/"]').getAttribute('value');

test('the slider sets CLASS; through a share it tells what sharees see and asks before hiding', async ({ page }) => {
  // the slider sits in the status card, which shows itself only once added or for a CLASS other than public
  // Team's levels are not set (seed): private and confidential events are hidden from its sharees
  await loginAs(page, 'erin');
  await openNew(page, '2026-11-02');
  const cal = await teamCopy(page);
  expect(cal, 'erin has bob\'s Team as a copy').toBeTruthy();
  await fillWhen(page, { title: 'F7 Geteilt', from: '09:00', to: '10:00', cal });
  await expect(page.locator('#ed-status-card')).toBeHidden();
  await addCard(page, 'status');
  const vis = page.locator('#ed-vis'), out = vis.locator('#ed-vis-out'), hint = vis.locator('#ed-vis-hint');
  // untouched: shown as public, nothing set
  await expect(out).toHaveText('öffentlich');
  await expect(hint).toHaveText('Empfänger sehen: Alles');
  expect(await page.locator('#edform select[name="class"]').inputValue()).toBe('');
  await vis.locator('.vis-steps button[data-i="1"]').click(); // a step's symbol moves the slider
  await expect(vis.locator('#ed-vis-range')).toHaveValue('1');
  await expect(vis.locator('.vis-steps button.on')).toHaveAttribute('data-i', '1');
  await expect(out).toHaveText('privat');
  await expect(hint).toHaveText('Empfänger sehen: Gar nicht');
  expect(await page.locator('#edform select[name="class"]').inputValue()).toBe('PRIVATE');
  // "back to editing": nothing saved, the dialog stays with its work
  await page.locator('#ed-save').click();
  await expect(page.locator('.ask')).toContainText('gar nicht mehr');
  await answer(page, false);
  await expect(page.locator('#ed')).toBeVisible();
  await expect(page.locator('#ed-error')).toHaveText('');
  expect(await instances(page, '2026-11-02', 'F7 Geteilt')).toHaveLength(0);
  // "save anyway": written, and erin no longer sees it
  await page.locator('#ed-save').click();
  await answer(page, true);
  await expect(page.locator('#ed')).toBeHidden();
  await settle(page);
  const day = await api(page, 'GET', '/api/events?start=2026-11-02&end=2026-11-03');
  expect(day.filter(e => e.summary === 'F7 Geteilt')).toHaveLength(0);
  expect(day.filter(e => `${e.principal}/${e.cal_id}` === cal), 'hidden from its writer too').toHaveLength(0);
  // it is there for its owner
  await loginAs(page, 'bob');
  await page.goto('/calendar');
  expect(await instances(page, '2026-11-02', 'F7 Geteilt')).toHaveLength(1);
});

test('an own calendar without shares: no hint, no question', async ({ page }) => {
  await openNew(page, '2026-11-03');
  await fillWhen(page, { title: 'F7 Eigen', from: '09:00', to: '10:00', cal: 'alice/5eed0000-0000-4000-8000-000000000001' });
  await addCard(page, 'status');
  await expect(page.locator('#ed-vis-hint')).toBeHidden();
  await page.locator('#ed-vis-range').fill('2');
  await expect(page.locator('#ed-vis-out')).toHaveText('vertraulich');
  await page.locator('#ed-save').click();
  await expect(page.locator('#ed')).toBeHidden();
  await expect(page.locator('.ask')).toHaveCount(0);
  await settle(page);
  const e = (await instances(page, '2026-11-03', 'F7 Eigen'))[0];
  expect((await api(page, 'GET', `/api/events/${e.principal}/${e.cal_id}/${e.object_id}`)).class).toBe('CONFIDENTIAL');
  // a CLASS other than public brings the status card along when the event opens again
  await openEdit(page, keyOf(e), '2026-11-03', { edit: false });
  await expect(page.locator('#ed-status-card')).toBeVisible();
  await expect(page.locator('#ed-vis-out')).toHaveText('vertraulich');
});

test('rule mode: "beliebig" first, the setting off until its chip turns it on; the chip sits in the status card', async ({ page }) => {
  await page.goto('/calendar?rule=Urlaub');
  await page.locator('#ed:not([hidden])').waitFor();
  await expect(page.locator('#ed-status-card'), 'the Urlaub rule shows as free: its status card is there').toBeVisible();
  const range = page.locator('#ed-vis-range');
  await expect(page.locator('#ed-vis-out')).toHaveText('beliebig');
  await expect(page.locator('#ed-vis .vis-steps button[data-i="-1"]')).toBeVisible();
  await expect(range).toBeDisabled();
  const chip = page.locator('.rfoot[data-rfoot="status"] [data-rmode="class"]');
  await expect(chip).toBeVisible();
  await expect(page.locator('.rfoot[data-rfoot="when"] [data-rmode="class"]')).toHaveCount(0);
  await chip.click();
  await page.locator('.rrpop button[data-v="default"]').click();
  await expect(range).toBeEnabled();
  await expect(page.locator('#ed-vis-out')).toHaveText('öffentlich');
  await range.fill('-1');
  await expect(page.locator('#ed-vis-out')).toHaveText('beliebig');
});

test('the shares of a calendar: one line each, the access switch saves, the trash ends one after asking', async ({ page }) => {
  // seed: alice shares "Privat" with dave to read
  await page.goto('/settings#cal-alice-5eed0000-0000-4000-8000-000000000002');
  const row = page.locator('[id="cal-alice-5eed0000-0000-4000-8000-000000000002"] .share-row', { hasText: 'dave' });
  await expect(row).toHaveCount(1);
  const box = await row.boundingBox();
  expect(box.height, 'one line').toBeLessThan(60);
  await expect(row.locator('.seg button[data-v="read"]')).toHaveAttribute('aria-pressed', 'true');
  await row.locator('.seg button[data-v="write"]').click();
  await expect(page.locator('.toast')).toBeVisible();
  expect(await (await page.request.get('/settings')).text(), 'saved: the page from the server says write').toContain('data-v="write" aria-pressed="true"');
  // the trash asks; "no" keeps it, "yes" ends it
  await row.locator('.val.end button').click();
  await answer(page, false);
  await expect(row).toHaveCount(1);
  await row.locator('.val.end button').click();
  await answer(page, true);
  await expect(page.locator('[id="cal-alice-5eed0000-0000-4000-8000-000000000002"] .share-row')).toHaveCount(0);
});

test('unlocking an event without CLASS: the slider of an added status card is there and moves', async ({ page }) => {
  await page.goto('/calendar'); // the API helper takes the page's session
  const key = await createApi(page, { summary: 'F7 Ohne Klasse', start: '2026-11-04T09:00:00', end: '2026-11-04T10:00:00', cal: 'alice/5eed0000-0000-4000-8000-000000000001' });
  await openEdit(page, key, '2026-11-04'); // view mode first, then the lock
  await addCard(page, 'status');
  await expect(page.locator('#ed-vis')).toBeVisible();
  await expect(page.locator('#ed-vis-range')).toBeEnabled();
  await page.locator('#ed-vis .vis-steps button[data-i="2"]').click();
  await expect(page.locator('#ed-vis-out')).toHaveText('vertraulich');
});

test('a CLASS another client wrote is shown as itself, sits on the private step and survives a save', async ({ page }) => {
  await page.goto('/calendar'); // the API helper takes the page's session
  const r = await api(page, 'POST', '/api/events', { principal: 'alice', cal_id: '5eed0000-0000-4000-8000-000000000001', summary: 'F7 Fremd', start: '2026-11-05T09:00:00', end: '2026-11-05T10:00:00', class: 'X-SECRET' });
  const key = `alice/5eed0000-0000-4000-8000-000000000001/${r.id}`;
  await openEdit(page, key, '2026-11-05', { edit: false });
  await expect(page.locator('#ed-status-card'), 'not public: the card shows itself').toBeVisible();
  await page.locator('#ed-lock').click();
  await expect(page.locator('#ed-vis-out')).toHaveText('X-SECRET');
  await expect(page.locator('#ed-vis-range')).toHaveValue('1');
  await page.locator('#edh').fill('F7 Fremd neu');
  await save(page);
  expect((await api(page, 'GET', `/api/events/${key}`)).class).toBe('X-SECRET');
  // the private step it sits on is a pick of its own: PRIVATE replaces the other client's value
  await openEdit(page, key, '2026-11-05');
  await page.locator('#ed-vis .vis-steps button[data-i="1"]').click();
  await expect(page.locator('#ed-vis-out')).toHaveText('privat');
  await save(page);
  expect((await api(page, 'GET', `/api/events/${key}`)).class).toBe('PRIVATE');
});

/** The suggestion list of `field` is on screen where it is drawn: no ancestor clips it (settings.css `.set .group`). */
const onScreen = list => list.evaluate(el => { const r = el.getBoundingClientRect(), hit = document.elementFromPoint(r.left + r.width / 2, r.bottom - 8); return !!hit && el.contains(hit); });

test('sharing: two letters suggest people, the second one picked goes into the field, the share asks whom first', async ({ page }) => {
  // seed: erin "Erin Schreiber" and erik "Erik Gast" both start with "er"; by name: Erik first
  await page.goto('/settings#cal-alice-5eed0000-0000-4000-8000-000000000001');
  const form = page.locator('[id="cal-alice-5eed0000-0000-4000-8000-000000000001"] form.share-add');
  const field = form.locator('input[name=sharee]'), list = form.locator('.combo');
  await field.pressSequentially('er');
  await expect(list.locator('.r')).toHaveCount(2);
  await expect(field).toHaveAttribute('aria-expanded', 'true');
  expect(await onScreen(list), 'not clipped by the group around it').toBe(true);
  await expect(list.locator('.r').nth(1)).toContainText('Erin Schreiber');
  await list.locator('.r').nth(1).click();
  await expect(field).toHaveValue('erin');
  await expect(list).toBeHidden();
  await expect(field).toBeFocused();
  await form.locator('button[type=submit]').click();
  await expect(page.locator('.ask')).toContainText('Erin Schreiber (erin)');
  await answer(page, true);
  await expect(page.locator('[id="cal-alice-5eed0000-0000-4000-8000-000000000001"] .share-row', { hasText: 'erin' })).toHaveCount(1);
});

test('a group\'s new member: typed, suggested without those already in it, picked by keyboard, added', async ({ page }) => {
  await page.goto('/settings#groups');
  const form = page.locator('form.member-add[action="/settings/groups/group.familie/members"]');
  const field = form.locator('input[name=member]'), list = form.locator('.combo');
  await expect(form.locator('select'), 'no list of everyone').toHaveCount(0);
  // the server offers bob (after the 150 ms debounce); the field leaves him out, he is in it already
  const [resp] = await Promise.all([page.waitForResponse(r => r.url().includes('/api/sharee/suggest?q=bo')), field.pressSequentially('bo')]);
  expect((await resp.json()).map(x => x.id)).toContain('bob');
  await settle(page);
  await expect(field).toHaveAttribute('aria-expanded', 'false');
  await expect(list.locator('.r')).toHaveCount(0);
  await field.fill('');
  await field.pressSequentially('er');
  await expect(list.locator('.r')).toHaveCount(2);
  expect(await onScreen(list)).toBe(true);
  await field.press('ArrowDown');
  await field.press('ArrowDown');
  await expect(list.locator('.r[aria-selected=true]')).toContainText('Erin Schreiber');
  await field.press('Enter');
  await expect(field).toHaveValue('erin');
  await Promise.all([page.waitForURL(/#groups$/), form.locator('button[type=submit]').click()]);
  await expect(page.locator('form[action="/settings/groups/group.familie/members/erin/remove"]')).toHaveCount(1);
});

test('a shared calendar keeps its owner\'s reminders only: the reminder card is not offered there', async ({ page }) => {
  await loginAs(page, 'erin');
  await openNew(page, '2026-11-06');
  await fillWhen(page, { title: 'F7 Ohne Erinnerung', from: '09:00', to: '10:00', cal: await teamCopy(page) });
  await page.locator('.addmenu .add').click();
  await expect(page.locator('.addmenu [data-card-add="alarm"]')).toBeDisabled();
  await expect(page.locator('.addmenu [data-card-add="status"]')).toBeEnabled();
  // inert, not only for the mouse: no Tab stop or assistive-tech action below the card's head and hint
  expect(await page.locator('#ed-alarm-card').evaluate(c => [...c.children].filter(x => x.tagName !== 'H2' && !x.classList.contains('al-share')).every(x => x.inert))).toBe(true);
});

test('a conflict after the visibility question keeps the answer: "keep mine" saves without asking again', async ({ page, browser }) => {
  await loginAs(page, 'erin');
  await page.goto('/calendar');
  const cal = await teamCopy(page), [principal, cal_id] = cal.split('/');
  const r = await api(page, 'POST', '/api/events', { principal, cal_id, summary: 'F7 Konflikt', start: '2026-11-07T09:00:00', end: '2026-11-07T10:00:00' });
  const key = `${cal}/${r.id}`;
  await openEdit(page, key, '2026-11-07');
  await addCard(page, 'status');
  await page.locator('#ed-vis .vis-steps button[data-i="1"]').click();
  await page.locator('#ed-save').click();
  await expect(page.locator('.ask')).toContainText('gar nicht mehr');
  // meanwhile bob changes it: the confirmed write meets a newer version
  const bobCtx = await browser.newContext({ baseURL: BASE, locale: 'de-DE' }), bob = await bobCtx.newPage();
  await loginAs(bob, 'bob');
  await bob.goto('/calendar');
  const team = (await api(bob, 'GET', '/api/events?start=2026-11-07&end=2026-11-08')).find(e => e.summary === 'F7 Konflikt');
  const d = await api(bob, 'GET', `/api/events/${team.principal}/${team.cal_id}/${team.object_id}`);
  await api(bob, 'PUT', `/api/events/${team.principal}/${team.cal_id}/${team.object_id}`, { ...d, summary: 'F7 Konflikt (Bob)' });
  await bobCtx.close();
  await answer(page, true); // save anyway
  await expect(page.locator('.ask')).toContainText(/überschreiben|Version|geändert/i); // the conflict question
  await answer(page, true); // keep mine
  await expect(page.locator('#ed')).toBeHidden();
  await expect(page.locator('.ask')).toHaveCount(0);
});

test('an access changed while the page is open reaches the editor without a reload', async ({ page, browser }) => {
  await loginAs(page, 'erin');
  await page.goto('/calendar?view=month&date=2026-11-10');
  const copy = await teamCopy(page);
  await expect(page.locator(`#ed-cal option[value="${copy}"]`)).toBeEnabled();
  const bobCtx = await browser.newContext({ baseURL: BASE, locale: 'de-DE' }), bob = await bobCtx.newPage();
  await loginAs(bob, 'bob');
  const team = `cal-bob-${copy.split('/')[1].replace(/_shared_by_bob$/, '')}`; // the copy is named after bob's calendar
  const flip = async v => {
    await bob.goto(`/settings#${team}`);
    await bob.locator(`[id="${team}"] .share-row`, { hasText: 'group.team' }).locator(`.seg button[data-v="${v}"]`).click();
    await expect(bob.locator('.toast')).toBeVisible();
  };
  try {
    await flip('read');
    // the live change reloads erin's view, whose toolbar lists the copy as read-only; the editor opened after takes it
    const listed = new RegExp(`(^|,)${copy.replace(/[.*+?^${}()|[\]\\/]/g, '\\$&')}(,|$)`);
    await expect(page.locator('.toolbar')).toHaveAttribute('data-ro', listed);
    const day = page.locator('.v-month .day[data-date="2026-11-10"]');
    await day.hover();
    await day.locator('.add').click();
    await page.locator('#ed:not([hidden])').waitFor();
    await expect(page.locator(`#ed-cal option[value="${copy}"]`)).toBeDisabled();
  } finally {
    await flip('write');
    await bobCtx.close();
  }
});
