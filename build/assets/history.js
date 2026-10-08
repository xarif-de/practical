// /history — the change history. One document (#history-data, again from /api/history after an undo
// or a live change) drawn two ways: the line map (one line per element over time, one stop per change,
// undo as an arc back to what it undid) and the list with details (before / after / today, what undo
// would do, the element's own history, a day ruler on the right). Undo asks the server for its plan
// first; warnings (a value changed later gets overwritten, later changes go inactive) need a yes.
(() => {
  const root = document.getElementById('hx');
  if (!root) return;
  const { $, $$, escN: esc } = practical; // app.js runs first (both deferred, in document order)
  const LOC = window.LOC;
  const fmtDay = new Intl.DateTimeFormat(LOC, { weekday: 'long', day: 'numeric', month: 'long' });
  const fmtShort = new Intl.DateTimeFormat(LOC, { weekday: 'short', day: 'numeric', month: 'numeric' });
  const fmtMonth = new Intl.DateTimeFormat(LOC, { month: 'short' });
  const fmtMonthL = new Intl.DateTimeFormat(LOC, { month: 'long' });
  const fmtWd = new Intl.DateTimeFormat(LOC, { weekday: 'short' });
  const ico = (id, cls = '') => `<svg class="ico ${cls}" aria-hidden="true"><use href="${practical.sprite}#${id}"/></svg>`;

  let D, log, byId, els;
  const S = k => (D.t && D.t[k]) || k;
  const tr = (k, vars = {}) => { const s = S(k); return (typeof s === 'object' ? plural(s, +vars[s.$]) : s).replace(/%(\w+)/g, (m, n) => n in vars ? vars[n] : m); }; // plural: app.js
  function load(doc) {
    D = doc;
    // fields at their default come without them (routes/history.rs `data`)
    log = D.changes.map(c => ({ dev: null, prodid: null, label: null, reverts: null, undoneBy: null, inactiveBy: null, tech: false, ...c, undo: { why: null, by: null, warn: 0, ...c.undo }, t: new Date(c.at) }));
    byId = Object.fromEntries(log.map(c => [c.id, c]));
    els = D.els;
    plans.clear();
  }
  const plans = new Map();
  load(JSON.parse($('#history-data').textContent));

  // ---- reading one change ---------------------------------------------------------------------
  const ACTS = ['create', 'update', 'move', 'delete', 'restore'];
  const kindOf = c => c.items[0].kind;
  const collOf = c => c.items[0].coll;
  const actOf = c => c.items.every(i => i.act === c.items[0].act) ? c.items[0].act : 'update';
  const isBulk = c => c.items.length > 1;
  const status = c => c.undoneBy ? 'undone' : c.inactiveBy ? 'inactive' : 'active';
  const coll = k => D.colls[k] || { name: k.split('/').pop(), color: 'var(--fg-3)' };
  const elTitle = el => (els[el] && els[el].title) || '';
  const ref = id => `<a href="#" class="hx-go" data-go="${id}">#${id}</a>`;
  const kindName = (k, n = 1) => S((n === 1 ? 'k-' : 'kp-') + k);
  const bulkTitle = c => {
    const l = c.label || '';
    if (l.startsWith('colors:')) return tr('bulk-colors', { name: l.slice(7) || '…' }).replace(' („…“)', '').replace(' ("…")', '');
    if (c.src === 'import') return l ? tr('bulk-import-name', { name: l }) : S('bulk-import');
    return `${c.items.length} ${kindName(kindOf(c), 2)}`;
  };
  const title = c => isBulk(c) ? bulkTitle(c) : (elTitle(c.items[0].el) || c.items[0].title || '—');
  const srcKey = c => c.src === 'dav' ? 'dav:' + (c.dev || '?') : c.src;
  const srcName = k => k.startsWith('dav:') ? (k.slice(4) === '?' ? S('src-dav') : k.slice(4)) : S('src-' + k);
  const srcLabel = c => srcName(srcKey(c));
  const devIcon = d => /ios|android|davx|jtx|tasks|fossify|etar/i.test(d || '') ? 'i-smartphone' : 'i-laptop';
  const srcIcon = c => c.src === 'dav' ? devIcon(c.dev) : c.src === 'import' ? 'i-cal-up' : c.src === 'undo' ? 'i-restore' : 'i-pencil';
  const person = w => (D.people[w] || { name: w, ini: (w || '?').slice(0, 2).toUpperCase() });
  const actor = c => c.who ? person(c.who).name : srcLabel(c);
  const avatar = c => c.who ? `<span class="hx-av" title="${esc(person(c.who).name)}">${esc(person(c.who).ini)}</span>` : `<span class="hx-av dev" title="${esc(srcLabel(c))}">${ico(srcIcon(c))}</span>`;
  const attrKeys = c => [...new Set(c.items.flatMap(it => it.attrs.map(a => a[0])))];
  const attrLabel = k => { for (const c of log) for (const it of c.items) for (const a of it.attrs) if (a[0] === k) return a[1]; return k; };
  const dayKey = practical.ymd;
  const now = () => new Date(D.now);
  const fmtTime = new Intl.DateTimeFormat(LOC, { hour: '2-digit', minute: '2-digit' }), time = t => fmtTime.format(t); // one formatter, not one per row (one per row cost 57 ms self time per load)
  const dayDiff = t => Math.round((new Date(dayKey(now())) - new Date(dayKey(t))) / 864e5);
  const dayLabel = t => { const d = dayDiff(t); return d === 0 ? S('today') : d === 1 ? S('yesterday') : fmtDay.format(t); };
  const when = t => { const d = dayDiff(t); return (d === 0 ? S('today') : d === 1 ? S('yesterday') : fmtShort.format(t)) + ' ' + time(t); };
  const oldest = () => { const t = now(); t.setDate(t.getDate() - D.retention); return t; };
  const actWord = a => S('act-' + a);
  const actTag = a => `<span class="hx-act ${a}">${esc(actWord(a))}</span>`;

  const sentence = (c, link = true) => {
    const who = `<b>${esc(actor(c))}</b>`, a = actOf(c), k = kindOf(c);
    if (c.src === 'undo') return tr('s-undo', { who, ref: ref(c.reverts) }) + ` <span class="muted">(${esc(isBulk(c) ? bulkTitle(c) : title(c))} ${esc(actWord(a))})</span>`;
    if (isBulk(c)) return tr('s-bulk', { who, n: c.items.length, kinds: esc(kindName(k, 2)), act: esc(actWord(a)) }) + (c.label !== null ? ` <span class="muted">· ${esc(bulkTitle(c))}</span>` : '');
    const tt = link ? `<a href="#" class="hx-el" data-el="${esc(c.items[0].el)}">„${esc(title(c))}“</a>` : `„${esc(title(c))}“`;
    return tr('s-' + k, { who, title: tt, act: esc(actWord(a)) });
  };
  const what = c => (c.src === 'undo' ? `#${c.reverts} ${S('undone-short')}: ` : '') + (isBulk(c) ? `${c.items.length} ${kindName(kindOf(c), 2)} ${actWord(actOf(c))}` : `${kindName(kindOf(c))} ${actWord(actOf(c))}`);
  // attribute rows; a multi-item change shows one row per element (up to `max`)
  const diffRows = (c, max = 4) => {
    if (isBulk(c)) {
      const rows = c.items.slice(0, max).map(it => {
        const a = it.attrs.find(x => x[0] !== 'SUMMARY' && x[0] !== 'FN') || it.attrs[0] || [];
        return { k: elTitle(it.el) || it.title, from: it.act === 'update' ? a[2] : '', to: it.act === 'update' ? a[3] : '', act: it.act };
      });
      if (c.items.length > max) rows.push({ more: c.items.length - max });
      return rows;
    }
    const it = c.items[0];
    return it.attrs.map(([k, label, from, to]) => ({ k: label, key: k, from, to, act: it.act }));
  };
  // several values (one per line: alarms, attendees, numbers …): which went, which came, which stayed
  const lines = v => v ? v.split('\n') : [];
  const lineDiff = (from, to) => {
    const a = lines(from), b = lines(to), gone = a.filter(x => !b.includes(x)), came = b.filter(x => !a.includes(x)), kept = a.filter(x => b.includes(x));
    return {
      inline: [...gone.map(x => `<del>${esc(x)}</del>`), ...came.map(x => `<ins>${esc(x)}</ins>`), ...kept.map(x => `<span class="hx-kept">${esc(x)}</span>`)].join(''),
      before: a.map(x => b.includes(x) ? `<span class="hx-kept">${esc(x)}</span>` : `<del>${esc(x)}</del>`).join('') || `<span class="muted">${esc(S('empty'))}</span>`,
      after: b.map(x => a.includes(x) ? `<span class="hx-kept">${esc(x)}</span>` : `<ins>${esc(x)}</ins>`).join('') || `<span class="muted">${esc(S('empty'))}</span>`,
    };
  };
  const multi = r => (r.from || '').includes('\n') || (r.to || '').includes('\n');
  const diff = (c, max) => `<div class="hx-diff">` + diffRows(c, max).map(r => r.more ? `<div class="hx-dr"><span class="k"></span><span class="hx-dv muted">${esc(tr('more', { n: r.more }))}</span></div>` :
    `<div class="hx-dr"><span class="k">${esc(r.k)}</span><span class="hx-dv${multi(r) ? ' hx-lines' : ''}">${(r.act === 'update' || r.act === 'move') && multi(r) ? lineDiff(r.from, r.to).inline : r.act === 'update' || r.act === 'move' ? `${r.from ? `<del>${esc(r.from)}</del>` : `<span class="muted">${esc(S('empty'))}</span>`}<span class="ar">→</span>${r.to ? `<ins>${esc(r.to)}</ins>` : `<span class="muted">${esc(S('empty'))}</span>`}`
      : r.act === 'delete' ? (r.from ? `<del>${esc(r.from)}</del>` : `<span class="muted">${esc(actWord('delete'))}</span>`) : (r.to ? `<ins>${esc(r.to)}</ins>` : `<span class="muted">${esc(actWord(r.act))}</span>`)}</span></div>`).join('') + '</div>';
  const statusTag = c => {
    const own = c.reverts ? `<span class="hx-st rev">${ico('i-restore')}${tr('st-reverts', { ref: ref(c.reverts) })}</span>` : '';
    if (c.undoneBy) return own + `<span class="hx-st undone">${tr('st-undone', { ref: ref(c.undoneBy) })}</span>`;
    if (c.inactiveBy) return own + `<span class="hx-st inactive">${tr('st-inactive', { ref: ref(c.inactiveBy) })}</span>`;
    return own;
  };
  const whyText = (why, by, c) => {
    const id = why === 'undone' ? c && c.undoneBy : why === 'inactive' ? c && c.inactiveBy : by;
    return tr('why-' + (why || 'same'), { ref: id ? '#' + id : '' }).replace(' ()', '').replace(' ( )', '');
  };
  const undoBtn = (c, cls = 'btn icon-btn', label = false) => {
    const u = c.undo || {};
    const tip = u.ok ? (u.warn ? S('undo-warn') : S('undo')) : whyText(u.why, u.by, c);
    return `<button class="${cls} hx-undo" type="button" data-undo="${c.id}"${u.ok ? '' : ' disabled'} title="${esc(tip)}" aria-label="${esc(tip)}">${ico('i-restore')}${label ? `<span>${esc(S('undo'))}</span>` : ''}${u.ok && u.warn ? '<span class="hx-warn" aria-hidden="true">!</span>' : ''}</button>`;
  };

  // ---- undo -----------------------------------------------------------------------------------
  const warnText = w => w.kind === 'later'
    ? tr('w-later', { title: w.title, n: w.ids.length, refs: w.ids.map(i => '#' + i).join(', ') })
    : tr('w-conflict', { label: w.label, by: w.by ? ` (#${w.by})` : '', cur: w.cur || S('empty'), old: w.old || S('empty') });
  const planText = p => [...p.warns.map(warnText), ...p.skips.map(s => tr('w-skip', { title: s.title, why: whyText(s.why, s.by) }))];
  // a 409 carries the undo's plan (JSON); any other failure its message (practical.req); a session that ended: nothing (the page leaves)
  async function post(id, confirm) {
    const r = await practical.send(`/api/history/${id}/undo`, 'POST', { confirm });
    if (r.ok) return { id: r.data.id };
    if (r.kind === 'conflict') try { return { plan: JSON.parse(r.body) }; } catch (_) {}
    return { err: r.kind === 'auth' ? null : r.msg, gone: r.kind === 'auth' };
  }
  // one undo at a time: a second click while one runs (its question included) is the same undo
  let undoing = null;
  const undo = (id, confirmed) => undoing || (undoing = undo1(id, confirmed).finally(() => { undoing = null; }));
  async function undo1(id, confirmed = false) {
    let res = await post(id, confirmed);
    if (res.plan) {
      if (!res.plan.ok) { toast(whyText(res.plan.why, (res.plan.skips[0] || {}).by, byId[id]), true); return; }
      const lines = planText(res.plan);
      if (!await askYesNo(tr('ask', { ref: '#' + id }) + '\n\n• ' + lines.join('\n• '), S('undo'), S('cancel'), true)) return;
      res = await post(id, true);
    }
    if (res.gone) return;
    if (res.err || res.plan) { toast(res.err || S('err'), true); return; }
    await reload();
    toast(tr('done', { ref: '#' + id, id: '#' + res.id }));
    emit({ added: res.id });
  }
  // false: the document came back unchanged (the live echo of an undo made here) — nothing to redraw
  let lastDoc = '';
  async function reload() {
    const r = practical.tell(await practical.req('/api/history', { headers: { Accept: 'application/json' }, as: 'text' }));
    if (!r.ok) return false; // the page stays as it is, the failure is told
    const t = r.data; if (t === lastDoc) return false;
    lastDoc = t; load(JSON.parse(t)); return true;
  }
  async function planOf(id) {
    if (!plans.has(id)) plans.set(id, fetch(`/api/history/${id}/plan`).then(r => r.ok ? r.json() : null).catch(() => null));
    return plans.get(id);
  }

  // ---- filters ----------------------------------------------------------------------------------
  const F = { kinds: new Set(), srcs: new Set(), who: new Set(), acts: new Set(), colls: new Set(), attrs: new Set(), status: 'all', period: 'all', from: '', to: '', q: '', tech: false, el: new URLSearchParams(location.search).get('el') };
  const SETS = ['kinds', 'srcs', 'who', 'acts', 'colls', 'attrs'];
  const resetF = () => { for (const k of SETS) F[k].clear(); Object.assign(F, { status: 'all', period: 'all', from: '', to: '', q: '', el: null }); };
  const fold = s => String(s).toLowerCase().normalize('NFD').replace(/[̀-ͯ]/g, '');
  const hay = c => c._hay || (c._hay = fold([title(c), actor(c), srcLabel(c), coll(collOf(c)).name, '#' + c.id, ...c.items.flatMap(it => [elTitle(it.el), it.title, ...it.attrs.flatMap(a => [a[1], a[2], a[3]])])].join(' ')));
  const facet = { kinds: c => [kindOf(c)], srcs: c => [srcKey(c)], who: c => c.who ? [c.who] : [], acts: c => [actOf(c)], colls: c => [...new Set(c.items.map(i => i.coll))], attrs: attrKeys };
  const inPeriod = c => {
    const d = dayDiff(c.t);
    switch (F.period) {
      case 'today': return d === 0; case 'yesterday': return d === 1; case '7': return d < 7; case '30': return d < 30;
      case 'custom': return (!F.from || dayKey(c.t) >= F.from) && (!F.to || dayKey(c.t) <= F.to);
      default: return true;
    }
  };
  const match = (c, except) => {
    if (!F.tech && c.tech) return false;
    if (F.el && !c.items.some(it => it.el === F.el)) return false;
    for (const k of SETS) if (k !== except && F[k].size && !facet[k](c).some(v => F[k].has(v))) return false;
    if (except !== 'status' && F.status !== 'all' && (F.status === 'active' ? status(c) !== 'active' : status(c) === 'active')) return false;
    if (except !== 'period' && !inPeriod(c)) return false;
    if (F.q) { const h = hay(c); if (!fold(F.q).split(/\s+/).filter(Boolean).every(w => h.includes(w))) return false; }
    return true;
  };
  const list = () => log.filter(c => match(c)).sort((a, b) => b.id - a.id);
  const facetCounts = key => { const m = new Map(); for (const c of log) if (match(c, key)) for (const v of facet[key](c)) m.set(v, (m.get(v) || 0) + 1); return m; };
  const facetValues = key => { const all = new Set(); for (const c of log) if (F.tech || !c.tech) for (const v of facet[key](c)) all.add(v); return [...all]; };
  const facetLabel = (key, v) => key === 'kinds' ? kindName(v, 2) : key === 'srcs' ? srcName(v) : key === 'acts' ? actWord(v) : key === 'colls' ? coll(v).name : key === 'attrs' ? attrLabel(v) : key === 'who' ? person(v).name : v;
  const FACETS = SETS.map(k => [k, S('f-' + k)]);
  const PERIODS = ['all', 'today', 'yesterday', '7', '30', 'custom'];
  const periodName = p => p === 'all' ? tr('p-all', { n: D.retention }) : S('p-' + p);
  const activeCount = () => SETS.reduce((n, k) => n + F[k].size, 0) + (F.status !== 'all') + (F.period !== 'all') + !!F.q + !!F.el + F.tech;
  const activeChips = () => {
    const out = [];
    for (const [k, name] of FACETS) for (const v of F[k]) out.push(`<button class="hx-chip" type="button" data-unf="${k}" data-v="${esc(v)}">${esc(name)}: ${esc(facetLabel(k, v))} ×</button>`);
    if (F.status !== 'all') out.push(`<button class="hx-chip" type="button" data-unf="status">${esc(S('st-only-' + F.status))} ×</button>`);
    if (F.period !== 'all') out.push(`<button class="hx-chip" type="button" data-unf="period">${esc(F.period === 'custom' ? `${F.from || '…'} – ${F.to || '…'}` : periodName(F.period))} ×</button>`);
    if (F.q) out.push(`<button class="hx-chip" type="button" data-unf="q">„${esc(F.q)}“ ×</button>`);
    if (F.el) out.push(`<button class="hx-chip el" type="button" data-unf="el">${esc(tr('only-el', { title: elTitle(F.el) || '…' }))} ×</button>`);
    if (F.tech) out.push(`<button class="hx-chip" type="button" data-unf="tech">${esc(S('with-tech'))} ×</button>`);
    return out.join('');
  };
  const filterPanel = () => {
    const groups = FACETS.map(([k, name]) => {
      const counts = facetCounts(k), vals = facetValues(k).sort((a, b) => (counts.get(b) || 0) - (counts.get(a) || 0) || String(facetLabel(k, a)).localeCompare(facetLabel(k, b)));
      if (!vals.length) return '';
      return `<div class="hx-fg"><h2>${esc(name)}</h2>${vals.map(v => `<label><input type="checkbox" data-f="${k}" value="${esc(v)}"${F[k].has(v) ? ' checked' : ''}>${k === 'colls' ? `<span class="hx-sw" style="--c:${esc(coll(v).color)}"></span>` : ''}<span>${esc(facetLabel(k, v))}</span><small>${counts.get(v) || 0}</small></label>`).join('')}</div>`;
    }).join('');
    const per = `<div class="hx-fg"><h2>${esc(S('f-period'))}</h2><select data-f="period" aria-label="${esc(S('f-period'))}">${PERIODS.map(p => `<option value="${p}"${F.period === p ? ' selected' : ''}>${esc(periodName(p))}</option>`).join('')}</select>${F.period === 'custom' ? `<div class="hx-range"><input type="date" data-f="from" value="${F.from}" min="${dayKey(oldest())}"><span>–</span><input type="date" data-f="to" value="${F.to}"></div>` : ''}</div>`;
    const st = `<div class="hx-fg"><h2>${esc(S('f-status'))}</h2><div class="seg" role="group">${['all', 'active', 'undone'].map(v => `<button type="button" data-fs="${v}" aria-pressed="${F.status === v}">${esc(S('fs-' + v))}</button>`).join('')}</div>
      <label class="hx-tech"><input type="checkbox" data-f="tech"${F.tech ? ' checked' : ''}><span>${esc(S('show-tech'))}</span><small>${log.filter(c => c.tech).length}</small></label></div>`;
    return per + st + groups + `<div class="row"><a href="#" data-reset>${esc(S('reset'))}</a></div>`;
  };

  // ---- page shell: toolbar with the view switch, then the view ---------------------------------
  const VIEWS = ['map', 'list'];
  let view = new URLSearchParams(location.search).get('view');
  if (!VIEWS.includes(view)) view = VIEWS.includes(localStorage.hxView) ? localStorage.hxView : 'map';
  root.innerHTML = `<div class="toolbar"><h1>${esc(S('title'))}</h1><span class="muted small" data-r="count"></span><span class="grow"></span>
    <span class="seg" role="group" data-r="zoom" aria-label="${esc(S('zoom'))}">${[90, 30, 7].map(z => `<button type="button" data-zoom="${z}">${esc(tr('days', { n: z }))}</button>`).join('')}</span>
    <label class="hx-q">${ico('i-filter')}<input type="search" data-f="q" placeholder="${esc(S('q'))}" autocomplete="off" aria-label="${esc(S('q'))}"></label>
    <div class="filter click hx-filter"><button class="icon-btn" type="button" aria-haspopup="true" title="${esc(S('filter'))}" aria-label="${esc(S('filter'))}">${ico('i-filter')}<span class="hx-badge" data-r="badge"></span></button><div class="pop hx-fpop" data-r="panel"></div></div>
    <span class="seg" role="group" aria-label="${esc(S('view'))}">
      <button type="button" data-view="map" title="${esc(S('v-map'))}" aria-label="${esc(S('v-map'))}">${ico('i-gantt')}</button>
      <button type="button" data-view="list" title="${esc(S('v-list'))}" aria-label="${esc(S('v-list'))}">${ico('i-list')}</button>
    </span>
  </div>
  <div class="hx-chips" data-r="chips"></div>
  <div data-r="view"></div>`;
  const R = n => $(`[data-r="${n}"]`, root);
  // the filter popover is drawn after app.js placed every popover on the page: placed here, the same way —
  // by chrome() once the toolbar holds its final count/badge/zoom (fitted before, the list view's hidden zoom
  // moved the button and left it 7.75 px past a 390 px screen until a resize) and on open
  const hxf = $('.hx-filter', root), fit = () => practical.fitPop(R('panel'));
  ['pointerenter', 'focusin', 'click'].forEach(t => hxf.addEventListener(t, fit));
  const subs = [];
  const emit = (info = {}) => { for (const fn of subs) fn(info); };
  const flash = id => { const n = $(`[data-cid="${id}"]`, root); if (n) { n.classList.remove('hx-flash'); void n.getBoundingClientRect(); n.classList.add('hx-flash'); } };
  function chrome() {
    const n = list().length;
    R('count').textContent = tr('count', { n });
    R('panel').innerHTML = filterPanel();
    const ac = activeCount(); R('badge').textContent = ac || '';
    R('chips').innerHTML = activeChips();
    $$('[data-view]', root).forEach(b => b.setAttribute('aria-pressed', b.dataset.view === view));
    R('zoom').hidden = view !== 'map';
    fit();
  }

  // ---- view: line map ---------------------------------------------------------------------------
  const Map_ = (() => {
    const PPD = { 7: 150, 30: 38, 90: 13 }, ROWH = 64, LY = 38; // LY + 22 = the stop's number, inside the row
    let zoom = +localStorage.hxZoom || 30, openId = null, host = null, resizeOn = false;
    // pixels per day: the zoom's minimum, stretched so the days fill the window's width
    let ppd = 38;
    const fit = () => {
      const sc = host && $('[data-m="map"]', host);
      const lw = sc ? parseFloat(getComputedStyle(sc).getPropertyValue('--lw')) || 210 : 210;
      const avail = sc ? sc.clientWidth - lw - 40 - 2 : 0;
      ppd = Math.max(PPD[zoom], avail / zoom);
    };
    if (!PPD[zoom]) zoom = 30;
    const shape = (act, hollow, big) => {
      const c = `var(--hx-${act})`, f = hollow ? 'var(--bg)' : c, s = `fill="${f}" stroke="${c}" stroke-width="2"`, k = big ? 1.25 : 1;
      switch (act) {
        case 'create': return `<circle r="${6.5 * k}" ${s}/><circle r="${2.2 * k}" fill="${hollow ? c : 'var(--bg)'}"/>`;
        case 'delete': return `<rect x="${-5.5 * k}" y="${-5.5 * k}" width="${11 * k}" height="${11 * k}" rx="2" ${s}/>`;
        case 'restore': return `<rect x="${-5 * k}" y="${-5 * k}" width="${10 * k}" height="${10 * k}" rx="1.5" transform="rotate(45)" ${s}/>`;
        case 'move': return `<path d="M${-6 * k} ${5 * k}L0 ${-6 * k}L${6 * k} ${5 * k}Z" ${s} stroke-linejoin="round"/>`;
        default: return `<circle r="${5.5 * k}" ${s}/>`;
      }
    };
    // a bulk change and its undo share one row
    const bulkRoot = c => { let x = c; while (x.reverts && byId[x.reverts] && isBulk(byId[x.reverts])) x = byId[x.reverts]; return x.id; };
    function rows(lst, start) {
      const map = new Map();
      for (const c of [...lst].reverse()) {
        if (c.t < start) continue;
        const keys = isBulk(c) && actOf(c) !== 'move' ? ['b' + bulkRoot(c)] : [...new Set(c.items.map(it => it.el))];
        for (const key of keys) {
          if (!map.has(key)) {
            const b = key[0] === 'b' ? byId[+key.slice(1)] : null, el = b ? b.items[0].el : key, e = els[el] || {};
            map.set(key, { key, bulk: b, el, label: b ? bulkTitle(b) : elTitle(el), kind: e.kind || kindOf(c), color: coll(e.coll || collOf(c)).color, stops: [] });
          }
          const n = key[0] === 'b' ? c.items.length : c.items.filter(i => i.el === key).length;
          const it = key[0] === 'b' ? c.items[0] : c.items.find(i => i.el === key);
          map.get(key).stops.push({ c, act: key[0] === 'b' ? actOf(c) : it.act, n });
        }
      }
      const out = [...map.values()];
      for (const r of out) {
        let last = -99;
        for (const s of r.stops) { s.x = Math.max((s.c.t - start) / 864e5 * ppd, last + 16); last = s.x; }
        r.alive = r.bulk ? r.bulk.items.some(i => (els[i.el] || {}).exists) : (els[r.el] || {}).exists;
      }
      return out.sort((a, b) => b.stops[b.stops.length - 1].c.id - a.stops[a.stops.length - 1].c.id);
    }
    function rowSvg(r, W, stopX, nx) {
      let segs = '', x0 = null, ghost = null;
      if (!(r.stops[0].act === 'create' || r.stops[0].act === 'restore')) x0 = 0;
      for (const s of r.stops) {
        if ((s.act === 'create' || s.act === 'restore') && x0 === null) { if (ghost !== null) segs += `<line x1="${ghost}" x2="${s.x}" y1="${LY}" y2="${LY}" class="hxg-ghost"/>`; ghost = null; x0 = s.x; }
        if (s.act === 'delete' && x0 !== null) { segs += `<line x1="${x0}" x2="${s.x}" y1="${LY}" y2="${LY}" class="hxg-line" style="stroke:${esc(r.color)}"/>`; x0 = null; ghost = s.x; }
      }
      if (x0 !== null) segs += `<line x1="${x0}" x2="${Math.max(nx, r.stops[r.stops.length - 1].x)}" y1="${LY}" y2="${LY}" class="hxg-line" style="stroke:${esc(r.color)}"/>`;
      // undo arcs: from the undo stop back to the change it undid (or off the left edge)
      let arcs = '';
      for (const s of r.stops) if (s.c.reverts) {
        const x1 = s.x, x2 = stopX.has(s.c.reverts) ? stopX.get(s.c.reverts) : -30, h = Math.min(28, 12 + Math.abs(x1 - x2) / 5); // the arc's top stays inside the row
        arcs += `<path d="M${x1} ${LY - 8} C${x1} ${LY - 8 - h}, ${x2} ${LY - 8 - h}, ${x2} ${LY - 9}" class="hxg-arc" marker-end="url(#hxg-ah)"/>`;
      }
      // stops closer than a label's width share one label on the first of them ("#16–18")
      const lab = new Map(); let g = null;
      for (const s of r.stops) { if (g && s.x - g.last < 28) { g.ids.push(s.c.id); g.last = s.x; } else { g = { ids: [s.c.id], last: s.x }; lab.set(s.c.id, g); } }
      const labelOf = s => { const q = lab.get(s.c.id); if (!q) return ''; const a = q.ids; return '#' + (a.length === 1 ? a[0] : a.length === 2 ? `${a[0]}·${a[1]}` : `${a[0]}–${a[a.length - 1]}`); };
      const stops = r.stops.map(s => {
        const st = status(s.c), hollow = st !== 'active';
        const body = s.n > 1 || r.bulk
          ? `<rect x="-15" y="-9" width="30" height="18" rx="9" fill="${hollow ? 'var(--bg)' : `var(--hx-${s.act})`}" stroke="var(--hx-${s.act})" stroke-width="2"/><text y="4" text-anchor="middle" class="hxg-n${hollow ? ' h' : ''}">${s.n}</text>`
          : shape(s.act, hollow, s.act === 'create');
        const ring = s.c.src === 'undo' ? '<circle r="11" class="hxg-ring"/>' : '';
        const txt = sentence(s.c, false).replace(/<[^>]+>/g, '');
        // The <title> IS the name — no aria-label beside it. axe counts an SVG title as the
        // element's visible text, so a name that said anything else was a
        // `label-content-name-mismatch` (WCAG 2.5.3, axe-core 4.14; the shape-only stops, which
        // show no label and no count badge, failed it). Through the subtree the name also picks
        // up the count badge and the shared label ("#16·17"), which is what a voice-control user
        // can actually say.
        return `<g class="hxg-stop${s.c.id === openId ? ' open' : ''}" data-cid="${s.c.id}" data-status="${st}" transform="translate(${s.x},${LY})" tabindex="0" role="button"><title>#${s.c.id} · ${esc(when(s.c.t))} · ${esc(txt)}</title><circle r="14" class="hxg-hit"/>${ring}${body}${labelOf(s) ? `<text y="22" text-anchor="middle" class="hxg-id">${labelOf(s)}</text>` : ''}</g>`;
      }).join('');
      return `<svg width="${W}" height="${ROWH}" class="hxg-row">${segs}${arcs}${stops}</svg>`;
    }
    function axis(start, W) {
      let t = '';
      for (let d = 0; d <= zoom; d++) {
        const day = new Date(start.getTime() + d * 864e5), x = d * ppd, mon = day.getDate() === 1, wk = day.getDay() === 1;
        const lab = zoom === 7 ? `${fmtWd.format(day)} ${day.getDate()}.${day.getMonth() + 1}.` : zoom === 30 ? (wk || mon ? `${day.getDate()}.${day.getMonth() + 1}.` : '') : (mon ? fmtMonthL.format(day) : wk ? `${day.getDate()}.` : '');
        if (d < zoom) t += `<line x1="${x}" x2="${x}" y1="${mon ? 14 : 22}" y2="30" class="hxg-tick${mon ? ' mon' : ''}"/>${lab ? `<text x="${x + 3}" y="${mon && zoom === 90 ? 12 : 20}" class="hxg-lab${mon ? ' mon' : ''}">${esc(lab)}</text>` : ''}`;
      }
      const nx = (now() - start) / 864e5 * ppd;
      return `<svg width="${W}" height="30" class="hxg-axis">${t}<line x1="${nx}" x2="${nx}" y1="4" y2="30" class="hxg-nowl"/><text x="${nx - 4}" y="10" text-anchor="end" class="hxg-now">${esc(S('now'))}</text></svg>`;
    }
    const legend = () => `<div class="hxg-legend">${ACTS.map(a => `<span><svg width="16" height="16" viewBox="-8 -8 16 16">${shape(a, false)}</svg>${esc(actWord(a))}</span>`).join('')}
      <span><svg width="16" height="16" viewBox="-8 -8 16 16">${shape('update', true)}</svg>${esc(S('lg-hollow'))}</span>
      <span><svg width="26" height="16" viewBox="0 0 26 16"><path d="M22 13 C22 1, 4 1, 4 13" class="hxg-arc"/></svg>${esc(S('lg-arc'))}</span></div>`;
    function mount(h) {
      host = h;
      h.innerHTML = `${legend()}<div class="hxg-scroll" data-m="map"></div><div class="hxg-pop" data-m="pop" hidden></div><div class="hx-foot" data-m="foot"></div>`;
      $('[data-m="map"]', h).addEventListener('scroll', () => { if (openId) pop(); });
      if (!resizeOn) { resizeOn = true; let rz; addEventListener('resize', () => { clearTimeout(rz); rz = setTimeout(() => { if (host) render(false); }, 150); }); } // once: every mount used to add one more
      render(true);
    }
    const M = n => $(`[data-m="${n}"]`, host);
    function render(scrollEnd) {
      if (!host) return;
      const lst = list();
      const end = new Date(dayKey(now())); end.setDate(end.getDate() + 1);
      const start = new Date(end.getTime() - zoom * 864e5);
      fit();
      let rs = rows(lst, start);
      // stops of one moment push each other past "now": make room for them inside the width
      const over = Math.max(0, ...rs.flatMap(r => r.stops.map(s => s.x + 30))) - zoom * ppd;
      if (over > 0 && ppd > PPD[zoom]) { ppd = Math.max(PPD[zoom], ppd - over / zoom); rs = rows(lst, start); }
      const shown = new Set(rs.flatMap(r => r.stops.map(s => s.c.id)));
      const stopX = new Map(rs.flatMap(r => r.stops.map(s => [s.c.id, s.x])));
      // stops of one moment push each other right, past "now": the map grows with them
      const W = Math.max(zoom * ppd, ...[...stopX.values()].map(x => x + 30));
      for (const b of R('zoom').children) b.setAttribute('aria-pressed', +b.dataset.zoom === zoom);
      const nx = (now() - start) / 864e5 * ppd;
      const sc = M('map'), keep = sc.scrollLeft;
      sc.innerHTML = rs.length ? `<div class="hxg-in" style="--w:${W}px">
        <svg width="0" height="0" style="position:absolute"><defs><marker id="hxg-ah" viewBox="0 0 8 8" refX="4" refY="4" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0 0L8 4L0 8z" fill="var(--hx-undo)"/></marker></defs></svg>
        <div class="hxg-r hxg-head"><div class="hxg-l"></div><div class="hxg-c">${axis(start, W)}</div></div>
        ${rs.map(r => `<div class="hxg-r"><div class="hxg-l"><span class="hxg-sw" style="background:${esc(r.color)}"></span>${ico(r.kind === 'task' ? 'i-tasks' : r.kind === 'contact' ? 'i-contacts' : 'i-calendar')}${r.bulk ? `<span class="t">${esc(r.label)}</span>` : `<a href="#" class="hx-el t" data-el="${esc(r.el)}" title="${esc(S('only-el-tip'))}">${esc(r.label || '—')}</a>`}${r.alive ? '' : `<small>${esc(actWord('delete'))}</small>`}</div><div class="hxg-c"><i class="hxg-nowc" style="left:${nx}px"></i>${rowSvg(r, W, stopX, nx)}</div></div>`).join('')}
      </div>` : `<div class="hx-empty">${esc(S('empty-window'))} <a href="#" data-reset>${esc(S('reset'))}</a>${zoom < 90 ? ` · <a href="#" data-zoom="90">${esc(tr('days', { n: 90 }))}</a>` : ''}</div>`;
      sc.scrollLeft = scrollEnd ? sc.scrollWidth : keep;
      const hidden = lst.filter(c => !shown.has(c.id)).length;
      M('foot').innerHTML = (hidden ? `${esc(tr('before-window', { n: hidden }))} <a href="#" data-zoom="90">${esc(tr('days', { n: 90 }))}</a>. ` : '') + esc(tr('retention', { n: D.retention, date: fmtShort.format(oldest()) }));
      if (openId && !shown.has(openId)) openId = null;
      pop();
    }
    function pop() {
      const p = M('pop');
      if (!openId || !byId[openId]) { p.hidden = true; return; }
      const c = byId[openId], g = $(`.hxg-stop[data-cid="${openId}"]`, host);
      p.innerHTML = `<button class="hxg-x" type="button" data-close aria-label="${esc(S('close'))}">×</button>
        <div class="hxg-ph" data-src="${c.src}">${avatar(c)}<div><div class="hxg-ps">${sentence(c)}</div>
        <div class="hxg-pm"><span class="hx-num">#${c.id}</span><span>${esc(dayLabel(c.t))}, ${esc(time(c.t))}</span><span class="hx-src">${ico(srcIcon(c))}${esc(srcLabel(c))}</span>${c.tech ? `<span class="hx-st">${esc(S('tech'))}</span>` : ''}</div></div></div>
        <div data-status="${status(c)}">${diff(c, 12)}</div>
        <div class="hxg-pf">${statusTag(c)}<span class="grow"></span>${undoBtn(c, 'btn btn-primary', true)}</div>`;
      p.hidden = false;
      $$('.hxg-stop.open', host).forEach(n => n.classList.remove('open')); if (g) g.classList.add('open');
      if (g && matchMedia('(min-width: 721px)').matches) {
        const b = g.getBoundingClientRect(), w = 380, x = Math.min(innerWidth - w - 12, Math.max(12, b.left + b.width / 2 - w / 2));
        const below = b.bottom + 10, h = p.offsetHeight;
        p.style.left = x + 'px'; p.style.top = (below + h > innerHeight - 12 ? Math.max(70, b.top - h - 10) : below) + 'px';
      } else { p.style.left = ''; p.style.top = ''; }
    }
    function click(e) {
      const z = e.target.closest('[data-zoom]'); if (z) { e.preventDefault(); zoom = +z.dataset.zoom; localStorage.hxZoom = zoom; render(!openId); const g = openId && $(`.hxg-stop[data-cid="${openId}"]`, host); if (g) { g.scrollIntoView({ block: 'nearest', inline: 'center' }); pop(); } return true; }
      if (!host) return false;
      if (e.target.closest('[data-close]')) { openId = null; pop(); return true; }
      const s = e.target.closest('.hxg-stop'); if (s) { openId = +s.dataset.cid; pop(); return true; }
      if (!e.target.closest('.hxg-pop') && !e.target.closest('.filter') && !e.target.closest('.ask') && openId) { openId = null; pop(); }
      return false;
    }
    function key(e) {
      const s = e.target.closest && e.target.closest('.hxg-stop'); if (s && (e.key === 'Enter' || e.key === ' ')) { e.preventDefault(); openId = +s.dataset.cid; pop(); }
      if (e.key === 'Escape' && openId && !$('.ask')) { openId = null; pop(); }
    }
    function go(id) {
      const c = byId[id], end = new Date(dayKey(now())); end.setDate(end.getDate() + 1);
      if (c.t < new Date(end.getTime() - zoom * 864e5)) zoom = 90;
      openId = id; render();
      const g = $(`.hxg-stop[data-cid="${id}"]`, host);
      if (g) { g.scrollIntoView({ block: 'center', inline: 'center' }); pop(); flash(id); }
    }
    return { mount, render: () => render(false), unmount: () => { host = null; openId = null; }, click, key, go, added: id => { openId = id; }, onScroll: () => { if (openId) pop(); } };
  })();

  // ---- view: list with details and the day ruler -------------------------------------------------
  const List = (() => {
    let host = null, sel = null, mobileDetail = false, lst = [];
    const li = c => `<button class="hxc-it${c.id === sel ? ' sel' : ''}" type="button" data-pick="${c.id}" data-cid="${c.id}" data-status="${status(c)}" data-src="${c.src}">
        ${avatar(c)}<span class="hxc-t"><b>${esc(title(c))}</b><small>${actTag(actOf(c))}<span>${esc(kindName(kindOf(c), isBulk(c) ? 2 : 1))} · ${esc(actor(c))}</span></small></span>
        <span class="hxc-w"><small>${esc(when(c.t))}</small>${status(c) !== 'active' ? `<i class="hxc-dot ${status(c)}" title="${esc(S('dot-' + status(c)))}"></i>` : ''}</span></button>`;
    // before / after / today per attribute; "today" differs from "after" = changed later
    function table(c, p) {
      const nowOf = (el, k) => { if (!p) return undefined; const x = p.now.find(n => n[0] === el && (n[1] === k || n[1] === '')); return x ? (x[1] === '' ? null : x[2]) : undefined; };
      const head = `<thead><tr><th></th><th>${esc(S('col-before'))}</th><th>${esc(S('col-after'))}</th><th>${esc(S('col-now'))}</th></tr></thead>`;
      if (isBulk(c)) return `<table class="hxc-tab">${head.replace('<th></th>', `<th>${esc(S('col-el'))}</th>`)}<tbody>${c.items.map(it => {
        const e = els[it.el] || {}, a = it.attrs.find(x => x[0] !== 'SUMMARY' && x[0] !== 'FN') || [];
        const upd = it.act === 'update' || it.act === 'move';
        const before = upd ? a[2] : it.act === 'create' ? '—' : it.act === 'restore' ? actWord('delete') : S('present');
        const after = upd ? a[3] : it.act === 'delete' ? actWord('delete') : it.act === 'create' ? S('new') : S('present');
        const today = !e.exists ? actWord('delete') : upd ? '' : S('present');
        return `<tr><th>${esc(elTitle(it.el) || it.title)}</th><td class="hxc-vb">${esc(before || S('empty'))}</td><td class="hxc-va">${esc(after || S('empty'))}</td><td class="${today === actWord('delete') && after !== today ? 'hxc-vx' : 'hxc-vn'}">${esc(today || '·')}</td></tr>`;
      }).join('')}</tbody></table>`;
      const it = c.items[0], e = els[it.el] || {};
      if (it.act !== 'update' && it.act !== 'move') {
        const rows = it.attrs.filter(a => a[2] || a[3]);
        return `<p class="hxc-note">${esc(S('note-' + it.act))}</p><table class="hxc-tab"><tbody>${rows.map(([k, label, from, to]) => `<tr><th>${esc(label)}</th><td colspan="3">${esc(to || from)}</td></tr>`).join('')}</tbody></table>`;
      }
      return `<table class="hxc-tab">${head}<tbody>${it.attrs.map(([k, label, from, to]) => {
        // the plan knows today's value of an edited attribute; the collection is the element's own
        const cur = !e.exists ? null : k === 'X-HX-COLL' ? coll(e.coll).name : p && nowOf(it.el, k) === undefined ? to : nowOf(it.el, k);
        const cls = cur === null ? 'hxc-vg' : cur === undefined || cur === to ? 'hxc-vn' : 'hxc-vx';
        const txt = cur === null ? esc(actWord('delete')) : cur === undefined ? '<span class="muted">…</span>' : cur === to ? `<span class="muted">${esc(S('same-as-after'))}</span>` : esc(cur || S('empty'));
        const ld = (from || '').includes('\n') || (to || '').includes('\n') ? lineDiff(from, to) : null;
        // a cell stays a table cell: the one-per-line values get a box of their own
        const before = ld ? `<div class="hx-lines">${ld.before}</div>` : from ? `<del>${esc(from)}</del>` : `<span class="muted">${esc(S('empty'))}</span>`;
        const after = ld ? `<div class="hx-lines">${ld.after}</div>` : to ? `<ins>${esc(to)}</ins>` : `<span class="muted">${esc(S('empty'))}</span>`;
        return `<tr><th>${esc(label)}</th><td class="hxc-vb">${before}</td><td class="hxc-va">${after}</td><td class="${cls}">${txt}</td></tr>`;
      }).join('')}</tbody></table>`;
    }
    function preview(c, p) {
      if (!p) return `<div class="hxc-pv off">${ico('i-restore')}<div><p class="muted">…</p></div></div>`;
      if (!p.ok) return `<div class="hxc-pv off">${ico('i-info')}<div><b>${esc(S('pv-no'))}</b><p>${esc(whyText(p.why, (p.skips[0] || {}).by, c))}</p></div></div>`;
      const lines = planText(p);
      return `<div class="hxc-pv${p.warns.length ? ' warn' : ''}">${ico(p.warns.length ? 'i-info' : 'i-restore')}<div><b>${esc(S('undo'))}</b>
        ${lines.length ? `<ul>${p.warns.map(w => `<li>${esc(warnText(w))}${w.by ? ` <a href="#" class="hx-go" data-go="${w.by}">${esc(tr('see', { ref: '#' + w.by }))}</a>` : ''}</li>`).join('')}${p.skips.map(s => `<li class="muted">${esc(tr('w-skip', { title: s.title, why: whyText(s.why, s.by) }))}</li>`).join('')}</ul>` : `<p>${esc(S('pv-plain'))}</p>`}
        <p class="muted small">${esc(S('pv-note'))}</p>
        <button class="btn btn-primary hx-undo" type="button" data-undo-now="${c.id}">${ico('i-restore')}<span>${esc(p.warns.length ? S('undo-anyway') : S('undo'))}</span></button></div></div>`;
    }
    function detail(p) {
      const c = byId[sel];
      if (!c) return `<div class="hx-empty">${esc(S('pick'))}</div>`;
      const cl = coll(collOf(c)), el = !isBulk(c) ? c.items[0].el : null;
      const hist = el ? log.filter(x => x.items.some(i => i.el === el)).reverse() : [];
      return `<div class="hxc-card" data-cid="${c.id}" data-status="${status(c)}" data-src="${c.src}">
        <button class="btn hxc-back" type="button" data-back>‹ ${esc(S('back'))}</button>
        <header><span class="hx-num">#${c.id}</span><h2>${esc(title(c))}</h2>${actTag(actOf(c))}</header>
        <p class="hxc-s">${sentence(c)}</p>
        <div class="hxc-meta"><span>${ico('i-clock')}${esc(dayLabel(c.t))}, ${esc(time(c.t))}</span><span class="hx-src">${ico(srcIcon(c))}${esc(srcLabel(c))}</span><span><i class="hxc-sw" style="background:${esc(cl.color)}"></i>${esc(cl.name)}</span>
          ${c.src === 'dav' && c.prodid ? `<span class="muted" title="PRODID">${esc(c.prodid)}</span>` : ''}${c.tech ? `<span class="hx-st">${esc(S('tech'))}</span>` : ''}</div>
        ${statusTag(c) ? `<div class="hxc-st">${statusTag(c)}</div>` : ''}
        ${table(c, p)}
        ${preview(c, p)}
        ${hist.length > 1 ? `<h3>${esc(tr('hist-of', { title: elTitle(el) }))}</h3><ol class="hxc-hist">${hist.map(x => `<li class="${x.id === c.id ? 'cur' : ''}" data-status="${status(x)}"><button type="button" data-pick="${x.id}"><span class="hx-num">#${x.id}</span>${actTag(actOf(x))}<span>${esc(what(x))}</span><small>${esc(when(x.t))} · ${esc(actor(x))}</small>${status(x) !== 'active' ? `<small class="hxc-hs">${esc(S('dot-' + status(x)))}</small>` : ''}</button></li>`).join('')}</ol>` : ''}
      </div>`;
    }
    function ruler() {
      const days = D.retention, per = new Map();
      for (const c of lst) { const d = dayDiff(c.t); per.set(d, (per.get(d) || 0) + 1); }
      const cur = byId[sel], curD = cur ? dayDiff(cur.t) : -1;
      let html = '';
      for (let d = days; d >= 0; d--) {
        const t = now(); t.setDate(t.getDate() - d);
        const n = per.get(d) || 0, month = t.getDate() === 1 || d === days;
        html += `<div class="hxf-day${n ? ' has' : ''}${d === curD ? ' cur' : ''}${month ? ' mon' : ''}" data-day="${d}" style="--n:${Math.min(n, 4)}" title="${esc(dayLabel(t))}${n ? ` · ${esc(tr('count', { n }))}` : ''}">${month ? `<b>${esc(fmtMonth.format(t))}</b>` : ''}${d === 0 ? `<b class="now">${esc(S('today'))}</b>` : ''}<i></i></div>`;
      }
      return html;
    }
    function mount(h) {
      host = h;
      h.innerHTML = `<div class="hxc" data-m="wrap"><aside class="hxc-l"><div class="hxc-list" data-m="list"></div><div class="hx-foot" data-m="foot"></div></aside><section class="hxc-d" data-m="detail"></section><div class="hxf-ruler" data-m="ruler" aria-label="${esc(S('ruler'))}"></div></div>`;
      render();
    }
    const M = n => $(`[data-m="${n}"]`, host);
    async function showDetail() {
      const id = sel, p = byId[id] ? await planOf(id) : null;
      if (!host || sel !== id) return;
      M('detail').innerHTML = detail(p);
    }
    function render() {
      if (!host) return;
      lst = list();
      if (!byId[sel]) sel = lst[0] && lst[0].id;
      let html = '', day = '';
      for (const c of lst) { const d = dayKey(c.t); if (d !== day) { html += `<h2>${esc(dayLabel(c.t))}</h2>`; day = d; } html += li(c); }
      M('list').innerHTML = html || `<div class="hx-empty">${esc(S('none'))} <a href="#" data-reset>${esc(S('reset'))}</a></div>`;
      M('foot').textContent = tr('retention', { n: D.retention, date: fmtShort.format(oldest()) });
      M('detail').innerHTML = detail(null);
      M('ruler').innerHTML = ruler();
      M('wrap').classList.toggle('show-d', mobileDetail);
      showDetail();
    }
    const pick = id => { sel = id; mobileDetail = true; render(); const n = $(`.hxc-list [data-cid="${id}"]`, host); if (n) n.scrollIntoView({ block: 'nearest' }); };
    function click(e) {
      if (!host) return false;
      const p = e.target.closest('[data-pick]'); if (p) { pick(+p.dataset.pick); return true; }
      const u = e.target.closest('[data-undo-now]'); if (u) { undo(+u.dataset.undoNow, true); return true; }
      if (e.target.closest('[data-back]')) { mobileDetail = false; render(); return true; }
      const day = e.target.closest('.hxf-day.has'); if (day) { const d = +day.dataset.day, c = lst.find(x => dayDiff(x.t) === d); if (c) pick(c.id); return true; }
      return false;
    }
    function key(e) {
      if (!host || $('.ask') || /input|textarea|select/i.test((e.target.tagName || ''))) return;
      if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
      const i = lst.findIndex(c => c.id === sel), j = Math.min(lst.length - 1, Math.max(0, i + (e.key === 'ArrowDown' ? 1 : -1)));
      if (lst[j] && j !== i) { e.preventDefault(); pick(lst[j].id); }
    }
    function go(id) { pick(id); const n = $('.hxc-card', host); if (n) { n.classList.remove('hx-flash'); void n.offsetWidth; n.classList.add('hx-flash'); } }
    return { mount, render, unmount: () => { host = null; }, click, key, go, added: id => { sel = id; }, onScroll: () => {} };
  })();

  // ---- wiring ---------------------------------------------------------------------------------
  const V = () => view === 'map' ? Map_ : List;
  function show() {
    localStorage.hxView = view;
    Map_.unmount(); List.unmount();
    chrome();
    V().mount(R('view'));
  }
  subs.push(info => { if (info.added) V().added(info.added); chrome(); V().render(); if (info.added) requestAnimationFrame(() => V().go(info.added)); });
  const go = id => {
    if (!byId[id]) return;
    if (!match(byId[id])) { resetF(); if (byId[id].tech) F.tech = true; $('[data-f="q"]', root).value = ''; chrome(); V().render(); }
    V().go(id);
  };
  let qt;
  root.addEventListener('input', e => {
    const f = e.target.closest('[data-f="q"]'); if (!f) return;
    clearTimeout(qt); qt = setTimeout(() => { F.q = f.value.trim(); emit(); }, 150);
  });
  root.addEventListener('change', e => {
    const f = e.target.closest('[data-f]'); if (!f || f.dataset.f === 'q') return;
    const k = f.dataset.f;
    if (facet[k]) f.checked ? F[k].add(f.value) : F[k].delete(f.value);
    else if (k === 'period') { F.period = f.value; if (f.value !== 'custom') F.from = F.to = ''; }
    else if (k === 'from' || k === 'to') F[k] = f.value;
    else if (k === 'tech') F.tech = f.checked;
    emit();
  });
  root.addEventListener('click', e => {
    const t = e.target;
    const fb = t.closest('.hx-filter > button'); if (fb) { e.stopPropagation(); fb.parentElement.classList.toggle('open'); return; }
    const u = t.closest('[data-undo]'); if (u) { e.preventDefault(); if (!u.disabled) undo(+u.dataset.undo); return; }
    const g = t.closest('.hx-go'); if (g) { e.preventDefault(); go(+g.dataset.go); return; }
    const el = t.closest('.hx-el'); if (el) { e.preventDefault(); F.el = el.dataset.el; emit(); return; }
    const vb = t.closest('[data-view]'); if (vb) { const v = vb.dataset.view; practical.slide(Math.sign(VIEWS.indexOf(v) - VIEWS.indexOf(view)), () => { view = v; show(); }); return; } // it slides in from its side
    const fs = t.closest('[data-fs]'); if (fs) { F.status = fs.dataset.fs; emit(); return; }
    const un = t.closest('[data-unf]'); if (un) { const k = un.dataset.unf; if (facet[k]) F[k].delete(un.dataset.v); else if (k === 'status') F.status = 'all'; else if (k === 'period') { F.period = 'all'; F.from = F.to = ''; } else if (k === 'q') { F.q = ''; $('[data-f="q"]', root).value = ''; } else if (k === 'el') F.el = null; else if (k === 'tech') F.tech = false; emit(); return; }
    if (t.closest('[data-reset]')) { e.preventDefault(); resetF(); $('[data-f="q"]', root).value = ''; emit(); return; }
    V().click(e);
  });
  document.addEventListener('click', e => { if (!e.target.closest('.hx-filter')) $$('.hx-filter.open', root).forEach(f => f.classList.remove('open')); });
  document.addEventListener('keydown', e => V().key(e));
  const main = $('.main'); if (main) main.addEventListener('scroll', () => V().onScroll());
  // a sideways swipe: map | list (practical.swipeViews); not while a stop's details are open on the map
  if (main) practical.swipeViews(main, () => $$('.toolbar [data-view]', root), bs => bs.findIndex(b => b.dataset.view === view), () => !!$('.hxg-pop:not([hidden])', root));
  // a change anywhere (another tab, a device): reload the document, keep what is open
  let rt;
  document.addEventListener('rustical:change', () => { clearTimeout(rt); rt = setTimeout(async () => { if (await reload()) { chrome(); V().render(); } }, 400); });
  show();
})();
