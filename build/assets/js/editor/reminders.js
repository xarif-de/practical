// js/editor/reminders.js — part of the editor IIFE (src/bundle.rs; shares its scope): list rows from <template>, the reminders strip
  // ---- list rows from <template>: reminders, links, exceptions, extra dates ----
  const addRow = list => { const box = $(`[data-list="${list}"]`, form); box.appendChild($('template', box).content.cloneNode(true)); return box.lastElementChild; };
  const w = (row, k) => $(`[data-w=${k}]`, row);
  const setSel = (sel, v, unit = '') => { v = String(v); if (![...sel.options].some(o => o.value === v)) sel.add(new Option(v + unit, v)); sel.value = v; };
  // ---- reminders: a model of alarms rendered as a timeline strip (markers snap to the
  // usual offsets), a bar of per-user preset chips (a click adds or removes the matching alarm) and
  // one expandable entry per alarm whose settings follow the chosen kind and trigger. The model is
  // mirrored into the hidden `alarms_json` input so dirty tracking and reset work like for any field. ----
  const AL = $('#ed-alarm-card'), AT = AL.dataset, alBox = $('[data-list="alarms"]', AL), tline = $('#ed-tline'), apre = $('#ed-apre');
  let ALARMS = [], alSeq = 0;
  /* a share's calendar (`#ed-cal` option data-share) keeps only its owner's reminders and drops the writer's
     (rustical_ical visibility.rs `with_stored_alarms`): there the card is inert with its hint and cannot be added;
     a save that still carries reminders (an own event moved there, a category rule) is asked about first (the
     server's 422 confirm-alarms, req.js writer). Not in rule mode: a rule belongs to no calendar. */
  const alShared = () => { const o = f.calendar && f.calendar.selectedOptions[0]; return !ruleMode && !!o && o.hasAttribute('data-share'); };
  function alShareSync() { const sh = alShared(); AL.classList.toggle('shared', sh); $x('#ed-al-share', AL).hidden = !sh; }
  const AL_ACTS = ['DISPLAY', 'AUDIO', 'EMAIL'], AL_ICO = { DISPLAY: '#i-monitor-dot', AUDIO: '#i-bell', EMAIL: '#i-mail' }, AL_PROX = ['ARRIVE', 'DEPART', 'CONNECT', 'DISCONNECT'];
  const SNAPS = (() => { try { const j = JSON.parse($('#alarm-snaps-data').textContent); if (Array.isArray(j.start) && j.start.length) return j; } catch (_) {} return { start: [2, 5, 15, 30, 60, 180, 480, 1440, 2880, 4320, 10080, 20160], end: [5, 10, 15, 30, 60] }; })(); // per-user snap points of the strip (settings)
  const SNAP_START = SNAPS.start, SNAP_END_ALL = SNAPS.end, AL_UNITS = [1, 60, 1440, 10080, 43200]; // a "month" is 30 days: iCalendar durations have no months
  const DEFAULT_PRESETS = [0, 10].map(m => ({ action: 'DISPLAY', minutes: m, related_end: false, repeat: 0, duration_minutes: 0, attendees: [], attach: '', proximity: '' }));
  let PRESETS = (() => { try { const p = JSON.parse($('#alarm-presets-data').textContent || 'null'); return Array.isArray(p) ? p : DEFAULT_PRESETS; } catch (_) { return DEFAULT_PRESETS; } })();
  const newAlarm = o => ({ idx: null, action: 'DISPLAY', minutes: 15, related_end: false, at: null, description: '', summary: '', repeat: 0, duration_minutes: 0, attendees: [], attach: '', proximity: '', acknowledged: null, ...o, _id: ++alSeq });
  // a preset is the setting without text, subject and acknowledgement; an absolute trigger cannot be one
  const presetSig = a => a.at ? null : JSON.stringify([!!a.related_end, a.minutes, a.action, a.repeat, a.duration_minutes, a.proximity || '', a.attach || '', [...(a.attendees || [])].sort()]);
  const fromPreset = p => newAlarm({ action: p.action, minutes: p.minutes, related_end: !!p.related_end, repeat: p.repeat || 0, duration_minutes: p.duration_minutes || 0, attendees: [...(p.attendees || [])], attach: p.attach || '', proximity: p.proximity || '' });
  const matchesPreset = a => !a.at && PRESETS.some(p => presetSig(p) === presetSig(a));
  async function savePresets() { const r = await practical.send('/api/alarm-presets', 'PUT', PRESETS); if (!r.ok && r.kind !== 'auth') $('#ed-error').textContent = r.msg; }
  const amount = m => { m = Math.abs(m); if (m && m % 10080 === 0) return plural(AT.tWeeks, m / 10080).replace('{n}', m / 10080); if (m && m % 1440 === 0) return plural(D.tDays, m / 1440).replace('{n}', m / 1440); if (m && m % 60 === 0) return D.tHours.replace('{n}', m / 60); return D.tMin.replace('{n}', m); };
  const whenText = a => { if (a.at) return fmtDT(a.at); if (a.minutes === 0) return a.related_end ? AT.tAtEndTxt : AT.tAtStart; return amount(a.minutes) + ' ' + (a.minutes > 0 ? (a.related_end ? D.tBeforeEnd : D.tBefore) : (a.related_end ? AT.tAfterEnd : AT.tAfter)); };
  const proxText = a => a.proximity ? AT.tProxVals.split('|')[AL_PROX.indexOf(a.proximity)] || a.proximity : '';
  const alarmText = a => { let t = whenText(a); const acts = D.tActs.split('|'); if (a.action === 'AUDIO') t += ' · ' + acts[1]; if (a.action === 'EMAIL') t += ' · ' + acts[2]; if (a.repeat > 0) t += ' · ' + D.tRepeat.replace('{n}', a.repeat) + (a.duration_minutes ? ' ' + AT.tRepeatEvery + ' ' + amount(a.duration_minutes) : ''); if (a.proximity) t += ' · ' + proxText(a); if (a.action === 'EMAIL' && a.attendees.length) t += ' · ' + AT.tToTxt + ' ' + a.attendees.join(', '); if (a.description && a.description !== f.summary.value) t += ' – ' + a.description; return t; };
  const extrasOf = a => [a.repeat > 0 ? D.tRepeat.replace('{n}', a.repeat) + (a.duration_minutes ? ' ' + AT.tRepeatEvery + ' ' + amount(a.duration_minutes) : '') : '', proxText(a), a.action === 'EMAIL' ? (a.attendees.length ? AT.tToTxt + ' ' + a.attendees.join(', ') : AT.tToMissing) : '', a.description ? '„' + a.description + '“' : ''].filter(Boolean).join(' · ');
  const alIco = id => `<svg class="ico" aria-hidden="true"><use href="${practical.sprite}${id}"/></svg>`;
  const unitOf = m => { m = Math.abs(m); for (const u of [...AL_UNITS].reverse()) if (m && m % u === 0) return u; return 1; };
  const fmtNum = n => String(Math.round(n * 100) / 100).replace('.', ',');
  // the event on the strip: start and length in minutes from the "when" card
  const evStart = () => {
    if (TASK) { const [due] = getStamp('due'), [st] = getStamp('start'); return new Date(due || st || (today + 'T00:00:00')); }
    return new Date((f.date.value || today) + 'T' + (f.all_day.checked ? '00:00' : (f.from.value || '00:00')) + ':00');
  };
  const evLen = () => {
    if (TASK) { const [due] = getStamp('due'), [st] = getStamp('start'); return due && st ? Math.max(1, Math.round((new Date(due) - new Date(st)) / 60000)) : 1; }
    const s = evStart(); const e = f.all_day.checked ? new Date(nextDay(f.end_date.value || f.date.value || today) + 'T00:00:00') : new Date((f.end_date.value || f.date.value || today) + 'T' + (f.to.value || f.from.value || '00:00') + ':00'); return Math.max(1, Math.round((e - s) / 60000)); };
  const minutesBefore = a => a.at ? Math.round((evStart() - new Date(a.at)) / 60000) : a.related_end ? a.minutes - evLen() : a.minutes;
  // axis: every snap point owns a zone of the same width (the strip is ordinal, not to scale) — the
  // marker follows the pointer and the value changes at regular intervals; an alarm off the grid sits
  // between its neighbours
  const alCands = () => { const len = evLen(), ends = SNAP_END_ALL.filter(m => m < len);
    const order = [...SNAP_START].reverse().map(m => ({ related_end: false, minutes: m })).concat([{ related_end: false, minutes: 0 }], [...ends].reverse().map(m => ({ related_end: true, minutes: m })), [{ related_end: true, minutes: 0 }]);
    const step = 1 / (order.length + 1); return order.map((c, i) => ({ ...c, x: step * (i + 1), mb: c.related_end ? c.minutes - len : c.minutes })); };
  const evEdges = () => { const c = alCands(); return [c.find(k => !k.related_end && k.minutes === 0).x, c[c.length - 1].x]; };
  const xa = a => { if (a._dragX != null) return a._dragX; const mb = minutesBefore(a), pts = alCands(); if (mb >= pts[0].mb) return pts[0].x; if (mb <= pts[pts.length - 1].mb) return pts[pts.length - 1].x; for (let i = 0; i < pts.length - 1; i++) { const p = pts[i], q = pts[i + 1]; if (mb <= p.mb && mb >= q.mb) return p.mb === q.mb ? p.x : p.x + (q.x - p.x) * (p.mb - mb) / (p.mb - q.mb); } return pts[0].x; };
  const snapX = f => alCands().reduce((b, c) => Math.abs(c.x - f) < Math.abs(b.x - f) ? c : b);
  function alTimeline() {
    if (!form.classList.contains('mode-edit') || AL.hidden) return;
    const color = $('#ed-dot').style.background || '';
    const [EV0, EV1] = evEdges(), cx = alCands();
    let h = `<div class="axis"></div><div class="evb" style="left:${EV0 * 100}%;width:${(EV1 - EV0) * 100}%;--evc:${color}"></div>`;
    h += [60, 1440, 10080].map(m => cx.find(c => !c.related_end && c.minutes === m)).filter(Boolean).map(c => `<span class="tick" style="left:${c.x * 100}%">${esc(amount(c.minutes))}</span>`).join('');
    tline.innerHTML = h;
    { let prev = null; for (const t of $$('.tick', tline).sort((a, b) => a.getBoundingClientRect().left - b.getBoundingClientRect().left)) { const r = t.getBoundingClientRect(); if (prev !== null && r.left < prev + 4) { t.remove(); continue; } prev = r.right; } } // left to right: a label that would run into its kept neighbour is dropped (narrow card)
    const groups = new Map();
    for (const a of ALARMS) { const k = Math.round(xa(a) * 1000); if (!groups.has(k)) groups.set(k, []); groups.get(k).push(a); }
    for (const [k, list] of groups) {
      const g = document.createElement('div'); g.className = 'mkg'; g.style.left = `${k / 10}%`;
      for (const a of list) {
        const mk = document.createElement('div'); mk.className = `mk${a._open || a._drag ? ' sel' : ''}${a.acknowledged ? ' ack' : ''}`; mk.title = alarmText(a); mk.innerHTML = `<span class="lab">${esc(whenText(a))}</span>${alIco(AL_ICO[a.action] || AL_ICO.DISPLAY)}`;
        // a click on the marker opens or closes the entry; a drag only moves the alarm and leaves the entries as they are
        mk.onpointerdown = e => { e.preventDefault(); if (a._locked) { ALARMS.forEach(x => x._open = x === a ? !x._open : false); alRender(); return; } const rect = tline.getBoundingClientRect(); let moved = false;
          const move = ev => { const fr = Math.min(1, Math.max(0, (ev.clientX - rect.left) / rect.width)); const c = snapX(fr); a._dragX = fr; moved = true; if (!(c.related_end === a.related_end && c.minutes === a.minutes && !a.at)) { a.related_end = c.related_end; a.minutes = c.minutes; a.at = null; } a._drag = true; alRender(); refresh(); };
          const up = () => { document.removeEventListener('pointermove', move); document.removeEventListener('pointerup', up); delete a._drag; delete a._dragX; if (!moved) ALARMS.forEach(x => x._open = x === a ? !x._open : false); alRender(); refresh(); };
          document.addEventListener('pointermove', move); document.addEventListener('pointerup', up); };
        g.appendChild(mk);
      }
      tline.appendChild(g);
    }
  }
  const presetLabel = p => { const acts = D.tActs.split('|'); let t = !p.related_end && p.minutes > 0 ? amount(p.minutes) : whenText({ ...p, at: null, attendees: p.attendees || [] }); if (p.action === 'AUDIO') t += ' · ' + acts[1]; if (p.action === 'EMAIL') t += ' · ' + acts[2]; if (p.repeat) t += ` · ${p.repeat}×`; if (p.proximity) t += ' · ' + proxText(p); return t; };
  function alPresets() {
    const order = [...PRESETS].sort((p, q) => (!!p.related_end) - (!!q.related_end) || q.minutes - p.minutes);
    apre.innerHTML = `<span class="lbl">${esc(AT.tPresets)}</span>` + order.map(p => { const sig = presetSig(p); return `<button type="button" data-sig="${esc(sig)}" aria-pressed="${ALARMS.some(a => presetSig(a) === sig)}">${p.action !== 'DISPLAY' ? alIco(AL_ICO[p.action]) : ''}${esc(presetLabel(p))}</button>`; }).join('');
  }
  apre.addEventListener('click', e => { const b = e.target.closest('button[data-sig]'); if (!b) return; const sig = b.dataset.sig; const ex = ALARMS.find(a => presetSig(a) === sig); if (ex) ALARMS = ALARMS.filter(a => a !== ex); else { const p = PRESETS.find(q => presetSig(q) === sig); if (p) ALARMS.push(fromPreset(p)); } alRender(); refresh(); });
  // the settings of one entry: the fields follow the kind (text | sound | subject, message, recipients)
  // and the trigger (number + unit for a relative one, date and time for an absolute one); "after …"
  // only shows when the stored alarm already fires after its anchor (a valid but foreign setting)
  function alDetails(a, row) {
    const rels = AT.tRels.split('|'), units = AT.tUnits.split('|'), acts = D.tActs.split('|');
    const relv = a.at ? 'at' : (a.minutes < 0 ? 'after-' : 'before-') + (a.related_end ? 'end' : 'start');
    const REL_ICO = { 'before-start': '#i-al-bs', 'before-end': '#i-al-be', 'after-start': '#i-al-as', 'after-end': '#i-al-ae', at: '#i-today' };
    const relOpts = [['before-start', rels[0]], ['before-end', rels[1]]].concat(a.minutes < 0 ? [['after-start', rels[2]], ['after-end', rels[3]]] : []).concat([['at', rels[4]]]);
    const u = unitOf(a.minutes), isPreset = matchesPreset(a);
    const d = $('.aldet', row);
    const relLabel = a.at ? rels[4] : a.minutes < 0 ? (a.related_end ? rels[3] : rels[2]) : (a.related_end ? rels[1] : rels[0]);
    const field = (label, inner) => `<div class="field"><span>${esc(label)}</span>${inner}</div>`;
    const seg = (name, opts, cur, icons) => `<div class="segs"><span class="seg" data-w="${name}">${opts.map(([v, l]) => `<button type="button" data-v="${v}" aria-pressed="${v === cur}" title="${esc(l)}" aria-label="${esc(l)}">${alIco(icons[v])}</button>`).join('')}</span></div>`;
    d.innerHTML = `${seg('rel', relOpts, relv, REL_ICO)}
      ${a.at ? field(relLabel, `<input type="datetime-local" data-w="at" value="${esc((a.at || '').slice(0, 16))}" aria-label="${esc(relLabel)}">`) : field(relLabel, `<div class="inl"><input type="text" inputmode="decimal" data-w="n" value="${fmtNum(Math.abs(a.minutes) / u)}" aria-label="${esc(relLabel)}"><select data-w="unit" aria-label="${esc(relLabel)}">${AL_UNITS.map((v, i) => `<option value="${v}" ${v === u ? 'selected' : ''}>${esc(units[i])}</option>`).join('')}</select></div>`)}
      ${field(AT.tRepeat, `<div class="inl"><select data-w="repeat" aria-label="${esc(AT.tRepeat)}">${[0, 1, 2, 3, 5, 10].map(n => `<option value="${n}" ${n === a.repeat ? 'selected' : ''}>${n ? n + '×' : esc(AT.tNever)}</option>`).join('')}</select>${a.repeat > 0 ? `<span class="muted small">${esc(AT.tEvery)}</span><select data-w="every" aria-label="${esc(AT.tEvery)}">${[1, 5, 10, 15, 30, 60].map(n => `<option value="${n}" ${n === a.duration_minutes ? 'selected' : ''}>${esc(amount(n))}</option>`).join('')}</select>` : ''}</div>`)}
      ${seg('act', AL_ACTS.map((v, i) => [v, acts[i]]), a.action, AL_ICO)}
      ${a.action === 'DISPLAY' ? field(AT.tText, `<input type="text" data-w="text" value="${esc(a.description)}" placeholder="${esc(AT.tTextHint)}">`) : ''}
      ${a.action === 'AUDIO' ? field(AT.tSound, `<input type="url" data-w="attach" value="${esc(a.attach)}" placeholder="${esc(AT.tSoundHint)}">`) : ''}
      ${a.action === 'EMAIL' ? field(AT.tSubject, `<input type="text" data-w="subject" value="${esc(a.summary)}" placeholder="${esc(AT.tTextHint)}">`) + field(AT.tBody, `<input type="text" data-w="text" value="${esc(a.description)}" placeholder="${esc(AT.tTextHint)}">`) + field(AT.tTo, `<input type="text" data-w="to" value="${esc(a.attendees.join(', '))}" placeholder="${esc(AT.tToHint)}">`) : ''}
      <div class="foot"><button class="pset" type="button" data-w="pset" aria-pressed="${isPreset}" ${a.at ? 'disabled' : ''} title="${esc(a.at ? AT.tPresetAt : isPreset ? AT.tPresetRm : AT.tPresetAdd)}" aria-label="${esc(a.at ? AT.tPresetAt : isPreset ? AT.tPresetRm : AT.tPresetAdd)}">${alIco('#i-star-h')}</button><span>${esc(a.at ? AT.tPresetAt : AT.tPresetHint)}</span></div>`;
    const w = k => $(`[data-w=${k}]`, d);
    w('rel').onclick = e => { const b = e.target.closest('button'); if (!b) return; const v = b.dataset.v; if (v === 'at') { a.at = a.at || (TASK ? (getStamp('due')[0] || today + 'T09:00:00') : (f.date.value || today) + 'T' + (f.all_day.checked ? '09:00' : (f.from.value || '09:00')) + ':00'); } else { const [dir, anc] = v.split('-'); a.related_end = anc === 'end'; a.at = null; a.minutes = (Math.abs(a.minutes) || 15) * (dir === 'after' ? -1 : 1); } alRender(); refresh(); };
    if (w('n')) { const apply = () => { const n = parseFloat(w('n').value.replace(',', '.')); if (isNaN(n) || n < 0) { w('n').classList.add('err'); return; } a.minutes = Math.sign(a.minutes || 1) * Math.round(n * +w('unit').value); alRender(); refresh(); }; w('n').onchange = apply; w('unit').onchange = apply; w('n').onkeydown = e => { if (e.key === 'Enter') { e.preventDefault(); apply(); } }; }
    if (w('at')) w('at').onchange = () => { if (w('at').value) { a.at = w('at').value + ':00'; alRender(); refresh(); } };
    w('act').onclick = e => { const b = e.target.closest('button'); if (b) { a.action = b.dataset.v; alRender(); refresh(); } };
    if (w('text')) w('text').oninput = () => { a.description = w('text').value; alSync(row, a); };
    if (w('subject')) w('subject').oninput = () => { a.summary = w('subject').value; alSync(row, a); };
    if (w('to')) w('to').onchange = () => { a.attendees = w('to').value.split(/[,;\s]+/).filter(Boolean); alRender(); refresh(); };
    if (w('attach')) w('attach').onchange = () => { a.attach = w('attach').value.trim(); alRender(); refresh(); };
    w('repeat').onchange = () => { a.repeat = +w('repeat').value; a.duration_minutes = a.repeat ? (a.duration_minutes || 10) : 0; alRender(); refresh(); };
    if (w('every')) w('every').onchange = () => { a.duration_minutes = +w('every').value; alRender(); refresh(); };
    w('pset').onclick = () => { if (a.at) return; if (matchesPreset(a)) PRESETS = PRESETS.filter(p => presetSig(p) !== presetSig(a)); else { const { action, minutes, related_end, repeat, duration_minutes, attendees, attach, proximity } = a; PRESETS.push({ action, minutes, related_end, repeat, duration_minutes, attendees: [...attendees], attach, proximity }); } savePresets(); alRender(); };
  }
  // text edits update the row's summary and the mirror without rebuilding the entry (the field keeps its focus)
  const alMirror = () => { f.alarms_json.value = JSON.stringify(ALARMS.map(({ _id, _open, _drag, _dragX, ...a }) => a)); };
  function alSync(row, a) { $('.txt', row).textContent = alarmText(a); $('.extra', row).textContent = extrasOf(a); alMirror(); refresh(); }
  let alBusy = false;
  function alRender() {
    if (alBusy) return; alBusy = true; // a field inside a row commits on blur when its row is rebuilt; that nested render is skipped, the model is current anyway
    try { alRenderNow(); } finally { alBusy = false; }
  }
  function alRenderNow() {
    if (alBox.contains(document.activeElement)) document.activeElement.blur();
    $$('.row', alBox).forEach(r => r.remove());
    for (const a of ALARMS) {
      const r = addRow('alarms'); r.classList.toggle('open', !!a._open); r._alarm = a;
      const tx = $('.txt', r); tx.textContent = alarmText(a); tx.classList.toggle('ack', !!a.acknowledged); $('.sum use', r).setAttribute('href', practical.sprite + (AL_ICO[a.action] || AL_ICO.DISPLAY)); const t = $('.sum .t', r); t.textContent = whenText(a); t.classList.toggle('ack', !!a.acknowledged); $('.extra', r).textContent = extrasOf(a); r.classList.toggle('locked', !!a._locked); if (a._locked && !ruleMode) $$('[data-rm]', r).forEach(x => x.hidden = true); /* a rule entry on "beliebig" is greyed but removable */
      $('.pdot', r).classList.toggle('on', matchesPreset(a)); $('.act', r).setAttribute('aria-pressed', a._open ? 'true' : 'false');
      const d = $('.aldet', r); d.hidden = !a._open; if (a._open) alDetails(a, r);
    }
    $('#ed-alarm-n').textContent = ALARMS.length ? `(${ALARMS.length})` : '';
    alPresets(); alTimeline(); alMirror();
  }
  alBox.addEventListener('click', e => {
    const b = e.target.closest('[data-al-open], [data-rm]'); if (!b) return; e.stopPropagation(); const row = b.closest('.row'), a = row && row._alarm; if (!a) return;
    if (b.hasAttribute('data-rm')) ALARMS = ALARMS.filter(x => x !== a); else ALARMS.forEach(x => x._open = x === a ? !x._open : false);
    alRender(); refresh();
  });
  function alarmsInit(list) { ALARMS = list.map(a => newAlarm({ ...a, attendees: [...(a.attendees || [])] })); alRender(); }
  const alarmAdd = () => { const a = newAlarm(); if (ruleMode) a._mode = 'default'; ALARMS.push(a); alRender(); return $$('.row', alBox).pop(); }; // the new entry stays collapsed; the row header, the dots or the marker open it
  const alarmsOf = () => ALARMS.map(({ _id, _open, _drag, _dragX, _rule, _locked, _mode, ...a }) => ({ ...a, description: a.description.trim(), summary: a.summary.trim(), attach: a.attach.trim() }));
  // people: the organizer is the row with the ORGANIZER role
