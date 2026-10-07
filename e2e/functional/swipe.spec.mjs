// Sideways swipes switch a page's views (core.js practical.swipeViews over wheelPages/wheelAxis): calendar
// day | week | month | year, tasks, contacts, history. Wheel (trackpad stream, synthetic momentum, recorded swipes),
// touch (phone, CDP touch input), the edges, content that scrolls sideways first, and what must stay as it was.
import { test, expect, settle } from '../lib/test.mjs';
import fs from 'node:fs';

const cal = (view, date = '2026-10-14') => `/calendar?view=${view}&date=${date}`;
const qp = (page, k) => new URL(page.url()).searchParams.get(k);
const calView = page => page.locator('#viewroot .toolbar .seg > button[data-view][aria-pressed=true]');
const rest = page => page.waitForTimeout(400); // > 150 ms without an event: the next wheel event starts a new touch (practical.wheelTouch)

async function over(page, sel) {
  const bb = await page.locator(sel).first().boundingBox(), top = Math.max(0, bb.y), bottom = Math.min(bb.y + bb.height, page.viewportSize().height);
  await page.mouse.move(bb.x + bb.width / 2, top + Math.min((bottom - top) / 2, 200));
}
// one trackpad touch with a long momentum tail, as one burst of events (in-page: no gap of the test driver can
// split it into several touches), dispatched on the element under the pointer's spot of `sel`
async function burst(page, sel, sign) {
  return page.evaluate(([sel, sign]) => {
    const r = document.querySelector(sel).getBoundingClientRect(), top = Math.max(0, r.top), el = document.elementFromPoint(r.x + r.width / 2, top + Math.min((Math.min(r.bottom, innerHeight) - top) / 2, 200));
    let cancelled = 0;
    for (let i = 0; i < 80; i++) { const e = new WheelEvent('wheel', { deltaX: sign * Math.max(1, 40 * 0.94 ** i), deltaY: (i % 3) - 1, bubbles: true, cancelable: true }); el.dispatchEvent(e); if (e.defaultPrevented) cancelled++; }
    return cancelled;
  }, [sel, sign]);
}
async function touchSwipe(page, x0, y0, x1, y1, steps = 10) {
  const cdp = await page.context().newCDPSession(page);
  const at = (x, y) => [{ x, y, id: 1, radiusX: 2, radiusY: 2, force: 1 }];
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: at(x0, y0) });
  for (let i = 1; i <= steps; i++) await cdp.send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: at(x0 + (x1 - x0) * i / steps, y0 + (y1 - y0) * i / steps) });
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
  await cdp.detach();
}

