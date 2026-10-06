// Tasks page, end to end (inventory §2.7, §3 tasks rows). Every write targets tasks this file creates
// (prefix "F3 "); the column choice is restored. Outcomes are read back after a reload and via the API.
import { test, expect, api } from '../lib/test.mjs';

const CAL = '5eed0000-0000-4000-8000-000000000003'; // alice's "Aufgaben" (fixture)
const waitFor = (page, method, re) => page.waitForResponse(r => r.request().method() === method && re.test(new URL(r.url()).pathname));
const item = (page, key) => page.locator(`#tkmain [data-key="${key}"]`).first();

async function view(page, v, back = 'all', hash = '') {
  await page.goto(`/tasks?view=${v}${back ? '&back=' + back : ''}${hash}`);
  await expect(page.locator(`#tk-views [data-tkview="${v}"][aria-pressed=true]`)).toBeVisible();
}
/** A task through the API (setup for tests that exercise something else); returns its key. */
async function seedTask(page, summary, extra = {}) {
  if (!page.url().includes('/tasks')) await view(page, 'groups');
  const { id } = await api(page, 'POST', '/api/tasks', { principal: 'alice', cal_id: CAL, summary, ...extra });
  return `alice/${CAL}/${id}`;
}
const get = (page, key) => api(page, 'GET', `/api/tasks/${key}`);
async function yes(page) {
  await expect(page.locator('.ask')).toBeVisible();
  await page.click('.ask [data-a="1"]');
}

test('create a task in the docked editor: title, due date, calendar, priority', async ({ page }) => {
  await view(page, 'groups');
  await expect(page.locator('#ed.docked #edform.new')).toBeVisible();
  await page.fill('#edh', 'F3 Neue Aufgabe');
  await page.fill('#edform input[name=due_date]', '2026-10-20');
  await page.selectOption('#edform select[name=calendar]', `alice/${CAL}`);
  await page.click('#ed-gen-tiles [data-sel=priority]');
  await page.click('.rrpop button[data-v="1"]');
  await expect(page.locator('#ed-gen-tiles [data-sel=priority]')).not.toHaveClass(/\boff\b/);
  const saved = waitFor(page, 'POST', /^\/api\/tasks$/);
  await page.click('#ed-save');
  expect((await saved).ok()).toBe(true);
  const card = page.locator('#tkmain .tkcard').filter({ hasText: 'F3 Neue Aufgabe' });
  await expect(card).toBeVisible(); // after the reload
  await expect(page.locator('#edh')).toHaveValue('F3 Neue Aufgabe'); // the docked pane shows what was saved
  const t = await get(page, await card.getAttribute('data-key'));
  expect(t.summary).toBe('F3 Neue Aufgabe');
  expect(String(t.due).slice(0, 10)).toBe('2026-10-20');
  expect(t.due_all_day).toBe(true);
  expect(t.priority).toBe(1);
  expect(t.cal_id).toBe(CAL);
});

test('create a task from the + button (dialog) in the board view', async ({ page }) => {
  await view(page, 'board');
  await page.click('#fab');
  await expect(page.locator('#ed:not([hidden]) #edform.new')).toBeVisible();
  await page.fill('#edh', 'F3 Board neu');
  await page.selectOption('#edform select[name=calendar]', `alice/${CAL}`);
  const saved = waitFor(page, 'POST', /^\/api\/tasks$/);
  await page.click('#ed-save');
  expect((await saved).ok()).toBe(true);
  const card = page.locator('.tkcol[data-st="NEEDS-ACTION"] .tkkc').filter({ hasText: 'F3 Board neu' });
  await expect(card).toBeVisible(); // after the reload; the editor writes STATUS:NEEDS-ACTION
  expect((await get(page, await card.getAttribute('data-key'))).status).toBe('NEEDS-ACTION');
});

