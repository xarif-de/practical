// js/shell/popovers-stream.js — part of the shell IIFE (src/bundle.rs; shares its scope): user menu, popovers kept on screen (practical.fitPop), "/" hotkey,
// the live change stream (`rustical:change`)
  // user menu: click toggles (hover works via CSS, touch needs the click)
  $$('.filter > button').forEach(b => b.addEventListener('click', e => { e.stopPropagation(); b.parentElement.classList.toggle('open'); }));
  document.addEventListener('click', e => { if (!e.target.closest('.filter')) $$('.filter.open').forEach(f => f.classList.remove('open')); });
  // keep every hover/click popover inside the screen. Its CSS anchor (right: 0, or -60px on a phone) is
  // right for some toolbar positions only: the contacts column picker sits at the left of a wrapped
  // toolbar row at 390px (ran 148px off the left edge) and at the right at 600px. Measured while hidden
  // too — visibility keeps the layout — so a closed popover never widens the page.
  const fitPop = pop => {
    pop.style.translate = '';
    const r = pop.getBoundingClientRect();
    if (!r.width) return;
    let dx = Math.min(0, innerWidth - 8 - r.right);
    if (r.left + dx < 8) dx = 8 - r.left;
    if (dx) pop.style.translate = dx + 'px 0';
  };
  practical.fitPop = fitPop; // history.js draws its filter popover after this runs
  const fitPops = () => $$('.filter > .pop, .more > .pop').forEach(fitPop);
  fitPops(); addEventListener('resize', fitPops);
  // a suggestion list (body-level .rrpop, practical.placeUnder) stays inside a window that got narrower (the chaos
  // monkey: placed at 1280 px, then 390 px wide → the page 940 px wide)
  addEventListener('resize', () => $$('body > .rrpop').forEach(p => { const r = p.getBoundingClientRect(); if (r.right > innerWidth - 8) p.style.left = Math.max(8, innerWidth - 8 - r.width) + scrollX + 'px'; }));
  $$('.filter, .more').forEach(f => ['pointerenter', 'focusin', 'click'].forEach(t => f.addEventListener(t, () => { const p = f.querySelector(':scope > .pop'); if (p) fitPop(p); })));

  document.addEventListener('click', e => {
    const a = e.target.closest && e.target.closest('a[href="/calendar"], a[href="/tasks"]');
    if (!a || e.defaultPrevented || e.button || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey || !localStorage.calCals) return;
    let k; try { k = JSON.parse(localStorage.calCals)[a.getAttribute('href').slice(1)]; } catch (_) { return; }
    if (!k || !k[1] || k[0] !== (localStorage.calHidden || '[]')) return; // computed for another hidden set: the page corrects it itself
    e.preventDefault(); location.href = a.getAttribute('href') + '?cals=' + encodeURIComponent(k[1]);
  });

  // "/" focuses the search
  const q = $('#q');
  if (q) addEventListener('keydown', e => { if (e.key === '/' && document.activeElement !== q && !/input|textarea|select/i.test(document.activeElement.tagName)) { e.preventDefault(); q.focus(); } });

  // live updates: server-sent changes → custom event for the views. A hidden tab keeps its stream for
  // HIDE_GRACE, then closes it (no keep-alive every 15 s, no redraws nobody sees); a change that arrives
  // while hidden waits. Back in view the stream reopens and, if anything may have been missed, the views get
  // ONE `rustical:change` ({ resync: true }). After an error the retry waits 5–10 s, doubling up to 5–10 min; the
  // browser coming back online retries at once; a stream that opens again after an error resyncs too, since the changes
  // broadcast meanwhile never arrive (measured: a write during a 6.7 s gap stayed invisible).
  if (document.body.dataset.user && 'EventSource' in window) {
    const HIDE_GRACE = 15000, RETRY = 5000, RETRY_MAX = 300000;
    let es = null, fails = 0, retryT = 0, graceT = 0, away = false, missed = false, broke = false;
    const fire = detail => document.dispatchEvent(new CustomEvent('rustical:change', { detail }));
    const connect = () => {
      retryT = 0; es = new EventSource('/api/changes');
      es.onopen = () => { fails = 0; practical.stream(true); if (broke) { broke = false; if (document.hidden) missed = true; else fire({ resync: true }); } };
      es.addEventListener('change', ev => { let d; try { d = JSON.parse(ev.data); } catch (_) { return; } if (document.hidden) missed = true; else fire(d); });
      es.onerror = () => { practical.stream(false); es.close(); es = null; broke = true; const ms = Math.min(RETRY_MAX, RETRY * 2 ** fails++); retryT = setTimeout(connect, ms + Math.random() * ms); };
    };
    addEventListener('online', () => { if (es || away) return; clearTimeout(retryT); fails = 0; connect(); });
    const sleep = () => { away = true; missed = true; broke = false; /* the return resyncs once, through `missed` */ clearTimeout(retryT); retryT = 0; practical.stream(false); if (es) { es.close(); es = null; } };
    document.addEventListener('visibilitychange', () => {
      clearTimeout(graceT); graceT = 0;
      if (document.hidden) { graceT = setTimeout(sleep, HIDE_GRACE); return; }
      if (away) { away = false; fails = 0; connect(); }
      if (missed) { missed = false; fire({ resync: true }); }
    });
    connect();
    if (document.hidden) graceT = setTimeout(sleep, HIDE_GRACE);
  }
