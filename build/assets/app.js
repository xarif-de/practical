/* rustical-ui — shared behaviour (vanilla ES2020, no framework). */
(function () {
  const $ = (s, r = document) => r.querySelector(s);
  const $$ = (s, r = document) => [...r.querySelectorAll(s)];
  const csrf = () => ($('meta[name=csrf]') || {}).content || '';

  // development only (body[data-dev]): reload after a rebuild (new boot id) or an asset edit
  if (document.body.dataset.dev) {
    let boot = sessionStorage.devBoot; const connect = () => {
      const es = new EventSource('/dev/reload');
      es.addEventListener('boot', ev => { if (boot && boot !== ev.data) location.reload(); boot = sessionStorage.devBoot = ev.data; });
      es.addEventListener('reload', () => location.reload());
      es.onerror = () => { es.close(); setTimeout(connect, 700); };
    };
    connect();
  }

  // fixed page header: the toolbar sticks under the topbar, sidebars under both — measured heights
  // (both bars wrap on narrow screens; the calendar toolbar is replaced with every fragment swap)
  const setBars = () => { const st = document.documentElement.style, tb = $('.topbar'), tl = $('.toolbar'); if (tb) st.setProperty('--topbar-h', tb.offsetHeight + 'px'); if (tl) st.setProperty('--toolbar-h', tl.offsetHeight + 'px'); };
  if ('ResizeObserver' in window) {
    const ro = new ResizeObserver(setBars);
    const watch = () => { ro.disconnect(); ['.topbar', '.toolbar'].forEach(s => { const e = $(s); if (e) ro.observe(e); }); setBars(); };
    watch(); const mainEl = $('.main'); if (mainEl) new MutationObserver(watch).observe(mainEl, { childList: true, subtree: true });
  } else { setBars(); addEventListener('resize', setBars); }

  const h = document.documentElement;
  const sb = $('#scheme');
  if (sb) sb.addEventListener('click', () => { h.dataset.scheme = h.dataset.scheme === 'dark' ? 'light' : 'dark'; localStorage.scheme = h.dataset.scheme; });

  // confirmations on destructive forms
  $$('[data-confirm]').forEach(b => b.addEventListener('click', e => { if (!confirm(b.dataset.confirm)) e.preventDefault(); }));

  // copy-to-clipboard
  $$('[data-copy]').forEach(b => b.addEventListener('click', async () => {
    const el = $(b.dataset.copy); if (!el) return;
    try { await navigator.clipboard.writeText(el.textContent.trim()); b.textContent = '✓'; } catch (_) { const r = document.createRange(); r.selectNodeContents(el); getSelection().removeAllRanges(); getSelection().addRange(r); }
  }));

  // settings tabs (hash-driven; without JS every panel is visible)
  const settings = $('.settings');
  if (settings) {
    const tabs = $$('.tabs a');
    const show = id => {
      settings.classList.add('tabbed');
      $$('.panel', settings).forEach(p => p.classList.toggle('on', p.id === id));
      tabs.forEach(a => { if (a.getAttribute('href') === '#' + id) a.setAttribute('aria-current', 'page'); else a.removeAttribute('aria-current'); });
    };
    const fromHash = () => { const id = location.hash.slice(1); show(id && $('#' + CSS.escape(id), settings) ? id : 'profile'); }; // empty hash → '#' is not a selector
    tabs.forEach(a => a.addEventListener('click', e => { e.preventDefault(); history.replaceState(null, '', a.getAttribute('href')); fromHash(); }));
    addEventListener('hashchange', fromHash); fromHash();
    // a flash after a failed confirmation should keep the details open
    $$('.flash.error').forEach(() => $$('details.danger-zone', settings).forEach(d => d.open = true));
  }

  // user menu: click toggles (hover works via CSS, touch needs the click)
  $$('.filter > button').forEach(b => b.addEventListener('click', e => { e.stopPropagation(); b.parentElement.classList.toggle('open'); }));
  document.addEventListener('click', e => { if (!e.target.closest('.filter')) $$('.filter.open').forEach(f => f.classList.remove('open')); });

  // "/" focuses the search
  const q = $('#q');
  if (q) addEventListener('keydown', e => { if (e.key === '/' && document.activeElement !== q && !/input|textarea|select/i.test(document.activeElement.tagName)) { e.preventDefault(); q.focus(); } });

  // live updates: server-sent changes → custom event for the views
  if (document.body.dataset.user && 'EventSource' in window) {
    let es; const connect = () => {
      es = new EventSource('/api/changes');
      es.addEventListener('change', ev => { try { document.dispatchEvent(new CustomEvent('rustical:change', { detail: JSON.parse(ev.data) })); } catch (_) {} });
      es.onerror = () => { es.close(); setTimeout(connect, 5000 + Math.random() * 5000); };
    };
    connect();
  }

  window.rusticalUi = { csrf, $, $$ };
})();

