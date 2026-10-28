// js/editor/wiring.js — part of the editor IIFE (src/bundle.rs; shares its scope): form events, save, delete, the day line, work kept over a lost session; window.rusticalEd
  // typing: the save/reset buttons follow at once (Enter must find them right), the derived parts (when line,
  // rule sentence, note height, reminder strip, footers — two of them force a layout) once per frame; whatever
  // reads them (save, close) runs a pending refresh first
  form.addEventListener('input', () => { updateButtons(); if (!refreshQ) refreshQ = requestAnimationFrame(refresh); });
  form.addEventListener('change', e => { const t = e.target; if (t === f.calendar) { syncDot(); visRender(); cards(); /* a share's calendar: the reminder card inert, not offered */ } if (t === f.date) { if (f.end_date.value < f.date.value) f.end_date.value = f.date.value; dayline(); } if (t === f.date || t === f.from || t === f.all_day) { rrFocus = null; rrRender(); } if (!ruleMode && CATS.length && !t.dataset.rv) rulesMark(); refresh(); });
  form.addEventListener('click', async e => {
    const b = e.target.closest('button'); if (!b || !form.contains(b)) return;
    if (b.dataset.add) { const l = b.dataset.add; const r = l === 'alarms' ? alarmAdd() : addDate(l, ''); const i = $('input:not([hidden]), select', r); if (i) i.focus(); refresh(); return; }
    if (b.hasAttribute('data-rm')) { b.closest('.row').remove(); refresh(); return; }
    if (b.dataset.cardAdd) { const c = addCard(b.dataset.cardAdd); b.closest('.addmenu').classList.remove('open'); if (ruleMode) { if (b.dataset.cardAdd === 'where') RM.location = 'default'; if (b.dataset.cardAdd === 'repeat') RM.rrule = 'default'; rulesApply('change'); } /* a one-setting rule card starts as template */ if (b.dataset.cardAdd === 'alarm') $('[data-add=alarms]', c).click(); else if (b.dataset.cardAdd === 'links') $('#ed-lk-url').focus(); else if (b.dataset.cardAdd === 'where') { locInit(); locin.focus(); } else if (b.dataset.cardAdd === 'repeat') { rrFocus = null; rrRender(); } else { const i = $('input:not([type=hidden]), select, textarea', c); if (i) i.focus(); } refresh(); return; }
    if (b.hasAttribute('data-card-rm')) { if (!await ask(D.tRmcard)) return; const c = b.closest('.card'); $$('input:not([type=hidden]), textarea', c).forEach(i => i.value = ''); $$('select.dat', c).forEach(s => s.value = ''); $$('.rows .row, .tags .chip', c).forEach(x => x.remove()); if (c.dataset.card === 'where') locInit(); if (c.dataset.card === 'alarm') alarmsInit([]); if (c.dataset.card === 'repeat') rrInit(null, null, [], []); if (c.dataset.card === 'status') { if (c.contains(f.class)) f.class.value = ''; stRender(); } if (c.dataset.card === 'links') linksInit([], []); if (c.dataset.card === 'note') { ntRender(); ntFound.innerHTML = ''; } if (c.dataset.card === 'cats') { catsInit([]); rulesApply('change'); } if (ruleMode) { SCALARS.filter(k => CARD_OF(k) === c.dataset.card).forEach(k => { RM[k] = 'off'; }); rulesApply('change'); } /* a removed rule card is off */ delete c.dataset.added; c.hidden = true; cards(); refresh(); return; }
    if (b.dataset.geo === 'swap') { locSwap(); return; }
    if (b.dataset.geo === 'map') { openMap(); return; }
    if (b.dataset.act) { const row = b.closest('.row'); const v = row ? (w(row, 'email') || w(row, 'uri')).value : f.location.value, open_ = u => window.open(u, '_blank', 'noopener'), g = geoOf(); if (b.dataset.act === 'osm') open_(g ? `https://www.openstreetmap.org/?mlat=${g[0]}&mlon=${g[1]}#map=17/${g[0]}/${g[1]}` : 'https://www.openstreetmap.org/search?query=' + encodeURIComponent(v)); else if (b.dataset.act === 'gmaps') open_('https://www.google.com/maps/search/?api=1&query=' + encodeURIComponent(g ? g.join(',') : v)); else if (b.dataset.act === 'mailto') location.href = 'mailto:' + v; else if (b.dataset.act === 'open') open_(/^[a-z]+:/i.test(v) ? v : 'https://' + v); else practical.copy(v).then(ok => { if (!ok) return; b.title = D.tCopied; b.style.color = 'var(--accent)'; setTimeout(() => b.style.color = '', 800); }); }
  });
  form.addEventListener('click', e => { const x = e.target.closest('.chip b'); if (x && form.contains(x)) { x.closest('.chip').remove(); refresh(); } });
  $('#ed-lock').onclick = () => { mode('edit'); f.summary.focus(); };
  $('#ed-reset').onclick = () => { if (ruleMode) openRuleEditor(ruleMode); else fill(current, openDate); };
  /* the day line, drawn like a metro map: each timed event of the day is a line in its calendar's colour from its start
     to its end station (.ln); events at the same time take their own track (greedy, at most 5); the titles (.blk, in
     the event's colour) stand at their start station, pushed down so none covers another. The whole day, 0-24 h at
     DL_H px an hour, scrolls inside the box (.dl-in); opening puts the event at hand (else the form's start, an all-day
     one 8:00) in the middle. An event from the night before starts at 0:00. A hidden day line (#ed-tl, form.js) asks for
     nothing; showing it draws it. Only the newest call draws (dlSeq): a date typed digit by digit asks once per digit */
  const DL_H = 36; let dlSeq = 0;
  async function dayline() {
    const seq = ++dlSeq, box = $('#ed-dayline'), day = f.date ? f.date.value : ''; if (!box || !day || ed.classList.contains('no-tl')) return; // (counted first: an older call still in flight must not draw either)
    const py = m => Math.min(24 * DL_H, Math.max(0, m / 60 * DL_H)), hm = t => +t.slice(0, 2) * 60 + +t.slice(3, 5);
    let h = ''; for (let i = 1; i < 24; i++) h += `<span class="h" style="top:${i * DL_H}px">${i}:00</span>`;
    let mid = !(f.all_day && f.all_day.checked) && /^\d\d:\d\d/.test(f.from ? f.from.value : '') ? hm(f.from.value) : 8 * 60, low = 24 * DL_H;
    try {
      const end = addDays(day, 1);
      // the page's calendar filter (state.cals: '' = all, '-' = none), plus the calendar of the event at hand
      const own = f.key.value.split('/').slice(0, 2).join('/'), cs = state.cals && own && !state.cals.split(',').includes(own) ? (state.cals === '-' ? own : state.cals + ',' + own) : state.cals;
      const r = await practical.req(`/api/events?start=${day}&end=${end}${cs ? '&cals=' + encodeURIComponent(cs) : ''}`); if (seq !== dlSeq) return; const evs = r.ok ? r.data : [];
      const L = evs.filter(ev => !ev.all_day).map(ev => { const s = ev.start.slice(0, 10) < day ? 0 : hm(ev.start.slice(11)); return { ev, s, e: ev.end.slice(0, 10) > day ? 1440 : Math.max(s, hm(ev.end.slice(11))), me: f.key.value === `${ev.principal}/${ev.cal_id}/${ev.object_id}` }; })
        .filter(x => x.e > x.s || x.ev.start.slice(0, 10) === day).sort((a, b) => a.s - b.s || b.e - a.e); // (one that ended at 0:00 is not on the day)
      let run = [], until = -1; // the cluster of events that overlap one another, and where it ends
      for (const x of L) { if (x.s >= until) run = []; const used = run.filter(o => o.e > x.s).map(o => o.col); let c = 0; while (used.includes(c) && c < 4) c++; x.col = c; run.push(x); until = Math.max(until, x.e); }
      const tracks = Math.max(1, ...L.map(x => x.col + 1)), lx = 40 + tracks * 11; let last = -99;
      for (const x of L) {
        const c = x.ev.color && !x.me && CSS.supports('color', x.ev.color) ? `--c:${esc(x.ev.color)}` : '', y = Math.max(0, py(x.s) - 7, last + 14), tip = `${x.ev.start.slice(11, 16)}–${x.ev.end.slice(11, 16)} ${x.ev.summary}`; last = y;
        if (x.me) mid = (x.s + x.e) / 2; low = Math.max(low, y + 14);
        h += `<i class="ln${x.me ? ' me' : ''}" style="top:${py(x.s)}px;height:${py(x.e) - py(x.s)}px;left:${38 + x.col * 11}px;${c}"></i><div class="blk${x.me ? ' me' : ''}" style="top:${y}px;left:${lx}px;${c}" title="${esc(tip)}"><time>${x.ev.start.slice(0, 10) < day ? '←' : ''}${x.ev.start.slice(11, 16)}</time> ${esc(x.ev.summary)}</div>`; // ← from the night before, drawn from 0:00
      }
    } catch (_) {}
    box.innerHTML = `<div class="dl-in" style="height:${low}px">${h}</div>`; // (titles pushed past 24:00 still inside)
    // centred once laid out (a dialog that is still hidden has no height yet)
    const centre = () => { box.scrollTop = py(mid) - box.clientHeight / 2; };
    if (box.clientHeight) centre(); else requestAnimationFrame(centre);
  }
  /* the save request of the dialog as it is now (also the conflict's "overwrite", with the ETag it fetched; extra:
     confirm_visibility once the writer asked) */
  async function put(extra) {
    const body = { ...(TASK ? taskBody() : eventBody()), ...extra }, key = f.key.value;
    const r = await practical.req(key ? API + '/' + key : API, { method: key ? 'PUT' : 'POST', json: body });
    if (!r.ok) return r;
    const res = r.data || {}, saved = key || (res.id && body.principal && body.cal_id ? `${body.principal}/${body.cal_id}/${res.id}` : null);
    ntInvalidate(); ctInvalidate(); close(); load(false, saved);
    return r;
  }
  // Enter in a field and a click on Save (or two clicks) are one save: W.run hands the second the first one's answer
  form.addEventListener('submit', e => {
    e.preventDefault();
    flush();
    W.run(ruleMode ? ruleSave : put);
  });
  $('#ed-delete').onclick = async () => { if (W.busy) return; if (ruleMode) { if (await ask(T.delRule)) W.run(ruleDelete); return; } if (!await ask(TASK ? T.delTask : T.del)) return;
    W.run(async () => { const r = await practical.req(API + '/' + f.key.value, { method: 'DELETE' }); if (r.ok) { close(); load(false); } return r; }); };
  /* a session that ended while the dialog held unsaved work (practical.authLost): after the login the page comes back here, and so does
     the work — an event or task, or a rule (as it would be saved, with the event it was opened from and its way back). The page is told
     what the dialog holds (rusticalHost.restored), so its own layout keeps it open (the tasks page closes a dialog it did not open). */
  practical.keep(() => {
    if (ed.hidden || !touched || !dirty()) return;
    practical.stash('ed-' + ED_SCOPE, ruleMode ? { rule: ruleMode, r: ruleOf(), ret: ruleReturn, back: ruleBack } : { date: openDate, isNew: form.classList.contains('new'), d: snapDetail() });
  });
  practical.guard(() => !ed.hidden && touched && dirty());
  setTimeout(() => { // after the page's own start (the tasks page fills its docked new-task form first; a ?rule= link opens its rule)
    const k = practical.unstash('ed-' + ED_SCOPE); if (!k) return;
    soloEnd();
    if (k.rule) { // the rule editor builds itself from RULES: the kept rule stands in for the stored one while it does
      const stored = RULES[k.rule]; RULES[k.rule] = k.r; openRuleEditor(k.rule); if (stored) RULES[k.rule] = stored; else delete RULES[k.rule];
      ruleReturn = k.ret || null; ruleBack = ruleBack || k.back || null;
    } else { fill(k.d, k.date, k.isNew); mode('edit'); }
    baseline = ''; touched = true; updateButtons(); ed.hidden = false; if (k.rule) $('#ed-delete').disabled = !RULES[k.rule]; else dayline(); toast(tr('js-req-restored'));
    if (window.rusticalHost && rusticalHost.restored) rusticalHost.restored(k.rule || k.isNew ? null : f.key.value);
  });

  // rule(): a rule over a page that loaded the dialog itself (the settings' categories): no way back but closing, the
  // focus into the dialog and, on closing, back to what had it
  window.rusticalEd = { open: openEditor, rule: name => { if (ed.hidden) opener = document.activeElement; openRuleEditor(name); $('#ed-close').focus(); } };
  { const sp = new URLSearchParams(location.search), rp = sp.get('rule'); if (rp) { const b = sp.get('back'); ruleBack = b && /^\/(?![/\\])/.test(b) ? b : null; openRuleEditor(rp); } } // ?back=: a path of this site only, the link may come from anywhere
