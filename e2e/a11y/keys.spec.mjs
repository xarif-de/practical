// Keyboard-only flows (project a11y-desktop-light only, playwright.config.mjs): every Tab stop shows that it has the
// focus; the editor opens, edits, saves and closes from the keyboard with the focus kept inside it and given back; the
// questions, search, the day ledger, contacts, tasks; the page at 200 % zoom; reduced motion. Own data only ("A11y …");
// fresh servers per spec file (lib/test.mjs).
import { api, expect, settle, test } from '../lib/test.mjs';

const active = page => page.evaluate(() => { const a = document.activeElement; return a && a !== document.body ? (a.id ? '#' + a.id : a.tagName.toLowerCase() + (a.className && typeof a.className === 'string' ? '.' + a.className.trim().split(/\s+/).join('.') : '')) : 'body'; });
/** Tab (Shift+Tab with back) until `sel` has the focus; at most `n` presses. */
async function tabTo(page, sel, { n = 120, back = false } = {}) {
  const trail = [];
  for (let i = 0; i < n; i++) {
    await page.keyboard.press(back ? 'Shift+Tab' : 'Tab');
    if (await page.evaluate(s => !!document.activeElement && document.activeElement.matches(s), sel)) return;
    trail.push(await active(page));
  }
  throw new Error(`${sel} not reached in ${n} Tab presses; the last stops: ${trail.slice(-12).join(' → ')}`);
}
const inDialog = (page, box) => page.evaluate(b => !!document.activeElement && !!document.activeElement.closest(b), box);

test('every Tab stop shows the focus (the text fields: their caret)', async ({ page }) => {
  const bare = [];
  for (const url of ['/calendar?view=month&date=2026-10-14', '/contacts', '/tasks', '/settings', '/history']) {
    await page.goto(url);
    await settle(page);
    for (let i = 0; i < 200; i++) {
      await page.keyboard.press('Tab');
      const d = await page.evaluate(() => {
        const el = document.activeElement;
        if (!el || el === document.body) return null;
        if (el.dataset.e2eSeen) return { again: true };
        el.dataset.e2eSeen = '1';
        // a field that takes text shows a caret; a range, a checkbox and the like have no caret
        if (el.isContentEditable || el.tagName === 'TEXTAREA' || (el.tagName === 'INPUT' && /^(text|search|url|email|tel|number|password|date|time|datetime-local)$/.test(el.type))) return { ok: true };
        // the element's own look or a descendant's (a history stop colours its hit area)
        const props = ['outline-style', 'outline-width', 'box-shadow', 'border-top-color', 'border-bottom-color', 'background-color', 'color', 'text-decoration-line', 'fill', 'stroke'];
        const els = [el, ...el.querySelectorAll('*')].slice(0, 30), look = () => els.flatMap(e => { const cs = getComputedStyle(e); return props.map(p => cs.getPropertyValue(p)); });
        const on = look();
        el.blur();
        const off = look();
        el.focus();
        const r = el.getBoundingClientRect();
        return { ok: on.some((v, i) => v !== off[i]) || !r.width, what: `${el.tagName.toLowerCase()}${el.id ? '#' + el.id : ''}.${(el.getAttribute('class') || '').trim().split(/\s+/).join('.')} "${(el.getAttribute('aria-label') || el.title || el.textContent).trim().slice(0, 30)}"` };
      });
      if (!d) continue;
      if (d.again) break;
      if (!d.ok) bare.push(`${url}: ${d.what}`);
    }
  }
  expect(bare, 'Tab stops that look the same with and without the focus').toEqual([]);
});

