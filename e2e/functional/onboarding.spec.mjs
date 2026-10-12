// First start (domain::onboarding): the language gate, the hints leading click by click to the real
// setting, the free order, and the end.
//
// The fixture is through the setup, so every test asks for it again — `reopen` forgets the
// completion and keeps the confirmations. The three users differ, and the tests use that:
//   alice  collections, named regions and a stored zone → only the language is open
//   bob    nothing of it                                → every point is open
//   carol  the same, used where a test has to WRITE
// A spec file gets fresh fixture copies, but its tests share one DB. A preference can be written
// back over HTTP; a confirmation cannot, so whoever confirms runs after whoever needs that point
// open. The 24 h nap has no button any more (nothing but the hints is left), so the clock side of
// it lives in src/it/onboarding.rs, which can move time.
import { test, expect, settle, loginAs, api } from '../lib/test.mjs';
import { BASE } from '../lib/env.mjs';

const post = async (page, path, form) => {
  const csrf = await page.locator('meta[name=csrf]').getAttribute('content');
  const r = await page.request.post(path, { form: { csrf, ...form }, headers: { Origin: BASE }, maxRedirects: 0 });
  expect([200, 204, 302, 303]).toContain(r.status());
};
const reopen = page => post(page, '/settings/onboarding/reopen', {});
/** Back to "through" where the server allows it: it refuses (409) while a point is open, and a user
 *  left open is no problem — every test reopens first, and each spec file gets a fresh fixture. */
const finish = async (page) => {
  const csrf = await page.locator('meta[name=csrf]').getAttribute('content');
  const r = await page.request.post('/settings/onboarding/complete', { headers: { 'X-CSRF-Token': csrf, Origin: BASE } });
  expect([204, 409]).toContain(r.status());
};
const confirmLang = (page, back) => post(page, '/settings/onboarding/lang', { lang: 'de', back });
const confirmStep = (page, step) => api(page, 'POST', '/settings/onboarding/confirm', { step });

/** The settings form writes every field it carries, so a test can hand a preference back. */
const savePrefs = (page, fields) => post(page, '/settings/prefs', {
  lang: 'de', scheme: 'auto', timezone: '', holiday_color: '', school_color: '',
  public_regions: '', school_regions: '', vacation_days: '30', vacation_kind: 'category',
  vacation_category: 'Urlaub', vacation_calendar: '', vacation_title: '',
  alarm_snaps_start: '5 min', alarm_snaps_end: '', search_history_days: '90', ...fields,
});

/** A user with everything still to do, standing on the settings page with the first hint up. */
async function asFresh(page, who = 'bob') {
  await loginAs(page, who);
  await page.goto('/settings');
  await reopen(page);
  await confirmLang(page, '/settings');
  await page.goto('/settings');
  await page.locator('.onb-tip').waitFor();
}
const andBack = async page => { await finish(page); await loginAs(page, 'alice'); };

/* The island is the state at page load and stays as it is after an in-page confirm — the hints keep
   the fresh one in memory and the next page load re-reads it. `open` asks the live state,
   `openIsland` the embedded one. */
const open = page => page.evaluate(() => window.onb.view.open);
const openIsland = page => page.evaluate(() => JSON.parse(document.getElementById('onboarding').textContent).open);
const aims = page => page.$$eval('.onb-aim', els => els.map(e => e.getAttribute('href') || e.textContent.trim().slice(0, 20)));
const arrows = page => page.$$eval('.onb-arrows polygon', els => els.length);
const head = page => page.locator('.onb-tip .onb-count');
const hash = (page, h) => page.evaluate(x => { location.hash = x; }, h);

test.beforeEach(async ({ page }) => { await page.goto('/settings'); await reopen(page); });
test.afterEach(async ({ page }) => { await page.goto('/settings'); await finish(page); });