/* ===== calendar page ===== */
(function () {
  const root = document.getElementById('viewroot');
  if (!root) return;
  const $ = (s, r = document) => r.querySelector(s), $$ = (s, r = document) => [...r.querySelectorAll(s)];
  const csrf = document.querySelector('meta[name=csrf]').content;
  const lang = document.body.dataset.lang || 'de';
  const MON = lang === 'en' ? ['January','February','March','April','May','June','July','August','September','October','November','December'] : ['Januar','Februar','März','April','Mai','Juni','Juli','August','September','Oktober','November','Dezember'];
  const DOW = lang === 'en' ? ['Mon','Tue','Wed','Thu','Fri','Sat','Sun'] : ['Mo','Di','Mi','Do','Fr','Sa','So'];
  const T = lang === 'en' ? {suggNone:'No suggestions', saved:'saved', err:'Error', del:'Delete this event?', series:'This is a recurring event; the whole series is changed.', newEv:'New event', editEv:'Edit event', conflict:'Changed elsewhere — reload.'} : {suggNone:'Keine Vorschläge', saved:'gespeichert', err:'Fehler', del:'Diesen Termin löschen?', series:'Serientermin — Änderungen gelten für die ganze Serie.', newEv:'Neuer Termin', editEv:'Termin bearbeiten', conflict:'Anderswo geändert — bitte neu laden.'};
  const iso = d => d.toISOString().slice(0, 10);
  const parse = s => { const [y, m, d] = s.split('-').map(Number); return new Date(y, m - 1, d); };
  const pad = n => String(n).padStart(2, '0');
  let state = { view: root.dataset.view, date: root.dataset.date, cals: root.dataset.cals || '' };
  const today = root.dataset.today;

  // ---- filter (localStorage-persisted set of hidden calendars) ----
  const filterBox = $('#calfilter');
  const hidden = new Set(JSON.parse(localStorage.calHidden || '[]'));
  function calsParam() { const all = $$('input[data-cal]', filterBox).map(i => i.dataset.cal); const on = all.filter(c => !hidden.has(c)); return on.length === all.length ? '' : on.join(','); }
  if (filterBox) {
    $$('input[data-cal]', filterBox).forEach(i => { i.checked = !hidden.has(i.dataset.cal); i.addEventListener('change', () => { if (i.checked) hidden.delete(i.dataset.cal); else hidden.add(i.dataset.cal); persistFilter(); load(); }); });
    $$('[data-filter]', filterBox).forEach(a => a.addEventListener('click', e => { e.preventDefault(); const all = $$('input[data-cal]', filterBox); all.forEach(i => { const mine = !i.parentElement.querySelector('small'); i.checked = a.dataset.filter === 'all' || (a.dataset.filter === 'mine' && mine); if (i.checked) hidden.delete(i.dataset.cal); else hidden.add(i.dataset.cal); }); persistFilter(); load(); }));
    state.cals = calsParam();
    if (state.cals !== (root.dataset.cals || '')) load(false);
  }
  function persistFilter() { localStorage.calHidden = JSON.stringify([...hidden]); state.cals = calsParam(); }

  // ---- navigation / fragment loading ----
  function url(fragment) { const u = new URL('/calendar', location.origin); u.searchParams.set('view', state.view); u.searchParams.set('date', state.date); if (state.cals) u.searchParams.set('cals', state.cals); if (fragment) u.searchParams.set('fragment', '1'); return u; }
  let loading = null;
  async function load(push = true) {
    const u = url(true);
    loading = u;
    try {
      const r = await fetch(u, { headers: { Accept: 'text/html' } });
      if (loading !== u) return;
      if (!r.ok) throw new Error(r.status);
      const html = await r.text(); tlScroll = $('.tl', root) ? $('.tl', root).scrollTop : null;
      root.innerHTML = html;
      root.dataset.view = state.view; root.dataset.date = state.date;
      const h1 = root.querySelector('#title'); if (h1) document.title = h1.textContent.trim() + ' · ' + document.title.split(' · ').slice(1).join(' · ');
      if (push) history.pushState(state, '', url(false).pathname + url(false).search);
      wire();
    } catch (e) { console.error(e); }
  }
  addEventListener('popstate', e => { if (e.state) { state = e.state; load(false); } });
  function shift(dir) {
    const d = parse(state.date);
    if (state.view === 'day') d.setDate(d.getDate() + dir);
    else if (state.view === 'week') d.setDate(d.getDate() + 7 * dir);
    else if (['month'].includes(state.view)) { d.setDate(1); d.setMonth(d.getMonth() + dir); }
    else d.setFullYear(d.getFullYear() + dir);
    state.date = iso(new Date(d.getTime() - d.getTimezoneOffset() * 60000));
    load();
  }
  function go(view, date) { if (view) state.view = view; if (date) state.date = date; load(); }

  // ---- timeline (right rail): client-rendered, endless in both directions; the SVG node and the
  // scroll position survive fragment swaps of the same kind, so selecting never re-centres ----
  const tip = document.createElement('div'); tip.className = 'tip'; tip.hidden = true; document.body.appendChild(tip);
  const TLK = { weeks: { step: 30, y0: 16, r: 3.5, chunk: 26 }, months: { step: 24, y0: 14, r: 3.5, chunk: 24 } };
  const fmt = d => d.getFullYear() + '-' + String(d.getMonth() + 1).padStart(2, '0') + '-' + String(d.getDate()).padStart(2, '0');
  const dm = d => lang === 'en' ? `${d.getMonth() + 1}/${d.getDate()}` : `${d.getDate()}.${d.getMonth() + 1}.`;
  const isoWeek = d => { const t = new Date(Date.UTC(d.getFullYear(), d.getMonth(), d.getDate())); const day = t.getUTCDay() || 7; t.setUTCDate(t.getUTCDate() + 4 - day); return Math.ceil(((t - Date.UTC(t.getUTCFullYear(), 0, 1)) / 864e5 + 1) / 7); };
  const weekStart = d => { const w = new Date(d); w.setDate(w.getDate() - ((w.getDay() + 6) % 7)); return w; };
  const stepDate = (kind, d, n) => { const x = new Date(d); if (kind === 'weeks') x.setDate(x.getDate() + 7 * n); else x.setMonth(x.getMonth() + n); return x; };
  const periodOf = (kind, d) => kind === 'weeks' ? weekStart(d) : new Date(d.getFullYear(), d.getMonth(), 1);
  let tlSvg = null, tlKind = null, tlFirst = null, tlLast = null, tlItems = [], tlScroll = null;
  // items: one per period, optional line before it (month start in weeks, January in months)
  function tlBuild(kind) {
    const k = TLK[kind], todayD = parse(today), nowP = fmt(periodOf(kind, todayD));
    let y = k.y0, out = '', items = [];
    for (let d = new Date(tlFirst); d <= tlLast; d = stepDate(kind, d, 1)) {
      if (kind === 'weeks') {
        const first = [0, 1, 2, 3, 4, 5, 6].map(i => { const x = new Date(d); x.setDate(x.getDate() + i); return x; }).find(x => x.getDate() === 1);
        if (first) { const lbl = lang === 'en' ? `${MON[first.getMonth()]} 1, ${first.getFullYear()}` : `1. ${MON[first.getMonth()]} ${first.getFullYear()}`; out += `<g class="mline" data-y="${y}" data-date="${fmt(first)}" data-label="${lbl}"><line x1="18" x2="46" y1="${y}" y2="${y}"/><text x="50" y="${y + 4}">${first.getMonth() === 0 ? first.getFullYear() : MON[first.getMonth()].slice(0, 3)}</text></g>`; items.push({ y, date: fmt(first), label: lbl }); y += k.step; }
        const e = new Date(d); e.setDate(e.getDate() + 6);
        const lbl = `KW ${isoWeek(d)} · ${dm(d)} – ${dm(e)}`, ds = fmt(d);
        out += `<g class="wdot${ds === nowP ? ' now' : ''}" data-y="${y}" data-date="${ds}" data-label="${lbl}"><circle cx="32" cy="${y}" r="${k.r}"/></g>`;
        items.push({ y, date: ds, label: lbl, period: ds }); y += k.step;
      } else {
        if (d.getMonth() === 0) { out += `<g class="mline" data-y="${y}" data-date="${fmt(d)}" data-label="${d.getFullYear()}"><line x1="18" x2="46" y1="${y}" y2="${y}"/><text x="50" y="${y + 4}">${d.getFullYear()}</text></g>`; items.push({ y, date: fmt(d), label: String(d.getFullYear()) }); y += k.step; }
        const lbl = `${MON[d.getMonth()]} ${d.getFullYear()}`, ds = fmt(d);
        out += `<g class="wdot${ds === nowP ? ' now' : ''}" data-y="${y}" data-date="${ds}" data-label="${lbl}"><circle cx="32" cy="${y}" r="${k.r}"/></g>`;
        items.push({ y, date: ds, label: lbl, period: ds }); y += k.step;
      }
    }
    tlSvg.setAttribute('height', y); tlSvg.setAttribute('viewBox', `0 0 88 ${y}`);
    tlSvg.innerHTML = out + '<circle class="sel" cx="32" r="7.5" hidden/><circle class="pre" cx="32" r="7.5" hidden/>';
    tlItems = items; tlMark();
    return y;
  }
  const svgShow = (el, on) => on ? el.removeAttribute('hidden') : el.setAttribute('hidden', ''); // SVG elements have no .hidden property
  function tlMark() { // selection ring on the viewed period
    if (!tlSvg) return; const sel = tlSvg.querySelector('.sel'); const p = fmt(periodOf(tlKind, parse(state.date)));
    const it = tlItems.find(i => i.period === p); if (it) sel.setAttribute('cy', it.y); svgShow(sel, !!it);
  }
  function tlExtend(box) { // keep two chunks of slack above and below the viewport
    const k = TLK[tlKind]; let changed = false;
    if (box.scrollTop < 300) { tlFirst = stepDate(tlKind, tlFirst, -k.chunk); const before = tlSvg.getBoundingClientRect().height; const h = tlBuild(tlKind); box.scrollTop += h - before; changed = true; }
    if (box.scrollTop + box.clientHeight > box.scrollHeight - 300) { tlLast = stepDate(tlKind, tlLast, k.chunk); tlBuild(tlKind); changed = true; }
    return changed;
  }
  function mountTimeline(box) {
    if (!box) return;
    const kind = box.dataset.kind, center = periodOf(kind, parse(box.dataset.center));
    if (tlSvg && tlKind === kind) { box.appendChild(tlSvg); if (tlScroll != null) box.scrollTop = tlScroll; tlMark(); }
    else {
      tlKind = kind; tlFirst = stepDate(kind, center, -TLK[kind].chunk); tlLast = stepDate(kind, center, TLK[kind].chunk);
      tlSvg = document.createElementNS('http://www.w3.org/2000/svg', 'svg'); tlSvg.setAttribute('width', 88); box.appendChild(tlSvg); tlBuild(kind);
      const it = tlItems.find(i => i.period === fmt(center)); box.scrollTop = it ? Math.max(0, it.y - box.clientHeight / 2) : 0;
    }
    const nearest = e => { const top = tlSvg.getBoundingClientRect().top; const y = e.clientY - top; let best = null; for (const i of tlItems) if (!best || Math.abs(i.y - y) < Math.abs(best.y - y)) best = i; return best; };
    box.onmousemove = e => { const it = nearest(e); if (!it) return; const pre = tlSvg.querySelector('.pre'); pre.setAttribute('cy', it.y); svgShow(pre, true); tip.textContent = it.label; tip.hidden = false; tip.style.left = e.clientX + 'px'; tip.style.top = e.clientY + 'px'; };
    box.onmouseleave = () => { tip.hidden = true; const pre = tlSvg.querySelector('.pre'); if (pre) svgShow(pre, false); };
    box.onclick = e => { const it = nearest(e); if (it) { tip.hidden = true; go(null, it.date); } };
    box.onscroll = () => { if (!box._busy) { box._busy = true; requestAnimationFrame(() => { tlExtend(box); box._busy = false; }); } };
    tlExtend(box);
  }
  function wire() {
    $$('[data-nav]', root).forEach(b => b.onclick = () => b.dataset.nav === 'today' ? go(null, today) : shift(b.dataset.nav === 'next' ? 1 : -1));
    $$('.toolbar [data-view]', root).forEach(b => b.onclick = e => { e.preventDefault(); go(b.dataset.view, null); });
    const more = $('.more', root); if (more) { $('[data-more]', more).onclick = e => { e.stopPropagation(); more.classList.toggle('open'); }; }
    mountTimeline($('.tl', root));
    $$('.heat i[data-date], .c[data-date], .we[data-date]', root).forEach(el => el.onclick = () => go(state.view === 'week' ? 'week' : 'day', el.dataset.date));
    $$('.lrow[data-date]', root).forEach(el => el.onclick = e => { if (e.target.classList.contains('add')) return; e.preventDefault(); go('day', el.dataset.date); });
    $$('.day[data-date] .n, .more[data-date]', root).forEach(el => el.onclick = () => go('day', el.closest('[data-date]').dataset.date));
    $$('.dayblock .dl', root).forEach(el => el.onclick = () => go('day', el.closest('[data-date]').dataset.date));
    $$('[data-key]', root).forEach(el => el.addEventListener('click', e => { e.stopPropagation(); openEditor(el.dataset.key, el.dataset.date); }));
    $$('.add', root).forEach(b => b.onclick = e => { e.stopPropagation(); e.preventDefault(); const d = b.closest('[data-date]'); openEditor(null, d ? d.dataset.date : state.date); });
    const title = $('#title', root); if (title) title.onclick = openPicker;
  }
  document.addEventListener('click', e => { if (!e.target.closest('.more')) $$('.more.open').forEach(m => m.classList.remove('open')); });
  document.addEventListener('rustical:change', () => { clearTimeout(load._t); load._t = setTimeout(() => load(false), 800); });
  $('#fab').onclick = () => openEditor(null, state.date);

  // ---- orbit picker ----
  const pk = $('#picker');
  let pkYear = +pk.dataset.year, pkMonth = +pk.dataset.month, pkBase = +pk.dataset.year;
  const yn = $$('.ynode', pk);
  function py() { yn.forEach((g, i) => { const y = pkBase + (8 - i); g.dataset.year = y; g.querySelector('text').textContent = y; g.classList.toggle('cur', y === pkYear); }); }
  function pm() { $$('.mnode', pk).forEach(g => g.classList.toggle('cur', +g.dataset.month === pkMonth)); $('#pk-title').textContent = MON[pkMonth - 1] + ' ' + pkYear; grid(); }
  function grid() {
    const g = $('#pk-grid'); let h = DOW.map(d => `<div class="dow">${d}</div>`).join('');
    const first = new Date(pkYear, pkMonth - 1, 1); const start = new Date(first); start.setDate(1 - ((first.getDay() + 6) % 7));
    for (let i = 0; i < 42; i++) { const d = new Date(start); d.setDate(start.getDate() + i); if (i >= 35 && d.getMonth() !== pkMonth - 1) break; const s = `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`; h += `<div class="day${d.getMonth() !== pkMonth - 1 ? ' other' : ''}${s === today ? ' today' : ''}" data-date="${s}"><span class="n">${d.getDate()}</span></div>`; }
    g.innerHTML = h;
    $$('.day', g).forEach(d => d.onclick = () => { pk.hidden = true; go(state.view === 'week' || state.view === 'day' ? state.view : (['planner','weekends','vacation','countdown','numbers'].includes(state.view) ? state.view : 'month'), d.dataset.date); });
  }
  async function density() { try { const r = await fetch(`/api/density?year=${pkYear}${state.cals ? '&cals=' + encodeURIComponent(state.cals) : ''}`); const d = await r.json(); const mx = Math.max(1, ...d); $$('.mnode', pk).forEach(g => { const m = +g.dataset.month; if (!g.classList.contains('cur')) g.querySelector('circle').setAttribute('r', 14 + 10 * d[m - 1] / mx); }); } catch (_) {} }
  function openPicker() { const d = parse(state.date); pkYear = d.getFullYear(); pkMonth = d.getMonth() + 1; pkBase = pkYear; py(); pm(); density(); pk.hidden = false; }
  yn.forEach(g => g.onclick = () => { pkYear = +g.dataset.year; py(); pm(); density(); });
  $$('.mnode', pk).forEach(g => g.onclick = () => { pkMonth = +g.dataset.month; pm(); });
  $('.ybtn.prev', pk).onclick = () => { pkBase -= 7; py(); }; $('.ybtn.next', pk).onclick = () => { pkBase += 7; py(); };
  $('.ybtn.reset', pk).onclick = () => { const d = parse(today); pkYear = d.getFullYear(); pkMonth = d.getMonth() + 1; pkBase = pkYear; py(); pm(); density(); };
  $('#pk-close').onclick = () => pk.hidden = true; pk.addEventListener('click', e => { if (e.target === pk) pk.hidden = true; });

  // ---- editor: a card form like the contact editor — view mode behind the lock, cards shown only
  // when they hold data, "add card" menu in edit mode, icon buttons, dirty tracking ----
  const ed = $('#ed'), form = $('#edform'), f = form.elements, D = form.dataset;
  let current = null, openDate = null, baseline = '';
  const presets = (() => { try { return JSON.parse($('#presets-data').textContent || '[]'); } catch (_) { return []; } })();
  const presetBox = $('#ed-presets');
  if (presets.length) presetBox.innerHTML = presets.map((p, i) => `<button type="button" class="btn" data-preset="${i}">${p.title}</button>`).join('');
  $$('[data-preset]', presetBox).forEach(b => b.onclick = () => { const p = presets[+b.dataset.preset]; if (p.title) f.summary.value = p.title; if (p.calendar) f.calendar.value = p.calendar; if (p.category) { addCard('cats'); addChip(p.category); } if (p.all_day) setAllDay(true); syncDot(); refresh(); });
  function setAllDay(on) { f.all_day.checked = on; $('#ed-allday').classList.toggle('on', on); f.from.disabled = on; f.to.disabled = on; $('#ed-times').hidden = on; }
  $('#ed-allday').addEventListener('click', e => { e.preventDefault(); setAllDay(!f.all_day.checked); refresh(); });
  function syncDot() { const o = f.calendar.selectedOptions[0]; $('#ed-dot').style.background = f.color_cal.checked ? (o ? o.dataset.color : '') : f.color.value; $('#ed-sub').textContent = o ? o.textContent : ''; if (f.color_cal.checked && o) f.color.value = toHex(o.dataset.color); }
  const tl = $('#ed-tl'); function setTl(on) { ed.classList.toggle('no-tl', !on); tl.setAttribute('aria-pressed', on ? 'true' : 'false'); localStorage.edtl = on ? '1' : '0'; }
  tl.onclick = () => setTl(ed.classList.contains('no-tl')); setTl(localStorage.edtl !== '0');
  // the dialog closes only through its buttons; unsaved edits ask first (a click on the backdrop does nothing)
  function close() { ed.hidden = true; $('#ed-confirm').hidden = true; }
  const dirty = () => form.classList.contains('mode-edit') && serialize() !== baseline;
  function requestClose() { if (dirty()) { $('#ed-confirm').hidden = false; $('#ed-back').focus(); } else close(); }
  $('#ed-close').onclick = requestClose; $('#ed-back').onclick = () => { $('#ed-confirm').hidden = true; }; $('#ed-discard').onclick = close;
  addEventListener('keydown', e => { if (e.key === 'Escape') { if (!ed.hidden) requestClose(); pk.hidden = true; } });
  const fmtD = s => { const d = parse(s.slice(0, 10)); return `${DOW[(d.getDay() + 6) % 7]}, ${pad(d.getDate())}.${pad(d.getMonth() + 1)}.${d.getFullYear()}`; };
  const fmtDT = s => fmtD(s) + ' ' + s.slice(11, 16);
  // categories as chips (same widget as the contact editor)
  const chips = () => $$('.tags .chip', form).map(c => c.firstChild.textContent.trim());
  const addChip = v => { v = v.replace(/,/g, '').trim(); if (!v || chips().includes(v)) return; $('.tags input', form).insertAdjacentHTML('beforebegin', `<span class="chip">${v.replace(/</g, '&lt;')}<b class="edits">×</b></span>`); };
  const tagIn = $('.tags input', form);
  tagIn.addEventListener('keydown', e => { if (e.key === 'Enter' || e.key === ',') { e.preventDefault(); addChip(tagIn.value); tagIn.value = ''; refresh(); } else if (e.key === 'Backspace' && !tagIn.value) { const c = tagIn.previousElementSibling; if (c && c.classList.contains('chip')) { c.remove(); refresh(); } } });
  tagIn.addEventListener('blur', () => { if (tagIn.value.trim()) { addChip(tagIn.value); tagIn.value = ''; refresh(); } });
  // ---- list rows from <template>: reminders, people, exceptions, extra dates ----
  const rows = list => $$(`[data-list="${list}"] .row`, form);
  const addRow = list => { const box = $(`[data-list="${list}"]`, form); box.appendChild($('template', box).content.cloneNode(true)); return box.lastElementChild; };
  const w = (row, k) => $(`[data-w=${k}]`, row);
  const setSel = (sel, v, unit = '') => { v = String(v); if (![...sel.options].some(o => o.value === v)) sel.add(new Option(v + unit, v)); sel.value = v; };
  // ---- reminders (design K): a model of alarms rendered as a timeline strip (markers snap to the
  // usual offsets), a bar of per-user preset chips (a click adds or removes the matching alarm) and
  // one expandable entry per alarm whose settings follow the chosen kind and trigger. The model is
  // mirrored into the hidden `alarms_json` input so dirty tracking and reset work like for any field. ----
  const AL = $('#ed-alarm-card'), AT = AL.dataset, alBox = $('[data-list="alarms"]', AL), tline = $('#ed-tline'), apre = $('#ed-apre');
  let ALARMS = [], alSeq = 0;
  const AL_ACTS = ['DISPLAY', 'AUDIO', 'EMAIL'], AL_ICO = { DISPLAY: '#i-bell', AUDIO: '#i-sound', EMAIL: '#i-mail' }, AL_PROX = ['ARRIVE', 'DEPART', 'CONNECT', 'DISCONNECT'];
  const SNAP_START = [2, 5, 15, 30, 60, 180, 480, 1440, 2880, 4320, 10080, 20160], SNAP_END_ALL = [5, 10, 15, 30, 60], AL_UNITS = [1, 60, 1440, 10080, 43200]; // a "month" is 30 days: iCalendar durations have no months
  const DEFAULT_PRESETS = [0, 5, 10, 15, 30, 60, 120, 1440, 2880, 10080].map(m => ({ action: 'DISPLAY', minutes: m, related_end: false, repeat: 0, duration_minutes: 0, attendees: [], attach: '', proximity: '' }));
  let PRESETS = (() => { try { const p = JSON.parse($('#alarm-presets-data').textContent || 'null'); return Array.isArray(p) ? p : DEFAULT_PRESETS; } catch (_) { return DEFAULT_PRESETS; } })();
  const newAlarm = o => ({ idx: null, action: 'DISPLAY', minutes: 15, related_end: false, at: null, description: '', summary: '', repeat: 0, duration_minutes: 0, attendees: [], attach: '', proximity: '', acknowledged: null, ...o, _id: ++alSeq });
  // a preset is the setting without text, subject and acknowledgement; an absolute trigger cannot be one
  const presetSig = a => a.at ? null : JSON.stringify([!!a.related_end, a.minutes, a.action, a.repeat, a.duration_minutes, a.proximity || '', a.attach || '', [...(a.attendees || [])].sort()]);
  const fromPreset = p => newAlarm({ action: p.action, minutes: p.minutes, related_end: !!p.related_end, repeat: p.repeat || 0, duration_minutes: p.duration_minutes || 0, attendees: [...(p.attendees || [])], attach: p.attach || '', proximity: p.proximity || '' });
  const matchesPreset = a => !a.at && PRESETS.some(p => presetSig(p) === presetSig(a));
  async function savePresets() { try { const r = await fetch('/api/alarm-presets', { method: 'PUT', headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf }, body: JSON.stringify(PRESETS) }); if (!r.ok) throw new Error(r.status); } catch (e) { $('#ed-error').textContent = T.err + ': ' + e.message; } }
  const amount = m => { m = Math.abs(m); if (m && m % 10080 === 0) return m === 10080 ? AT.tWeek : AT.tWeeks.replace('{n}', m / 10080); if (m && m % 1440 === 0) return m === 1440 ? D.tDay : D.tDays.replace('{n}', m / 1440); if (m && m % 60 === 0) return m === 60 ? D.tHour : D.tHours.replace('{n}', m / 60); return D.tMin.replace('{n}', m); };
  const whenText = a => { if (a.at) return fmtDT(a.at); if (a.minutes === 0) return a.related_end ? AT.tAtEndTxt : AT.tAtStart; return amount(a.minutes) + ' ' + (a.minutes > 0 ? (a.related_end ? D.tBeforeEnd : D.tBefore) : (a.related_end ? AT.tAfterEnd : AT.tAfter)); };
  const proxText = a => a.proximity ? AT.tProxVals.split('|')[AL_PROX.indexOf(a.proximity)] || a.proximity : '';
  const alarmText = a => { let t = whenText(a); const acts = D.tActs.split('|'); if (a.action === 'AUDIO') t += ' · ' + acts[1]; if (a.action === 'EMAIL') t += ' · ' + acts[2]; if (a.repeat > 0) t += ' · ' + D.tRepeat.replace('{n}', a.repeat) + (a.duration_minutes ? ' ' + AT.tRepeatEvery + ' ' + amount(a.duration_minutes) : ''); if (a.proximity) t += ' · ' + proxText(a); if (a.action === 'EMAIL' && a.attendees.length) t += ' · ' + AT.tToTxt + ' ' + a.attendees.join(', '); if (a.description && a.description !== f.summary.value) t += ' – ' + a.description; if (a.acknowledged) t += ' · ' + AT.tAcked; return t; };
  const extrasOf = a => [a.repeat > 0 ? D.tRepeat.replace('{n}', a.repeat) + (a.duration_minutes ? ' ' + AT.tRepeatEvery + ' ' + amount(a.duration_minutes) : '') : '', proxText(a), a.action === 'EMAIL' ? (a.attendees.length ? AT.tToTxt + ' ' + a.attendees.join(', ') : AT.tToMissing) : '', a.acknowledged ? AT.tAcked : '', a.description ? '„' + a.description + '“' : ''].filter(Boolean).join(' · ');
  const esc = s => String(s).replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]);
  const alIco = id => `<svg class="ico" aria-hidden="true"><use href="${id}"/></svg>`;
  const unitOf = m => { m = Math.abs(m); for (const u of [...AL_UNITS].reverse()) if (m && m % u === 0) return u; return 1; };
  const fmtNum = n => String(Math.round(n * 100) / 100).replace('.', ',');
  // the event on the strip: start and length in minutes from the "when" card
  const evStart = () => new Date((f.date.value || today) + 'T' + (f.all_day.checked ? '00:00' : (f.from.value || '00:00')) + ':00');
  const evLen = () => { const s = evStart(); const e = f.all_day.checked ? new Date(nextDay(f.end_date.value || f.date.value || today) + 'T00:00:00') : new Date((f.end_date.value || f.date.value || today) + 'T' + (f.to.value || f.from.value || '00:00') + ':00'); return Math.max(1, Math.round((e - s) / 60000)); };
  const minutesBefore = a => a.at ? Math.round((evStart() - new Date(a.at)) / 60000) : a.related_end ? a.minutes - evLen() : a.minutes;
  // axis: a concave scale (minutes^0.35) — small offsets sit close to the event, larger ones spread out
  const AL_P = 0.35, EV0 = 0.72, EV1 = 0.97, SPAN = 20160;
  const xs = m => EV0 * (1 - Math.pow(Math.min(m, SPAN) / SPAN, AL_P));
  const xe = (e, len) => EV1 - (EV1 - EV0) * Math.pow(Math.min(e, len) / len, AL_P);
  const alCands = () => { const len = evLen(); const ends = SNAP_END_ALL.filter(m => m < len); return [...SNAP_START].reverse().map(m => ({ related_end: false, minutes: m, x: xs(m) })).concat([{ related_end: false, minutes: 0, x: EV0 }], [...ends].reverse().map(m => ({ related_end: true, minutes: m, x: xe(m, len) })), [{ related_end: true, minutes: 0, x: EV1 }]); };
  const xa = a => { const len = evLen(), mb = minutesBefore(a); if (mb >= 0) return xs(mb); const e = len + mb; return e <= 0 ? EV1 + 0.01 : xe(e, len); };
  const snapX = f => alCands().reduce((b, c) => Math.abs(c.x - f) < Math.abs(b.x - f) ? c : b);
  function alTimeline() {
    if (!form.classList.contains('mode-edit') || AL.hidden) return;
    const len = evLen(), color = $('#ed-dot').style.background || '';
    let h = `<div class="axis"></div><div class="evb" style="left:${EV0 * 100}%;width:${(EV1 - EV0) * 100}%;--evc:${color}">${esc(AT.tEvent)}</div>`;
    h += alCands().filter(c => c.minutes).map(c => `<i class="snap" style="left:${c.x * 100}%"></i>`).join('');
    h += [5, 60, 1440, 10080].map(m => `<span class="tick" style="left:${xs(m) * 100}%">${esc(amount(m))}</span>`).join('');
    tline.innerHTML = h;
    const groups = new Map();
    for (const a of ALARMS) { const k = Math.round(xa(a) * 1000); if (!groups.has(k)) groups.set(k, []); groups.get(k).push(a); }
    for (const [k, list] of groups) {
      const g = document.createElement('div'); g.className = 'mkg'; g.style.left = `${k / 10}%`;
      for (const a of list) {
        const mk = document.createElement('div'); mk.className = `mk${a._open ? ' sel' : ''}${a.acknowledged ? ' ack' : ''}`; mk.title = alarmText(a); mk.innerHTML = `<span class="lab">${esc(whenText(a))}</span>${alIco(AL_ICO[a.action] || AL_ICO.DISPLAY)}`;
        mk.onpointerdown = e => { e.preventDefault(); const rect = tline.getBoundingClientRect();
          const move = ev => { const fr = Math.min(1, Math.max(0, (ev.clientX - rect.left) / rect.width)); const c = snapX(fr); if (c.related_end === a.related_end && c.minutes === a.minutes && !a.at) return; a.related_end = c.related_end; a.minutes = c.minutes; a.at = null; ALARMS.forEach(x => x._open = x === a); alRender(); refresh(); };
          const up = () => { document.removeEventListener('pointermove', move); document.removeEventListener('pointerup', up); ALARMS.forEach(x => x._open = x === a); alRender(); refresh(); };
          document.addEventListener('pointermove', move); document.addEventListener('pointerup', up); };
        g.appendChild(mk);
      }
      tline.appendChild(g);
    }
    void len;
  }
  const presetLabel = p => { const acts = D.tActs.split('|'); let t = !p.related_end && p.minutes > 0 ? amount(p.minutes) : whenText({ ...p, at: null, attendees: p.attendees || [] }); if (p.action === 'AUDIO') t += ' · ' + acts[1]; if (p.action === 'EMAIL') t += ' · ' + acts[2]; if (p.repeat) t += ` · ${p.repeat}×`; if (p.proximity) t += ' · ' + proxText(p); return t; };
  function alPresets() {
    const order = [...PRESETS].sort((p, q) => (!!p.related_end) - (!!q.related_end) || q.minutes - p.minutes);
    apre.innerHTML = `<span class="lbl">${esc(AT.tPresets)}</span>` + order.map(p => { const sig = presetSig(p); return `<button type="button" data-sig="${esc(sig)}" aria-pressed="${ALARMS.some(a => presetSig(a) === sig)}">${p.action !== 'DISPLAY' ? alIco(AL_ICO[p.action]) : ''}${esc(presetLabel(p))}</button>`; }).join('');
  }
  apre.addEventListener('click', e => { const b = e.target.closest('button[data-sig]'); if (!b) return; const sig = b.dataset.sig; const ex = ALARMS.find(a => presetSig(a) === sig); if (ex) ALARMS = ALARMS.filter(a => a !== ex); else { const p = PRESETS.find(q => presetSig(q) === sig); if (p) ALARMS.push(fromPreset(p)); } alRender(); refresh(); });
  // the settings of one entry: the fields follow the kind (text | sound | subject, message, recipients)
  // and the trigger (number + unit for a relative one, date and time for an absolute one); "after …"
  // only shows when the stored alarm already fires after its anchor (a valid but foreign setting)
  function alDetails(a, row) {
    const rels = AT.tRels.split('|'), units = AT.tUnits.split('|'), acts = D.tActs.split('|'), prox = AT.tProxVals.split('|');
    const relv = a.at ? 'at' : (a.minutes < 0 ? 'after-' : 'before-') + (a.related_end ? 'end' : 'start');
    const relOpts = [['before-start', rels[0]], ['before-end', rels[1]]].concat(a.minutes < 0 ? [['after-start', rels[2]], ['after-end', rels[3]]] : []).concat([['at', rels[4]]]);
    const u = unitOf(a.minutes), isPreset = matchesPreset(a);
    const d = $('.aldet', row);
    d.innerHTML = `<span>${esc(AT.tWhen)}</span><span class="l"><span class="seg" data-w="rel">${relOpts.map(([v, l]) => `<button type="button" data-v="${v}" aria-pressed="${v === relv}">${esc(l)}</button>`).join('')}</span>${a.at ? `<input type="datetime-local" data-w="at" value="${esc((a.at || '').slice(0, 16))}">` : `<input type="text" inputmode="decimal" data-w="n" value="${fmtNum(Math.abs(a.minutes) / u)}"><select data-w="unit">${AL_UNITS.map((v, i) => `<option value="${v}" ${v === u ? 'selected' : ''}>${esc(units[i])}</option>`).join('')}</select>`}</span>
      <span>${esc(AT.tKind)}</span><span class="seg" data-w="act">${AL_ACTS.map((v, i) => `<button type="button" data-v="${v}" aria-pressed="${v === a.action}">${alIco(AL_ICO[v])} ${esc(acts[i])}</button>`).join('')}</span>
      ${a.action === 'DISPLAY' ? `<span>${esc(AT.tText)}</span><input type="text" data-w="text" value="${esc(a.description)}" placeholder="${esc(AT.tTextHint)}">` : ''}
      ${a.action === 'AUDIO' ? `<span>${esc(AT.tSound)}</span><input type="url" data-w="attach" value="${esc(a.attach)}" placeholder="${esc(AT.tSoundHint)}">` : ''}
      ${a.action === 'EMAIL' ? `<span>${esc(AT.tSubject)}</span><input type="text" data-w="subject" value="${esc(a.summary)}" placeholder="${esc(AT.tTextHint)}"><span>${esc(AT.tBody)}</span><input type="text" data-w="text" value="${esc(a.description)}" placeholder="${esc(AT.tTextHint)}"><span>${esc(AT.tTo)}</span><input type="text" data-w="to" value="${esc(a.attendees.join(', '))}" placeholder="${esc(AT.tToHint)}">` : ''}
      <span>${esc(AT.tRepeat)}</span><span class="l"><select data-w="repeat">${[0, 1, 2, 3, 5, 10].map(n => `<option value="${n}" ${n === a.repeat ? 'selected' : ''}>${n ? n + '×' : esc(AT.tNever)}</option>`).join('')}</select>${a.repeat > 0 ? `<span class="muted small">${esc(AT.tEvery)}</span><select data-w="every">${[1, 5, 10, 15, 30, 60].map(n => `<option value="${n}" ${n === a.duration_minutes ? 'selected' : ''}>${esc(amount(n))}</option>`).join('')}</select>` : ''}</span>
      <span>${esc(AT.tProx)}</span><select data-w="prox"><option value="">${esc(AT.tProxNone)}</option>${AL_PROX.map((v, i) => `<option value="${v}" ${v === a.proximity ? 'selected' : ''}>${esc(prox[i])}</option>`).join('')}</select>
      <span>${esc(AT.tAck)}</span><label class="muted small"><input type="checkbox" data-w="ack" ${a.acknowledged ? 'checked' : ''}> ${esc(AT.tAckHint)}${a.acknowledged ? ' (' + esc(AT.tAckSince) + ' ' + esc(fmtDT(a.acknowledged)) + ')' : ''}</label>
      <div class="foot"><button class="pset" type="button" data-w="pset" aria-pressed="${isPreset}" ${a.at ? 'disabled' : ''} title="${esc(a.at ? AT.tPresetAt : isPreset ? AT.tPresetRm : AT.tPresetAdd)}">${alIco('#i-star')} ${esc(isPreset ? AT.tPreset : AT.tPresetAdd)}</button><span>${esc(a.at ? AT.tPresetAt : AT.tPresetHint)}</span></div>`;
    const w = k => $(`[data-w=${k}]`, d);
    w('rel').onclick = e => { const b = e.target.closest('button'); if (!b) return; const v = b.dataset.v; if (v === 'at') { a.at = a.at || (f.date.value || today) + 'T' + (f.all_day.checked ? '09:00' : (f.from.value || '09:00')) + ':00'; } else { const [dir, anc] = v.split('-'); a.related_end = anc === 'end'; a.at = null; a.minutes = (Math.abs(a.minutes) || 15) * (dir === 'after' ? -1 : 1); } alRender(); refresh(); };
    if (w('n')) { const apply = () => { const n = parseFloat(w('n').value.replace(',', '.')); if (isNaN(n) || n < 0) { w('n').classList.add('err'); return; } a.minutes = Math.sign(a.minutes || 1) * Math.round(n * +w('unit').value); alRender(); refresh(); }; w('n').onchange = apply; w('unit').onchange = apply; w('n').onkeydown = e => { if (e.key === 'Enter') { e.preventDefault(); apply(); } }; }
    if (w('at')) w('at').onchange = () => { if (w('at').value) { a.at = w('at').value + ':00'; alRender(); refresh(); } };
    w('act').onclick = e => { const b = e.target.closest('button'); if (b) { a.action = b.dataset.v; alRender(); refresh(); } };
    if (w('text')) w('text').oninput = () => { a.description = w('text').value; alSync(row, a); };
    if (w('subject')) w('subject').oninput = () => { a.summary = w('subject').value; alSync(row, a); };
    if (w('to')) w('to').onchange = () => { a.attendees = w('to').value.split(/[,;\s]+/).filter(Boolean); alRender(); refresh(); };
    if (w('attach')) w('attach').onchange = () => { a.attach = w('attach').value.trim(); alRender(); refresh(); };
    w('repeat').onchange = () => { a.repeat = +w('repeat').value; a.duration_minutes = a.repeat ? (a.duration_minutes || 10) : 0; alRender(); refresh(); };
    if (w('every')) w('every').onchange = () => { a.duration_minutes = +w('every').value; alRender(); refresh(); };
    w('prox').onchange = () => { a.proximity = w('prox').value; alRender(); refresh(); };
    w('ack').onchange = () => { a.acknowledged = w('ack').checked ? new Date(Date.now() - new Date().getTimezoneOffset() * 60000).toISOString().slice(0, 19) : null; alRender(); refresh(); };
    w('pset').onclick = () => { if (a.at) return; if (matchesPreset(a)) PRESETS = PRESETS.filter(p => presetSig(p) !== presetSig(a)); else { const { action, minutes, related_end, repeat, duration_minutes, attendees, attach, proximity } = a; PRESETS.push({ action, minutes, related_end, repeat, duration_minutes, attendees: [...attendees], attach, proximity }); } savePresets(); alRender(); };
  }
  // text edits update the row's summary and the mirror without rebuilding the entry (the field keeps its focus)
  const alMirror = () => { f.alarms_json.value = JSON.stringify(ALARMS.map(({ _id, _open, ...a }) => a)); };
  function alSync(row, a) { $('.txt', row).textContent = alarmText(a); $('.extra', row).textContent = extrasOf(a); alMirror(); refresh(); }
  let alBusy = false;
  function alRender() {
    if (alBusy) return; alBusy = true; // a field inside a row commits on blur when its row is rebuilt; that nested render is skipped, the model is current anyway
    try { alRenderNow(); } finally { alBusy = false; }
  }
  function alRenderNow() {
    if (alBox.contains(document.activeElement)) document.activeElement.blur();
    $$('.row', alBox).forEach(r => r.remove());
    for (const a of ALARMS) {
      const r = addRow('alarms'); r.classList.toggle('open', !!a._open); r._alarm = a;
      $('.txt', r).textContent = alarmText(a); $('.sum use', r).setAttribute('href', AL_ICO[a.action] || AL_ICO.DISPLAY); const t = $('.sum .t', r); t.textContent = whenText(a); t.classList.toggle('ack', !!a.acknowledged); $('.extra', r).textContent = extrasOf(a);
      $('.pdot', r).classList.toggle('on', matchesPreset(a)); $('.act', r).setAttribute('aria-pressed', a._open ? 'true' : 'false');
      const d = $('.aldet', r); d.hidden = !a._open; if (a._open) alDetails(a, r);
    }
    $('#ed-alarm-n').textContent = ALARMS.length ? `(${ALARMS.length})` : '';
    alPresets(); alTimeline(); alMirror();
  }
  alBox.addEventListener('click', e => {
    const b = e.target.closest('[data-al-open], [data-rm]'); if (!b) return; e.stopPropagation(); const row = b.closest('.row'), a = row && row._alarm; if (!a) return;
    if (b.hasAttribute('data-rm')) ALARMS = ALARMS.filter(x => x !== a); else ALARMS.forEach(x => x._open = x === a ? !x._open : false);
    alRender(); refresh();
  });
  function alarmsInit(list) { ALARMS = list.map(a => newAlarm({ ...a, attendees: [...(a.attendees || [])] })); alRender(); }
  const alarmAdd = () => { const a = newAlarm(); ALARMS.forEach(x => x._open = false); a._open = true; ALARMS.push(a); alRender(); return $(`.row.open`, alBox); };
  const alarmsOf = () => ALARMS.map(({ _id, _open, ...a }) => ({ ...a, description: a.description.trim(), summary: a.summary.trim(), attach: a.attach.trim() }));
  // people: the organizer is the row with the ORGANIZER role
  const statusText = p => { const s = D.tStatus.split('|'); return { 'NEEDS-ACTION': s[0], ACCEPTED: s[1], DECLINED: s[2], TENTATIVE: s[3], DELEGATED: s[4] }[p.status] || ''; };
  const addPerson = (p, role) => { const r = addRow('people'); r._orig = p; w(r, 'role').value = role; w(r, 'name').value = p.name || ''; w(r, 'email').value = p.email || ''; $('.st', r).textContent = [statusText(p), p.cutype === 'RESOURCE' || p.cutype === 'ROOM' ? p.cutype.toLowerCase() : ''].filter(Boolean).join(' · '); return r; };
  const peopleOf = () => { const all = rows('people').map(r => ({ ...(r._orig || { status: '', cutype: '', extra: [] }), role: w(r, 'role').value, name: w(r, 'name').value.trim(), email: w(r, 'email').value.trim() })).filter(p => p.email); const org = all.find(p => p.role === 'ORGANIZER'); return { organizer: org ? { ...org, role: '' } : null, attendees: all.filter(p => p.role !== 'ORGANIZER') }; };
  // links: URL / attachment / conference; an embedded (binary) attachment is shown read-only and never re-sent
  const addLink = l => { const r = addRow('links'); r._orig = l; w(r, 'kind').value = l.kind || 'url'; w(r, 'uri').value = l.uri || ''; w(r, 'label').value = l.embedded ? D.tEmbedded + (l.label ? ' · ' + l.label : '') : (l.label || ''); if (l.embedded) { w(r, 'uri').placeholder = ''; $$('input, select, .rm', r).forEach(x => x.disabled = true); $('[data-act=open]', r).hidden = true; } return r; };
  const linksOf = () => rows('links').filter(r => !(r._orig || {}).embedded).map(r => ({ ...(r._orig || { extra: [] }), embedded: false, kind: w(r, 'kind').value, uri: w(r, 'uri').value.trim(), label: w(r, 'label').value.trim() })).filter(l => l.uri);
  // colour: a hex input plus a "calendar colour" toggle that means "no COLOR property"
  const toHex = c => { if (/^#[0-9a-f]{6}$/i.test(c)) return c.toLowerCase(); const el = document.createElement('i'); el.style.color = c; document.body.appendChild(el); const rgb = getComputedStyle(el).color.match(/\d+/g); el.remove(); return rgb && rgb.length >= 3 ? '#' + rgb.slice(0, 3).map(n => (+n).toString(16).padStart(2, '0')).join('') : '#3478f6'; };
  const setColor = c => { const own = !!c; f.color_cal.checked = !own; $('#ed-color-cal').classList.toggle('on', !own); f.color.value = toHex(own ? c : (f.calendar.selectedOptions[0] || {}).dataset?.color || '#3478f6'); f.color.disabled = !own; };
  $('#ed-color-cal').addEventListener('click', e => { e.preventDefault(); setColor(f.color_cal.checked ? f.color.value : ''); syncDot(); refresh(); });
  f.color.addEventListener('input', syncDot);
  const colorOf = () => f.color_cal.checked ? '' : f.color.value;
  const geoOf = () => { const a = f.lat.value.trim(), b = f.lon.value.trim(); return a !== '' && b !== '' && !isNaN(+a) && !isNaN(+b) ? [+a, +b] : null; };
  // ---- location (design G): ONE editable field — the address or the coordinates — the other part is
  // derived from it (geocoder) and shown as a chip below, never edited by hand. The user may swap
  // which part is primary, or drop a part; errors of the derivation show in red. ----
  const sugg = $('#ed-sugg'), ring = $('#ed-ring'), locin = $('#ed-locin'), locsub = $('#ed-loc-sub'), LT = locsub.dataset;
  let suggTimer = 0, suggRaf = 0, suggSeq = 0, suggItems = [], suggIdx = -1, deriveSeq = 0;
  const SUGG_DELAY = 2000;
  const LOC = { addr: true, coords: false, primary: 'addr', errAddr: false, errCoords: false, invalid: false, pending: false };
  const other = () => LOC.primary === 'addr' ? 'coords' : 'addr';
  const coordsText = () => geoOf() ? `${(+f.lat.value).toFixed(5)}, ${(+f.lon.value).toFixed(5)}` : '';
  const parseCoords = t => { const m = t.match(/^\s*(-?\d+(?:[.,]\d+)?)\s*[,; ]\s*(-?\d+(?:[.,]\d+)?)\s*$/); if (!m) return null; const a = +m[1].replace(',', '.'), b = +m[2].replace(',', '.'); return Math.abs(a) <= 90 && Math.abs(b) <= 180 ? [a, b] : null; };
  const ringStop = () => { cancelAnimationFrame(suggRaf); ring.setAttribute('hidden', ''); ring.classList.remove('busy'); }; // SVG elements have no `hidden` property
  const ringCount = ms => { const c = $('circle', ring), t0 = performance.now(); ring.removeAttribute('hidden'); ring.classList.remove('busy'); const step = () => { const k = Math.min(1, (performance.now() - t0) / ms); c.style.strokeDashoffset = (50.27 * k).toFixed(2); if (k < 1) suggRaf = requestAnimationFrame(step); }; step(); };
  const ringBusy = on => { if (on) { ring.removeAttribute('hidden'); ring.classList.add('busy'); } else ringStop(); };
  const suggClose = () => { sugg.hidden = true; sugg.innerHTML = ''; suggItems = []; suggIdx = -1; locin.setAttribute('aria-expanded', 'false'); };
  const suggShow = items => { suggItems = items; suggIdx = -1; sugg.innerHTML = items.length ? items.map((p, i) => `<button type="button" role="option" data-i="${i}">${p.label.replace(/</g, '&lt;')}</button>`).join('') : `<div class="none">${T.suggNone}</div>`; sugg.hidden = false; locin.setAttribute('aria-expanded', 'true'); };
  function locRender() {
    const P = LOC.primary, O = other();
    $('#ed-loc-lbl').textContent = P === 'addr' ? LT.tAddr : LT.tGeo;
    locin.placeholder = P === 'addr' ? D.tLocation || '' : LT.tCoordsHint;
    if (document.activeElement !== locin) locin.value = P === 'addr' ? f.location.value : coordsText();
    const err = $('#ed-loc-err'); err.textContent = LOC.invalid ? LT.tErrInvalid : ''; err.hidden = !LOC.invalid; $('#ed-locp').classList.toggle('err', LOC.invalid);
    $('#ed-loc-swap').hidden = !LOC[O];
    $('[data-geo=map]', form).classList.toggle('hot', LOC.errAddr || LOC.errCoords);
    let h = '';
    if (LOC[O]) { const e = O === 'addr' ? LOC.errAddr : LOC.errCoords, v = O === 'addr' ? f.location.value : coordsText(); h = `<span class="chip${e ? ' err' : ''}" title="${O === 'addr' ? LT.tAddr : LT.tGeo}"><svg class="ico" aria-hidden="true"><use href="#${O === 'addr' ? 'i-pin' : 'i-target'}"/></svg><span class="txt">${(v || (LOC.pending ? '…' : '–')).replace(/</g, '&lt;')}</span><b data-locrm="${O}" title="${O === 'addr' ? LT.tRmAddr : LT.tRmCoords}">×</b></span>` + (e ? `<span class="errtxt">${O === 'addr' ? LT.tErrAddr : LT.tErrCoords}</span>` : ''); }
    else h = `<button class="chip ghost" type="button" data-locadd="${O}"><svg class="ico" aria-hidden="true"><use href="#i-plus"/></svg> ${O === 'addr' ? LT.tAddAddr : LT.tAddCoords}</button>`;
    locsub.innerHTML = h;
    const addr = f.location.value.trim(); $('#ed-loc-view').hidden = !addr; $('#ed-geo-view').hidden = !geoOf();
    $('#ed-addr-lines').innerHTML = addr.split(/,\s*/).filter(Boolean).map(l => `<span class="ln">${l.replace(/</g, '&lt;')}</span>`).join('');
    $('#ed-geo-txt').textContent = coordsText();
    requestAnimationFrame(fitAddrLines);
  }
  // a line wider than the card shrinks its text until it fits (never below 10 px)
  function fitAddrLines() { $$('#ed-addr-lines .ln').forEach(l => { l.style.fontSize = ''; l.classList.remove('wrap'); const w = l.clientWidth; if (!w || l.scrollWidth <= w) return; const fs = 15 * w / l.scrollWidth - 0.3; if (fs >= 9) l.style.fontSize = fs.toFixed(1) + 'px'; else { l.style.fontSize = '11px'; l.classList.add('wrap'); } }); } // below 9 px the line wraps instead of becoming unreadable
  async function geoSearch(q, near) { const u = new URL('/api/geo/search', location.origin); u.searchParams.set('q', q); if (near) { u.searchParams.set('lat', near[0]); u.searchParams.set('lon', near[1]); } const r = await fetch(u); if (!r.ok) throw new Error(r.status); return r.json(); }
  async function geoReverse(lat, lon) { const r = await fetch(`/api/geo/reverse?lat=${(+lat).toFixed(6)}&lon=${(+lon).toFixed(6)}`); if (r.status === 404) return null; if (!r.ok) throw new Error(r.status); return r.json(); }
  // derive the secondary part from the primary one (only when that part is in use)
  async function deriveCoords() { if (!LOC.coords || LOC.primary !== 'addr') return; const q = f.location.value.trim(); const seq = ++deriveSeq; if (!q) { LOC.errCoords = false; locRender(); return; } LOC.pending = true; locRender(); ringBusy(true); try { const items = await geoSearch(q, geoOf()); if (seq !== deriveSeq) return; if (items.length) { f.lat.value = items[0].lat.toFixed(6); f.lon.value = items[0].lon.toFixed(6); LOC.errCoords = false; } else LOC.errCoords = true; } catch (_) { if (seq === deriveSeq) LOC.errCoords = true; } finally { if (seq === deriveSeq) { LOC.pending = false; ringBusy(false); locRender(); refresh(); } } }
  async function deriveAddr() { if (!LOC.addr || LOC.primary !== 'coords') return; const g = geoOf(); const seq = ++deriveSeq; if (!g) { locRender(); return; } LOC.pending = true; locRender(); ringBusy(true); try { const p = await geoReverse(g[0], g[1]); if (seq !== deriveSeq) return; if (p) { f.location.value = p.label; LOC.errAddr = false; } else LOC.errAddr = true; } catch (_) { if (seq === deriveSeq) LOC.errAddr = true; } finally { if (seq === deriveSeq) { LOC.pending = false; ringBusy(false); locRender(); refresh(); } } }
  // the primary field: address → suggestions after a pause, the pick or a leave derives the coordinates;
  // coordinates → validated, then the address is derived
  let primaryTouched = false;
  const commitPrimary = () => { if (!primaryTouched) return; primaryTouched = false; if (LOC.primary === 'addr') { f.location.value = locin.value.trim(); deriveCoords(); } else { const c = parseCoords(locin.value); LOC.invalid = !c && locin.value.trim() !== ''; if (c) { f.lat.value = c[0].toFixed(6); f.lon.value = c[1].toFixed(6); deriveAddr(); } else if (!locin.value.trim()) { f.lat.value = ''; f.lon.value = ''; } locRender(); } refresh(); };
  const suggPick = i => { const p = suggItems[i]; if (!p) return; locin.value = p.label; f.location.value = p.label; primaryTouched = false; if (LOC.coords) { f.lat.value = p.lat.toFixed(6); f.lon.value = p.lon.toFixed(6); LOC.errCoords = false; } suggClose(); ringStop(); locRender(); syncDot(); refresh(); };
  locin.addEventListener('input', () => { primaryTouched = true; clearTimeout(suggTimer); const q = locin.value.trim();
    if (LOC.primary === 'addr') { f.location.value = q; if (q.length < 3) { ringStop(); suggClose(); refresh(); return; } ringCount(SUGG_DELAY); suggTimer = setTimeout(async () => { if (document.activeElement !== locin) return; const seq = ++suggSeq; ring.classList.add('busy'); try { const items = await geoSearch(q, geoOf()); if (seq !== suggSeq || document.activeElement !== locin) return; suggShow(items); } catch (_) { suggClose(); } finally { if (seq === suggSeq) ringStop(); } }, SUGG_DELAY); }
    else { LOC.invalid = false; $('#ed-locp').classList.remove('err'); ringCount(SUGG_DELAY); suggTimer = setTimeout(() => { if (document.activeElement === locin) { ringStop(); commitPrimary(); } }, SUGG_DELAY); }
    refresh(); });
  locin.addEventListener('keydown', e => { if (e.key === 'Enter') { e.preventDefault(); if (!sugg.hidden && suggIdx >= 0) suggPick(suggIdx); else locin.blur(); return; } if (sugg.hidden) return; if (e.key === 'ArrowDown' || e.key === 'ArrowUp') { e.preventDefault(); suggIdx = (suggIdx + (e.key === 'ArrowDown' ? 1 : -1) + suggItems.length) % Math.max(1, suggItems.length); $$('button', sugg).forEach((b, i) => b.setAttribute('aria-selected', i === suggIdx ? 'true' : 'false')); } else if (e.key === 'Escape') { e.stopPropagation(); suggClose(); } });
  sugg.addEventListener('mousedown', e => { const b = e.target.closest('button[data-i]'); if (b) { e.preventDefault(); suggPick(+b.dataset.i); } });
  locin.addEventListener('blur', () => { clearTimeout(suggTimer); suggSeq++; ringStop(); suggClose(); commitPrimary(); });
  locsub.addEventListener('click', e => { const b = e.target.closest('[data-locrm],[data-locadd]'); if (!b) return;
    if (b.dataset.locrm) { LOC[b.dataset.locrm] = false; if (b.dataset.locrm === 'addr') { f.location.value = ''; LOC.errAddr = false; } else { f.lat.value = ''; f.lon.value = ''; LOC.errCoords = false; } }
    if (b.dataset.locadd) { LOC[b.dataset.locadd] = true; if (b.dataset.locadd === 'coords') deriveCoords(); else deriveAddr(); }
    locRender(); refresh(); });
  function locSwap() { const O = other(); if (!LOC[O]) return; LOC.primary = O; LOC.invalid = false; primaryTouched = false; locRender(); }
  // called by fill(): derive the parts from the event
  function locInit() { LOC.addr = !!f.location.value.trim() || !geoOf(); LOC.coords = !!geoOf(); LOC.primary = LOC.addr ? 'addr' : 'coords'; LOC.errAddr = LOC.errCoords = LOC.invalid = LOC.pending = false; primaryTouched = false; clearTimeout(suggTimer); ringStop(); suggClose(); locRender(); }
  // ---- map dialog: the crosshair is the position; with the address part in use the address must be
  // determined for a moved map before "apply" — apply writes both parts at once ----
  const gm = $('#geomap'); let map = null, gmSeq = 0, gmMoved = false, gmAddrKnown = false, gmAddrErr = false, gmAddr = '', gmMoving = false;
  const assetV = (($('script[src*="app.js"]') || {}).src || '').split('?')[1] || '';
  const loadLeaflet = () => new Promise((res, rej) => { if (window.L) return res(); const css = document.createElement('link'); css.rel = 'stylesheet'; css.href = '/assets/leaflet.css?' + assetV; document.head.appendChild(css); const js = document.createElement('script'); js.src = '/assets/leaflet.js?' + assetV; js.onload = res; js.onerror = () => rej(new Error('leaflet')); document.head.appendChild(js); });
  const centre = () => { const c = map.getCenter(); return [+c.lat.toFixed(6), +c.lng.toFixed(6)]; };
  const coordsChanged = () => { const c = centre(), g = geoOf(); return !g || Math.abs(g[0] - c[0]) > 5e-6 || Math.abs(g[1] - c[1]) > 5e-6; };
  // one main button: "determine address" (red) while the address for the crosshair is unknown, else
  // "apply" — green when there is something new to apply, grey when nothing changed
  function gmState() {
    const c = map ? centre() : null;
    $('#gm-addr-col').hidden = !LOC.addr; $('#gm-coords-col').hidden = !LOC.coords;
    $('#gm-coords').textContent = c && LOC.coords ? `${c[0].toFixed(5)}, ${c[1].toFixed(5)}` : '';
    const at = $('#gm-addr-txt'); at.textContent = gmAddrErr ? gm.dataset.tNone : gmAddrKnown ? gmAddr : ''; at.classList.toggle('unknown', gmAddrErr || !gmAddrKnown);
    const b = $('#gm-ok'), determine = LOC.addr && !gmAddrKnown;
    $('#gm-ok-txt').textContent = determine ? b.dataset.tDetermine : b.dataset.tApply;
    $('#gm-ok-ico').setAttribute('href', determine ? '#i-pin' : '#i-check');
    const changed = LOC.addr ? gmMoved : (map ? coordsChanged() : false);
    b.classList.toggle('hot', determine); b.classList.toggle('good', !determine && changed);
    b.disabled = gmBusy || (!determine && !changed);
  }
  let gmBusy = false;
  async function openMap() {
    gm.hidden = false; $('#gm-hint').textContent = ''; const seq = ++gmSeq; gmMoved = false; gmAddrErr = false; gmAddr = f.location.value.trim(); gmAddrKnown = LOC.addr && !!gmAddr && !LOC.errAddr && !LOC.errCoords;
    try { await loadLeaflet(); } catch (_) { $('#gm-hint').textContent = gm.dataset.tError; return; }
    if (!map) { map = L.map('gm-map', { zoomControl: true, attributionControl: true, inertia: false /* no glide after a drag: a click on "determine" right after releasing would be undone by the late move events */ }).setView([20, 0], 2); L.tileLayer('https://tile.openstreetmap.org/{z}/{x}/{y}.png', { maxZoom: 19, attribution: '&copy; <a href="https://www.openstreetmap.org/copyright" target="_blank" rel="noopener">OpenStreetMap</a> contributors' }).addTo(map); map.on('move', () => { if (gmMoving) return; gmMoved = true; gmAddrKnown = false; gmAddrErr = false; gmState(); }); }
    map.invalidateSize();
    const g = geoOf(), q = gmAddr; gmMoving = true;
    const jump = (ll, z) => map.setView(ll, z, { animate: false }); // an animated jump would fire move events
    if (g) jump(g, 16);
    else if (q.length >= 3) { $('#gm-hint').textContent = '…'; try { const items = await geoSearch(q, null); if (seq !== gmSeq) return; if (items.length) { jump([items[0].lat, items[0].lon], 16); $('#gm-hint').textContent = ''; } else { jump([20, 0], 2); gmAddrKnown = false; $('#gm-hint').textContent = gm.dataset.tNone + ' · ' + gm.dataset.tWorld; } } catch (_) { jump([20, 0], 2); gmAddrKnown = false; $('#gm-hint').textContent = gm.dataset.tError; } }
    else { jump([20, 0], 2); gmAddrKnown = false; $('#gm-hint').textContent = gm.dataset.tWorld; }
    gmState(); setTimeout(() => { gmMoving = false; }, 120);
  }
  const closeMap = () => { gm.hidden = true; };
  $('#gm-close').onclick = closeMap;
  async function gmDetermine() { const c = centre(); gmBusy = true; gmState(); $('#gm-hint').textContent = '…'; try { const p = await geoReverse(c[0], c[1]); if (p) { gmAddr = p.label; gmAddrKnown = true; gmAddrErr = false; } else { gmAddrKnown = false; gmAddrErr = true; } $('#gm-hint').textContent = ''; } catch (_) { gmAddrKnown = false; gmAddrErr = true; $('#gm-hint').textContent = gm.dataset.tError; } gmBusy = false; gmState(); }
  function gmApply() { const c = centre(); if (LOC.coords) { f.lat.value = c[0].toFixed(6); f.lon.value = c[1].toFixed(6); LOC.errCoords = false; LOC.invalid = false; } if (LOC.addr) { f.location.value = gmAddr; LOC.errAddr = false; } closeMap(); locRender(); syncDot(); refresh(); }
  $('#gm-ok').onclick = () => { if (LOC.addr && !gmAddrKnown) gmDetermine(); else gmApply(); };
  addEventListener('keydown', e => { if (e.key === 'Escape' && !gm.hidden) { e.stopImmediatePropagation(); closeMap(); } }, true);
  // exceptions / extra dates: a date; timed events get the start time appended on save
  const addDate = (list, iso) => { const r = addRow(list); w(r, 'd').value = (iso || '').slice(0, 10); $('.txt', r).textContent = iso ? fmtD(iso) : ''; return r; };
  const datesOf = list => rows(list).map(r => w(r, 'd').value).filter(Boolean).map(d => f.all_day.checked ? d : `${d}T${f.from.value || '00:00'}:00`);
  // recurrence: widgets for the common rules, raw text for everything else
  const FREQS = ['DAILY', 'WEEKLY', 'MONTHLY', 'YEARLY'], DAYS = ['MO', 'TU', 'WE', 'TH', 'FR', 'SA', 'SU'];
  $('#ed-days').innerHTML = DAYS.map((d, i) => `<button type="button" data-day="${d}" aria-pressed="false">${DOW[i]}</button>`).join('');
  $$('#ed-days button').forEach(b => b.onclick = () => { b.setAttribute('aria-pressed', b.getAttribute('aria-pressed') === 'true' ? 'false' : 'true'); refresh(); });
  const parseRR = r => { const o = { freq: '', interval: 1, byday: [], count: 0, until: '', extra: false }; (r || '').split(';').forEach(p => { const [k, v] = p.split('='); if (k === 'FREQ') o.freq = v; else if (k === 'INTERVAL') o.interval = +v || 1; else if (k === 'BYDAY' && o.freq === 'WEEKLY') o.byday = v.split(','); else if (k === 'COUNT') o.count = +v; else if (k === 'UNTIL') o.until = v; else if (k && k !== 'WKST') o.extra = true; }); return o; };
  function setRR(r, untilLocal) { const o = parseRR(r); const simple = FREQS.includes(o.freq) && !o.extra && !(o.byday.length && o.freq !== 'WEEKLY'); f.rrule.value = !r ? '' : simple ? o.freq : 'custom'; f.rrule_custom.value = simple ? '' : (r || ''); f.interval.value = o.interval; $$('#ed-days button').forEach(b => b.setAttribute('aria-pressed', o.byday.includes(b.dataset.day) ? 'true' : 'false')); f.rend.value = o.count ? 'count' : o.until ? 'until' : ''; f.count.value = o.count || 10; f.until.value = untilLocal || (o.until ? `${o.until.slice(0, 4)}-${o.until.slice(4, 6)}-${o.until.slice(6, 8)}` : ''); }
  const buildRR = () => { const v = f.rrule.value; if (!v) return null; if (v === 'custom') return f.rrule_custom.value.trim() || null; let r = 'FREQ=' + v; if (+f.interval.value > 1) r += ';INTERVAL=' + f.interval.value; if (v === 'WEEKLY') { const d = $$('#ed-days [aria-pressed=true]').map(b => b.dataset.day); if (d.length) r += ';BYDAY=' + d.join(','); } if (f.rend.value === 'count') r += ';COUNT=' + (+f.count.value || 1); else if (f.rend.value === 'until' && f.until.value) r += ';UNTIL=' + f.until.value.replace(/-/g, ''); return r; };
  const rrText = () => { const v = f.rrule.value; if (!v) return ''; if (v === 'custom') return f.rrule_custom.value; let t = f.rrule.selectedOptions[0].textContent; const units = D.tUnits.split('|'); if (+f.interval.value > 1) t += ', ' + D.tEvery.replace('{n}', f.interval.value) + ' ' + units[FREQS.indexOf(v)]; if (v === 'WEEKLY') { const d = $$('#ed-days [aria-pressed=true]').map(b => DOW[DAYS.indexOf(b.dataset.day)]); if (d.length) t += ' ' + D.tOn + ' ' + d.join(', '); } if (f.rend.value === 'count') t += ', ' + D.tCount.replace('{n}', f.count.value); else if (f.rend.value === 'until' && f.until.value) t += ', ' + D.tUntil + ' ' + fmtD(f.until.value); return t; };
  function syncRR() { const v = f.rrule.value, on = v && v !== 'custom'; $('#ed-rr').hidden = !on; f.rrule_custom.hidden = v !== 'custom'; f.rrule_custom.closest('.field').hidden = v !== 'custom'; $('#ed-unit').textContent = on ? D.tUnits.split('|')[FREQS.indexOf(v)] : ''; $('#ed-days').hidden = v !== 'WEEKLY'; f.count.hidden = f.rend.value !== 'count'; $('.cnt', form).hidden = f.rend.value !== 'count'; f.until.hidden = f.rend.value !== 'until'; $('#ed-rrule-text').textContent = rrText(); ['exdates', 'rdates'].forEach(l => { const box = $('#ed-' + l); box.hidden = !v; box.classList.toggle('empty', !rows(l).length); }); }
  // cards: the "when" card always, the others when they hold data or were added in edit mode
  // `.aux` inputs (rule interval, count, until, raw rule) only qualify the rule and never count as data on their own
  const hasData = card => $$('input:not([type=hidden]):not(.aux):not(.mirror), textarea, select.dat', card).some(i => i.value.trim() !== '') || !!$('.tags .chip', card) || $$('.rows .row', card).length > 0;
  function cards() { const edit = form.classList.contains('mode-edit'); $$('.card', form).forEach(c => { c.hidden = !(c.hasAttribute('data-always') || hasData(c) || (edit && c.dataset.added)); }); $$('.addmenu [data-card-add]', form).forEach(b => b.disabled = !$(`.card[data-card="${b.dataset.cardAdd}"]`, form).hidden); }
  function addCard(name) { const c = $(`.card[data-card="${name}"]`, form); c.dataset.added = '1'; c.hidden = false; cards(); return c; }
  const serialize = () => $$('input:not([type=hidden]):not(.mirror), select, textarea', form).map(e => e.type === 'checkbox' ? (e.checked ? '1' : '0') : e.value).join('|') + '|' + chips().join(',') + '|' + $$('#ed-days [aria-pressed=true]', form).length;
  function updateButtons() { const changed = serialize() !== baseline; $('#ed-save').disabled = !changed; $('#ed-reset').disabled = !changed && form.classList.contains('new'); $('#ed-delete').disabled = form.classList.contains('mode-view') || form.classList.contains('new'); }
  function whenLine() { const s = f.date.value; if (!s) return; const e = f.end_date.value || s, multi = e > s; const t = f.all_day.checked ? (multi ? `${fmtD(s)} – ${fmtD(e)}` : fmtD(s)) + ' · ' + D.tAllday : multi ? `${fmtD(s)} ${f.from.value} – ${fmtD(e)} ${f.to.value}` : `${fmtD(s)} · ${f.from.value} – ${f.to.value}`; $('#ed-whenline').textContent = t; }
  const grow = () => { const t = f.description; t.style.height = 'auto'; t.style.height = (t.scrollHeight + 2) + 'px'; }; // the note shows its whole text in both modes
  function refresh() { whenLine(); syncRR(); grow(); alTimeline(); updateButtons(); }
  function mode(m) { form.classList.toggle('mode-view', m === 'view'); form.classList.toggle('mode-edit', m === 'edit'); ed.classList.toggle('mode-edit', m === 'edit'); cards(); refresh(); requestAnimationFrame(fitAddrLines); }
  // all-day: the API end is the exclusive DTEND; the editor shows the last day (empty = one day)
  const addDays = (s, n) => { const e = new Date(s + 'T00:00:00Z'); e.setUTCDate(e.getUTCDate() + n); return e.toISOString().slice(0, 10); }; // UTC arithmetic: no zone shift
  const lastDay = d => { const s = addDays(d.end.slice(0, 10), -1); return s > d.start.slice(0, 10) ? s : ''; };
  const nextDay = s => addDays(s, 1);
  function fill(d, date) {
    current = d; openDate = date; form.reset(); $('#ed-error').textContent = '';
    $$('.card[data-added]', form).forEach(x => x.removeAttribute('data-added')); $$('.tags .chip', form).forEach(x => x.remove()); $$('.rows .row', form).forEach(x => x.remove());
    f.key.value = d ? `${d.principal}/${d.cal_id}/${d.object_id}` : ''; f.etag.value = d ? d.etag : '';
    form.classList.toggle('new', !d); f.summary.placeholder = d ? D.tTitle : D.tNewtitle;
    const b = $('#ed-save'); b.title = d ? D.tSave : D.tCreate; b.setAttribute('aria-label', b.title);
    if (d) {
      f.summary.value = d.summary; f.location.value = d.location; f.description.value = d.description; d.categories.forEach(addChip);
      f.calendar.value = `${d.principal}/${d.cal_id}`; f.date.value = d.start.slice(0, 10); f.end_date.value = d.all_day ? lastDay(d) : d.end.slice(0, 10);
      f.from.value = d.start.slice(11, 16); f.to.value = d.end.slice(11, 16); setAllDay(d.all_day);
      alarmsInit(d.alarms); if (d.organizer) addPerson(d.organizer, 'ORGANIZER'); d.attendees.forEach(p => addPerson(p, p.role || 'REQ-PARTICIPANT'));
      setRR(d.rrule, d.rrule_until); d.exdates.forEach(x => addDate('exdates', x)); d.rdates.forEach(x => addDate('rdates', x));
      f.status.value = d.status; f.transp.value = d.transp; f.class.value = d.class; if (d.priority != null) setSel(f.priority, d.priority); d.links.forEach(addLink); setColor(d.color); if (d.geo) { f.lat.value = d.geo[0]; f.lon.value = d.geo[1]; }
      $('#ed-hint').textContent = d.rrule ? D.tSeries : '';
    } else {
      f.date.value = date || state.date; f.end_date.value = f.date.value; f.from.value = '09:00'; f.to.value = '10:00'; setAllDay(false); setRR('', ''); setColor(''); alarmsInit([]); $('#ed-hint').textContent = '';
      if (state.cals) { const first = state.cals.split(',')[0]; if ([...f.calendar.options].some(o => o.value === first)) f.calendar.value = first; }
    }
    locInit(); syncDot(); mode(d ? 'view' : 'edit'); baseline = serialize(); updateButtons();
  }
  async function openEditor(key, date) {
    if (key) {
      try { const r = await fetch('/api/events/' + key); if (!r.ok) throw new Error(r.status); fill(await r.json()); } catch (e) { alert(T.err + ': ' + e.message); return; }
    } else fill(null, date);
    ed.hidden = false; if (!key) f.summary.focus(); dayline();
  }
  form.addEventListener('input', refresh);
  form.addEventListener('change', e => { const t = e.target; if (t === f.calendar) syncDot(); if (t === f.date) { if (f.end_date.value < f.date.value) f.end_date.value = f.date.value; dayline(); } refresh(); });
  form.addEventListener('click', e => {
    const b = e.target.closest('button'); if (!b || !form.contains(b)) return;
    if (b.dataset.add) { const l = b.dataset.add; const r = l === 'alarms' ? alarmAdd() : l === 'people' ? addPerson({ name: '', email: '', status: '', cutype: '', extra: [] }, rows('people').length ? 'REQ-PARTICIPANT' : 'ORGANIZER') : l === 'links' ? addLink({ kind: rows('links').some(r => w(r, 'kind').value === 'url') ? 'attach' : 'url', uri: '', label: '', embedded: false, extra: [] }) : addDate(l, ''); const i = $('input:not([hidden]), select', r); if (i) i.focus(); refresh(); return; }
    if (b.hasAttribute('data-rm')) { b.closest('.row').remove(); refresh(); return; }
    if (b.dataset.cardAdd) { const c = addCard(b.dataset.cardAdd); b.closest('.addmenu').classList.remove('open'); if (b.dataset.cardAdd === 'alarm') $('[data-add=alarms]', c).click(); else if (b.dataset.cardAdd === 'people') $('[data-add=people]', c).click(); else if (b.dataset.cardAdd === 'links') $('[data-add=links]', c).click(); else if (b.dataset.cardAdd === 'where') { locInit(); locin.focus(); } else { const i = $('input:not([type=hidden]), select, textarea', c); if (i) i.focus(); } refresh(); return; }
    if (b.hasAttribute('data-card-rm')) { if (!confirm(D.tRmcard)) return; const c = b.closest('.card'); $$('input:not([type=hidden]), textarea', c).forEach(i => i.value = ''); $$('select.dat', c).forEach(s => s.value = ''); $$('.rows .row, .tags .chip', c).forEach(x => x.remove()); if (c.dataset.card === 'where') locInit(); if (c.dataset.card === 'alarm') alarmsInit([]); delete c.dataset.added; c.hidden = true; cards(); refresh(); return; }
    if (b.dataset.geo === 'swap') { locSwap(); return; }
    if (b.dataset.geo === 'map') { openMap(); return; }
    if (b.dataset.act) { const row = b.closest('.row'); const v = row ? (w(row, 'email') || w(row, 'uri')).value : f.location.value, open_ = u => window.open(u, '_blank', 'noopener'), g = geoOf(); if (b.dataset.act === 'osm') open_(g ? `https://www.openstreetmap.org/?mlat=${g[0]}&mlon=${g[1]}#map=17/${g[0]}/${g[1]}` : 'https://www.openstreetmap.org/search?query=' + encodeURIComponent(v)); else if (b.dataset.act === 'gmaps') open_('https://www.google.com/maps/search/?api=1&query=' + encodeURIComponent(g ? g.join(',') : v)); else if (b.dataset.act === 'mailto') location.href = 'mailto:' + v; else if (b.dataset.act === 'open') open_(/^[a-z]+:/i.test(v) ? v : 'https://' + v); else navigator.clipboard.writeText(v).then(() => { b.title = D.tCopied; b.style.color = 'var(--accent)'; setTimeout(() => b.style.color = '', 800); }); }
  });
  form.addEventListener('click', e => { const x = e.target.closest('.chip b'); if (x && form.contains(x)) { x.closest('.chip').remove(); refresh(); } });
  $('#ed-lock').onclick = () => { mode('edit'); f.summary.focus(); };
  $('#ed-reset').onclick = () => fill(current, openDate);
  async function dayline() {
    const box = $('#ed-dayline'); const day = f.date.value; if (!day) return;
    let h = ''; for (let i = 0; i < 7; i++) h += `<span class="h" style="top:${(i * 14.3).toFixed(1)}%">${7 + i * 2}:00</span>`;
    try {
      const nd = parse(day); nd.setDate(nd.getDate() + 1); const end = `${nd.getFullYear()}-${pad(nd.getMonth() + 1)}-${pad(nd.getDate())}`;
      const r = await fetch(`/api/events?start=${day}&end=${end}`); const evs = await r.json();
      for (const ev of evs) { if (ev.all_day || ev.start.slice(0, 10) !== day) continue; const s = ev.start.slice(11, 16).split(':'), e2 = ev.end.slice(11, 16).split(':'); const top = ((+s[0] + s[1] / 60) - 7) / 14 * 100, hh = Math.max(3, ((+e2[0] + e2[1] / 60) - (+s[0] + s[1] / 60)) / 14 * 100); const me = f.key.value === `${ev.principal}/${ev.cal_id}/${ev.object_id}`; h += `<div class="blk${me ? ' me' : ''}" style="top:${top.toFixed(1)}%;height:${hh.toFixed(1)}%;--c:${ev.color}">${ev.summary}</div>`; }
    } catch (_) {}
    box.innerHTML = h;
  }
  form.addEventListener('submit', async e => {
    e.preventDefault();
    const [principal, cal_id] = f.calendar.value.split('/');
    const allDay = f.all_day.checked;
    const start = allDay ? f.date.value + 'T00:00:00' : f.date.value + 'T' + (f.from.value || '09:00') + ':00';
    const endDate = f.end_date.value || f.date.value;
    const end = allDay ? nextDay(endDate) + 'T00:00:00' : endDate + 'T' + (f.to.value || f.from.value || '10:00') + ':00';
    const people = peopleOf();
    const body = { principal, cal_id, summary: f.summary.value, location: f.location.value, description: f.description.value, start, end, all_day: allDay, rrule: buildRR(), exdates: datesOf('exdates'), rdates: datesOf('rdates'), alarms: alarmsOf(), organizer: people.organizer, attendees: people.attendees, categories: chips(), status: f.status.value, transp: f.transp.value, class: f.class.value, priority: f.priority.value === '' ? null : +f.priority.value, color: colorOf(), geo: geoOf(), links: linksOf(), timezone: root.dataset.tz, etag: f.etag.value };
    const key = f.key.value;
    try {
      const r = await fetch(key ? '/api/events/' + key : '/api/events', { method: key ? 'PUT' : 'POST', headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf }, body: JSON.stringify(body) });
      if (!r.ok) { $('#ed-error').textContent = (r.status === 409 ? T.conflict : T.err + ' ' + r.status) + ': ' + (await r.text()).slice(0, 200); return; }
      close(); load(false);
    } catch (e2) { $('#ed-error').textContent = T.err + ': ' + e2.message; }
  });
  $('#ed-delete').onclick = async () => { if (!confirm(T.del)) return; const r = await fetch('/api/events/' + f.key.value, { method: 'DELETE', headers: { 'X-CSRF-Token': csrf } }); if (r.ok) { close(); load(false); } else $('#ed-error').textContent = T.err + ' ' + r.status; };

  wire();
  window.rusticalCal = { go, openEditor };
})();

