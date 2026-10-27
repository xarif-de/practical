// js/editor/cards.js — part of the editor IIFE (src/bundle.rs; shares its scope): card visibility, buttons, the "when" line, refresh, view/edit mode, task stamps and body
  // cards: the "when" card always, the others when they hold data or were added in edit mode
  // `.aux` inputs (rule interval, count, until, raw rule) only qualify the rule and never count as data on their own
  // a CLASS other than the default (public) counts as data: the status card shows itself for it
  const hasData = card => $$('input:not([type=hidden]):not(.aux):not(.mirror), textarea, select.dat', card).some(i => i.value.trim() !== '') || !!$('.pchips .pchip', card) || $$('.rows .row', card).length > 0 || (card.contains(f.class) && !['', 'PUBLIC'].includes(f.class.value));
  function cards() { const edit = form.classList.contains('mode-edit'); $$('.card', form).forEach(c => { c.hidden = !(c.hasAttribute('data-always') || (edit && c.hasAttribute('data-always-edit')) || hasData(c) || (edit && c.dataset.added)); });
    // a task that is simply open has no progress to show: STATUS:NEEDS-ACTION is the absence of one
    const pc = TASK && $('.card[data-card="progress"]', form);
    if (pc && !pc.dataset.added) pc.hidden = !(f.status.value && f.status.value !== 'NEEDS-ACTION') && !(+f.percent.value > 0) && !f.completed_date.value; /* data-always-edit: the categories card is the quick pick while editing, in view mode only with data */ $$('.addmenu [data-card-add]', form).forEach(b => b.disabled = !$(`.card[data-card="${b.dataset.cardAdd}"]`, form).hidden); soloCards(); }
  function addCard(name) { const c = $(`.card[data-card="${name}"]`, form); c.dataset.added = '1'; c.hidden = false; cards(); return c; }
  const serialize = () => $$('input:not([type=hidden]):not(.mirror), select, textarea', form).map(e => e.type === 'checkbox' ? (e.checked ? '1' : '0') : e.value).join('|') + '|' + chips().join(',');
  function updateButtons() { const changed = sig() !== baseline, busy = W.busy; $('#ed-save').disabled = !changed || busy; $('#ed-reset').disabled = (!changed && form.classList.contains('new')) || busy; $('#ed-delete').disabled = form.classList.contains('mode-view') || form.classList.contains('new') || busy; }
  function taskLine() {
    const [start] = getStamp('start'), [due] = getStamp('due');
    const box = $('#ed-whenline');
    const one = v => v ? (v.slice(11, 16) === '00:00' ? fmtD(v) : fmtDT(v)) : '';
    box.textContent = due ? (start ? one(start) + ' – ' + one(due) : one(due)) : start ? one(start) : '';
    const pt = $('#ed-progress-text');
    if (pt) {
      const P = $('#ed-progress-card').dataset, parts = [];
      if (stVal(f.status)) parts.push(stText(f.status));
      if (+f.percent.value > 0 && +f.percent.value < 100) parts.push(P.tViewPct.replace('{n}', f.percent.value));
      const [done] = getStamp('completed');
      if (done) parts.push(P.tViewDone.replace('{at}', fmtDT(done)));
      pt.textContent = parts.join(' · ');
    }
    pctOut();
  }
  function whenLine() { if (TASK) return taskLine(); const s = f.date.value; if (!s) return; const e = f.end_date.value || s, multi = e > s; const t = f.all_day.checked ? (multi ? `${fmtD(s)} – ${fmtD(e)}` : fmtD(s)) + ' · ' + D.tAllday : multi ? `${fmtD(s)} ${f.from.value} – ${fmtD(e)} ${f.to.value}` : `${fmtD(s)} · ${f.from.value} – ${f.to.value}`; $('#ed-whenline').textContent = t; }
  const grow = () => { const t = f.description; t.style.height = 'auto'; t.style.height = (t.scrollHeight + 2) + 'px'; }; // the note shows its whole text in both modes
  function refresh() { if (refreshQ) { cancelAnimationFrame(refreshQ); refreshQ = 0; } whenLine(); rrSync(); grow(); alTimeline(); if (ruleMode || Object.keys(RFX).length || RFXL.alarms.length || RFXL.links.length) { if (ruleMode) { ALARMS.forEach(a => { a._mode ||= 'default'; }); LK.links.forEach(l => { l._mode ||= 'default'; }); } rulesMark(); } /* footers follow added or removed entries */ soloCards(); updateButtons(); }
  function mode(m) { if (m === 'edit' && form.classList.contains('ro')) m = 'view'; /* a read-only calendar's entry (d.read_only): view mode only */ form.classList.toggle('mode-view', m === 'view'); form.classList.toggle('mode-edit', m === 'edit'); ed.classList.toggle('mode-edit', m === 'edit'); cards(); lkRender(); ntClosePop(); if (m === 'view') ntRender(); else { grow(); ntDetect(); } if (m === 'edit') rrRender(); /* the strip measures itself: it must be visible */ refresh(); requestAnimationFrame(fitAddrLines); }
  // all-day: the API end is the exclusive DTEND; the editor shows the last day (empty = one day)
  const addDays = (s, n) => { const e = new Date(s + 'T00:00:00Z'); e.setUTCDate(e.getUTCDate() + n); return e.toISOString().slice(0, 10); }; // UTC arithmetic: no zone shift
  const lastDay = d => { const s = addDays(d.end.slice(0, 10), -1); return s > d.start.slice(0, 10) ? s : ''; };
  const nextDay = s => addDays(s, 1);
  // a task carries an optional start and an optional due, each with an optional time
  function setStamp(name, v, allDay) {
    const dEl = f[name + '_date'], tEl = f[name + '_time'];
    if (!dEl) return;
    dEl.value = v ? String(v).slice(0, 10) : '';
    tEl.value = v && !allDay ? String(v).slice(11, 16) : '';
  }
  function getStamp(name) {
    const dEl = f[name + '_date'], tEl = f[name + '_time'];
    if (!dEl || !dEl.value) return [null, true];
    return [dEl.value + (tEl.value ? 'T' + tEl.value + ':00' : 'T00:00:00'), !tEl.value];
  }
  const pctOut = () => { const o = $('#ed-pct-out'); if (o && f.percent) o.textContent = f.percent.value + ' %'; };
  function taskBody() {
    const [principal, cal_id] = f.calendar.value.split('/');
    const [start, startAll] = getStamp('start'), [due, dueAll] = getStamp('due'), [done] = getStamp('completed');
    return {
      principal, cal_id, summary: f.summary.value, description: f.description.value, location: f.location.value,
      start, start_all_day: startAll, due, due_all_day: dueAll,
      status: f.status ? f.status.value : '', percent: f.percent ? +f.percent.value : null, completed_at: done,
      priority: f.priority && f.priority.value ? +f.priority.value : null, class: f.class ? f.class.value : '',
      categories: chips(), color: colorOf(), geo: geoOf(),
      alarms: alarmsOf(), ...linksOf(), etag: f.etag.value,
    };
  }
