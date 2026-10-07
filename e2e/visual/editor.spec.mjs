// Event/rule editor (inventory §2.3, §2.10): view, edit, new and rule mode, every card and its sub-dialogs.
// Read-only: nothing is saved (lib/visual.mjs aborts writes).
import { isPhone, shot } from '../lib/visual.mjs';
import { EV, ds, edReady, rasterAgain, edScrollTo, edSnap, openEvent, openNew, unlock } from './_editor.mjs';

shot('editor view+edit: Projekt-Review @main', async (page, snap) => {
  await openEvent(page, EV.review);
  await edSnap(page, snap, 'ed-review-view');
  await unlock(page);
  await edSnap(page, snap, 'ed-review-edit');
});

shot('editor view+edit: demo event (every property)', async (page, snap) => {
  await openEvent(page, EV.demo);
  await edSnap(page, snap, 'ed-demo-view');
  await unlock(page);
  await edSnap(page, snap, 'ed-demo-edit');
});

shot('editor view: Zahnarzt (own colour, private)', async (page, snap) => {
  await openEvent(page, EV.zahnarzt);
  await edSnap(page, snap, 'ed-zahnarzt-view');
});

shot('editor view+edit: series occurrence', async (page, snap) => {
  await openEvent(page, EV.standup);
  await edSnap(page, snap, 'ed-standup-view');
  await unlock(page);
  await edScrollTo(page, '#ed-repeat-card');
  await ds(page, snap)('ed-standup-repeat');
  // the interval pill and the end pill open their popovers
  await page.locator('#ed-rr .pillbtn[data-s="i"]').click();
  await page.locator('.rrpop').waitFor();
  await ds(page, snap)('ed-standup-repeat-interval');
  // Escape would close the dialog: a click elsewhere closes the popover
  await page.locator('#ed-repeat-card h2').click();
  await page.locator('.rrpop').waitFor({ state: 'detached' });
  await page.locator('#ed-rr .pillbtn[data-s="e"]').click();
  await page.locator('.rrpop').waitFor();
  await ds(page, snap)('ed-standup-repeat-end');
});

shot('editor view+edit: all-day vacation with a locked rule', async (page, snap) => {
  await openEvent(page, EV.urlaub);
  await edSnap(page, snap, 'ed-urlaub-view');
  await unlock(page);
  await edSnap(page, snap, 'ed-urlaub-edit');
});

shot('editor view: day line off', async (page, snap, testInfo) => {
  await openEvent(page, EV.review);
  if (isPhone(testInfo)) return; // the day line is a desktop column
  await page.locator('#ed-tl').click();
  await page.locator('#ed.no-tl').waitFor();
  await edSnap(page, snap, 'ed-review-noline');
});

shot('editor new: empty, add-card menu, every card', async (page, snap, testInfo) => {
  await openNew(page, isPhone(testInfo));
  await edSnap(page, snap, 'ed-new');
  await page.locator('.addmenu .add').click();
  await page.locator('.addmenu.open').waitFor();
  await ds(page, snap)('ed-new-addmenu');
  for (const c of ['where', 'alarm', 'repeat', 'status', 'links', 'note']) {
    if (!(await page.locator('.addmenu.open').count())) await page.locator('.addmenu .add').click();
    await page.locator(`.addmenu [data-card-add="${c}"]`).click();
  }
  await page.locator('#edh').focus();
  await page.locator('#edh').blur();
  await edSnap(page, snap, 'ed-new-cards');
});

shot('editor new: template suggestions', async (page, snap, testInfo) => {
  await openNew(page, isPhone(testInfo));
  // suggestions are PAST events with a like title (before NOW): the stand-up series
  await page.locator('#edh').pressSequentially('Team');
  await page.locator('.tplpop .lk-hit').first().waitFor();
  await ds(page, snap)('ed-new-templates');
  await page.keyboard.press('ArrowDown');
  await ds(page, snap)('ed-new-templates-sel');
});

// the geocoder needs egress (blocked): its answers are canned here, a list and then none
const PLACE = (label, lat, lon) => ({ label, name: label.split(',')[0], street: '', housenumber: '', postcode: '', city: 'Dresden', country: 'Deutschland', country_code: 'de', lat, lon, kind: 'place' });
shot('editor new: location suggestions and none', async (page, snap, testInfo) => {
  let answer = [PLACE('Dresden Hauptbahnhof, Wiener Platz 4, 01069 Dresden', 51.0405, 13.7320), PLACE('Dresden-Neustadt Bahnhof, Schlesischer Platz 1, 01097 Dresden', 51.0656, 13.7409)];
  await page.route(u => u.pathname === '/api/geo/search', r => r.fulfill({ json: answer }));
  await openNew(page, isPhone(testInfo));
  await page.locator('.addmenu .add').click();
  await page.locator('.addmenu [data-card-add="where"]').click();
  await page.locator('#ed-locin').pressSequentially('Dresden Hbf');
  await page.locator('#ed-sugg button[data-i]').first().waitFor({ timeout: 15000 });
  await page.locator('#ed-ring[hidden]').waitFor({ state: 'attached' });
  await rasterAgain(page);
  await ds(page, snap)('ed-new-loc-sugg');
  answer = [];
  await page.locator('#ed-locin').pressSequentially('x');
  await page.locator('#ed-sugg .none').waitFor({ timeout: 15000 });
  await page.locator('#ed-ring[hidden]').waitFor({ state: 'attached' });
  await rasterAgain(page);
  await ds(page, snap)('ed-new-loc-none');
});