/* ===== universal search ===== */
(function () {
  const q = document.getElementById('q'), box = document.getElementById('sres');
  if (!q) return;
  const lang = document.body.dataset.lang || 'de';
  const T = lang === 'en' ? { jump: 'Jump to', ev: 'Events', ct: 'Contacts', none: 'Nothing found' } : { jump: 'Springe zu', ev: 'Termine', ct: 'Kontakte', none: 'Nichts gefunden' };
  const esc = s => String(s).replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
  let t, last = '';
  async function render() {
    const v = q.value.trim(); if (!v) { box.hidden = true; return; } if (v === last) { box.hidden = false; return; } last = v;
    try {
      const r = await fetch('/api/search?q=' + encodeURIComponent(v)); const d = await r.json(); let h = '';
      if (d.jump) h += `<a class="r jump" href="/calendar?view=${d.jump.view}&date=${d.jump.date}"><span class="ic">⇢</span><b>${T.jump} ${esc(d.jump.label)}</b><small>${d.jump.date}</small></a>`;
      if (d.events.length) h += `<h5>${T.ev}</h5>` + d.events.map(e => `<a class="r" href="/calendar?view=day&date=${e.date}" data-key="${esc(e.key)}"><span class="sw" style="--c:${esc(e.color)}"></span><b>${esc(e.summary)}</b><small>${e.date}${e.time ? ' · ' + e.time : ''} · ${esc(e.cal_name)}</small></a>`).join('');
      if (d.contacts.length) h += `<h5>${T.ct}</h5>` + d.contacts.map(c => `<a class="r" href="/contacts#${esc(c.key)}"><span class="av" style="background:var(--c${c.color_index})">${esc(c.initials)}</span><b>${esc(c.name)}</b><small>${esc(c.sub)}</small></a>`).join('');
      if (!h) h = `<div class="none">${T.none}</div>`;
      box.innerHTML = h; box.hidden = false;
      box.querySelectorAll('a.r[href^="/calendar"]').forEach(a => a.addEventListener('click', e => { if (window.rusticalCal) { e.preventDefault(); const u = new URL(a.href); window.rusticalCal.go(u.searchParams.get('view'), u.searchParams.get('date')); box.hidden = true; } }));
    } catch (_) { box.hidden = true; }
  }
  q.addEventListener('input', () => { clearTimeout(t); t = setTimeout(render, 180); });
  q.addEventListener('focus', () => { if (q.value.trim()) box.hidden = false; });
  document.addEventListener('click', e => { if (!e.target.closest('.usearch')) box.hidden = true; });
  addEventListener('keydown', e => { if (e.key === 'Escape') { box.hidden = true; q.blur(); } });
})();

