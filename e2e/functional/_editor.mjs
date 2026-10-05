// Helpers of the editor/colour functional specs (F2). Everything created here carries the title prefix "F2 ".
import { api, expect, settle } from '../lib/test.mjs';

export const CAL = { work: 'alice/5eed0000-0000-4000-8000-000000000001', privat: 'alice/5eed0000-0000-4000-8000-000000000002' };
export const next = d => { const t = new Date(d + 'T12:00:00Z'); t.setUTCDate(t.getUTCDate() + 1); return t.toISOString().slice(0, 10); };

/** Instances of `date` (API) whose summary is `summary`. */
export async function instances(page, date, summary) {
  const all = await api(page, 'GET', `/api/events?start=${date}&end=${next(date)}`);
  return all.filter(e => e.summary === summary);
}
export const keyOf = e => `${e.principal}/${e.cal_id}/${e.object_id}`;

/** The one instance of `summary` on `date` (fails when absent or ambiguous). */
export async function one(page, date, summary) {
  const hits = await instances(page, date, summary);
  expect(hits, `${summary} on ${date}`).toHaveLength(1);
  return hits[0];
}

/** New event through a month day's add button (desktop). */
export async function openNew(page, date) {
  await page.goto(`/calendar?view=month&date=${date}`);
  const day = page.locator(`.v-month .day[data-date="${date}"]`);
  await day.hover();
  await day.locator('.add').click();
  await page.locator('#ed:not([hidden])').waitFor();
  await expect(page.locator('#edform')).toHaveClass(/mode-edit/);
}

/** Fills the when card of an open editor. */
export async function fillWhen(page, { title, date, from, to, cal, allDay }) {
  if (title != null) await page.locator('#edh').fill(title);
  if (date) await page.locator('#edform input[name="date"]').fill(date);
  if (cal) await page.locator('#ed-cal').selectOption(cal);
  if (allDay) await page.locator('#ed-allday').click();
  if (from) await page.locator('#edform input[name="from"]').fill(from);
  if (to) await page.locator('#edform input[name="to"]').fill(to);
}

/** Saves and waits for the dialog to close and the page to redraw. */
export async function save(page) {
  await expect(page.locator('#ed-save')).toBeEnabled();
  await page.locator('#ed-save').click();
  await expect(page.locator('#ed')).toBeHidden();
  await settle(page);
}

/** Creates an event through the API (setup for edit tests); returns its key. */
export async function createApi(page, input) {
  const [principal, cal_id] = (input.cal || CAL.privat).split('/');
  const body = { principal, cal_id, summary: input.summary, start: input.start, end: input.end, all_day: !!input.all_day, rrule: input.rrule || null, categories: input.categories || [], color: input.color || '', alarms: input.alarms || [], location: input.location || '' };
  const r = await api(page, 'POST', '/api/events', body);
  return `${principal}/${cal_id}/${r.id}`;
}

/** Opens `key` on `date` from the day view's document and unlocks it. */
export async function openEdit(page, key, date, { edit = true } = {}) {
  await page.goto(`/calendar?view=day&date=${date}`);
  await page.locator(`article.lnk[data-key="${key}"][data-date="${date}"] h3`).click();
  await page.locator('#ed:not([hidden])').waitFor();
  await settle(page);
  if (edit) {
    await page.locator('#ed-lock').click();
    await expect(page.locator('#edform')).toHaveClass(/mode-edit/);
  }
}

export const detail = (page, key) => api(page, 'GET', `/api/events/${key}`);

/** Answers the app's own yes/no question. */
export async function answer(page, yes) {
  await page.locator('.ask [data-a="1"]').waitFor();
  await page.locator(`.ask [data-a="${yes ? 1 : 0}"]`).click();
  await expect(page.locator('.ask')).toHaveCount(0);
}

export async function addCard(page, name) {
  await page.locator('.addmenu .add').click();
  await page.locator(`.addmenu [data-card-add="${name}"]`).click();
  await page.mouse.move(1, 1); // the menu is also hover-revealed: it must not cover the card
}