test('the editor from the keyboard: open, focus inside, trapped, Escape, the focus back', async ({ page }) => {
  await page.goto('/calendar?view=month&date=2026-10-14');
  const chip = page.locator('.v-month .day[data-date="2026-10-14"] .ev[data-key]').first();
  const key = await chip.getAttribute('data-key');
  await tabTo(page, `.v-month .day[data-date="2026-10-14"] .ev[data-key="${key}"]`, { n: 200 });
  await page.keyboard.press('Enter');
  await page.locator('#ed:not([hidden])').waitFor();
  await expect.poll(() => active(page)).toBe('#ed-lock');
  for (let i = 0; i < 30; i++) { await page.keyboard.press('Tab'); expect(await inDialog(page, '#ed .box'), `Tab ${i + 1} left the dialog`).toBe(true); }
  for (let i = 0; i < 30; i++) { await page.keyboard.press('Shift+Tab'); expect(await inDialog(page, '#ed .box'), `Shift+Tab ${i + 1} left the dialog`).toBe(true); }
  await page.keyboard.press('Escape');
  await expect(page.locator('#ed')).toBeHidden();
  expect(await page.evaluate(() => document.activeElement.dataset.key)).toBe(key);
});

test('a new event, edited and saved from the keyboard', async ({ page }) => {
  await page.goto('/calendar?view=month&date=2026-11-02');
  await tabTo(page, '.v-month .day[data-date="2026-11-02"] .add', { n: 200 });
  await page.keyboard.press('Enter');
  await page.locator('#ed:not([hidden])').waitFor();
  await expect.poll(() => active(page)).toBe('#edh');
  await page.keyboard.type('A11y Tastatur');
  await tabTo(page, '#ed-save');
  await page.keyboard.press('Enter');
  await expect(page.locator('#ed')).toBeHidden();
  const evs = await api(page, 'GET', '/api/events?start=2026-11-02&end=2026-11-03');
  expect(evs.map(e => e.summary)).toContain('A11y Tastatur');
});

test('the question: focus on "yes", named by its text, trapped, Escape gives the focus back', async ({ page }) => {
  await page.goto('/calendar?view=day&date=2026-10-14');
  await tabTo(page, 'article.lnk[data-key]', { n: 200 }); // the day ledger is one stop
  await page.keyboard.press('Enter');
  await page.locator('#ed:not([hidden])').waitFor();
  await page.keyboard.press('Enter'); // the unlock button has the focus: edit mode, where delete is enabled
  await page.locator('#edform.mode-edit').waitFor();
  await tabTo(page, '#ed-delete');
  await page.keyboard.press('Enter');
  const ask = page.locator('.ask[role=alertdialog]');
  await ask.waitFor();
  await expect(ask).toHaveAttribute('aria-label', /.+/);
  expect(await page.evaluate(() => document.activeElement.dataset.a)).toBe('1');
  for (let i = 0; i < 4; i++) { await page.keyboard.press('Tab'); expect(await inDialog(page, '.ask')).toBe(true); }
  await page.keyboard.press('Escape');
  await expect(ask).toHaveCount(0);
  await expect.poll(() => active(page)).toBe('#ed-delete');
  await expect(page.locator('#ed')).toBeVisible();
});

test('search from the keyboard: "/", a hit, Enter, the entry opens', async ({ page }) => {
  await page.goto('/calendar?view=day&date=2026-10-01');
  await settle(page);
  await page.keyboard.press('/');
  await expect.poll(() => active(page)).toBe('#q');
  await page.keyboard.type('Zahnarzt');
  await page.locator('#sres .r').first().waitFor();
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('Enter');
  await expect(page.locator('#viewroot')).toHaveAttribute('data-date', '2026-10-14');
  await tabTo(page, 'article.lnk[data-key*="zahnarzt"]', { n: 200 });
  await page.keyboard.press('Enter');
  await expect(page.locator('#edh')).toHaveValue(/Zahnarzt/);
});

test('the day ledger is one Tab stop: ↓ moves along it, Enter opens that day', async ({ page }) => {
  await page.goto('/calendar?view=day&date=2026-10-14');
  await tabTo(page, '.ledger .lrow');
  expect(await page.evaluate(() => document.activeElement.dataset.date)).toBe('2026-10-14');
  await page.keyboard.press('ArrowDown');
  expect(await page.evaluate(() => document.activeElement.dataset.date)).toBe('2026-10-15');
  await page.keyboard.press('Tab'); // out of the ledger in one step
  expect(await page.evaluate(() => !!document.activeElement.closest('.ledger'))).toBe(false);
  await page.keyboard.press('Shift+Tab'); // back: the row it left
  expect(await page.evaluate(() => document.activeElement.dataset.date)).toBe('2026-10-15');
  await page.keyboard.press('Enter');
  await expect(page.locator('#viewroot')).toHaveAttribute('data-date', '2026-10-15');
});