/* ===== contacts page ===== */
(function () {
  const wrap = document.getElementById('contacts');
  if (!wrap) return;
  const $ = (s, r = document) => r.querySelector(s), $$ = (s, r = document) => [...r.querySelectorAll(s)];
  const csrf = document.querySelector('meta[name=csrf]').content;
  const lang = document.body.dataset.lang || 'de';
  const T = lang === 'en' ? { del: 'Delete this contact?', err: 'Error', phone: 'Phone', email: 'Email', kinds: ['home', 'work', 'cell', 'other'], create: 'Create contact', save: 'Save', reset: 'Reset', cancel: 'Cancel', delete: 'Delete' } : { del: 'Diesen Kontakt löschen?', err: 'Fehler', phone: 'Telefon', email: 'E-Mail', kinds: ['home', 'work', 'cell', 'other'], create: 'Kontakt anlegen', save: 'Speichern', reset: 'Zurücksetzen', cancel: 'Abbrechen', delete: 'Löschen' };
  // view switch
  $$('[data-ctview]').forEach(b => b.onclick = () => { const table = b.dataset.ctview === 'table'; wrap.classList.toggle('view-table', table); $$('#colpop [data-for]').forEach(g => g.hidden = g.dataset.for !== (table ? 'table' : 'list')); $$('[data-ctview]').forEach(x => x.setAttribute('aria-pressed', x === b ? 'true' : 'false')); localStorage.ctview = b.dataset.ctview; });
  if (localStorage.ctview === 'table' && !wrap.classList.contains('view-table')) $('[data-ctview=table]').click();
  // columns
  function applyCols() { const on = $$('#colpop input[data-col]').filter(i => i.checked).map(i => i.dataset.col); $$('.sheet [data-c]').forEach(el => el.classList.toggle('off', !on.includes(el.dataset.c))); return on; }
  $$('#colpop input[data-col]').forEach(i => i.addEventListener('change', () => { const on = applyCols(); fetch('/settings/columns', { method: 'POST', headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf }, body: JSON.stringify(on) }); }));
  applyCols();
  // list view: the same picker toggles the lines of every entry
  function applyListFields() { const on = $$('#colpop input[data-lf]').filter(i => i.checked).map(i => i.dataset.lf); $$('.panels [data-lf]').forEach(el => el.classList.toggle('off', !on.includes(el.dataset.lf))); return on; }
  $$('#colpop input[data-lf]').forEach(i => i.addEventListener('change', () => { const on = applyListFields(); fetch('/settings/list-fields', { method: 'POST', headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf }, body: JSON.stringify(on) }); }));
  applyListFields(); $$('#colpop [data-for]').forEach(g => g.hidden = g.dataset.for !== (wrap.classList.contains('view-table') ? 'table' : 'list'));
  // filter by addressbook / category
  const fb = $('#abfilter');
  function applyFilter() {
    const abs = $$('input[data-ab]', fb).filter(i => !i.checked).map(i => i.dataset.ab); const cats = $$('input[data-cat]', fb).filter(i => !i.checked).map(i => i.dataset.cat);
    $$('[data-key][data-ab]', wrap).forEach(el => el.classList.toggle('hide', abs.includes(el.dataset.ab) || cats.includes(el.dataset.cat)));
    $$('.lpanel', wrap).forEach(p => p.classList.toggle('hide', !$$('li:not(.hide)', p).length));
    $$('.ct-rail [data-jump]', wrap).forEach(b => { const p = $(`.lpanel[data-letter="${CSS.escape(b.dataset.jump)}"]`, wrap); b.classList.toggle('hide', !p || p.classList.contains('hide')); });
    if (typeof railMark === 'function') setTimeout(railMark, 0);
  }
  if (fb) $$('input', fb).forEach(i => i.addEventListener('change', applyFilter));
  // quick-jump rail: scroll the letter's panel (list) or first row (table) to the top of its scroller
  const scrollerOf = el => { for (let p = el.parentElement; p; p = p.parentElement) { const o = getComputedStyle(p).overflowY; if (o === 'auto' || o === 'scroll') return p; } return document.querySelector('.main'); };
  $$('.ct-rail [data-jump]', wrap).forEach(b => b.onclick = () => {
    const L = CSS.escape(b.dataset.jump);
    const target = wrap.classList.contains('view-table') ? $(`tr[data-letter="${L}"]:not(.hide)`, wrap) : $(`.lpanel[data-letter="${L}"]`, wrap);
    if (!target) return; const sc = scrollerOf(target); const head = sc.classList.contains('main') ? ($('.toolbar') || {}).offsetHeight || 0 : ($('thead', sc) || {}).offsetHeight || 0;
    sc.scrollTop += target.getBoundingClientRect().top - sc.getBoundingClientRect().top - head - 6;
  });
  // rail: ring on the letter whose group is at the top of the scroller (list: .main, table: .sheetwrap)
  function railMark() {
    const table = wrap.classList.contains('view-table'); const sc = table ? $('.sheetwrap', wrap) : $('.main');
    const line = sc.getBoundingClientRect().top + (table ? ($('thead', sc) || {}).offsetHeight || 0 : ($('.toolbar') || {}).offsetHeight || 0) + 8;
    let cur = null;
    for (const el of $$(table ? 'tr[data-letter]:not(.hide)' : '.lpanel:not(.hide)', wrap)) { if (el.getBoundingClientRect().top <= line) cur = el.dataset.letter; else break; }
    $$('.ct-rail [data-jump]', wrap).forEach(b => b.classList.toggle('cur', b.dataset.jump === cur));
  }
  const railTick = () => { if (!railTick._q) { railTick._q = true; requestAnimationFrame(() => { railTick._q = false; railMark(); }); } };
  $('.main').addEventListener('scroll', railTick); $('.sheetwrap', wrap).addEventListener('scroll', railTick);
  $$('[data-ctview]').forEach(b => b.addEventListener('click', () => setTimeout(railMark, 0)));
  railMark();
  // ---- contact editor: cards, view/edit mode (lock), per-row actions ----
  const det = $('#detail'), form = $('#ctform'), f = form.elements, D = form.dataset;
  const KEY = c => `${c.principal}/${c.ab_id}/${c.object_id}`;
  let current = null, baseline = '', fnAuto = true;
  const wide = () => matchMedia('(min-width: 1100px)').matches;
  // partial dates: "YYYY-MM-DD" | "--MM-DD" <-> three inputs
  const setDate = (p, iso) => { const m = (iso || '').match(/^(?:(\d{4})|-)-(\d{2})-(\d{2})$/); $('[data-p=d]', p).value = m ? m[3] : ''; $('[data-p=m]', p).value = m ? m[2] : ''; $('[data-p=y]', p).value = m && m[1] ? m[1] : ''; };
  const getDate = p => { const d = $('[data-p=d]', p).value.trim(), m = $('[data-p=m]', p).value.trim(), y = $('[data-p=y]', p).value.trim(); if (!d || !m) return ''; const dd = d.padStart(2, '0'), mm = m.padStart(2, '0'); return (y ? y.padStart(4, '0') : '-') + `-${mm}-${dd}`; };
  const dateInfo = p => { const d = +$('[data-p=d]', p).value, m = +$('[data-p=m]', p).value, y = +$('[data-p=y]', p).value; const info = $('.dateinfo', p); if (!info) return; if (!d || !m) { info.textContent = ''; return; } const now = new Date(), t0 = new Date(now.getFullYear(), now.getMonth(), now.getDate()); let next = new Date(now.getFullYear(), m - 1, d); if (next < t0) next = new Date(now.getFullYear() + 1, m - 1, d); const days = Math.round((next - t0) / 864e5); info.textContent = (days === 0 ? D.tToday : D.tIn.replace('{n}', days)) + (y ? ' · ' + D.tTurns.replace('{n}', next.getFullYear() - y) : ''); };
  // list rows from <template>
  const addRow = (list, item) => { const box = $(`[data-list="${list}"]`, form); const tpl = $('template', box); box.appendChild(tpl.content.cloneNode(true)); const row = box.lastElementChild; if (item) { const sel = $('select.typ', row); if (sel && item.kind) { if (![...sel.options].some(o => o.value === item.kind)) sel.add(new Option(item.kind, item.kind)); sel.value = item.kind; } if (row.classList.contains('adr')) { ['street', 'code', 'city', 'country', 'region'].forEach(k => $(`[data-w=${k}]`, row).value = item[k] || ''); } else if (row.classList.contains('xd')) { $('.lbl', row).value = item.label || ''; setDate($('.pdate', row), item.date); dateInfo($('.pdate', row)); } else { $('input', row).value = item.value || ''; } const pref = $('.pref', row); if (pref) pref.setAttribute('aria-pressed', item.pref ? 'true' : 'false'); } return row; };
  const rows = list => [...$$(`[data-list="${list}"] .row`, form)];
  const rowVals = list => rows(list).map(r => { const p = $('.pref', r); return { kind: $('select.typ', r).value, value: $('input:not([type=hidden])', r).value.trim(), pref: !!p && p.getAttribute('aria-pressed') === 'true' }; }).filter(v => v.value);
  const chips = () => $$('.tags .chip', form).map(c => c.firstChild.textContent.trim());
  const addChip = v => { v = v.replace(/,/g, '').trim(); if (!v || chips().includes(v)) return; $('.tags input', form).insertAdjacentHTML('beforebegin', `<span class="chip">${v.replace(/</g, '&lt;')}<b class="edits">×</b></span>`); };
  const hasData = card => $$('input:not([type=hidden]), textarea', card).some(i => !i.closest('template') && i.value.trim() !== '') || !!$('.chip', card);
  function markEmpty() { $$('.field', form).forEach(fl => { const p = $('.pdate', fl); const v = p ? getDate(p) : ($('input, select, textarea', fl) || {}).value; fl.classList.toggle('empty', !(v || '').trim()); }); const mn = $('.morenames', form); mn.classList.toggle('filled', $$('input', mn).some(i => i.value.trim())); }
  function cards() { const edit = form.classList.contains('mode-edit'); $$('.card', form).forEach(c => { c.hidden = !(c.hasAttribute('data-always') || hasData(c) || (edit && c.dataset.added)); }); $$('.addmenu [data-card-add]', form).forEach(b => b.disabled = !$(`.card[data-card="${b.dataset.cardAdd}"]`, form).hidden); }
  function mode(m) { form.classList.toggle('mode-view', m === 'view'); form.classList.toggle('mode-edit', m === 'edit'); cards(); markEmpty(); $$('.pdate', form).forEach(dateInfo); updateButtons(); }
  const serialize = () => [...form.querySelectorAll('input:not([type=hidden]), select, textarea')].filter(e => !e.closest('template')).map(e => e.value).join('|') + '|' + chips().join(',') + '|' + $$('.pref[aria-pressed=true]', form).length;
  function updateButtons() { const changed = serialize() !== baseline; $('#ct-save').disabled = !changed; $('#ct-reset').disabled = !changed && form.classList.contains('new'); $('#ct-delete').disabled = form.classList.contains('mode-view') || form.classList.contains('new'); }
  const initials = n => n.split(/\s+/).map(w => w[0]).slice(0, 2).join('').toUpperCase();
  function show(c) {
    current = c; form.reset(); $('#ct-error').textContent = '';
    f.key.value = c ? KEY(c) : ''; f.etag.value = c ? c.etag : '';
    ['given', 'family', 'nickname', 'prefix', 'additional', 'suffix', 'org', 'department', 'title', 'note', 'full_name'].forEach(k => f[k].value = c ? (c[k] || '') : '');
    fnAuto = !c || !c.full_name || c.full_name === [c.given, c.family].filter(Boolean).join(' ');
    f.ab.value = c ? `${c.principal}/${c.ab_id}` : f.ab.options[0].value;
    setDate($('[data-date=birthday]', form), c ? c.birthday : ''); setDate($('[data-date=anniversary]', form), c ? c.anniversary : '');
    ['phones', 'emails', 'addresses', 'urls', 'impp', 'related', 'dates'].forEach(l => { rows(l).forEach(r => r.remove()); (c ? c[l] : []).forEach(it => addRow(l, it)); });
    if (!c) addRow('phones', { kind: 'cell', value: '' });
    $$('.tags .chip', form).forEach(x => x.remove()); (c ? c.categories : []).forEach(addChip);
    $('.morenames', form).hidden = true; $('[data-more]', form).hidden = false;
    $$('.card[data-added]', form).forEach(x => x.removeAttribute('data-added'));
    $('#ct-av').textContent = c ? initials(c.full_name || '?') : '?'; $('#ct-av').style.background = c ? `var(--c${1 + (c.uid.length % 6)})` : 'var(--accent)';
    $('#ct-sub').textContent = c ? [c.org, c.categories.join(', ')].filter(Boolean).join(' · ') : '';
    form.classList.toggle('new', !c); f.full_name.placeholder = c ? D.tDisplay : D.tNewtitle;
    const b = $('#ct-save'); b.title = c ? D.tSave : D.tCreate; b.setAttribute('aria-label', b.title);
    mode(c ? 'view' : 'edit'); baseline = serialize(); updateButtons();
    det.hidden = false; det.classList.add('on');
    $$('.list li.on').forEach(l => l.classList.remove('on')); if (c) { const li = $(`.list li[data-key="${f.key.value}"]`); if (li) li.classList.add('on'); }
  }
  async function open(key) { try { const r = await fetch('/api/contacts/' + key); if (!r.ok) throw new Error(r.status); show(await r.json()); } catch (e) { alert(T.err + ': ' + e.message); } }
  $$('.list li[data-key]').forEach(li => li.onclick = () => open(li.dataset.key));
  $$('[data-open]').forEach(b => b.onclick = () => { wrap.classList.remove('view-table'); $('[data-ctview=list]').click(); open(b.dataset.open); });
  // editing: dirty tracking, display name follows given + family, dates re-evaluate
  form.addEventListener('input', e => { if (e.target === f.given || e.target === f.family) { if (fnAuto) f.full_name.value = [f.given.value, f.family.value].filter(Boolean).join(' '); } else if (e.target === f.full_name) fnAuto = !f.full_name.value; const p = e.target.closest('.pdate'); if (p) dateInfo(p); updateButtons(); });
  form.addEventListener('change', updateButtons);
  form.addEventListener('click', e => {
    const b = e.target.closest('button'); if (!b || !form.contains(b)) return;
    if (b.dataset.add) { const row = addRow(b.dataset.add, b.dataset.add === 'phones' ? { kind: 'cell' } : b.dataset.add === 'emails' || b.dataset.add === 'addresses' ? { kind: 'home' } : null); const i = $('input:not([type=hidden])', row); if (i) i.focus(); updateButtons(); return; }
    if (b.hasAttribute('data-rm')) { b.closest('.row').remove(); updateButtons(); return; }
    if (b.classList.contains('pref')) { const list = b.closest('[data-list]'); const on = b.getAttribute('aria-pressed') === 'true'; $$('.pref', list).forEach(p => p.setAttribute('aria-pressed', p === b && !on ? 'true' : 'false')); updateButtons(); return; }
    if (b.hasAttribute('data-more')) { $('.morenames', form).hidden = false; b.hidden = true; return; }
    if (b.dataset.cardAdd) { const c = $(`.card[data-card="${b.dataset.cardAdd}"]`, form); c.dataset.added = '1'; c.hidden = false; b.disabled = true; b.closest('.addmenu').classList.remove('open'); if (b.dataset.cardAdd === 'email') addRow('emails', { kind: 'home' }); if (b.dataset.cardAdd === 'adr') addRow('addresses', { kind: 'home' }); const i = $('input:not([type=hidden]), textarea', c); if (i && !i.closest('template')) i.focus(); return; }
    if (b.hasAttribute('data-card-rm')) { if (!confirm(D.tRmcard)) return; const c = b.closest('.card'); $$('input:not([type=hidden]), textarea', c).forEach(i => { if (!i.closest('template')) i.value = ''; }); $$('.rows .row', c).forEach(x => x.remove()); $$('.chip', c).forEach(x => x.remove()); delete c.dataset.added; c.hidden = true; cards(); updateButtons(); return; }
    if (b.dataset.act) { action(b); return; }
  });
  form.addEventListener('click', e => { const x = e.target.closest('.chip b'); if (x && form.contains(x)) { x.closest('.chip').remove(); updateButtons(); } });
  const tagIn = $('.tags input', form);
  tagIn.addEventListener('keydown', e => { if (e.key === 'Enter' || e.key === ',') { e.preventDefault(); addChip(tagIn.value); tagIn.value = ''; updateButtons(); } else if (e.key === 'Backspace' && !tagIn.value) { const c = tagIn.previousElementSibling; if (c && c.classList.contains('chip')) { c.remove(); updateButtons(); } } });
  tagIn.addEventListener('blur', () => { if (tagIn.value.trim()) { addChip(tagIn.value); tagIn.value = ''; updateButtons(); } });
  // view-mode actions
  function action(b) {
    const row = b.closest('.row'); const val = row ? ($('input:not([type=hidden])', row) || {}).value || '' : ''; const digits = val.replace(/[^0-9+]/g, ''); const open_ = u => window.open(u, '_blank', 'noopener');
    const adrText = () => ['street', 'code', 'city', 'country'].map(k => ($(`[data-w=${k}]`, row) || {}).value || '').filter(Boolean).join(', ');
    switch (b.dataset.act) {
      case 'tel': location.href = 'tel:' + digits; break;
      case 'sms': location.href = 'sms:' + digits; break;
      case 'wa': open_('https://wa.me/' + digits.replace(/^\+/, '').replace(/^00/, '').replace(/^0/, '49')); break;
      case 'mailto': location.href = 'mailto:' + val; break;
      case 'open': open_(/^https?:/.test(val) ? val : 'https://' + val); break;
      case 'osm': open_('https://www.openstreetmap.org/search?query=' + encodeURIComponent(adrText())); break;
      case 'gmaps': open_('https://www.google.com/maps/search/?api=1&query=' + encodeURIComponent(adrText())); break;
      case 'copy': navigator.clipboard.writeText(row.classList.contains('adr') ? adrText() : val).then(() => { b.title = D.tCopied; b.style.color = 'var(--accent)'; setTimeout(() => b.style.color = '', 800); }); break;
      case 'im': { const svc = $('select.typ', row).value; const u = { Signal: 'https://signal.me/#p/' + digits, WhatsApp: 'https://wa.me/' + digits.replace(/^\+/, ''), Telegram: 'https://t.me/' + val.replace(/^@/, ''), Matrix: 'https://matrix.to/#/' + val, XMPP: 'xmpp:' + val, Threema: 'https://threema.id/' + val }[svc]; if (u) open_(u); break; }
      case 'goto': { const li = $$('.list li[data-key]').find(l => (l.dataset.name || '').toLowerCase() === val.toLowerCase()); if (li) { open(li.dataset.key); const sc = $('.main'); sc.scrollTop += li.getBoundingClientRect().top - sc.getBoundingClientRect().top - 120; } else alert(D.tMissing); break; }
    }
  }
  $('#ct-lock').onclick = () => mode('edit');
  $('#ct-reset').onclick = () => show(current);
  $('#ct-cancel').onclick = () => { if (wide()) show(null); else { det.classList.remove('on'); det.hidden = true; } };
  $('#ct-vcf').onclick = () => { if (f.key.value) location.href = '/api/contacts/' + f.key.value + '/vcf'; };
  if (location.hash.length > 1) open(location.hash.slice(1)); else if (wide()) show(null);
  function collect() {
    const [principal, ab_id] = f.ab.value.split('/');
    const g = k => f[k].value.trim();
    return { principal, ab_id, full_name: g('full_name'), given: g('given'), family: g('family'), nickname: g('nickname'), prefix: g('prefix'), additional: g('additional'), suffix: g('suffix'), org: g('org'), department: g('department'), title: g('title'), note: g('note'),
      phones: rowVals('phones'), emails: rowVals('emails'), urls: rowVals('urls'), impp: rowVals('impp'), related: rowVals('related'),
      addresses: rows('addresses').map(r => { const o = { kind: $('select.typ', r).value, pref: $('.pref', r).getAttribute('aria-pressed') === 'true' }; ['street', 'code', 'city', 'country', 'region'].forEach(k => o[k] = $(`[data-w=${k}]`, r).value.trim()); return o; }).filter(a => a.street || a.city || a.code || a.country),
      dates: rows('dates').map(r => ({ label: $('.lbl', r).value.trim(), date: getDate($('.pdate', r)) })).filter(d => d.date),
      birthday: getDate($('[data-date=birthday]', form)), anniversary: getDate($('[data-date=anniversary]', form)), categories: chips() };
  }
  form.addEventListener('submit', async e => {
    e.preventDefault(); const key = f.key.value; const body = collect();
    const r = await fetch(key ? '/api/contacts/' + key : '/api/contacts', { method: key ? 'PUT' : 'POST', headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf }, body: JSON.stringify(key ? { ...body, etag: f.etag.value } : body) });
    if (!r.ok) { $('#ct-error').textContent = T.err + ' ' + r.status + ': ' + (await r.text()).slice(0, 200); return; }
    const { id } = await r.json(); location.hash = `${body.principal}/${body.ab_id}/${id}`; location.reload();
  });
  $('#ct-delete').onclick = async () => { if (!confirm(T.del)) return; const r = await fetch('/api/contacts/' + f.key.value, { method: 'DELETE', headers: { 'X-CSRF-Token': csrf } }); if (r.ok) { location.hash = ''; location.reload(); } };
  // table inline edit: fetch → patch one field → PUT
  $$('.sheet td[contenteditable][data-f], .sheet .pin [contenteditable][data-f]').forEach(cell => {
    let before = '';
    cell.addEventListener('focus', () => before = cell.textContent);
    cell.addEventListener('keydown', e => { if (e.key === 'Enter') { e.preventDefault(); cell.blur(); } });
    cell.addEventListener('blur', async () => {
      const v = cell.textContent.trim(); if (v === before.trim()) return;
      const tr = cell.closest('tr'); const key = tr.dataset.key;
      try {
        const c = await (await fetch('/api/contacts/' + key)).json();
        const body = { ...c }; // the contact JSON is a superset of the input: nothing gets dropped
        switch (cell.dataset.f) {
          case 'full_name': { body.full_name = v; const parts = v.split(/\s+/); body.given = parts.slice(0, -1).join(' '); body.family = parts.slice(-1)[0] || ''; break; }
          case 'phone': body.phones = v ? [{ kind: (c.phones[0] || {}).kind || 'cell', value: v }, ...c.phones.slice(1)] : c.phones.slice(1); break;
          case 'email': body.emails = v ? [{ kind: (c.emails[0] || {}).kind || 'home', value: v }, ...c.emails.slice(1)] : c.emails.slice(1); break;
          case 'category': body.categories = v ? [v] : []; break;
          case 'birthday': { const m = v.match(/^(\d{1,2})\.(\d{1,2})\.(\d{4})?$/); body.birthday = m ? (m[3] ? `${m[3]}-${m[2].padStart(2, '0')}-${m[1].padStart(2, '0')}` : `--${m[2].padStart(2, '0')}-${m[1].padStart(2, '0')}`) : v; break; }
          case 'address': body.addresses = v ? [{ ...(c.addresses[0] || { kind: 'home' }), street: v, code: '', city: '', region: '', country: '' }, ...c.addresses.slice(1)] : c.addresses.slice(1); break;
          case 'org': body.org = v; break;
        }
        const r = await fetch('/api/contacts/' + key, { method: 'PUT', headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf }, body: JSON.stringify(body) });
        if (!r.ok) throw new Error(r.status);
        cell.style.outline = '2px solid var(--c2)'; setTimeout(() => cell.style.outline = '', 800);
      } catch (e) { cell.textContent = before; alert(T.err + ': ' + e.message); }
    });
  });
  const newRow = $('.sheet tr.new [data-new]'); if (newRow) { newRow.addEventListener('focus', () => { if (newRow.textContent.endsWith('…')) newRow.textContent = ''; }); newRow.addEventListener('keydown', e => { if (e.key === 'Enter') { e.preventDefault(); const v = newRow.textContent.trim(); if (!v) return; show(null); const parts = v.split(/\s+/); f.given.value = parts.slice(0, -1).join(' '); f.family.value = parts.slice(-1)[0]; wrap.classList.remove('view-table'); } }); }
})();

