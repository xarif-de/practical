// js/onboarding/guide.js — first start, part 2: the hint that leads to a setting (a bubble beside
// the target with a dashed arrow to each one it means). The bubble IS the whole guidance: there is
// no overview dialog and no way to skip a point, so a bubble carries words and nothing else.
//
// Nothing is dimmed while a hint is up: the app stays usable, the user may walk off, and the hint
// re-aims from wherever they are. The hints stay until every point is done.
// `ST` and the helpers come from steps.js: both files are fragments of one function scope
// (src/bundle.rs Wrap), so a second declaration of either would break the join.

const el = (tag, cls) => { const e = document.createElement(tag); e.className = cls; return e; };

/* The one dialog left: shown once, when the last point has just been answered. Without it the last
   bubble simply vanishes and the user is left wondering whether they finished or lost the thread. */
function done() {
  const host = el('div', 'onb-veil');
  host.id = 'onb-done';
  host.innerHTML = `<div class="onb-box onb-fin" role="dialog" aria-modal="true" aria-labelledby="onb-fin-h">
      <span class="onb-gate-ic">${ico('check')}</span>
      <h2 class="onb-h" id="onb-fin-h">${esc(tr('js-onb-done-title'))}</h2>
      <p class="onb-body">${esc(tr('js-onb-done-body'))}</p>
      <button type="button" class="btn btn-primary onb-wide" data-close>${esc(tr('js-onb-done-go'))}</button>
    </div>`;
  document.body.appendChild(host);
  const btn = $('[data-close]', host);
  const close = () => { host.remove(); removeEventListener('keydown', onKey); };
  const onKey = (e) => { if (e.key === 'Escape') close(); };   // a modal dialog must answer Escape
  btn.addEventListener('click', close);
  addEventListener('keydown', onKey);
  btn.focus();
}

// ---- the hint -----------------------------------------------------------------------------------
function guide() {
  ST.cur = ST.pick();                                // the user's position decides, not the order
  const step = ST.step(ST.cur);
  if (!step) return finished(true);          // the last point was just answered, in this very page
  const wp = ST.route(step), targets = wp.sel();
  if (!targets.length) return clear();               // nothing of this waypoint is on this page

  // every caller clears first, so there is never one to reuse
  const tip = el('div', 'onb-box onb-tip');
  tip.setAttribute('role', 'dialog');
  tip.setAttribute('aria-live', 'polite');
  document.body.appendChild(tip);
  // a waypoint may wear its own head; otherwise it wears the step's
  const head = { title: wp.title ? tr(wp.title) : ST.title(step), icon: wp.icon || step.icon, color: wp.color || step.color };
  const open = ST.openSteps();
  tip.setAttribute('aria-label', head.title);   // a dialog without a name is a finding, and unreadable
  tip.innerHTML = `<div class="onb-tip-head">
      <span class="onb-ic" style="background:var(--${head.color})">${ico(head.icon)}</span>
      <span class="onb-count">${esc(head.title)}</span>
      <span class="onb-sp"></span>
      <span class="onb-dots">${open.map(s => `<i class="${s.id === step.id ? 'on' : ''}"></i>`).join('')}</span>
    </div>
    <p class="onb-lead">${esc(ST.say(wp.lead))}</p>
    <p class="onb-body">${esc(ST.say(wp.body))}</p>
    ${targets.length > 1 ? `<p class="onb-either">${esc(tr('js-onb-either', { n: targets.length }))}</p>` : ''}
    ${wp.confirm ? `<p class="onb-keep"><button type="button" class="link" data-confirm="${esc(wp.confirm)}">${ico('check')}<span>${esc(tr(wp.confirmText))}</span></button></p>` : ''}`;
  // confirming what is in force: the server records the mark and writes nothing (domain::onboarding)
  const ok = $('[data-confirm]', tip);
  if (ok) {
    ok.addEventListener('click', async () => {
      ok.disabled = true;
      const r = await req('/settings/onboarding/confirm', { method: 'POST', json: { step: ok.dataset.confirm } });
      if (r.ok && r.data && Array.isArray(r.data.open)) { ST.view = r.data; ST.cur = null; draw(); } else { tell(r); ok.disabled = false; }
    });
  }
  aim(targets);
  place(tip, targets, wp.prefer || 'right');
  ST.mode = 'guide';
  ST.remember();
  // the user's own click on a real target carries the step forward: the page navigates or reloads,
  // and `draw` re-derives step and waypoint from the new state
}