test.describe('desktop wheel', () => {
  test('calendar: a sideways touch steps one view, none past the ends; vertical turns pages as before', async ({ page }) => {
    await page.goto(cal('month'));
    await over(page, '.v-month .month');
    await page.mouse.wheel(120, 0); // fingers to the left: the view to the right
    await expect(calView(page)).toHaveAttribute('data-view', 'planner');
    expect(qp(page, 'view')).toBe('planner');
    await rest(page); await over(page, '.planner');
    await page.mouse.wheel(120, 0); // year is the last
    await settle(page);
    expect(qp(page, 'view')).toBe('planner');
    for (const v of ['month', 'week', 'day']) { await rest(page); await page.mouse.wheel(-120, 0); await expect(calView(page)).toHaveAttribute('data-view', v); }
    await rest(page); await page.mouse.wheel(-120, 0); // day is the first
    await settle(page);
    expect(qp(page, 'view')).toBe('day');
    await page.goBack(); // a view switch is a history entry, as the button's
    await expect(calView(page)).toHaveAttribute('data-view', 'week');
    // vertical: the month grid still turns one month, the view stays
    await page.goto(cal('month'));
    await over(page, '.v-month .month');
    await page.mouse.wheel(0, 120);
    await expect(page.locator('#title')).toHaveText(/November 2026/);
    expect(qp(page, 'view')).toBe('month');
    // diagonal (sideways share not twice the vertical): a vertical touch, never a view switch
    await rest(page); await page.mouse.wheel(70, -60);
    await expect(page.locator('#title')).toHaveText(/Oktober 2026/);
    await settle(page);
    expect(qp(page, 'view')).toBe('month');
  });

  test('calendar: one long touch with momentum switches once; the "more" views and an open menu do not swipe', async ({ page }) => {
    await page.goto(cal('week'));
    expect(await burst(page, '.stream', 1)).toBe(80); // all of its events cancelled (no browser back/forward swipe)
    await expect(calView(page)).toHaveAttribute('data-view', 'month');
    await settle(page);
    expect(qp(page, 'view')).toBe('month'); // not on to the year
    await page.goto(cal('weekends'));
    await over(page, '.cal');
    await page.mouse.wheel(120, 0); await rest(page); await page.mouse.wheel(-120, 0);
    await settle(page);
    expect(qp(page, 'view')).toBe('weekends');
    await page.goto(cal('month'));
    await page.locator('[data-more]').click();
    await expect(page.locator('.more.open')).toBeVisible();
    await over(page, '.v-month .month');
    await page.mouse.wheel(120, 0);
    await settle(page);
    expect(qp(page, 'view')).toBe('month');
  });

  test('tasks: sideways steps through the switch in order; not from inside the docked editor', async ({ page }) => {
    await page.goto('/tasks?view=groups&back=all');
    const on = page.locator('#tk-views button[aria-pressed=true]');
    await expect(on).toHaveAttribute('data-tkview', 'groups');
    await over(page, '#tkmain');
    await page.mouse.wheel(-120, 0); // the first
    await settle(page);
    await expect(on).toHaveAttribute('data-tkview', 'groups');
    await over(page, '#ed.docked .box');
    await rest(page); await page.mouse.wheel(120, 0);
    await settle(page);
    await expect(on).toHaveAttribute('data-tkview', 'groups');
    await over(page, '#tkmain');
    await rest(page); await page.mouse.wheel(120, 0);
    await expect(on).toHaveAttribute('data-tkview', 'table');
    // the table is wider than the window (1513 px in 1408): the touch moves it, the view stays
    const sw = page.locator('#tkmain .sheetwrap');
    expect(await sw.evaluate(e => e.scrollWidth - e.clientWidth)).toBeGreaterThan(50);
    await over(page, '#tkmain .sheetwrap');
    await rest(page); await page.mouse.wheel(40, 0);
    await expect.poll(() => sw.evaluate(e => e.scrollLeft)).toBeGreaterThan(0);
    await settle(page);
    await expect(on).toHaveAttribute('data-tkview', 'table');
    await sw.evaluate(e => { e.scrollLeft = e.scrollWidth; });
    for (const v of ['board', 'timeline', 'deck', 'history']) { await rest(page); await page.mouse.wheel(120, 0); await expect(on).toHaveAttribute('data-tkview', v); }
    expect(qp(page, 'view')).toBe('history');
    await rest(page); await page.mouse.wheel(120, 0); // the last
    await settle(page);
    await expect(on).toHaveAttribute('data-tkview', 'history');
    await page.goto('/tasks?view=groups&back=all');
    await burst(page, '#tkmain', 1);
    await expect(on).toHaveAttribute('data-tkview', 'table');
    await settle(page);
    await expect(on).toHaveAttribute('data-tkview', 'table');
  });

  test('contacts: list and table by a sideways touch', async ({ page }) => {
    await page.goto('/contacts');
    await expect(page.locator('.list li[data-key]').first()).toBeVisible();
    await over(page, '.panels');
    await page.mouse.wheel(0, 400); // vertical: the page scrolls as before
    await expect.poll(() => page.evaluate(() => document.querySelector('.main').scrollTop)).toBeGreaterThan(0);
    await expect(page.locator('[data-ctview=list]')).toHaveAttribute('aria-pressed', 'true');
    await rest(page); await page.mouse.wheel(120, 0);
    await expect(page.locator('#contacts.view-table')).toBeVisible();
    await expect(page.locator('[data-ctview=table]')).toHaveAttribute('aria-pressed', 'true');
    await rest(page); await page.mouse.wheel(120, 0); // the last
    await rest(page); await over(page, '.sheetwrap'); await page.mouse.wheel(-120, 0);
    await expect(page.locator('#contacts:not(.view-table)')).toBeVisible();
    await expect(page.locator('[data-ctview=list]')).toHaveAttribute('aria-pressed', 'true');
  });

  test('history: map and list by a sideways touch', async ({ page }) => {
    await page.goto('/history?view=map');
    await expect(page.locator('.hxg-scroll')).toBeVisible();
    await over(page, '.hxg-scroll');
    await page.mouse.wheel(-120, 0); // the first
    await settle(page);
    await expect(page.locator('.toolbar [data-view=map]')).toHaveAttribute('aria-pressed', 'true');
    await rest(page); await page.mouse.wheel(120, 0);
    await expect(page.locator('.toolbar [data-view=list]')).toHaveAttribute('aria-pressed', 'true');
    expect(await page.evaluate(() => localStorage.hxView)).toBe('list');
    await rest(page); await over(page, '.hxc-list'); await page.mouse.wheel(-120, 0);
    await expect(page.locator('.toolbar [data-view=map]')).toHaveAttribute('aria-pressed', 'true');
  });
});