/* ===== tasks page ===== */
(function () {
  const wrap = document.getElementById('tasks');
  if (!wrap) return;
  const csrf = document.querySelector('meta[name=csrf]').content;
  const data = (() => { try { return JSON.parse(document.getElementById('tasks-data').textContent); } catch (_) { return []; } })();
  const byKey = Object.fromEntries(data.map(t => [`${t.principal}/${t.cal_id}/${t.object_id}`, t]));
  const form = document.getElementById('taskform');
  form.addEventListener('submit', async e => {
    e.preventDefault(); const f = form.elements; const [principal, cal_id] = f.calendar.value.split('/');
    const r = await fetch('/api/tasks', { method: 'POST', headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf }, body: JSON.stringify({ principal, cal_id, summary: f.summary.value, due: f.due.value || null, priority: 0 }) });
    if (r.ok) location.reload(); else alert('Error ' + r.status + ': ' + await r.text());
  });
  wrap.querySelectorAll('[data-toggle]').forEach(cb => cb.addEventListener('change', async () => {
    const t = byKey[cb.dataset.toggle]; if (!t) return;
    const r = await fetch('/api/tasks/' + cb.dataset.toggle, { method: 'PUT', headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf }, body: JSON.stringify({ principal: t.principal, cal_id: t.cal_id, summary: t.summary, description: t.description, due: t.due ? t.due.slice(0, 10) : null, priority: t.priority, completed: cb.checked }) });
    if (r.ok) location.reload(); else { cb.checked = !cb.checked; alert('Error ' + r.status); }
  }));
  wrap.querySelectorAll('[data-del]').forEach(b => b.addEventListener('click', async () => {
    if (!confirm(document.body.dataset.lang === 'en' ? 'Delete this task?' : 'Diese Aufgabe löschen?')) return;
    const r = await fetch('/api/tasks/' + b.dataset.del, { method: 'DELETE', headers: { 'X-CSRF-Token': csrf } }); if (r.ok) location.reload();
  }));
})();

