// Calendar page: navigation (views, more menu, URL, today/prev/next, day picks, orbit picker, rail,
// wheel, endless week stream and day ledger), keyboard, filter + holiday switches, opening events
// from every view, the FAB / add buttons, and live updates from another user (SSE).
import { test, expect, settle, api } from '../lib/test.mjs';
import fs from 'node:fs';
import { BASE } from '../lib/env.mjs';

const D = '2026-10-14';
const FAM = 'group.familie/5eed0000-0000-4000-8000-000000000004';
const cal = (view, date = D) => `/calendar?view=${view}&date=${date}`;
const qp = (page, k) => new URL(page.url()).searchParams.get(k);
const pageFilter = page => page.locator('header .filter:not(.user):not(.shelp)');
const chip = (page, text) => page.locator('.v-month .ev[data-key]', { hasText: text });

async function open(page, url) {
  // requests the previous document had in flight die with it without a requestfailed event: they
  // would hold settle() for its full timeout (see report: lib/test.mjs should clear on navigation)
  page.__inflight.clear();
  await page.goto(url);
  await expect(page.locator('#viewroot')).toBeVisible();
  await settle(page);
}
async function expectEditor(page, title) {
  await expect(page.locator('#ed')).toBeVisible();
  await expect(page.locator('#edh')).toHaveValue(title);
  await expect(page.locator('#edform')).toHaveClass(/\bmode-view\b/);
  await page.keyboard.press('Escape');
  await expect(page.locator('#ed')).toBeHidden();
}

test.afterEach(async ({ page }) => {
  expect(page.__dialogs, 'no alert() during the test').toEqual([]);
});

test('view seg switches day/week/month/year, browser back returns', async ({ page }) => {
  await open(page, cal('month'));
  for (const [view, sel] of [['day', '.v-day'], ['week', '.v-week'], ['planner', '.planner'], ['month', '.v-month']]) {
    await page.locator(`.toolbar .seg [data-view="${view}"]`).click();
    await expect(page.locator(sel)).toBeVisible();
    await expect(page.locator(`.toolbar .seg [data-view="${view}"]`)).toHaveAttribute('aria-pressed', 'true');
    expect(qp(page, 'view')).toBe(view);
    expect(qp(page, 'date')).toBe(D);
  }
  await page.goBack();
  await expect(page.locator('.planner')).toBeVisible();
  expect(qp(page, 'view')).toBe('planner');
  // a reload keeps what the URL says
  await page.reload();
  await expect(page.locator('.planner')).toBeVisible();
});

test('more menu opens each extra view; the menu button names it', async ({ page }) => {
  await open(page, cal('month'));
  for (const [view, sel] of [['weekends', '.toolbar'], ['vacation', '.toolbar'], ['countdown', '.toolbar'], ['numbers', '.toolbar']]) {
    await page.locator('.toolbar .more [data-more]').click();
    await expect(page.locator('.toolbar .more')).toHaveClass(/\bopen\b/);
    await page.locator(`.toolbar .more .pop a[data-view="${view}"]`).click();
    await expect.poll(() => qp(page, 'view')).toBe(view);
    await expect(page.locator('.toolbar .more [data-more]')).toHaveAttribute('aria-pressed', 'true');
    await expect(page.locator(sel)).toBeVisible();
  }
  // a click elsewhere closes the menu
  await page.locator('.toolbar .more [data-more]').click();
  await expect(page.locator('.toolbar .more')).toHaveClass(/\bopen\b/);
  await page.mouse.click(5, 300); // outside the menu
  await expect(page.locator('.toolbar .more')).not.toHaveClass(/\bopen\b/);
});

test('URL picks the view; an unknown view falls back to month', async ({ page }) => {
  await open(page, cal('countdown'));
  await expect(page.locator('.toolbar .more [data-more]')).toHaveAttribute('aria-pressed', 'true');
  await open(page, cal('nonsense'));
  await expect(page.locator('.v-month')).toBeVisible();
  await open(page, '/calendar?view=month&date=2026-12-24');
  await expect(page.locator('#title')).toHaveText(/Dezember 2026/);
});

