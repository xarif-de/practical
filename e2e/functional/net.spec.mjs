// Network chaos on the editors and the navigation (route interception, offline emulation): offline, slow answers,
// 401/409/400/500/503, a session that really ended (login server), a broken live stream. Each test fails on the code
// before practical.req / practical.writer (audit3-robustness R5, R6, R10, R13, R17, R18; perf3 #1, #10).
// Own data only ("F9 …" in November 2026).
import { api, expect, settle, test } from '../lib/test.mjs';
import { LOGIN_BASE } from '../lib/env.mjs';
import { CAL, addCard, createApi, detail, fillWhen, instances, openEdit, openNew, save } from './_editor.mjs';

const tr = (page, k) => page.evaluate(k => window.tr(k), k);
/** Holds every `method` request to `pattern` until release(); `first` resolves when one arrived. */
async function gate(page, pattern, method) {
  let open, arrived; const opened = new Promise(r => { open = r; }), first = new Promise(r => { arrived = r; }); let n = 0;
  await page.route(pattern, async route => { if (route.request().method() !== method) return route.fallback(); n++; arrived(); await opened; await route.fallback(); });
  return { first, release: () => open(), count: () => n };
}
const errors = page => { const out = []; page.on('pageerror', e => out.push(String(e))); return out; };
// every EventSource of the page in window.__es (an init script); streamOpen: the live stream's is open
const trackStreams = () => { const E = window.EventSource; window.__es = []; window.EventSource = class extends E { constructor(...a) { super(...a); window.__es.push(this); } }; };
const streamOpen = page => page.waitForFunction(() => window.__es.filter(e => e.url.endsWith('/api/changes')).at(-1)?.readyState === 1);

test.afterEach(async ({ page }) => {
  expect(page.__dialogs.filter(d => d), 'no alert() from the app').toEqual([]);
});

test('save: a double click and Enter are one write; Save waits disabled, the dialog stays until the answer', async ({ page }) => {
  await openNew(page, '2026-11-09');
  await fillWhen(page, { title: 'F9 Doppelt', from: '10:00', to: '11:00', cal: CAL.privat });
  const g = await gate(page, '**/api/events', 'POST');
  await page.locator('#ed-save').dblclick();
  await g.first;
  await page.locator('#edh').press('Enter');
  await expect(page.locator('#ed-save')).toBeDisabled();
  await expect(page.locator('#edform')).toHaveAttribute('data-state', 'saving');
  await expect(page.locator('#edform')).toHaveAttribute('aria-busy', 'true');
  await page.keyboard.press('Escape'); // closing waits for the answer
  await page.locator('#ed-close').click();
  await expect(page.locator('#ed')).toBeVisible();
  g.release();
  await expect(page.locator('#ed')).toBeHidden();
  await settle(page);
  expect(g.count()).toBe(1);
  expect(await instances(page, '2026-11-09', 'F9 Doppelt')).toHaveLength(1);
  await expect(page.locator('#edform')).not.toHaveAttribute('data-state', /./);
});

test('save offline: the reason is told, the work stays; online again it saves', async ({ page, context }) => {
  await openNew(page, '2026-11-10');
  await fillWhen(page, { title: 'F9 Offline', from: '10:00', to: '11:00', cal: CAL.privat });
  await context.setOffline(true);
  await page.locator('#ed-save').click();
  await expect(page.locator('#ed-error')).toContainText(await tr(page, 'js-req-offline'));
  await expect(page.locator('#edform')).toHaveAttribute('data-state', 'error');
  await expect(page.locator('.toast.net')).toHaveText(await tr(page, 'js-req-offline-bar'));
  await expect(page.locator('#edh')).toHaveValue('F9 Offline');
  await expect(page.locator('#ed-save')).toBeEnabled();
  await context.setOffline(false);
  await expect(page.locator('.toast.net')).toHaveCount(0);
  await save(page);
  expect(await instances(page, '2026-11-10', 'F9 Offline')).toHaveLength(1);
});

