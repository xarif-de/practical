/* ===== contacts page ===== */
(function () {
  const wrap = document.getElementById('contacts');
  if (!wrap) return;
  const { $, $$ } = practical, csrf = practical.csrf();
  const lang = document.body.dataset.lang || 'de';
  const T = { ...trs('js-ct-', ['del', 'err', 'phone', 'email', 'create', 'save', 'reset', 'cancel', 'delete']), kinds: ['home', 'work', 'cell', 'other'] };
  // view switch; the picked contact (activeKey) stays picked and in view
  let activeKey = null;
  $$('[data-ctview]').forEach(b => b.onclick = () => { const table = b.dataset.ctview === 'table'; wrap.classList.toggle('view-table', table); $$('#colpop [data-for]').forEach(g => g.hidden = g.dataset.for !== (table ? 'table' : 'list')); segPress(b); localStorage.ctview = b.dataset.ctview; if (activeKey) reveal(activeKey); });
  if (localStorage.ctview === 'table' && !wrap.classList.contains('view-table') && location.hash.length <= 1) $('[data-ctview=table]').click(); // a link to one contact (#key) opens the list
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
    $$('[data-key][data-ab]', wrap).forEach(el => el.classList.toggle('hide', abs.includes(el.dataset.ab) || cats.includes(el.dataset.cat)));
    $$('.lpanel', wrap).forEach(p => p.classList.toggle('hide', !$$('li:not(.hide)', p).length));
    $$('.ct-rail [data-jump]', wrap).forEach(b => { const p = $(`.lpanel[data-letter="${CSS.escape(b.dataset.jump)}"]`, wrap); b.classList.toggle('hide', !p || p.classList.contains('hide')); });
    if (typeof railMark === 'function') setTimeout(railMark, 0);
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
  $$('[data-ctview]').forEach(b => b.addEventListener('click', () => setTimeout(railMark, 0)));
  /* sort field + direction, the same box as on the tasks page. The list regroups (initial letter, or
     birth month), the table reorders, the rail follows; entries without the value stay at the end. */
  const SL = trs('js-ctsort-', ['given', 'family', 'bday', 'email', 'none']), TS = trs('js-tk-', ['sort', 'asc', 'desc']);
  const FIELDS = ['given', 'family', 'bday', 'email'];
  let sort = FIELDS.includes(localStorage.ctSort) ? localStorage.ctSort : 'given', desc = localStorage.ctDesc === '1';
  const hesc = practical.esc;
  const coll = new Intl.Collator(lang, { sensitivity: 'base', numeric: true });
  const MONTHS = Array.from({ length: 12 }, (_, i) => dfmt(new Date(2000, i, 1), { month: 'long' }));
  const initial = v => { const c = v.normalize('NFD').charAt(0).toUpperCase(); return /\p{L}/u.test(c) ? c : '#'; };
  const rowOf = new Map(); $$('tr[data-key]', wrap).forEach(tr => { if (!rowOf.has(tr.dataset.key)) rowOf.set(tr.dataset.key, tr); }); // one pass, not one search per contact
  const recs = $$('.panels li[data-key]', wrap).map(li => {
    const d = li.dataset, name = d.name.trim(), md = d.bday.match(/(\d{2})-(\d{2})$/);
    return { li, tr: rowOf.get(d.key) || null, given: `${d.given || name} ${d.family}`.trim(), family: `${d.family || name} ${d.given}`.trim(), bday: md ? `${md[1]}-${md[2]}` : '', email: d.email.trim().toLowerCase() };
  });
  const groupOf = r => !r[sort] ? '-' : sort === 'bday' ? r.bday.slice(0, 2) : initial(r[sort]);
  const label = g => g === '-' ? SL.none : sort === 'bday' ? MONTHS[+g - 1] : g;
  const short = g => g === '-' ? '–' : sort === 'bday' ? MONTHS[+g - 1].slice(0, 3) : g;
  function resort() {
    const has = recs.filter(r => r[sort]), none = recs.filter(r => !r[sort]);
    has.sort((a, b) => coll.compare(a[sort], b[sort]) || coll.compare(a.given, b.given));
    if (desc) has.reverse();
    none.sort((a, b) => coll.compare(a.given, b.given));
    const groups = new Map();
    for (const r of has.concat(none)) { const g = groupOf(r); if (!groups.has(g)) groups.set(g, []); groups.get(g).push(r); }
    const panels = $('.panels', wrap), tbody = $('.sheet tbody', wrap), ph = $('.placeholder', panels);
    $$('.lpanel', panels).forEach(p => p.remove());
    let rail = '';
    for (const [g, rs] of groups) {
      const sec = document.createElement('section'); sec.className = 'lpanel'; sec.dataset.letter = g;
      sec.innerHTML = `<h3>${hesc(label(g))}</h3><ul class="list"></ul>`;
      const ul = sec.lastElementChild;
      rs.forEach(r => { ul.appendChild(r.li); if (r.tr) { r.tr.dataset.letter = g; tbody.appendChild(r.tr); } });
      panels.insertBefore(sec, ph);
      rail += `<button type="button" data-jump="${hesc(g)}" title="${hesc(label(g))}">${hesc(short(g))}</button>`;
    }
    $('.ct-rail', wrap).innerHTML = rail;
    sortSel.value = sort;
    sortDir.innerHTML = `<svg class="ico s" aria-hidden="true"><use href="${practical.sprite}#i-${desc ? 'sort-desc' : 'sort-asc'}"/></svg>`;
    sortDir.title = desc ? TS.desc : TS.asc; sortDir.setAttribute('aria-label', sortDir.title);
    applyFilter();
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
  async function open(key) { const r = practical.tell(await practical.req('/api/contacts/' + key)); if (r.ok) show(r.data); return r.ok; }
  $$('.list li[data-key]').forEach(li => li.onclick = () => open(li.dataset.key));
  $$('[data-open]').forEach(b => b.onclick = () => { wrap.classList.remove('view-table'); $('[data-ctview=list]').click(); open(b.dataset.open); });
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
      case 'goto': { const li = $$('.list li[data-key]').find(l => (l.dataset.name || '').toLowerCase() === val.toLowerCase()); if (li) { open(li.dataset.key); const sc = $('.main'); sc.scrollTop += li.getBoundingClientRect().top - sc.getBoundingClientRect().top - 120; } else toast(D.tMissing, true); break; }
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
  // the table picks a contact with a click on its row; the list opens it (above)
  $$('.sheet tr[data-key]', wrap).forEach(tr => tr.addEventListener('click', () => { activeKey = tr.dataset.key; markRow(activeKey); }));
  window.rusticalContacts = { reveal };
  // the work an ended session left unsaved (practical.keep above) comes back before anything else is opened
  const kept = practical.unstash('ct');
  if (kept) { show(kept.d); if (kept.isNew) { f.key.value = f.etag.value = ''; form.classList.add('new'); const b = $('#ct-save'); b.title = D.tCreate; b.setAttribute('aria-label', b.title); } mode('edit'); baseline = ''; updateButtons(); toast(tr('js-req-restored')); }
  else if (location.hash.length > 1) reveal(decodeURIComponent(location.hash.slice(1))); else if (wide()) show(null);
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
  $$('.sheet td[contenteditable][data-f], .sheet .pin [contenteditable][data-f]').forEach(cell => {
    let before = '';
    cell.addEventListener('focus', () => before = cell.textContent);
    cell.addEventListener('keydown', e => { if (e.key === 'Enter') { e.preventDefault(); cell.blur(); } });
    cell.addEventListener('blur', async () => {
      const v = cell.textContent.trim(); if (v === before.trim()) return;
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
      } catch (e) { cell.textContent = before; if (e && e.kind) practical.tell(e); else console.error(e); }
    });
  });
  // table view: + opens the editor as a dialog over the table (the list view has it beside the list)
  const dlgClose = () => { wrap.classList.remove('dlg', 'solo'); form.classList.remove('solo'); f.full_name.readOnly = false; det.classList.remove('on'); if (!wide()) det.hidden = true; };
  $('#ct-fab').onclick = () => { show(null); wrap.classList.add('dlg'); f.full_name.focus(); };
  /* a cell the table cannot edit (categories, address, note) opens just that card of the editor,
     already unlocked, as a dialog; saving writes the whole contact as the full editor does */
  async function openSolo(key, card) {
    await open(key);
    const c = $(`.card[data-card="${card}"]`, form); if (!c) return;
    c.dataset.added = '1'; mode('edit');
    $$('.card', form).forEach(x => x.classList.toggle('solo-on', x === c)); c.hidden = false;
    form.classList.add('solo'); wrap.classList.add('dlg', 'solo'); f.full_name.readOnly = true;
    // an empty list card (addresses …) starts with one empty row, as a new contact does with its phone
    const add = $('[data-add]', c); if (add && !$('.rows .row', c)) add.click();
    const first = $('input:not([type=hidden]):not([readonly]), textarea', c) || $('select', c); if (first) first.focus();
  }
  $$('.sheet td.cardcell').forEach(td => td.onclick = () => openSolo(td.closest('tr').dataset.key, td.dataset.card));
  addEventListener('keydown', e => { if (e.key === 'Escape' && wrap.classList.contains('dlg')) dlgClose(); });
})();
