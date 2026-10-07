  // ---- map dialog: the crosshair is the position; with the address part in use the address must be
  // determined for a moved map before "apply" — apply writes both parts at once ----
  const gm = $('#geomap'); let map = null, gmSeq = 0, gmMoved = false, gmAddrKnown = false, gmAddrErr = false, gmAddr = '', gmMoving = false;
  const [leafletJs, leafletCss] = ((gm && gm.dataset.leaflet) || '').split(' '); // each file's own versioned URL (editor.html)
  const loadLeaflet = () => new Promise((res, rej) => { if (window.L) return res(); const css = document.createElement('link'); css.rel = 'stylesheet'; css.href = leafletCss; document.head.appendChild(css); const js = document.createElement('script'); js.src = leafletJs; js.onload = res; js.onerror = () => rej(new Error('leaflet')); document.head.appendChild(js); });
  const centre = () => { const c = map.getCenter(); return [+c.lat.toFixed(6), +c.lng.toFixed(6)]; };
  const coordsChanged = () => { const c = centre(), g = geoOf(); return !g || Math.abs(g[0] - c[0]) > 5e-6 || Math.abs(g[1] - c[1]) > 5e-6; };
  // one main button: "determine address" (red) while the address for the crosshair is unknown, else
  // "apply" — green when there is something new to apply, grey when nothing changed
  function gmState() {
    const c = map ? centre() : null;
    $('#gm-addr-col').hidden = !LOC.addr; $('#gm-coords-col').hidden = !LOC.coords;
    $('#gm-coords').textContent = c && LOC.coords ? `${c[0].toFixed(5)}, ${c[1].toFixed(5)}` : '';
    const at = $('#gm-addr-txt'); at.textContent = gmAddrErr ? gm.dataset.tNone : gmAddrKnown ? gmAddr : ''; at.classList.toggle('unknown', gmAddrErr || !gmAddrKnown);
    const b = $('#gm-ok'), determine = LOC.addr && !gmAddrKnown;
    $('#gm-ok-txt').textContent = determine ? b.dataset.tDetermine : b.dataset.tApply;
    $('#gm-ok-ico').setAttribute('href', practical.sprite + (determine ? '#i-pin' : '#i-check'));
    const changed = LOC.addr ? gmMoved : (map ? coordsChanged() : false);
    b.classList.toggle('hot', determine); b.classList.toggle('good', !determine && changed);
    b.disabled = gmBusy || (!determine && !changed);
  }
  let gmBusy = false;
  async function openMap() {
    gm.hidden = false; $('#gm-hint').textContent = ''; const seq = ++gmSeq; gmMoved = false; gmAddrErr = false; gmAddr = f.location.value.trim(); gmAddrKnown = LOC.addr && !!gmAddr && !LOC.errAddr && !LOC.errCoords;
    try { await loadLeaflet(); } catch (_) { $('#gm-hint').textContent = gm.dataset.tError; return; }
    if (!map) { map = L.map('gm-map', { zoomControl: true, attributionControl: true, inertia: false /* no glide after a drag: a click on "determine" right after releasing would be undone by the late move events */ }).setView([20, 0], 2); L.tileLayer('https://tile.openstreetmap.org/{z}/{x}/{y}.png', { maxZoom: 19, attribution: '&copy; <a href="https://www.openstreetmap.org/copyright" target="_blank" rel="noopener">OpenStreetMap</a> contributors' }).addTo(map); map.on('move', () => { if (gmMoving) return; gmMoved = true; gmAddrKnown = false; gmAddrErr = false; gmState(); }); }
    map.invalidateSize();
    const g = geoOf(), q = gmAddr; gmMoving = true;
    const jump = (ll, z) => map.setView(ll, z, { animate: false }); // an animated jump would fire move events
    if (g) jump(g, 16);
    else if (q.length >= 3) { $('#gm-hint').textContent = '…'; try { const items = await geoSearch(q, null); if (seq !== gmSeq) return; if (items.length) { jump([items[0].lat, items[0].lon], 16); $('#gm-hint').textContent = ''; } else { jump([20, 0], 2); gmAddrKnown = false; $('#gm-hint').textContent = gm.dataset.tNone + ' · ' + gm.dataset.tWorld; } } catch (_) { jump([20, 0], 2); gmAddrKnown = false; $('#gm-hint').textContent = gm.dataset.tError; } }
    else { jump([20, 0], 2); gmAddrKnown = false; $('#gm-hint').textContent = gm.dataset.tWorld; }
    gmState(); setTimeout(() => { gmMoving = false; }, 120);
  }
  const closeMap = () => { gm.hidden = true; };
  $('#gm-close').onclick = closeMap;
  async function gmDetermine() { const c = centre(); gmBusy = true; gmState(); $('#gm-hint').textContent = '…'; try { const p = await geoReverse(c[0], c[1]); if (p) { gmAddr = p.label; gmAddrKnown = true; gmAddrErr = false; } else { gmAddrKnown = false; gmAddrErr = true; } $('#gm-hint').textContent = ''; } catch (_) { gmAddrKnown = false; gmAddrErr = true; $('#gm-hint').textContent = gm.dataset.tError; } gmBusy = false; gmState(); }
  function gmApply() { const c = centre(); if (LOC.coords) { f.lat.value = c[0].toFixed(6); f.lon.value = c[1].toFixed(6); LOC.errCoords = false; LOC.invalid = false; } if (LOC.addr) { f.location.value = gmAddr; LOC.errAddr = false; } closeMap(); locRender(); syncDot(); refresh(); }
  $('#gm-ok').onclick = () => { if (LOC.addr && !gmAddrKnown) gmDetermine(); else gmApply(); };
  addEventListener('keydown', e => { if (e.key === 'Escape' && !gm.hidden) { e.stopImmediatePropagation(); closeMap(); } }, true);