test('save: a busy server (503) is asked again; a 500 and a 400 are told in the dialog, the work stays', async ({ page }) => {
  await openNew(page, '2026-11-11');
  await fillWhen(page, { title: 'F9 Busy', from: '10:00', to: '11:00', cal: CAL.privat });
  let n = 0;
  await page.route('**/api/events', r => r.request().method() === 'POST' && n++ === 0 ? r.fulfill({ status: 503, contentType: 'text/plain', body: 'busy' }) : r.fallback());
  await save(page);
  expect(n).toBe(2);
  expect(await instances(page, '2026-11-11', 'F9 Busy')).toHaveLength(1);
  await page.unroute('**/api/events');
  await openNew(page, '2026-11-11');
  await fillWhen(page, { title: 'F9 Kaputt', from: '12:00', to: '13:00', cal: CAL.privat });
  await page.route('**/api/events', r => r.request().method() === 'POST' ? r.fulfill({ status: 500, contentType: 'text/plain', body: 'boom' }) : r.fallback());
  await page.locator('#ed-save').click();
  await expect(page.locator('#ed-error')).toContainText(await tr(page, 'js-req-server'));
  await page.unroute('**/api/events');
  await page.route('**/api/events', r => r.request().method() === 'POST' ? r.fulfill({ status: 400, contentType: 'text/plain', body: 'Das Datum gibt es nicht.' }) : r.fallback());
  await page.locator('#ed-save').click();
  await expect(page.locator('#ed-error')).toHaveText('Das Datum gibt es nicht.'); // the server's own (localized) words
  await expect(page.locator('#edh')).toHaveValue('F9 Kaputt');
  expect(await instances(page, '2026-11-11', 'F9 Kaputt')).toHaveLength(0);
});

test('save after a change elsewhere (409): Escape keeps editing, "mine" overwrites, "theirs" loads the other version', async ({ page }) => {
  await page.goto('/calendar?view=month&date=2026-11-12');
  const key = await createApi(page, { summary: 'F9 Konflikt', start: '2026-11-12T09:00:00', end: '2026-11-12T10:00:00', cal: CAL.privat });
  const [principal, cal_id] = CAL.privat.split('/');
  // another client's save: based on the version stored now (a write without one is refused with 428)
  const theirs = async summary => api(page, 'PUT', '/api/events/' + key, { principal, cal_id, summary, start: '2026-11-12T09:00:00', end: '2026-11-12T10:00:00', all_day: false, categories: [], alarms: [], color: '', location: '', etag: (await detail(page, key)).etag });
  await openEdit(page, key, '2026-11-12');
  await fillWhen(page, { title: 'F9 Meins' });
  await theirs('F9 Deren');
  await page.locator('#ed-save').click();
  await expect(page.locator('.ask')).toBeVisible();
  await expect(page.locator('#edform')).toHaveAttribute('data-state', 'conflict');
  await page.keyboard.press('Escape');
  await expect(page.locator('.ask')).toHaveCount(0);
  await expect(page.locator('#ed-error')).not.toBeEmpty();
  await expect(page.locator('#edh')).toHaveValue('F9 Meins');
  // keep mine: saved over theirs with the server's current ETag
  await page.locator('#ed-save').click();
  await page.locator('.ask [data-a="1"]').click();
  await expect(page.locator('#ed')).toBeHidden();
  expect((await detail(page, key)).summary).toBe('F9 Meins');
  // load theirs: the dialog shows the other version, nothing written
  await openEdit(page, key, '2026-11-12');
  await fillWhen(page, { title: 'F9 Nochmal meins' });
  await theirs('F9 Wieder deren');
  await page.locator('#ed-save').click();
  await page.locator('.ask [data-a="0"]').click();
  await expect(page.locator('#edh')).toHaveValue('F9 Wieder deren');
  await expect(page.locator('#edform')).toHaveClass(/mode-view/);
  await expect(page.locator('#ed-error')).toBeEmpty();
  expect((await detail(page, key)).summary).toBe('F9 Wieder deren');
});

test('a session that ended on save (401): to the login and back to the same view, the typed work restored', async ({ page, context }) => {
  await openNew(page, '2026-11-13');
  await fillWhen(page, { title: 'F9 Sitzung', from: '10:00', to: '11:00', cal: CAL.privat });
  await context.clearCookies(); // the server forgot the session (a restart): the save answers 401
  const toLogin = page.waitForRequest(r => r.isNavigationRequest() && r.url().includes('/login?redirect_uri=%2Fcalendar%3Fview%3Dmonth%26date%3D2026-11-13'));
  await page.locator('#ed-save').click();
  await toLogin; // development mode signs alice in again and sends her back
  await expect(page.locator('#ed:not([hidden]) #edh')).toHaveValue('F9 Sitzung');
  expect(new URL(page.url()).search).toBe('?view=month&date=2026-11-13');
  await expect(page.locator('.toast')).toContainText(await tr(page, 'js-req-restored'));
  await save(page);
  expect(await instances(page, '2026-11-13', 'F9 Sitzung')).toHaveLength(1);
});

