// js/editor/location.js — part of the editor IIFE (src/bundle.rs; shares its scope): the location card
  // ---- location: ONE editable field — the address or the coordinates — the other part is
  // derived from it (geocoder) and shown as a chip below, never edited by hand. The user may swap
  // which part is primary, or drop a part; errors of the derivation show in red. ----
  const sugg = $('#ed-sugg'), ring = $('#ed-ring'), locin = $('#ed-locin'), locsub = $('#ed-loc-sub'), LT = locsub.dataset;
  let suggTimer = 0, suggRaf = 0, suggSeq = 0, suggItems = [], suggIdx = -1, deriveSeq = 0;
  const SUGG_DELAY = 2000;
  const LOC = { addr: true, coords: false, primary: 'addr', errAddr: false, errCoords: false, invalid: false, pending: false };
  const other = () => LOC.primary === 'addr' ? 'coords' : 'addr';
  const coordsText = () => geoOf() ? `${(+f.lat.value).toFixed(5)}, ${(+f.lon.value).toFixed(5)}` : '';
  const parseCoords = t => { const m = t.match(/^\s*(-?\d+(?:[.,]\d+)?)\s*[,; ]\s*(-?\d+(?:[.,]\d+)?)\s*$/); if (!m) return null; const a = +m[1].replace(',', '.'), b = +m[2].replace(',', '.'); return Math.abs(a) <= 90 && Math.abs(b) <= 180 ? [a, b] : null; };
  const ringStop = () => { cancelAnimationFrame(suggRaf); ring.setAttribute('hidden', ''); ring.classList.remove('busy'); }; // SVG elements have no `hidden` property
  const ringCount = ms => { const c = $('circle', ring), t0 = performance.now(); ring.removeAttribute('hidden'); ring.classList.remove('busy'); const step = () => { const k = Math.min(1, (performance.now() - t0) / ms); c.style.strokeDashoffset = (50.27 * k).toFixed(2); if (k < 1) suggRaf = requestAnimationFrame(step); }; step(); };
  const ringBusy = on => { if (on) { ring.removeAttribute('hidden'); ring.classList.add('busy'); } else ringStop(); };
  const suggClose = () => { sugg.hidden = true; sugg.innerHTML = ''; suggItems = []; suggIdx = -1; locin.setAttribute('aria-expanded', 'false'); };
  const suggShow = items => { suggItems = items; suggIdx = -1; sugg.innerHTML = items.length ? items.map((p, i) => `<button type="button" role="option" data-i="${i}">${p.label.replace(/</g, '&lt;')}</button>`).join('') : `<div class="none">${T.suggNone}</div>`; sugg.hidden = false; locin.setAttribute('aria-expanded', 'true'); };
  function locRender() {
    const P = LOC.primary, O = other();
    $('#ed-loc-lbl').textContent = P === 'addr' ? LT.tAddr : LT.tGeo;
    locin.placeholder = P === 'addr' ? D.tLocation || '' : LT.tCoordsHint;
    if (document.activeElement !== locin) locin.value = P === 'addr' ? f.location.value : coordsText();
    const err = $('#ed-loc-err'); err.textContent = LOC.invalid ? LT.tErrInvalid : ''; err.hidden = !LOC.invalid; $('#ed-locp').classList.toggle('err', LOC.invalid);
    $('#ed-loc-swap').hidden = !LOC[O];
    $('[data-geo=map]', form).classList.toggle('hot', LOC.errAddr || LOC.errCoords);
    let h = '';
    if (LOC[O]) { const e = O === 'addr' ? LOC.errAddr : LOC.errCoords, v = O === 'addr' ? f.location.value : coordsText(); h = `<span class="chip${e ? ' err' : ''}" title="${O === 'addr' ? LT.tAddr : LT.tGeo}"><svg class="ico" aria-hidden="true"><use href="${practical.sprite}#${O === 'addr' ? 'i-pin' : 'i-target'}"/></svg><span class="txt">${(v || (LOC.pending ? '…' : '–')).replace(/</g, '&lt;')}</span><b data-locrm="${O}" title="${O === 'addr' ? LT.tRmAddr : LT.tRmCoords}">×</b></span>` + (e ? `<span class="errtxt">${O === 'addr' ? LT.tErrAddr : LT.tErrCoords}</span>` : ''); }
    else h = `<button class="chip ghost" type="button" data-locadd="${O}">${ico('plus')} ${O === 'addr' ? LT.tAddAddr : LT.tAddCoords}</button>`;
    locsub.innerHTML = h;
    const addr = f.location.value.trim(); $('#ed-loc-view').hidden = !addr; $('#ed-geo-view').hidden = !geoOf();
    $('#ed-addr-lines').innerHTML = addr.split(/,\s*/).filter(Boolean).map(l => `<span class="ln">${l.replace(/</g, '&lt;')}</span>`).join('');
    $('#ed-geo-txt').textContent = coordsText();
    requestAnimationFrame(fitAddrLines);
  }
  // a line wider than the card shrinks its text until it fits (never below 10 px)
  function fitAddrLines() { $$('#ed-addr-lines .ln').forEach(l => { l.style.fontSize = ''; l.classList.remove('wrap'); const w = l.clientWidth; if (!w || l.scrollWidth <= w) return; const fs = 15 * w / l.scrollWidth - 0.3; if (fs >= 9) l.style.fontSize = fs.toFixed(1) + 'px'; else { l.style.fontSize = '11px'; l.classList.add('wrap'); } }); } // below 9 px the line wraps instead of becoming unreadable
  async function geoSearch(q, near) { const u = new URL('/api/geo/search', location.origin); u.searchParams.set('q', q); if (near) { u.searchParams.set('lat', near[0]); u.searchParams.set('lon', near[1]); } const r = await fetch(u); if (!r.ok) throw new Error(r.status); return r.json(); }
  async function geoReverse(lat, lon) { const r = await fetch(`/api/geo/reverse?lat=${(+lat).toFixed(6)}&lon=${(+lon).toFixed(6)}`); if (r.status === 404) return null; if (!r.ok) throw new Error(r.status); return r.json(); }
  // derive the secondary part from the primary one (only when that part is in use)
  async function deriveCoords() { if (!LOC.coords || LOC.primary !== 'addr') return; const q = f.location.value.trim(); const seq = ++deriveSeq; if (!q) { LOC.errCoords = false; locRender(); return; } LOC.pending = true; locRender(); ringBusy(true); try { const items = await geoSearch(q, geoOf()); if (seq !== deriveSeq) return; if (items.length) { f.lat.value = items[0].lat.toFixed(6); f.lon.value = items[0].lon.toFixed(6); LOC.errCoords = false; } else LOC.errCoords = true; } catch (_) { if (seq === deriveSeq) LOC.errCoords = true; } finally { if (seq === deriveSeq) { LOC.pending = false; ringBusy(false); locRender(); refresh(); } } }
  async function deriveAddr() { if (!LOC.addr || LOC.primary !== 'coords') return; const g = geoOf(); const seq = ++deriveSeq; if (!g) { locRender(); return; } LOC.pending = true; locRender(); /* no ring: the address chip shows "…", the ring belongs to the address field */ try { const p = await geoReverse(g[0], g[1]); if (seq !== deriveSeq) return; if (p) { f.location.value = p.label; LOC.errAddr = false; } else LOC.errAddr = true; } catch (_) { if (seq === deriveSeq) LOC.errAddr = true; } finally { if (seq === deriveSeq) { LOC.pending = false; locRender(); refresh(); } } }
  // the primary field: address → suggestions after a pause, the pick or a leave derives the coordinates;
  // coordinates → validated, then the address is derived
  let primaryTouched = false;
  const commitPrimary = () => { if (!primaryTouched) return; primaryTouched = false; if (LOC.primary === 'addr') { f.location.value = locin.value.trim(); deriveCoords(); } else { const c = parseCoords(locin.value); LOC.invalid = !c && locin.value.trim() !== ''; if (c) { f.lat.value = c[0].toFixed(6); f.lon.value = c[1].toFixed(6); deriveAddr(); } else if (!locin.value.trim()) { f.lat.value = ''; f.lon.value = ''; } locRender(); } refresh(); };
  const suggPick = i => { const p = suggItems[i]; if (!p) return; locin.value = p.label; f.location.value = p.label; primaryTouched = false; if (LOC.coords) { f.lat.value = p.lat.toFixed(6); f.lon.value = p.lon.toFixed(6); LOC.errCoords = false; } suggClose(); ringStop(); locRender(); syncDot(); refresh(); };
  locin.addEventListener('input', () => { primaryTouched = true; clearTimeout(suggTimer); const q = locin.value.trim();
    if (LOC.primary === 'addr') { f.location.value = q; if (q.length < 3) { ringStop(); suggClose(); refresh(); return; } ringCount(SUGG_DELAY); suggTimer = setTimeout(async () => { if (document.activeElement !== locin) return; const seq = ++suggSeq; ring.classList.add('busy'); try { const items = await geoSearch(q, geoOf()); if (seq !== suggSeq || document.activeElement !== locin) return; suggShow(items); } catch (_) { suggClose(); } finally { if (seq === suggSeq) ringStop(); } }, SUGG_DELAY); }
    else { LOC.invalid = false; $('#ed-locp').classList.remove('err'); ringStop(); suggTimer = setTimeout(() => { if (document.activeElement === locin) commitPrimary(); }, SUGG_DELAY); } // coordinates commit after the same pause, but without the ring: it signals the address lookup only
    refresh(); });
  locin.addEventListener('keydown', e => { if (e.key === 'Enter') { e.preventDefault(); if (!sugg.hidden && suggIdx >= 0) suggPick(suggIdx); else locin.blur(); return; } if (sugg.hidden) return; if (e.key === 'ArrowDown' || e.key === 'ArrowUp') { e.preventDefault(); suggIdx = (suggIdx + (e.key === 'ArrowDown' ? 1 : -1) + suggItems.length) % Math.max(1, suggItems.length); $$('button', sugg).forEach((b, i) => b.setAttribute('aria-selected', i === suggIdx ? 'true' : 'false')); } else if (e.key === 'Escape') { e.stopPropagation(); suggClose(); } });
  sugg.addEventListener('mousedown', e => { const b = e.target.closest('button[data-i]'); if (b) { e.preventDefault(); suggPick(+b.dataset.i); } });
  locin.addEventListener('blur', () => { clearTimeout(suggTimer); suggSeq++; ringStop(); suggClose(); commitPrimary(); });
  locsub.addEventListener('mousedown', e => { if (e.target.closest('[data-locrm],[data-locadd]')) e.preventDefault(); }); /* keep the focus: a blur would re-render the chips under the pointer and swallow the click */
  locsub.addEventListener('click', e => { const b = e.target.closest('[data-locrm],[data-locadd]'); if (!b) return; clearTimeout(suggTimer); commitPrimary(); /* typed value first, the derivation reads the model */
    if (b.dataset.locrm) { LOC[b.dataset.locrm] = false; if (b.dataset.locrm === 'addr') { f.location.value = ''; LOC.errAddr = false; } else { f.lat.value = ''; f.lon.value = ''; LOC.errCoords = false; } }
    if (b.dataset.locadd) { LOC[b.dataset.locadd] = true; if (b.dataset.locadd === 'coords') deriveCoords(); else deriveAddr(); }
    locRender(); refresh(); });
  function locSwap() { const O = other(); if (!LOC[O]) return; LOC.primary = O; LOC.invalid = false; primaryTouched = false; locRender(); }
  // called by fill(): derive the parts from the event
  function locInit() { LOC.addr = !!f.location.value.trim() || !geoOf(); LOC.coords = !!geoOf(); LOC.primary = LOC.addr ? 'addr' : 'coords'; LOC.errAddr = LOC.errCoords = LOC.invalid = LOC.pending = false; primaryTouched = false; clearTimeout(suggTimer); ringStop(); suggClose(); locRender(); }
