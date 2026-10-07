// js/editor/recurrence.js — part of the editor IIFE (src/bundle.rs; shares its scope): the recurrence model
  // ---- recurrence: the rule is a model painted on a scrollable week strip — the month in full
  // with a neighbouring week on each side and separators around it; a click on a day toggles the pattern
  // (weekday, day of month) or, in the exception mode, strikes an occurrence or adds an extra date. Interval
  // and end are two pills with popovers; the end date is picked on the strip. Rules the strip cannot paint
  // (BYSETPOS, several ordinals, BYHOUR …) stay a raw text that is saved as typed. The strip owns the wheel:
  // one gesture moves at most to the neighbouring month, the next needs a short pause. ----
  const RC = $x('#ed-repeat-card'), RT = RC.dataset, rrBox = $x('#ed-rr');
  const DAYS = ['MO', 'TU', 'WE', 'TH', 'FR', 'SA', 'SU'], RR_FREQS = ['DAILY', 'WEEKLY', 'MONTHLY', 'YEARLY'];
  const DOWL = WEEKDAYS_LONG;
  const blankRR = () => ({ freq: '', interval: 1, byday: [], mode: 'date', bymonthday: null, ordinal: 1, ordday: 'TU', bymonth: null, end: { kind: 'never', count: 10, until: '' }, exdates: [], rdates: [], raw: '' });
  let RR = blankRR(), rrMode = 'pattern', rrPicking = false, rrFocus = null, rrLocked = false;
  const rrIso = fmt;
  const rrWd = d => (d.getDay() + 6) % 7; // 0 = Monday
  const rrStart = () => parse((f.date.value || today).slice(0, 10));
  // a stored rule → the model; parts the strip cannot paint keep the rule as raw text
  function rrParse(r, untilLocal) {
    const m = blankRR(); if (!r) return m;
    const kv = {}; r.replace(/^RRULE:/i, '').split(';').forEach(p => { const [k, v] = p.split('='); if (k) kv[k.toUpperCase()] = (v || '').toUpperCase(); });
    const raw = () => Object.assign(blankRR(), { freq: 'RAW', raw: r });
    if (!RR_FREQS.includes(kv.FREQ)) return raw();
    if (Object.keys(kv).some(k => !['FREQ', 'INTERVAL', 'BYDAY', 'BYMONTHDAY', 'BYMONTH', 'COUNT', 'UNTIL', 'WKST'].includes(k))) return raw();
    m.freq = kv.FREQ; m.interval = +(kv.INTERVAL || 1) || 1;
    if (kv.BYDAY) { const one = kv.BYDAY.match(/^([+-]?[1-4]|-1)([A-Z]{2})$/); if (one && m.freq !== 'WEEKLY') { m.mode = 'ordinal'; m.ordinal = +one[1]; m.ordday = one[2]; } else if (m.freq === 'WEEKLY' && /^([A-Z]{2})(,[A-Z]{2})*$/.test(kv.BYDAY)) m.byday = DAYS.filter(d => kv.BYDAY.split(',').includes(d)); else return raw(); }
    if (kv.BYMONTHDAY) { if (!/^\d{1,2}$/.test(kv.BYMONTHDAY) || m.mode === 'ordinal') return raw(); m.bymonthday = +kv.BYMONTHDAY; }
    if (kv.BYMONTH) { if (!/^\d{1,2}$/.test(kv.BYMONTH) || m.freq !== 'YEARLY') return raw(); m.bymonth = +kv.BYMONTH; }
    if (kv.COUNT) m.end = { ...m.end, kind: 'count', count: +kv.COUNT || 1 };
    if (kv.UNTIL) m.end = { ...m.end, kind: 'until', until: untilLocal || `${kv.UNTIL.slice(0, 4)}-${kv.UNTIL.slice(4, 6)}-${kv.UNTIL.slice(6, 8)}` };
    return m;
  }
  const rrBuild = () => { const r = RR; if (!r.freq) return null; if (r.freq === 'RAW') return r.raw.trim() || null; const p = ['FREQ=' + r.freq]; if (r.interval > 1) p.push('INTERVAL=' + r.interval);
    if (r.freq === 'WEEKLY' && r.byday.length) p.push('BYDAY=' + r.byday.join(','));
    if ((r.freq === 'MONTHLY' || r.freq === 'YEARLY') && r.mode === 'ordinal') p.push('BYDAY=' + r.ordinal + r.ordday);
    if ((r.freq === 'MONTHLY' || r.freq === 'YEARLY') && r.mode === 'date' && r.bymonthday) p.push('BYMONTHDAY=' + r.bymonthday);
    if (r.freq === 'YEARLY' && r.bymonth) p.push('BYMONTH=' + r.bymonth);
    if (r.end.kind === 'count') p.push('COUNT=' + r.end.count); if (r.end.kind === 'until' && r.end.until) p.push('UNTIL=' + r.end.until.replace(/-/g, '')); // the server turns a bare date into the local end of day
    return p.join(';'); };
  const rrUnit = () => RT.tUnits.split('|')[RR_FREQS.indexOf(RR.freq)] || '';
  const rrOrdTxt = () => RT.tOrd.split('|')[RR.ordinal === -1 ? 4 : RR.ordinal - 1];
  // the sentence of the view mode: "Wöchentlich am Dienstag, 6-mal"
  function rrSentence() { const r = RR; if (!r.freq) return ''; if (r.freq === 'RAW') return r.raw; const longs = RT.tFreqsLong.split('|'), s0 = rrStart();
    let t = r.interval > 1 ? D.tEvery.replace('{n}', r.interval) + ' ' + rrUnit() : longs[RR_FREQS.indexOf(r.freq) + 1];
    if (r.freq === 'WEEKLY' && r.byday.length) t += ' ' + D.tOn + ' ' + r.byday.map(d => DOWL[DAYS.indexOf(d)]).join(', ');
    if (r.freq === 'MONTHLY' || r.freq === 'YEARLY') { t += ' ' + D.tOn + ' ' + (r.mode === 'ordinal' ? `${rrOrdTxt()} ${DOWL[DAYS.indexOf(r.ordday)]}` : `${r.bymonthday || s0.getDate()}.`); if (r.freq === 'YEARLY') t += ' ' + RT.tIn + ' ' + MON[(r.bymonth || s0.getMonth() + 1) - 1]; }
    if (r.end.kind === 'count') t += ', ' + plural(D.tCount, r.end.count).replace('{n}', r.end.count); if (r.end.kind === 'until' && r.end.until) t += ', ' + D.tUntil + ' ' + fmtD(r.end.until);
    return t.charAt(0).toUpperCase() + t.slice(1); }
  // occurrences for the strip (the server's caldata is authoritative; this covers what the strip can paint)
  function rrOccurrences(limit) { const r = RR, s0 = rrStart(), out = new Set(); if (!r.freq || r.freq === 'RAW') return out;
    const max = r.end.kind === 'count' ? r.end.count : Infinity, until = r.end.kind === 'until' && r.end.until ? r.end.until : null;
    const nth = (d, n, day) => { if (DAYS[rrWd(d)] !== day) return false; const dom = d.getDate(); if (n > 0) return Math.ceil(dom / 7) === n; const last = new Date(d.getFullYear(), d.getMonth() + 1, 0).getDate(); return Math.ceil((last - dom + 1) / 7) === -n; };
    const week0 = Math.floor((s0.getTime() / 864e5 + 3) / 7); let n = 0;
    for (let i = 0; i < limit && n < max; i++) { const d = new Date(s0); d.setDate(s0.getDate() + i); const iso = rrIso(d); if (until && iso > until) break; let ok = false;
      if (r.freq === 'DAILY') ok = i % r.interval === 0;
      else if (r.freq === 'WEEKLY') { const days = r.byday.length ? r.byday : [DAYS[rrWd(s0)]]; ok = days.includes(DAYS[rrWd(d)]) && (Math.floor((d.getTime() / 864e5 + 3) / 7) - week0) % r.interval === 0; }
      else if (r.freq === 'MONTHLY') { const mi = (d.getFullYear() - s0.getFullYear()) * 12 + d.getMonth() - s0.getMonth(); ok = mi % r.interval === 0 && (r.mode === 'ordinal' ? nth(d, r.ordinal, r.ordday) : d.getDate() === (r.bymonthday || s0.getDate())); }
      else if (r.freq === 'YEARLY') { ok = (d.getFullYear() - s0.getFullYear()) % r.interval === 0 && d.getMonth() + 1 === (r.bymonth || s0.getMonth() + 1) && (r.mode === 'ordinal' ? nth(d, r.ordinal, r.ordday) : d.getDate() === (r.bymonthday || s0.getDate())); }
      if (ok) { out.add(iso); n++; } }
    return out; }
  const rrMirror = () => { f.rrule_json.value = RR.freq ? JSON.stringify(RR) : (RR.exdates.length || RR.rdates.length ? JSON.stringify(RR) : ''); };
  function rrSync() { if (TASK) return; rrMirror(); $('#ed-rrule-text').textContent = rrSentence(); $('#ed-exdates').classList.toggle('empty', !RR.exdates.length); $('#ed-rdates').classList.toggle('empty', !RR.rdates.length); } // the view lists show only when set
  // popovers for the pills: a list of choices or a number entry
  let rrOpenPop = null; const rrClosePop = () => { if (rrOpenPop) { rrOpenPop.remove(); rrOpenPop = null; document.removeEventListener('click', rrOutside, true); } };
  function rrOutside(e) { if (rrOpenPop && !rrOpenPop.contains(e.target)) rrClosePop(); }
  function rrPop(anchor, items, cur, onpick, extra) { rrClosePop(); const p = document.createElement('div'); p.className = 'rrpop'; p.innerHTML = (extra || '') + items.map(i => `<button type="button" data-v="${esc(i.v)}"${String(i.v) === String(cur) ? ' aria-current="true"' : ''}>${esc(i.label)}</button>`).join(''); const r = anchor.getBoundingClientRect(); document.body.appendChild(p); placeUnder(p, anchor, r); rrOpenPop = p; /* kept inside the dialog box */ p.addEventListener('click', e => { const b = e.target.closest('button[data-v]'); if (b) { rrClosePop(); onpick(b.dataset.v); } }); setTimeout(() => document.addEventListener('click', rrOutside, true), 0); return p; }
  function rrAskNumber(anchor, label, value, on) { const p = rrPop(anchor, [], null, () => {}, `<div class="q">${esc(label)}</div><input type="number" min="1" max="999" value="${value}"><button type="button" class="ok">${esc(RT.tApply)}</button>`); const i = $('input', p); i.focus(); i.select(); const ok = () => { const n = +i.value; if (n >= 1) on(n); rrClosePop(); }; i.onkeydown = e => { if (e.key === 'Enter') { e.preventDefault(); ok(); } }; $('.ok', p).onclick = ok; }
  const rrSet = () => { rrRender(); refresh(); };
  // a short eased glide of a scroll container (200–420 ms); any input stops it; with reduced motion it jumps
  function rrGlide(el) { let raf = 0, on = false; const stop = () => { cancelAnimationFrame(raf); on = false; };
    const to = target => { stop(); if (matchMedia('(prefers-reduced-motion: reduce)').matches) { el.scrollTop = target; return; } const from = el.scrollTop, dist = target - from; if (Math.abs(dist) < 1) return; const t0 = performance.now(), ms = Math.min(420, 200 + Math.abs(dist) * 0.8); on = true;
      const step = now => { const k = Math.min(1, (now - t0) / ms), e = k < .5 ? 4 * k * k * k : 1 - Math.pow(-2 * k + 2, 3) / 2; el.scrollTop = from + dist * e; if (k < 1) raf = requestAnimationFrame(step); else on = false; }; raf = requestAnimationFrame(step); };
    return { to, stop, active: () => on }; }
