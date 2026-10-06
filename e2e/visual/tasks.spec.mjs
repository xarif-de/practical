// Tasks page (inventory §2.7): the six views, lookback, sort, column picker, the task-mode editor
// (docked ≥1100 in groups, dialog otherwise) and its questions. Read-only (lib/visual.mjs aborts writes).
import { expect } from '../lib/test.mjs';
import { isPhone, shot } from '../lib/visual.mjs';

const dialog = { segments: 0 };

async function view(page, v, back = 'all') {
  await page.goto(`/tasks?view=${v}${back ? '&back=' + back : ''}`);
  await expect(page.locator(`#tk-views [data-tkview="${v}"][aria-pressed=true]`)).toBeVisible();
  await expect(page.locator('#tkmain > *').first()).toBeVisible();
}
const card = (page, title) => page.locator('#tkmain [data-key]').filter({ hasText: title }).first();

shot('tasks groups @main', async (page, snap) => {
  await view(page, 'groups');
  await snap('tk-groups-all');
});

shot('tasks groups: default lookback, sort by priority descending, filter popover', async (page, snap) => {
  await view(page, 'groups', null); // no cookie: one month back
  await expect(page.locator('#tk-back')).toHaveValue('month');
  await snap('tk-groups-month');
  await page.selectOption('#tk-sort', 'prio');
  await page.click('#tk-dir');
  await expect(page.locator('#tk-dir use')).toHaveAttribute('href', /#i-sort-desc$/); // the sprite file's URL + the symbol
  await snap('tk-groups-prio-desc');
  await page.click('.topbar .filter:has(#calfilter) > button');
  await expect(page.locator('#calfilter')).toBeVisible();
  await snap('tk-filter-pop', dialog);
});

shot('tasks table', async (page, snap) => {
  await view(page, 'table');
  await snap('tk-table');
  await page.click('#tk-colpick > button');
  await expect(page.locator('#tk-colpop')).toBeVisible();
  await snap('tk-table-colpick', dialog);
  await page.click('.toolbar h1');
  await page.click('.sheet th[data-sk=title]');
  await expect(page.locator('.sheet th[data-sk=title] .a.asc')).toBeVisible();
  await snap('tk-table-sort-title');
});

shot('tasks table: a card cell opens one card of the editor', async (page, snap) => {
  await view(page, 'table');
  await card(page, 'Reifen wechseln').locator('td.cardcell[data-card=cats]').click();
  await expect(page.locator('#ed.solo')).toBeVisible();
  await snap('tk-table-solo-cats', dialog);
});

shot('tasks board, timeline, history', async (page, snap) => {
  await view(page, 'board');
  await snap('tk-board');
  await view(page, 'timeline');
  await snap('tk-timeline');
  await view(page, 'history');
  await snap('tk-history');
  await page.click('.tkhm .dy[data-d="2026-10-12"]');
  await expect(page.locator('.tkhm .dy.on[data-d="2026-10-12"]')).toBeVisible();
  await snap('tk-history-day');
});

shot('tasks history: every heat level', async (page, snap) => {
  // the fixture completed two tasks, on two days (level 1): the page's own data gets more per-day counts, which the
  // heat map alone reads (app.js vHistory, level = min(3, count)) — the levels 2 and 3 had no state (e2e/theme-cover.mjs)
  await page.route(u => u.pathname === '/tasks', async route => {
    const r = await route.fetch();
    const html = (await r.text()).replace(/"per_day":\{(\}?)/, (_, end) => `"per_day":{"2026-10-05":2,"2026-10-06":3,"2026-10-07":5${end || ','}`);
    if (!html.includes('"2026-10-07":5')) throw new Error('tasks-data: no per_day to extend');
    await route.fulfill({ response: r, body: html });
  });
  await view(page, 'history');
  await expect(page.locator('.tkhm .dy[data-n="3"]')).toHaveCount(2);
  await snap('tk-history-levels', dialog);
});

shot('tasks deck and its keys', async (page, snap) => {
  await view(page, 'deck');
  await expect(page.locator('.tkdeck')).toBeFocused();
  await snap('tk-deck');
  await page.keyboard.press('j');
  await page.keyboard.press('j');
  await expect(page.locator('.tkcnt')).toContainText('3');
  await snap('tk-deck-jj');
  await page.keyboard.press('k');
  await expect(page.locator('.tkcnt')).toContainText('2');
  // space asks before finishing the top card; Escape answers "no"
  await page.keyboard.press(' ');
  await expect(page.locator('.ask')).toBeVisible();
  await snap('tk-deck-space-ask', dialog);
  await page.keyboard.press('Escape');
  await expect(page.locator('.ask')).toHaveCount(0);
  await page.locator('.tkdeck').focus();
  await page.keyboard.press('e');
  await expect(page.locator('#ed:not([hidden]) #edform.mode-view')).toBeVisible();
  await snap('tk-deck-e-editor', dialog);
});

shot('tasks: tick asks first', async (page, snap) => {
  await view(page, 'groups');
  await card(page, 'Reifen wechseln').locator('.tcb').click();
  await expect(page.locator('.ask')).toBeVisible();
  await snap('tk-tick-ask', dialog);
  await page.keyboard.press('Escape');
  await expect(page.locator('.ask')).toHaveCount(0);
});

shot('tasks editor: view, edit, popovers, questions', async (page, snap, testInfo) => {
  const o = isPhone(testInfo) ? dialog : {};
  await view(page, 'groups');
  await card(page, 'Angebot schreiben').click();
  await expect(page.locator('#edh')).toHaveValue('Angebot schreiben');
  await expect(page.locator('#edform.mode-view')).toBeVisible();
  await snap('tk-ed-view', o);
  await page.click('#ed-lock');
  await expect(page.locator('#edform.mode-edit')).toBeVisible();
  await snap('tk-ed-edit', o);
  await page.click('#ed-gen-tiles [data-sel=priority]');
  await expect(page.locator('.rrpop')).toBeVisible();
  await snap('tk-ed-prio-pop', dialog);
  await page.click('#edh'); // a click outside closes it
  await expect(page.locator('.rrpop')).toHaveCount(0);
  await page.click('#edform .addmenu > button');
  await expect(page.locator('#edform .addmenu .pop')).toBeVisible();
  await snap('tk-ed-addmenu', dialog);
  await page.click('#edh');
  await page.fill('#edh', 'Angebot schreiben (geändert)');
  await page.click('#ed-close');
  await expect(page.locator('#ed-confirm')).toBeVisible();
  await snap('tk-ed-unsaved', dialog);
  await page.click('#ed-back');
  await page.click('#ed-delete');
  await expect(page.locator('.ask')).toBeVisible();
  await snap('tk-ed-delete-ask', dialog);
  await page.keyboard.press('Escape');
  await expect(page.locator('.ask')).toHaveCount(0);
});

shot('tasks: new task form', async (page, snap, testInfo) => {
  // groups ≥1100 docks the new-task form (tk-groups-all); elsewhere + opens it as a dialog
  await view(page, isPhone(testInfo) ? 'groups' : 'board');
  await page.click('#fab');
  await expect(page.locator('#ed:not([hidden]) #edform.new')).toBeVisible();
  await page.fill('#edh', 'Neue Aufgabe');
  await snap('tk-new-dialog', dialog);
});