// FIRST: this confirms alice's language, and that cannot be taken back
test('the language gate comes first, switches on click, and only its button ends it', async ({ page }) => {
  await page.goto('/calendar');
  await page.locator('#onb-gate').waitFor();
  // the gate is the page's own form: it works without JavaScript and carries the four languages
  await expect(page.locator('#onb-gate form')).toHaveAttribute('action', '/settings/onboarding/lang');
  expect(await page.$$eval('#onb-gate .onb-lang', els => els.length)).toBe(4);
  expect(await page.$$eval('#onb-gate .onb-flag', els => els.map(e => e.textContent).join(''))).toBe('🇩🇪🇬🇧🇫🇷🇪🇸');
  expect(await page.locator('.onb-tip').count(), 'nothing runs ahead of the gate').toBe(0);

  // picking a language shows the gate IN it, so the user can read what they are choosing — and that
  // is not yet the decision: the gate stays and the point stays open
  await page.locator('#onb-gate label.onb-lang:has(input[value=en])').click();
  await page.locator('#onb-gate').waitFor();
  await expect(page.locator('#onb-gate button[type=submit]')).toHaveText('Continue');
  expect(await openIsland(page), 'trying a language on is no answer').toContain('lang');

  await page.locator('#onb-gate label.onb-lang:has(input[value=de])').click();
  await page.locator('#onb-gate button[type=submit]').click();
  await settle(page);
  expect(await openIsland(page)).not.toContain('lang');   // the gate's form reloads the page
  expect(await page.locator('#onb-gate').count()).toBe(0);
});

test('a hint leads click by click from the calendar to the real setting', async ({ page }) => {
  await loginAs(page, 'bob');
  await page.goto('/settings');
  await reopen(page);
  await confirmLang(page, '/calendar');
  await page.goto('/calendar');

  // on the calendar the only way on is the settings button of the top bar
  await page.locator('.onb-tip').waitFor();
  expect(await aims(page)).toEqual(['/settings']);
  expect(await arrows(page)).toBe(1);
  await expect(head(page), 'this bubble is about no single setting').toHaveText('Erste Schritte');

  await page.locator('.topbar .nav a[href="/settings"]').click();
  await settle(page);
  // the hint survives the page change and now names the row where the time zone lives
  await page.locator('.onb-tip').waitFor();
  expect(await aims(page)).toEqual(['#pick-tz']);
  await expect(head(page)).toHaveText('Zeitzone');
  await andBack(page);
});

/* A user in the first start owns nothing, and that is a state the rest of the app has to survive:
   contacts.js read the first option of an EMPTY address-book picker and threw, which killed the
   rest of app.js with it — including these hints (found 2026-10-09). It has to be carol: bob is in
   a group and therefore SEES an address book, which fills the picker and hides the fault. */
test('the pages of a user who owns nothing still work, hints and all', async ({ page }) => {
  await asFresh(page, 'carol');
  for (const path of ['/contacts', '/calendar', '/tasks']) {
    await page.goto(path);
    await page.locator('.onb-tip').waitFor();
    expect(await aims(page), `no hint on ${path}`).toEqual(['/settings']);
  }
  await andBack(page);
});

test('a bubble carries words and nothing else, and wears a red edge', async ({ page }) => {
  await asFresh(page);
  // no overview and no skip: there is nowhere else to go, so a bubble has no buttons
  expect(await page.locator('.onb-tip .btn').count()).toBe(0);
  const edge = await page.$eval('.onb-tip', e => {
    // var(--err) resolved the way the border resolves it, so the two are comparable
    const probe = e.appendChild(document.createElement('span'));
    probe.style.color = 'var(--err)';
    const err = getComputedStyle(probe).color;
    probe.remove();
    const after = getComputedStyle(e, '::after');
    /* The halo pulses on its own layer and only its OPACITY moves: animating the box itself would
       repaint the bubble's text on every frame. The whole suite runs with reduced motion, so the
       computed animation is off here — that it is DECLARED is read from the stylesheet. */
    const rules = [...document.styleSheets].flatMap(ss => { try { return [...ss.cssRules]; } catch { return []; } });
    const pulse = rules.some(r => r.selectorText === '.onb-tip::after' && /onb-edge/.test(r.style.animation));
    return { border: getComputedStyle(e).borderTopColor, err, halo: after.boxShadow, anim: after.animationName, pulse };
  });
  expect(edge.border, 'the edge must be the error colour, so it reads as "act here"').toBe(edge.err);
  expect(edge.halo, 'the halo is what pulses').not.toBe('none');
  expect(edge.pulse, 'the pulse must be declared on the halo, not on the box').toBe(true);
  expect(edge.anim, 'and it must stop for a user who asked for less motion').toBe('none');
  await andBack(page);
});

