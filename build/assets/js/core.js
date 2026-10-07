// js/core.js — app.js starts here (classic script, vanilla ES2020; parts joined in src/bundle.rs order): OIDC auto-submit,
// i18n and dates (window.LOC tr trs plural dfmt MONTHS WEEKDAYS WEEKDAYS_LONG), window.practical helpers
/* i18n: the page loads its js-* strings first (window.I18N: /assets/i18n-<lang>.js, build.rs from locales/*.ftl; placeholders %name);
   month and weekday names and date formats come from Intl in the page's locale */
// login: the single sign-on form submits itself when it is the only way in (login.html `data-auto`; was an
// inline script, which the Content-Security-Policy forbids)
{ const f = document.getElementById('oidc-form'); if (f && f.hasAttribute('data-auto')) f.submit(); }
(function () {
  const S = window.I18N || {};
  const LOC = window.LOC = document.body.dataset.locale || 'de-DE';
  // a plural message (i18n.rs plural_forms: {"one": …, "other": …}, as an object or as a data attribute's JSON):
  // the form for n by the page locale's CLDR rules, the one the server would pick
  const PR = new Intl.PluralRules(LOC);
  window.plural = (s, n) => { if (typeof s === 'string' && s.startsWith('{"')) try { s = JSON.parse(s); } catch (_) { return s; } return s && typeof s === 'object' ? s[PR.select(n)] ?? s.other : s; };
  window.tr = (k, v) => { let s = S[k] ?? k; if (s && typeof s === 'object') s = plural(s, +(v || {})[s.$]); if (v) for (const [n, x] of Object.entries(v)) s = s.split('%' + n).join(x); return s; };
  window.trs = (prefix, names) => Object.fromEntries(names.map(n => [n, tr(prefix + n)]));
  // one formatter per option set: building one per call cost 65 ms for 1,700 history times, reusing it 1.1 ms (measured in node)
  const fmts = new Map(), f = o => { const k = JSON.stringify(o); let x = fmts.get(k); if (!x) fmts.set(k, x = new Intl.DateTimeFormat(LOC, o)); return x; };
  window.dfmt = (d, o) => f(o).format(d);
  const days = style => Array.from({ length: 7 }, (_, i) => f({ weekday: style }).format(new Date(2024, 0, 1 + i)).replace(/\.$/, '')); // 2024-01-01 was a Monday
  window.MONTHS = Array.from({ length: 12 }, (_, i) => f({ month: 'long' }).format(new Date(2024, i, 1)));
  window.WEEKDAYS = days('short'); window.WEEKDAYS_LONG = days('long');
  // helpers of every block below and of history.js (loaded after this file): one definition each
  const $ = (s, r = document) => r.querySelector(s), ESC = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' };
  let slideT = null; // the running view-switch slide (practical.slide)
  const pad = n => String(n).padStart(2, '0'), SPRITE = ($('meta[name=icons]') || {}).content || '';
  window.practical = {
    $, $$: (s, r = document) => [...r.querySelectorAll(s)],
    csrf: () => ($('meta[name=csrf]') || {}).content || '',
    // HTML text and double-quoted attributes: `esc` prints null as "null"; escN prints '' for null/undefined; escQ also escapes '
    esc: s => String(s).replace(/[&<>"]/g, c => ESC[c]),
    escN: s => String(s ?? '').replace(/[&<>"]/g, c => ESC[c]),
    escQ: s => String(s ?? '').replace(/[&<>"']/g, c => ESC[c]),
    pad,
    ymd: d => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`, // the local calendar day
    parseYmd: s => { const [y, m, d] = String(s).slice(0, 10).split('-').map(Number); return new Date(y, m - 1, d); },
    // the icon sprite (<meta name=icons>, a cached file): every <use href> is `${practical.sprite}#i-…`
    sprite: SPRITE,
    ico: (n, cls) => `<svg class="ico${cls ? ' ' + cls : ''}" aria-hidden="true"><use href="${SPRITE}#i-${n}"/></svg>`,
    /* the calendar filter kept in this browser (localStorage.calHidden) is a ?cals= list of the shown calendars,
       which only the calendar and the tasks page can compute (each lists its own calendars): each records its
       list for the hidden set it was computed from (localStorage.calCals), and a plain click on a bare link to
       that page carries it, so the server renders the filtered page at once instead of the page fetching it again */
    keepCals: (page, cals) => { let m = {}; try { m = JSON.parse(localStorage.calCals || '{}'); } catch (_) {} m[page] = [localStorage.calHidden || '[]', cals]; localStorage.calCals = JSON.stringify(m); },
    // a JSON write: practical.req's answer (below), never a thrown error
    send: (url, method, body) => window.practical.req(url, { method, json: body }),
    /* an entry a click opens (a calendar event, a task card, a contact) works from the keyboard too: a Tab stop, Enter
       or Space on it is the click; role "button" only on a plain div/span that holds no control of its own (a list
       item or an article keeps its own role) */
    keyClick: el => {
      el.tabIndex = 0; if (/^(DIV|SPAN)$/.test(el.tagName) && !el.querySelector('a, button, input, select, textarea')) el.setAttribute('role', 'button');
      el.onkeydown = e => { if ((e.key === 'Enter' || e.key === ' ') && e.target === el) { e.preventDefault(); el.click(); } };
    },
    // a suggestion list (body-level .rrpop) under `anchor`, kept inside the dialog box; `r` = the anchor's rect
    placeUnder: (pop, anchor, r = anchor.getBoundingClientRect()) => { const box = anchor.closest('.box'), maxRight = (box ? box.getBoundingClientRect().right : innerWidth) - 8;
      pop.style.left = `${Math.max(8, Math.min(r.left, maxRight - pop.offsetWidth)) + scrollX}px`; pop.style.top = `${r.bottom + scrollY + 4}px`; },
    // what a note (DESCRIPTION) links and mentions; the editor's note card reads and detects them (noteHtml, ntDetect)
    noteRe: { RE_URL: /\bhttps?:\/\/[^\s<>"')\]]+/gi, RE_MAIL: /(?<![\w.+-])[\w.+-]+@[\w-]+(?:\.[\w-]+)+\b/g, RE_TEL: /(?<![\w@.+-])\+?\d[\d ()\/-]{6,}\d(?![\w@])/g, RE_MENTION: /(?<![\w.])([@#!])(\p{L}[\p{L}\p{N}.-]*(?: \p{Lu}[\p{L}\p{N}.-]*){0,2})/gu },
    /* a note as the view's HTML. Each pattern runs over the plain text, in this order: URLs, e-mail addresses,
       phone numbers, @/#/! mentions; a later one only where no earlier one matched, so a URL that holds
       `x@y.de` or `#tag` stays one link. Every piece of the text is escaped once. Shared (not inside the
       editor) so e2e/functional can test it directly. */
    noteHtml: t => {
      const { esc: e, noteRe: R } = window.practical, hits = [];
      const take = (re, html) => { for (const m of t.matchAll(re)) { const a = m.index, b = a + m[0].length; if (!hits.some(h => a < h.b && h.a < b)) hits.push({ a, b, html: html(m) }); } };
      take(R.RE_URL, ([u]) => `<a href="${e(u)}" target="_blank" rel="noopener">${e(u)}</a>`);
      take(R.RE_MAIL, ([m]) => `<a href="mailto:${e(m)}">${e(m)}</a>`);
      take(R.RE_TEL, ([m]) => `<a href="tel:${m.replace(/[^\d+]/g, '')}">${e(m)}</a>`);
      take(R.RE_MENTION, ([, p, v]) => { const name = v.replace(/[.,;:!?]+$/, ''); return `<span class="nt-m" data-k="${p}" data-v="${e(name)}">${e(name)}</span>${e(v.slice(name.length))}`; });
      let h = '', i = 0;
      for (const x of hits.sort((x, y) => x.a - y.a)) { h += e(t.slice(i, x.a)) + x.html; i = x.b; }
      return (h + e(t.slice(i))).replace(/^- (.*)$/gm, '• $1');
    },
    /* the touches in a wheel stream: wheelTouch() → (t, delta) → true where a new touch starts. A page never learns when
       fingers touch a trackpad, and macOS scrolls on after they lift (momentum: a steady frame cadence, deltas that only
       shrink), so a touch is read from the stream: a rest (> 150 ms), a reversal, or a coasting stream that breaks — a
       gap no coasting stream has (≥ 24 ms) with a delta far off the coasting speed (a touch stops the momentum: 30–57 ms
       of silence, then a slow finger), or any such gap once the tail has faded below 0.1 px/ms (what is left of it can
       turn no page). Speeds are taken over pairs of events in the input's own time, so a coalesced event keeps its speed
       and a finger's uneven samples are never "coasting". Tuned on a MacBook Pro trackpad, 120 Hz, Edge 154 (2026-10-07):
       30 swipes normal/fast/strong/gentle → 30 touches (the former speed-jump rule: 62), then 57 free swipes the user
       judged one page each; each part but the reversal guard errs on the replay when left out
       (e2e/functional/wheel-touch.json). Shared (not inside the calendar) so e2e/functional can test it directly. */
    wheelTouch: () => {
      let last = -1e9, dir = 0, h = []; // [gap ms, |delta|] of this touch's last 5 events
      const g = i => Math.max(1, h[i - 1][0] + h[i][0]), v = i => (h[i - 1][1] + h[i][1]) / g(i); // ms and px/ms of the event pair ending at i
      return (t, delta) => {
        const gap = t - last, a = Math.abs(delta), s = Math.sign(delta); last = t;
        let fresh = gap > 150 || (s && dir && s !== dir);
        if (!fresh && gap >= 24 && h.length === 5 && [2, 3, 4].every(i => v(i) <= v(i - 1) * 1.6 + 1.5 / g(i))) { // coasting: pair speeds that never grow
          const sp = (h[2][1] + h[3][1] + h[4][1]) / Math.max(1, h[2][0] + h[3][0] + h[4][0]), want = sp * gap; fresh = sp < 0.1 || a < want / 2 || a > 2 * want + 2; }
        if (fresh) { dir = s; h = []; } else if (!dir) dir = s;
        h.push([gap, a]); if (h.length > 5) h.shift();
        return fresh;
      };
    },
    /* the axis of each touch in a wheel stream: wheelAxis() → (e) → { n, fresh, axis, lean, dx, dy } (dx/dy in px, n counts
       the touches). One wheelTouch per axis; a decided touch ends where its own axis says (the other one only carries
       noise, and a run of zeros reads as a coast that breaks), an undecided one only where both say so (a rest). The
       axis is decided after 12 px of travel: 'x' only where the sideways travel is more than twice the vertical one
       (< 26.6° off horizontal), so a diagonal or wobbly scroll stays 'y'. lean: the axis, before that the event's
       larger delta (Chromium dispatches the rest of a scroll sequence uncancelable once its first event was not
       cancelled, so the first one must already go to the right side). */
    wheelAxis: () => {
      const P = window.practical, tx = P.wheelTouch(), ty = P.wheelTouch(); let n = 0, axis = null, sx = 0, sy = 0;
      return e => {
        const k = e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? e.currentTarget.clientHeight : 1, dx = e.deltaX * k, dy = e.deltaY * k;
        const fx = tx(e.timeStamp, dx), fy = ty(e.timeStamp, dy), fresh = axis === 'x' ? fx : axis === 'y' ? fy : fx && fy;
        if (fresh) { n++; axis = null; sx = sy = 0; }
        if (!axis) { sx += Math.abs(dx); sy += Math.abs(dy); if (sx + sy >= 12) axis = sx > 2 * sy ? 'x' : 'y'; }
        return { n, fresh, axis, lean: axis || (Math.abs(dx) > Math.abs(dy) ? 'x' : 'y'), dx, dy };
      };
    },
    /* a view that turns its pages by the wheel: wheelPages(turn, pass, axis = 'y') → a wheel listener that calls turn(±1)
       once per touch (wheelAxis), after 60 px along `axis`; a touch along the other axis is left alone (not cancelled:
       the page or another listener has it). One listener per view for the page's life: attached again to each
       swapped-in fragment, the events that arrive after a swap still count as the same touch. pass(e, g) true: the
       event is left to the page (it scrolls). Used by the month grid and the year planner (calendar.js, 'y') and by
       swipeViews ('x'). */
    wheelPages: (turn, pass, axis = 'y') => {
      const gesture = window.practical.wheelAxis(); let acc = 0, done = false;
      return e => {
        const g = gesture(e); if (g.fresh) { acc = 0; done = false; }
        if (g.lean !== axis || (pass && pass(e, g))) return;
        e.preventDefault(); if (done) return;
        acc += axis === 'x' ? g.dx : g.dy; if (Math.abs(acc) >= 60) { done = true; turn(acc > 0 ? 1 : -1); }
      };
    },
    /* a view switch slides: slide(dir, swap) runs swap() (the DOM change) inside a same-document view transition in
       which `.main` (the page under the top bar, clipped there; its toolbar with the view switch is captured on its own
       and shown at once, frame.css `pc-bar`) leaves to one side and comes back from the other — dir > 0: the view to the right in the switch (the old one goes left), < 0 mirrored, 0 none.
       Only transform on two snapshots, run by the compositor (frame.css `pc-view`); the root is not captured, so the
       rest of the page stays live and the attribute that turns the names on (html[data-slide]) is gone once it
       ends. Without the API or with prefers-reduced-motion, swap() runs at once. → updateCallbackDone (swap's
       errors reject it). A second switch during the slide skips the first to its end (the API's own rule). */
    slide: (dir, swap) => {
      const d = document.documentElement;
      if (!dir || !document.startViewTransition || matchMedia('(prefers-reduced-motion: reduce)').matches) { try { swap(); return Promise.resolve(); } catch (e) { return Promise.reject(e); } }
      d.dataset.slide = dir > 0 ? 'next' : 'prev';
      const t = slideT = document.startViewTransition(swap);
      t.ready.catch(() => {}); t.finished.finally(() => { if (slideT === t) { slideT = null; delete d.dataset.slide; } }).catch(() => {}); // a failed swap is told by updateCallbackDone
      return t.updateCallbackDone;
    },
    /* a sideways swipe switches the page's views: swipeViews(area, views, at, busy) — views() = the view switch's
       buttons in order, at(buttons) = the index on screen (default: the aria-pressed one; -1: none, no swipe), busy()
       = the page's own reasons to leave a swipe alone. Fingers to the left bring the view to the right (as paging
       does), none past either end; the switch is its button's click (the page's own path), and a clearly sideways
       swipe first counts as the intent on that button (practical.intend: its prefetch). Never from inside a field, an
       editor, a dialog or a popover (a contenteditable only while it is being edited: the tables' cells are all
       contenteditable), nor while a modal dialog or a popover is open or the page is zoomed in. Content
       that scrolls sideways comes first: a touch whose scroller (between the target and `area`) could still move its
       way when it began is that scroller's, all of it, momentum included.
       Wheel (trackpads, tilt wheels, Shift+wheel): wheelPages on 'x', one switch per touch; its events are cancelled,
       which also keeps the browser's back/forward swipe off the page. Touch: passive listeners (vertical scrolling,
       taps, long-press selection stay native), decided on touchend: ≥ 60 px sideways at more than twice the vertical
       travel, one finger throughout, no drag (dragstart) and no selection change in between. */
    swipeViews: (area, views, at, busy) => {
      const P = window.practical;
      at = at || (bs => bs.findIndex(b => b.getAttribute('aria-pressed') === 'true'));
      const target = dir => { const bs = views(), i = at(bs); return i < 0 ? null : bs[i + dir] || null; };
      const go = dir => { const b = target(dir); if (b) b.click(); };
      const off = el => !el.closest || !!el.closest('input, textarea, select, .ed, .cardform, .pop, .ask') || (document.activeElement && document.activeElement.isContentEditable && document.activeElement.contains(el))
        || P.$$('[aria-modal="true"]').some(d => d.getClientRects().length) || !!P.$('.filter.open, .more.open') || (window.visualViewport && visualViewport.scale > 1.01) || !!(busy && busy());
      const scrolls = (el, dir) => { for (let p = el; p && p !== area.parentElement; p = p.parentElement) { const o = getComputedStyle(p).overflowX, max = p.scrollWidth - p.clientWidth; if ((o === 'auto' || o === 'scroll') && max > 1 && (dir > 0 ? p.scrollLeft < max - 1 : p.scrollLeft > 1)) return true; } return false; };
      let tn = -1, took = false; // the wheel touch decided last, and whether the content (or a reason above) has it
      area.addEventListener('wheel', P.wheelPages(go, (e, g) => {
        if (g.n !== tn && g.dx) { tn = g.n; const dir = Math.sign(g.dx); took = off(e.target) || scrolls(e.target, dir); if (!took) P.intend(target(dir)); }
        return g.n !== tn || took;
      }, 'x'), { passive: false });
      let t0 = null; const sel = () => String(getSelection());
      area.addEventListener('touchstart', e => { t0 = null; if (e.touches.length !== 1 || off(e.target)) return; const p = e.touches[0];
        t0 = { x: p.clientX, y: p.clientY, sel: sel(), hint: 0, r: scrolls(e.target, 1), l: scrolls(e.target, -1) }; }, { passive: true });
      area.addEventListener('touchmove', e => { if (!t0) return; if (e.touches.length !== 1) { t0 = null; return; }
        const dx = e.touches[0].clientX - t0.x, dy = e.touches[0].clientY - t0.y, dir = dx < 0 ? 1 : -1;
        if (t0.hint !== dir && !(dir > 0 ? t0.r : t0.l) && Math.abs(dx) >= 24 && Math.abs(dx) > 2 * Math.abs(dy)) { t0.hint = dir; P.intend(target(dir)); } }, { passive: true });
      area.addEventListener('touchend', e => { const s = t0; t0 = null; if (!s || e.touches.length) return;
        const p = e.changedTouches[0], dx = p.clientX - s.x, dy = p.clientY - s.y, dir = dx < 0 ? 1 : -1;
        if (Math.abs(dx) >= 60 && Math.abs(dx) > 2 * Math.abs(dy) && !(dir > 0 ? s.r : s.l) && sel() === s.sel) go(dir); });
      ['touchcancel', 'dragstart'].forEach(t => area.addEventListener(t, () => { t0 = null; }, true));
    },
  };
})();