/* ===== settings: import files, browser push ===== */
(function () {
  const settings = document.querySelector('.settings');
  if (!settings) return;
  const csrf = document.querySelector('meta[name=csrf]').content;
  const lang = document.body.dataset.lang || 'de';
  const T = lang === 'en' ? { done: n => n + ' objects imported.', err: 'Error', active: 'Active on this device.', denied: 'Notifications were denied in the browser.', unsupported: 'This browser does not support push notifications.' } : { done: n => n + ' Objekte importiert.', err: 'Fehler', active: 'Aktiv auf diesem Gerät.', denied: 'Benachrichtigungen wurden im Browser abgelehnt.', unsupported: 'Dieser Browser unterstützt keine Push-Nachrichten.' };
  // import: read the chosen file, POST its bytes (no multipart)
  settings.querySelectorAll('input[type=file][data-import]').forEach(inp => inp.addEventListener('change', async () => {
    const file = inp.files[0]; if (!file) return;
    const r = await fetch(inp.dataset.import, { method: 'POST', headers: { 'Content-Type': 'application/octet-stream', 'X-CSRF-Token': csrf }, body: await file.arrayBuffer() });
    if (r.ok) { const d = await r.json(); alert(T.done(d.imported)); location.reload(); } else alert(T.err + ' ' + r.status + ': ' + (await r.text()).slice(0, 300));
    inp.value = '';
  }));
  // push
  const en = document.getElementById('push-enable'), dis = document.getElementById('push-disable'), st = document.getElementById('push-status'), list = document.getElementById('push-devices');
  if (!en) return;
  const b64 = s => { const p = '='.repeat((4 - s.length % 4) % 4); const raw = atob((s + p).replace(/-/g, '+').replace(/_/g, '/')); return Uint8Array.from(raw, c => c.charCodeAt(0)); };
  async function refresh() {
    try { const subs = await (await fetch('/api/push/subscriptions')).json(); list.innerHTML = subs.map(s => `<li><small class="muted">${(s.user_agent || '').slice(0, 60)} · ${s.created_at}</small></li>`).join('') || ''; } catch (_) {}
    if (!('serviceWorker' in navigator) || !('PushManager' in window)) { st.textContent = T.unsupported; en.disabled = true; return; }
    const reg = await navigator.serviceWorker.getRegistration('/'); const sub = reg && await reg.pushManager.getSubscription();
    en.hidden = !!sub; dis.hidden = !sub; st.textContent = sub ? T.active : '';
  }
  en.onclick = async () => {
    try {
      const perm = await Notification.requestPermission(); if (perm !== 'granted') { st.textContent = T.denied; return; }
      const reg = await navigator.serviceWorker.register('/assets/sw.js', { scope: '/' });
      const key = (await (await fetch('/api/push/key')).json()).key;
      const sub = await reg.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: b64(key) });
      const j = sub.toJSON();
      const r = await fetch('/api/push/subscriptions', { method: 'POST', headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf }, body: JSON.stringify({ endpoint: j.endpoint, keys: j.keys }) });
      if (!r.ok) throw new Error(r.status);
      refresh();
    } catch (e) { st.textContent = T.err + ': ' + e.message; }
  };
  dis.onclick = async () => {
    const reg = await navigator.serviceWorker.getRegistration('/'); const sub = reg && await reg.pushManager.getSubscription();
    if (sub) { await fetch('/api/push/subscriptions', { method: 'DELETE', headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf }, body: JSON.stringify({ endpoint: sub.endpoint }) }); await sub.unsubscribe(); }
    refresh();
  };
  refresh();
})();