/**
 * Nothing left: take the hints away, say so once, and tell the server to stop looking.
 *
 * `say` tells apart the two ways of getting here. Only a tab that was being guided has just
 * finished something; a tab that merely loads a finished account would be congratulated for
 * nothing. The usual last step navigates (creating a calendar reloads the page), so the deciding
 * state is the one carried over in sessionStorage, not the live one.
 */
async function finished(say) {
  clear();
  /* Only an EMPTY list ends the setup. `guide` also lands here when it finds no step for what is
     open — an id this build does not know, say — and ending a user's onboarding over that would
     mark them done with points still open, with no way back except "Erste Schritte". */
  if (ST.reported || !ST.view || ST.view.open.length) return;
  ST.reported = true;
  if (say) {
    done();
  }
  ST.mode = 'off';
  ST.remember();
  await req('/settings/onboarding/complete', { method: 'POST' });
}

/* ---- the arrow (ONE dashed curve with an arrowhead) --------------------------------------------
   One arrow, whatever the number of targets: it points at the GROUP, and the ring around the group
   says which rows belong to it. A fan of arrows, one per row, read as four separate instructions
   for what is really one choice — the user picks any of the ways, not all of them. */
function place(tip, targets, prefer) {
  const u = union(targets);
  const side = anchor(tip, u, prefer, targets.length > 1 ? 44 : 26);
  let svg = $('.onb-arrows');
  if (!svg) {
    svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    svg.setAttribute('class', 'onb-arrows');
    document.body.appendChild(svg);
  }
  const b = tip.getBoundingClientRect(), flat = side === 'left' || side === 'right';
  // the group's own edge, so the tip lands on the ring and not on one row inside it
  const r = u;
  const from = flat
    ? { x: side === 'left' ? b.right : b.left, y: Math.min(Math.max(r.top + r.height / 2, b.top + 14), b.bottom - 14) }
    : { x: Math.min(Math.max(r.left + r.width / 2, b.left + 14), b.right - 14), y: side === 'top' ? b.bottom : b.top };
  // the tip stops GAP short of the edge: an arrowhead drawn inside the ring looked like part of the
  // row rather than something pointing at it
  const G = 5;
  const to = flat
    ? { x: side === 'left' ? r.left - G : r.right + G, y: r.top + r.height / 2 }
    : { x: r.left + r.width / 2, y: side === 'top' ? r.top - G : r.bottom + G };
  // control points along the leaving axis, so the tangent at the tip points AT the target
  const c1 = flat ? { x: (from.x + to.x) / 2, y: from.y } : { x: from.x, y: (from.y + to.y) / 2 };
  const c2 = flat ? { x: (from.x + to.x) / 2, y: to.y } : { x: to.x, y: (from.y + to.y) / 2 };
  const dx = to.x - c2.x, dy = to.y - c2.y, len = Math.hypot(dx, dy) || 1, ux = dx / len, uy = dy / len;
  const H = 7, W = 4.2, bx = to.x - ux * H, by = to.y - uy * H;
  svg.innerHTML = `<path fill="none" stroke="var(--accent)" stroke-width="1.6" stroke-dasharray="4 4" d="M ${from.x} ${from.y} C ${c1.x} ${c1.y}, ${c2.x} ${c2.y}, ${bx} ${by}"/>`
    + `<polygon fill="var(--accent)" points="${to.x},${to.y} ${bx - uy * W},${by + ux * W} ${bx + uy * W},${by - ux * W}"/>`;
}

