// js/editor/setup.js — the editor IIFE starts here (src/bundle.rs joins every js/editor/ part into ONE function: shared
// scope, a part alone is no script, the early return below covers all): shared state and helpers. The function is
// `practical.editorInit`, run at load and again by a page that loads the dialog later (GET /editor, the settings'
// categories); a dialog removed from the page stays hidden, which every window listener here asks first.
// ---- editor: the card dialog. One implementation for THREE consumers — the calendar's events,
// the category rules and the tasks page; the markup comes from templates/editor.html, which the
// calendar and the tasks page both include. The page it runs on publishes `window.rusticalHost`
// (where "now" is and how to reload); everything else lives here. ----
  if (!document.getElementById('edform')) return;
  const { $, $$, esc, ico, pad, ymd: fmt, parseYmd: parse, placeUnder } = practical;
  const lang = document.body.dataset.lang || 'de';
  const MON = MONTHS;
  const DOW = WEEKDAYS;
  const T = trs('js-ed-', ['suggNone', 'err', 'del', 'delRule', 'newEv', 'newTask', 'conflict', 'delTask']);
  const HOST = () => window.rusticalHost || {};
  let refreshQ = 0; // a refresh() waiting for the next frame (typing, see the form's input listener)
  const flush = () => { if (refreshQ) refresh(); };
  let solo = null, soloSigs = {}; // one-card mode (see soloStart); declared early, cards() reads it from the start
  const root = document.getElementById('viewroot') || document.body;
  const today = root.dataset.today || fmt(new Date());
  // the host owns the current date and the calendar filter; without one the editor still works
  const state = { get date() { const h = HOST(); return (h.date && h.date()) || today; }, get cals() { const h = HOST(); return (h.cals && h.cals()) || ''; } };
  // `saved`: the key just written, so a page that keeps the editor open can show it again after the reload
  const load = (push, saved) => { const h = HOST(); if (h.reload) h.reload(push, saved); };
  const pk = document.getElementById('picker') || {};
  // which component this dialog edits: an event or a task (templates/editor.html, `editor_kind`)
  const ED_SCOPE = document.getElementById('ed').classList.contains('taskmode') ? 'task' : 'event';
  const TASK = ED_SCOPE === 'task';
  const API = TASK ? '/api/tasks' : '/api/events';
  // a card this consumer does not render: the stand-in swallows the wiring instead of throwing
  const NIL = document.createElement('div');
  const $x = (sel, r = document) => r.querySelector(sel) || NIL;
