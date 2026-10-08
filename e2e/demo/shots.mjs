// README/gallery screenshots on the showcase DB (e2e/demo): one dev server of this checkout's build (DEV_LOGIN emma,
// DEV_NOW = the frozen Wednesday, egress blocked), chromium 1440×900 @2x, en-GB, Europe/Berlin, light (one dark).
// Every state waits for real readiness (lib/test.mjs settle: no request in flight, the page's due timers ran, fonts
// ready, no running animation, two frames). Output: docs/images/<NN>-<slug>.webp, shots.json;
// the 2x PNGs stay in e2e/.work/demo/png.
// Usage: node e2e/demo/shots.mjs <practical-binary> <demo.sqlite3>   (E2E_PORT = the server's port, default 4101;
// SHOTS=<slug,…> retakes only those, without touching the others or shots.json's order)
import { spawn, execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { chromium } from '@playwright/test';
import { E2E, NOW, REPO, TZ } from '../lib/env.mjs';
import { FREEZE_CSS, initScript, settle } from '../lib/test.mjs';

const [BIN, DB_SRC] = process.argv.slice(2);
if (!BIN || !DB_SRC) throw new Error('usage: shots.mjs <practical-binary> <demo.sqlite3>');
const PORT = Number(process.env.E2E_PORT || 4101);
const BASE = `http://127.0.0.1:${PORT}`;
const RUN = path.join(E2E, '.work', 'demo', 'run');
const PNG = path.join(E2E, '.work', 'demo', 'png');
const IMG = path.join(REPO, 'docs', 'images');
const BLOCK = 'http://127.0.0.1:9';
const TOOLS = '/opt/homebrew/bin';
const tool = n => (fs.existsSync(path.join(TOOLS, n)) ? path.join(TOOLS, n) : n);

// ---------------------------------------------------------------- server (lib/servers.mjs `start`, emma, en)
function startServer() {
  fs.rmSync(RUN, { recursive: true, force: true });
  fs.mkdirSync(RUN, { recursive: true });
  const db = path.join(RUN, 'db.sqlite3');
  fs.copyFileSync(DB_SRC, db);
  const log = path.join(RUN, 'server.log');
  const env = {
    PATH: process.env.PATH, HOME: process.env.HOME, RUST_LOG: 'info,tower_http=warn',
    PRACTICAL__DB_PATH: db, PRACTICAL__RUSTICAL_URL: '', PRACTICAL__BIND: `127.0.0.1:${PORT}`,
    PRACTICAL__PUBLIC_URL: BASE, PRACTICAL__ALLOW_PASSWORD_LOGIN: 'true',
    PRACTICAL__HOLIDAYS_COUNTRY: 'DE', PRACTICAL__HOLIDAYS_SUBDIVISION: 'DE-SN',
    PRACTICAL__PUBLIC_DAV_URL: 'https://dav.example.org', PRACTICAL__DEFAULT_TIMEZONE: TZ, PRACTICAL__DEFAULT_LANG: 'en',
    PRACTICAL__DEV_ASSETS_DIR: `${BIN}.assets`, PRACTICAL__DEV_LOGIN: 'emma', PRACTICAL__DEV_NOW: NOW,
    HTTP_PROXY: BLOCK, HTTPS_PROXY: BLOCK, http_proxy: BLOCK, https_proxy: BLOCK, ALL_PROXY: BLOCK, NO_PROXY: '', no_proxy: '',
  };
  const out = fs.openSync(log, 'w');
  const child = spawn(BIN, ['serve'], { env, stdio: ['ignore', out, out] });
  fs.closeSync(out);
  return { child, log };
}

async function waitHealthy({ child, log }) {
  for (let i = 0; i < 1500; i++) {
    if (child.exitCode !== null) throw new Error(`server exited with ${child.exitCode}, see ${log}`);
    try {
      const r = await fetch(BASE + '/healthz');
      if (r.ok && (await r.text()) === 'ok') break;
    } catch {}
    await new Promise(r => setTimeout(r, 20)); // the connection is refused until the socket listens: a retry, not a fixed wait
  }
  if (!fs.readFileSync(log, 'utf8').includes('development clock')) throw new Error(`${log}: no "development clock" line (wrong build?)`);
}

async function stopServer({ child }) {
  if (child.exitCode !== null || child.signalCode !== null) return;
  const gone = new Promise(r => child.once('exit', r));
  child.kill('SIGINT');
  const t = setTimeout(() => child.kill('SIGKILL'), 5000);
  await gone;
  clearTimeout(t);
}

// ---------------------------------------------------------------- browser page (lib/test.mjs `page` fixture)
async function newPage(browser, scheme) {
  const ctx = await browser.newContext({
    baseURL: BASE, viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2, locale: 'en-GB', timezoneId: TZ,
    colorScheme: scheme, reducedMotion: 'reduce', serviceWorkers: 'block', extraHTTPHeaders: { 'Accept-Language': 'en' },
  });
  const page = await ctx.newPage();
  const inflight = new Set();
  const long = u => /\/api\/changes|\/dev\/reload/.test(u);
  let nav = null;
  page.on('request', r => {
    if (r.isNavigationRequest() && r.frame() === page.mainFrame()) { inflight.clear(); nav = r; }
    if (!long(r.url())) { inflight.add(r); page.__requests++; }
  });
  page.on('framenavigated', f => {
    if (f !== page.mainFrame() || !nav) return;
    for (const r of inflight) if (r !== nav) inflight.delete(r);
    nav = null;
  });
  const done = r => { inflight.delete(r); if (r === nav) nav = null; };
  page.on('requestfinished', done);
  page.on('requestfailed', done);
  page.__inflight = inflight;
  page.__requests = 0;
  page.__errors = [];
  page.on('pageerror', e => page.__errors.push(String(e)));
  page.on('dialog', d => { page.__errors.push(`dialog: ${d.message()}`); d.dismiss().catch(() => {}); });
  await page.clock.setFixedTime(new Date(NOW));
  await page.addInitScript(initScript, [FREEZE_CSS]);
  // nothing leaves 127.0.0.1; and the showcase writes nothing (a write would change the next shot)
  await page.route(u => !u.href.startsWith(BASE) && !u.href.startsWith('data:') && !u.href.startsWith('blob:'), r => r.abort('blockedbyclient'));
  await page.route(u => u.href.startsWith(BASE), r => {
    const q = r.request();
    if (q.method() === 'GET' || q.method() === 'HEAD') return r.fallback();
    page.__errors.push(`write blocked: ${q.method()} ${q.url()}`);
    return r.abort('blockedbyclient');
  });
  return page;
}

// ---------------------------------------------------------------- states
const D = '2026-10-14';
const cal = (view, date = D) => `/calendar?view=${view}&date=${date}`;
const park = page => page.mouse.move(720, 899); // bottom edge of the viewport: no hover, no tooltip
async function calView(page, view, date) {
  await page.goto(cal(view, date));
  await page.locator('#viewroot').waitFor();
}
async function tkView(page, v, back = 'all') {
  await page.goto(`/tasks?view=${v}&back=${back}`);
  await page.locator(`#tk-views [data-tkview="${v}"][aria-pressed=true]`).waitFor();
  await page.locator('#tkmain > *').first().waitFor();
}
async function openEvent(page, title, date) {
  await page.goto(cal('day', date));
  await page.locator(`article.lnk[data-date="${date}"] h3`, { hasText: title }).first().click();
  await page.locator('#ed:not([hidden])').waitFor();
}
async function openTask(page, title) {
  await tkView(page, 'groups', 'month');
  if (title === null) return;
  await page.locator('#tkmain [data-key]').filter({ hasText: title }).first().click();
  await page.locator('#edform.mode-view').waitFor();
}
// a deep link: the list renders its letter panels on demand, a later letter's contact is not in the first document
async function ctList(page) {
  await page.goto('/contacts');
  const list = page.locator('[data-ctview=list]');
  if ((await list.getAttribute('aria-pressed')) !== 'true') await list.click(); // the view choice is remembered
  await page.locator('.list li[data-ab]').first().waitFor();
}
async function openContact(page, uid, name) {
  await ctList(page);
  const ab = await page.locator('.list li[data-ab]').first().getAttribute('data-ab');
  await page.goto('about:blank'); // a new document: the page reads the hash on load
  await page.goto(`/contacts#${ab}/${uid}`);
  await page.waitForFunction(n => document.querySelector('#detail #ctform input[name=full_name]')?.value === n, name);
}
// the list scrolls inside main.main under the sticky toolbar: the letter's panel goes right below the toolbar
const letterTop = (page, l) => page.locator(`.lpanel[data-letter="${l}"]`).evaluate(el => {
  const m = document.querySelector('main.main'), tb = document.querySelector('.toolbar');
  m.scrollTop += el.getBoundingClientRect().top - tb.getBoundingClientRect().bottom - 8;
});
async function typeQ(page, v) {
  const q = page.locator('#q');
  await q.click();
  await q.fill(v);
  await page.waitForFunction(x => { const b = document.getElementById('sres'); return !b.hidden && (b.querySelector('.r.more') || b.querySelector('.none')) && document.getElementById('q').value === x; }, v);
}

const SHOTS = [
  ['day', 'Day view', 'The day view: today\'s agenda with overlapping meetings, a timeline of the day and the month at a glance.', async p => {
    await calView(p, 'day'); await p.locator(`.doc[data-date="${D}"]`).waitFor(); }],
  ['week', 'Week view', 'A busy week with overlapping meetings, recurring stand-ups, sports and family events side by side.', async p => {
    await calView(p, 'week'); await p.locator('.stream .week.focus').waitFor(); }],
  ['month', 'Month view', 'October at a glance: work, personal, sports and the shared family calendar, holidays and an all-day trip.', async p => {
    await calView(p, 'month'); }],
  ['year-planner', 'Year planner', 'The whole year on one page: public and school holidays, vacations and the first event of every day.', async p => {
    await calView(p, 'planner'); await p.locator('.planner').waitFor(); }],
  ['free-weekends', 'Free weekends', 'Every weekend of the year, free or planned, with long weekends extended by a public holiday.', async p => {
    await calView(p, 'weekends'); }],
  ['vacation-planner', 'Vacation planner', 'Vacations against school and public holidays, with planned and remaining vacation days.', async p => {
    await calView(p, 'vacation'); }],
  ['countdown', 'Countdown wall', 'Days until the next birthdays, vacations and holidays.', async p => {
    await calView(p, 'countdown'); }],
  ['year-in-numbers', 'Year in numbers', 'Statistics of the year: events per calendar, load per week, busy evenings and weekends.', async p => {
    await calView(p, 'numbers'); }],
  ['tasks-list', 'Task list', 'Tasks grouped by due date, with priorities, progress, categories and the task editor docked beside the list.', async p => {
    await openTask(p, "Organise Lily's birthday party"); }],
  ['tasks-table', 'Task table', 'All tasks as a sortable table with configurable columns.', async p => {
    await tkView(p, 'table', 'month'); }],
  ['tasks-board', 'Task board', 'A kanban board: tasks in columns by status.', async p => {
    await tkView(p, 'board', 'month'); }],
  ['tasks-timeline', 'Task timeline', 'Tasks with a start and a due date as bars on a timeline.', async p => {
    await tkView(p, 'timeline', 'month'); }],
  ['tasks-deck', 'Task deck', 'One task at a time as a card deck, worked through with the keyboard.', async p => {
    await tkView(p, 'deck', 'month'); }],
  ['tasks-done', 'Completed tasks', 'What got done: completed tasks by day with a heat map.', async p => {
    await tkView(p, 'history', 'year'); }],
  ['contacts-list', 'Contacts', 'The address book as a list with a letter rail, the selected contact beside it.', async p => {
    await openContact(p, 'c-david@demo', 'David Kim');
    await p.evaluate(() => { document.querySelector('main.main').scrollTop = 0; }); }],
  ['contacts-table', 'Contacts table', 'The address book as a table with phone, email, categories and birthdays.', async p => {
    await ctList(p);
    await p.click('[data-ctview=table]'); await p.locator('#contacts.view-table').waitFor(); }],
  ['event-view', 'Event (view mode)', 'An event opened read-only: time, location, attendees, video call, reminders and the agenda.', async p => {
    await openEvent(p, 'Q4 roadmap workshop', D); }],
  ['event-edit', 'Event (edit mode)', 'The same event in edit mode: every property as an editable card.', async p => {
    await openEvent(p, 'Q4 roadmap workshop', D);
    await p.locator('#ed-lock').click(); await p.locator('#edform.mode-edit').waitFor(); await p.locator('#edh').blur(); }],
  ['task-view', 'Task (view mode)', 'A task opened read-only: due date, progress, priority, categories, notes and its link to an event.', async p => {
    await openTask(p, 'Prepare Q4 roadmap slides'); }],
  ['task-edit', 'Task (edit mode)', 'The same task in edit mode.', async p => {
    await openTask(p, 'Prepare Q4 roadmap slides');
    await p.click('#ed-lock'); await p.locator('#edform.mode-edit').waitFor(); await p.locator('#edh').blur(); }],
  ['contact-view', 'Contact (view mode)', 'A contact opened read-only: phones, emails, messenger, organisation, birthday and relationships.', async p => {
    await openContact(p, 'c-priya@demo', 'Priya Sharma'); await letterTop(p, 'P'); }],
  ['contact-edit', 'Contact (edit mode)', 'The same contact in edit mode.', async p => {
    await openContact(p, 'c-priya@demo', 'Priya Sharma'); await letterTop(p, 'P');
    await p.click('#ct-lock'); await p.locator('#ctform.mode-edit').waitFor(); }],
  ['colours', 'Colour manager', 'Every colour in use: calendars, holidays, category rules, and events and tasks with a colour of their own.', async p => {
    await p.goto('/settings#colors'); await p.locator('#colors-list .cfo-opt').first().waitFor(); }],
  ['colour-optimiser', 'Colour optimiser', 'Suggestions to merge near-duplicate colours and pull apart colours that look alike, as a flow from element to new colour.', async p => {
    await p.goto('/settings#colors'); await p.locator('#colors-list .cfo-opt[data-opt="event"]').click();
    await p.locator('.cfo-wrap .cfo-body .cfo-level').waitFor(); }],
  ['search', 'Universal search', 'One search box for events, contacts and tasks, with results from all three.', async p => {
    await p.goto(cal('month')); await typeQ(p, 'lily'); }],
  ['month-dark', 'Month view (dark)', 'The month view in dark mode.', async p => {
    await calView(p, 'month'); }, 'dark'],
];

// ---------------------------------------------------------------- main
const only = process.env.SHOTS ? new Set(process.env.SHOTS.split(',')) : null;
// a full run owns docs/images/NN-*.webp: names of former runs go (a renamed or dropped shot leaves nothing behind)
if (!only) for (const d of [IMG, PNG]) if (fs.existsSync(d)) for (const f of fs.readdirSync(d)) if (/^\d\d-.*\.(webp|png)$/.test(f)) fs.rmSync(path.join(d, f));
fs.mkdirSync(PNG, { recursive: true });
const server = startServer();
let browser;
try {
  await waitHealthy(server);
  browser = await chromium.launch({ args: ['--disable-gpu', '--disable-partial-raster', '--lang=en-GB'] });
  const pages = {};
  const meta = [];
  let n = 0;
  for (const [slug, title, caption, go, scheme = 'light'] of SHOTS) {
    const file = `${String(++n).padStart(2, '0')}-${slug}.webp`;
    meta.push({ file, title, caption });
    if (only && !only.has(slug)) continue;
    const page = pages[scheme] ||= await newPage(browser, scheme);
    page.__errors.length = 0;
    await go(page);
    await park(page);
    await settle(page);
    // no focus ring in the picture: whatever kept focus loses it (the page's own blur handlers run, settle again)
    await page.evaluate(() => { const a = document.activeElement; if (a && a !== document.body && a.id !== 'edh' && a.id !== 'q') a.blur(); });
    await settle(page);
    const bad = await page.evaluate(() => [...document.querySelectorAll('.toast')].filter(t => t.offsetParent).map(t => t.textContent.trim()));
    if (bad.length) page.__errors.push(`toast: ${bad.join(' | ')}`);
    if (page.__errors.length) throw new Error(`${slug}: ${page.__errors.join('; ')}`);
    const png = path.join(PNG, file.replace(/\.webp$/, '.png'));
    await page.screenshot({ path: png, animations: 'disabled', caret: 'hide', scale: 'device' });
    // q 82; a picture dense enough to exceed 250 KB (the year planner) is encoded to that size instead
    const webp = path.join(IMG, file);
    execFileSync(tool('cwebp'), ['-quiet', '-q', '82', '-m', '6', '-sharp_yuv', png, '-o', webp]);
    if (fs.statSync(webp).size > 250_000) execFileSync(tool('cwebp'), ['-quiet', '-size', '245000', '-pass', '6', '-m', '6', '-sharp_yuv', png, '-o', webp]);
    console.log(`${file}  ${(fs.statSync(path.join(IMG, file)).size / 1024).toFixed(0)} KB`);
  }
  fs.writeFileSync(path.join(E2E, 'demo', 'shots.json'), JSON.stringify(meta, null, 2) + '\n');
} finally {
  if (browser) await browser.close();
  await stopServer(server);
  fs.rmSync(RUN, { recursive: true, force: true });
}
