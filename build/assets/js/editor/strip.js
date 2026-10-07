// js/editor/strip.js — part of the editor IIFE (src/bundle.rs; shares its scope): the recurrence week strip, exception and extra dates
  // ---- the strip: NWEEKS rows of ROWH px, starting a year before the event on a Monday; the separators are the
  // only variable spacing (a GAP margin above / below at most three rows), so every row position is arithmetic ----
  const ROWH = 32, GAP = 16, NWEEKS = 130;
  const GESTURE_PAUSE = 40; // ms without wheel input that end a gesture — the next one may reach the next month
  const SETTLE_MS = 120;    // ms after the last input before the strip glides to the nearest month
  const WHEEL_DRAG = 0.8;   // share of the wheel distance the strip follows
  const rrW0 = () => { const d = rrStart(); d.setDate(d.getDate() - rrWd(d) - 364); return d; };
  function rrRender() {
    if (TASK) return; // a task has no recurrence card in this editor
    rrBox.innerHTML = ''; ed.classList.toggle('picking', rrPicking);
    const s0 = rrStart(), freqs = RT.tFreqs.split('|'), longs = RT.tFreqsLong.split('|');
    const isRaw = RR.freq === 'RAW';
    rrBox.insertAdjacentHTML('beforeend', `<div class="rrsegs"><span class="seg" data-w="freq">${[''].concat(RR_FREQS).map((v, i) => `<button type="button" data-v="${v}" aria-pressed="${RR.freq === v}" title="${esc(longs[i])}">${esc(freqs[i])}</button>`).join('')}${isRaw ? `<button type="button" data-v="RAW" aria-pressed="true" title="${esc(RT.tCustom)}">${esc(RT.tCustomShort)}</button>` : ''}</span></div>`);
    $('[data-w=freq]', rrBox).onclick = e => { const b = e.target.closest('button'); if (!b) return; RR.freq = b.dataset.v; if (RR.freq === 'WEEKLY' && !RR.byday.length) RR.byday = [DAYS[rrWd(s0)]]; rrSet(); };
    if (!RR.freq) rrBox.insertAdjacentHTML('beforeend', `<div class="rrbottom"><button class="pillbtn ghost" type="button" disabled>${ico('ellipsis')}<span>${esc(RT.tInterval)}</span></button><button class="pillbtn ghost" type="button" disabled>${ico('to-line')}<span>${esc(RT.tEnd)}</span></button></div><hr class="rrsep">`); // no frequency yet: the row stays, inert
    if (RR.freq && !isRaw) {
      const u = rrUnit(), each = RT.tEach.split('|')[RR_FREQS.indexOf(RR.freq)];
      const shortD = d => `${d.slice(8, 10)}.${d.slice(5, 7)}.${d.slice(2, 4)}`;
      const endLabel = () => RR.end.kind === 'count' ? plural(D.tCount, RR.end.count).replace('{n}', RR.end.count) : RR.end.kind === 'until' && RR.end.until ? D.tUntil + ' ' + shortD(RR.end.until) : RT.tNoEnd;
      rrBox.insertAdjacentHTML('beforeend', `<div class="rrbottom"><button class="pillbtn${RR.interval > 1 ? '' : ' ghost'}" type="button" data-s="i">${ico('ellipsis')}<span>${esc(RR.interval > 1 ? D.tEvery.replace('{n}', RR.interval) + ' ' + u : each)}</span></button><button class="pillbtn${RR.end.kind === 'never' ? ' ghost' : ''}" type="button" data-s="e">${ico('to-line')}<span>${esc(endLabel())}</span></button></div><hr class="rrsep">`);
      $('.rrbottom', rrBox).onclick = e => { const b = e.target.closest('.pillbtn'); if (!b) return;
        if (b.dataset.s === 'i') rrPop(b, [1, 2, 3, 4, 6].map(n => ({ v: n, label: n === 1 ? each : D.tEvery.replace('{n}', n) + ' ' + u })).concat({ v: 'n', label: RT.tN }), RR.interval, v => { if (v === 'n') rrAskNumber(b, RT.tIntQ.replace('{u}', u), RR.interval, n => { RR.interval = n; rrSet(); }); else { RR.interval = +v; rrSet(); } });
        else rrPop(b, [{ v: 'never', label: RT.tNoEnd }, { v: 'count', label: RT.tEndCount + ' …' }, { v: 'until', label: RT.tEndUntil + ' …' }], RR.end.kind, v => { if (v === 'count') rrAskNumber(b, RT.tCountQ, RR.end.count, n => { RR.end = { ...RR.end, kind: 'count', count: n }; rrSet(); }); else if (v === 'until') { rrPicking = true; rrRender(); } else { RR.end = { ...RR.end, kind: 'never' }; rrSet(); } }); };
    }
    if (isRaw) { rrBox.insertAdjacentHTML('beforeend', `<div class="rrhint">${esc(RT.tCustomHint)}</div><div class="rrraw"><input type="text" value="${esc(RR.raw)}" placeholder="FREQ=…"></div>`); $('.rrraw input', rrBox).onchange = e => { RR.raw = e.target.value.trim(); rrSet(); }; }
    if (rrPicking) { rrBox.insertAdjacentHTML('beforeend', `<div class="pickmsg">${ico('today')}<span>${esc(RT.tPick)}</span><button class="btn" type="button">${esc(RT.tCancel)}</button></div>`); $('.pickmsg .btn', rrBox).onclick = () => { rrPicking = false; rrRender(); }; }
    else rrBox.insertAdjacentHTML('beforeend', `<div class="rrhint">${esc(rrMode === 'pattern' ? RT.tHintPattern : RT.tHintEx)}</div>`);
    // week rows
    const occ = rrOccurrences(1100), ex = new Set(RR.exdates), rd = new Set(RR.rdates), untilD = RR.end.kind === 'until' ? RR.end.until : null, W0 = rrW0();
    if (rrFocus == null) rrFocus = s0.getFullYear() * 12 + s0.getMonth();
    const cal = document.createElement('div'); cal.className = 'calstrip'; cal.innerHTML = `<div class="calhead"><b></b><div class="dows">${DOW.map(d => `<span>${d}</span>`).join('')}</div></div><div class="calscroll"><div class="weeks${rrPicking ? ' pick' : ''}"></div></div>`;
    const weeks = $('.weeks', cal), scroller = $('.calscroll', cal), head = $('.calhead b', cal); let html = ''; const startIso = rrIso(s0);
    for (let i = 0; i < NWEEKS; i++) { const ws = new Date(W0); ws.setDate(W0.getDate() + i * 7); let row = '';
      for (let k = 0; k < 7; k++) { const d = new Date(ws); d.setDate(ws.getDate() + k); const iso = rrIso(d), mi = d.getFullYear() * 12 + d.getMonth();
        const cls = [iso === startIso ? 'start' : '', ex.has(iso) ? 'ex' : rd.has(iso) ? 'rd' : occ.has(iso) ? 'occ' : '', iso === untilD ? 'until' : '', iso < startIso ? 'past' : '', mi !== rrFocus ? 'dim' : '', k >= 5 ? 'we' : ''].filter(Boolean).join(' ');
        row += `<b data-d="${iso}" data-m="${mi}" class="${cls}">${d.getDate()}</b>`; }
      html += `<div class="wrow">${row}</div>`; }
    weeks.innerHTML = html;
    // geometry without layout reads
    const rowOf = d => Math.floor(Math.round((d - W0) / 864e5) / 7); // rounding absorbs the DST hour
    const monthRows = mi => { const y = Math.floor(mi / 12), m = mi % 12; return [rowOf(new Date(y, m, 1)), rowOf(new Date(y, m + 1, 0))]; }; // first and last week holding a day of the month
    const inRange = mi => { const [a, b] = monthRows(mi); return a >= 0 && b < NWEEKS; };
    const rowMonth = i => { const t = new Date(W0); t.setDate(W0.getDate() + i * 7 + 3); return t.getFullYear() * 12 + t.getMonth(); }; // the heading follows the week's Thursday
    let seps = { before: [], after: [] }; // rows carrying the GAP margin above / below
    const rowTop = i => i * ROWH + GAP * (seps.before.filter(r => r <= i).length + seps.after.filter(r => r < i).length);
    const rowAt = y => { let i = 0; while (i + 1 < NWEEKS && rowTop(i + 1) <= y) i++; return i; };
    const setSeps = (mi, nextM) => { const [a, b] = monthRows(mi); seps = { before: [a], after: [b] }; if (nextM != null && inRange(nextM)) { const [na, nb] = monthRows(nextM); if (nextM > mi) seps.after.push(nb); else seps.before.push(na); } // while scrolling only the far edge of the neighbour is pre-drawn
      $$('.wrow.sep-before, .wrow.sep-after', weeks).forEach(el => el.classList.remove('sep-before', 'sep-after')); seps.before.forEach(r => weeks.children[r].classList.add('sep-before')); seps.after.forEach(r => weeks.children[r].classList.add('sep-after')); };
    const snapTop = mi => Math.max(0, rowTop(monthRows(mi)[0]) - GAP - ROWH);
    const glide = rrGlide(scroller);
    const focusMonth = (mi, animate, nextM) => { if (!inRange(mi)) return; rrFocus = mi;
      const anchor = rowAt(scroller.scrollTop + scroller.clientHeight / 2), before = rowTop(anchor);
      setSeps(mi, nextM); if (animate) scroller.scrollTop += rowTop(anchor) - before; // the separators moved the rows: keep the row under the centre in place
      const top = snapTop(mi); scroller.style.height = `${rowTop(monthRows(mi)[1]) + ROWH + GAP + ROWH - top}px`;
      head.textContent = `${MON[mi % 12]} ${Math.floor(mi / 12)}`; $$('b[data-m]', weeks).forEach(el => el.classList.toggle('dim', +el.dataset.m !== mi));
      if (!animate || Math.abs(scroller.scrollTop - top) <= GAP) scroller.scrollTop = top; else glide.to(top); }; // a gap-sized rest is set at once: a glide would fight an ongoing gesture
    const nearestMonth = () => { const st = scroller.scrollTop; let best = rrFocus, dist = Math.abs(snapTop(rrFocus) - st); for (const mi of [rrFocus - 1, rrFocus + 1]) if (inRange(mi) && Math.abs(snapTop(mi) - st) < dist) { best = mi; dist = Math.abs(snapTop(mi) - st); } return best; }; // past half the way the neighbour wins
    const settle = () => focusMonth(nearestMonth(), true, null);
    // the strip owns the wheel: one gesture reaches at most the neighbouring month and snaps in on arrival
    let lastWheel = 0, gestureFrom = null, gestureDir = 0, settleT = 0;
    scroller.addEventListener('wheel', e => { e.preventDefault(); glide.stop(); const now = e.timeStamp; // the input's own time: a busy main thread must not split a gesture
      if (now - lastWheel > GESTURE_PAUSE || gestureFrom == null) { gestureFrom = rrFocus; gestureDir = 0; } lastWheel = now;
      const dir = Math.sign(e.deltaY); if (dir && dir !== gestureDir) { gestureDir = dir; setSeps(rrFocus, rrFocus + dir); }
      const lo = snapTop(inRange(gestureFrom - 1) ? gestureFrom - 1 : gestureFrom), hi = snapTop(inRange(gestureFrom + 1) ? gestureFrom + 1 : gestureFrom);
      const delta = e.deltaY * (e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? scroller.clientHeight : 1) * WHEEL_DRAG;
      const target = Math.min(hi, Math.max(lo, scroller.scrollTop + delta)); scroller.scrollTop = target;
      clearTimeout(settleT); if ((target === hi && dir > 0) || (target === lo && dir < 0)) settle(); else settleT = setTimeout(settle, SETTLE_MS); }, { passive: false });
    scroller.addEventListener('pointerdown', glide.stop); scroller.addEventListener('touchstart', glide.stop, { passive: true });
    scroller.addEventListener('scroll', () => { const mi = rowMonth(rowAt(scroller.scrollTop + scroller.clientHeight / 2)); head.textContent = `${MON[mi % 12]} ${Math.floor(mi / 12)}`; /* the heading follows at once */ if (glide.active() || performance.now() - lastWheel < SETTLE_MS) return; clearTimeout(settleT); settleT = setTimeout(settle, SETTLE_MS); });
    weeks.addEventListener('click', e => { const b = e.target.closest('b[data-d]'); if (!b) return; const iso = b.dataset.d, d = parse(iso);
      if (rrPicking) { if (iso < startIso) return; RR.end = { ...RR.end, kind: 'until', until: iso }; rrPicking = false; rrSet(); return; }
      if (isRaw) return;
      if (rrMode === 'pattern') { if (!RR.freq) RR.freq = 'WEEKLY'; if (RR.freq === 'WEEKLY') { const day = DAYS[rrWd(d)]; RR.byday = RR.byday.includes(day) ? RR.byday.filter(x => x !== day) : DAYS.filter(x => RR.byday.includes(x) || x === day); if (!RR.byday.length) RR.byday = [day]; } else if (RR.freq === 'MONTHLY' || RR.freq === 'YEARLY') { RR.mode = 'date'; RR.bymonthday = d.getDate(); if (RR.freq === 'YEARLY') RR.bymonth = d.getMonth() + 1; } }
      else { if (occ.has(iso)) RR.exdates = ex.has(iso) ? RR.exdates.filter(x => x !== iso) : RR.exdates.concat(iso); else if (ex.has(iso)) RR.exdates = RR.exdates.filter(x => x !== iso); else RR.rdates = rd.has(iso) ? RR.rdates.filter(x => x !== iso) : RR.rdates.concat(iso); }
      rrSet(); });
    rrBox.appendChild(cal); focusMonth(rrFocus, false, null);
    const lg = RT.tLegend.split('|');
    rrBox.insertAdjacentHTML('beforeend', `<div class="rrlegend"><span><i style="background:var(--accent)"></i>${esc(lg[0])}</span><span><i style="background:var(--bg-3)"></i>${esc(lg[1])}</span><span><i style="background:var(--c2)"></i>${esc(lg[2])}</span><span><i style="outline:2px solid var(--c5);outline-offset:-2px"></i>${esc(lg[3])}</span>${untilD ? `<span><i style="background:var(--c4)"></i>${esc(lg[4])}</span>` : ''}</div>`);
    const md = RT.tMode.split('|');
    rrBox.insertAdjacentHTML('beforeend', `<hr class="rrsep"><div class="rrmode"><span>${esc(md[0])}</span><span class="seg" data-w="mode"><button type="button" data-v="pattern" aria-pressed="${rrMode === 'pattern'}">${esc(md[1])}</button><button type="button" data-v="ex" aria-pressed="${rrMode === 'ex'}">${esc(md[2])}</button></span></div>`);
    $('[data-w=mode]', rrBox).onclick = e => { const b = e.target.closest('button'); if (b) { rrMode = b.dataset.v; rrRender(); } };
    rrMirror();
  }
  // view mode rows for exceptions and extra dates (edited on the strip)
  const addDate = (list, iso) => { const r = addRow(list); $('.txt', r).textContent = iso ? fmtD(iso) : ''; return r; };
  function rrInit(rule, untilLocal, exdates, rdates) { RR = rrParse(rule, untilLocal); RR.exdates = (exdates || []).map(x => x.slice(0, 10)); RR.rdates = (rdates || []).map(x => x.slice(0, 10)); rrMode = 'pattern'; rrPicking = false; rrFocus = null; rrClosePop(); RR.exdates.forEach(x => addDate('exdates', x)); RR.rdates.forEach(x => addDate('rdates', x)); rrRender(); }
  const rrDates = list => RR[list].map(d => f.all_day.checked ? d : `${d}T${f.from.value || '00:00'}:00`);
  addEventListener('keydown', e => { if (e.key === 'Escape' && rrPicking && !ed.hidden) { e.stopImmediatePropagation(); rrPicking = false; rrRender(); } }, true);