test('the year planner turns by the wheel, the extra views by prev/next; today in all', async ({ page }) => {
  await open(page, cal('planner'));
  await expect(page.locator('#title')).toHaveText(/2026/);
  await expect(page.locator('.toolbar [data-nav="next"]')).toHaveCount(0); // the wheel turns the year (calendar.js yearWheel)
  const bb = await page.locator('.planner').boundingBox();
  await page.mouse.move(bb.x + bb.width / 2, bb.y + bb.height / 2);
  await page.mouse.wheel(0, 120);
  await expect(page.locator('#title')).toHaveText(/2027/);
  expect(qp(page, 'date')).toBe('2027-10-14');
  for (const y of [2026, 2025]) { await page.waitForTimeout(400); await page.mouse.wheel(0, -120); await expect(page.locator('#title')).toHaveText(new RegExp(y)); } // a rest between: two touches
  await page.locator('.toolbar [data-nav="today"]').click();
  await expect(page.locator('#title')).toHaveText(/2026/);
  expect(qp(page, 'date')).toBe(D);
  for (const v of ['weekends', 'vacation', 'countdown', 'numbers']) {
    await open(page, cal(v));
    // these views name no year in the title: the fragment request carries the date
    let frag = page.waitForResponse(r => r.url().includes('fragment=1') && r.url().includes('date=2027-10-14'));
    await page.locator('.toolbar [data-nav="next"]').click();
    expect((await frag).ok()).toBe(true);
    await expect.poll(() => qp(page, 'date')).toBe('2027-10-14');
    frag = page.waitForResponse(r => r.url().includes('fragment=1') && r.url().includes('date=2025-10-14'));
    await page.locator('.toolbar [data-nav="prev"]').click();
    await page.locator('.toolbar [data-nav="prev"]').click();
    expect((await frag).ok()).toBe(true);
    await page.locator('.toolbar [data-nav="today"]').click();
    await expect.poll(() => qp(page, 'date')).toBe(D);
    await expect(page.locator('.toolbar .more [data-more]')).toHaveAttribute('aria-pressed', 'true');
  }
});

// a short window: the planner is taller than .main (the page's scroller), so the wheel scrolls to its lower days instead
// of turning the year (calendar.js yearWheel's pass; the year turns by the title's picker there)
test('year planner in a short window: the wheel scrolls it, the year stays', async ({ page }) => {
  await page.setViewportSize({ width: 800, height: 560 });
  await open(page, cal('planner'));
  expect(await page.evaluate(() => { const m = document.querySelector('.main'); return m.scrollHeight > m.clientHeight; })).toBe(true);
  const bb = await page.locator('.planner').boundingBox();
  await page.mouse.move(bb.x + bb.width / 2, bb.y + 60); // its top rows: the window ends above its middle
  await page.mouse.wheel(0, 300);
  await expect.poll(() => page.evaluate(() => document.querySelector('.main').scrollTop)).toBeGreaterThan(0);
  await expect(page.locator('#title')).toHaveText(/2026/);
  expect(qp(page, 'date')).toBe(D);
});

test('picking a day: month cell, planner cell, ledger row, "+N more", today button', async ({ page }) => {
  await open(page, cal('month'));
  const day = page.locator('.v-month .day[data-date="2026-10-16"]');
  const bb = await day.boundingBox();
  await day.click({ position: { x: bb.width / 2, y: bb.height - 6 } });
  await expect(page.locator('.v-month .day[data-date="2026-10-16"]')).toHaveClass(/\bsel\b/);
  expect(qp(page, 'date')).toBe('2026-10-16');
  // "+N weitere" no longer leads to the day view: it unfolds the tile (next test)
  await page.locator('.v-month .day[data-date="2026-10-19"] .dmore').dispatchEvent('click');
  await expect(page.locator('.v-month .day[data-date="2026-10-19"]')).toHaveClass(/\bopen\b/);
  expect(qp(page, 'view')).toBe('month');
  await open(page, cal('day', '2026-10-19'));
  await expect(page.locator('.doc[data-date="2026-10-19"]')).toBeVisible();
  await expect(page.locator('.doc[data-date="2026-10-19"]')).toContainText('Termin 5');
  // a ledger row picks its day, the ledger stays
  await page.locator('.lrow[data-date="2026-10-15"]').click();
  await expect(page.locator('.doc[data-date="2026-10-15"]')).toBeVisible();
  await expect(page.locator('.lrow.sel[data-date="2026-10-15"]')).toHaveCount(1);
  await expect(page.locator('.doc')).toContainText('Spätschicht');
  // today
  await page.locator('.toolbar [data-nav="today"]').click();
  await expect(page.locator('.doc[data-date="2026-10-14"]')).toBeVisible();
  // planner
  await open(page, cal('planner'));
  await page.locator('.planner .c[data-date="2026-03-03"]').click();
  await expect(page.locator('.planner .c.sel[data-date="2026-03-03"]')).toHaveCount(1);
  expect(qp(page, 'date')).toBe('2026-03-03');
  expect(qp(page, 'view')).toBe('planner');
  // week list: the picked day's bar runs down its whole row, not only beside the date label (chrome.css `.dayblock.sel`)
  await open(page, cal('week'));
  const block = page.locator('#viewroot .stream .dayblock[data-date="2026-10-15"]');
  await block.locator('.dl').click();
  await expect(block).toHaveClass(/\bsel\b/);
  expect(await block.evaluate(e => getComputedStyle(e).boxShadow)).toMatch(/inset$/);
  expect(await block.locator('.dl').evaluate(e => getComputedStyle(e).boxShadow)).toBe('none');
});

