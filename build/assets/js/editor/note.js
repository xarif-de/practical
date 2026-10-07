  // ---- note (round 17): the textarea stays plain text (DESCRIPTION). Suggestions while typing come from a
  // lazily loaded index (contacts, categories, events, tasks) even without a prefix; picking one inserts
  // @Name / #Category / !Title. Detected persons, links and categories are offered as chips that create the
  // entry in the matching card. The view mode renders links, mentions and "- " lists. ----
  const NC = $('#ed-note-card'), NT = NC.dataset, ntView = $('#ed-note-view'), ntFound = $('#ed-note-found');
  let NTX = null, ntLoading = null, ntPop = null, ntTimer = 0;
  const ntIndex = () => NTX ? Promise.resolve(NTX) : (ntLoading ||= fetch('/api/mentions').then(r => r.ok ? r.json() : { contacts: [], categories: [], events: [], tasks: [] }).then(d => (NTX = d)).catch(() => { ntLoading = null; return { contacts: [], categories: [], events: [], tasks: [] }; }));
  const ntInvalidate = () => { NTX = null; ntLoading = null; };
  const { RE_URL, RE_MAIL, RE_TEL, RE_MENTION } = practical.noteRe;
  const ntWordAtCaret = () => { const t = f.description, v = t.value.slice(0, t.selectionStart); const m = /(^|\s)([@#!]?)([^\s@#!]{1,60})$/.exec(v); return m ? { prefix: m[2], word: m[3], start: v.length - m[2].length - m[3].length } : null; };
  const ntClosePop = () => { if (ntPop) { ntPop.remove(); ntPop = null; } };
  async function ntSuggest() {
    const w = ntWordAtCaret();
    if (!w || (w.prefix ? w.word.length < 1 : w.word.length < 3)) { ntClosePop(); return; }
    const ix = await ntIndex(), q = w.word.toLowerCase(), has = s => s.toLowerCase().includes(q);
    const hits = [];
    if (!w.prefix || w.prefix === '@') ix.contacts.filter(c => has(c.name)).slice(0, 4).forEach(c => hits.push({ p: '@', v: c.name, sub: NT.tContact + (c.email ? ' · ' + c.email : '') }));
    if (!w.prefix || w.prefix === '#') ix.categories.filter(c => has(c.name)).slice(0, 3).forEach(c => hits.push({ p: '#', v: c.name, sub: NT.tCat + ' · ' + c.count }));
    if (!w.prefix || w.prefix === '!') { ix.events.filter(e => has(e.summary)).slice(0, 3).forEach(e => hits.push({ p: '!', v: e.summary, sub: NT.tEvent + (e.date ? ' · ' + fmtD(e.date) : '') })); ix.tasks.filter(e => has(e.summary)).slice(0, 2).forEach(e => hits.push({ p: '!', v: e.summary, sub: NT.tTask })); }
    ntClosePop();
    if (!hits.length || ntWordAtCaret()?.word !== w.word) return;
    ntPop = document.createElement('div'); ntPop.className = 'rrpop';
    ntPop.innerHTML = hits.slice(0, 7).map((h, i) => `<button type="button" class="lk-hit" data-h="${i}"><span class="nt-pre">${h.p}</span><span>${esc(h.v)}</span><small>${esc(h.sub)}</small></button>`).join('');
    document.body.appendChild(ntPop);
    placeUnder(ntPop, f.description);
    ntPop.addEventListener('mousedown', e => e.preventDefault()); // keep the textarea focused
    ntPop.addEventListener('click', e => { const b = e.target.closest('.lk-hit'); if (b) ntPick(hits[+b.dataset.h]); });
    ntPop._pick = () => ntPick(hits[0]);
  }
  function ntPick(h) { const w = ntWordAtCaret(); if (!w) return; const t = f.description, v = t.value, ins = h.p + h.v + ' '; t.value = v.slice(0, w.start) + ins + v.slice(t.selectionStart); const c = w.start + ins.length; t.setSelectionRange(c, c); ntClosePop(); ntChanged(); }
  f.description.addEventListener('keydown', e => { if (ntPop && (e.key === 'Tab' || e.key === 'Enter') && e.target === f.description) { e.preventDefault(); ntPop._pick(); } else if (e.key === 'Escape' && ntPop) { e.stopPropagation(); ntClosePop(); } });
  f.description.addEventListener('blur', () => setTimeout(ntClosePop, 150));
  f.description.addEventListener('input', () => { ntSuggest(); clearTimeout(ntTimer); ntTimer = setTimeout(ntDetect, 300); });
  const ntChanged = () => { grow(); refresh(); ntDetect(); };
  // detection: what the note mentions that the other cards do not have yet
  const chips2 = () => chips(); // the event's categories: ntDetect has a local `chips` of its own
  async function ntDetect() {
    const text = f.description.value; ntFound.innerHTML = '';
    if (!text.trim() || !form.classList.contains('mode-edit')) return;
    const ix = await ntIndex(); if (f.description.value !== text) return;
    const chips = [];
    const present = new Set(peAll().map(p => p.email.toLowerCase()));
    const mentioned = new Set([...text.matchAll(RE_MENTION)].filter(m => m[1] === '@').map(m => m[2].replace(/[.,;:!?]+$/, '').toLowerCase()));
    ix.contacts.filter(c => c.email && !present.has(c.email.toLowerCase())).filter(c => mentioned.has(c.name.toLowerCase()) || (c.name.length >= 5 && new RegExp('(?<![\\p{L}])' + c.name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&') + '(?![\\p{L}])', 'iu').test(text))).slice(0, 4).forEach(c => chips.push({ k: 'person', icon: 'i-users', text: c.name, sub: NT.tPeople, data: c }));
    [...new Set((text.match(RE_URL) || []).map(u => u.replace(/[.,;:!?]+$/, '')))].filter(u => !LK.links.some(l => l.uri === u)).slice(0, 4).forEach(u => chips.push({ k: 'link', icon: 'i-link', text: u.replace(/^https?:\/\/(www\.)?/i, ''), sub: NT.tLinks, data: u }));
    const have = new Set(chips2().map(c => c.toLowerCase()));
    const tags = new Set([...text.matchAll(RE_MENTION)].filter(m => m[1] === '#').map(m => m[2].replace(/[.,;:!?]+$/, '')));
    ix.categories.filter(c => c.name.length >= 4 && new RegExp('(?<![\\p{L}#])' + c.name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&') + '(?![\\p{L}])', 'iu').test(text)).forEach(c => tags.add(c.name));
    [...tags].filter(t => !have.has(t.toLowerCase())).slice(0, 4).forEach(t => chips.push({ k: 'cat', icon: 'i-tag', text: t, sub: NT.tCats, data: t }));
    [...new Set(text.match(RE_TEL) || [])].slice(0, 2).forEach(t => chips.push({ k: 'tel', icon: 'i-phone', text: t.trim(), sub: NT.tCall, data: 'tel:' + t.replace(/[^\d+]/g, ''), ghost: true }));
    [...new Set(text.match(RE_MAIL) || [])].filter(m => !present.has(m.toLowerCase())).slice(0, 2).forEach(m => chips.push({ k: 'mail', icon: 'i-mail', text: m, sub: NT.tMail, data: 'mailto:' + m, ghost: true }));
    ntFound.innerHTML = chips.map((c, i) => `<button class="pillbtn${c.ghost ? ' ghost' : ''}" type="button" data-nt="${i}"><svg class="ico"><use href="${practical.sprite}#${c.icon}"/></svg><span>${esc(c.text)}</span><small>${esc(c.sub)}</small></button>`).join('');
    ntFound._chips = chips;
  }
  ntFound.addEventListener('click', e => { const b = e.target.closest('[data-nt]'); if (!b) return; const c = ntFound._chips[+b.dataset.nt];
    if (c.k === 'person') { addCard('people'); PEOPLE.attendees.push({ name: c.data.name, email: c.data.email, role: 'REQ-PARTICIPANT', status: '', cutype: '', extra: [], contacts: [{ key: c.data.key, name: c.data.name, sub: c.data.email, initials: c.data.name.split(/\s+/).map(w => w[0]).slice(0, 2).join('').toUpperCase(), color_index: 1 }] }); peopleRender(); }
    else if (c.k === 'link') { addCard('links'); lkAddUri(c.data); }
    else if (c.k === 'cat') { catAdd(c.data); }
    else { location.href = c.data; return; }
    refresh(); ntDetect(); });
  // view mode: links, phone numbers, e-mail addresses and @ # ! mentions become clickable; "- " lines are bullets
  function ntRender() { ntView.innerHTML = practical.noteHtml(f.description.value); }
  const ntSearch = v => { const q = $('#q'); if (q) { q.value = v; q.focus(); q.dispatchEvent(new Event('input', { bubbles: true })); } };
  ntView.addEventListener('click', async e => { const m = e.target.closest('.nt-m'); if (!m) return; const k = m.dataset.k, v = m.dataset.v.toLowerCase();
    if (k === '#') { ntSearch(m.dataset.v); return; }
    const ix = await ntIndex();
    if (k === '@') { const c = ix.contacts.find(x => x.name.toLowerCase() === v); location.href = c ? '/contacts#' + c.key : '/contacts'; }
    else { const ev = ix.events.find(x => x.summary.toLowerCase() === v) || ix.events.find(x => x.summary.toLowerCase().startsWith(v)); const tk = !ev && (ix.tasks.find(x => x.summary.toLowerCase() === v) || ix.tasks.find(x => x.summary.toLowerCase().startsWith(v))); if (ev) openEditor(ev.key); else if (tk) location.href = '/tasks#' + tk.key; else ntSearch(m.dataset.v); } });