test('an arrow stops short of what it points at, never inside it', async ({ page }) => {
  await asFresh(page);
  const { tip, box } = await page.evaluate(() => {
    // the arrowhead's first point is its tip (guide.js place())
    const [x, y] = document.querySelector('.onb-arrows polygon').getAttribute('points').split(' ')[0].split(',').map(Number);
    const r = document.querySelector('.onb-aim').getBoundingClientRect();
    return { tip: { x, y }, box: { left: r.left, right: r.right, top: r.top, bottom: r.bottom } };
  });
  const inside = tip.x > box.left && tip.x < box.right && tip.y > box.top && tip.y < box.bottom;
  expect(inside, `the tip at ${tip.x},${tip.y} sits inside ${JSON.stringify(box)}`).toBe(false);
  // and close enough to still mean that element
  expect(Math.min(Math.abs(tip.x - box.left), Math.abs(tip.x - box.right))).toBeLessThan(12);
  await andBack(page);
});

test('the ring around a small target sits outside it, not over its text', async ({ page }) => {
  await asFresh(page);
  // a settings page of no open point: the way on is the back link, and its ring must not eat the text
  await hash(page, '#account');
  await settle(page);
  const back = page.locator('.set .page.on .back.onb-aim');
  await back.waitFor();
  const inset = await back.evaluate(e => getComputedStyle(e, '::after').inset);
  expect(inset, 'an inset ring would sit on the link text').toMatch(/-/);
  await andBack(page);
});

test('a collection offers every way at once: one arrow, one ring around them all', async ({ page }) => {
  await asFresh(page);
  await hash(page, '#collections');
  await settle(page);
  await expect(head(page)).toHaveText('Kalender');
  // the two remote rows only exist where a server is configured, which the e2e build is not
  expect(await aims(page)).toEqual(['#new-calendar', '#import-calendar']);
  // ONE arrow, however many ways: any one of them is enough, so a fan of arrows would read as a
  // list of things to do
  expect(await arrows(page), 'one arrow for the group').toBe(1);
  // and ONE ring, around all of them, instead of a ring per row
  const ring = page.locator('.onb-ring');
  await expect(ring).toBeVisible();
  const { r, rows } = await page.evaluate(() => ({
    r: document.querySelector('.onb-ring').getBoundingClientRect().toJSON(),
    rows: [...document.querySelectorAll('.onb-aim')].map(e => e.getBoundingClientRect().toJSON()),
  }));
  for (const [i, b] of rows.entries()) {
    expect(r.left, `row ${i} must sit inside the ring`).toBeLessThanOrEqual(b.left);
    expect(r.right).toBeGreaterThanOrEqual(b.right);
    expect(r.top).toBeLessThanOrEqual(b.top);
    expect(r.bottom).toBeGreaterThanOrEqual(b.bottom);
  }
  /* And it follows the rows when they move. The ring is a body-level `fixed` box, while the
     scroller is `.main` and not the document (frame.css) — so `scrollY` stays 0 and document
     coordinates would not help: measured, a body-level ABSOLUTE box sat 197 px away after scrolling
     `.main` by 197. `guide.js` re-places it with the bubble on every scroll. */
  // a short window, so the page has somewhere to scroll at all (it is still the desktop layout)
  await page.setViewportSize({ width: 1280, height: 420 });
  await settle(page);
  const moved = await page.evaluate(async () => {
    const main = document.querySelector('.main');
    const row = () => document.querySelector('.onb-aim').getBoundingClientRect().top;
    const ring = () => document.querySelector('.onb-ring').getBoundingClientRect().top;
    const gapBefore = Math.round(ring() - row());
    main.scrollTop = Math.min(120, main.scrollHeight - main.clientHeight);
    // the scroll event arrives asynchronously and the handler is frame-gated
    await new Promise(r => setTimeout(r, 250));
    return { scrolled: Math.round(main.scrollTop), docScroll: scrollY, gapBefore, gapAfter: Math.round(ring() - row()) };
  });
  expect(moved.scrolled, 'the page has to scroll for this to mean anything').toBeGreaterThan(0);
  expect(moved.docScroll, 'and it is .main that scrolls, not the document').toBe(0);
  expect(moved.gapAfter, 'the ring keeps its distance to the rows after a scroll').toBe(moved.gapBefore);
  await page.setViewportSize({ width: 1280, height: 900 });
  await settle(page);
  // the rows keep their tint but lose their own ring, or there would be one ring too many
  const own = await page.$$eval('.onb-aim', els => els.map(e => getComputedStyle(e, '::after').content));
  expect(own.every(c => c === 'none'), `a grouped row still draws its own ring: ${own}`).toBe(true);
  await expect(page.locator('.onb-tip .onb-either')).toBeVisible();
  await andBack(page);
});