test('a month tile with more than fits unfolds in place: down where there is room, else up with the bars kept', async ({ page }) => {
  await open(page, cal('month'));
  const t = page.locator('.v-month .day[data-date="2026-10-19"]'), next = page.locator('.v-month .day[data-date="2026-10-26"]');
  // as many chips as the tile's height holds; when not all fit, as many as leave room for the "+N" line below them;
  // nothing past the tile's edge
  const fitOk = () => page.evaluate(() => [...document.querySelectorAll('.v-month .day')].every(d => { const r = d.getBoundingClientRect();
    return [...d.children].every(c => !c.getClientRects().length || getComputedStyle(c).position === 'absolute' || c.getBoundingClientRect().bottom <= r.bottom + 0.5); }));
  expect(await fitOk()).toBe(true);
  const vp = page.viewportSize(), shown = () => t.locator(':scope > .ev:visible').count(), n0 = await shown();
  await page.setViewportSize({ width: vp.width, height: vp.height + 400 });
  await expect.poll(shown).toBeGreaterThan(n0);
  expect(await fitOk()).toBe(true);
  await page.setViewportSize(vp);
  await expect.poll(shown).toBe(n0);
  const cell = await t.boundingBox(), below = await next.boundingBox();
  await expect(t.locator('.ev', { hasText: 'Termin 5' })).toBeHidden();
  await t.hover({ position: { x: 30, y: 8 } });
  await expect(t).toHaveClass(/\bopen\b/);
  await expect(t.locator('.ev', { hasText: 'Termin 5' })).toBeVisible();
  await expect(t.locator('.dmore')).toHaveAttribute('aria-expanded', 'true');
  const o = await t.boundingBox();
  expect(Math.abs(o.y - cell.y)).toBeLessThan(0.5); // room below: it only grows downwards
  expect(o.height).toBeGreaterThan(cell.height);
  expect(await next.boundingBox()).toEqual(below); // over its neighbours: the grid keeps its rows
  await page.mouse.move(2, 2);
  await expect(t).not.toHaveClass(/\bopen\b/);
  await expect(t.locator('.ev', { hasText: 'Termin 5' })).toBeHidden();

  // the bottom row (Dec 28 - Jan 3) with a bar across it: no room below, so up; the singles run in time order from the
  // top, the bar keeps its place (the row's band stays unbroken)
  const [principal, cal_id] = 'alice/5eed0000-0000-4000-8000-000000000002'.split('/');
  const mk = (summary, start, end, all_day = false) => api(page, 'POST', '/api/events', { principal, cal_id, summary, start, end, all_day, rrule: null, categories: [], color: '', alarms: [], location: '' });
  await mk('U-Bar', '2026-12-28T00:00:00', '2027-01-01T00:00:00', true);
  // a bar in the second lane stays in it on the days after the first lane's bar ended (the empty lane keeps a chip's height)
  await mk('L-Eins', '2026-12-07T00:00:00', '2026-12-09T00:00:00', true);
  await mk('L-Zwei', '2026-12-08T00:00:00', '2026-12-11T00:00:00', true); // starts while L-Eins runs: the second lane
  const times = ['08:00', '09:00', '10:00', '11:00', '12:00', '13:00', '14:00'];
  for (const [i, h] of times.entries()) await mk(`U${i + 1}`, `2026-12-30T${h}:00`, `2026-12-30T${h.slice(0, 2)}:30:00`);
  await open(page, '/calendar?view=month&date=2026-12-14');
  const y8 = (await page.locator('.v-month .day[data-date="2026-12-08"] .ev.span[title="L-Zwei"]').boundingBox()).y;
  for (const d of ['2026-12-09', '2026-12-10']) expect(Math.abs((await page.locator(`.v-month .day[data-date="${d}"] .ev.span[title="L-Zwei"]`).boundingBox()).y - y8), d).toBeLessThan(0.5);
  const u = page.locator('.v-month .day[data-date="2026-12-30"]');
  const c0 = await u.boundingBox(), bar0 = await u.locator('.ev.span').boundingBox();
  await u.hover({ position: { x: 30, y: 8 } });
  await expect(u).toHaveClass(/\bopen\b/);
  const c1 = await u.boundingBox(), bar1 = await u.locator('.ev.span').boundingBox();
  expect(c1.y).toBeLessThan(c0.y - 1);
  expect(c1.y + c1.height).toBeLessThanOrEqual(c0.y + c0.height + 0.5);
  expect(Math.abs(bar1.y - bar0.y), 'the bar stays where it was').toBeLessThan(0.5);
  const seen = await u.locator('.ev').evaluateAll(l => l.filter(e => e.getClientRects().length).sort((a, b) => a.getBoundingClientRect().top - b.getBoundingClientRect().top).map(e => e.classList.contains('span') ? '|' : e.textContent.split(' ')[0]));
  expect(seen.filter(x => x !== '|')).toEqual(['U1', 'U2', 'U3', 'U4', 'U5', 'U6', 'U7']);
  expect(seen.indexOf('|'), JSON.stringify(seen)).toBeGreaterThan(0); // some above the bar
  // the keyboard unfolds it too (focus on the "+N"), and leaving closes it
  await page.mouse.move(2, 2);
  await expect(u).not.toHaveClass(/\bopen\b/);
  await u.locator('.dmore').focus();
  await expect(u).toHaveClass(/\bopen\b/);
  await page.locator('.toolbar [data-nav="today"]').focus();
  await expect(u).not.toHaveClass(/\bopen\b/);
  // the phone grid shows dots, not chips: there the "+N" picks the day (its agenda lists them all) and unfolds nothing
  await page.setViewportSize({ width: 390, height: 800 });
  await open(page, cal('month'));
  const p = page.locator('.v-month .day[data-date="2026-10-19"]');
  await p.locator('.dmore').click();
  await expect(p).toHaveClass(/\bsel\b/);
  await expect(p).not.toHaveClass(/\bopen\b/);
  await expect(page.locator('.agenda')).toContainText('Termin 5');
});

