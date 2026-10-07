/* js/tasks.js — the tasks page (#taskroot): six views over one list, the shared card editor beside them;
   window.rusticalTasks, rusticalHost.
   The server puts the tasks it wants shown into `#tasks-data` (the lookback decides how far
   back finished ones reach); everything here is drawing and picking. The editor is the same
   dialog the calendar uses (templates/editor.html) — on a wide window it is docked to the
   right instead of opening over the page. */
(function () {
  const root = document.getElementById('taskroot');
  if (!root) return;
  const { $, $$, parseYmd: parse, ymd: iso } = practical;
  const T = trs('js-tk-', ['open', 'doing', 'done', 'cancelled', 'nodate', 'overdue', 'today', 'week', 'later', 'del', 'err', 'sort', 'askDone', 'yes', 'no', 'asc', 'desc', 'done_at', 'due', 'wasdue', 'empty', 'prio', 'pct', 'count', 'keys', 'hist', 'histNone', 'nodateBox', 'loading', 'start', 'cats', 'edit']);
  const SORTL = trs('js-sort-', ['due', 'done', 'prio', 'pct', 'title', 'cal', 'created']);

  // the page ships every open task (the working set, bounded) and the first page of the finished
  // ones (unbounded by the lookback); the rest arrives as a view scrolls towards it
  const DATA = (() => { try { return JSON.parse($('#tasks-data').textContent); } catch (_) { return { open: [], done: [], done_total: 0, per_day: {} }; } })();
  let OPEN, PER_DAY, COMPLETED_TOTAL, CANCELLED_TOTAL, DONE, DONE_TOTAL, TASKS, gen = 0; // gen: which data a pending page of finished tasks belongs to
  const rebuild = () => { TASKS = OPEN.concat(DONE); };
  const setData = d => { OPEN = d.open || []; PER_DAY = d.per_day || {}; COMPLETED_TOTAL = d.completed_total || 0; CANCELLED_TOTAL = d.cancelled_total || 0; DONE = d.done || []; DONE_TOTAL = d.done_total || 0; rebuild(); gen++; };
  setData(DATA);
  /* a page of the finished list is a valid prefix of these two sorts only; every other one
     needs the whole list, so it is fetched before the view draws */
  const PREFIX_SAFE = ['due', 'done'];
  let loading = null;
  async function loadMore(all) {
    if (loading) return loading;
    if (DONE.length >= DONE_TOTAL) return null;
    const limit = all ? Math.min(500, DONE_TOTAL - DONE.length) : +root.dataset.chunk || 120;
    const u = `/api/tasks/done?back=${encodeURIComponent(root.dataset.back)}&offset=${DONE.length}&limit=${limit}` + (root.dataset.cals ? '&cals=' + encodeURIComponent(root.dataset.cals) : '');
    const g = gen;
    loading = fetch(u).then(r => r.ok ? r.json() : null).then(d => {
      if (g !== gen) return null; // the page's data was replaced meanwhile (reload)
      loading = null;
      if (!d) return null;
      DONE = DONE.concat(d.items);
      DONE_TOTAL = d.total;
      rebuild();
      return d;
    }).catch(() => { if (g === gen) loading = null; return null; });
    return loading;
  }
  async function ensureAll() { while (DONE.length < DONE_TOTAL) { if (!await loadMore(true)) break; } }
  /* the finished list runs newest first, so a day is fully loaded once the tail reaches past it */
  async function ensureUntil(day) {
    const oldest = () => { const l = DONE[DONE.length - 1]; return l && l.completed_at ? String(l.completed_at).slice(0, 10) : '9999-99-99'; };
    while (DONE.length < DONE_TOTAL && oldest() > day) { if (!await loadMore()) break; }
  }
  const main = $('#tkmain'), ed = $('#ed');
  const TODAY = parse(root.dataset.today);
  let view = root.dataset.view, sort = localStorage.tkSort || 'due', desc = localStorage.tkDesc === '1', cur = null, act = null; // cur: the task in the editor; act: the picked one (marked in every view that shows it)

  const dt = s => s ? new Date(String(s).replace(' ', 'T')) : null;
  const day = d => Math.round((d - TODAY) / 864e5);
  const esc = practical.escN;
  const ic = (n, cl) => practical.ico(n, cl || 's');
  // dfmt: cached formatters, the same text as toLocale*String(LOC, o) (o names its fields, so no defaults are added)
  const de = d => d ? dfmt(d, { day: '2-digit', month: '2-digit', year: 'numeric' }) : '';
  const deS = d => d ? dfmt(d, { day: '2-digit', month: '2-digit' }) : '';
  const deT = d => d ? dfmt(d, { day: '2-digit', month: '2-digit', year: '2-digit', hour: '2-digit', minute: '2-digit' }) : '';
  const key = t => `${t.principal}/${t.cal_id}/${t.object_id}`;

  const isDone = t => t.status === 'COMPLETED' || (!t.status && !!t.completed_at);
  const isOpen = t => !isDone(t) && t.status !== 'CANCELLED';
  const overdue = t => isOpen(t) && t.due && day(parse(t.due)) < 0;
  const pctOf = t => t.percent == null ? (isDone(t) ? 100 : 0) : t.percent;
  const dueTxt = t => t.due ? (t.due_all_day ? de(parse(t.due)) : deT(dt(t.due))) : '';

  // ---- sorting: the completed ones drop to the end, unless that is what we sort by ----
  function skey(t, k) {
    switch (k) {
      case 'due': return t.due ? parse(t.due).getTime() : 9e15;
      case 'done': return t.completed_at ? -dt(t.completed_at).getTime() : 9e15;
      case 'prio': return t.priority ? t.priority : 99;
      case 'pct': return -pctOf(t);
      case 'title': return t.summary.toLowerCase();
      case 'cal': return t.cal_name;
      case 'created': return t.created ? -dt(t.created).getTime() : 9e15;
    }
  }
  function sortList(list, k, d) {
    const s = list.slice().sort((a, b) => { const x = skey(a, k), y = skey(b, k); return x < y ? -1 : x > y ? 1 : a.summary.localeCompare(b.summary); });
    return d ? s.reverse() : s;
  }
  function sortView(list) {
    if (sort === 'done') return sortList(list.filter(t => t.completed_at), sort, desc).concat(sortList(list.filter(t => !t.completed_at), 'due', false));
    return sortList(list.filter(isOpen), sort, desc).concat(sortList(list.filter(t => !isOpen(t)), 'done', false));
  }

  // ---- shared atoms ----
  function chips(t, short) {
    const out = [];
    if (t.due) out.push(`<span class="tchip${overdue(t) ? ' warn' : ''}">${ic('clock', 'xs')}${t.start && !short ? deS(parse(t.start)) + '–' : ''}${dueTxt(t)}</span>`);
    if (t.completed_at) out.push(`<span class="tchip good">${ic('check', 'xs')}${T.done_at} ${deT(dt(t.completed_at))}</span>`);
    if (t.priority) out.push(`<span class="tchip">${ic(t.priority <= 4 ? 'prio-high' : t.priority === 5 ? 'prio-mid' : 'prio-low', 'xs')}${T.prio} ${t.priority}</span>`);
    const p = pctOf(t);
    if (p > 0 && p < 100) out.push(`<span class="tchip">${ic('target', 'xs')}${p} %</span>`);
    (t.categories || []).forEach(c => out.push(`<span class="tchip">${ic('tag', 'xs')}${esc(c)}</span>`));
    if (t.location) out.push(`<span class="tchip">${ic('pin', 'xs')}${esc(t.location.split(',')[0])}</span>`);
    if (t.alarms) out.push(`<span class="tchip">${ic('bell', 'xs')}${t.alarms}</span>`);
    if (t.links) out.push(`<span class="tchip">${ic('ext', 'xs')}${t.links}</span>`);
    if (t.class === 'PRIVATE' || t.class === 'CONFIDENTIAL') out.push(`<span class="tchip">${ic('lock', 'xs')}</span>`);
    if (t.status === 'CANCELLED') out.push(`<span class="tchip warn">${ic('thumbs-down', 'xs')}${T.cancelled}</span>`);
    if (t.status === 'IN-PROCESS') out.push(`<span class="tchip">${ic('dots', 'xs')}${T.doing}</span>`);
    return out.join('');
  }
  const cbox = t => `<button class="tcb${t.status === 'IN-PROCESS' ? ' pp' : ''}" role="checkbox" aria-checked="${isDone(t)}" data-tog="${esc(key(t))}" title="${T.done}">${ic(t.status === 'IN-PROCESS' ? 'dots' : 'check', 'xs')}</button>`;

  /* sort field and direction live in the page toolbar, between lookback and view switch; the
     direction button always shows the current order. The history lists by completion only, so
     there it keeps just the direction. */
  const sortBox = $('#tk-sortbox');
  sortBox.innerHTML = `<span class="sel" title="${esc(T.sort)}">${ic('swap')}<select id="tk-sort" aria-label="${esc(T.sort)}">${Object.entries(SORTL).map(([v, l]) => `<option value="${v}">${esc(l)}</option>`).join('')}</select></span><button class="btn icon-btn" id="tk-dir" type="button"></button>`;
  const sortSel = $('#tk-sort'), sortDir = $('#tk-dir');
  function syncSort() {
    sortSel.value = sort;
    sortSel.closest('.sel').hidden = view === 'history';
    sortDir.innerHTML = ic(desc ? 'sort-desc' : 'sort-asc');
    sortDir.title = desc ? T.desc : T.asc; sortDir.setAttribute('aria-label', sortDir.title);
  }
  sortSel.onchange = () => { sort = sortSel.value; localStorage.tkSort = sort; render(); };
  sortDir.onclick = () => { desc = !desc; localStorage.tkDesc = desc ? '1' : '0'; render(); };

  // ---- A: groups ----
  function bucket(t) {
    if (isDone(t)) return T.done;
    if (t.status === 'CANCELLED') return T.cancelled;
    if (!t.due) return T.nodate;
    const n = day(parse(t.due));
    return n < 0 ? T.overdue : n === 0 ? T.today : n <= 7 ? T.week : T.later;
  }
  const GICON = { [T.overdue]: 'prio-high', [T.today]: 'today', [T.week]: 'dayline', [T.later]: 'calendar', [T.nodate]: 'note', [T.done]: 'check', [T.cancelled]: 'thumbs-down' };
  function vGroups() {
    let h = '';
    const order = [T.overdue, T.today, T.week, T.later, T.nodate, T.done, T.cancelled];
    order.forEach(g => {
      const items = sortList(TASKS.filter(t => bucket(t) === g), g === T.done && sort === 'due' ? 'done' : sort, desc);
      if (!items.length) return;
      const n = g === T.done ? COMPLETED_TOTAL : g === T.cancelled ? CANCELLED_TOTAL : items.length;
      h += `<div class="tkg"><div class="tkgh${g === T.overdue ? ' due' : ''}">${ic(GICON[g] || 'list')}${esc(g)}<span class="n">${n}</span></div>`;
      items.forEach(t => {
        h += `<div class="tkcard${isDone(t) ? ' off' : ''}" data-key="${esc(key(t))}">${cbox(t)}
          <div><div class="ti">${esc(t.summary)}</div><div class="meta">${chips(t, true)}</div></div>
          <span class="sw" style="--c:${esc(t.color)}"></span></div>`;
      });
      h += '</div>';
    });
    return h;
  }

  // ---- B: table — the sheet is the editor: each cell saves on its own, like the contacts table ----
  const STL = { 'NEEDS-ACTION': T.open, 'IN-PROCESS': T.doing, COMPLETED: T.done, CANCELLED: T.cancelled };
  // the choices come from the editor's own selects, so both speak the same words
  const optsOf = name => $$(`#ed select[name=${name}] option`).map(o => [o.value, o.textContent]);
  const shownCols = () => $$('#tk-colpop input[data-col]').filter(i => i.checked).map(i => i.dataset.col);
  const colLabel = c => { const i = $(`#tk-colpop input[data-col="${c}"]`); return i ? i.dataset.label : c; };
  const SORTABLE = { cal: 'cal', due: 'due', percent: 'pct', priority: 'prio', completed: 'done', created: 'created' };
  // a date input stays a date, a time stays a time: switching between the two is the dialog's job
  function dtIn(t, f) {
    const v = t[f] ? String(t[f]) : '';
    const allDay = f === 'completed_at' ? false : (v ? t[f + '_all_day'] : true);
    return `<input type="${allDay ? 'date' : 'datetime-local'}" data-f="${f}" value="${esc(allDay ? v.slice(0, 10) : v.slice(0, 16))}"${f === 'due' && overdue(t) ? ' class="late"' : ''}${v ? '' : ' data-empty'}>`;
  }
  function selIn(f, cur, opts) {
    const list = opts.some(([v]) => v === cur) ? opts : opts.concat([[cur, cur]]);
    return `<select data-f="${f}">${list.map(([v, l]) => `<option value="${esc(v)}"${v === cur ? ' selected' : ''}>${esc(l)}</option>`).join('')}</select>`;
  }
  const cardCell = (card, html, cl) => `<td class="cardcell${cl ? ' ' + cl : ''}" data-card="${card}" title="${esc(T.edit)}">${html || '<span class="mut">–</span>'}</td>`;
  const edTxt = (f, v) => `<span contenteditable="plaintext-only" data-f="${f}">${esc(v)}</span>`;
  // the calendar of a task is fixed (a task cannot move)
  const CELL = {
    cal: t => `<td class="ro"><span class="sw" style="--c:${esc(t.color)}"></span> ${esc(t.cal_name)}</td>`,
    start: t => `<td>${dtIn(t, 'start')}</td>`,
    due: t => `<td>${dtIn(t, 'due')}</td>`,
    status: t => `<td>${selIn('status', t.status, Object.entries(STL))}</td>`,
    percent: t => `<td class="tkc"><input type="number" min="0" max="100" step="5" data-f="percent" value="${t.percent == null ? '' : t.percent}" placeholder="—"></td>`,
    priority: t => `<td>${selIn('priority', t.priority ? String(t.priority) : '', optsOf('priority'))}</td>`,
    completed: t => `<td>${dtIn(t, 'completed_at')}</td>`,
    // what a cell cannot hold opens the editor's own card for it (cardCell)
    categories: t => cardCell('cats', (t.categories || []).map(c => `<span class="tchip">${esc(c)}</span>`).join(' ')),
    location: t => cardCell('where', esc(t.location)),
    class: t => `<td>${selIn('class', t.class || '', optsOf('class'))}</td>`,
    created: t => `<td class="ro tnum">${t.created ? deT(dt(t.created)) : '—'}</td>`,
    description: t => cardCell('note', esc(t.description), 'desc'),
    alarms: t => cardCell('alarm', t.alarms ? `${ic('bell', 'xs')} ${t.alarms}` : ''),
    links: t => cardCell('links', t.links ? `${ic('ext', 'xs')} ${t.links}` : ''),
  };
  const rowHtml = (t, cols) => `<tr class="${isDone(t) ? 'off' : ''}" data-key="${esc(key(t))}"><td class="pin"><div class="pinw">${cbox(t)}${edTxt('summary', t.summary)}</div></td>` +
    cols.map(c => CELL[c](t)).join('') + '</tr>';
  function vTable() {
    const cols = shownCols().filter(c => CELL[c]);
    const th = (label, sk, cl) => `<th${cl ? ` class="${cl}"` : ''}${sk ? ` data-sk="${sk}"` : ''}>${esc(label)}${sk && sort === sk ? ` <span class="a ${desc ? 'desc' : 'asc'}">${ic('sort', 'xs')}</span>` : ''}</th>`;
    return `<div class="sheetwrap"><table class="sheet"><thead><tr>${th(SORTL.title, 'title', 'pin')}${cols.map(c => th(colLabel(c), SORTABLE[c])).join('')}</tr></thead><tbody>` +
      sortView(TASKS).map(t => rowHtml(t, cols)).join('') + `</tbody></table>${SENTINEL()}</div>`;
  }
  const find = k => TASKS.find(x => key(x) === k);
  // a row with the cursor in it is updated cell by cell, leaving the focused one alone —
  // tabbing on from a saved cell never loses the cursor and still sees the derived values
  function redrawRow(k) {
    const tr = $$('.sheet tbody tr', main).find(r => r.dataset.key === k), t = find(k);
    if (!tr || !t) return;
    const tmp = document.createElement('tbody');
    tmp.innerHTML = rowHtml(t, shownCols().filter(c => CELL[c]));
    const fresh = tmp.firstElementChild;
    if (!tr.contains(document.activeElement)) { fresh.classList.toggle('sel', tr.classList.contains('sel')); tr.replaceWith(fresh); return; }
    tr.classList.toggle('off', fresh.classList.contains('off'));
    $('.tcb', tr).replaceWith($('.tcb', fresh));
    $$('[data-f]', fresh).forEach(n => {
      const o = $(`[data-f="${n.dataset.f}"]`, tr);
      if (!o || o === document.activeElement) return;
      o.replaceWith(n);
    });
  }
  // after a save the server's answer wins: it keeps status, progress and completion consistent
  async function refresh(k) {
    const r = await practical.req('/api/tasks/' + k), d = r.data, t = find(k);
    if (!r.ok || !t) return;
    ['summary', 'description', 'location', 'start', 'start_all_day', 'due', 'due_all_day', 'status', 'percent', 'completed_at', 'class', 'categories'].forEach(f => { t[f] = d[f]; });
    t.priority = d.priority || 0; if (!t.status) t.status = d.completed_at ? 'COMPLETED' : 'NEEDS-ACTION'; // the list rows' rule (tasks.rs shown_status); the editor's answer keeps a missing STATUS missing
    t.alarms = (d.alarms || []).length; t.links = (d.links || []).length;
    $('#task-count').textContent = $('#task-count').textContent.replace(/^\d+/, TASKS.filter(isOpen).length);
    redrawRow(k);
  }
  async function saveCell(el) {
    const k = el.closest('tr').dataset.key, f = el.dataset.f;
    const v = el.isContentEditable ? el.textContent.trim() : el.value;
    const got = practical.tell(await practical.req('/api/tasks/' + k)); if (!got.ok) { refresh(k); return; }
    const d = got.data;
    switch (f) {
      case 'summary': if (!v) { refresh(k); return; } d.summary = v; break;
      case 'start': case 'due': d[f] = v || null; d[f + '_all_day'] = el.type === 'date'; break;
      case 'class': d.class = v; break;
      case 'priority': d.priority = v ? +v : null; break;
      case 'status': d.status = v; if (v !== 'COMPLETED') d.completed_at = null; break;
      // the three progress fields move together, the same rule the editor applies
      case 'percent': { const n = v === '' ? null : Math.max(0, Math.min(100, Math.round(+v)));
        d.percent = n; d.status = n === 100 ? 'COMPLETED' : n ? 'IN-PROCESS' : (d.status === 'COMPLETED' || d.status === 'IN-PROCESS' ? 'NEEDS-ACTION' : d.status); break; }
      case 'completed_at': d.completed_at = v || null; d.status = v ? 'COMPLETED' : (d.status === 'COMPLETED' ? 'NEEDS-ACTION' : d.status); break;
      default: return;
    }
    if (!await put(k, d)) { refresh(k); return; }
    const td = el.closest('td'); td.classList.add('saved'); setTimeout(() => td.classList.remove('saved'), 800);
    await refresh(k);
  }
  main.addEventListener('focusin', e => { const el = e.target.closest('.sheet [contenteditable][data-f]'); if (el) el.dataset.before = el.textContent; });
  main.addEventListener('focusout', e => {
    const el = e.target.closest('.sheet tbody [contenteditable][data-f]');
    if (el && el.textContent.trim() !== (el.dataset.before || '').trim()) saveCell(el);
  });
  main.addEventListener('change', e => { const el = e.target.closest('.sheet tbody input[data-f], .sheet tbody select[data-f]'); if (el) saveCell(el); });
  main.addEventListener('keydown', e => {
    const el = e.target.closest('.sheet [contenteditable]');
    if (!el) return;
    if (e.key === 'Escape' && el.dataset.f) { el.textContent = el.dataset.before || ''; el.blur(); }
    if (e.key === 'Enter') { e.preventDefault(); el.blur(); }
  });
  $$('#tk-colpop input[data-col]').forEach(i => i.addEventListener('change', () => {
    render();
    practical.send('/settings/task-columns', 'POST', shownCols()).then(practical.tell);
  }));

  // ---- C: board ----
  const BCOLS = [['NEEDS-ACTION', T.open, 'list'], ['IN-PROCESS', T.doing, 'dots'], ['COMPLETED', T.done, 'check'], ['CANCELLED', T.cancelled, 'thumbs-down']];
  function vBoard() {
    let h = '<div class="tkboard">';
    BCOLS.forEach(([st, label, icon]) => {
      const items = sortList(TASKS.filter(t => (st === 'COMPLETED' ? isDone(t) : t.status === st && !isDone(t))), st === 'COMPLETED' && sort === 'due' ? 'done' : sort, desc);
      const n = st === 'COMPLETED' ? COMPLETED_TOTAL : st === 'CANCELLED' ? CANCELLED_TOTAL : items.length;
      h += `<div class="tkcol${st === 'COMPLETED' ? ' done' : ''}" data-st="${st}"><div class="chd">${ic(icon, 'xs')}${esc(label)}<span class="n">${n}</span></div>`;
      items.forEach(t => {
        const p = pctOf(t);
        h += `<div class="tkkc" draggable="true" data-key="${esc(key(t))}" style="--c:${esc(t.color)}"><div class="ti">${esc(t.summary)}</div>
          ${p > 0 && p < 100 ? `<div class="pb"><i style="width:${p}%"></i></div>` : ''}<div class="ft">${chips(t, true)}</div></div>`;
      });
      h += '</div>';
    });
    return h + '</div>';
  }

  // ---- D: timeline ----
  function vTimeline() {
    const LO = -25, HI = 75, span = HI - LO;
    const x = n => (Math.max(LO, Math.min(HI, n)) - LO) / span * 100;
    let ax = '';
    for (let m = -1; m <= 2; m++) {
      const d = new Date(TODAY.getFullYear(), TODAY.getMonth() + m, 1), n = day(d);
      if (n < LO || n > HI) continue;
      ax += `<span style="left:${x(n)}%">${dfmt(d, { month: 'short', year: '2-digit' })}</span><i style="left:${x(n)}%"></i>`;
    }
    let h = `<div class="tktl"><div class="axis">${ax}</div><div class="now" style="left:${x(0)}%"><b>${T.today}</b></div>`;
    sortList(TASKS.filter(t => t.due || t.completed_at), sort, desc).forEach(t => {
      const anchor = t.completed_at ? day(dt(t.completed_at)) : day(parse(t.due));
      const from = (!t.completed_at && t.start) ? day(parse(t.start)) : anchor;
      const l = x(from), r = x(anchor), w = Math.max(r - l, 0), wide = w > 3.5;
      h += `<div class="lane"><div class="bar2${wide ? '' : ' dot'}${isDone(t) || t.status === 'CANCELLED' ? ' off' : ''}" data-key="${esc(key(t))}"
        style="--c:${esc(t.color)};left:calc(${l}% - ${wide ? 0 : 8}px)${wide ? `;width:${w}%` : ''}" title="${esc(t.summary)}">${isDone(t) ? ic('check', 'xs') : ''}${wide ? esc(t.summary) : ''}</div>
        ${wide ? '' : `<div class="lbl" style="left:calc(${l}% + 14px)">${esc(t.summary)}</div>`}</div>`;
    });
    h += '</div>';
    const nd = TASKS.filter(t => !t.due && !t.completed_at);
    if (nd.length) h += `<div class="muted small" style="margin-top:14px">${T.nodateBox}</div><div class="tknd">${nd.map(t => `<span class="tchip" data-key="${esc(key(t))}">${ic('note', 'xs')}${esc(t.summary)}</span>`).join('')}</div>`;
    return h;
  }

  // ---- E: deck ----
  let deckI = 0;
  function deckGo(i) { const list = sortView(TASKS); deckI = i >= list.length ? 0 : i; if (list[deckI]) act = key(list[deckI]); } // the card on top is the picked task
  function vDeck() {
    const list = sortView(TASKS);
    if (deckI >= list.length) deckI = 0;
    let h = '<div class="tkdeck" tabindex="0">';
    for (let k = Math.min(list.length - 1, deckI + 2); k >= deckI; k--) {
      const t = list[k], o = k - deckI;
      const p = pctOf(t);
      h += `<div class="cd" data-key="${esc(key(t))}" style="transform:translateY(${-o * 14}px) scale(${1 - o * 0.05});opacity:${o === 0 ? 1 : 0.85 - o * 0.25};z-index:${10 - o}${o ? ';height:120px;overflow:hidden' : ''}">
        <h3${isDone(t) ? ' class="off"' : ''}>${esc(t.summary)}</h3>${o ? '' :
        `${t.description ? `<p class="dsc">${esc(t.description)}</p>` : ''}<div class="fx">${chips(t)}<span class="tchip">${ic('calendar', 'xs')}${esc(t.cal_name)}</span></div>
         ${p > 0 && p < 100 ? `<div class="pb"><i style="width:${p}%"></i></div>` : ''}
         <div class="act"><button class="btn" data-tog="${esc(key(t))}">${isDone(t) ? '↺' : '✓'}</button><button class="btn icon-btn" data-open title="${esc(T.edit)}" aria-label="${esc(T.edit)}">${ic('pencil')}</button><button class="btn" data-next>→</button></div>`}</div>`;
    }
    return h + `</div><div class="tkcnt">${list.length ? deckI + 1 : 0} ${T.count} ${list.length}</div><div class="tkkeys">${esc(T.keys)}</div>`;
  }

  // ---- F: history ----
  let pick = null;
  function vHistory() {
    const done = TASKS.filter(t => t.completed_at);
    const per = PER_DAY; // the server counted the whole window; the list below is what is loaded
    const start = new Date(TODAY); start.setDate(start.getDate() - 364 - ((start.getDay() + 6) % 7));
    let cols = '';
    for (let w = 0; w < 53; w++) {
      let cells = '';
      for (let d = 0; d < 7; d++) {
        const c = new Date(start); c.setDate(c.getDate() + w * 7 + d);
        if (c > TODAY) { cells += '<div class="dy" style="visibility:hidden"></div>'; continue; }
        const k = iso(c), n = Math.min(3, per[k] || 0);
        cells += `<div class="dy${pick === k ? ' on' : ''}" data-n="${n}" data-d="${k}" title="${de(c)}: ${per[k] || 0}"></div>`;
      }
      cols += `<div class="wk">${cells}</div>`;
    }
    let h = `<div class="tkhm">${cols}</div><div class="muted small">${T.hist}</div>`;
    const shown = sortList(pick ? done.filter(t => String(t.completed_at).slice(0, 10) === pick) : done, 'done', desc);
    h += '<div class="tkdl">';
    if (!shown.length) h += `<p class="muted">${T.histNone}</p>`;
    let last = null;
    shown.forEach(t => {
      const k = String(t.completed_at).slice(0, 10);
      if (k !== last) { last = k; h += `<div class="dd">${de(parse(k))}</div>`; }
      h += `<div class="ri" data-key="${esc(key(t))}"><span class="muted tnum">${String(t.completed_at).slice(11, 16)}</span><span class="sw" style="--c:${esc(t.color)}"></span>${esc(t.summary)}
        ${t.due ? `<span class="tchip">${ic('clock', 'xs')}${T.wasdue} ${deS(parse(t.due))}</span>` : ''}</div>`;
    });
    return h + '</div>';
  }

  const VIEWS = { groups: vGroups, table: vTable, board: vBoard, timeline: vTimeline, deck: vDeck, history: vHistory };
  /* the sentinel sits at the end of the finished tasks; the observer reaches far enough ahead
     (a screenful and a half) that the next page is there before the sentinel is on screen */
  const SENTINEL = () => DONE.length < DONE_TOTAL && view !== 'deck' ? `<div class="tksent" id="tk-sent">${esc(T.loading)}</div>` : '';
  let obs = null;
  /* the page scrolls inside <main>, not the window: an observer left on the viewport sees the
     sentinel only once it is clipped in, which is too late to be a prefetch */
  function scrollParent(el) {
    for (let p = el.parentElement; p; p = p.parentElement) {
      const o = getComputedStyle(p).overflowY;
      if ((o === 'auto' || o === 'scroll') && p.scrollHeight > p.clientHeight) return p;
    }
    return null;
  }
  function watchSentinel() {
    if (obs) obs.disconnect();
    const el = $('#tk-sent');
    if (!el) return;
    obs = new IntersectionObserver(async es => { if (es.some(e => e.isIntersecting) && !loading) { if (await loadMore()) renderIdle(); } },
      { root: scrollParent(el), rootMargin: '1200px 0px' });
    obs.observe(el);
  }

  // a redraw would take the cursor out of a cell being typed in: wait until the focus leaves the view
  function renderIdle() {
    const a = document.activeElement;
    if (a && main.contains(a) && a.matches('input, select, [contenteditable]')) { main.addEventListener('focusout', () => setTimeout(renderIdle, 0), { once: true }); return; }
    render();
  }
  function render() {
    if (!PREFIX_SAFE.includes(sort) && DONE.length < DONE_TOTAL) {
      main.innerHTML = `<div class="tksent">${esc(T.loading)}</div>`;
      ensureAll().then(render);
      return;
    }
    // the table scrolls inside its own box (sticky head and new row): keep its place across a redraw
    const sw = $('.sheetwrap', main), top = sw ? sw.scrollTop : 0;
    main.classList.toggle('view-table', view === 'table');
    main.innerHTML = (VIEWS[view] || vGroups)() + (view === 'table' ? '' : SENTINEL());
    const sw2 = $('.sheetwrap', main); if (sw2) sw2.scrollTop = top;
    watchSentinel();
    syncSort();
    alignEd();
    $$('[data-sk]', main).forEach(th => th.onclick = () => { if (sort === th.dataset.sk) desc = !desc; else { sort = th.dataset.sk; desc = false; } localStorage.tkSort = sort; localStorage.tkDesc = desc ? '1' : '0'; render(); });
    $$('[data-d]', main).forEach(el => el.onclick = async () => { pick = pick === el.dataset.d ? null : el.dataset.d; if (pick) await ensureUntil(pick); render(); });
    $$('.tkkc[draggable]', main).forEach(c => c.ondragstart = e => e.dataTransfer.setData('text/plain', c.dataset.key));
    $$('.tkcol', main).forEach(col => {
      col.ondragover = e => e.preventDefault();
      col.ondrop = async e => { e.preventDefault(); await setStatus(e.dataTransfer.getData('text/plain'), col.dataset.st); };
    });
    const deck = $('.tkdeck', main);
    if (deck) deck.onkeydown = e => {
      const list = sortView(TASKS);
      if (e.key === 'j' || e.key === 'ArrowDown') { deckGo(Math.min(list.length - 1, deckI + 1)); if (deckI > list.length - 6) loadMore().then(d => d && render()); render(); $('.tkdeck', main).focus(); }
      if (e.key === 'k' || e.key === 'ArrowUp') { deckGo(Math.max(0, deckI - 1)); render(); $('.tkdeck', main).focus(); }
      if (e.key === ' ') { e.preventDefault(); const t = list[deckI]; if (t) toggle(key(t)); }
      if (e.key === 'e') { e.preventDefault(); const t = list[deckI]; if (t) open(key(t)); }
    };
    markAct();
    if (deck && view === 'deck' && !document.activeElement.closest('input, select, textarea')) deck.focus();
  }

  main.addEventListener('click', e => {
    const tog = e.target.closest('[data-tog]');
    if (tog) { e.stopPropagation(); toggle(tog.dataset.tog); return; }
    if (e.target.closest('[data-next]')) { deckGo(deckI + 1); render(); return; }
    const holder = e.target.closest('[data-key]');
    if (!holder) return;
    // the table is its own editor; a card cell opens that one card of the editor
    if (view === 'table') { const cc = e.target.closest('.cardcell'); if (cc) openCard(holder.dataset.key, cc.dataset.card); else { act = holder.dataset.key; markAct(); } return; } // a row click picks
    open(holder.dataset.key);
  });

  // ---- writing ----
  async function put(k, body) { return practical.tell(await practical.send('/api/tasks/' + k, 'PUT', body)).ok; }
  // one click or key press is easily an accident: finishing a task asks first (reopening does not)
  async function toggle(k) {
    const t = TASKS.find(x => key(x) === k); if (!t) return;
    if (!isDone(t) && !await askYesNo(T.askDone.replace('%t', t.summary), T.yes, T.no)) return;
    if (await put(k, { principal: t.principal, cal_id: t.cal_id, summary: t.summary, completed: !isDone(t) })) { if (view === 'table') refresh(k); else reload(); }
  }
  async function setStatus(k, st) {
    const t = TASKS.find(x => key(x) === k); if (!t || t.status === st) return;
    const got = practical.tell(await practical.req('/api/tasks/' + k)); if (!got.ok) return;
    const d = got.data;
    d.status = st;
    if (st !== 'COMPLETED') { d.completed_at = null; if (d.percent === 100) d.percent = 0; } else d.percent = 100;
    if (await put(k, d)) reload();
  }
  /* after a write: the page as a navigation to url() would show it, without the navigation. The server's
     render of that address brings the data, the counter and the finished total; the parts of the page this
     does not carry over (filter, columns, rules, presets, snaps, calendars) must be unchanged, else it
     navigates after all. Then the page starts again as after a load (nothing picked, first deck card). */
  const fixedOf = d => ['#calfilter', '#tk-colpop', '#edform select[name=calendar]'].map(x => (d.querySelector(x) || {}).innerHTML).concat(['category-rules-data', 'alarm-presets-data', 'alarm-snaps-data'].map(x => (d.getElementById(x) || {}).textContent)).join('\u0001');
  const FIXED = fixedOf(document);
  let reloads = 0;
  async function reload(hash = '') {
    const u = url(), n = ++reloads;
    let doc = null, data = null;
    const r = await practical.req(u, { headers: { Accept: 'text/html' }, as: 'text', wait: 300 });
    if (n !== reloads) return;
    if (!r.ok) { practical.tell(r); return; } // the page stays as it is; a session that ended goes to the login (practical.req)
    try { doc = new DOMParser().parseFromString(r.data, 'text/html'); data = JSON.parse(doc.getElementById('tasks-data').textContent); } catch (_) { doc = null; }
    if (!doc || !data || fixedOf(doc) !== FIXED) { location.href = u + hash; return; }
    $('#tasks-data').textContent = doc.getElementById('tasks-data').textContent;
    $('#task-count').textContent = doc.getElementById('task-count').textContent;
    root.dataset.doneTotal = doc.getElementById('taskroot').dataset.doneTotal;
    setData(data); loading = null;
    if (location.href !== u) history.pushState(null, '', u);
    deckI = 0; pick = null; act = cur = null; held = undefined; wantNew = false;
    start();
  }
  // ---- calendar filter: the calendar page's hidden set (localStorage.calHidden), sent as ?cals= like there ----
  const cf = $('#calfilter');
  let refetch = false; // the filter kept in this browser differs from the page's: the page's data is not the one to show
  if (cf) {
    const hid = new Set(JSON.parse(localStorage.calHidden || '[]')), boxes = $$('input[data-cal]', cf);
    const param = () => { const on = boxes.filter(i => i.checked).map(i => i.dataset.cal); return on.length === boxes.length ? '' : on.join(',') || '-'; };
    const apply = () => { boxes.forEach(i => { if (i.checked) hid.delete(i.dataset.cal); else hid.add(i.dataset.cal); }); localStorage.calHidden = JSON.stringify([...hid]); root.dataset.cals = param(); practical.keepCals('tasks', root.dataset.cals); reload(); };
    boxes.forEach(i => { i.checked = !hid.has(i.dataset.cal); i.addEventListener('change', apply); });
    practical.keepCals('tasks', param());
    $$('[data-filter]', cf).forEach(a => a.addEventListener('click', e => { e.preventDefault(); boxes.forEach(i => { i.checked = a.dataset.filter === 'all' || (a.dataset.filter === 'mine' && !i.parentElement.querySelector('small')); }); apply(); }));
    // a filter set on the calendar page applies here too; a rule link (?rule=) keeps its page
    if (param() !== (root.dataset.cals || '') && !new URL(location.href).searchParams.has('rule')) { root.dataset.cals = param(); refetch = true; }
  }
  function url(v) { const u = new URL('/tasks', location.origin); u.searchParams.set('view', v || view); u.searchParams.set('back', root.dataset.back); if (root.dataset.cals) u.searchParams.set('cals', root.dataset.cals); return u.toString(); }

  // ---- the editor, placed the way the contacts page places its own: the groups view on a wide
  // window is list | editor and the editor is always there — with nothing picked it is the form for
  // a new task. Every other view (and a narrow window) opens it as the calendar's dialog. ----
  const MIN_DOCK = 1100;
  let held;       // what the editor holds: a task key, null = the new-task form, undefined = nothing
  let wantNew = false; // a new task asked for through + (outside the docked pane it is a dialog)
  const docked = () => view === 'groups' && innerWidth >= MIN_DOCK;
  function load(k, quiet) { held = k; window.rusticalEd.open(k, undefined, quiet); }
  /* the round + (bottom left) adds a task where nothing else can: table, board, timeline and cards,
     and the groups view when its editor is not docked; the history shows finished work only */
  const FAB_VIEWS = ['table', 'board', 'timeline', 'deck'];
  function fit() {
    const dock = docked();
    const fab = $('#fab'), withFab = FAB_VIEWS.includes(view) || (view === 'groups' && !dock);
    fab.hidden = !withFab; fab.classList.toggle('fab-add', withFab);
    root.classList.toggle('dock', dock);
    ed.classList.toggle('docked', dock);
    if (dock) { ed.hidden = false; if (held === undefined) load(null, true); }
    else ed.hidden = !(cur || wantNew);
    alignEd();
  }
  addEventListener('resize', fit);
  // the docked pane starts level with the first task (below the first group heading)
  function alignEd() {
    const c = docked() && $('.tkcard', main);
    ed.style.setProperty('--ed-off', c ? Math.round(c.getBoundingClientRect().top - main.getBoundingClientRect().top) + 'px' : '0px');
  }
  function open(k) {
    cur = act = k;
    render();
    load(k);
    fit();
  }
  function openCard(k, card) { cur = act = k; held = k; markAct(); window.rusticalEd.open(k, undefined, false, card); fit(); }
  function newTask() {
    cur = null; wantNew = true; render(); load(null); fit();
  }
  $('#fab').onclick = newTask;

  // what the editor module asks of the page it runs on
  window.rusticalHost = {
    date: () => iso(TODAY),
    cals: () => root.dataset.cals || '',
    // after a save the docked pane shows the saved task again (the contacts page keeps it in the hash)
    reload: (push, saved) => {
      if (saved && view === 'table' && find(saved)) { refresh(saved); return; } // a card saved from a cell: redraw that row only
      if (saved && docked()) sessionStorage.tkSel = saved;
      reload();
    },
    // closing the docked pane falls back to the new-task form; a dialog just goes away
    closed: () => { if (docked()) act = null; cur = null; wantNew = false; held = undefined; render(); fit(); }, // the docked pane falls back to the new-task form: nothing is picked
  };

  $('#tk-back').onchange = e => { document.cookie = `tk_back=${e.target.value}; path=/tasks; max-age=31536000; SameSite=Lax`; const u = new URL(url()); u.searchParams.set('back', e.target.value); location.href = u.toString(); }; // the cookie: the page reached from the navigation keeps the choice (routes/tasks.rs)
  // ---- the picked task: a search hit, a click or a view switch keeps it marked and in view where the view shows it ----
  function markAct() { $$('.sel[data-key]', main).forEach(el => el.classList.remove('sel')); if (act) $$(`[data-key="${CSS.escape(act)}"]`, main).forEach(el => el.classList.add('sel')); }
  function deckTo() { if (!act) return; const i = sortView(TASKS).findIndex(t => key(t) === act); if (i >= 0) deckI = i; }
  function showAct() { // false: this view has no place for it (a history day, a finished task on the deck …)
    if (!act) return false;
    if (docked() && held !== act && find(act)) open(act); // the docked pane shows the picked task
    const el = $(`[data-key="${CSS.escape(act)}"]`, main); if (!el) return false;
    el.scrollIntoView({ block: 'center' }); return true;
  }
  function setView(v) {
    view = v;
    segPress($(`#tk-views button[data-tkview="${v}"]`));
    $('#tk-colpick').hidden = view !== 'table';
    history.replaceState(null, '', url());
    // the editor does not travel between views (the dialog would pop up in the next one); the pick does
    cur = null; wantNew = false; held = undefined;
    deckTo(); render(); fit();
  }
  $$('#tk-views button').forEach(b => b.onclick = () => { setView(b.dataset.tkview); showAct(); });
  async function reveal(k) {
    act = k;
    if (!find(k)) await ensureAll();
    if (!find(k) && root.dataset.back !== 'all') { const u = new URL(url()); u.searchParams.set('back', 'all'); location.href = u + '#' + encodeURIComponent(k); return; } // finished before the lookback: widen it, same view
    if (!find(k)) { cur = k; load(k); fit(); return; } // hidden by the calendar filter: the editor still shows it
    deckTo(); render();
    if (!showAct()) { setView('groups'); showAct(); } // this view cannot show it: the list can
  }
  window.rusticalTasks = { reveal };
  addEventListener('popstate', () => location.reload());

  // the start, also after an in-place reload: back from a save in the docked pane, show what was saved
  function start() {
    const sel = sessionStorage.tkSel; delete sessionStorage.tkSel;
    if (sel && docked() && find(sel)) open(sel); else { fit(); render(); }
  }
  // a link to one task (#key, e.g. a search hit from another page)
  const hk = decodeURIComponent(location.hash.slice(1)), hash = location.hash;
  const linked = () => { if (hk) { history.replaceState(null, '', url()); reveal(hk); } };
  if (refetch) reload(hash).then(linked); else { start(); linked(); }
})();