test('contacts and tasks: an entry opens from the keyboard; the docked task pane is no modal', async ({ page }) => {
  await page.goto('/contacts');
  await tabTo(page, '.list li[data-name="Anna Albrecht"]', { n: 200 });
  await page.keyboard.press('Enter');
  await expect(page.locator('#ctform input[name=full_name]')).toHaveValue('Anna Albrecht');
  // the list inserted later (/contacts/part, the page opened as the table): its rows work the same
  await page.goto('/contacts?view=table');
  await page.click('[data-ctview=list]');
  await page.locator('.list li[data-name="Ben Albrecht"]').waitFor();
  await tabTo(page, '.list li[data-name="Ben Albrecht"]', { n: 200 });
  await page.keyboard.press('Enter');
  await expect(page.locator('#ctform input[name=full_name]')).toHaveValue('Ben Albrecht');
  await page.goto('/tasks?view=groups&back=all');
  await page.locator('#ed.docked').waitFor();
  await expect(page.locator('#ed .box')).toHaveAttribute('aria-modal', 'false');
  const card = page.locator('#tkmain .tkcard[data-key]').first();
  const title = (await card.locator('.ti').textContent()).trim();
  await tabTo(page, `#tkmain .tkcard[data-key="${await card.getAttribute('data-key')}"]`, { n: 200 });
  await page.keyboard.press('Enter');
  await expect(page.locator('#edh')).toHaveValue(title);
  await tabTo(page, '#tkmain .tkcard', { n: 200, back: true }); // the docked pane lets the focus out
});

test('200 % zoom (720 × 450 CSS px): no page scrolls sideways, the navigation stays reachable', async ({ page }) => {
  await page.setViewportSize({ width: 720, height: 450 });
  for (const url of ['/calendar?view=month&date=2026-10-14', '/calendar?view=week&date=2026-10-14', '/contacts', '/tasks', '/settings', '/history']) {
    await page.goto(url);
    await settle(page);
    const w = await page.evaluate(() => [document.documentElement.scrollWidth, innerWidth]);
    expect(w[0], `${url} is wider than the window`).toBeLessThanOrEqual(w[1]);
    await expect(page.locator('a[href="/settings"]:visible').first(), url).toBeInViewport();
  }
});

test('reduced motion: a page turn in the month view does not glide', async ({ page }) => {
  await page.goto('/calendar?view=month&date=2026-11-14');
  await settle(page);
  // the glide (calendar.js monthFlip) shows the pages in a .mflip column while it runs: record whether one appears
  await page.evaluate(() => { window.__flip = false; new MutationObserver(() => { if (document.querySelector('.mflip')) window.__flip = true; }).observe(document.body, { childList: true, subtree: true }); });
  await page.locator('[data-nav="today"]').click();
  await expect(page.locator('#viewroot')).toHaveAttribute('data-date', /2026-10/);
  await settle(page);
  expect(await page.evaluate(() => window.__flip)).toBe(false);
});

test('reduced motion: the recurrence strip snaps without a glide, its height and the colour fan do not animate', async ({ page }) => {
  await page.goto('/calendar?view=day&date=2026-10-14');
  await page.locator('article.lnk[data-key$="/standup@seed"] h3').first().click();
  await page.locator('#ed-lock').click();
  const sc = page.locator('#ed-rr .calscroll');
  await sc.waitFor();
  // the harness turns every transition off (lib/test.mjs FREEZE_CSS): the page's own rules are read without it
  const dur = await page.evaluate(() => { document.getElementById('e2e-freeze')?.remove(); const fan = document.createElement('div'); fan.className = 'cpk-card'; document.body.append(fan); const d = [getComputedStyle(document.querySelector('#ed-rr .calscroll')).transitionDuration, getComputedStyle(fan).transitionDuration]; fan.remove(); return d; });
  expect(dur).toEqual(['0s', '0s']);
  // a wheel step under half a month: the strip settles back to its month (strip.js settle) — the 2nd scroll is the snap; a glide moves on after it
  await sc.evaluate(el => { window.__glide = null; let n = 0;
    el.addEventListener('scroll', () => { if (++n !== 2) return; const t = el.scrollTop; requestAnimationFrame(() => requestAnimationFrame(() => { window.__glide = el.scrollTop !== t; })); });
    el.dispatchEvent(new WheelEvent('wheel', { deltaY: 40, bubbles: true, cancelable: true })); });
  await expect.poll(() => page.evaluate(() => window.__glide)).toBe(false);
});

