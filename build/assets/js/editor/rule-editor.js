// js/editor/rule-editor.js — part of the editor IIFE (src/bundle.rs; shares its scope): the dialog in rule mode
  // ---- rule editor: the dialog in rule mode. The cards are the event cards; the footer chips carry the mode per
  // setting or entry (`RM`, `_mode`); "beliebig" disables the setting. `sig()` extends the dirty check by the modes. ----
  const RULE_NEW = { color: () => '#3478f6', status: () => 'CONFIRMED', transp: () => 'OPAQUE', class: () => 'PUBLIC', priority: () => 5 };
  function setRuleMode(key, v) {
    if (key.startsWith('alarm:')) { const a = ALARMS.find(x => String(x._id) === key.slice(6)); if (a) a._mode = v; }
    else if (key.startsWith('link:')) { const l = LK.links[+key.slice(5)]; if (l) l._mode = v; }
    else { RM[key] = v; if (v !== 'off') { if (key === 'color' && !colorOf()) { setColor(RULE_NEW.color()); syncDot(); } else if (STATUS_KEYS.includes(key) && f[key].value === '') { if (key === 'priority') setSel(f.priority, RULE_NEW.priority()); else f[key].value = RULE_NEW[key](); } } }
    rulesApply('change'); refresh();
  }
  form.addEventListener('click', e => { const b = e.target.closest('[data-rmode]'); if (!b || !ruleMode) return; e.stopPropagation(); const key = b.dataset.rmode; const cur = key.startsWith('alarm:') ? (ALARMS.find(x => String(x._id) === key.slice(6)) || {})._mode : key.startsWith('link:') ? (LK.links[+key.slice(5)] || {})._mode : RM[key];
    rrPop(b, ['off', 'default', 'lock'].map(m => ({ v: m, label: modeText(m) })), cur || 'off', v => setRuleMode(key, v)); });
  const eventBody = () => { const [principal, cal_id] = f.calendar.value.split('/'); const allDay = f.all_day.checked; const start = allDay ? f.date.value + 'T00:00:00' : f.date.value + 'T' + (f.from.value || '09:00') + ':00'; const endDate = f.end_date.value || f.date.value; const end = allDay ? nextDay(endDate) + 'T00:00:00' : endDate + 'T' + (f.to.value || f.from.value || '10:00') + ':00';
    return { principal, cal_id, summary: f.summary.value, location: f.location.value, description: f.description.value, start, end, all_day: allDay, rrule: rrBuild(), exdates: rrDates('exdates'), rdates: rrDates('rdates'), alarms: alarmsOf(), organizer: PEOPLE.organizer, attendees: PEOPLE.attendees, categories: chips(), status: f.status.value, transp: f.transp.value, class: f.class.value, priority: f.priority.value === '' ? null : +f.priority.value, color: colorOf(), geo: geoOf(), ...linksOf(), timezone: root.dataset.tz, etag: f.etag.value }; };
  const sig = () => serialize() + (ruleMode ? '|' + JSON.stringify(ruleOf()) : '');
  // the dialog's content as a detail `fill` takes back: the rule editor's way back, and the work kept over a new login
  const snapDetail = () => ({ ...(TASK ? taskBody() : { ...eventBody(), rrule_until: RR.end.kind === 'until' ? RR.end.until : null, has_overrides: !!(current && current.has_overrides), related: LK.related }), object_id: f.key.value.split('/')[2] || '', uid: (current || {}).uid || '' });
  function openRuleEditor(name, stack) {
    if (stack && !ruleMode) ruleReturn = { detail: snapDetail(), baseline, isNew: form.classList.contains('new'), summaryPh: f.summary.placeholder, date: openDate };
    const r = RULES[name] || {}, today = state.date || new Date().toISOString().slice(0, 10), cal = ((f.calendar.options[0] || {}).value || '/').split('/');
    const st = k => ((r[FLD[k][0]] || {})[k]) || null, where = r.where || { mode: 'off' }, repeat = r.repeat || { mode: 'off' }, ralarms = (r.alarm || {}).alarms || [], rlinks = (r.links || {}).links || [];
    const calv = st('calendar') && [...f.calendar.options].some(o => o.value === st('calendar').value) ? st('calendar').value.split('/') : cal;
    ruleMode = name; form.classList.add('rulemode'); ed.classList.add('rulemode');
    RM = {}; WHEN_KEYS.concat(STATUS_KEYS).forEach(k => { RM[k] = st(k) ? st(k).mode : 'off'; }); RM.summary = r.title ? r.title.mode : 'off'; RM.location = where.mode || 'off'; RM.rrule = repeat.mode || 'off';
    // a rule is edited through the same cards, so it needs a detail of the shape this consumer fills
    if (TASK) {
      fill({ principal: calv[0], cal_id: calv[1], object_id: '', uid: '', summary: r.title ? r.title.value : '', description: '',
        location: where.mode !== 'off' ? where.location || '' : '', start: null, start_all_day: true, due: null, due_all_day: true,
        status: st('status') ? st('status').value : '', percent: null, completed_at: null,
        class: st('class') ? st('class').value : '', priority: st('priority') ? st('priority').value : null, categories: [],
        color: st('color') ? st('color').value : '', geo: where.mode !== 'off' ? where.geo || null : null,
        alarms: ralarms.map(({ mode, ...a }) => a), links: rlinks.map(({ mode, ...l }) => l), related: [], etag: '', has_rrule: false });
    } else
    fill({ principal: calv[0], cal_id: calv[1], object_id: '', uid: '', summary: r.title ? r.title.value : '', location: where.mode !== 'off' ? where.location || '' : '', description: '', start: today + 'T09:00:00', end: today + 'T10:00:00', all_day: st('all_day') ? !!st('all_day').value : false, rrule: repeat.mode !== 'off' ? repeat.rrule || null : null, rrule_until: null, exdates: [], rdates: [], organizer: null, attendees: [], alarms: ralarms.map(({ mode, ...a }) => a), categories: [], status: st('status') ? st('status').value : '', transp: st('transp') ? st('transp').value : '', class: st('class') ? st('class').value : '', priority: st('priority') ? st('priority').value : null, color: st('color') ? st('color').value : '', geo: where.mode !== 'off' ? where.geo || null : null, links: rlinks.map(({ mode, ...l }) => l), related: [], etag: '', has_overrides: false });
    ralarms.forEach((a, i) => { if (ALARMS[i]) ALARMS[i]._mode = a.mode; }); rlinks.forEach((l, i) => { if (LK.links[i]) LK.links[i]._mode = l.mode; });
    const has = { when: true, where: RM.location !== 'off', alarm: ALARMS.length > 0, repeat: RM.rrule !== 'off', status: STATUS_KEYS.some(k => RM[k] !== 'off'), links: LK.links.length > 0 };
    RULE_CARDS.forEach(n => { const c = $(`.card[data-card="${n}"]`, form); if (!c) return; if (has[n]) c.dataset.added = '1'; else delete c.dataset.added; });
    mode('edit'); rulesApply('open'); f.summary.placeholder = D.tTitle; f.summary.required = false; $('#ed-sub').textContent = D.tRuleTitle.replace('{name}', name); $x('#ed-hint').textContent = ''; /* the title field holds the title template; the rule name goes to the subtitle; no series hint */
    ed.hidden = false; baseline = sig(); updateButtons(); $('#ed-delete').disabled = !RULES[name];
  }
  function ruleOf() {
    const setting = (k, v) => (RM[k] || 'off') !== 'off' && v !== '' && v != null ? { mode: RM[k], value: v } : null;
    const alarms = alarmsOf().map((a, i) => ({ mode: ALARMS[i]._mode || 'default', ...a })).filter(a => a.mode !== 'off'), plain = LK.links.filter(l => !l.embedded), links = linksOf().links.filter(l => !l.embedded).map((l, i) => ({ mode: (plain[i] || {})._mode || 'default', ...l })).filter(l => l.mode !== 'off');
    return { title: setting('summary', f.summary.value.trim()), when: { all_day: f.all_day ? setting('all_day', f.all_day.checked) : null, calendar: setting('calendar', f.calendar.value), color: setting('color', colorOf()) }, where: { mode: RM.location || 'off', location: f.location.value.trim(), geo: geoOf() }, alarm: { alarms }, repeat: { mode: TASK ? 'off' : (RM.rrule || 'off'), rrule: TASK ? '' : (rrBuild() || '') }, status: Object.fromEntries(STATUS_KEYS.filter(k => f[k]).map(k => [k, setting(k, k === 'priority' ? (f.priority.value === '' ? null : +f.priority.value) : f[k].value)])), links: { links } }; }
  // leaving rule mode: back to the event it was opened from (rules re-applied by `fill`), or to the page it
  // was called from (`?back=`), or simply closed
  function ruleLeave() { if (!ruleMode) return false; ruleMode = null; RM = {}; form.classList.remove('rulemode'); ed.classList.remove('rulemode'); f.summary.readOnly = false; f.summary.disabled = false; f.summary.required = true; $$('.rfoot', form).forEach(el => { el.innerHTML = ''; });
    if (ruleReturn) { const r = ruleReturn; ruleReturn = null; fill(r.detail, r.date); touched = true; if (r.isNew) { form.classList.add('new'); f.key.value = ''; f.etag.value = ''; f.summary.placeholder = r.summaryPh; const b = $('#ed-save'); b.title = D.tCreate; b.setAttribute('aria-label', b.title); } mode('edit'); rulesApply('open'); baseline = r.baseline; updateButtons(); ed.hidden = false; return true; }
    if (ruleBack) { const to = ruleBack; ruleBack = null; practical.leaving = true; location.href = to; return true; }
    return false; }
  const rulesChanged = name => { const h = HOST(); if (h.rulesChanged) h.rulesChanged(name); }; // the page lists the rules (settings)
  async function ruleSave() { const rule = ruleOf(), name = ruleMode; const r = await practical.send('/api/category-rules/' + encodeURIComponent(name) + '?scope=' + ED_SCOPE, 'PUT', rule); if (!r.ok) return r; RULES[name] = r.data; ctInvalidate(); rulesChanged(name); if (!ruleLeave()) close(); return r; }
  async function ruleDelete() { const name = ruleMode; const r = await practical.req('/api/category-rules/' + encodeURIComponent(name) + '?scope=' + ED_SCOPE, { method: 'DELETE' }); if (r.ok) { delete RULES[name]; ctInvalidate(); rulesChanged(name); if (!ruleLeave()) close(); } return r; }