// the recording behind wheel-touch.json through the real listeners: vertical as recorded turns the same pages it did,
// with sideways noise (a third of each delta) too; the same swipes sideways switch exactly as often and turn nothing
test('recorded trackpad swipes: one page per vertical swipe, one view per sideways swipe', async ({ page }) => {
  await page.goto(cal('month'));
  const rec = JSON.parse(fs.readFileSync(new URL('./wheel-touch.json', import.meta.url), 'utf8'));
  const got = await page.evaluate(parts => parts.map(p => {
    const run = (mk) => { let y = '', x = ''; const ly = practical.wheelPages(d => { y += d > 0 ? '+' : '-'; }), lx = practical.wheelPages(d => { x += d > 0 ? '+' : '-'; }, null, 'x');
      for (const [t, d] of p.ev) { const e = { ...mk(d), deltaMode: 0, timeStamp: t, currentTarget: document.body, preventDefault() {} }; ly(e); lx(e); }
      return [y, x]; };
    return { vertical: run(d => ({ deltaX: 0, deltaY: d })), noisy: run(d => ({ deltaX: d / 3, deltaY: d })), sideways: run(d => ({ deltaX: d, deltaY: 0 })) };
  }), rec.parts);
  for (const [i, p] of rec.parts.entries()) {
    expect(got[i].vertical, p.name).toEqual([p.turns, '']);
    expect(got[i].noisy, p.name).toEqual([p.turns, '']);
    expect(got[i].sideways, p.name).toEqual(['', p.turns]);
  }
});