test('the date picker from the keyboard: the title opens it, arrows and PageDown move, Enter picks, Escape closes', async ({ page }) => {
  await page.goto('/calendar?view=month&date=2026-10-14');
  await tabTo(page, '#title');
  await page.keyboard.press('Enter');
  const pk = page.locator('#picker[role=dialog][aria-modal=true]');
  await expect(pk).toBeVisible();
  const day = () => page.evaluate(() => document.activeElement.dataset.date);
  expect(await day()).toBe('2026-10-14');
  await page.keyboard.press('ArrowRight');
  expect(await day()).toBe('2026-10-15');
  await page.keyboard.press('ArrowDown');
  expect(await day()).toBe('2026-10-22');
  await page.keyboard.press('PageDown'); // a month on: the grid turns with it
  expect(await day()).toBe('2026-11-22');
  await expect(page.locator('#pk-title')).toHaveText(/2026/);
  for (let i = 0; i < 4; i++) { await page.keyboard.press('Tab'); expect(await inDialog(page, '#picker')).toBe(true); }
  await page.keyboard.press('Escape');
  await expect(pk).toBeHidden();
  await expect.poll(() => active(page)).toBe('#title');
  await page.keyboard.press('Enter');
  await expect(pk).toBeVisible();
  await page.keyboard.press('ArrowLeft');
  await page.keyboard.press('Enter');
  await expect(page.locator('#viewroot')).toHaveAttribute('data-date', /^2026-10/);
  await expect.poll(() => page.evaluate(() => new URL(location.href).searchParams.get('date'))).toBe('2026-10-13');
});

test('search: the field is a combobox, the pick its active descendant, the rows grouped; Tab leaves the list out', async ({ page }) => {
  await page.goto('/calendar?view=month&date=2026-10-14');
  await settle(page);
  const q = page.locator('#q');
  await q.focus();
  await page.keyboard.type('anna');
  await page.locator('#sres .r').first().waitFor();
  await expect(q).toHaveAttribute('aria-expanded', 'true');
  await page.keyboard.press('ArrowDown');
  const on = page.locator('#sres .r.on');
  await expect(on).toHaveAttribute('aria-selected', 'true');
  await expect(q).toHaveAttribute('aria-activedescendant', await on.getAttribute('id'));
  // a listbox owns options and groups only; each group is named by its category
  expect(await page.evaluate(() => [...document.querySelectorAll('#sres > *')].filter(e => !['group', 'option', null].includes(e.getAttribute('role'))).length)).toBe(0);
  expect(await page.locator('#sres [role=group]').evaluateAll(gs => gs.every(g => (document.getElementById(g.getAttribute('aria-labelledby')) || {}).textContent))).toBe(true);
  await page.keyboard.press('Tab');
  expect(await page.evaluate(() => !!document.activeElement.closest('#sres'))).toBe(false);
});

test('contacts table: its editor dialog is a named modal dialog that keeps the focus', async ({ page }) => {
  await page.goto('/contacts?view=table');
  await page.locator('#contacts.view-table').waitFor();
  await page.locator('#ct-fab').click();
  const det = page.locator('#detail[role=dialog][aria-modal=true]');
  await expect(det).toBeVisible();
  await expect(det).toHaveAttribute('aria-label', /.+/);
  for (let i = 0; i < 12; i++) { await page.keyboard.press('Tab'); expect(await inDialog(page, '#detail'), `Tab ${i + 1} left the dialog`).toBe(true); }
  await page.keyboard.press('Escape');
  await expect(page.locator('#detail[role=dialog]')).toHaveCount(0);
});