test('a real session loss (login server): navigation and save go to the login page, never into the view; the work comes back', async ({ page, context }) => {
  await page.addInitScript(trackStreams);
  const login = async () => { await page.locator('form.pwform input[name=username]').fill('alice'); await page.locator('form.pwform input[name=password]').fill('devpass'); await page.locator('form.pwform button[type=submit]').click(); };
  const url = LOGIN_BASE + '/calendar?view=planner&date=2026-11-16'; // the year view turns with ‹ ›
  await page.goto(url); await login();
  await expect(page).toHaveURL(url);
  // the server restarted (every session gone): the next page turn. The live stream is open first: one that connects
  // without the session ends in the login by itself (the test below); with it open, the pointer resting on the year
  // prefetches the neighbours while the session lives — refused here, so the turn asks the server itself
  await page.route(u => u.href.startsWith(LOGIN_BASE), r => r.request().headers().purpose === 'prefetch' ? r.abort() : r.fallback());
  await streamOpen(page);
  await context.clearCookies();
  await page.locator('[data-nav=next]').click();
  await page.waitForURL(/\/login\?redirect_uri=%2Fcalendar%3Fview%3Dplanner%26date%3D2026-11-16$/);
  await expect(page.locator('#viewroot')).toHaveCount(0);
  await login();
  await expect(page).toHaveURL(url);
  // and a save
  const url2 = LOGIN_BASE + '/calendar?view=month&date=2026-11-16';
  await page.goto(url2);
  const day = page.locator('.v-month .day[data-date="2026-11-16"]');
  await day.hover(); await day.locator('.add').click();
  await page.locator('#edh').fill('F9 Echt');
  await streamOpen(page);
  await context.clearCookies();
  await page.locator('#ed-save').click();
  await page.waitForURL(/\/login\?redirect_uri=/);
  await login();
  await expect(page).toHaveURL(url2);
  await expect(page.locator('#ed:not([hidden]) #edh')).toHaveValue('F9 Echt');
  await page.locator('#ed-save').click();
  await expect(page.locator('#ed')).toBeHidden();
  const evs = await (await page.request.get(LOGIN_BASE + '/api/events?start=2026-11-16&end=2026-11-17')).json();
  expect(evs.filter(e => e.summary === 'F9 Echt')).toHaveLength(1);
});

test('navigation: a 500 and offline are told, the view stays; a slow fragment shows the progress line', async ({ page, context }) => {
  const errs = errors(page), frag = u => u.searchParams.get('fragment') === '1'; // the page turn's fetch and its prefetch alike
  await page.goto('/calendar?view=planner&date=2026-10-14'); // the year view turns with ‹ ›
  await page.route(frag, r => r.fulfill({ status: 500, contentType: 'text/plain', body: 'boom' }));
  await page.locator('[data-nav=next]').click();
  await expect(page.locator('.toast.error')).toContainText(await tr(page, 'js-req-server'));
  await expect(page.locator('#viewroot')).toHaveAttribute('data-date', '2026-10-14');
  await page.unroute(frag);
  const g = await gate(page, frag, 'GET');
  await page.locator('[data-nav=next]').click();
  await g.first;
  await expect(page.locator('html')).toHaveAttribute('data-busy', ''); // after 300 ms
  g.release();
  await expect(page.locator('#viewroot')).toHaveAttribute('data-date', '2027-10-14');
  await expect(page.locator('html')).not.toHaveAttribute('data-busy', /.*/);
  await page.unroute(frag);
  await settle(page);
  await context.setOffline(true);
  await page.locator('[data-nav=prev]').click(); // not ›: the pointer resting on it prefetched the next year already (a fresh prefetch is used offline too)
  await expect(page.locator('.toast.error:not(.net)')).toContainText(await tr(page, 'js-req-offline'));
  await expect(page.locator('#viewroot')).toHaveAttribute('data-date', '2027-10-14');
  await context.setOffline(false);
  expect(errs).toEqual([]);
});