test.describe('phone touch', () => {
  test.use({ viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, isMobile: true, hasTouch: true });
  test('calendar: a sideways swipe switches one view; short, vertical and diagonal ones do not', async ({ page }) => {
    await page.goto(cal('month'));
    const y = 450;
    await touchSwipe(page, 300, y, 100, y + 20); // fingers to the left: the view to the right
    await expect(calView(page)).toHaveAttribute('data-view', 'planner');
    await touchSwipe(page, 300, y, 100, y); // year is the last
    await settle(page);
    expect(qp(page, 'view')).toBe('planner');
    await touchSwipe(page, 100, y, 300, y);
    await expect(calView(page)).toHaveAttribute('data-view', 'month');
    await touchSwipe(page, 200, y, 160, y); // 40 px: a tap or a nudge
    await touchSwipe(page, 200, 600, 120, 450); // diagonal: 80 sideways, 150 up
    await touchSwipe(page, 200, 650, 200, 350); // vertical
    await settle(page);
    expect(qp(page, 'view')).toBe('month');
  });
  test('contacts: the table scrolls sideways first, at its edge the swipe switches to the list', async ({ page }) => {
    await page.goto('/contacts?view=table');
    const sw = page.locator('.sheetwrap');
    await expect(sw).toBeVisible();
    const max = await sw.evaluate(e => e.scrollWidth - e.clientWidth);
    expect(max).toBeGreaterThan(100);
    await sw.evaluate(e => { e.scrollLeft = e.scrollWidth; });
    const bb = await sw.boundingBox(), y = bb.y + Math.min(bb.height / 2, 200);
    await touchSwipe(page, 100, y, 300, y); // fingers right: the table moves back, the view stays
    await expect.poll(() => sw.evaluate(e => e.scrollLeft)).toBeLessThan(max - 50);
    await settle(page);
    await expect(page.locator('#contacts.view-table')).toBeVisible();
    await sw.evaluate(e => { e.scrollLeft = 0; });
    await touchSwipe(page, 100, y, 300, y);
    await expect(page.locator('#contacts:not(.view-table)')).toBeVisible();
    await expect(page.locator('[data-ctview=list]')).toHaveAttribute('aria-pressed', 'true');
  });
  test('history: the map scrolls sideways first, at its edge the swipe switches to the list', async ({ page }) => {
    await page.goto('/history?view=map');
    const sc = page.locator('.hxg-scroll'), list = page.locator('.toolbar [data-view=list]');
    await expect(sc).toBeVisible();
    const max = await sc.evaluate(e => e.scrollWidth - e.clientWidth);
    expect(max).toBeGreaterThan(200);
    await sc.evaluate(e => { e.scrollLeft = 0; });
    const bb = await sc.boundingBox(), y = bb.y + 120;
    await touchSwipe(page, 300, y, 100, y); // fingers left: the map moves on, the view stays
    await expect.poll(() => sc.evaluate(e => e.scrollLeft)).toBeGreaterThan(50);
    await settle(page);
    await expect(list).toHaveAttribute('aria-pressed', 'false');
    await sc.evaluate(e => { e.scrollLeft = e.scrollWidth; });
    await touchSwipe(page, 300, y, 100, y);
    await expect(list).toHaveAttribute('aria-pressed', 'true');
  });
  test('tasks: a swipe from inside a field does nothing', async ({ page }) => {
    await page.goto('/tasks?view=table&back=all');
    const on = page.locator('#tk-views button[aria-pressed=true]');
    await expect(on).toHaveAttribute('data-tkview', 'table');
    const bb = await page.locator('#tk-back').boundingBox();
    await touchSwipe(page, bb.x + bb.width / 2, bb.y + bb.height / 2, bb.x + bb.width / 2 + 200, bb.y + bb.height / 2);
    await settle(page);
    await expect(on).toHaveAttribute('data-tkview', 'table');
    await touchSwipe(page, 100, 500, 300, 500);
    await expect(on).toHaveAttribute('data-tkview', 'groups');
  });
});

test('the browser\'s own back/forward swipe is off on every page (base.css): a sideways swipe never leaves the page', async ({ page }) => {
  for (const u of [cal('month'), '/tasks', '/contacts', '/history', '/settings']) {
    await page.goto(u);
    expect(await page.evaluate(() => getComputedStyle(document.documentElement).overscrollBehaviorX), u).toBe('none');
  }
});

