/* ===== calendar page ===== */
(function () {
  const root = document.getElementById('viewroot');
  if (!root) return;
  const { $, $$, esc, pad, ymd, parseYmd: parse } = practical, csrf = practical.csrf();
  const MON = MONTHS;
  const DOW = WEEKDAYS;
  const T = trs('js-ed-', ['copied']);
  const edOpen = (key, date) => window.rusticalEd && window.rusticalEd.open(key, date);
  // date = the picked day, which every view shows as selected; month (YYYY-MM) / week (its Monday) = what the
  // month / week view has on screen once scrolling left the picked day's — scrolling never changes the picked day
  const q0 = new URL(location.href).searchParams;
  let state = { view: root.dataset.view, date: root.dataset.date, cals: root.dataset.cals || '', month: q0.get('month'), week: q0.get('week') };
  const shown = (s = state) => s.view === 'month' && s.month ? s.month + '-01' : s.view === 'week' && s.week ? s.week : s.date; // the date whose month or week is on screen
  root.dataset.date = shown();
  let loading = null; // before the filter setup below: a filter kept in localStorage calls load() right away
  // the fragment on screen as fetched ({ href, html }) while nothing was added to it since (week blocks,
  // ledger rows, a carried ledger): a redraw-only load that gets the same text back leaves the view alone —
  // the live echo of the own save, a change elsewhere that this view does not show
  let onScreen = null;
  const today = root.dataset.today;

  // ---- filter (localStorage-persisted set of hidden calendars) ----
  const filterBox = $('#calfilter');
  const hidden = new Set(JSON.parse(localStorage.calHidden || '[]'));
  function calsParam() { const all = $$('input[data-cal]', filterBox).map(i => i.dataset.cal); const on = all.filter(c => !hidden.has(c)); return on.length === all.length ? '' : on.join(',') || '-'; } // '-' matches no calendar: an empty list means all of them
  if (filterBox) {
    $$('input[data-cal]', filterBox).forEach(i => { i.checked = !hidden.has(i.dataset.cal); i.addEventListener('change', () => { if (i.checked) hidden.delete(i.dataset.cal); else hidden.add(i.dataset.cal); persistFilter(); load(); }); });
    $$('[data-filter]', filterBox).forEach(a => a.addEventListener('click', e => { e.preventDefault(); const all = $$('input[data-cal]', filterBox); all.forEach(i => { const mine = !i.parentElement.querySelector('small'); i.checked = a.dataset.filter === 'all' || (a.dataset.filter === 'mine' && mine); if (i.checked) hidden.delete(i.dataset.cal); else hidden.add(i.dataset.cal); }); persistFilter(); load(); }));
    state.cals = calsParam(); practical.keepCals('calendar', state.cals);
    if (state.cals !== (root.dataset.cals || '')) load(false);
  }
  function persistFilter() { localStorage.calHidden = JSON.stringify([...hidden]); state.cals = calsParam(); practical.keepCals('calendar', state.cals); dropCarried(); }
  // holidays: a setting of the user (not of this browser), saved at once, then the view redraws
  $$('input[data-hol]', filterBox || document.createElement('div')).forEach(i => i.addEventListener('change', async () => {
    const body = {}; $$('input[data-hol]', filterBox).forEach(x => { body[x.dataset.hol] = x.checked; }); // no school switch → the setting stays
    practical.tell(await practical.send('/settings/holiday-display', 'POST', body));
    dropCarried(); load(false);
  }));

  // ---- navigation / fragment loading ----
  function url(fragment, s = state) { const u = new URL('/calendar', location.origin); u.searchParams.set('view', s.view); u.searchParams.set('date', s.date); if (s.view === 'month' && s.month && s.month !== s.date.slice(0, 7)) u.searchParams.set('month', s.month); if (s.view === 'week' && s.week && s.week !== localIso(weekStart(parse(s.date)))) u.searchParams.set('week', s.week); if (s.cals) u.searchParams.set('cals', s.cals); if (fragment) u.searchParams.set('fragment', '1'); return u; }
  async function load(push = true, prev = null) { // prev: the state before a page turn / view switch, back in place when its fetch fails
    const u = url(true);
    loading = u;
    // a month-to-month change glides (wheel, rail, orbit picker, today alike); the months in between are fetched alongside the target
    const turnOf = from => root.dataset.view === 'month' && state.view === 'month' && from.slice(0, 7) !== shown().slice(0, 7) ? (shown() > from ? 1 : -1) : 0;
    const from0 = root.dataset.date, between0 = turnOf(from0) ? monthsBetween(from0, shown()) : [];
    const midsP = between0.length && between0.length <= FLIP_FETCH && !reducedMotion() ? fetchMonths(between0) : Promise.resolve(null);
    const done = practical.progress(300); // also while a prefetch of it is still on its way
    try {
      let html = await practical.prefetched(u);
      // a failed fetch is told and leaves the view as it is; a session that ended goes to the login (practical.req), never into the view
      if (html === null) { const r = await practical.req(u, { headers: { Accept: 'text/html' }, as: 'text' }); if (loading !== u) return; if (!r.ok) { if (prev) state = prev; practical.tell(r); return; } html = r.data; }
      if (loading !== u) return;
      if (!push && onScreen && onScreen.href === u.href && onScreen.html === html) return;
      const mids = await midsP; tlScroll = $('.tl', root) ? $('.tl', root).scrollTop : null;
      ledScroll = ledNode ? ledNode.scrollTop : null; // the ledger is carried over, and a detached node forgets where it stood
      if (flip) flip.end(); const from = root.dataset.date, turn = turnOf(from), oldMonth = $('.month', root);
      const between = !turn ? [] : from === from0 ? between0 : monthsBetween(from, shown()); // another load swapped meanwhile: the prefetched months no longer fit
      root.innerHTML = html;
      root.dataset.view = state.view; root.dataset.date = shown();
      const h1 = root.querySelector('#title'); if (h1) document.title = h1.textContent.trim() + ' · ' + document.title.split(' · ').slice(1).join(' · ');
      if (push) history.pushState(state, '', url(false).pathname + url(false).search);
      onScreen = { href: u.href, html };
      wire(); if (turn) monthFlip(oldMonth, turn, between, from === from0 ? mids : null);
    } catch (e) { console.error(e); } finally { done(); }
  }
  addEventListener('popstate', e => { if (e.state) { state = e.state; load(false); } });
  // the state shift() / go() lead to, without going there (also what a prefetch asks for)
  function shifted(dir) {
    const s = { ...state }, d = parse(s.date);
    if (s.view === 'day') d.setDate(d.getDate() + dir);
    else if (s.view === 'week') d.setDate(d.getDate() + 7 * dir);
    else if (s.view === 'month') { const m = parse(shown(s)); m.setDate(1); m.setMonth(m.getMonth() + dir); s.month = localIso(m).slice(0, 7); return s; }
    else d.setFullYear(d.getFullYear() + dir);
    s.date = ymd(d); return s;
  }
  function gone(view, date) { const s = { ...state }; if (date || (view && view !== s.view)) s.month = s.week = null; if (view) s.view = view; if (date) s.date = date; return s; } // a picked day or another view shows the picked day's month again
  function shift(dir) { const prev = state; state = shifted(dir); load(true, prev); }
  function go(view, date) { const prev = state; state = gone(view, date); load(true, prev); }
  // the fragments a navigation to `s` fetches: the target and the months a page turn glides past (load())
  function fetches(s) {
    const out = [url(true, s)].filter(u => !onScreen || u.href !== onScreen.href), from = root.dataset.date, to = shown(s); // not the view on screen
    if (root.dataset.view === 'month' && s.view === 'month' && from.slice(0, 7) !== to.slice(0, 7) && !reducedMotion()) { const b = monthsBetween(from, to); if (b.length <= FLIP_FETCH) out.push(...b.map(m => monthUrl(m, s))); }
    return out;
  }
  function pickDay(date) { // a click picks the day; the month or week on screen stays
    if (state.view === 'month') { state.month = shown().slice(0, 7); state.date = date; load(false); } // the agenda beside the grid shows it
    else { state.date = date; markPicked(); }
    history.replaceState(state, '', url(false).pathname + url(false).search); }
  function markPicked() { $$('.stream .dayblock, .planner .c[data-date]', root).forEach(b => b.classList.toggle('sel', b.dataset.date === state.date)); } // hoisted: wireItems calls it
  // the month grid owns the wheel: one gesture turns exactly one page, however far it goes. A new
  // gesture is told by speed, not by a pause: a trackpad's momentum tail only ever slows down, while
  // a fresh swipe arrives faster than the tail it interrupts — the speed must have fallen below half
  // the gesture's peak and then jump above twice the recent mean (a finger still accelerating after
  // the page turned is neither). Time gaps proved unusable: the lift-off before the tail and Chrome
  // coalescing events while the page swaps both open gaps that look like rests (two pages per swipe),
  // and the rest between two swipes can be shorter than either (no page at all). Speed is taken over
  // the event's own time, so a coalesced event (one big delta over a long gap) keeps its true speed.
  // State lives outside the fragment so the events that arrive after the swap still count as the
  // same gesture.
  const MONTH_FLIP = 60, MONTH_REST = 150, MONTH_JUMP = 2, MONTH_MINV = 0.25; // px that turn the page; ms of rest that end a gesture for sure; speed ratio and floor (px/ms) of a fresh swipe
  let mwLast = 0, mwAcc = 0, mwDone = false, mwVs = [], mwPeak = 0;
  function monthWheel(e) {
    e.preventDefault(); const now = e.timeStamp, dy = e.deltaY * (e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? e.currentTarget.clientHeight : 1);
    const gap = now - mwLast, v = Math.abs(dy) / Math.max(1, gap), ref = mwVs.reduce((a, b) => a + b, 0) / (mwVs.length || 1);
    const fresh = gap > MONTH_REST || (mwDone && v >= MONTH_MINV && ref < mwPeak / 2 && v > MONTH_JUMP * ref);
    if (fresh) { mwAcc = 0; mwDone = false; mwVs = []; mwPeak = 0; } mwLast = now; mwVs = mwVs.concat(v).slice(-3); mwPeak = Math.max(mwPeak, v);
    if (mwDone) return;
    mwAcc += dy;
    if (Math.abs(mwAcc) >= MONTH_FLIP) { mwDone = true; shift(mwAcc > 0 ? 1 : -1); }
  }
  // page turn: the leaving grid, every month in between and the arriving grid form one column that
  // glides past, eased in and out — the same glide whatever changed the month, and a far jump passes
  // every month on the way. The months in between are fetched with the target (up to FLIP_FETCH of
  // them); beyond that they are drawn as bare day grids, at that speed only the shape is visible.
  // A turn that starts mid-glide ends the running one first (load()).
  const FLIP_MS = 480, FLIP_STEP = 90, FLIP_MAX = 1500, FLIP_EASE = 'cubic-bezier(.65,0,.35,1)', FLIP_GAP = 24, FLIP_FETCH = 12; // ms for one page, ms per further page, cap; px between pages
  let flip = null;
  const reducedMotion = () => matchMedia('(prefers-reduced-motion: reduce)').matches;
  const localIso = ymd;
  function monthsBetween(a, b) { const out = [], d = parse(a.slice(0, 7) + '-01'), dir = b > a ? 1 : -1; for (;;) { d.setMonth(d.getMonth() + dir); const m = localIso(d); if (m.slice(0, 7) === b.slice(0, 7)) return out; out.push(m); } } // first days of the months strictly between, in travel order
  const monthUrl = (m, s) => { const u = url(true, s); u.searchParams.set('month', m.slice(0, 7)); return u; };
  const fetchMonths = list => Promise.all(list.map(async m => { try { const u = monthUrl(m); let html = await practical.prefetched(u); if (html === null) { const r = await fetch(u, { headers: { Accept: 'text/html' } }); html = r.ok ? await r.text() : null; } return html === null ? null : new DOMParser().parseFromString(html, 'text/html').querySelector('.month'); } catch { return null; } }));
  function skeleton(m, dows) { const d = parse(m), y = d.getFullYear(), mo = d.getMonth(), first = new Date(y, mo, 1), c = new Date(y, mo, 1 - ((first.getDay() + 6) % 7)); let h = dows.map(x => `<div class="dow">${esc(x)}</div>`).join('');
    for (let i = 0; ; i++, c.setDate(c.getDate() + 1)) { if (i && i % 7 === 0 && (c.getMonth() !== mo || c.getFullYear() !== y)) break; h += `<div class="day${c.getMonth() !== mo ? ' other' : ''}"><span class="n">${c.getDate()}</span></div>`; }
    const el = document.createElement('div'); el.className = 'month'; el.innerHTML = h; return el; } // same week rule as render.rs month_weeks
  function monthFlip(old, dir, between, mids) {
    const cur = $('.month', root); if (!old || !cur || reducedMotion()) return;
    const dows = $$('.dow', cur).map(x => x.textContent), pages = between.map((m, i) => (mids && mids[i]) || skeleton(m, dows));
    const H = cur.getBoundingClientRect().height, step = H + FLIP_GAP, n = pages.length + 1;
    const wrap = document.createElement('div'); wrap.className = 'mflip'; wrap.style.height = `${H}px`; cur.replaceWith(wrap);
    const col = document.createElement('div'); col.className = 'mcol'; col.style.gridAutoRows = `${H}px`; col.style.gap = `${FLIP_GAP}px`;
    col.append(...(dir > 0 ? [old, ...pages, cur] : [cur, ...pages.reverse(), old])); wrap.append(col); // top to bottom = earlier to later
    wrap.addEventListener('wheel', monthWheel, { passive: false }); // the wheel keeps working over the moving column
    const a = col.animate([{ transform: `translateY(${dir > 0 ? 0 : -n * step}px)` }, { transform: `translateY(${dir > 0 ? -n * step : 0}px)` }], { duration: Math.min(FLIP_MAX, FLIP_MS + FLIP_STEP * (n - 1)), easing: FLIP_EASE, fill: 'forwards' });
    const end = () => { if (!flip) return; flip = null; a.cancel(); if (wrap.parentNode) wrap.replaceWith(cur); };
    flip = { end }; a.onfinish = end;
  }

  // ---- timeline (right rail): client-rendered, endless in both directions; the SVG node and the
  // scroll position survive fragment swaps of the same kind, so selecting never re-centres ----
  const tip = document.createElement('div'); tip.className = 'tip'; tip.hidden = true; document.body.appendChild(tip);
  // `per` = items a year takes (weeks: 52-53 dots + 12 month lines; months: 12 dots + 1 year line).
  // The step sits halfway between the loose `step` and the one at which a whole year would fit the
  // rail — dense enough to show a good stretch, loose enough to read. A year marker is always on
  // screen regardless, because the top one sticks (`tlYearSync`). The chunk follows the density, or
  // a short rail would never fill its own height.
  const TLK = { weeks: { step: 30, y0: 16, r: 3.5, chunk: 26, per: 65 }, months: { step: 24, y0: 14, r: 3.5, chunk: 24, per: 13 } };
  const TL_LABEL = 16, TL_MID = 8; // height of the sticky year label and the height its line sits at
  let tlStep = 0, tlR = 0, tlRing = 0, tlChunk = 0;
  function tlMetrics(box, kind) { const k = TLK[kind], h = Math.max(200, box.clientHeight);
    const step = Math.max(6, Math.min(k.step, Math.round((k.step + h / k.per) / 2)));
    return { step, r: Math.min(k.r, step * 0.3), ring: Math.max(4.5, Math.min(7.5, step * 0.62)), chunk: Math.max(k.chunk, Math.ceil(h / step)) }; }
  const fmt = ymd;
  const dm = d => dfmt(d, { day: 'numeric', month: 'numeric' });
  const isoWeek = d => { const t = new Date(Date.UTC(d.getFullYear(), d.getMonth(), d.getDate())); const day = t.getUTCDay() || 7; t.setUTCDate(t.getUTCDate() + 4 - day); return Math.ceil(((t - Date.UTC(t.getUTCFullYear(), 0, 1)) / 864e5 + 1) / 7); };
  const weekStart = d => { const w = new Date(d); w.setDate(w.getDate() - ((w.getDay() + 6) % 7)); return w; };
  const stepDate = (kind, d, n) => { const x = new Date(d); if (kind === 'weeks') x.setDate(x.getDate() + 7 * n); else x.setMonth(x.getMonth() + n); return x; };
  const periodOf = (kind, d) => kind === 'weeks' ? weekStart(d) : new Date(d.getFullYear(), d.getMonth(), 1);
  let tlSvg = null, tlKind = null, tlFirst = null, tlLast = null, tlItems = [], tlScroll = null;
  // items: one per period, optional line before it (month start in weeks, January in months)
  function tlBuild(kind) {
    const k = { ...TLK[kind], step: tlStep, r: tlR }, todayD = parse(today), nowP = fmt(periodOf(kind, todayD));
    let y = k.y0, out = '', items = [];
    for (let d = new Date(tlFirst); d <= tlLast; d = stepDate(kind, d, 1)) {
      if (kind === 'weeks') {
        const first = [0, 1, 2, 3, 4, 5, 6].map(i => { const x = new Date(d); x.setDate(x.getDate() + i); return x; }).find(x => x.getDate() === 1);
        if (first) { const lbl = dfmt(first, { day: 'numeric', month: 'long', year: 'numeric' }), isY = first.getMonth() === 0; out += `<g class="mline${isY ? ' y' : ''}" data-y="${y}" data-date="${fmt(first)}" data-label="${lbl}"><line x1="18" x2="46" y1="${y}" y2="${y}"/><text x="50" y="${y + 4}">${isY ? first.getFullYear() : MON[first.getMonth()].slice(0, 3)}</text></g>`; items.push({ y, date: fmt(first), label: lbl, year: isY ? first.getFullYear() : 0 }); y += k.step; }
        const e = new Date(d); e.setDate(e.getDate() + 6);
        const lbl = `${tr('js-week-short', { w: isoWeek(d) })} · ${dm(d)} – ${dm(e)}`, ds = fmt(d);
        out += `<g class="wdot${ds === nowP ? ' now' : ''}" data-y="${y}" data-date="${ds}" data-label="${lbl}"><circle cx="32" cy="${y}" r="${k.r}"/></g>`;
        items.push({ y, date: ds, label: lbl, period: ds }); y += k.step;
      } else {
        if (d.getMonth() === 0) { out += `<g class="mline y" data-y="${y}" data-date="${fmt(d)}" data-label="${d.getFullYear()}"><line x1="18" x2="46" y1="${y}" y2="${y}"/><text x="50" y="${y + 4}">${d.getFullYear()}</text></g>`; items.push({ y, date: fmt(d), label: String(d.getFullYear()), year: d.getFullYear() }); y += k.step; }
        const lbl = `${MON[d.getMonth()]} ${d.getFullYear()}`, ds = fmt(d);
        out += `<g class="wdot${ds === nowP ? ' now' : ''}" data-y="${y}" data-date="${ds}" data-label="${lbl}"><circle cx="32" cy="${y}" r="${k.r}"/></g>`;
        items.push({ y, date: ds, label: lbl, period: ds }); y += k.step;
      }
    }
    tlSvg.setAttribute('height', y); tlSvg.setAttribute('viewBox', `0 0 88 ${y}`);
    tlSvg.innerHTML = out + `<circle class="sel" cx="32" r="${tlRing}" hidden/><circle class="pre" cx="32" r="${tlRing}" hidden/>`;
    tlItems = items; tlMark();
    return y;
  }
  function tlYearSync(box) { // the top year marker stays put until the next one pushes it out
    const el = $('.tly', box); if (!el || !tlItems.length) return;
    // The label draws its line at its own middle, so it stands for a marker at TL_MID. Handover
    // happens exactly when the arriving marker reaches that height: the old label is pushed out by
    // then and the new one takes its place with both lines at the same height. The year comes from
    // the last marker crossed, never from the item at the top — in the week rail the dot after the
    // January line carries its Monday, which usually still lies in December.
    const top = box.scrollTop + TL_MID;
    let cur = 0; for (let i = tlItems.length - 1; i >= 0; i--) if (tlItems[i].year && tlItems[i].y <= top) { cur = tlItems[i].year; break; }
    const next = tlItems.find(i => i.year && i.y > top);
    if (!cur) cur = next ? next.year - 1 : parse(tlItems[0].date).getFullYear();
    el.firstElementChild.textContent = cur;
    el.style.transform = next && next.y - top < TL_LABEL ? `translateY(${next.y - top - TL_LABEL}px)` : '';
  }
  const svgShow = (el, on) => on ? el.removeAttribute('hidden') : el.setAttribute('hidden', ''); // SVG elements have no .hidden property
  function tlMark() { // selection ring on the viewed period
    if (!tlSvg) return; const sel = tlSvg.querySelector('.sel'); const p = fmt(periodOf(tlKind, parse(shown())));
    const it = tlItems.find(i => i.period === p); if (it) sel.setAttribute('cy', it.y); svgShow(sel, !!it);
  }
  const TL_EDGE = 14; // px the ring keeps from the rail's edge — its own radius plus a little air
  function tlFollow() { // one rule for every rail (weeks in day/week, months elsewhere): it only moves when the
    const rail = $('.tl', root); if (!rail || !tlSvg) return; // ring would leave it, and then just far enough to let the ring hang at that edge
    const it = tlItems.find(i => i.period === fmt(periodOf(tlKind, parse(shown())))); if (!it) return;
    const y = it.y - rail.scrollTop, h = rail.clientHeight, edge = TL_EDGE + TL_LABEL; // the sticky year label sits on the top edge
    if (y < edge) rail.scrollTop = Math.max(0, it.y - edge);
    else if (y > h - TL_EDGE) rail.scrollTop = it.y - (h - TL_EDGE);
  }
  const tlSelect = () => { tlMark(); tlFollow(); }; // the viewed period changed: mark it and keep it in sight
  let tlHome = false; // the today button is the one jump that centres the rail instead of letting the ring hang at an edge
  const tlCentre = box => { tlMark(); const it = tlItems.find(i => i.period === fmt(periodOf(tlKind, parse(shown())))); box.scrollTop = it ? Math.max(0, it.y - box.clientHeight / 2) : 0; };
  function tlExtend(box) { // keep two chunks of slack above and below the viewport
    let changed = false;
    if (box.scrollTop < 300) { tlFirst = stepDate(tlKind, tlFirst, -tlChunk); const before = tlSvg.getBoundingClientRect().height; const h = tlBuild(tlKind); box.scrollTop += h - before; changed = true; }
    if (box.scrollTop + box.clientHeight > box.scrollHeight - 300) { tlLast = stepDate(tlKind, tlLast, tlChunk); tlBuild(tlKind); changed = true; }
    return changed;
  }
  function mountTimeline(box) {
    if (!box) return;
    const kind = box.dataset.kind, center = periodOf(kind, parse(box.dataset.center)), m = tlMetrics(box, kind);
    // a rail of the same kind is carried over with its scroll position — but only while it reaches
    // the period we are moving to; a jump past its ends (today from another year, the orbit picker)
    // gets a fresh rail around the target instead of one whose ring is nowhere
    if (tlSvg && tlKind === kind && tlStep === m.step && tlItems.some(i => i.period === fmt(center))) { box.appendChild(tlSvg); if (tlScroll != null) box.scrollTop = tlScroll; if (tlHome) tlCentre(box); else tlSelect(); }
    else { // a resized window changes the step, and a rail built at the old one has to go
      tlKind = kind; tlStep = m.step; tlR = m.r; tlRing = m.ring; tlChunk = m.chunk;
      tlFirst = stepDate(kind, center, -tlChunk); tlLast = stepDate(kind, center, tlChunk);
      tlSvg = document.createElementNS('http://www.w3.org/2000/svg', 'svg'); tlSvg.setAttribute('width', 88); box.appendChild(tlSvg); tlBuild(kind);
      const it = tlItems.find(i => i.period === fmt(center)); box.scrollTop = it ? Math.max(0, it.y - box.clientHeight / 2) : 0;
    }
    if (!$('.tly', box)) { const y = document.createElement('div'); y.className = 'tly'; y.innerHTML = '<b></b>'; box.insertBefore(y, box.firstChild); }
    tlYearSync(box);
    tlHome = false;
    const nearest = e => { const top = tlSvg.getBoundingClientRect().top; const y = e.clientY - top; let best = null; for (const i of tlItems) if (!best || Math.abs(i.y - y) < Math.abs(best.y - y)) best = i; return best; };
    box.onmousemove = e => { const it = nearest(e); if (!it) return; const pre = tlSvg.querySelector('.pre'); pre.setAttribute('cy', it.y); svgShow(pre, true); tip.textContent = it.label; tip.hidden = false; tip.style.left = e.clientX + 'px'; tip.style.top = e.clientY + 'px'; };
    box.onmouseleave = () => { tip.hidden = true; const pre = tlSvg.querySelector('.pre'); if (pre) svgShow(pre, false); };
    box.onclick = e => { const it = nearest(e); if (it) { tip.hidden = true; if (state.view === 'week') goWeek(it.date); else if (state.view === 'month') { state.month = it.date.slice(0, 7); load(); } else go(null, it.date); } };
    box.onscroll = () => { tlYearSync(box); if (!box._busy) { box._busy = true; requestAnimationFrame(() => { tlExtend(box); tlYearSync(box); box._busy = false; }); } };
    tlExtend(box);
  }
  // ---- week stream: endless in both directions, the week in focus drives outline, title and rail ----
  // No prev/next buttons: the week changes by scrolling alone. Focus = the week covering the middle
  // of the free viewport, which is also the one taking up most of it. It is tracked per frame, so
  // the title, the rail's ring, the heat panel and the URL follow the scroll without reloading.
  const WK_CHUNK = 8, WK_MAX = 80, WK_SLACK = 1200; // weeks per fetch, weeks kept in the DOM, px of slack before the next fetch
  let wkOff = null;
  const isoYear = d => { const t = new Date(d.getFullYear(), d.getMonth(), d.getDate()); t.setDate(t.getDate() + 3 - ((t.getDay() + 6) % 7)); return t.getFullYear(); }; // the Thursday's year
  const scrollerOf = el => { for (let p = el.parentElement; p; p = p.parentElement) { const o = getComputedStyle(p).overflowY; if ((o === 'auto' || o === 'scroll') && p.scrollHeight > p.clientHeight + 1) return p; } return document.scrollingElement; };
  const centreY = () => { const t = $('.toolbar', root); return ((t ? t.getBoundingClientRect().bottom : 0) + innerHeight) / 2; };
  function goWeek(date) { // a rail or heat click scrolls there when the week is loaded, and reloads around it when it is not
    const stream = $('.stream', root), mon = localIso(weekStart(parse(date))), b = stream && $(`.week[data-week="${mon}"]`, stream);
    if (!b) return go(null, date);
    const box = scrollerOf(stream), r = b.getBoundingClientRect();
    box.scrollTo({ top: box.scrollTop + r.top - (centreY() - r.height / 2), behavior: reducedMotion() ? 'auto' : 'smooth' });
  }
  function mountWeeks(stream) {
    if (wkOff) { wkOff(); wkOff = null; } // the listeners sit on .main, which survives the fragment swap
    if (!stream || state.view !== 'week') return;
    const box = scrollerOf(stream), target = box === document.scrollingElement ? window : box;
    let focus = null, tick = 0, busy = false;
    const blocks = () => $$('.week', stream);
    const pick = () => { const y = centreY(); let best = null, bestD = Infinity;
      for (const w of blocks()) { const r = w.getBoundingClientRect(); if (r.top <= y && r.bottom >= y) return w; const d = Math.min(Math.abs(r.top - y), Math.abs(r.bottom - y)); if (d < bestD) { bestD = d; best = w; } }
      return best; };
    function setFocus(w) {
      if (!w || w === focus) return;
      if (focus) focus.classList.remove('focus');
      focus = w; w.classList.add('focus');
      const d = parse(w.dataset.week);
      state.week = w.dataset.week; // the week on screen; the picked day stays
      const h1 = $('#title', root); if (h1) { h1.textContent = tr('js-week', { w: isoWeek(d), y: isoYear(d) }); document.title = h1.textContent + ' · ' + document.title.split(' · ').slice(1).join(' · '); }
      history.replaceState(state, '', url(false).pathname + url(false).search);
      tlSelect(); heatSync(d);
    }
    function heatSync(d) { // the panel names its month, so it must not keep showing the one we scrolled away from
      const heat = $('.heat', root); if (!heat) return;
      const y = d.getFullYear(), m = d.getMonth(), key = `${y}-${pad(m + 1)}`;
      if (heat.dataset.month !== key) {
        const cells = []; const c = weekStart(new Date(y, m, 1));
        for (let i = 0; ; i++, c.setDate(c.getDate() + 1)) { if (i && i % 7 === 0 && (c.getMonth() !== m || c.getFullYear() !== y)) break; cells.push(new Date(c)); } // same week rule as render.rs month_weeks
        const dayOf = x => $(`.dayblock[data-date="${x}"]`, stream);
        if (cells.some(x => !dayOf(fmt(x)))) return; // a day of that month is not loaded: leave the panel alone rather than show a wrong count
        heat.dataset.month = key; const hm = $('.hm', heat); if (hm) hm.textContent = `${MON[m]} ${y}`;
        $('.grid', heat).innerHTML = Array.from({ length: cells.length / 7 }, (_, r) => { const w = cells.slice(r * 7, r * 7 + 7);
          return `<div class="hw" data-week="${fmt(w[0])}"><b>${isoWeek(w[0])}</b>` + w.map(x => { const n = Math.min(4, $$('li', dayOf(fmt(x))).filter(li => !li.classList.contains('none')).length);
            return `<i class="${x.getMonth() !== m ? 'o' : ''}${fmt(x) === today ? ' t' : ''}" style="--n:${n}" data-date="${fmt(x)}" title="${esc(dm(x))}"></i>`; }).join('') + '</div>'; }).join('');
        wireItems(heat);
      }
      $$('.hw', heat).forEach(r => r.classList.toggle('focus', r.dataset.week === state.week)); }
    async function extend(dir) {
      if (busy) return; const bs = blocks(); if (!bs.length) return;
      const edge = dir > 0 ? bs[bs.length - 1] : bs[0];
      const from = localIso(stepDate('weeks', parse(edge.dataset.week), dir > 0 ? 1 : -WK_CHUNK));
      busy = true;
      try {
        const u = new URL('/api/weeks', location.origin); u.searchParams.set('start', from); u.searchParams.set('count', WK_CHUNK); if (state.cals) u.searchParams.set('cals', state.cals);
        const r = await practical.req(u, { headers: { Accept: 'text/html' }, as: 'text' }); if (!r.ok) return; // offline: the notice says so; the next scroll asks again
        const tmp = document.createElement('div'); tmp.innerHTML = r.data;
        const fresh = [...tmp.children].filter(w => !$(`.week[data-week="${w.dataset.week}"]`, stream));
        if (!fresh.length) return;
        // put a block that stays back where it was: measuring the displacement afterwards also absorbs
        // the browser's own scroll anchoring, which a scrollHeight delta would have added a second time
        const anchor = focus && focus.isConnected ? focus : bs[0], y0 = anchor.getBoundingClientRect().top;
        if (dir > 0) stream.append(...fresh); else stream.prepend(...fresh);
        onScreen = null;
        fresh.forEach(wireItems); markPicked();
        const over = blocks().length - WK_MAX;
        if (over > 0) (dir > 0 ? blocks().slice(0, over) : blocks().slice(-over)).forEach(b => b.remove());
        box.scrollTop += anchor.getBoundingClientRect().top - y0;
      } catch (e) { console.error(e); } finally { busy = false; }
    }
    const onScroll = () => { if (tick) return; tick = requestAnimationFrame(() => { tick = 0;
      setFocus(pick()); const bs = blocks(); if (!bs.length) return;
      if (bs[0].getBoundingClientRect().top > -WK_SLACK) extend(-1);
      if (bs[bs.length - 1].getBoundingClientRect().bottom < innerHeight + WK_SLACK) extend(1); }); };
    target.addEventListener('scroll', onScroll, { passive: true }); addEventListener('resize', onScroll);
    wkOff = () => { target.removeEventListener('scroll', onScroll); removeEventListener('resize', onScroll); };
    const cal = $('.cal', root), first = (cal && $(`.week[data-week="${cal.dataset.focus}"]`, stream)) || blocks()[0];
    if (first) { const r = first.getBoundingClientRect(); box.scrollTop += r.top - (centreY() - r.height / 2); } // land with the week in focus centred
    markPicked(); setFocus(pick());
  }
  // ---- day ledger: endless in both directions. Picking a day in it must not move it — only a day
  // that arrives some other way (the today button, the orbit picker, the rail) is scrolled to the
  // middle. The node itself is carried across fragment swaps, so the loaded days survive a pick. ----
  const LED_CHUNK = 30, LED_MAX = 400, LED_SLACK = 600; // days per fetch, days kept, px of slack before the next fetch
  let ledNode = null, ledScroll = null, ledPin = false, ledOff = null;
  // a filter change (calendars, holidays) makes the carried ledger stale: the next swap takes the server's
  function dropCarried() { ledNode = null; ledScroll = null; onScreen = null; }
  function mountLedger(box) {
    if (ledOff) { ledOff(); ledOff = null; }
    if (!box || state.view !== 'day') { ledNode = null; ledPin = false; return; }
    const keep = ledNode && ledNode !== box && $(`.lrow[data-date="${state.date}"]`, ledNode); // a day outside the loaded days starts from the page's own ledger
    if (keep) { box.replaceWith(ledNode); if (ledScroll != null) ledNode.scrollTop = ledScroll; onScreen = null; } else ledNode = box;
    const led = ledNode, list = $('.lrows', led), head = $('.lhd', led), rows = () => $$('.lrow', led);
    $$('.lrow.sel', led).forEach(r => r.classList.remove('sel'));
    const sel = $(`.lrow[data-date="${state.date}"]`, led); if (sel) sel.classList.add('sel');
    if (sel && !(keep && ledPin)) led.scrollTop += sel.getBoundingClientRect().top - led.getBoundingClientRect().top - (led.clientHeight - sel.offsetHeight) / 2;
    ledPin = false;
    wireItems(led);
    const headSync = () => { if (!head) return; // the heading names the month with the most days in sight (row middle visible); a tie goes to the one covering more pixels
      const top = led.getBoundingClientRect().top + head.offsetHeight, bottom = Math.min(led.getBoundingClientRect().bottom, innerHeight), seen = new Map();
      for (const x of rows()) { const r = x.getBoundingClientRect(); if (r.bottom <= top) continue; if (r.top >= bottom) break;
        const m = x.dataset.date.slice(0, 7), s = seen.get(m) || { n: 0, px: 0 }, mid = (r.top + r.bottom) / 2; if (mid >= top && mid <= bottom) s.n++; s.px += Math.min(r.bottom, bottom) - Math.max(r.top, top); seen.set(m, s); }
      let best = null; for (const [m, s] of seen) if (!best || s.n > best[1].n || (s.n === best[1].n && s.px > best[1].px)) best = [m, s]; if (!best) return;
      const d = parse(best[0] + '-01'); head.textContent = `${MON[d.getMonth()]} ${d.getFullYear()}`; };
    headSync();
    if (led.scrollHeight <= led.clientHeight + 1) return; // narrow layout: the page scrolls, not the ledger, and an endless list would never stop growing
    let busy = false, tick = 0;
    async function extend(dir) {
      if (busy) return; const rs = rows(); if (!rs.length) return;
      const d0 = parse((dir > 0 ? rs[rs.length - 1] : rs[0]).dataset.date); d0.setDate(d0.getDate() + (dir > 0 ? 1 : -LED_CHUNK));
      busy = true;
      try {
        const u = new URL('/api/days', location.origin); u.searchParams.set('start', localIso(d0)); u.searchParams.set('count', LED_CHUNK); if (state.cals) u.searchParams.set('cals', state.cals);
        const r = await practical.req(u, { headers: { Accept: 'text/html' }, as: 'text' }); if (!r.ok) return;
        const tmp = document.createElement('div'); tmp.innerHTML = r.data;
        const fresh = [...tmp.children].filter(x => !$(`.lrow[data-date="${x.dataset.date}"]`, led));
        if (!fresh.length) return;
        const top = led.getBoundingClientRect().top, anchor = rows().find(x => x.getBoundingClientRect().bottom > top) || rs[0], y0 = anchor.getBoundingClientRect().top; // a visible row is never the one that gets trimmed
        if (dir > 0) list.append(...fresh); else list.prepend(...fresh);
        onScreen = null;
        wireItems(list); // the fetched nodes ARE the rows: wiring each one would search inside it and find nothing, and the untouched <a href> would reload the page
        const over = rows().length - LED_MAX;
        if (over > 0) (dir > 0 ? rows().slice(0, over) : rows().slice(-over)).forEach(x => x.remove());
        led.scrollTop += anchor.getBoundingClientRect().top - y0;
      } catch (e) { console.error(e); } finally { busy = false; }
    }
    const onScroll = () => { if (tick) return; tick = requestAnimationFrame(() => { tick = 0; headSync();
      const rs = rows(); if (!rs.length) return; const r = led.getBoundingClientRect();
      if (rs[0].getBoundingClientRect().bottom > r.top - LED_SLACK) extend(-1);
      if (rs[rs.length - 1].getBoundingClientRect().top < r.bottom + LED_SLACK) extend(1); }); };
    led.addEventListener('scroll', onScroll, { passive: true });
    ledOff = () => led.removeEventListener('scroll', onScroll);
    onScroll();
  }
  function wireItems(scope) { // everything a fetched week block or a rebuilt heat grid brings along; assignments only, so a second call is harmless
    $$('.heat i[data-date], .we[data-date]', scope).forEach(el => el.onclick = () => state.view === 'week' ? goWeek(el.dataset.date) : go('day', el.dataset.date));
    $$('.planner .c[data-date]', scope).forEach(el => el.onclick = () => { if (el.dataset.date !== state.date) pickDay(el.dataset.date); }); // the year planner picks the day; the view stays
    $$('.lrow[data-date]', scope).forEach(el => el.onclick = e => { if (e.target.classList.contains('add')) return; e.preventDefault(); ledPin = true; go('day', el.dataset.date); }); // picking in the ledger leaves it exactly where it is
    $$('.more[data-date]', scope).forEach(el => el.onclick = e => { e.stopPropagation(); go('day', el.closest('[data-date]').dataset.date); });
    $$('.v-month > .month > .day[data-date]', scope).forEach(el => el.onclick = e => { if (!e.defaultPrevented && !e.target.closest('a, button, .ev, .more') && el.dataset.date !== state.date) pickDay(el.dataset.date); });
    $$('.dayblock[data-date]', scope).forEach(el => el.onclick = e => { if (!e.defaultPrevented && !e.target.closest('a, button, .ev') && el.dataset.date !== state.date) pickDay(el.dataset.date); }); // the date label picks too, it no longer opens the day view
    markPicked();
    $$('[data-key]', scope).forEach(el => el.onclick = e => { e.stopPropagation(); edOpen(el.dataset.key, (el.closest('[data-date]') || el).dataset.date); }); // a week entry on its start day: the day block's date
    // the day document's details sit inside the clickable entry, so each of them stops the click
    $$('.doc [data-copytext]', scope).forEach(b => b.onclick = e => { e.stopPropagation(); e.preventDefault(); // not data-copy: that one takes a selector (top of this file)
      practical.copy(b.dataset.copytext).then(ok => { if (!ok) return; b.classList.add('ok'); b.title = T.copied; setTimeout(() => b.classList.remove('ok'), 1200); }); });
    $$('.doc [data-map]', scope).forEach(b => b.onclick = e => { e.stopPropagation(); e.preventDefault(); const q = encodeURIComponent(b.dataset.q);
      window.open(b.dataset.map === 'osm' ? 'https://www.openstreetmap.org/search?query=' + q : 'https://www.google.com/maps/search/?api=1&query=' + q, '_blank', 'noopener'); });
    $$('.doc a.lk', scope).forEach(a => a.onclick = e => e.stopPropagation());
    $$('.add', scope).forEach(b => b.onclick = e => { e.stopPropagation(); e.preventDefault(); const d = b.closest('[data-date]'); edOpen(null, d ? d.dataset.date : state.date); });
  }
  function wire() {
    $$('[data-nav]', root).forEach(b => { b.onclick = () => { if (b.dataset.nav === 'today') { tlHome = true; go(null, today); } else shift(b.dataset.nav === 'next' ? 1 : -1); };
      practical.intent(b, () => fetches(b.dataset.nav === 'today' ? gone(null, today) : shifted(b.dataset.nav === 'next' ? 1 : -1))); });
    $$('.toolbar [data-view]', root).forEach(b => { b.onclick = e => { e.preventDefault(); go(b.dataset.view, null); }; practical.intent(b, () => fetches(gone(b.dataset.view, null))); });
    const more = $('.more', root); if (more) { $('[data-more]', more).onclick = e => { e.stopPropagation(); more.classList.toggle('open'); }; }
    mountTimeline($('.tl', root));
    if (state.view === 'month') { const m = $('.month', root); if (m) { m.addEventListener('wheel', monthWheel, { passive: false });
      // the wheel turns to a neighbour month: the pointer over the grid is the intent (also right after a turn, unmoved)
      practical.intent(m, () => [...fetches(shifted(1)), ...fetches(shifted(-1))], true); } }
    wireItems(root); mountWeeks($('.stream', root)); mountLedger($('.ledger', root));
    const title = $('#title', root); if (title) title.onclick = openPicker;
  }
  document.addEventListener('click', e => { if (!e.target.closest('.more')) $$('.more.open').forEach(m => m.classList.remove('open')); });
  document.addEventListener('rustical:change', () => { clearTimeout(load._t); load._t = setTimeout(() => load(false), 800); });
  $('#fab').onclick = () => edOpen(null, state.date);

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
  async function density() { try { const r = await practical.req(`/api/density?year=${pkYear}${state.cals ? '&cals=' + encodeURIComponent(state.cals) : ''}`); if (!r.ok) return; const d = r.data; const mx = Math.max(1, ...d); $$('.mnode', pk).forEach(g => { const m = +g.dataset.month; if (!g.classList.contains('cur')) g.querySelector('circle').setAttribute('r', 14 + 10 * d[m - 1] / mx); }); } catch (_) {} }
  function openPicker() { const d = parse(state.date); pkYear = d.getFullYear(); pkMonth = d.getMonth() + 1; pkBase = pkYear; py(); pm(); density(); pk.hidden = false; }
  yn.forEach(g => g.onclick = () => { pkYear = +g.dataset.year; py(); pm(); density(); });
  $$('.mnode', pk).forEach(g => g.onclick = () => { pkMonth = +g.dataset.month; pm(); });
  $('.ybtn.prev', pk).onclick = () => { pkBase -= 7; py(); }; $('.ybtn.next', pk).onclick = () => { pkBase += 7; py(); };
  $('.ybtn.reset', pk).onclick = () => { const d = parse(today); pkYear = d.getFullYear(); pkMonth = d.getMonth() + 1; pkBase = pkYear; py(); pm(); density(); };
  $('#pk-close').onclick = () => pk.hidden = true; pk.addEventListener('click', e => { if (e.target === pk) pk.hidden = true; });


  wire();
  window.rusticalCal = { go, view: () => state.view };
  // the editor module asks the page where "now" is and how to redraw after a save
  window.rusticalHost = { date: () => state.date, cals: () => state.cals, reload: (push) => load(push) };
})();
