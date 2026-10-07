  // typing: the save/reset buttons follow at once (Enter must find them right), the derived parts (when line,
  // rule sentence, note height, reminder strip, footers — two of them force a layout) once per frame; whatever
  // reads them (save, close) runs a pending refresh first
  form.addEventListener('input', () => { updateButtons(); if (!refreshQ) refreshQ = requestAnimationFrame(refresh); });
  form.addEventListener('change', e => { const t = e.target; if (t === f.calendar) syncDot(); if (t === f.date) { if (f.end_date.value < f.date.value) f.end_date.value = f.date.value; dayline(); } if (t === f.date || t === f.from || t === f.all_day) { rrFocus = null; rrRender(); } if (!ruleMode && CATS.length && !t.dataset.rv) rulesMark(); refresh(); });
  form.addEventListener('click', async e => {
    const b = e.target.closest('button'); if (!b || !form.contains(b)) return;
    if (b.dataset.add) { const l = b.dataset.add; const r = l === 'alarms' ? alarmAdd() : addDate(l, ''); const i = $('input:not([hidden]), select', r); if (i) i.focus(); refresh(); return; }
    if (b.hasAttribute('data-rm')) { b.closest('.row').remove(); refresh(); return; }
    if (b.dataset.cardAdd) { const c = addCard(b.dataset.cardAdd); b.closest('.addmenu').classList.remove('open'); if (ruleMode) { if (b.dataset.cardAdd === 'where') RM.location = 'default'; if (b.dataset.cardAdd === 'repeat') RM.rrule = 'default'; rulesApply('change'); } /* a one-setting rule card starts as template */ if (b.dataset.cardAdd === 'alarm') $('[data-add=alarms]', c).click(); else if (b.dataset.cardAdd === 'links') $('#ed-lk-url').focus(); else if (b.dataset.cardAdd === 'where') { locInit(); locin.focus(); } else if (b.dataset.cardAdd === 'repeat') { rrFocus = null; rrRender(); } else { const i = $('input:not([type=hidden]), select, textarea', c); if (i) i.focus(); } refresh(); return; }
    if (b.hasAttribute('data-card-rm')) { if (!await ask(D.tRmcard)) return; const c = b.closest('.card'); $$('input:not([type=hidden]), textarea', c).forEach(i => i.value = ''); $$('select.dat', c).forEach(s => s.value = ''); $$('.rows .row, .tags .chip', c).forEach(x => x.remove()); if (c.dataset.card === 'where') locInit(); if (c.dataset.card === 'alarm') alarmsInit([]); if (c.dataset.card === 'repeat') rrInit(null, null, [], []); if (c.dataset.card === 'status') stRender(); if (c.dataset.card === 'links') linksInit([], []); if (c.dataset.card === 'note') { ntRender(); ntFound.innerHTML = ''; } if (c.dataset.card === 'cats') { catsInit([]); rulesApply('change'); } if (ruleMode) { SCALARS.filter(k => FLD[k][0] === c.dataset.card).forEach(k => { RM[k] = 'off'; }); rulesApply('change'); } /* a removed rule card is off */ delete c.dataset.added; c.hidden = true; cards(); refresh(); return; }
    if (b.dataset.geo === 'swap') { locSwap(); return; }
    if (b.dataset.geo === 'map') { openMap(); return; }
    if (b.dataset.act) { const row = b.closest('.row'); const v = row ? (w(row, 'email') || w(row, 'uri')).value : f.location.value, open_ = u => window.open(u, '_blank', 'noopener'), g = geoOf(); if (b.dataset.act === 'osm') open_(g ? `https://www.openstreetmap.org/?mlat=${g[0]}&mlon=${g[1]}#map=17/${g[0]}/${g[1]}` : 'https://www.openstreetmap.org/search?query=' + encodeURIComponent(v)); else if (b.dataset.act === 'gmaps') open_('https://www.google.com/maps/search/?api=1&query=' + encodeURIComponent(g ? g.join(',') : v)); else if (b.dataset.act === 'mailto') location.href = 'mailto:' + v; else if (b.dataset.act === 'open') open_(/^[a-z]+:/i.test(v) ? v : 'https://' + v); else practical.copy(v).then(ok => { if (!ok) return; b.title = D.tCopied; b.style.color = 'var(--accent)'; setTimeout(() => b.style.color = '', 800); }); }
  });
  form.addEventListener('click', e => { const x = e.target.closest('.chip b'); if (x && form.contains(x)) { x.closest('.chip').remove(); refresh(); } });
  $('#ed-lock').onclick = () => { mode('edit'); f.summary.focus(); };
  $('#ed-reset').onclick = () => { if (ruleMode) openRuleEditor(ruleMode); else fill(current, openDate); };
  async function dayline() {
    const box = $('#ed-dayline'); const day = f.date ? f.date.value : ''; if (!box || !day) return;
    let h = ''; for (let i = 0; i < 7; i++) h += `<span class="h" style="top:${(i * 14.3).toFixed(1)}%">${7 + i * 2}:00</span>`;
    try {
      const nd = parse(day); nd.setDate(nd.getDate() + 1); const end = `${nd.getFullYear()}-${pad(nd.getMonth() + 1)}-${pad(nd.getDate())}`;
      const r = await practical.req(`/api/events?start=${day}&end=${end}`); const evs = r.ok ? r.data : [];
      for (const ev of evs) { if (ev.all_day || ev.start.slice(0, 10) !== day) continue; const s = ev.start.slice(11, 16).split(':'), e2 = ev.end.slice(11, 16).split(':'); const top = ((+s[0] + s[1] / 60) - 7) / 14 * 100, hh = Math.max(3, ((+e2[0] + e2[1] / 60) - (+s[0] + s[1] / 60)) / 14 * 100); const me = f.key.value === `${ev.principal}/${ev.cal_id}/${ev.object_id}`; h += `<div class="blk${me ? ' me' : ''}" style="top:${top.toFixed(1)}%;height:${hh.toFixed(1)}%;--c:${esc(ev.color)}">${esc(ev.summary)}</div>`; }
    } catch (_) {}
    box.innerHTML = h;
  }
  // the save request of the dialog as it is now (also the conflict's "overwrite", with the ETag it fetched)
  async function put() {
    const body = TASK ? taskBody() : eventBody(), key = f.key.value;
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
  // a session that ended while the dialog held unsaved work (practical.authLost): after the login the page comes back here, and so does the work
  practical.keep(() => { if (!ed.hidden && !ruleMode && touched && dirty()) practical.stash('ed-' + ED_SCOPE, { date: openDate, isNew: form.classList.contains('new'), d: snapDetail() }); });
  practical.guard(() => !ed.hidden && touched && dirty());
  setTimeout(() => { // after the page's own start (the tasks page fills its docked new-task form first)
    const k = practical.unstash('ed-' + ED_SCOPE); if (!k) return;
    soloEnd(); fill(k.d, k.date, k.isNew); mode('edit'); baseline = ''; touched = true; updateButtons(); ed.hidden = false; dayline(); toast(tr('js-req-restored'));
  });

  window.rusticalEd = { open: openEditor };
  { const sp = new URLSearchParams(location.search), rp = sp.get('rule'); if (rp) { const b = sp.get('back'); ruleBack = b && /^\/(?![/\\])/.test(b) ? b : null; openRuleEditor(rp); } } // ?back=: a path of this site only, the link may come from anywhere