test('orbit picker: years, months, day; prev/next/reset; close by button, Esc and backdrop', async ({ page }) => {
  await open(page, cal('month'));
  const pk = page.locator('#picker');
  await page.locator('#title').click();
  await expect(pk).toBeVisible();
  await expect(page.locator('#pk-title')).toHaveText('Oktober 2026');
  await expect(page.locator('#pk-grid .day.today[data-date="2026-10-14"]')).toHaveCount(1);
  // ±7 years shifts the ring; reset goes back to today
  await page.locator('#picker .ybtn.next').click();
  await expect(page.locator('#picker .ynode[data-year="2041"]')).toHaveCount(1);
  await page.locator('#picker .ybtn.prev').click();
  await page.locator('#picker .ybtn.prev').click();
  await expect(page.locator('#picker .ynode[data-year="2011"]')).toHaveCount(1);
  await page.locator('#picker .ynode[data-year="2018"]').click();
  await expect(page.locator('#pk-title')).toHaveText('Oktober 2018');
  await page.locator('#picker .ybtn.reset').click();
  await expect(page.locator('#pk-title')).toHaveText('Oktober 2026');
  // the density request answers for the chosen year
  const dens = page.waitForResponse(r => r.url().includes('/api/density?year=2027'));
  await page.locator('#picker .ynode[data-year="2027"]').click();
  expect((await dens).ok()).toBe(true);
  await page.locator('#picker .mnode[data-month="3"]').click();
  await expect(page.locator('#pk-title')).toHaveText('März 2027');
  await page.locator('#pk-grid .day[data-date="2027-03-15"]').click();
  await expect(pk).toBeHidden();
  await expect(page.locator('#title')).toHaveText(/März 2027/);
  expect(qp(page, 'date')).toBe('2027-03-15');
  // close: button, Escape, backdrop
  await page.locator('#title').click();
  await page.locator('#pk-close').click();
  await expect(pk).toBeHidden();
  await page.locator('#title').click();
  await page.keyboard.press('Escape');
  await expect(pk).toBeHidden();
  await page.locator('#title').click();
  await pk.click({ position: { x: 5, y: 5 } });
  await expect(pk).toBeHidden();
  // the picker keeps the view: in the week view a picked day stays in the week view
  await open(page, cal('week'));
  await page.locator('#title').click();
  await page.locator('#pk-grid .day[data-date="2026-10-28"]').click();
  await expect.poll(() => qp(page, 'date')).toBe('2026-10-28');
  expect(qp(page, 'view')).toBe('week');
});