// a view switch slides (core.js practical.slide, frame.css pc-view): the side follows the switch's order, for clicks and
// swipes alike; nothing of it stays behind; with reduced motion (the suite's default) there is none
test.describe('slide', () => {
  test.use({ reducedMotion: 'no-preference' });
  test.beforeEach(({ page }) => page.addInitScript(() => {
    window.__slides = []; window.__anims = []; window.__bars = [];
    const vt = Document.prototype.startViewTransition; // what runs once the transition is ready
    if (vt) Document.prototype.startViewTransition = function (...a) { const t = vt.apply(this, a); t.ready.then(() => { const bar = document.querySelector('.main .toolbar:has(.seg [aria-pressed])'); window.__bars.push(bar ? getComputedStyle(bar).viewTransitionName : 'no toolbar'); window.__anims.push(document.getAnimations().filter(x => x.effect && x.effect.pseudoElement).map(x => `${x.animationName} ${x.effect.pseudoElement}`).sort().join(', ')); }, () => {}); return t; };
    new MutationObserver(() => { const v = document.documentElement.dataset.slide; if (v) window.__slides.push(v); }).observe(document, { attributes: true, subtree: true, attributeFilter: ['data-slide'] }); // the document: an init script runs before <html> exists
  }));
  const slides = page => page.evaluate(() => window.__slides);
  const clean = async page => {
    await expect.poll(() => page.evaluate(() => document.documentElement.hasAttribute('data-slide'))).toBe(false);
    expect(await page.evaluate(() => [getComputedStyle(document.querySelector('.main')).viewTransitionName, document.getAnimations().length])).toEqual(['none', 0]);
  };
  test('calendar: buttons and swipes slide towards the view, one per switch; a page turn does not', async ({ page }) => {
    await page.goto(cal('month'));
    await page.locator('.toolbar [data-view=week]').click();
    await expect(calView(page)).toHaveAttribute('data-view', 'week');
    await clean(page);
    expect(await slides(page)).toEqual(['prev']);
    expect(await page.evaluate(() => window.__anims[0])).toBe('pc-from-l ::view-transition-new(pc-view), pc-to-r ::view-transition-old(pc-view)');
    await over(page, '.stream'); await page.mouse.wheel(120, 0);
    await expect(calView(page)).toHaveAttribute('data-view', 'month');
    await clean(page);
    await over(page, '.v-month .month'); await rest(page); await page.mouse.wheel(0, 120); // a page turn: the month glide, no slide
    await expect(page.locator('#title')).toHaveText(/November 2026/);
    await page.locator('.toolbar [data-view=planner]').click();
    await expect(calView(page)).toHaveAttribute('data-view', 'planner');
    await clean(page);
    expect(await slides(page)).toEqual(['prev', 'next', 'next']);
    await page.goBack(); // back to the month: from the left
    await expect(calView(page)).toHaveAttribute('data-view', 'month');
    await clean(page);
    expect(await slides(page)).toEqual(['prev', 'next', 'next', 'prev']);
    // the toolbar with the view switch is its own capture (pc-bar): it does not slide (no animation names it, above)
    expect(await page.evaluate(() => window.__bars)).toEqual(['pc-bar', 'pc-bar', 'pc-bar', 'pc-bar']);
  });
  test('tasks, contacts, history: the button and the swipe slide; the same view again does not', async ({ page }) => {
    await page.goto('/tasks?view=groups&back=all');
    await page.locator('#tk-views [data-tkview=board]').click();
    await expect(page.locator('#tk-views [data-tkview=board]')).toHaveAttribute('aria-pressed', 'true');
    await clean(page);
    await page.locator('#tk-views [data-tkview=board]').click();
    await page.locator('#tk-views [data-tkview=table]').click();
    await expect(page.locator('#tk-views [data-tkview=table]')).toHaveAttribute('aria-pressed', 'true');
    await clean(page);
    expect(await slides(page)).toEqual(['next', 'prev']);
    await page.goto('/contacts');
    await expect(page.locator('.list li[data-key]').first()).toBeVisible();
    await over(page, '.panels'); await page.mouse.wheel(120, 0);
    await expect(page.locator('#contacts.view-table')).toBeVisible();
    await clean(page);
    await page.locator('[data-ctview=list]').click();
    await expect(page.locator('#contacts:not(.view-table)')).toBeVisible();
    await clean(page);
    expect(await slides(page)).toEqual(['next', 'prev']);
    await page.goto('/history?view=map');
    await page.locator('.toolbar [data-view=list]').click();
    await expect(page.locator('.hxc-list')).toBeVisible();
    await clean(page);
    expect(await slides(page)).toEqual(['next']);
    expect(await page.evaluate(() => [...new Set(window.__bars)])).toEqual(['pc-bar']); // the view switch stays, on every page
    expect(await page.evaluate(() => window.__anims.every(a => !a.includes('pc-bar')))).toBe(true);
  });
  test('reduced motion: no slide', async ({ page }) => {
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await page.goto('/tasks?view=groups&back=all');
    await page.locator('#tk-views [data-tkview=board]').click();
    await expect(page.locator('#tk-views [data-tkview=board]')).toHaveAttribute('aria-pressed', 'true');
    expect(await slides(page)).toEqual([]);
  });
});