test('the live stream: after a break it reconnects as soon as the browser is online and shows what changed meanwhile', async ({ page }) => {
  await page.addInitScript(() => { const E = window.EventSource; window.__es = []; window.EventSource = class extends E { constructor(...a) { super(...a); window.__es.push(this); } }; });
  await page.goto('/calendar?view=day&date=2026-11-18');
  const live = () => window.__es.filter(e => e.url.endsWith('/api/changes')).at(-1);
  await page.waitForFunction(`(${live})()?.readyState === 1`);
  await page.evaluate(`(${live})().dispatchEvent(new Event('error'))`); // the app closes it; its retry would wait 5–10 s
  await createApi(page, { summary: 'F9 Lücke', start: '2026-11-18T09:00:00', end: '2026-11-18T10:00:00', cal: CAL.privat });
  await page.evaluate(() => dispatchEvent(new Event('online')));
  await expect(page.locator('#viewroot').getByText('F9 Lücke').first()).toBeVisible();
});

test('failures that were unseen or native: a refused clipboard, a task tick offline, a contact that cannot load', async ({ page, context }) => {
  const errs = errors(page);
  await context.clearPermissions();
  await page.goto('/calendar?view=day&date=2026-10-14');
  await page.locator('.doc [data-copytext]').first().click();
  await expect(page.locator('.toast.error')).toContainText(await tr(page, 'js-req-copy'));
  await page.goto('/tasks');
  const tick = page.locator('#tkmain .tkcard [data-tog]').first();
  await context.setOffline(true);
  await tick.click();
  await page.locator('.ask [data-a="1"]').click();
  await expect(page.locator('.toast.error:not(.net)')).toContainText(await tr(page, 'js-req-offline'));
  await context.setOffline(false);
  await page.goto('/contacts');
  await page.route('**/api/contacts/**', r => r.request().method() === 'GET' ? r.fulfill({ status: 500, contentType: 'text/plain', body: 'boom' }) : r.fallback());
  await page.locator('.list li[data-key]').first().click();
  await expect(page.locator('.toast.error')).toContainText(await tr(page, 'js-req-server'));
  expect(errs).toEqual([]);
});

