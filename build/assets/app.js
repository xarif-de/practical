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
  const T = lang === 'en' ? {saved:'saved', err:'Error', del:'Delete this event?', series:'This is a recurring event; the whole series is changed.', newEv:'New event', editEv:'Edit event', conflict:'Changed elsewhere — reload.'} : {saved:'gespeichert', err:'Fehler', del:'Diesen Termin löschen?', series:'Serientermin — Änderungen gelten für die ganze Serie.', newEv:'Neuer Termin', editEv:'Termin bearbeiten', conflict:'Anderswo geändert — bitte neu laden.'};
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

  // ---- editor ----
  const ed = $('#ed'), form = $('#edform'), f = form.elements;
  const presets = (() => { try { return JSON.parse($('#presets-data').textContent || '[]'); } catch (_) { return []; } })();
  const presetBox = $('#ed-presets');
  if (presets.length) presetBox.innerHTML = '<div class="presetchips">' + presets.map((p, i) => `<button type="button" class="btn" data-preset="${i}">${p.title}</button>`).join('') + '</div>';
  $$('[data-preset]', presetBox).forEach(b => b.onclick = () => { const p = presets[+b.dataset.preset]; if (p.title) f.summary.value = p.title; if (p.calendar) f.calendar.value = p.calendar; if (p.category) f.categories.value = p.category; if (p.all_day) setAllDay(true); syncDot(); });
  function setAllDay(on) { f.all_day.checked = on; $('#ed-allday').classList.toggle('on', on); f.from.disabled = on; f.to.disabled = on; $('#ed-times').hidden = on; }
  $('#ed-allday').addEventListener('click', e => { e.preventDefault(); setAllDay(!f.all_day.checked); });
  f.rrule.addEventListener('change', () => { f.rrule_custom.hidden = f.rrule.value !== 'custom'; });
  function syncDot() { const o = f.calendar.selectedOptions[0]; $('#ed-dot').style.background = o ? o.dataset.color : ''; }
  f.calendar.addEventListener('change', syncDot);
  const tl = $('#ed-tl'); function setTl(on) { ed.classList.toggle('no-tl', !on); tl.setAttribute('aria-pressed', on ? 'true' : 'false'); localStorage.edtl = on ? '1' : '0'; }
  tl.onclick = () => setTl(ed.classList.contains('no-tl')); setTl(localStorage.edtl !== '0');
  function close() { ed.hidden = true; }
  $('#ed-close').onclick = close; $('#ed-cancel').onclick = close; ed.addEventListener('click', e => { if (e.target === ed) close(); });
  addEventListener('keydown', e => { if (e.key === 'Escape') { close(); pk.hidden = true; } });
  // all-day: the API end is the exclusive DTEND; the editor shows the last day (empty = one day)
  const addDays = (s, n) => { const e = new Date(s + 'T00:00:00Z'); e.setUTCDate(e.getUTCDate() + n); return e.toISOString().slice(0, 10); }; // UTC arithmetic: no zone shift
  const lastDay = d => { const s = addDays(d.end.slice(0, 10), -1); return s > d.start.slice(0, 10) ? s : ''; };
  const nextDay = s => addDays(s, 1);
  function fill(d) {
    form.reset(); $('#ed-error').textContent = ''; f.rrule_custom.hidden = true;
    f.key.value = d ? `${d.principal}/${d.cal_id}/${d.object_id}` : ''; f.etag.value = d ? d.etag : '';
    $('#edh').textContent = d ? T.editEv : T.newEv; $('#ed-delete').hidden = !d;
    if (d) {
      f.summary.value = d.summary; f.location.value = d.location; f.description.value = d.description; f.categories.value = d.categories.join(', ');
      f.calendar.value = `${d.principal}/${d.cal_id}`; f.date.value = d.start.slice(0, 10); f.end_date.value = d.all_day ? lastDay(d) : d.end.slice(0, 10);
      f.from.value = d.start.slice(11, 16); f.to.value = d.end.slice(11, 16); setAllDay(d.all_day);
      f.alarm.value = d.alarm_minutes == null ? '' : String(d.alarm_minutes);
      if (d.rrule) { const std = ['FREQ=DAILY','FREQ=WEEKLY','FREQ=MONTHLY','FREQ=YEARLY']; if (std.includes(d.rrule)) f.rrule.value = d.rrule; else { f.rrule.value = 'custom'; f.rrule_custom.value = d.rrule; f.rrule_custom.hidden = false; } }
      $('#ed-hint').textContent = d.rrule ? T.series : '';
    } else { $('#ed-hint').textContent = ''; }
    syncDot();
  }
  async function openEditor(key, date) {
    if (key) {
      try { const r = await fetch('/api/events/' + key); if (!r.ok) throw new Error(r.status); fill(await r.json()); } catch (e) { alert(T.err + ': ' + e.message); return; }
    } else {
      fill(null); f.date.value = date || state.date; f.end_date.value = f.date.value; f.from.value = '09:00'; f.to.value = '10:00'; setAllDay(false);
      if (state.cals) { const first = state.cals.split(',')[0]; if ([...f.calendar.options].some(o => o.value === first)) f.calendar.value = first; syncDot(); }
    }
    ed.hidden = false; f.summary.focus(); dayline();
  }
  f.date.addEventListener('change', () => { if (f.end_date.value < f.date.value) f.end_date.value = f.date.value; dayline(); });
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
    const rrule = f.rrule.value === 'custom' ? f.rrule_custom.value : f.rrule.value;
    const body = { principal, cal_id, summary: f.summary.value, location: f.location.value, description: f.description.value, start, end, all_day: allDay, rrule: rrule || null, alarm_minutes: f.alarm.value === '' ? null : +f.alarm.value, categories: f.categories.value.split(',').map(s => s.trim()).filter(Boolean), timezone: root.dataset.tz, etag: f.etag.value };
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
  const T = lang === 'en' ? { del: 'Delete this contact?', err: 'Error', phone: 'Phone', email: 'Email', kinds: ['home', 'work', 'cell', 'other'] } : { del: 'Diesen Kontakt löschen?', err: 'Fehler', phone: 'Telefon', email: 'E-Mail', kinds: ['home', 'work', 'cell', 'other'] };
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
  // detail editor
  const det = $('#detail'), form = $('#ctform'), f = form.elements, multi = $('#ct-multi');
  function row(kind, type, value) { const k = kind === 'phone' ? 'phones' : 'emails'; return `<div class="multi" data-k="${k}"><select>${T.kinds.map(x => `<option${x === type ? ' selected' : ''}>${x}</option>`).join('')}</select><input value="${String(value).replace(/"/g, '&quot;')}" placeholder="${kind === 'phone' ? T.phone : T.email}"><button class="link danger" type="button" data-rm>✕</button></div>`; }
  multi.addEventListener('click', e => { if (e.target.dataset.rm !== undefined) e.target.closest('.multi').remove(); });
  $('#ct-addf').onclick = () => { multi.insertAdjacentHTML('beforeend', row('phone', 'cell', '') + row('email', 'home', '')); };
  function show(c) {
    form.reset(); $('#ct-error').textContent = '';
    f.key.value = c ? `${c.principal}/${c.ab_id}/${c.object_id}` : ''; f.etag.value = c ? c.etag : '';
    $('#ct-h').textContent = c ? c.full_name : (lang === 'en' ? 'New contact' : 'Neuer Kontakt'); $('#ct-sub').textContent = c ? c.ab_name : '';
    $('#ct-av').textContent = c ? c.full_name.split(/\s+/).map(w => w[0]).slice(0, 2).join('').toUpperCase() : '+'; $('#ct-av').style.background = c ? `var(--c${1 + (c.uid.length % 6)})` : 'var(--accent)';
    multi.innerHTML = '';
    if (c) { f.given.value = c.given; f.family.value = c.family; f.org.value = c.org; f.birthday.value = c.birthday; f.address.value = (c.addresses[0] || {}).value || ''; f.note.value = c.note; f.ab.value = `${c.principal}/${c.ab_id}`; f.category.value = c.categories[0] || '';
      c.phones.forEach(p => multi.insertAdjacentHTML('beforeend', row('phone', p.kind, p.value))); c.emails.forEach(p => multi.insertAdjacentHTML('beforeend', row('email', p.kind, p.value))); }
    else { multi.insertAdjacentHTML('beforeend', row('phone', 'cell', '') + row('email', 'home', '')); }
    $('#ct-delete').hidden = !c;
    det.hidden = false; det.classList.add('on');
    $$('.list li.on').forEach(l => l.classList.remove('on')); if (c) { const li = $(`.list li[data-key="${f.key.value}"]`); if (li) li.classList.add('on'); }
  }
  async function open(key) { try { const r = await fetch('/api/contacts/' + key); if (!r.ok) throw new Error(r.status); show(await r.json()); } catch (e) { alert(T.err + ': ' + e.message); } }
  $$('.list li[data-key]').forEach(li => li.onclick = () => open(li.dataset.key));
  $$('[data-open]').forEach(b => b.onclick = () => { wrap.classList.remove('view-table'); $('[data-ctview=list]').click(); open(b.dataset.open); });
  $('#ct-new').onclick = () => show(null);
  // wide list view: the editor is always visible, closing just returns to the empty (new contact) form
  const wide = () => matchMedia('(min-width: 1100px)').matches;
  $('#ct-close').onclick = () => { if (wide()) show(null); else { det.classList.remove('on'); det.hidden = true; } }; $('#ct-cancel').onclick = $('#ct-close').onclick;
  if (location.hash.length > 1) open(location.hash.slice(1)); else if (wide()) show(null);
  function collect() {
    const [principal, ab_id] = f.ab.value.split('/');
    const lists = { phones: [], emails: [] };
    $$('.multi', multi).forEach(m => { const v = m.querySelector('input').value.trim(); if (v) lists[m.dataset.k].push({ kind: m.querySelector('select').value, value: v }); });
    return { principal, ab_id, given: f.given.value, family: f.family.value, org: f.org.value, birthday: f.birthday.value, addresses: f.address.value.trim() ? [{ kind: 'home', value: f.address.value.trim() }] : [], note: f.note.value, categories: f.category.value.trim() ? [f.category.value.trim()] : [], phones: lists.phones, emails: lists.emails, etag: f.etag.value };
  }
  form.addEventListener('submit', async e => {
    e.preventDefault(); const key = f.key.value; const body = collect();
    const r = await fetch(key ? '/api/contacts/' + key : '/api/contacts', { method: key ? 'PUT' : 'POST', headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf }, body: JSON.stringify(body) });
    if (!r.ok) { $('#ct-error').textContent = T.err + ' ' + r.status + ': ' + (await r.text()).slice(0, 200); return; }
    location.reload();
  });
  $('#ct-delete').onclick = async () => { if (!confirm(T.del)) return; const r = await fetch('/api/contacts/' + f.key.value, { method: 'DELETE', headers: { 'X-CSRF-Token': csrf } }); if (r.ok) location.reload(); };
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
        const body = { principal: c.principal, ab_id: c.ab_id, given: c.given, family: c.family, full_name: c.full_name, org: c.org, title: c.title, emails: c.emails, phones: c.phones, addresses: c.addresses, birthday: c.birthday, anniversary: c.anniversary, categories: c.categories, note: c.note, url: c.url, etag: c.etag };
        switch (cell.dataset.f) {
          case 'full_name': { body.full_name = v; const parts = v.split(/\s+/); body.given = parts.slice(0, -1).join(' '); body.family = parts.slice(-1)[0] || ''; break; }
          case 'phone': body.phones = v ? [{ kind: (c.phones[0] || {}).kind || 'cell', value: v }, ...c.phones.slice(1)] : c.phones.slice(1); break;
          case 'email': body.emails = v ? [{ kind: (c.emails[0] || {}).kind || 'home', value: v }, ...c.emails.slice(1)] : c.emails.slice(1); break;
          case 'category': body.categories = v ? [v] : []; break;
          case 'birthday': { const m = v.match(/^(\d{1,2})\.(\d{1,2})\.(\d{4})?$/); body.birthday = m ? (m[3] ? `${m[3]}-${m[2].padStart(2, '0')}-${m[1].padStart(2, '0')}` : `--${m[2].padStart(2, '0')}-${m[1].padStart(2, '0')}`) : v; break; }
          case 'address': body.addresses = v ? [{ kind: (c.addresses[0] || {}).kind || 'home', value: v }] : []; break;
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