test('keyboard: "/" and Ctrl+K focus the search', async ({ page }) => {
  await open(page, cal('month'));
  await page.locator('#title').focus();
  await page.keyboard.press('/');
  await expect(page.locator('#q')).toBeFocused();
  await page.locator('#q').blur();
  await page.keyboard.press('Control+k');
  await expect(page.locator('#q')).toBeFocused();
  await page.locator('#q').blur();
  await page.keyboard.press('Meta+k');
  await expect(page.locator('#q')).toBeFocused();
});

test('month wheel turns exactly one month per gesture', async ({ page }) => {
  await open(page, cal('month'));
  const m = page.locator('.v-month .month');
  const bb = await m.boundingBox();
  await page.mouse.move(bb.x + bb.width / 2, bb.y + bb.height / 2);
  await page.mouse.wheel(0, 120);
  await expect(page.locator('#title')).toHaveText(/November 2026/);
  expect(qp(page, 'month')).toBe('2026-11');
  expect(qp(page, 'date')).toBe(D); // the picked day stays
  // a second gesture after a rest (> 150 ms between events, practical.wheelTouch) turns back
  await page.waitForTimeout(400);
  await page.mouse.wheel(0, -120);
  await expect(page.locator('#title')).toHaveText(/Oktober 2026/);
});

// a real trackpad's stream, momentum included (wheel-touch.json): exactly one page per swipe — the rule the month
// grid turns its pages by (calendar.js monthWheel: a touch resets, 60 px turn once)
test('recorded trackpad swipes turn one month each', async ({ page }) => {
  await open(page, cal('month'));
  const rec = JSON.parse(fs.readFileSync(new URL('./wheel-touch.json', import.meta.url), 'utf8'));
  const got = await page.evaluate(parts => parts.map(p => { const touch = practical.wheelTouch(); let acc = 0, done = false, out = '';
    for (const [t, dy] of p.ev) { if (touch(t, dy)) { acc = 0; done = false; } if (done) continue; acc += dy; if (Math.abs(acc) >= 60) { done = true; out += acc > 0 ? '+' : '-'; } }
    return out; }), rec.parts);
  expect(got).toEqual(rec.parts.map(p => p.turns));
});

test('rail click jumps to a month', async ({ page }) => {
  await open(page, cal('month'));
  const dot = page.locator('.tl g.wdot[data-date="2026-12-01"]');
  await dot.scrollIntoViewIfNeeded();
  const bb = await dot.boundingBox();
  await page.mouse.click(bb.x + bb.width / 2, bb.y + bb.height / 2);
  await expect(page.locator('#title')).toHaveText(/Dezember 2026/);
  expect(qp(page, 'month')).toBe('2026-12');
});

test('week stream loads more weeks at both ends; title follows the focus week', async ({ page }) => {
  await open(page, cal('week'));
  await expect(page.locator('#title')).toHaveText(/KW 42 2026/);
  const weeks = page.locator('.stream .week');
  const n0 = await weeks.count();
  const later = page.waitForResponse(r => r.url().includes('/api/weeks?'));
  await page.locator('main.main').evaluate(m => { m.scrollTop = m.scrollHeight; });
  expect((await later).ok()).toBe(true);
  await expect.poll(() => weeks.count()).toBeGreaterThan(n0);
  await expect(page.locator('#title')).not.toHaveText(/KW 42 2026/);
  expect(qp(page, 'week')).not.toBeNull();
  const n1 = await weeks.count();
  const earlier = page.waitForResponse(r => r.url().includes('/api/weeks?'));
  await page.locator('main.main').evaluate(m => { m.scrollTop = 0; });
  await earlier;
  await expect.poll(() => weeks.count()).toBeGreaterThan(n1);
  // a heat-panel day scrolls its week into focus
  await open(page, cal('week'));
  await page.locator('.heat i[data-date="2026-10-28"]').click();
  await expect(page.locator('#title')).toHaveText(/KW 44 2026/);
  // the panel the client redraws for a month counts as the server's did: holidays are no entry, a multi-day event's
  // later days are (calendar.js `heatSync`, render/week.rs)
  const counts = () => page.locator('.heat i[data-date]').evaluateAll(c => c.map(i => `${i.dataset.date}:${i.style.getPropertyValue('--n')}`));
  const server = await counts();
  await page.locator('.week[data-week="2026-11-16"]').evaluate(w => w.scrollIntoView({ block: 'start' }));
  await expect(page.locator('.heat')).toHaveAttribute('data-month', '2026-11');
  await page.locator('.heat i[data-date="2026-10-26"]').click();
  await expect(page.locator('.heat')).toHaveAttribute('data-month', '2026-10');
  expect(await counts()).toEqual(server);
});