test('the holidays are two points; each is silent while choosing and names its own button', async ({ page }) => {
  await asFresh(page);
  expect(await open(page)).toEqual(['tz', 'regions', 'school', 'calendar', 'addressbook']);
  for (const [h, title, add] of [['#regions-public', 'Feiertage', '#pick-country-public'], ['#regions-school', 'Schulferien', '#pick-country-school']]) {
    await hash(page, h);
    await settle(page);
    // on the list page the way on is its "add a region" button, which is where several are offered
    await expect(head(page)).toHaveText(title);
    expect(await aims(page)).toEqual([add]);
    await expect(page.locator('.onb-tip .onb-body')).toContainText('mehrere');
  }
  // inside the pickers the hint says nothing: the user is choosing there
  for (const h of ['#pick-country-public', '#pick-region-public-DE', '#pick-country-school']) {
    await hash(page, h);
    await settle(page);
    expect(await page.locator('.onb-tip').count(), `a hint showed up on ${h}`).toBe(0);
  }
  await andBack(page);
});

test('the order is free: walking into another point adopts it', async ({ page }) => {
  await asFresh(page);
  await hash(page, '#pick-tz');
  await settle(page);
  await expect(head(page)).toHaveText('Zeitzone');
  await hash(page, '#collections');
  await settle(page);
  // no "go back up": the hint switched to the point whose page the user is on
  await expect(head(page)).toHaveText('Kalender');
  // a settings page belonging to no open point does send the user up: nothing is to be done there
  await hash(page, '#account');
  await settle(page);
  expect(await aims(page)).toEqual(['#']);
  await andBack(page);
});

test('every hint has a drawn icon on a coloured tile', async ({ page }) => {
  await asFresh(page);
  // a colour that does not resolve leaves the tile blank: that is how a point once lost its own
  // (a second colour table in guide.js had no entry for it)
  for (const h of ['', '#pick-tz', '#regions-public', '#regions-school', '#collections']) {
    await hash(page, h || '');
    await settle(page);
    const tile = await page.$eval('.onb-tip .onb-ic', e => ({
      bg: getComputedStyle(e).backgroundColor, icon: !!e.querySelector('svg use'),
    }));
    const who = await head(page).textContent();
    expect(tile.icon, who).toBe(true);
    expect(tile.bg, who).not.toBe('rgba(0, 0, 0, 0)');
  }
  await andBack(page);
});

