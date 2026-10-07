// js/contacts.js — the contacts page: list/table, column picker, sorting, the contact editor; window.rusticalContacts
(function () {
  const wrap = document.getElementById('contacts');
  if (!wrap) return;
  const { $, $$ } = practical;
  const lang = document.body.dataset.lang || 'de';
  const T = { ...trs('js-ct-', ['del', 'err', 'phone', 'email', 'create', 'save', 'reset', 'cancel', 'delete']), kinds: ['home', 'work', 'cell', 'other'] };
  /* one view per page (perf3 #8: the table was 417 KB of the 764 KB page, and every row was wired at load): the
     server renders the one the page opens in (?view=, else the `ctview` cookie set here, else the list); the other
     comes from /contacts/part (the page's address book filter applies) on the first switch to it, fetched on the
     intent to switch (practical.intent: hover, focus, press) and put in the page while hidden, so the click itself only
     switches (a hover-fetched table parsed on the click took 270 ms instead of 172 at 4x CPU, measured). `has[v]`: the
     view's entries are in the page. */
  let activeKey = null;
  const panels = $('.panels', wrap), tbody = $('.sheet tbody', wrap), table0 = wrap.classList.contains('view-table');
  const has = { list: !table0, table: table0 }, loading = {};
  const partUrl = v => { const u = new URL('/contacts/part', location.href), abs = new URLSearchParams(location.search).get('abs'); u.searchParams.set('view', v); if (abs) u.searchParams.set('abs', abs); return u.href; };
  // quiet (the intent): only what the prefetch brought (practical.prefetch keeps to its rules: an open live stream, no Save-Data), unseen
  function load(v, quiet) {
    if (has[v]) return Promise.resolve(true);
    let l = loading[v];
    if (!l) {
      l = loading[v] = { quiet, p: (async () => {
        const u = partUrl(v); let html = await practical.prefetched(u);
        if (html !== null && !/^(<tr |<section |$)/.test(html)) html = null; // a prefetch sent on to the login page: ask again (req tells why)
        if (html === null && !quiet) { const r = practical.tell(await practical.req(u, { as: 'text', wait: 300 })); if (r.ok) html = r.data; }
        if (html === null) return false;
        if (!has[v]) arrive(v, html);
        return true;
      })().finally(() => { loading[v] = null; }) };
    }
    return quiet || !l.quiet ? l.p : l.p.then(ok => ok || load(v)); // a click during the intent's load: that one, else its own
  }
  // the view switch; the picked contact (activeKey) stays picked and in view. The choice is kept at once (this
  // browser: localStorage, and the cookie the server reads), the view shows once its entries are there.
  let want = null;
  async function setView(b, remember = true) {
    const v = b.dataset.ctview; want = v;
    if (remember) { localStorage.ctview = v; document.cookie = `ctview=${v}; path=/contacts; max-age=31536000; SameSite=Lax`; }
    if (!await load(v) || want !== v) return;
    const table = v === 'table'; wrap.classList.toggle('view-table', table); $$('#colpop [data-for]').forEach(g => g.hidden = g.dataset.for !== v); segPress(b); if (activeKey) reveal(activeKey);
    railTick();
  }
  $$('[data-ctview]').forEach(b => { b.onclick = () => setView(b); practical.intent(b, () => { const v = b.dataset.ctview; if (has[v]) return []; setTimeout(() => load(v, true)); return [partUrl(v)]; }); }); // the load takes the prefetch made right after this
  // columns
  function applyCols() { const on = $$('#colpop input[data-col]').filter(i => i.checked).map(i => i.dataset.col); $$('.sheet [data-c]').forEach(el => el.classList.toggle('off', !on.includes(el.dataset.c))); return on; }
  $$('#colpop input[data-col]').forEach(i => i.addEventListener('change', () => { const on = applyCols(); practical.send('/settings/columns', 'POST', on).then(practical.tell); }));
  applyCols();
  // list view: the same picker toggles the lines of every entry
  function applyListFields() { const on = $$('#colpop input[data-lf]').filter(i => i.checked).map(i => i.dataset.lf); $$('.panels [data-lf]').forEach(el => el.classList.toggle('off', !on.includes(el.dataset.lf))); return on; }
  $$('#colpop input[data-lf]').forEach(i => i.addEventListener('change', () => { const on = applyListFields(); practical.send('/settings/list-fields', 'POST', on).then(practical.tell); }));
  applyListFields(); $$('#colpop [data-for]').forEach(g => g.hidden = g.dataset.for !== (wrap.classList.contains('view-table') ? 'table' : 'list'));
  // filter by addressbook / category
  const fb = $('#abfilter');
  function applyFilter() {
    const abs = $$('input[data-ab]', fb).filter(i => !i.checked).map(i => i.dataset.ab); const cats = $$('input[data-cat]', fb).filter(i => !i.checked).map(i => i.dataset.cat);
    const off = el => abs.includes(el.dataset.ab) || cats.includes(el.dataset.cat);
    $$('[data-key][data-ab]', wrap).forEach(el => el.classList.toggle('hide', off(el)));
    $$('.lpanel', wrap).forEach(p => p.classList.toggle('hide', !$$('li:not(.hide)', p).length));
    // a letter of the rail shows while one of its contacts does (the list may not be in the page)
    const shown = new Set(recs.filter(r => !off(r.li || r.tr)).map(r => r.g));
    $$('.ct-rail [data-jump]', wrap).forEach(b => b.classList.toggle('hide', !shown.has(b.dataset.jump)));
    railTick(); // with the frame: a timer before it laid the page out once more (measured: +47 ms at 4x CPU on a switch)
  }
  if (fb) $$('input', fb).forEach(i => i.addEventListener('change', applyFilter));
  if (fb) $$('[data-filter]', fb).forEach(a => a.addEventListener('click', e => { e.preventDefault(); $$('input[data-ab]', fb).forEach(i => { i.checked = a.dataset.filter === 'all' || (a.dataset.filter === 'mine' && !i.parentElement.querySelector('small')); }); applyFilter(); })); // as the calendar filter
  // quick-jump rail: scroll the letter's panel (list) or first row (table) to the top of its scroller
  const scrollerOf = el => { for (let p = el.parentElement; p; p = p.parentElement) { const o = getComputedStyle(p).overflowY; if (o === 'auto' || o === 'scroll') return p; } return document.querySelector('.main'); };
  $('.ct-rail', wrap).addEventListener('click', e => { // delegated: the sort rebuilds the buttons
    const b = e.target.closest('[data-jump]'); if (!b) return;
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
  /* sort field + direction, the same box as on the tasks page. The list regroups (initial letter, or
     birth month), the table reorders, the rail follows; entries without the value stay at the end. */
  const SL = trs('js-ctsort-', ['given', 'family', 'bday', 'email', 'none']), TS = trs('js-tk-', ['sort', 'asc', 'desc']);
  const FIELDS = ['given', 'family', 'bday', 'email'];
  let sort = FIELDS.includes(localStorage.ctSort) ? localStorage.ctSort : 'given', desc = localStorage.ctDesc === '1';
  const hesc = practical.esc;
  const coll = new Intl.Collator(lang, { sensitivity: 'base', numeric: true });
  const MONTHS = Array.from({ length: 12 }, (_, i) => dfmt(new Date(2000, i, 1), { month: 'long' }));
  const initial = v => { const c = v.normalize('NFD').charAt(0).toUpperCase(); return /\p{L}/u.test(c) ? c : '#'; };
  // a contact's sort keys, from its list entry or — the list not in the page — its table row (contacts_rows.html `keys`)
  const recs = $$(table0 ? 'tr[data-key]' : '.panels li[data-key]', wrap).map(el => {
    const d = el.dataset, name = d.name.trim(), md = d.bday.match(/(\d{2})-(\d{2})$/);
    return { key: d.key, li: table0 ? null : el, tr: table0 ? el : null, g: null, given: `${d.given || name} ${d.family}`.trim(), family: `${d.family || name} ${d.given}`.trim(), bday: md ? `${md[1]}-${md[2]}` : '', email: d.email.trim().toLowerCase() };
  });
  // the entries of the view that came later to their contacts: the first of each key (one pass, not one search per contact)
  const attach = () => {
    const of = new Map(); $$(table0 ? '.panels li[data-key]' : 'tr[data-key]', wrap).forEach(el => { if (!of.has(el.dataset.key)) of.set(el.dataset.key, el); });
    recs.forEach(r => { r[table0 ? 'li' : 'tr'] = of.get(r.key) || null; });
  };
  const groupOf = r => !r[sort] ? '-' : sort === 'bday' ? r.bday.slice(0, 2) : initial(r[sort]);
  const label = g => g === '-' ? SL.none : sort === 'bday' ? MONTHS[+g - 1] : g;
  const short = g => g === '-' ? '–' : sort === 'bday' ? MONTHS[+g - 1].slice(0, 3) : g;
  // the groups of the last sort, [[group, contacts]]: a view that comes later is laid out in them (place)
  let order = [];
  function resort() {
    const valued = recs.filter(r => r[sort]), none = recs.filter(r => !r[sort]);
    valued.sort((a, b) => coll.compare(a[sort], b[sort]) || coll.compare(a.given, b.given));
    if (desc) valued.reverse();
    none.sort((a, b) => coll.compare(a.given, b.given));
    const groups = new Map();
    for (const r of valued.concat(none)) { const g = groupOf(r); if (!groups.has(g)) groups.set(g, []); groups.get(g).push(r); }
    order = [...groups];
    place(has.list, has.table);
    let rail = '';
    for (const [g] of order) rail += `<button type="button" data-jump="${hesc(g)}" title="${hesc(label(g))}">${hesc(short(g))}</button>`;
    $('.ct-rail', wrap).innerHTML = rail;
    sortSel.value = sort;
    sortDir.innerHTML = `<svg class="ico s" aria-hidden="true"><use href="${practical.sprite}#i-${desc ? 'sort-desc' : 'sort-asc'}"/></svg>`;
    sortDir.title = desc ? TS.desc : TS.asc; sortDir.setAttribute('aria-label', sortDir.title);
    applyFilter();
  }
  // the entries of the list (letter panels) and/or the table (rows, their letter) in the order of the last sort
  function place(list, table) {
    const ph = $('.placeholder', panels);
    if (list) $$('.lpanel', panels).forEach(p => p.remove());
    for (const [g, rs] of order) {
      let sec = null;
      if (list) { sec = document.createElement('section'); sec.className = 'lpanel'; sec.dataset.letter = g; sec.innerHTML = `<h2>${hesc(label(g))}</h2><ul class="list"></ul>`; }
      const ul = sec && sec.lastElementChild;
      rs.forEach(r => { r.g = g; if (ul && r.li) ul.appendChild(r.li); if (table && r.tr) { r.tr.dataset.letter = g; tbody.appendChild(r.tr); } });
      if (sec) panels.insertBefore(sec, ph);
    }
  }
  const sortBox = $('#ct-sortbox');
  sortBox.innerHTML = `<span class="sel" title="${hesc(TS.sort)}"><svg class="ico s" aria-hidden="true"><use href="${practical.sprite}#i-swap"/></svg><select id="ct-sort" aria-label="${hesc(TS.sort)}">${FIELDS.map(v => `<option value="${v}">${hesc(SL[v])}</option>`).join('')}</select></span><button class="btn icon-btn" id="ct-dir" type="button"></button>`;
  const sortSel = $('#ct-sort'), sortDir = $('#ct-dir');
  const resorted = () => { resort(); if (activeKey) reveal(activeKey); };
  sortSel.onchange = () => { sort = sortSel.value; localStorage.ctSort = sort; resorted(); };
  sortDir.onclick = () => { desc = !desc; localStorage.ctDesc = desc ? '1' : '0'; resorted(); };
  resort();
  railMark();
  // ---- contact editor: cards, view/edit mode (lock), per-row actions ----
  const det = $('#detail'), form = $('#ctform'), f = form.elements, D = form.dataset;
  const KEY = c => `${c.principal}/${c.ab_id}/${c.object_id}`;
  let current = null, baseline = '', fnAuto = true;
  const wide = () => matchMedia('(min-width: 1100px)').matches;
  // partial dates: "YYYY-MM-DD" | "--MM-DD" <-> three inputs
  const setDate = (p, iso) => { const m = (iso || '').match(/^(?:(\d{4})|-)-(\d{2})-(\d{2})$/); $('[data-p=d]', p).value = m ? m[3] : ''; $('[data-p=m]', p).value = m ? m[2] : ''; $('[data-p=y]', p).value = m && m[1] ? m[1] : ''; };
  const getDate = p => { const d = $('[data-p=d]', p).value.trim(), m = $('[data-p=m]', p).value.trim(), y = $('[data-p=y]', p).value.trim(); if (!d || !m) return ''; const dd = d.padStart(2, '0'), mm = m.padStart(2, '0'); return (y ? y.padStart(4, '0') : '-') + `-${mm}-${dd}`; };
  const dateInfo = p => { const d = +$('[data-p=d]', p).value, m = +$('[data-p=m]', p).value, y = +$('[data-p=y]', p).value; const info = $('.dateinfo', p); if (!info) return; if (!d || !m) { info.textContent = ''; return; } const now = new Date(), t0 = new Date(now.getFullYear(), now.getMonth(), now.getDate()); const on = yr => new Date(yr, m - 1, Math.min(d, new Date(yr, m, 0).getDate())); /* 29 Feb → 28 Feb in a common year, as contacts::yearly_in */ let next = on(now.getFullYear()); if (next < t0) next = on(now.getFullYear() + 1); const days = Math.round((next - t0) / 864e5); info.textContent = (days === 0 ? D.tToday : plural(D.tIn, days).replace('{n}', days)) + (y ? ' · ' + D.tTurns.replace('{n}', next.getFullYear() - y) : ''); };
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
  function updateButtons() { const changed = serialize() !== baseline, busy = W.busy; $('#ct-save').disabled = !changed || busy; $('#ct-reset').disabled = (!changed && form.classList.contains('new')) || busy; $('#ct-delete').disabled = form.classList.contains('mode-view') || form.classList.contains('new') || busy; }
  const W = practical.writer(form, { buttons: () => [$('#ct-save'), $('#ct-delete'), $('#ct-reset')], error: $('#ct-error'), idle: () => updateButtons(),
    conflict: { overwrite: async () => { const r = await practical.req('/api/contacts/' + f.key.value); if (!r.ok) return r; f.etag.value = r.data.etag; return put(); }, reload: () => open(f.key.value) } });
  const dirty = () => !det.hidden && form.classList.contains('mode-edit') && serialize() !== baseline;
  practical.guard(dirty);
  practical.keep(() => { if (dirty()) practical.stash('ct', { isNew: form.classList.contains('new'), d: { ...collect(), object_id: f.key.value.split('/')[2] || '', uid: current ? current.uid : '', etag: f.etag.value } }); });
  const initials = n => n.split(/\s+/).map(w => w[0]).slice(0, 2).join('').toUpperCase();
  function show(c) {
    current = c; form.reset(); form.classList.remove('solo'); wrap.classList.remove('solo'); f.full_name.readOnly = false; $('#ct-error').textContent = '';
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
    activeKey = c ? KEY(c) : null; markRow(activeKey);
    $$('.list li.on').forEach(l => l.classList.remove('on')); if (c) { const li = $(`.list li[data-key="${f.key.value}"]`); if (li) li.classList.add('on'); }
  }
  // one load per contact at a time: a switch to the list reveals the picked contact while its own open is on the way
  let opening = null;
  function open(key) {
    if (opening && opening.key === key) return opening.p;
    const p = (async () => { const r = practical.tell(await practical.req('/api/contacts/' + key)); if (r.ok) show(r.data); return r.ok; })();
    opening = { key, p }; p.finally(() => { if (opening && opening.p === p) opening = null; });
    return p;
  }
  // the entries of both views: one listener per view, not one per entry (entries come with the sort and the first switch)
  panels.addEventListener('click', e => { const li = e.target.closest('.list li[data-key]'); if (li) open(li.dataset.key); });
  // the keyboard opens one too: a row is a Tab stop (contacts_list.html tabindex), Enter or Space on it is the click
  panels.addEventListener('keydown', e => { if ((e.key === 'Enter' || e.key === ' ') && e.target.matches && e.target.matches('.list li[data-key]')) { e.preventDefault(); open(e.target.dataset.key); } });
  // the table picks a contact with a click on its row; ↗ opens it in the list, a card cell opens that card (openSolo)
  tbody.addEventListener('click', e => {
    const tr = e.target.closest('tr[data-key]'); if (!tr) return;
    const b = e.target.closest('[data-open]'), td = e.target.closest('td.cardcell');
    if (b) { setView($('[data-ctview=list]')); open(b.dataset.open); } else if (td) openSolo(tr.dataset.key, td.dataset.card);
    activeKey = tr.dataset.key; markRow(activeKey);
  });
  const preList = e => { if (!has.list && e.target.closest && e.target.closest('[data-open]')) practical.prefetch(partUrl('list')); }; // ↗ needs the list
  tbody.addEventListener('pointerover', preList); tbody.addEventListener('focusin', preList);
  // the other view's entries, as its page renders them (contacts_list.html, contacts_rows.html), sorted and filtered as this one
  function arrive(v, html) {
    has[v] = true;
    (v === 'list' ? panels : tbody).insertAdjacentHTML('afterbegin', html);
    attach();
    if (v === 'list') applyListFields(); else applyCols();
    place(v === 'list', v === 'table'); applyFilter();
    if (v === 'table') markRow(activeKey); else if (current) { const li = $(`.list li[data-key="${CSS.escape(KEY(current))}"]`, wrap); if (li) li.classList.add('on'); }
  }
  // editing: dirty tracking, display name follows given + family, dates re-evaluate
  form.addEventListener('input', e => { if (e.target === f.given || e.target === f.family) { if (fnAuto) f.full_name.value = [f.given.value, f.family.value].filter(Boolean).join(' '); } else if (e.target === f.full_name) fnAuto = !f.full_name.value; const p = e.target.closest('.pdate'); if (p) dateInfo(p); updateButtons(); });
  form.addEventListener('change', updateButtons);
  form.addEventListener('click', async e => {
    const b = e.target.closest('button'); if (!b || !form.contains(b)) return;
    if (b.dataset.add) { const row = addRow(b.dataset.add, b.dataset.add === 'phones' ? { kind: 'cell' } : b.dataset.add === 'emails' || b.dataset.add === 'addresses' ? { kind: 'home' } : null); const i = $('input:not([type=hidden])', row); if (i) i.focus(); updateButtons(); return; }
    if (b.hasAttribute('data-rm')) { b.closest('.row').remove(); updateButtons(); return; }
    if (b.classList.contains('pref')) { const list = b.closest('[data-list]'); const on = b.getAttribute('aria-pressed') === 'true'; $$('.pref', list).forEach(p => p.setAttribute('aria-pressed', p === b && !on ? 'true' : 'false')); updateButtons(); return; }
    if (b.hasAttribute('data-more')) { $('.morenames', form).hidden = false; b.hidden = true; return; }
    if (b.dataset.cardAdd) { const c = $(`.card[data-card="${b.dataset.cardAdd}"]`, form); c.dataset.added = '1'; c.hidden = false; b.disabled = true; b.closest('.addmenu').classList.remove('open'); if (b.dataset.cardAdd === 'email') addRow('emails', { kind: 'home' }); if (b.dataset.cardAdd === 'adr') addRow('addresses', { kind: 'home' }); const i = $('input:not([type=hidden]), textarea', c); if (i && !i.closest('template')) i.focus(); return; }
    if (b.hasAttribute('data-card-rm')) { if (!await ask(D.tRmcard)) return; const c = b.closest('.card'); $$('input:not([type=hidden]), textarea', c).forEach(i => { if (!i.closest('template')) i.value = ''; }); $$('.rows .row', c).forEach(x => x.remove()); $$('.chip', c).forEach(x => x.remove()); delete c.dataset.added; c.hidden = true; cards(); updateButtons(); return; }
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
      case 'copy': practical.copy(row.classList.contains('adr') ? adrText() : val).then(ok => { if (!ok) return; b.title = D.tCopied; b.style.color = 'var(--accent)'; setTimeout(() => b.style.color = '', 800); }); break;
      case 'im': { const svc = $('select.typ', row).value; const u = { Signal: 'https://signal.me/#p/' + digits, WhatsApp: 'https://wa.me/' + digits.replace(/^\+/, ''), Telegram: 'https://t.me/' + val.replace(/^@/, ''), Matrix: 'https://matrix.to/#/' + val, XMPP: 'xmpp:' + val, Threema: 'https://threema.id/' + val }[svc]; if (u) open_(u); break; }
      case 'goto': load('list').then(() => { const li = $$('.list li[data-key]').find(l => (l.dataset.name || '').toLowerCase() === val.toLowerCase()); if (li) { open(li.dataset.key); const sc = $('.main'); sc.scrollTop += li.getBoundingClientRect().top - sc.getBoundingClientRect().top - 120; } else toast(D.tMissing, true); }); break; // the names are the list's
    }
  }
  $('#ct-lock').onclick = () => mode('edit');
  $('#ct-reset').onclick = () => show(current);
  $('#ct-cancel').onclick = () => { if (wrap.classList.contains('dlg')) dlgClose(); else if (wide()) show(null); else { det.classList.remove('on'); det.hidden = true; } };
  $('#ct-vcf').onclick = () => { if (f.key.value) location.href = '/api/contacts/' + f.key.value + '/vcf'; };
  // reveal a contact (a search hit, a #key link): the table marks and scrolls to its row, the list opens it and scrolls its entry into view
  function markRow(key) { let hit = null; $$('.sheet tr[data-key]', wrap).forEach(r => { const on = r.dataset.key === key; r.classList.toggle('on', on); if (on) hit = r; }); return hit; }
  async function reveal(key) {
    activeKey = key;
    if (wrap.classList.contains('view-table')) { const tr = markRow(key); if (tr) tr.scrollIntoView({ block: 'center' }); return; }
    if (!current || KEY(current) !== key) await open(key); const li = $(`.list li[data-key="${CSS.escape(key)}"]`, wrap); if (li) li.scrollIntoView({ block: 'center' });
  }
  window.rusticalContacts = { reveal };
  // the work an ended session left unsaved (practical.keep above) comes back before anything else is opened
  const kept = practical.unstash('ct');
  if (kept) { show(kept.d); if (kept.isNew) { f.key.value = f.etag.value = ''; form.classList.add('new'); const b = $('#ct-save'); b.title = D.tCreate; b.setAttribute('aria-label', b.title); } mode('edit'); baseline = ''; updateButtons(); toast(tr('js-req-restored')); }
  else if (location.hash.length > 1) {
    // a link to one contact opens the list — also where the remembered view is the table (?view=table shows it there)
    const key = decodeURIComponent(location.hash.slice(1));
    if (table0 && !new URLSearchParams(location.search).has('view')) { activeKey = key; setView($('[data-ctview=list]'), false); } else reveal(key);
  } else if (wide()) show(null);
  if (localStorage.ctview === 'table' && !table0 && location.hash.length <= 1) setView($('[data-ctview=table]')); // picked before the cookie existed
  function collect() {
    const [principal, ab_id] = f.ab.value.split('/');
    const g = k => f[k].value.trim();
    return { principal, ab_id, full_name: g('full_name'), given: g('given'), family: g('family'), nickname: g('nickname'), prefix: g('prefix'), additional: g('additional'), suffix: g('suffix'), org: g('org'), department: g('department'), title: g('title'), note: g('note'),
      phones: rowVals('phones'), emails: rowVals('emails'), urls: rowVals('urls'), impp: rowVals('impp'), related: rowVals('related'),
      addresses: rows('addresses').map(r => { const o = { kind: $('select.typ', r).value, pref: $('.pref', r).getAttribute('aria-pressed') === 'true' }; ['street', 'code', 'city', 'country', 'region'].forEach(k => o[k] = $(`[data-w=${k}]`, r).value.trim()); return o; }).filter(a => a.street || a.city || a.code || a.country),
      dates: rows('dates').map(r => ({ label: $('.lbl', r).value.trim(), date: getDate($('.pdate', r)) })).filter(d => d.date),
      birthday: getDate($('[data-date=birthday]', form)), anniversary: getDate($('[data-date=anniversary]', form)), categories: chips() };
  }
  // writes: one at a time, a 409 asks keep mine / load theirs, as in the event editor (practical.writer)
  async function put() {
    const key = f.key.value, body = collect();
    const r = await practical.req(key ? '/api/contacts/' + key : '/api/contacts', { method: key ? 'PUT' : 'POST', json: key ? { ...body, etag: f.etag.value } : body });
    if (r.ok) { baseline = serialize(); location.hash = `${body.principal}/${body.ab_id}/${r.data.id}`; practical.leaving = true; location.reload(); }
    return r;
  }
  form.addEventListener('submit', e => { e.preventDefault(); W.run(put); });
  $('#ct-delete').onclick = async () => { if (W.busy || !await ask(T.del)) return; W.run(async () => { const r = await practical.req('/api/contacts/' + f.key.value, { method: 'DELETE' }); if (r.ok) { location.hash = ''; practical.leaving = true; location.reload(); } return r; }); };
  // table inline edit: fetch → patch one field → PUT
  const before = new WeakMap(), editable = e => e.target.closest && e.target.closest('[contenteditable][data-f]');
  tbody.addEventListener('focusin', e => { const cell = editable(e); if (cell) before.set(cell, cell.textContent); });
  tbody.addEventListener('keydown', e => { const cell = editable(e); if (cell && e.key === 'Enter') { e.preventDefault(); cell.blur(); } });
  tbody.addEventListener('focusout', async e => {
    const cell = editable(e); if (!cell) return;
    const was = before.get(cell) || '', v = cell.textContent.trim(); if (v === was.trim()) return;
    const tr = cell.closest('tr'); const key = tr.dataset.key;
    try {
      const got = await practical.req('/api/contacts/' + key); if (!got.ok) throw got;
      const c = got.data, body = { ...c }; // the contact JSON is a superset of the input: nothing gets dropped
      switch (cell.dataset.f) {
        case 'full_name': { body.full_name = v; const parts = v.split(/\s+/); body.given = parts.slice(0, -1).join(' '); body.family = parts.slice(-1)[0] || ''; break; }
        case 'phone': body.phones = v ? [{ kind: (c.phones[0] || {}).kind || 'cell', value: v }, ...c.phones.slice(1)] : c.phones.slice(1); break;
        case 'email': body.emails = v ? [{ kind: (c.emails[0] || {}).kind || 'home', value: v }, ...c.emails.slice(1)] : c.emails.slice(1); break;
        case 'birthday': { const m = v.match(/^(\d{1,2})\.(\d{1,2})\.(\d{4})?$/); body.birthday = m ? (m[3] ? `${m[3]}-${m[2].padStart(2, '0')}-${m[1].padStart(2, '0')}` : `--${m[2].padStart(2, '0')}-${m[1].padStart(2, '0')}`) : v; break; }
        case 'org': body.org = v; break;
      }
      const r = await practical.send('/api/contacts/' + key, 'PUT', body);
      if (!r.ok) throw r;
      cell.style.outline = '2px solid var(--c2)'; setTimeout(() => cell.style.outline = '', 800);
    } catch (e) { cell.textContent = was; if (e && e.kind) practical.tell(e); else console.error(e); }
  });
  // table view: + opens the editor as a dialog over the table (the list view has it beside the list)
  // the dialog is a modal one for assistive technology too, named by the contact (shell/base.js keeps Tab inside an aria-modal box)
  const dlgRole = on => { if (!on) { ['role', 'aria-modal', 'aria-label'].forEach(a => det.removeAttribute(a)); return; } det.setAttribute('role', 'dialog'); det.setAttribute('aria-modal', 'true'); det.setAttribute('aria-label', f.full_name.value || f.full_name.placeholder); };
  const dlgClose = () => { wrap.classList.remove('dlg', 'solo'); dlgRole(false); form.classList.remove('solo'); f.full_name.readOnly = false; det.classList.remove('on'); if (!wide()) det.hidden = true; };
  $('#ct-fab').onclick = () => { show(null); wrap.classList.add('dlg'); dlgRole(true); f.full_name.focus(); };
  /* a cell the table cannot edit (categories, address, note) opens just that card of the editor,
     already unlocked, as a dialog; saving writes the whole contact as the full editor does */
  async function openSolo(key, card) {
    await open(key);
    const c = $(`.card[data-card="${card}"]`, form); if (!c) return;
    c.dataset.added = '1'; mode('edit');
    $$('.card', form).forEach(x => x.classList.toggle('solo-on', x === c)); c.hidden = false;
    form.classList.add('solo'); wrap.classList.add('dlg', 'solo'); dlgRole(true); f.full_name.readOnly = true;
    // an empty list card (addresses …) starts with one empty row, as a new contact does with its phone
    const add = $('[data-add]', c); if (add && !$('.rows .row', c)) add.click();
    const first = $('input:not([type=hidden]):not([readonly]), textarea', c) || $('select', c); if (first) first.focus();
  }
  addEventListener('keydown', e => { if (e.key === 'Escape' && wrap.classList.contains('dlg')) dlgClose(); });
})();