test('day ledger loads more days when scrolled', async ({ page }) => {
  await open(page, cal('day'));
  const rows = page.locator('.ledger .lrow');
  const n0 = await rows.count();
  const more = page.waitForResponse(r => r.url().includes('/api/days?'));
  await page.locator('.ledger').evaluate(l => { l.scrollTop = l.scrollHeight; });
  expect((await more).ok()).toBe(true);
  await expect.poll(() => rows.count()).toBeGreaterThan(n0);
  const n1 = await rows.count();
  const before = page.waitForResponse(r => r.url().includes('/api/days?'));
  await page.locator('.ledger').evaluate(l => { l.scrollTop = 0; });
  await before;
  await expect.poll(() => rows.count()).toBeGreaterThan(n1);
});

test('calendar filter: one off, none, mine, all — kept in localStorage', async ({ page }) => {
  await open(page, cal('month'));
  const f = pageFilter(page);
  await f.locator('> button').click();
  await f.locator('label', { hasText: 'Arbeit' }).locator('input').uncheck();
  await expect(chip(page, 'Projekt-Review')).toHaveCount(0);
  // the popover is as wide as its longest row: name, owner, eye and star stay on one line (chrome.css `.frow`) — a long
  // name of a group calendar, as real ones are, set here in place (the fixture's names fit any width)
  await f.locator('.frow', { hasText: 'Familienkalender' }).locator('label').evaluate(l => { [...l.childNodes].find(n => n.nodeType === 3 && n.textContent.includes('Familienkalender')).textContent = 'Familienkalender der ganzen Verwandtschaft'; });
  const heights = await f.locator('#calfilter .frow').evaluateAll(rows => rows.map(r => Math.round(r.getBoundingClientRect().height)));
  expect(new Set(heights).size, `${heights}`).toBe(1);
  // the box is invisible: a grey struck eye says hidden, a blue eye shown (chrome.css `.eye`)
  const eye = (name, off) => f.locator('label', { hasText: name }).locator(off ? '.eye.no' : '.eye:not(.no)');
  await expect(eye('Arbeit', true)).toBeVisible();
  await expect(eye('Arbeit', false)).toBeHidden();
  await expect(eye('Familienkalender', false)).toBeVisible();
  await expect(eye('Familienkalender', true)).toBeHidden();
  await expect(chip(page, 'Zahnarzt')).toHaveCount(1);
  expect(JSON.parse(await page.evaluate(() => localStorage.calHidden))).toEqual(['alice/5eed0000-0000-4000-8000-000000000001']);
  expect(qp(page, 'cals')).toContain('alice/5eed0000-0000-4000-8000-000000000002');
  await page.reload();
  await expect(page.locator('#viewroot')).toBeVisible();
  await expect(chip(page, 'Zahnarzt')).toHaveCount(1);
  await expect(chip(page, 'Projekt-Review')).toHaveCount(0);
  await f.locator('> button').click();
  await f.locator('[data-filter="none"]').click();
  await expect(page.locator('.v-month .ev[data-key]')).toHaveCount(0);
  await f.locator('[data-filter="mine"]').click();
  await expect(chip(page, 'Familienfest')).toHaveCount(0);
  await expect(chip(page, 'Projekt-Review')).toHaveCount(1);
  await expect(f.locator('label', { hasText: 'Familienkalender' }).locator('input')).not.toBeChecked();
  await f.locator('[data-filter="all"]').click();
  await expect(chip(page, 'Familienfest')).toHaveCount(1);
  expect(JSON.parse(await page.evaluate(() => localStorage.calHidden))).toEqual([]);
  // a click on the eye is one on its box (the box lies right under it)
  const at = await eye('Familienkalender', false).boundingBox();
  await page.mouse.click(at.x + at.width / 2, at.y + at.height / 2);
  await expect(f.locator('label', { hasText: 'Familienkalender' }).locator('input')).not.toBeChecked();
  await expect(chip(page, 'Familienfest')).toHaveCount(0);
  await page.mouse.click(at.x + at.width / 2, at.y + at.height / 2);
  await expect(chip(page, 'Familienfest')).toHaveCount(1);
  expect(JSON.parse(await page.evaluate(() => localStorage.calHidden))).toEqual([]);
});