test('the create page suggests a name and hands the whole word to the first keystroke', async ({ page }) => {
  await asFresh(page);
  await page.goto('/settings#new-calendar');
  const name = page.locator('#new-calendar input[name=name]');
  await expect(name).toHaveValue('Mein Kalender');
  await name.click();
  expect(await name.evaluate(e => e.value.slice(e.selectionStart, e.selectionEnd))).toBe('Mein Kalender');
  await expect(page.locator('#new-addressbook input[name=name]')).toHaveValue('Meine Kontakte');
  // the import pages take the name from the file, so they suggest nothing
  await expect(page.locator('#import-calendar input[name=name]')).toHaveValue('');
  // and a user whose first start is through is not told what to call the next one
  await loginAs(page, 'alice');
  await page.goto('/settings#new-calendar');
  await page.reload();   // same URL as above: without this the browser keeps the old document
  await expect(page.locator('#new-calendar input[name=name]')).toHaveValue('');
});

// alice: her language is answered by the first test, and the fixture gave her everything else, so
// she is the user the server lets through
/* Once connected, the address is done: the hint leaves it for the list of the server's collections
   and the button that starts the batch. The two sync pages take the same branch (steps.js, `k`) but
   only exist with PRACTICAL__REMOTE_SECRET_KEY, which the e2e servers do not set: a key would add
   the sync row every collection test counts. Connecting happens without a
   page load or a hash change, so this is also the test that the hint hears of it at all (it stayed
   on the address before). The two targets are one instruction in sequence, so the "take any one of
   them" line must not appear. No Nextcloud: the sign-in is reused and the list is answered here. */
test('connected to a server, the hint points at the collections and the button that starts', async ({ page }) => {
  await asFresh(page);
  await page.route('**/settings/remote/reuse', r => r.fulfill({ json: { handle: 'h' } }));
  await page.route('**/settings/remote/collections', r => r.fulfill({ json: {
    calendars: [{ name: 'Familie', color: '#ff9500', href: '/c/1/', writable: true }, { name: 'Arbeit', href: '/c/2/', writable: true }],
    addressbooks: [{ name: 'Kontakte', href: '/a/1/', writable: true }],
  } }));
  const cases = [
    ['#remote-cal', 'Kalender', 'Einmalig importieren', 'Wähle die Kalender und starte den Import'],
    ['#remote-ab', 'Adressbuch', 'Einmalig importieren', 'Wähle die Adressbücher und starte den Import'],
  ];
  for (const [page_, step, button, lead] of cases) {
    await hash(page, page_);
    await settle(page);
    await expect(head(page)).toHaveText(step);
    // before: the address is what is missing
    expect(await page.$$eval('.onb-aim', els => els.map(e => e.className)), page_).toEqual([expect.stringContaining('rm-in')]);
    await page.locator(`${page_} .rm-origin`).fill('nextcloud.example.com');
    await page.locator(`${page_} .rm-go`).click();
    await expect(page.locator(`${page_} .rm-list .rm-pick`).first()).toBeVisible();
    await expect(page.locator(`${page_} .rm-take.onb-aim`)).toBeVisible();
    // after: the list and the button, ringed together, under one arrow
    const aimed = await page.$$eval('.onb-aim', els => els.map(e => (e.classList.contains('rm-list') ? 'list' : e.classList.contains('rm-take') ? e.textContent.trim() : e.className)));
    expect(aimed, page_).toEqual(['list', button]);
    await expect(page.locator('.onb-ring')).toBeVisible();
    expect(await arrows(page)).toBe(1);
    await expect(page.locator('.onb-tip .onb-lead')).toContainText(lead);
    await expect(page.locator('.onb-tip .onb-body')).toContainText(button);
    await expect(page.locator('.onb-tip .onb-either'), 'choose, then start: not alternatives').toHaveCount(0);
  }
  await page.unroute('**/settings/remote/reuse');
  await page.unroute('**/settings/remote/collections');
  await andBack(page);
});

test('the settings bring the hints back after they were through', async ({ page }) => {
  await page.goto('/settings');
  expect(await open(page)).toEqual([]);
  await finish(page);
  await page.goto('/settings');
  expect(await page.locator('#onboarding').count(), 'a finished setup costs the page nothing').toBe(0);
  await page.locator('button[form=onb-reopen]').click();
  await settle(page);
  expect(await page.locator('#onboarding').count()).toBe(1);
});

// ---- these WRITE, so they come last and use the user nobody above needs untouched ---------------

