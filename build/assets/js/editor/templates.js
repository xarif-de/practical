// js/editor/templates.js — part of the editor IIFE (src/bundle.rs; shares its scope): templates from past events while a new title is typed
  /* templates: while a NEW event's title is typed, up to three past events with a like title (GET
     /api/events/suggest, the most frequent title first). Picking one takes every attribute but the
     date: the day stays the dialog's, times, duration, series pattern and absolute reminders move
     along with it; what names dates of the old event (UNTIL, EXDATE/RDATE), the attendees' replies and
     dismissed reminders stay behind. Embedded files are fetched and sent again as new ones. */
  let tplPop = null, tplHits = [], tplSel = -1, tplTimer = 0, tplReq = 0;
  const tplOn = () => !TASK && !ruleMode && !solo && form.classList.contains('new');
  const tplClose = () => { clearTimeout(tplTimer); tplReq++; if (tplPop) tplPop.remove(); tplPop = null; tplHits = []; tplSel = -1; };
  const tplMark = () => $$('.lk-hit', tplPop).forEach((b, i) => b.setAttribute('aria-selected', i === tplSel ? 'true' : 'false'));
  function tplShow(hits) {
    if (tplPop) tplPop.remove(); tplPop = null; tplSel = -1; tplHits = hits; if (!hits.length) return;
    tplPop = document.createElement('div'); tplPop.className = 'rrpop tplpop'; tplPop.setAttribute('role', 'listbox');
    tplPop.innerHTML = `<div class="q">${esc(D.tTplHead)}</div>` + hits.map((h, i) => `<button type="button" class="lk-hit" role="option" data-h="${i}" aria-selected="false"><svg class="ico" style="color:${esc(h.color)}"><use href="${practical.sprite}#i-calendar"/></svg><span>${esc(h.summary)}</span><small>${esc([D.tTplCount.replace('{n}', h.count), D.tTplLast.replace('{date}', fmtD(h.last) + (h.time ? ' ' + h.time : '')), h.cal_name].filter(Boolean).join(' · '))}</small></button>`).join('');
    document.body.appendChild(tplPop);
    placeUnder(tplPop, f.summary);
    tplPop.addEventListener('mousedown', e => e.preventDefault()); // the title keeps the focus
    tplPop.addEventListener('click', e => { const b = e.target.closest('.lk-hit'); if (b) tplUse(tplHits[+b.dataset.h]); });
  }
  f.summary.addEventListener('input', () => {
    const q = f.summary.value.trim(); tplClose(); if (!tplOn() || q.length < 2) return;
    const n = tplReq; tplTimer = setTimeout(async () => { const r = await practical.req('/api/events/suggest?q=' + encodeURIComponent(q)), hits = r.ok ? r.data : []; if (n === tplReq && tplOn() && document.activeElement === f.summary) tplShow(hits); }, 200);
  });
  f.summary.addEventListener('keydown', e => {
    if (!tplPop) return;
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') { e.preventDefault(); const n = tplHits.length + 1; tplSel = ((tplSel + (e.key === 'ArrowDown' ? 2 : 0)) % n + n) % n - 1; tplMark(); } // -1: back in the title
    else if (e.key === 'Enter' && tplSel >= 0) { e.preventDefault(); tplUse(tplHits[tplSel]); }
    else if (e.key === 'Escape') { e.stopPropagation(); tplClose(); } // closes the list, not the dialog
  });
  f.summary.addEventListener('blur', () => setTimeout(() => { if (document.activeElement !== f.summary) tplClose(); }, 150));
  const b64Of = async url => { const r = await fetch(url); if (!r.ok) throw new Error(r.status); const b = await r.blob(); return new Promise((ok, no) => { const rd = new FileReader(); rd.onload = () => ok(String(rd.result).replace(/^data:[^,]*,/, '')); rd.onerror = no; rd.readAsDataURL(b); }); };
  async function tplUse(h) {
    tplClose();
    const got = await practical.req(API + '/' + h.key); if (!got.ok) { if (got.kind !== 'auth') $('#ed-error').textContent = got.msg; return; } const d = got.data;
    const day = f.date.value || state.date, delta = Math.round((parse(day) - parse(d.start.slice(0, 10))) / 864e5), move = s => s && addDays(s.slice(0, 10), delta) + s.slice(10);
    const links = await Promise.all(d.links.map(async l => { if (!l.embedded) return l; try { return { ...l, data: await b64Of(`${API}/${h.key}/attach/${l.bin_index}`), bin_index: null, keep: false }; } catch (_) { return null; } }));
    const b0 = baseline, cal = f.calendar.value;
    fill({ ...d, start: move(d.start), end: move(d.end), rrule: d.rrule ? d.rrule.split(';').filter(p => !/^UNTIL=/i.test(p)).join(';') : null, rrule_until: null, exdates: [], rdates: [],
      attendees: d.attendees.map(p => ({ ...p, status: p.status ? 'NEEDS-ACTION' : '' })), alarms: d.alarms.map(a => ({ ...a, idx: null, at: move(a.at), acknowledged: null })), links: links.filter(Boolean) }, day, true);
    if (!f.calendar.value) { f.calendar.value = cal; syncDot(); } // the old calendar is gone: the one chosen stays
    baseline = b0; touched = true; updateButtons(); f.summary.focus(); dayline();
  }