test('edit a task in the editor', async ({ page }) => {
  const key = await seedTask(page, 'F3 Edit vorher', { due: '2026-10-22' });
  await view(page, 'groups');
  await item(page, key).click();
  await expect(page.locator('#edh')).toHaveValue('F3 Edit vorher');
  await page.click('#ed-lock');
  await page.fill('#edh', 'F3 Edit nachher');
  await page.fill('#edform input[name=due_date]', '2026-10-23');
  const saved = waitFor(page, 'PUT', /^\/api\/tasks\//);
  await page.click('#ed-save');
  expect((await saved).ok()).toBe(true);
  await expect(page.locator('#tkmain .tkcard').filter({ hasText: 'F3 Edit nachher' })).toBeVisible();
  const t = await get(page, key);
  expect([t.summary, String(t.due).slice(0, 10)]).toEqual(['F3 Edit nachher', '2026-10-23']);
});

test('delete a task (asks first)', async ({ page }) => {
  const key = await seedTask(page, 'F3 Weg damit');
  await view(page, 'groups');
  await item(page, key).click();
  await page.click('#ed-lock');
  const gone = waitFor(page, 'DELETE', /^\/api\/tasks\//);
  await page.click('#ed-delete');
  await yes(page);
  expect((await gone).ok()).toBe(true);
  await expect(page.locator(`#tkmain [data-key="${key}"]`)).toHaveCount(0);
  expect((await page.request.get(`/api/tasks/${key}`)).status()).toBe(404);
});

test('tick: completing asks (no keeps it open), reopening does not ask', async ({ page }) => {
  const key = await seedTask(page, 'F3 Abhaken', { due: '2026-10-16' });
  await view(page, 'groups');
  await item(page, key).locator('.tcb').click();
  await expect(page.locator('.ask')).toBeVisible();
  await page.click('.ask [data-a="0"]');
  expect((await get(page, key)).status).not.toBe('COMPLETED');
  const put = waitFor(page, 'PUT', /^\/api\/tasks\//);
  await item(page, key).locator('.tcb').click();
  await yes(page);
  expect((await put).ok()).toBe(true);
  await expect(item(page, key)).toHaveClass(/\boff\b/); // reloaded: in the finished group
  const done = await get(page, key);
  expect(done.status).toBe('COMPLETED');
  expect(done.completed_at).toBeTruthy();
  const put2 = waitFor(page, 'PUT', /^\/api\/tasks\//);
  await item(page, key).locator('.tcb').click();
  expect((await put2).ok()).toBe(true);
  await expect(page.locator('.ask')).toHaveCount(0);
  await expect(item(page, key)).not.toHaveClass(/\boff\b/);
  const open = await get(page, key);
  expect(open.status).not.toBe('COMPLETED');
  expect(open.completed_at).toBeFalsy();
});

test('every view shows the task (groups, table, board, timeline, deck, history)', async ({ page }) => {
  // without a STATUS: the board files it as open like every other view
  const key = await seedTask(page, 'F3 Überall', { start: '2026-10-10', start_all_day: true, due: '2026-11-05' });
  expect((await get(page, key)).status).toBe('');
  for (const v of ['groups', 'table', 'board', 'timeline']) {
    await view(page, v);
    await expect(item(page, key)).toBeVisible();
  }
  await view(page, 'deck', 'all', '#' + encodeURIComponent(key)); // a link picks it: the top card
  await expect(page.locator('.tkdeck .cd').last()).toHaveAttribute('data-key', key);
  await api(page, 'PUT', `/api/tasks/${key}`, { principal: 'alice', cal_id: CAL, summary: 'F3 Überall', completed: true });
  await view(page, 'history');
  await expect(page.locator(`.tkdl .ri[data-key="${key}"]`)).toBeVisible();
  await page.click('.tkhm .dy[data-d="2026-10-14"]'); // completed "now" (DEV_NOW)
  await expect(page.locator(`.tkdl .ri[data-key="${key}"]`)).toBeVisible();
  await expect(page.locator('.tkdl .ri').filter({ hasText: 'Rechnung bezahlt' })).toHaveCount(0); // finished on 10-12
});

test('a task without STATUS is open on the board and in the table; a save does not add one', async ({ page }) => {
  const key = await seedTask(page, 'F3 Ohne Status', { due: '2026-11-06' });
  await view(page, 'board');
  await expect(page.locator(`.tkcol[data-st="NEEDS-ACTION"] .tkkc[data-key="${key}"]`)).toBeVisible();
  await view(page, 'table');
  const row = page.locator(`.sheet tbody tr[data-key="${key}"]`);
  await expect(row.locator('select[data-f=status]')).toHaveValue('NEEDS-ACTION');
  // an edit of another cell keeps the stored task without STATUS, the row still says open
  const title = row.locator('[data-f=summary]');
  await title.click();
  await page.keyboard.press('End');
  await page.keyboard.type(' 2');
  const put = waitFor(page, 'PUT', /^\/api\/tasks\//);
  await page.keyboard.press('Enter');
  expect((await put).ok()).toBe(true);
  await expect.poll(async () => (await get(page, key)).summary).toBe('F3 Ohne Status 2');
  expect((await get(page, key)).status).toBe('');
  await expect(row.locator('select[data-f=status]')).toHaveValue('NEEDS-ACTION');
  await expect(row.locator('select[data-f=status] option[value=""]')).toHaveCount(0);
});

test('a new task names itself "Neue Aufgabe" in the colour picker until it has a title', async ({ page }) => {
  await view(page, 'groups');
  await expect(page.locator('#ed.docked #edform.new')).toBeVisible();
  await page.click('#ed-color-btn');
  await expect(page.locator('.cpk .cpk-h b')).toHaveText('Neue Aufgabe');
});

test('board: drag a card into another column sets its status', async ({ page }) => {
  const key = await seedTask(page, 'F3 Ziehen', { status: 'NEEDS-ACTION' });
  await view(page, 'board');
  await expect(page.locator(`.tkcol[data-st="NEEDS-ACTION"] .tkkc[data-key="${key}"]`)).toBeVisible();
  const put = waitFor(page, 'PUT', /^\/api\/tasks\//);
  await page.dragAndDrop(`.tkkc[data-key="${key}"]`, '.tkcol[data-st="IN-PROCESS"]');
  expect((await put).ok()).toBe(true);
  await expect(page.locator(`.tkcol[data-st="IN-PROCESS"] .tkkc[data-key="${key}"]`)).toBeVisible();
  expect((await get(page, key)).status).toBe('IN-PROCESS');
  const put2 = waitFor(page, 'PUT', /^\/api\/tasks\//);
  await page.dragAndDrop(`.tkkc[data-key="${key}"]`, '.tkcol[data-st="COMPLETED"]');
  expect((await put2).ok()).toBe(true);
  await expect(page.locator(`.tkcol[data-st="COMPLETED"] .tkkc[data-key="${key}"]`)).toBeVisible();
  const t = await get(page, key);
  expect([t.status, t.percent]).toEqual(['COMPLETED', 100]);
});

test('table: cells save on their own (title, priority, due)', async ({ page }) => {
  const key = await seedTask(page, 'F3 Zelle', { due: '2026-10-25' });
  await view(page, 'table');
  const row = page.locator(`.sheet tbody tr[data-key="${key}"]`);
  const title = row.locator('[contenteditable][data-f=summary]');
  await title.click();
  await title.fill('F3 Zelle neu');
  let put = waitFor(page, 'PUT', /^\/api\/tasks\//);
  await title.press('Enter');
  expect((await put).ok()).toBe(true);
  put = waitFor(page, 'PUT', /^\/api\/tasks\//);
  await row.locator('select[data-f=priority]').selectOption('9');
  expect((await put).ok()).toBe(true);
  put = waitFor(page, 'PUT', /^\/api\/tasks\//);
  await row.locator('input[data-f=due]').fill('2026-10-26');
  expect((await put).ok()).toBe(true);
  await expect.poll(async () => { const t = await get(page, key); return [t.summary, t.priority, String(t.due).slice(0, 10)]; }).toEqual(['F3 Zelle neu', 9, '2026-10-26']);
  await page.reload();
  await expect(row.locator('[data-f=summary]')).toHaveText('F3 Zelle neu');
});

test('table: a card cell opens that card alone and saves the task', async ({ page }) => {
  const key = await seedTask(page, 'F3 Solo');
  await view(page, 'table');
  await page.locator(`.sheet tbody tr[data-key="${key}"] td.cardcell[data-card=cats]`).click();
  await expect(page.locator('#ed.solo')).toBeVisible();
  await page.fill('#ed-ct-in', 'F3-Tag');
  await page.press('#ed-ct-in', 'Enter');
  const put = waitFor(page, 'PUT', /^\/api\/tasks\//);
  await page.click('#ed-save');
  expect((await put).ok()).toBe(true);
  await expect.poll(async () => (await get(page, key)).categories).toEqual(['F3-Tag']);
  await expect(page.locator(`.sheet tbody tr[data-key="${key}"] td[data-card=cats]`)).toContainText('F3-Tag');
});

test('sorting: table headers and the sort box (kept in localStorage)', async ({ page }) => {
  await view(page, 'table');
  await page.click('.sheet th[data-sk=title]');
  // the open ones sort by the field; finished and cancelled follow (by completion)
  const titles = () => page.locator('.sheet tbody tr').evaluateAll(rs => rs.filter(r => !r.classList.contains('off') && r.querySelector('select[data-f=status]').value !== 'CANCELLED').map(r => r.querySelector('[data-f=summary]').textContent));
  const open = await titles();
  expect(open).toEqual([...open].sort((a, b) => a.toLowerCase() < b.toLowerCase() ? -1 : a.toLowerCase() > b.toLowerCase() ? 1 : 0));
  await page.click('.sheet th[data-sk=title]');
  await expect(page.locator('.sheet th[data-sk=title] .a.desc')).toBeVisible();
  expect(await titles()).toEqual([...open].reverse());
  expect(await page.evaluate(() => [localStorage.tkSort, localStorage.tkDesc])).toEqual(['title', '1']);
  await view(page, 'groups');
  await expect(page.locator('#tk-sort')).toHaveValue('title');
  await page.selectOption('#tk-sort', 'prio');
  await page.reload();
  await expect(page.locator('#tk-sort')).toHaveValue('prio');
});

test('lookback: the choice is a cookie and limits the finished tasks', async ({ page }) => {
  await view(page, 'groups');
  const old = page.locator('#tkmain .tkcard').filter({ hasText: 'Fahrrad repariert' }); // finished 2026-09-01
  const recent = page.locator('#tkmain .tkcard').filter({ hasText: 'Rechnung bezahlt' }); // finished 2026-10-12
  await expect(old).toBeVisible();
  await page.selectOption('#tk-back', 'week');
  await expect(page).toHaveURL(/back=week/);
  await expect(recent).toBeVisible();
  await expect(old).toHaveCount(0);
  expect((await page.context().cookies()).find(c => c.name === 'tk_back').value).toBe('week');
  await page.goto('/tasks'); // no parameter: the cookie decides
  await expect(page.locator('#tk-back')).toHaveValue('week');
  await expect(old).toHaveCount(0);
});

test('task table columns persist (POST /settings/task-columns) and are restored', async ({ page }) => {
  await view(page, 'table');
  await page.click('#tk-colpick > button');
  const loc = page.locator('#tk-colpop input[data-col=location]');
  const before = await loc.isChecked();
  const label = await loc.getAttribute('data-label');
  const head = page.locator('.sheet thead th', { hasText: new RegExp(`^\\s*${label}\\s*$`) });
  const post = waitFor(page, 'POST', /^\/settings\/task-columns$/);
  await loc.click();
  expect((await post).ok()).toBe(true);
  await page.reload();
  await expect(head).toHaveCount(before ? 0 : 1);
  await page.click('#tk-colpick > button');
  const back = waitFor(page, 'POST', /^\/settings\/task-columns$/);
  await page.locator('#tk-colpop input[data-col=location]').click();
  expect((await back).ok()).toBe(true);
  await page.reload();
  await expect(head).toHaveCount(before ? 1 : 0);
  await expect(page.locator('#tk-colpop input[data-col=location]')).toBeChecked({ checked: before });
});

test('finished tasks are paged in from /api/tasks/done while scrolling', async ({ page }) => {
  await view(page, 'groups');
  const first = await api(page, 'GET', '/api/tasks/done?back=all&offset=0&limit=1');
  expect(first.items).toHaveLength(1);
  expect(first.total).toBeGreaterThanOrEqual(2);
  const second = await api(page, 'GET', '/api/tasks/done?back=all&offset=1&limit=1');
  expect(second.items[0].summary).not.toBe(first.items[0].summary);
  // the page ships at most 120 finished tasks; the fixture has fewer, so the page is cut to one per
  // chunk here (response rewritten in the browser only) to drive the client's paging for real
  await page.route(u => u.pathname === '/tasks', async route => {
    const r = await route.fetch();
    let html = await r.text();
    html = html.replace('data-chunk="120"', 'data-chunk="1"').replace(/(<script type="application\/json" id="tasks-data">)(.*?)(<\/script>)/s, (_, a, j, b) => {
      const d = JSON.parse(j); d.done = d.done.slice(0, 1); return a + JSON.stringify(d).replace(/</g, '\\u003c') + b;
    });
    await route.fulfill({ response: r, body: html });
  });
  const pages = [];
  page.on('request', q => { if (new URL(q.url()).pathname === '/api/tasks/done') pages.push(q.url()); });
  await view(page, 'table');
  const all = await api(page, 'GET', '/api/tasks/done?back=all&offset=0&limit=500');
  await expect(page.locator('#tk-sent')).toHaveCount(0, { timeout: 15000 }); // everything loaded
  const rows = await page.locator('.sheet tbody [data-f=summary]').allTextContents();
  for (const t of all.items) expect(rows).toContain(t.summary);
  expect(pages.length).toBeGreaterThanOrEqual(all.total - 1);
  expect(pages[0]).toContain('offset=1');
});

test('deep link /tasks#key picks the task and shows it in the docked editor', async ({ page }) => {
  const key = await seedTask(page, 'F3 Verlinkt', { due: '2026-12-01' });
  await view(page, 'groups', 'all', '#' + encodeURIComponent(key));
  await expect(item(page, key)).toHaveClass(/\bsel\b/);
  await expect(page.locator('#ed.docked #edh')).toHaveValue('F3 Verlinkt');
});

test('deck keys: e opens, j/k move, space finishes (asks)', async ({ page }) => {
  const key = await seedTask(page, 'F3 Deck', { due: '2026-10-15' });
  await view(page, 'deck', 'all', '#' + encodeURIComponent(key));
  const top = page.locator('.tkdeck .cd').last();
  await expect(top).toHaveAttribute('data-key', key);
  const n = async () => +(await page.locator('.tkcnt').textContent()).trim().split(/\s+/)[0];
  const at = await n();
  await page.locator('.tkdeck').focus();
  await page.keyboard.press('e');
  await expect(page.locator('#ed:not([hidden]) #edh')).toHaveValue('F3 Deck');
  await page.keyboard.press('Escape');
  await expect(page.locator('#ed')).toBeHidden();
  await page.locator('.tkdeck').focus();
  await page.keyboard.press('j');
  await expect.poll(n).toBe(at + 1);
  await page.keyboard.press('k');
  await expect.poll(n).toBe(at);
  await expect(page.locator('.tkdeck .cd').last()).toHaveAttribute('data-key', key);
  const put = waitFor(page, 'PUT', /^\/api\/tasks\//);
  await page.keyboard.press(' ');
  await expect(page.locator('.ask')).toBeVisible();
  await page.keyboard.press('Enter'); // "yes" has the focus
  expect((await put).ok()).toBe(true);
  await expect.poll(async () => (await get(page, key)).status).toBe('COMPLETED');
});

test('calendar filter hides a calendar\'s tasks (shared with the calendar page)', async ({ page }) => {
  await view(page, 'groups');
  const mine = page.locator('#tkmain .tkcard').filter({ hasText: 'Keller aufräumen' }); // Aufgaben
  const work = page.locator('#tkmain .tkcard').filter({ hasText: 'Angebot schreiben' }); // Arbeit
  await expect(mine).toBeVisible();
  await page.click('.topbar .filter:has(#calfilter) > button');
  await page.locator(`#calfilter input[data-cal="alice/${CAL}"]`).uncheck();
  await expect(page).toHaveURL(/cals=/);
  await expect(mine).toHaveCount(0);
  await expect(work).toBeVisible();
  expect(JSON.parse(await page.evaluate(() => localStorage.calHidden))).toEqual([`alice/${CAL}`]);
  await page.click('.topbar .filter:has(#calfilter) > button');
  await page.click('#calfilter [data-filter=all]');
  await expect(mine).toBeVisible();
});