test('picking a city answers the time zone, and the hint moves on without a reload', async ({ page }) => {
  await asFresh(page, 'carol');
  await hash(page, '#pick-tz');
  await settle(page);
  /* The picker saves without a page load, so the hints have to ask the server again — on the SAVED
     event, not on the hash change: the hash empties the moment the picker closes, which is before
     the post lands, so a hint that listened only to that read the old state (found 2026-10-09). */
  await page.evaluate(() => {
    window.saved = 0;
    document.addEventListener('practical:prefs-saved', () => { window.saved++; });
  });
  await page.locator('#pick-list button[data-v="Africa/Accra"]').click();
  await expect.poll(() => page.evaluate(() => window.saved), { message: 'the save must announce itself' }).toBeGreaterThan(0);
  await expect.poll(() => open(page).catch(() => null)).not.toContain('tz');
  await expect(page.locator('#v-tz')).toHaveText('Africa/Accra');
  // the zone itself is the answer: no mark was needed for it
  expect(await api(page, 'GET', '/api/onboarding')).toMatchObject({ open: ['regions', 'school', 'calendar', 'addressbook'] });
  await savePrefs(page, { timezone: '' });   // hand it back for the next test
  await andBack(page);
});

test('the first region named fills the other list once, and never again', async ({ page }) => {
  await loginAs(page, 'carol');
  await page.goto('/settings');
  await savePrefs(page, { public_regions: 'DE-SN' });
  const lists = async () => page.evaluate(() => ['public', 'school'].map(k => document.querySelector(`input[name=${k}_regions]`).value));
  await page.goto('/settings');
  expect(await lists(), 'one region usually answers both').toEqual(['DE-SN', 'DE-SN']);
  // changing the one the user named leaves the other alone: from now on they are independent
  await savePrefs(page, { public_regions: 'DE-SN,DE-BW', school_regions: 'DE-SN' });
  await page.goto('/settings');
  expect(await lists()).toEqual(['DE-SN,DE-BW', 'DE-SN']);
  /* And emptying one again keeps it empty. This is the case a "both empty now" test would miss: any
     later save of any unrelated field would refill it from the other list (audit, 2026-10-09). */
  await savePrefs(page, { public_regions: 'DE-SN,DE-BW', school_regions: '' });
  await savePrefs(page, { public_regions: 'DE-SN,DE-BW', school_regions: '', scheme: 'dark' });
  await page.goto('/settings');
  expect(await lists(), 'an unrelated save must not bring the region back').toEqual(['DE-SN,DE-BW', '']);
  await savePrefs(page, { public_regions: '', school_regions: '' });
  await andBack(page);
});

test('taking the proposed zone is an answer too, and the last point closes the setup', async ({ page }) => {
  await asFresh(page, 'carol');
  // the picker's first row stores an EMPTY zone, so only the mark can say the user decided
  await hash(page, '#pick-tz');
  await settle(page);
  await page.locator('#pick-list button[data-v=""]').click();
  await expect.poll(() => open(page).catch(() => null)).not.toContain('tz');
  await expect(page.locator('#v-tz'), 'the row still follows the default').toContainText('Europe/Berlin');

  // declining the holidays needs no value either
  for (const step of ['regions', 'school']) await confirmStep(page, step);
  for (const kind of ['calendar', 'addressbook']) {
    await page.goto(`/settings#new-${kind}`);
    await page.locator(`#new-${kind} form button[type=submit]`).click();
    await settle(page);
  }
  // the last point was answered while the hints were up, so the setup says so, once
  await page.locator('#onb-done').waitFor();
  await expect(page.locator('#onb-done .onb-h')).toHaveText('Fertig eingerichtet');
  expect(await page.locator('.onb-tip').count()).toBe(0);
  await page.locator('#onb-done [data-close]').click();
  await expect(page.locator('#onb-done')).toHaveCount(0);
  // and the server was told, so the next page load costs nothing
  await page.goto('/settings');
  expect(await page.locator('#onboarding').count()).toBe(0);
  await loginAs(page, 'alice');
});