test('contact editor: one save per double click, a 409 asks, the leave-page guard while dirty', async ({ page }) => {
  await page.goto('/contacts');
  await page.fill('#ctform input[name=given]', 'F9 Doppel');
  await page.fill('#ctform input[name=family]', 'Klick');
  const g = await gate(page, '**/api/contacts', 'POST');
  await page.locator('#ct-save').dblclick();
  await g.first;
  await expect(page.locator('#ct-save')).toBeDisabled();
  const back = page.waitForURL(/#alice\//);
  g.release();
  await back;
  await expect(page.locator('#ctform input[name=full_name]')).toHaveValue('F9 Doppel Klick');
  expect(g.count()).toBe(1);
  // a change elsewhere, then a save here: keep mine
  const key = decodeURIComponent(new URL(page.url()).hash.slice(1));
  const c = await api(page, 'GET', '/api/contacts/' + key);
  await api(page, 'PUT', '/api/contacts/' + key, { ...c, org: 'F9 Deren GmbH' });
  await page.locator('#ct-lock').click();
  await page.fill('#ctform input[name=given]', 'F9 Meins');
  await page.locator('#ct-save').click();
  const reloaded = page.waitForEvent('framenavigated');
  await page.locator('.ask [data-a="1"]').click();
  await reloaded;
  await expect(page.locator('#ctform input[name=given]')).toHaveValue('F9 Meins');
  const now = await api(page, 'GET', '/api/contacts/' + key);
  expect([now.given, now.org]).toEqual(['F9 Meins', '']); // mine saved over theirs: the whole contact as this editor held it
  // unsaved work: leaving the page asks (the browser's own question); the fixture answers "stay"
  await page.locator('#ct-lock').click();
  await page.fill('#ctform input[name=given]', 'F9 Ungespeichert');
  const dlg = page.waitForEvent('dialog');
  await page.close({ runBeforeUnload: true });
  expect((await dlg).type()).toBe('beforeunload');
});

test('the category field: a 3,000-character name never widens the page', async ({ page }) => {
  await openNew(page, '2026-11-19');
  if (!(await page.locator('#ed-ct-in').isVisible())) { await page.locator('#edform .addmenu .add').click(); await page.locator('[data-card-add=cats]').click(); }
  await page.locator('#ed-ct-in').fill('x'.repeat(3000));
  await expect(page.locator('.rrpop .ct-hit.new')).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(await page.evaluate(() => innerWidth));
  // the window gets narrow while the list is open (the chaos monkey, seed 3: 940 px > 390)
  await page.locator('#ed-ct-in').fill('Kategorie mit einem recht langen Namen');
  await page.setViewportSize({ width: 390, height: 844 });
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
});

test('work kept over a new login stays open in a tasks view that does not dock the editor, also through a resize', async ({ page, context }) => {
  await page.goto('/tasks?view=table');
  await page.click('#fab');
  await page.locator('#edh').fill('F9 Behalten');
  await context.clearCookies(); // the save answers 401
  const toLogin = page.waitForRequest(r => r.isNavigationRequest() && r.url().includes('/login?redirect_uri=%2Ftasks%3Fview%3Dtable'));
  await page.locator('#ed-save').click();
  await toLogin; // development mode signs alice in again and sends her back
  await expect(page.locator('#ed:not([hidden]) #edh')).toHaveValue('F9 Behalten');
  // the page lays its editor out again on a resize (tasks.js fit): the restored work is what it holds
  const size = page.viewportSize();
  await page.setViewportSize({ width: size.width - 200, height: size.height });
  await page.setViewportSize(size);
  await expect(page.locator('#ed:not([hidden]) #edh')).toHaveValue('F9 Behalten');
  await page.locator('#ed-save').click();
  await expect(page.locator('#ed')).toBeHidden();
  await expect(page.locator('.sheet tbody [data-f=summary]', { hasText: 'F9 Behalten' })).toHaveCount(1);
});

test('a rule being edited when the session ends comes back after the login and saves', async ({ page, context }) => {
  await page.goto('/calendar?rule=F9-Kat&back=%2Fsettings%23categories');
  await page.locator('#ed.rulemode:not([hidden])').waitFor();
  await addCard(page, 'where');
  await page.locator('#ed-locin').fill('F9 Ort, Leipzig');
  await page.locator('#edh').focus();
  await context.clearCookies();
  const toLogin = page.waitForRequest(r => r.isNavigationRequest() && r.url().includes('/login?redirect_uri=%2Fcalendar%3Frule%3DF9-Kat'));
  await page.locator('#ed-save').click();
  await toLogin;
  await expect(page.locator('#ed.rulemode:not([hidden]) #ed-locin')).toHaveValue('F9 Ort, Leipzig');
  await expect(page.locator('.toast')).toContainText(await tr(page, 'js-req-restored'));
  await expect(page.locator('#ed-delete')).toBeDisabled(); // still no stored rule
  await page.locator('#ed-save').click();
  await page.waitForURL(/\/settings#categories/); // its way back
  expect((await api(page, 'GET', '/api/category-rules?scope=event'))['F9-Kat'].where.location).toBe('F9 Ort, Leipzig');
});

test('the live stream tells an ended session (EventSource hides the 401): to the login and back, the typed work restored', async ({ page, context }) => {
  await page.addInitScript(trackStreams);
  const login = async () => { await page.locator('form.pwform input[name=username]').fill('alice'); await page.locator('form.pwform input[name=password]').fill('devpass'); await page.locator('form.pwform button[type=submit]').click(); };
  const url = LOGIN_BASE + '/calendar?view=month&date=2026-11-19';
  await page.goto(url); await login();
  await expect(page).toHaveURL(url);
  await streamOpen(page);
  const day = page.locator('.v-month .day[data-date="2026-11-19"]');
  await day.hover(); await day.locator('.add').click();
  await page.locator('#edh').fill('F9 Strom');
  await settle(page); // what the typing asks of the server (suggestions) is answered: only the stream may notice the end
  await context.clearCookies(); // the session is gone
  await page.evaluate(() => window.__es.filter(e => e.url.endsWith('/api/changes')).at(-1).dispatchEvent(new Event('error'))); // the stream breaks …
  await page.evaluate(() => dispatchEvent(new Event('online'))); // … and is asked for again at once
  await page.waitForURL(/\/login\?redirect_uri=%2Fcalendar%3Fview%3Dmonth%26date%3D2026-11-19$/);
  await login();
  await expect(page).toHaveURL(url);
  await expect(page.locator('#ed:not([hidden]) #edh')).toHaveValue('F9 Strom');
});