/** The smallest rect covering every element. */
function union(els) {
  const rs = els.map(e => e.getBoundingClientRect());
  const left = Math.min(...rs.map(r => r.left)), right = Math.max(...rs.map(r => r.right));
  const top = Math.min(...rs.map(r => r.top)), bottom = Math.max(...rs.map(r => r.bottom));
  return { left, right, top, bottom, width: right - left, height: bottom - top };
}

/** Places `box` beside `t` inside the viewport; returns the side it took. */
function anchor(box, t, prefer, gap) {
  const b = box.getBoundingClientRect(), M = 10;
  const fits = {
    right: t.right + gap + b.width <= innerWidth - M,
    left: t.left - gap - b.width >= M,
    bottom: t.bottom + gap + b.height <= innerHeight - M,
    top: t.top - gap - b.height >= M,
  };
  const side = [prefer, 'right', 'bottom', 'left', 'top'].find(s => fits[s]) || prefer;
  let x, y;
  if (side === 'right') { x = t.right + gap; y = t.top + t.height / 2 - b.height / 2; }
  else if (side === 'left') { x = t.left - gap - b.width; y = t.top + t.height / 2 - b.height / 2; }
  else if (side === 'bottom') { x = t.left + t.width / 2 - b.width / 2; y = t.bottom + gap; }
  else { x = t.left + t.width / 2 - b.width / 2; y = t.top - gap - b.height; }
  box.style.left = `${Math.max(M, Math.min(x, innerWidth - b.width - M))}px`;
  box.style.top = `${Math.max(M, Math.min(y, innerHeight - b.height - M))}px`;
  box.dataset.side = side;
  return side;
}

/* Marks the targets and draws ONE ring.
   With several targets the ring goes around all of them (a body-level box over the union) instead of
   one ring per row: four rings read as four things to do, while the point is that any ONE of the
   ways is enough. A single target keeps its own inset ring, which follows the element's shape and
   its exceptions (the gear, the back link — onboarding.css). */
function aim(targets) {
  $$('.onb-aim').forEach(e => e.classList.remove('onb-aim', 'onb-grouped'));
  targets.forEach(t => t.classList.add('onb-aim'));
  const u = union(targets);
  let ring = $('.onb-ring');
  if (targets.length > 1) {
    targets.forEach(t => t.classList.add('onb-grouped'));
    if (!ring) {
      ring = document.createElement('div');
      ring.className = 'onb-ring';
      document.body.appendChild(ring);
    }
    ring_at(targets);
  } else {
    ring?.remove();
  }
  if (u.top < 70 || u.bottom > innerHeight - 120) targets[0].scrollIntoView({ block: 'center', behavior: 'smooth' });
}

/* Where the ring sits. Called by `aim` and again by `replace` on every scroll and resize: the box is
   `fixed`, and the rows move inside `.main`, which is the scroller. Left out of `aim`'s own
   expensive re-aim on purpose — this is one `getBoundingClientRect` per target, and `aim` ends with
   a smooth `scrollIntoView` whose own scroll events are what carry the ring along with it. */
function ring_at(targets) {
  const box = $('.onb-ring');
  if (!box || targets.length < 2) return;
  const u = union(targets), P = 4; // air between the rows and the ring
  Object.assign(box.style, { left: `${u.left - P}px`, top: `${u.top - P}px`, width: `${u.width + 2 * P}px`, height: `${u.height + 2 * P}px` });
}

function clear() {
  $('.onb-tip')?.remove();
  $('.onb-arrows')?.remove();
  $('.onb-ring')?.remove();
  $$('.onb-aim').forEach(e => e.classList.remove('onb-aim', 'onb-grouped'));
}

function draw() {
  if (ST.mode === 'off') return clear();
  clear();
  guide();
}