test('a starred filter row is a quick switch left of the filter: it hides and shows like the row, kept per user', async ({ page }) => {
  await open(page, cal('month'));
  const f = pageFilter(page), pins = page.locator('header .qsbar .qs:not([hidden])');
  const star = name => f.locator('.frow', { hasText: name }).locator('.pinstar');
  await expect(pins).toHaveCount(0);
  await f.locator('> button').click();
  await star('Arbeit').click();
  await expect(star('Arbeit')).toHaveAttribute('aria-pressed', 'true');
  await expect(pins).toHaveCount(1);
  const pin = pins.first();
  await expect(pin).toHaveAttribute('aria-label', 'Arbeit');
  await page.keyboard.press('Escape');
  await page.mouse.move(0, 400);
  // a ribbon in the row's (softened) colour hanging from the top bar's upper edge; long while shown, rolled up while hidden (chrome.css `.qs`)
  const ribbon = () => pin.evaluate(b => { const r = getComputedStyle(b, '::before'), top = b.closest('.topbar').getBoundingClientRect().top; return { c: r.backgroundColor, h: r.height, at: b.getBoundingClientRect().top - top }; });
  // its tint: the row's swatch colour mixed 55 % into the bar's background (computed by the browser on a probe)
  const tint = await pin.evaluate(b => { const sw = document.querySelector(`#calfilter .frow:has(.pinstar[data-pin="${b.dataset.pin}"]) .sw`), p = document.createElement('i'); p.style.cssText = `position:absolute;background:color-mix(in srgb, ${getComputedStyle(sw).backgroundColor} 55%, ${getComputedStyle(document.querySelector('.topbar')).backgroundColor})`; document.body.append(p); const c = getComputedStyle(p).backgroundColor; p.remove(); return c; });
  await expect.poll(ribbon).toEqual({ c: tint, h: '30px', at: 0 });
  await pin.click();
  await expect(chip(page, 'Projekt-Review')).toHaveCount(0);
  await expect(pin).toHaveAttribute('aria-pressed', 'false');
  await page.mouse.move(0, 400); // away from the ribbon: hovered, it reaches out further
  await expect.poll(async () => (await ribbon()).h).toBe('14px');
  await expect(f.locator('label', { hasText: 'Arbeit' }).locator('input')).not.toBeChecked();
  // the row's own box moves the switch too, and "all" does without a change event
  await f.locator('> button').click();
  await f.locator('[data-filter="all"]').click();
  await expect(pin).toHaveAttribute('aria-pressed', 'true');
  await expect(chip(page, 'Projekt-Review')).toHaveCount(1);
  // stored for the user: the next page brings it, the tasks page (same calendars) too
  await page.reload();
  await expect(pins).toHaveCount(1);
  await page.goto('/tasks');
  await expect(page.locator('header .qsbar .qs:not([hidden])')).toHaveAttribute('aria-label', 'Arbeit');
  await open(page, cal('month'));
  await f.locator('> button').click();
  await star('Arbeit').click();
  await expect(pins).toHaveCount(0);
  await page.reload();
  await expect(pins).toHaveCount(0);
  // a double click saves twice, one after the other: star, switch and server agree afterwards (unpinned again)
  await f.locator('> button').click();
  const saves = [];
  page.on('response', r => { if (/\/settings\/filter-pin$/.test(r.url())) saves.push(r); });
  await star('Arbeit').dblclick();
  await expect.poll(() => saves.length).toBe(2);
  await expect(star('Arbeit')).toHaveAttribute('aria-pressed', 'false');
  await expect(pins).toHaveCount(0);
  expect(await saves[1].json()).toEqual([]);
});