shot('editor edit: alarm details, people, status, links, categories, note', async (page, snap) => {
  await openEvent(page, EV.review);
  // people detail in view mode
  await page.locator('#ed-people .pchip').nth(1).click();
  await page.locator('#ed-people-detail .pdetail').waitFor();
  await edScrollTo(page, '#ed-people-card');
  await ds(page, snap)('ed-review-person');
  await unlock(page);
  // alarm entry details
  await page.locator('#ed-alarm-card .row.al button[data-al-open]').first().click();
  await page.locator('#ed-alarm-card .aldet:not([hidden])').waitFor();
  await edScrollTo(page, '#ed-alarm-card');
  await ds(page, snap)('ed-review-alarm-detail');
  // a status tile's popover
  await edScrollTo(page, '#ed-status-card');
  await page.locator('#ed-status-tiles .stile[data-sel="status"]').click();
  await page.locator('.rrpop').waitFor();
  await ds(page, snap)('ed-review-status-pop');
  await page.locator('#ed-status-card h2').click();
  await page.locator('.rrpop').waitFor({ state: 'detached' });
  // a link's detail and the paste field's kind detection
  await edScrollTo(page, '#ed-links-card');
  await page.locator('#ed-lk-list [data-lact="edit"]').first().click();
  await page.locator('#ed-lk-detail .lk-dh').waitFor();
  await ds(page, snap)('ed-review-link-detail');
  await page.locator('#ed-lk-url').fill('https://meet.jit.si/PractiCalProbe');
  await ds(page, snap)('ed-review-link-paste');
  // categories typeahead
  await edScrollTo(page, '#ed-cats-card');
  await page.locator('#ed-ct-in').pressSequentially('Ur');
  await page.locator('.rrpop .ct-hit').first().waitFor();
  await ds(page, snap)('ed-review-cats-typeahead');
  await page.locator('#ed-ct-in').fill('');
  // note mentions
  await edScrollTo(page, '#ed-note-card');
  const note = page.locator('#ed-note-card textarea[name="description"]');
  await note.click();
  await note.press('End');
  await note.pressSequentially(' @Ann');
  await page.locator('.rrpop .lk-hit').first().waitFor();
  await ds(page, snap)('ed-review-note-mention');
});

shot('editor edit: map dialog', async (page, snap) => {
  await openEvent(page, EV.review);
  await unlock(page);
  await page.locator('#ed-locp [data-geo="map"]').click();
  await page.locator('#geomap:not([hidden])').waitFor();
  await page.waitForFunction(() => { const t = [...document.querySelectorAll('#gm-map img.leaflet-tile')]; return t.length > 0 && t.every(i => i.complete && i.classList.contains('leaflet-tile-loaded')); });
  await ds(page, snap)('ed-review-map');
});

shot('editor edit: unsaved-changes bar, remove-card and delete questions', async (page, snap) => {
  await openEvent(page, EV.review);
  await unlock(page);
  await page.locator('#edh').fill('Projekt-Review (geändert)');
  await page.locator('#ed-close').click();
  await page.locator('#ed-confirm:not([hidden])').waitFor();
  await ds(page, snap)('ed-review-confirm');
  await page.locator('#ed-back').click();
  await page.locator('#ed-confirm[hidden]').waitFor({ state: 'attached' });
  await page.locator('#ed-alarm-card [data-card-rm]').click();
  await page.locator('.ask').waitFor();
  await ds(page, snap)('ed-review-ask-card');
  await page.keyboard.press('Escape');
  await page.locator('.ask').waitFor({ state: 'detached' });
  await page.locator('#ed-delete').click();
  await page.locator('.ask').waitFor();
  await ds(page, snap)('ed-review-ask-delete');
});

shot('editor rule mode: existing and new rule', async (page, snap) => {
  await page.goto('/calendar?rule=Arbeit&back=%2Fsettings%23categories');
  await page.locator('#ed.rulemode:not([hidden])').waitFor();
  await edReady(page);
  await edSnap(page, snap, 'ed-rule-arbeit');
  await page.goto('/calendar?rule=Urlaub&back=%2Fsettings%23categories');
  await page.locator('#ed.rulemode:not([hidden])').waitFor();
  await edReady(page);
  await edSnap(page, snap, 'ed-rule-urlaub');
  await page.goto('/calendar?rule=Neue%20Kategorie');
  await page.locator('#ed.rulemode:not([hidden])').waitFor();
  await edReady(page);
  await edSnap(page, snap, 'ed-rule-new');
});
