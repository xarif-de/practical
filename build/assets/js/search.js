/* js/search.js — the universal search of the top bar.
   The drop-down shows at most 5 hits per category and 10 in all, shared by each category's share of
   the matches (the server ranks them, domain::search); "all results" (Enter without a pick) opens a
   dialog with every hit, filtered by category. Finished searches — a result opened, or Enter — go to
   the history (settings → search), with the result last opened: the drop-down offers matching ones at
   its foot, and completes the query from them (grey behind the caret; Tab goes straight to that
   result). A pick keeps the view on screen where it can show it (calendar, contacts, tasks). */
(function () {
  const q = document.getElementById('q'), box = document.getElementById('sres'), ghost = document.getElementById('q-ghost');
  if (!q) return;
  const { $, ico } = practical, csrf = practical.csrf();
  const T = trs('js-search-', ['jump', 'ev', 'ct', 'tk', 'none', 'all', 'of', 'hist', 'histRm', 'dlg', 'tabAll', 'close', 'keys', 'only', 'series', 'why']);
  const esc = practical.escQ;
  // the server's fold (domain::search::fold): case and diacritics off, for highlighting and history matches
  const FOLD = { 'ß': 'ss', 'æ': 'ae', 'œ': 'oe', 'ø': 'o', 'ł': 'l' };
  const foldc = c => FOLD[c] ?? c.normalize('NFD').replace(/[\u0300-\u036f]/g, '');
  const fold = s => [...String(s).toLowerCase()].map(foldc).join('');
  const SCOPE = /^\s*[^\s:]+:\s*/;
  const wordsOf = (v, scoped) => fold(scoped ? v.replace(SCOPE, '') : v).split(/[^\p{L}\p{N}]+/u).filter(w => w.length >= 2);
  function marksOf(text, ws) { // which characters of `text` the query's words match (typos are not marked)
    const on = new Array(text.length).fill(false); if (!ws.length) return on;
    let f = '', g = ''; const map = [], gmap = []; // g: letters and digits only, for words typed without spaces
    for (let i = 0; i < text.length; i++) for (const x of foldc(text[i].toLowerCase())) { f += x; map.push(i); if (/[\p{L}\p{N}]/u.test(x)) { g += x; gmap.push(i); } }
    const mark = (hay, m, w) => { let hit = false; for (let p = hay.indexOf(w); p >= 0; p = hay.indexOf(w, p + w.length)) { hit = true; for (let j = p; j < p + w.length; j++) on[m[j]] = true; } return hit; };
    // starts of consecutive words ("dietstein"): the server's starts_chain, marking each part
    const W = []; for (const m of f.matchAll(/[\p{L}\p{N}]+/gu)) W.push({ s: m[0], at: m.index });
    const chain = (w, k, first) => { const x = W[k]; if (!x) return null; let c = 0; while (c < w.length && c < x.s.length && w[c] === x.s[c]) c++;
      for (let l = c; l >= 2; l--) { if (l === w.length) { if (!first && l >= 3) return [[x.at, l]]; continue; } const r = chain(w.slice(l), k + 1, false); if (r) return [[x.at, l], ...r]; } return null; };
    // letters left out ("chrstinbach"): the server's gaps_span — in order from a word's start, over at most twice the word
    const gaps = w => { let best = null, pos = 0; const gs = W.map(x => { const at = pos; pos += x.s.length; return at; });
      for (const st of gs) { if (g[st] !== w[0]) continue; const hit = []; let i = st; for (const ch of w) { while (i < g.length && g[i] !== ch) i++; if (i >= g.length) break; hit.push(i++); }
        if (hit.length === w.length && hit[hit.length - 1] - st < 2 * w.length && (!best || hit[hit.length - 1] - st < best[best.length - 1] - best[0])) best = hit; }
      if (best) best.forEach(j => { on[gmap[j]] = true; }); return !!best; };
    for (const w of ws) { if (mark(f, map, w) || mark(g, gmap, w)) continue; let done = false; if (w.length >= 5) for (let k = 0; k < W.length - 1 && !done; k++) { const r = chain(w, k, true); if (r) { r.forEach(([at, l]) => { for (let j = at; j < at + l; j++) on[map[j]] = true; }); done = true; } } if (!done && w.length >= 6) gaps(w); }
    return on;
  }
  function paintMarks(text, on, a = 0, b = text.length) {
    let h = '', open = false;
    for (let i = a; i < b; i++) { if (on[i] !== open) { h += on[i] ? '<mark>' : '</mark>'; open = on[i]; } h += esc(text[i]); }
    return h + (open ? '</mark>' : '');
  }
  const hl = (text, ws) => { text = String(text ?? ''); return paintMarks(text, marksOf(text, ws)); };
  // a long text cut to about `max` characters around its first match
  function snip(text, ws, max = 90) {
    text = String(text ?? '').replace(/\s+/g, ' ').trim(); const on = marksOf(text, ws);
    if (text.length <= max) return paintMarks(text, on);
    const first = Math.max(0, on.indexOf(true)), a = Math.max(0, Math.min(first - 30, text.length - max)), b = Math.min(text.length, a + max);
    return (a ? '…' : '') + paintMarks(text, on, a, b) + (b < text.length ? '…' : '');
  }
  /* scrolling a list moves its rows under a resting pointer: every row it passes would repaint its hover
     tint and move the pick — measured as nearly all the paint work while scrolling. While a list scrolls
     its rows ignore the pointer (`.scrolling`), 150 ms after the last scroll they react again. */
  function calmScroll(list) { let t; list.addEventListener('scroll', () => { list.classList.add('scrolling'); clearTimeout(t); t = setTimeout(() => list.classList.remove('scrolling'), 150); }, { passive: true }); }
  // the pick moves between two rows only; the keyboard's pick scrolls it into view, the pointer's does not
  function movePick(list, i, scroll) { const old = $('.r.on', list); if (old) old.classList.remove('on'); const el = $(`[data-i="${i}"]`, list); if (el) { el.classList.add('on'); if (scroll) el.scrollIntoView({ block: 'nearest' }); } }
  const day = iso => { const [y, m, d] = iso.split('-').map(Number); return dfmt(new Date(y, m - 1, d), { weekday: 'short', day: 'numeric', month: 'short', year: 'numeric' }); };

  // ---- fetching: debounced, the stale request aborted, answers kept 30 s for back-typing ----
  const cache = new Map();
  let ctl = null;
  async function fetchQ(v, all, signal) {
    const k = (all ? '1' : '0') + v, c = cache.get(k);
    if (c && Date.now() - c.t < 30000) return c.d;
    const r = await practical.req('/api/search?q=' + encodeURIComponent(v) + (all ? '&all=1' : ''), { signal }); if (!r.ok) throw r; // the callers keep what is shown
    const d = r.data; cache.set(k, { t: Date.now(), d }); if (cache.size > 40) cache.delete(cache.keys().next().value);
    return d;
  }

  // ---- history (domain::search_history): loaded once, then kept here ----
  let H = null;
  const loadH = async () => { if (H) return H; const r = await practical.req('/api/search/history'); H = r.ok ? r.data : []; return H; };
  function record(v, target) {
    v = v.trim(); if (fold(v).length < 2) return;
    fetch('/api/search/history', { method: 'POST', keepalive: true, headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf }, body: JSON.stringify({ q: v, target: target || null }) })
      .then(r => r.json()).then(on => { if (!on || !H) return; const n = fold(v), old = H.find(e => fold(e.q) === n); H = H.filter(e => e !== old); H.unshift({ id: old ? old.id : 0, q: v, target: target || (old && old.target) || null, uses: (old ? old.uses : 0) + 1 }); if (!old) H = null; }).catch(() => {});
  }
  async function forget(id) { if (!practical.tell(await practical.req('/api/search/history/' + id, { method: 'DELETE' })).ok) return; if (H) H = H.filter(e => e.id !== id); }
  const histFor = v => { const n = fold(v.trim()); return (H || []).filter(e => { const f = fold(e.q); return !n || (f !== n && (f.startsWith(n) || f.includes(n))); }); };

  // ---- one hit → markup + what a pick does ----
  const KEEP = ['day', 'week', 'month', 'planner'];
  function targetOf(kind, x) {
    if (kind === 'jump') return { kind, date: x.date, view: x.view, label: x.label };
    if (kind === 'event') return { kind, key: x.key, date: x.date, label: x.summary };
    if (kind === 'contact') return { kind, key: x.key, label: x.name };
    return { kind: 'task', key: x.key, done: !!x.completed, label: x.summary };
  }
  function open(t) { // a result: the view on screen keeps it where it can (calendar/contacts/tasks), else its page
    if (t.kind === 'jump' || t.kind === 'event') {
      const view = t.kind === 'jump' ? t.view : 'day';
      if (window.rusticalCal) { const cur = window.rusticalCal.view(); window.rusticalCal.go(KEEP.includes(cur) ? cur : view, t.date); } else location.href = `/calendar?view=${view}&date=${t.date}`;
    } else if (t.kind === 'contact') { if (window.rusticalContacts) { history.replaceState(null, '', '#' + t.key); window.rusticalContacts.reveal(t.key); } else location.href = '/contacts#' + t.key; }
    else if (window.rusticalTasks) window.rusticalTasks.reveal(t.key); else location.href = `/tasks?view=groups${t.done ? '&back=all' : ''}#${encodeURIComponent(t.key)}`;
  }
  // why a hit was found (domain::search Why): per field its text around the match and how each word matched
  const how = h => h.k === 'typo' ? tr('js-search-k-typo', { m: h.m }) : tr('js-search-k-' + h.k);
  const typos = x => (x.why || []).flatMap(w => w.hits.filter(h => h.m).map(h => fold(h.m)));
  const why = (x, ws) => x.why && x.why.length ? `<span class="why"><b>${esc(T.why)}</b>${x.why.map(w => `<span><em>${esc(tr('js-search-f-' + w.f))}:</em> ${snip(w.s, ws.concat(typos(x)))} <i>(${esc(w.hits.map(h => (h.k === 'date' ? '' : '„' + h.w + '“ ') + how(h)).join(', '))})</i></span>`).join('')}</span>` : '';
  // the drop-down's short form: only what the row does not show — a field it hides, a typo
  const whySm = x => {
    const loose = h => h.k === 'typo' || h.k === 'gaps';
    const v = (x.why || []).filter(w => (!w.shown || w.hits.some(loose)) && !w.hits.some(h => h.k === 'date'))
      .map(w => { const t = w.hits.find(h => h.k === 'typo'); return esc(tr('js-search-f-' + w.f)) + (t ? ' ≈ ' + esc(t.m) : w.hits.some(loose) ? ' ≈' : ''); });
    return v.length ? `<span class="why sm">${ico('info')}${v.join(' · ')}</span>` : '';
  };
  function row(kind, x, ws) {
    if (kind === 'jump') return `<span class="ic">⇢</span><b>${esc(T.jump)} ${esc(x.label)}</b><small>${day(x.date)}</small>`;
    if (kind === 'event') return `<span class="sw" style="--c:${esc(x.color)}"></span><b>${hl(x.summary, ws)}</b><small>${day(x.date)}${x.time ? ' · ' + esc(x.time) : ''} · ${esc(x.cal_name)}${x.count > 1 ? ' · ' + ico('repeat') + esc(tr('js-search-series', { n: x.count })) : ''}</small>`;
    if (kind === 'contact') return `<span class="av" style="background:var(--c${x.color_index})">${esc(x.initials)}</span><b>${hl(x.name, ws)}</b><small>${x.bday ? ico('cake') : ''}${hl(x.sub, ws)}</small>`;
    return `<span class="sw" style="--c:${esc(x.color)}"></span><b>${hl(x.summary, ws)}</b><small>${x.due ? day(x.due) + ' · ' : ''}${esc(x.cal_name)}</small>`;
  }
  const HREF = t => t.kind === 'jump' || t.kind === 'event' ? `/calendar?view=${t.kind === 'jump' ? t.view : 'day'}&date=${t.date}` : t.kind === 'contact' ? '/contacts#' + t.key : '/tasks#' + t.key;
  const CATS = [['event', 'events', T.ev], ['contact', 'contacts', T.ct], ['task', 'tasks', T.tk]];
  // the drop-down's share: at most `cap` per category and `max` in all, each category by its share of the matches (at least one)
  function alloc(tot, cap = 5, max = 10) {
    const ks = Object.keys(tot), want = Object.fromEntries(ks.map(k => [k, Math.min(cap, tot[k])]));
    if (ks.reduce((a, k) => a + want[k], 0) <= max) return want;
    const all = ks.reduce((a, k) => a + tot[k], 0), out = {}; let used = 0;
    ks.forEach(k => { out[k] = tot[k] ? Math.min(want[k], Math.max(1, Math.floor(max * tot[k] / all))) : 0; used += out[k]; });
    while (used < max) { let best = null, bd = -Infinity; for (const k of ks) if (out[k] < want[k]) { const d = max * tot[k] / all - out[k]; if (d > bd) { bd = d; best = k; } } if (!best) break; out[best]++; used++; }
    return out;
  }

  // ---- the drop-down ----
  let items = [], sel = -1, last = null, data = null;
  const setSel = (i, scroll = true) => { sel = i; movePick(box, i, scroll); };
  calmScroll(box);
  function draw() {
    const v = q.value, d = data, ws = d ? wordsOf(v, d.scope) : [];
    items = []; let h = '', allSum = null;
    const add = (it, inner, cls = '') => { items.push(it); return `<a class="r${cls}" href="${esc(it.href || '#')}" data-i="${items.length - 1}" role="option">${inner}</a>`; };
    if (d && v.trim()) {
      if (d.scope) { const c = CATS.find(c => c[0] === d.scope); h += `<div class="sscope">${ico('filter')}${esc(tr('js-search-only', { what: c[2] }))}</div>`; }
      if (d.jump) { const t = targetOf('jump', d.jump); h += add({ t, href: HREF(t) }, row('jump', d.jump, ws), ' jump'); }
      const tot = Object.fromEntries(CATS.map(([, k]) => [k, d.totals[k]])), n = alloc(tot);
      for (const [kind, k, label] of CATS) {
        if (!n[k]) continue;
        h += `<h5>${esc(label)}<span>${n[k] < tot[k] ? esc(tr('js-search-of', { n: n[k], total: tot[k] })) : tot[k]}</span></h5>`;
        d[k].slice(0, n[k]).forEach(x => { const t = targetOf(kind, x); h += add({ t, href: HREF(t) }, row(kind, x, ws.concat(typos(x))) + whySm(x), x.completed ? ' done' : ''); });
      }
      const sum = CATS.reduce((a, [, k]) => a + tot[k], 0);
      if (!sum && !d.jump) h += `<div class="none">${esc(T.none)}</div>`;
      allSum = sum;
    }
    const hs = histFor(v).slice(0, v.trim() ? 3 : 6);
    if (hs.length) {
      h += `<h5>${esc(T.hist)}</h5>`;
      hs.forEach(e => { h += add({ hist: e }, `<span class="ic">${ico('history')}</span><b>${hl(e.q, wordsOf(v))}</b><small>${e.target ? esc(e.target.label || '') : ''}</small><button type="button" class="hx" data-hdel="${e.id}" title="${esc(T.histRm)}" aria-label="${esc(T.histRm)}">${ico('x')}</button>`, ' hist'); });
    }
    // always there with a query, pinned to the foot: the dialog also says why each hit was found
    if (allSum !== null) h += add({ all: true }, `<span class="ic">${ico('list')}</span><b>${esc(T.all)} (${allSum})</b><small>Enter</small>`, ' more');
    box.innerHTML = h; box.hidden = !h; sel = -1;
    drawGhost();
  }
  function drawGhost() { // the newest history entry the input is the start of
    const v = q.value, at = q.selectionStart === v.length;
    const e = v && at && (H || []).find(e => e.q.length > v.length && e.q.toLowerCase().startsWith(v.toLowerCase()));
    ghost.innerHTML = e ? `<span class="t">${esc(v)}</span>${esc(e.q.slice(v.length))}` : ''; ghost.dataset.i = e ? H.indexOf(e) : '';
  }
  async function refresh() {
    const v = q.value.trim(); await loadH();
    if (!v) { data = null; last = null; draw(); return; }
    if (v === last) { draw(); return; }
    if (ctl) ctl.abort(); ctl = new AbortController(); const mine = ctl;
    try { const d = await fetchQ(v, false, mine.signal); if (mine !== ctl) return; data = d; last = v; draw(); } catch (_) { /* aborted or offline: keep what is shown */ }
  }
  function pick(it, from) {
    if (!it) return;
    if (it.all) { openAll(from); return; }
    if (it.hist) { if (it.hist.target) { record(it.hist.q, it.hist.target); close(); q.value = it.hist.q; open(it.hist.target); } else { q.value = it.hist.q; q.focus(); refresh(); } return; }
    record(from, it.t); close(); open(it.t);
  }
  const close = () => { box.hidden = true; ghost.innerHTML = ''; };
  let t;
  q.addEventListener('input', () => { drawGhost(); clearTimeout(t); t = setTimeout(refresh, 180); });
  q.addEventListener('focus', () => { refresh(); });
  box.addEventListener('mousedown', e => e.preventDefault()); // a click keeps the focus in the field
  box.addEventListener('click', async e => {
    const x = e.target.closest('[data-hdel]'); if (x) { e.preventDefault(); e.stopPropagation(); await forget(+x.dataset.hdel); draw(); return; }
    const a = e.target.closest('[data-i]'); if (!a) return; e.preventDefault(); pick(items[+a.dataset.i], q.value);
  });
  box.addEventListener('mousemove', e => { const a = e.target.closest('[data-i]'); if (a && +a.dataset.i !== sel) setSel(+a.dataset.i, false); });
  q.addEventListener('keydown', e => {
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') { if (box.hidden) { refresh(); return; } e.preventDefault(); const n = items.length; if (n) setSel(e.key === 'ArrowDown' ? (sel + 1) % n : (sel <= 0 ? n : sel) - 1); }
    else if (e.key === 'Enter') { e.preventDefault(); if (sel >= 0) pick(items[sel], q.value); else if (q.value.trim()) { record(q.value); openAll(q.value); } }
    else if (e.key === 'Tab' && !e.shiftKey && ghost.dataset.i !== '' && ghost.innerHTML) { e.preventDefault(); const h = H[+ghost.dataset.i]; if (h) pick({ hist: h }, h.q); }
    else if (e.key === 'ArrowRight' && ghost.innerHTML && q.selectionStart === q.value.length) { e.preventDefault(); q.value = H[+ghost.dataset.i].q; refresh(); } // → takes the completion without going
    else if (e.key === 'Escape') { if (!box.hidden) { e.stopPropagation(); close(); } else q.blur(); }
  });
  q.addEventListener('blur', () => setTimeout(close, 120));
  document.addEventListener('click', e => { if (!e.target.closest('.usearch')) close(); });
  // / and Ctrl/⌘ K put the cursor in the field
  addEventListener('keydown', e => { if ((e.key === 'k' && (e.metaKey || e.ctrlKey)) && !e.altKey) { e.preventDefault(); q.focus(); q.select(); } });

  // ---- "all results": every hit, filtered by category ----
  function openAll(v) {
    close();
    const w = document.createElement('div'); w.className = 'ask sdlg-wrap';
    w.innerHTML = `<div class="ask-box sdlg" role="dialog" aria-modal="true" aria-label="${esc(T.dlg)}"><div class="sd-head"><div class="sfield"><input type="search" autocomplete="off" spellcheck="false" aria-label="${esc(T.dlg)}"></div><button class="btn icon-btn" type="button" data-x title="${esc(T.close)}" aria-label="${esc(T.close)}">${ico('x')}</button></div><div class="seg sd-tabs" role="group"></div><div class="sres sd-list" role="listbox" aria-label="${esc(T.dlg)}"></div><p class="sd-foot">${esc(T.keys)}</p></div>`;
    document.body.appendChild(w);
    const inp = $('input', w), tabs = $('.sd-tabs', w), list = $('.sd-list', w);
    let d = null, cat = 'all', its = [], s = -1, t2, c2 = null;
    inp.value = v;
    const mark = (i, scroll = true) => { s = i; movePick(list, i, scroll); };
    calmScroll(list);
    function paint() {
      if (!d) return; const ws = wordsOf(inp.value, d.scope); its = []; let h = '';
      const add = (it, inner, cls = '') => { its.push(it); return `<a class="r${cls}" href="${esc(it.href)}" data-i="${its.length - 1}" role="option">${inner}</a>`; };
      const sum = CATS.reduce((a, [, k]) => a + d.totals[k], 0);
      tabs.innerHTML = `<button type="button" data-cat="all" aria-pressed="${cat === 'all'}">${esc(T.tabAll)} <span class="n">${sum}</span></button>` + CATS.map(([kind, k, label]) => `<button type="button" data-cat="${kind}" aria-pressed="${cat === kind}"${d.totals[k] ? '' : ' disabled'}>${esc(label)} <span class="n">${d.totals[k]}</span></button>`).join('');
      if (d.jump && cat === 'all') { const t = targetOf('jump', d.jump); h += add({ t, href: HREF(t) }, row('jump', d.jump, ws), ' jump'); }
      for (const [kind, k, label] of CATS) {
        if ((cat !== 'all' && cat !== kind) || !d[k].length) continue;
        h += `<h5>${esc(label)}<span>${d[k].length < d.totals[k] ? esc(tr('js-search-of', { n: d[k].length, total: d.totals[k] })) : d.totals[k]}</span></h5>`;
        d[k].forEach(x => { const t = targetOf(kind, x); h += add({ t, href: HREF(t) }, row(kind, x, ws.concat(typos(x))) + why(x, ws), x.completed ? ' done' : ''); });
      }
      list.innerHTML = h || `<div class="none">${esc(T.none)}</div>`; s = -1;
    }
    async function load() {
      const v2 = inp.value.trim(); if (!v2) { d = { totals: { events: 0, contacts: 0, tasks: 0 }, events: [], contacts: [], tasks: [] }; paint(); return; }
      if (c2) c2.abort(); c2 = new AbortController(); const mine = c2;
      try { const r = await fetchQ(v2, true, mine.signal); if (mine !== c2) return; d = r; if (cat !== 'all' && !d.totals[CATS.find(c => c[0] === cat)[1]]) cat = 'all'; paint(); } catch (_) {}
    }
    const end = () => { removeEventListener('keydown', key, true); w.remove(); q.focus(); };
    const choose = it => { if (!it) return; record(inp.value, it.t); end(); q.value = inp.value; close(); open(it.t); };
    const key = e => {
      if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); end(); }
      else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') { e.preventDefault(); const n = its.length; if (n) mark(e.key === 'ArrowDown' ? (s + 1) % n : (s <= 0 ? n : s) - 1); }
      else if (e.key === 'Enter' && document.activeElement === inp) { e.preventDefault(); choose(its[s >= 0 ? s : 0]); }
    };
    addEventListener('keydown', key, true);
    inp.addEventListener('input', () => { clearTimeout(t2); t2 = setTimeout(load, 180); });
    tabs.addEventListener('click', e => { const b = e.target.closest('[data-cat]'); if (!b || b.disabled) return; cat = b.dataset.cat; paint(); inp.focus(); });
    list.addEventListener('mousedown', e => e.preventDefault());
    list.addEventListener('click', e => { const a = e.target.closest('[data-i]'); if (!a) return; e.preventDefault(); choose(its[+a.dataset.i]); });
    list.addEventListener('mousemove', e => { const a = e.target.closest('[data-i]'); if (a && +a.dataset.i !== s) mark(+a.dataset.i, false); });
    w.addEventListener('click', e => { if (e.target === w || e.target.closest('[data-x]')) end(); });
    inp.focus(); inp.select(); load();
  }
})();
