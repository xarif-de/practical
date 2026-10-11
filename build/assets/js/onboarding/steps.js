// js/onboarding/steps.js — first start, part 1: the state, the steps and where each one leads.
//
// The server says which steps are open (`<script id="onboarding">`, domain::onboarding::View) and
// says it again after every page load. No progress is kept in the browser.
//
// A step's `at()` returns the waypoint for WHERE THE USER IS RIGHT NOW, derived, never stored: that
// lets the user walk off and come back without the hint going stale, and it keeps the order free —
// clicking into the pages of another open step adopts that step (`pick`).
// one declaration for both parts of this scope (bundle.rs Wrap: they share ONE function)
const { $, $$, esc, ico, req, tell } = window.practical;

const ST = window.onb = {
  view: null,          // { open: [...], tz } as the server sends it, or null
  mode: 'off',         // 'off' | 'guide'
  cur: null,           // the id of the step being guided
  reported: false,     // "nothing left" was reported to the server once
};

/* Where in the hints this TAB is. The server keeps what the user decided; this keeps only which
   point they are being shown, which is why it is sessionStorage and not the database. */
const SEEN = 'onbSeen';
ST.remember = () => { try { sessionStorage.setItem(SEEN, JSON.stringify({ mode: ST.mode, cur: ST.cur })); } catch (_) {} };
ST.recall = () => { try { return JSON.parse(sessionStorage.getItem(SEEN)) || null; } catch (_) { return null; } };

const onSettings = () => !!$('#settings');
/* Where the user is inside the settings. The hash is the router's own truth
   (shell/settings-pages.js reads it); the pickers all render into the one #pick page, so the active
   page's id could not tell the time-zone picker from the region picker. */
const where = () => (onSettings() ? decodeURIComponent(location.hash.slice(1)) || 'home' : '');

/** The visible settings button of the top bar or the tab bar (CSS hides one of the two). */
const navBtn = () => $$('.topbar .nav a[href="/settings"], .tabbar a[href="/settings"]')
  .filter(vis).slice(0, 1);

/** On screen at all: `.imp-go` exists before a file is chosen but is hidden, and must not be ringed. */
function vis(el) {
  const r = el.getBoundingClientRect();
  return r.width > 0 && r.height > 0;
}

/** The store a settings page belongs to: the remote pages are `#remote-cal` / `#remote-ab`. */
const short = kind => (kind === 'calendar' ? 'cal' : 'ab');

/** A waypoint's targets, in the order they are ringed; missing or hidden ones are dropped. */
const q = (...sels) => () => sels.map(s => $(s)).filter(e => e && vis(e));

/* Its own head: this waypoint is not about any one setting, it only shows the way in. Wearing the
   title and icon of whichever step happens to be first said "Zeitzone" over a text about the gear. */
const GO_SETTINGS = { sel: navBtn, prefer: 'bottom', lead: 'js-onb-go-settings', body: 'js-onb-go-settings-b',
  title: 'js-onb-start-title', icon: 'wand', color: 'c4' };
/* Only for a settings page that belongs to no open step: then the way on really is up. A page of
   another OPEN step never gets here, because `pick` switches to that step instead. */
// beside it, not below: the back link sits above the page title, and a bubble under it covered that
const BACK = { sel: q('#settings .page.on .back'), prefer: 'right', lead: 'js-onb-back', body: 'js-onb-back-b' };

/** The two collection steps differ only in their ids, words and pages. */
const collection = (id, kind) => ({
  id,
  icon: id === 'calendar' ? 'calendar' : 'contacts',
  color: id === 'calendar' ? 'c1' : 'c2',
  // the remote pages are named after the store (cal | ab), not after the settings page
  mine: w => w === 'collections' || w === `new-${kind}` || w === `import-${kind}` || w === `sync-${kind}` || w === `remote-${short(kind)}` || w === `remote-sync-${short(kind)}`,
  at: () => {
    const w = where();
    const sh = short(kind);
    /* On a remote page the address comes first: the sign-in does nothing without it (remote.js
       `needServer`), so pointing at the button while the field is empty would be the wrong order —
       the same reason the create page points at the name field first. */
    if (w === `remote-${sh}` || w === `remote-sync-${sh}`) {
      const k = w.startsWith('remote-sync') ? '-sync' : '';
      const addr = $(`#${w} .rm-origin`);
      const sel = addr && !addr.value.trim() ? q(`#${w} .rm-origin`) : q(`#${w} .rm-take`, `#${w} .rm-go`);
      return { sel, lead: `js-onb-remote${k}`, body: `js-onb-remote${k}-b` };
    }
    // the source lists: one row per place the data can come from
    if (w === `import-${kind}` || w === `sync-${kind}`) {
      const rows = w === `sync-${kind}`
        ? q(`#sync-${kind} a[href="#remote-sync-${sh}"]`)
        : q(`#import-${kind} .imp-pick`, `#import-${kind} a[href="#remote-${sh}"]`);
      // the import page also carries the file flow: once a file is chosen its button is the way on
      const go = $(`#import-${kind} .imp-go`);
      if (w === `import-${kind}` && go && vis(go)) return { sel: q(`#import-${kind} .imp-go`), lead: 'js-onb-import', body: 'js-onb-import-b' };
      return { sel: rows, lead: `js-onb-${w.startsWith('sync') ? 'sync' : 'from'}`, body: `js-onb-${w.startsWith('sync') ? 'sync' : 'from'}-b` };
    }
    if (w === `new-${kind}`) {
      // the name is a required field and starts EMPTY (macros.html coll_fields), so the submit
      // button does nothing until it is filled — pointing at it first is the honest order
      const name = $(`#new-${kind} input[name=name]`);
      return name && !name.value.trim()
        ? { sel: q(`#new-${kind} input[name=name]`), lead: 'js-onb-name', body: 'js-onb-name-b' }
        : { sel: q(`#new-${kind} form button[type=submit]`), lead: 'js-onb-make', body: 'js-onb-make-b' };
    }
    /* Every way in, in the order the list shows them. The sync row is only there when a server is
       configured (`remote_sync`), and `q` drops a missing or hidden target — so the same waypoint
       covers an instance with the feature and one without, and the ring closes around whatever is
       really offered. */
    if (w === 'collections') {
      return {
        sel: q(`a[href="#new-${kind}"]`, `a[href="#import-${kind}"]`, `a[href="#sync-${kind}"]`),
        lead: `js-onb-${id}-ways`,
        body: `js-onb-${id}-ways-b`,
      };
    }
    return { sel: q('a[href="#collections"]'), lead: `js-onb-${id}-row`, body: `js-onb-${id}-row-b` };
  },
});

