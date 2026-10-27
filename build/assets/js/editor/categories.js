// js/editor/categories.js — part of the editor IIFE (src/bundle.rs; shares its scope): categories and category rules applied to the fields
  // ---- categories and category rules: `CATS` is the ordered list mirrored into `cats_json`. A rule
  // (`RULES`, per user, see domain::catrules) puts values on the event as a template ("Vorlage": prefilled,
  // editable) or a lock ("Vorgabe": fixed here). Where and repeat carry one mode per card; when and status one
  // per setting; reminders and links one per entry. Order matters: per setting the first category wins, the
  // others are struck through. `rulesApply` sets and locks the fields; every card lists the templates and locks
  // that touch it in a footer (`.rfoot`, `rulesMark`). The same dialog edits a rule (rule mode): the cards look
  // the same, the footer chips switch each setting between "beliebig" (disabled), template and lock. Opened
  // from an event the event is snapshotted and restored afterwards. ----
  const CT = $('#ed-cats-card'), CTT = CT.dataset, ctRows = $('#ed-ct-rows'), ctIn = $('#ed-ct-in'), ctCloud = $('#ed-ct-cloud');
  const RULES = JSON.parse(($('#category-rules-data') || {}).textContent || '{}');
  let CATS = [], CT_IDX = null, ctPop = null, RFX = {}, RFXL = { alarms: [], links: [] }, RLOCK = new Set(), ruleMode = null, ruleBack = null, ruleReturn = null, RM = {};
  const CTF = CTT.tFields.split('|');
  const FLD = { summary: ['title', 11], all_day: ['when', 0], calendar: ['when', 1], color: ['when', 2], location: ['where', 3], rrule: ['repeat', 5], status: ['status', 6], transp: ['status', 7], class: ['status', 8], priority: ['status', 9] };
  // the card a setting's footer row sits on: FLD[k][0] is also its group in the stored rule (catrules.rs StatusRule.class), which stays
  const CARD_OF = k => k === 'class' && TASK ? 'when' : FLD[k][0]; // a task has no status card: its CLASS sits in the general one
  const SCALARS = Object.keys(FLD), WHEN_KEYS = ['all_day', 'calendar', 'color'], STATUS_KEYS = ['status', 'transp', 'class', 'priority'], RULE_CARDS = ['when', 'where', 'alarm', 'repeat', 'status', 'links'];
  const ruleSetting = (r, fld) => { if (fld === 'summary') { const st = r.title; return st && st.value ? { mode: st.mode, value: st.value } : null; } if (fld === 'location') { const p = r.where; return p && p.mode !== 'off' && (p.location || p.geo) ? { mode: p.mode, value: p.location || '', geo: p.geo } : null; } if (fld === 'rrule') { const p = r.repeat; return p && p.mode !== 'off' && p.rrule ? { mode: p.mode, value: p.rrule } : null; } const st = (r[FLD[fld][0]] || {})[fld]; return st && st.value != null && st.value !== '' ? { mode: st.mode, value: st.value } : null; };
  const alarmKey = a => [a.action || 'DISPLAY', a.related_end ? 1 : 0, a.at || '', a.minutes ?? ''].join('|');
  const chips = () => CATS.slice();
  const ctMirror = () => { f.cats_json.value = CATS.length ? JSON.stringify(CATS) : ''; };
  const optText = (sel, v) => ([...sel.options].find(o => o.value === String(v)) || {}).textContent || String(v);
  const fxVal = (fld, v) => fld === 'all_day' ? esc(v ? CTT.tYes : CTT.tNo) : fld === 'calendar' ? esc(optText(f.calendar, v)) : fld === 'color' ? `<i class="ct-sw" style="background:${esc(v)}"></i>` : fld === 'rrule' ? esc(String(v).replace(/^FREQ=/, '').toLowerCase().slice(0, 20)) : STATUS_KEYS.includes(fld) ? esc(optText(f[fld], v)) : esc(String(v).slice(0, 24));
  const curVal = fld => fld === 'summary' ? f.summary.value.trim() : fld === 'all_day' ? !!(f.all_day && f.all_day.checked) : fld === 'calendar' ? f.calendar.value : fld === 'color' ? (colorOf() || '').toLowerCase() : fld === 'location' ? f.location.value.trim() : fld === 'rrule' ? (rrBuild() || '') : f[fld].value;
  const sameVal = (fld, v) => fld === 'all_day' ? !!(f.all_day && f.all_day.checked) === !!v : fld === 'color' ? (colorOf() || '').toLowerCase() === String(v).toLowerCase() : String(curVal(fld)) === String(v);
  // effective rules of the event's categories: per setting (scalars by field, reminders and links by key) a lock
  // beats a template, among equals the upper category wins; every loser is recorded as overridden by the winner
  function effective() { const fx = {}, over = {}, alarms = [], links = [], cand = {};
    const push = (key, c) => { (cand[key] ||= []).push(c); };
    CATS.forEach(c => { const r = RULES[c]; if (!r) return;
      SCALARS.forEach(fld => { const st = ruleSetting(r, fld); if (st) push(fld, { cat: c, ...st }); });
      ((r.alarm || {}).alarms || []).forEach(a => push('alarm:' + alarmKey(a), { cat: c, mode: a.mode, entry: a }));
      ((r.links || {}).links || []).forEach(l => { if (l.uri) push('link:' + l.uri, { cat: c, mode: l.mode, entry: l }); }); });
    Object.entries(cand).forEach(([key, list]) => { const win = list.find(x => x.mode === 'lock') || list[0]; list.forEach(x => { if (x !== win) (over[x.cat] ||= {})[key] = win.cat; }); if (key.startsWith('alarm:')) alarms.push(win); else if (key.startsWith('link:')) links.push(win); else fx[key] = win; });
    return { fx, over, alarms, links }; }
  // effect line of a category row: one item per setting, struck through when an upper category wins ("überstimmt");
  // deviation from a template is shown in the card footers, not here
  const ctFx = (c, over) => { const r = RULES[c]; if (!r) return `<div class="ct-l"><span class="ct-t full">${esc(CTT.tNorule)}</span></div>`; const out = [];
    const item = (key, mode, label) => { const by = over[c] && over[c][key]; out.push(`<div class="ct-l${mode === 'lock' ? ' lock' : ''}${by ? ' over' : ''}" title="${esc(mode === 'lock' ? CTT.tLock : CTT.tDefault)}"><svg class="ico"><use href="${practical.sprite}#${mode === 'lock' ? 'i-lock' : 'i-tag'}"/></svg><span class="ct-t">${label}</span>${by ? `<span class="ct-by">${esc(CTT.tOver.replace('{n}', by))}</span>` : ''}</div>`); }; /* one rule per line: icon | text, "überschrieben von" below the text */
    SCALARS.forEach(fld => { const st = ruleSetting(r, fld); if (st) item(fld, st.mode, `${esc(CTF[FLD[fld][1]])}: ${fxVal(fld, st.value)}`); });
    ((r.alarm || {}).alarms || []).forEach(a => item('alarm:' + alarmKey(a), a.mode, `${esc(CTF[4])}: ${esc(alarmText(a))}`));
    ((r.links || {}).links || []).forEach(l => item('link:' + l.uri, l.mode, `${esc(CTF[10])}: ${esc(l.label || lkHost(l.uri) || l.uri)}`));
    return out.join(''); };
  function ctRender() {
    ctMirror();
    const { over } = effective();
    ctRows.innerHTML = CATS.map((c, i) => `<div class="ct-row" data-i="${i}" draggable="true"><span class="ct-grip edits" title="${esc(CTT.tUp)} / ${esc(CTT.tDown)}">⋮⋮</span><span class="ct-head"><span class="chip">${esc(c)}</span><button class="add edits ct-rule" type="button" data-ct-rule="${esc(c)}"><svg class="ico"><use href="${practical.sprite}#i-settings"/></svg> ${esc(CTT.tRule)}</button></span><button class="ct-x edits" type="button" data-ct-rm="${i}">×</button><span class="ct-fx">${ctFx(c, over)}</span></div>`).join('');
    ctCloudRender();
  }
  // one fetch however often the index is asked for meanwhile (a fill renders the categories 3 times); a failed one is asked again next time
  const ctIndex = () => { if (CT_IDX) return CT_IDX; const p = CT_IDX = fetch('/api/categories?scope=' + ED_SCOPE).then(r => r.ok ? r.json() : Promise.reject(r)).catch(() => { if (CT_IDX === p) CT_IDX = null; return []; }); return p; };
  const ctInvalidate = () => { CT_IDX = null; };
  async function ctCloudRender() { const idx = await ctIndex(); ctCloud.innerHTML = idx.filter(c => !CATS.includes(c.name)).slice(0, 24).map(c => `<button class="ct-tag" type="button" data-ct-add="${esc(c.name)}">${esc(c.name)}${c.count ? ` <small>${c.count}</small>` : ''}${c.rule ? ` <svg class="ico" style="width:10px;height:10px"><use href="${practical.sprite}#i-tag"/></svg>` : ''}</button>`).join(''); }
  function catsInit(list) { CATS = (list || []).map(c => String(c).trim()).filter(Boolean); ctRender(); }
  function catAdd(v) { v = String(v).replace(/,/g, '').trim(); if (!v || CATS.includes(v)) return; addCard('cats'); CATS.push(v); ctRender(); rulesApply('add'); refresh(); }
  const ctClosePop = () => { if (ctPop) { ctPop.remove(); ctPop = null; } };
  ctIn.addEventListener('input', async () => { const q = ctIn.value.replace(/,/g, '').trim().toLowerCase(); ctClosePop(); if (!q) return; const idx = await ctIndex(); if (ctIn.value.replace(/,/g, '').trim().toLowerCase() !== q) return; const hits = idx.filter(c => c.name.toLowerCase().includes(q) && !CATS.includes(c.name)).slice(0, 5); const exact = idx.some(c => c.name.toLowerCase() === q);
    ctPop = document.createElement('div'); ctPop.className = 'rrpop'; ctPop.innerHTML = hits.map(c => `<button type="button" class="ct-hit" data-v="${esc(c.name)}">${esc(c.name)}<small>${esc(CTT.tExisting)}${c.count ? ' · ' + c.count : ''}</small></button>`).join('') + (exact ? '' : `<button type="button" class="ct-hit new" data-v="${esc(ctIn.value.replace(/,/g, '').trim())}">${esc(CTT.tNew.replace('{n}', ctIn.value.replace(/,/g, '').trim()))}</button>`);
    document.body.appendChild(ctPop); placeUnder(ctPop, ctIn);
    ctPop.addEventListener('mousedown', e => e.preventDefault()); ctPop.addEventListener('click', e => { const b = e.target.closest('[data-v]'); if (b) { catAdd(b.dataset.v); ctIn.value = ''; ctClosePop(); } }); });
  ctIn.addEventListener('keydown', e => { if (e.key === 'Enter' || e.key === ',') { e.preventDefault(); const first = ctPop && $('.ct-hit:not(.new)', ctPop); if (first && e.key === 'Enter') catAdd(first.dataset.v); else catAdd(ctIn.value); ctIn.value = ''; ctClosePop(); } else if (e.key === 'Escape') ctClosePop(); }); /* Enter takes the first existing suggestion; a new name is created via the last entry or a comma */
  ctIn.addEventListener('blur', () => { setTimeout(ctClosePop, 150); if (ctIn.value.trim()) { catAdd(ctIn.value); ctIn.value = ''; } });
  CT.addEventListener('click', e => { const rm = e.target.closest('[data-ct-rm]'); if (rm) { CATS.splice(+rm.dataset.ctRm, 1); ctRender(); rulesApply('change'); refresh(); return; }
    const add = e.target.closest('[data-ct-add]'); if (add) { catAdd(add.dataset.ctAdd); return; }
    const rl = e.target.closest('[data-ct-rule]'); if (rl) { openRuleEditor(rl.dataset.ctRule, true); return; }
    const grip = e.target.closest('.ct-grip'); if (grip) { const i = +grip.closest('.ct-row').dataset.i; rrPop(grip, [{ v: 'up', label: CTT.tUp }, { v: 'down', label: CTT.tDown }], null, v => { const j = v === 'up' ? i - 1 : i + 1; if (j < 0 || j >= CATS.length) return; [CATS[i], CATS[j]] = [CATS[j], CATS[i]]; ctRender(); rulesApply('change'); refresh(); }); } });
  let ctDrag = null;
  ctRows.addEventListener('dragstart', e => { const row = e.target.closest('.ct-row'); if (!row) return; ctDrag = +row.dataset.i; row.classList.add('dragging'); e.dataTransfer.effectAllowed = 'move'; });
  ctRows.addEventListener('dragover', e => { const row = e.target.closest('.ct-row'); if (!row || ctDrag == null) return; e.preventDefault(); $$('.ct-row.over', ctRows).forEach(x => x.classList.remove('over')); row.classList.add('over'); });
  ctRows.addEventListener('drop', e => { const row = e.target.closest('.ct-row'); if (!row || ctDrag == null) return; e.preventDefault(); const to = +row.dataset.i; const [m] = CATS.splice(ctDrag, 1); CATS.splice(to, 0, m); ctDrag = null; ctRender(); rulesApply('change'); refresh(); });
  ctRows.addEventListener('dragend', () => { ctDrag = null; $$('.ct-row.dragging, .ct-row.over', ctRows).forEach(x => x.classList.remove('dragging', 'over')); });
  // ---- applying the rules to the fields ----
  const modeIcon = m => m === 'lock' ? 'i-lock' : m === 'default' ? 'i-tag' : 'i-dots';
  const modeText = m => m === 'lock' ? CTT.tLock : m === 'default' ? CTT.tDefault : CTT.tAny;
  // badge "Vorlage · Kat" / "Vorgabe · Kat"; `extra` appended with " · "; `changed` = the event deviates from a template → orange, " (geändert)"
  const ruleBadgeHtml = (e, extra, changed) => `<span class="rb${changed ? ' changed' : ''}"><span class="rbadge ${e.mode}"><svg class="ico"><use href="${practical.sprite}#${modeIcon(e.mode)}"/></svg>${esc(modeText(e.mode))} · ${esc(e.cat)}</span>${extra ? `<span class="rb-x">${esc(extra)}</span>` : ''}${changed ? `<span class="rb-x">(${esc(CTT.tChanged)})</span>` : ''}</span>`;
  const ruleBadge = (fld, cur) => { const e = RFX[fld]; if (!e) return ''; const same = cur === undefined || sameVal(fld, e.value); return ruleBadgeHtml(e, fld === 'calendar' && !form.classList.contains('new') ? CTT.tNewonly : '', !same && e.mode === 'default'); }; /* a lock shows no "adjusted": the forced value is visible in the field */
  // disabled looks for locked settings (event: locks; rule editor: settings on "beliebig")
  function applyLocks() {
    f.summary.readOnly = !ruleMode && RLOCK.has('summary'); f.summary.disabled = !!ruleMode && RLOCK.has('summary'); /* event: a locked title; rule editor: "beliebig" */
    if (f.all_day) f.all_day.disabled = RLOCK.has('all_day');
    f.calendar.disabled = RLOCK.has('calendar');
    f.color.disabled = f.color_cal.disabled = $('#ed-color-btn').disabled = RLOCK.has('color');
    $('.card[data-card="where"]', form).classList.toggle('locked-where', RLOCK.has('location'));
    rrLocked = RLOCK.has('rrule'); $x('#ed-repeat-card').classList.toggle('locked', rrLocked);
  }
  function rulesApply(reason) {
    if (ruleMode) { RFX = {}; RFXL = { alarms: [], links: [] }; RLOCK = new Set(SCALARS.filter(k => (RM[k] || 'off') === 'off')); ALARMS.forEach(a => { a._mode ||= 'default'; a._locked = a._mode === 'off'; }); LK.links.forEach(l => { l._mode ||= 'default'; l._locked = l._mode === 'off'; }); applyLocks(); rulesMark(); stRender(); alRender(); lkRender(); cards(); return false; }
    const { fx, alarms, links } = effective(); RFX = fx; RFXL = { alarms, links }; RLOCK = new Set(Object.keys(fx).filter(k => fx[k].mode === 'lock'));
    const isNew = form.classList.contains('new'), prefill = reason === 'add' || reason === 'new';
    let adjusted = false;
    const set = (fld, differs, apply) => { const e = fx[fld]; if (!e) return; if (e.mode === 'lock' ? differs() : (prefill && differs())) { apply(e.value); if (e.mode === 'lock' && reason === 'open') { adjusted = true; e.adjusted = true; } } };
    set('summary', () => fx.summary.mode === 'lock' ? f.summary.value.trim() !== String(fx.summary.value).trim() : !f.summary.value.trim(), v => { f.summary.value = v; });
    if (f.all_day) set('all_day', () => fx.all_day.mode === 'lock' ? f.all_day.checked !== !!fx.all_day.value : !f.all_day.checked && !!fx.all_day.value, v => setAllDay(!!v));
    if (isNew) set('calendar', () => f.calendar.value !== fx.calendar.value && [...f.calendar.options].some(o => o.value === fx.calendar.value), v => { f.calendar.value = v; syncDot(); }); else RLOCK.delete('calendar'); /* an existing event cannot move */
    set('color', () => fx.color.mode === 'lock' ? (colorOf() || '').toLowerCase() !== String(fx.color.value).toLowerCase() : !colorOf(), v => { setColor(v); syncDot(); });
    set('location', () => fx.location.mode === 'lock' ? f.location.value.trim() !== String(fx.location.value).trim() : !f.location.value.trim(), v => { f.location.value = v; const g = fx.location.geo; if (g) { f.lat.value = g[0]; f.lon.value = g[1]; } locInit(); });
    ALARMS.forEach(a => { delete a._rule; delete a._locked; delete a._mode; });
    alarms.forEach(e => { let a = ALARMS.find(x => alarmKey(x) === alarmKey(e.entry)); if (!a && (e.mode === 'lock' || prefill)) { const { mode, ...ra } = e.entry; a = newAlarm({ ...ra, attendees: [...(ra.attendees || [])] }); ALARMS.push(a); if (e.mode === 'lock' && reason === 'open') { adjusted = true; e.adjusted = true; } } if (a) { a._rule = e; a._locked = e.mode === 'lock'; } });
    LK.links.forEach(l => { delete l._rule; delete l._locked; delete l._mode; });
    links.forEach(e => { let l = LK.links.find(x => x.uri === e.entry.uri); if (!l && (e.mode === 'lock' || prefill)) { const { mode, ...rl } = e.entry; l = { ...rl, embedded: false, keep: false, features: rl.features || [], extra: rl.extra || [] }; LK.links.push(l); if (e.mode === 'lock' && reason === 'open') { adjusted = true; e.adjusted = true; } } if (l) { l._rule = e; l._locked = e.mode === 'lock'; } });
    if (fx.rrule) { const e = fx.rrule; const cur = rrBuild() || ''; if (e.mode === 'lock' ? cur !== e.value : (prefill && !cur)) { rrInit(e.value, null, [], []); if (e.mode === 'lock' && reason === 'open') { adjusted = true; e.adjusted = true; } } }
    STATUS_KEYS.filter(fld => f[fld]).forEach(fld => set(fld, () => fx[fld].mode === 'lock' ? f[fld].value !== String(fx[fld].value) : f[fld].value === '', v => { if (fld === 'priority') setSel(f.priority, v); else f[fld].value = String(v); }));
    applyLocks(); rulesMark(); stRender(); alRender(); lkRender(); cards(); /* cards that got rule entries must show */
    return adjusted;
  }
  // card footers: in an event every template/lock that touches the card (name · badge, "geändert" when the event
  // deviates from a template); in rule mode every setting of the card with its mode chip. Re-run on every change.
  function rulesMark() {
    const foot = {}; const add = (card, name, chip) => { (foot[card] ||= []).push(`<div class="rf-row"><span class="rf-name">${esc(name)}</span>${chip}</div>`); };
    if (ruleMode) {
      const chip = (key, mode) => `<button type="button" class="rbadge ${mode}" data-rmode="${esc(key)}" title="${esc(CTT.tRulemode)}"><svg class="ico"><use href="${practical.sprite}#${modeIcon(mode)}"/></svg>${esc(modeText(mode))}</button>`;
      add('title', CTF[11], chip('summary', RM.summary || 'off'));
      WHEN_KEYS.filter(k => k === 'color' || f[k]).forEach(k => add('when', CTF[FLD[k][1]], chip(k, RM[k] || 'off')));
      add('where', CTF[3], chip('location', RM.location || 'off'));
      ALARMS.forEach(a => add('alarm', alarmText(a), chip('alarm:' + a._id, a._mode || 'off')));
      add('repeat', CTF[5], chip('rrule', RM.rrule || 'off'));
      STATUS_KEYS.filter(k => f[k]).forEach(k => add(CARD_OF(k), CTF[FLD[k][1]], chip(k, RM[k] || 'off')));
      LK.links.forEach((l, i) => { if (!l.embedded) add('links', lkTitle(l), chip('link:' + i, l._mode || 'off')); });
    } else {
      const fx = RFX, entry = (e, present) => ruleBadgeHtml(e, '', !present && e.mode === 'default');
      if (fx.summary) add('title', CTF[11], ruleBadge('summary', curVal('summary')));
      WHEN_KEYS.forEach(k => { if (fx[k]) add('when', CTF[FLD[k][1]], ruleBadge(k, curVal(k))); });
      if (fx.location) add('where', CTF[3], ruleBadge('location', f.location.value.trim()));
      RFXL.alarms.forEach(e => add('alarm', alarmText(e.entry), entry(e, ALARMS.some(x => alarmKey(x) === alarmKey(e.entry)))));
      if (fx.rrule) add('repeat', CTF[5], ruleBadge('rrule', rrBuild() || ''));
      STATUS_KEYS.forEach(k => { if (fx[k]) add(CARD_OF(k), CTF[FLD[k][1]], ruleBadge(k, f[k].value)); });
      RFXL.links.forEach(e => add('links', e.entry.label || lkHost(e.entry.uri) || e.entry.uri, entry(e, LK.links.some(x => x.uri === e.entry.uri))));
    }
    ['title', ...RULE_CARDS].forEach(c => { $x(`.rfoot[data-rfoot="${c}"]`, form).innerHTML = (foot[c] || []).join(''); });
    if (!ruleMode) ctRender();
  }