// ---- start --------------------------------------------------------------------------------------
// The language gate is the page's own form (layout.html) and blocks until the language is confirmed:
// it decides every later word, so nothing may run ahead of it.
(async function start() {
  if (!await ST.load()) return;
  if ($('#onb-gate')) {
    /* Picking a language shows the gate in it right away, so the user can read what they are
       choosing. That is a language switch, not the decision: `try_only` keeps the mark unwritten,
       so the gate comes back in the new language and only the button ends it. Without JavaScript
       the radio does nothing and the button still works. */
    const form = $('#onb-lang-form'), only = form && $('input[name=try_only]', form);
    if (form && only) {
      // `form.submit()` fires no submit event, so the flag survives only until the reload; the
      // button's own submit clears it, which is what makes it the answer
      form.addEventListener('submit', () => { only.value = ''; });
      $$('input[name=lang]', form).forEach(r => r.addEventListener('change', () => {
        only.value = '1';
        form.submit();
      }));
    }
    return;                        // the language decides every later word: nothing runs ahead of it
  }
  const seen = ST.recall();
  const was = seen && seen.mode;
  ST.mode = was === 'off' ? 'off' : 'guide';
  if (!ST.view.open.length) {
    ST.mode = 'off';
    return finished(was === 'guide');   // this tab was guiding, so the last point was just answered
  }
  ST.cur = seen ? seen.cur : null;
  draw();

  /* Every re-aim measures elements (getBoundingClientRect), so none of these may run per event:
     scrolling alone would otherwise measure on every wheel tick. One frame gate for all of them,
     and the cheap `place` is used where only the position can have changed. */
  let queued = 0;
  const frame = (fn) => {
    if (ST.mode !== 'guide' || queued) return;
    queued = requestAnimationFrame(() => { queued = 0; fn(); });
  };
  const reaim = () => frame(() => { clear(); guide(); });
  // only this field changes which element is meant (an empty name makes the field the next step),
  // so a document-wide listener would rebuild the bubble on every keystroke anywhere
  const NAME = '#new-calendar input[name=name], #new-addressbook input[name=name]';
  const replace = () => frame(() => {
    const tip = $('.onb-tip'), step = ST.step(ST.cur);
    if (!tip || !step) return;
    const wp = ST.route(step), targets = wp.sel();
    if (!targets.length) return;
    place(tip, targets, wp.prefer || 'right');
    ring_at(targets);
  });
  addEventListener('resize', reaim);
  addEventListener('input', e => { if (e.target.matches?.(NAME)) reaim(); }, { capture: true });
  addEventListener('scroll', replace, { passive: true, capture: true });
  /* A picker saves without a page load, so the embedded state goes stale. Both of these ask the
     server again. The saved event is the one that matters for a closed point: the hash changes the
     moment the picker closes, which is BEFORE the post lands, so asking only then would read the
     old state and re-ask for something the user had just done (found 2026-10-09). */
  let asked = 0;
  const refresh = async () => {
    if (ST.mode === 'off') return;
    // hash changes come in bursts in a hash-routed UI; only the newest answer may become the state
    const mine = ++asked;
    await ST.load(true);
    if (mine !== asked || ST.mode === 'off') return;
    if (!ST.view.open.length) return finished(true);
    if (ST.mode === 'guide') { clear(); guide(); }
  };
  /* Taking the time-zone picker's first row ("Standard (Europe/Berlin)") is an answer as much as
     picking a city is, and the user should not have to choose a different zone to get past this
     point (asked for 2026-10-09). That row stores an EMPTY zone, so nothing in the settings would
     show they had decided — the mark does. */
  document.addEventListener('click', (e) => {
    if (ST.mode === 'off' || !ST.isOpen('tz') || !$('#settings')) return;
    const b = e.target.closest('#pick-list button[data-v]');
    // the empty value IS the "Standard" row; a city row stores a zone and closes the point by itself
    if (!b || b.dataset.v || where() !== 'pick-tz') return;
    req('/settings/onboarding/confirm', { method: 'POST', json: { step: 'tz' } }).then(r => {
      if (!r.ok || !r.data || !Array.isArray(r.data.open)) return;
      ST.view = r.data;
      ST.cur = null;
      if (!ST.view.open.length) return finished(true);
      draw();
    });
  }, { capture: true });   // the picker's own handler empties the hash, so `where()` must be read first
  addEventListener('hashchange', refresh);
  document.addEventListener('practical:prefs-saved', refresh);
})();