/* Public and school holidays are two points: a user may want the one and not the other. Each lives
   behind its own row of the Region group and its own picker pages (#regions-KIND →
   #pick-country-KIND → #pick-region-KIND-…), so one shape serves both. Inside those pages the hint
   falls silent — the user is choosing there, and a bubble would only be in the way. */
const holidays = (id, kind, icon, color) => {
  const pages = new RegExp(`^(regions-${kind}|pick-country-${kind}|pick-region-${kind}-)`);
  return {
  id,
  icon,
  color,
  mine: w => pages.test(w),
  at: () => (where() === `regions-${kind}`
    // the list page: the way on is the "add a region" button, and this is where saying that several
    // are allowed belongs — on the page that has the button, not two levels above it
    ? { sel: q(`#regions-${kind} a[href="#pick-country-${kind}"]`), lead: `js-onb-${id}-add`, body: `js-onb-${id}-add-b` }
    : pages.test(where())
    ? { sel: () => [], lead: `js-onb-${id}-row`, body: `js-onb-${id}-row` }   // quiet while choosing
    : {
      sel: q(`a[href="#regions-${kind}"]`),
      lead: `js-onb-${id}-row`,
      body: `js-onb-${id}-row-b`,
      // no default means there is nothing to keep: the only confirmation is "I want none"
      confirm: id,
      confirmText: `js-onb-${id}-none`,
    }),
  };
};

/* The picker's rows (#pick-list) are the targets once the time-zone picker is open. Its first row
   is "Standard: <zone>" and commits an EMPTY zone, so taking it leaves the step open on purpose;
   any city row stores a zone, and that is what closes the step (domain::onboarding::Facts). */
const pickRows = q('#pick-list button[data-v]');

ST.STEPS = [
  {
    id: 'tz', icon: 'clock', color: 'c6',
    mine: w => w === 'pick-tz',
    at: () => (where() === 'pick-tz'
      ? { sel: pickRows, lead: 'js-onb-tz-act', body: 'js-onb-tz-act-b' }
      : { sel: q('a[href="#pick-tz"]'), lead: 'js-onb-tz-row', body: 'js-onb-tz-row-b', confirm: 'tz', confirmText: 'js-onb-tz-keep' }),
  },
  holidays('regions', 'public', 'party', 'hol'),
  holidays('school', 'school', 'grad', 'fer'),
  collection('calendar', 'calendar'),
  collection('addressbook', 'addressbook'),
];

ST.step = id => ST.STEPS.find(s => s.id === id);
ST.isOpen = id => !!ST.view && ST.view.open.includes(id);
/** The open steps, in the order they are offered (the language has its own dialog). */
ST.openSteps = () => (ST.view ? ST.STEPS.filter(s => ST.view.open.includes(s.id)) : []);

/**
 * Which step to guide: the user's position decides. Walking into the pages of another open step
 * adopts that step, because the only alternative would be sending them back up, and that is the
 * order-forcing this must not do. Otherwise the current step stands, or the first open one.
 */
ST.pick = () => {
  const cur = ST.step(ST.cur), w = where();
  if (!cur || !ST.isOpen(cur.id)) return (ST.openSteps()[0] || {}).id || null;
  if (w && w !== 'home' && !cur.mine(w)) {
    const other = ST.openSteps().find(s => s.mine(w));
    if (other) return other.id;
  }
  return cur.id;
};

/** The next waypoint of `step`, seen from wherever the user is now. */
ST.route = step => {
  if (!onSettings()) return GO_SETTINGS;
  const w = where();
  if (w !== 'home' && !step.mine(w)) return BACK;
  return step.at();
};

/** A hint's words; `%tz` is the only value any of them names. */
ST.say = key => tr(key, { tz: (ST.view || {}).tz || '' });
ST.title = step => tr(`js-onb-${step.id}-title`);

/** The server's state: the copy in the page, or freshly asked after an in-page save. */
ST.load = async (refresh) => {
  if (!refresh) {
    const el = $('#onboarding');
    if (!el) return (ST.view = null);
    try { ST.view = JSON.parse(el.textContent); } catch (_) { ST.view = null; }
    return ST.view;
  }
  const r = await req('/api/onboarding', {});
  if (r.ok && r.data && Array.isArray(r.data.open)) ST.view = r.data;
  return ST.view;
};