test('holiday switches are saved for the user and redraw the view', async ({ page }) => {
  await open(page, cal('month'));
  const f = pageFilter(page);
  const pub = f.locator('input[data-hol="public"]'), school = f.locator('input[data-hol="school"]');
  const holName = page.locator('.v-month .day[data-date="2026-10-03"] .fname');
  await expect(holName).toContainText('Tag der Deutschen Einheit');
  await expect(page.locator('.v-month .day[data-date="2026-10-12"]')).toContainText('Herbstferien');
  try {
    await f.locator('> button').click();
    let saved = page.waitForResponse(r => r.url().endsWith('/settings/holiday-display') && r.request().method() === 'POST');
    await pub.uncheck();
    expect((await saved).status()).toBe(204);
    await expect(holName).toHaveCount(0);
    saved = page.waitForResponse(r => r.url().endsWith('/settings/holiday-display'));
    await school.uncheck();
    await saved;
    await expect(page.locator('.v-month .day[data-date="2026-10-12"]')).not.toContainText('Herbstferien');
    // stored server-side: a fresh load shows them off
    await open(page, cal('month'));
    await expect(holName).toHaveCount(0);
    await expect(pub).not.toBeChecked();
    await expect(school).not.toBeChecked();
  } finally {
    await api(page, 'POST', '/settings/holiday-display', { public: true, school: true });
  }
  await open(page, cal('month'));
  await expect(holName).toContainText('Tag der Deutschen Einheit');
  await expect(pub).toBeChecked();
});

test('events open in view mode from month chip, week item, day document and agenda', async ({ page }) => {
  await open(page, cal('month'));
  await chip(page, 'Projekt-Review').click();
  await expectEditor(page, 'Projekt-Review');
  await open(page, cal('week'));
  await page.locator('.stream [data-key]', { hasText: 'Kundentermin' }).first().click();
  await expectEditor(page, 'Kundentermin');
  await open(page, cal('day'));
  await page.locator('.doc [data-key]', { hasText: 'Mittagessen mit Bob' }).first().click();
  await expectEditor(page, 'Mittagessen mit Bob');
  // the agenda below the grid exists on narrow screens
  await page.setViewportSize({ width: 390, height: 844 });
  await open(page, cal('month'));
  await page.locator('.agenda .lnk[data-key]', { hasText: 'Zahnarzt' }).click();
  await expectEditor(page, 'Zahnarzt Dr. Weiß');
});

test('FAB and a day\'s add button open a new event on that day', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await open(page, cal('month', '2026-10-16'));
  await page.locator('#fab').click();
  await expect(page.locator('#ed')).toBeVisible();
  await expect(page.locator('#edform')).toHaveClass(/\bmode-edit\b/);
  await expect(page.locator('#edh')).toHaveValue('');
  await expect(page.locator('#edform [name="date"]')).toHaveValue('2026-10-16');
  await page.locator('#ed-close').click();
  await expect(page.locator('#ed')).toBeHidden();
  await page.setViewportSize({ width: 1440, height: 900 });
  await open(page, cal('day'));
  await page.locator('.lrow[data-date="2026-10-20"] .add').click();
  await expect(page.locator('#ed')).toBeVisible();
  await expect(page.locator('#edform [name="date"]')).toHaveValue('2026-10-20');
});

test('live update: an event bob adds to the shared calendar appears for alice without reload', async ({ page, browser }) => {
  await open(page, cal('month'));
  const title = `F1 Live ${Date.now() % 100000}`;
  await expect(chip(page, title)).toHaveCount(0);
  const ctx = await browser.newContext({ baseURL: BASE });
  const bob = await ctx.newPage();
  let id = null;
  try {
    await bob.goto('/calendar'); // development mode signs alice in first
    const csrf = await bob.locator('meta[name=csrf]').getAttribute('content');
    const r = await bob.request.post('/login', { form: { username: 'bob', password: 'devpass', csrf, redirect_uri: '/calendar' }, headers: { Origin: BASE }, maxRedirects: 0 });
    expect(r.status()).toBe(303);
    await bob.goto('/calendar');
    await expect(bob.locator('body')).toHaveAttribute('data-user', 'bob');
    const [p, c] = FAM.split('/');
    id = (await api(bob, 'POST', '/api/events', { principal: p, cal_id: c, summary: title, start: '2026-10-16T18:00:00', end: '2026-10-16T19:00:00' })).id;
    // no reload on alice's side: the SSE change reloads the fragment
    await expect(chip(page, title)).toHaveCount(1, { timeout: 15000 });
    await expect(page.locator('.v-month .day[data-date="2026-10-16"]')).toContainText(title);
    await api(bob, 'DELETE', `/api/events/${FAM}/${encodeURIComponent(id)}`);
    id = null;
    await expect(chip(page, title)).toHaveCount(0, { timeout: 15000 });
  } finally {
    if (id) await api(bob, 'DELETE', `/api/events/${FAM}/${encodeURIComponent(id)}`).catch(() => {});
    await ctx.close();
  }
});
