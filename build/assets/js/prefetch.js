/* prefetch on intent: what a navigation will fetch (practical.prefetch(url)) is fetched on hover (after HOVER_MS),
   focus or press of its control (practical.intent) and handed to that navigation once (practical.prefetched(url)),
   if it is younger than TTL and nothing happened since that could change it: every live change (`rustical:change`),
   every own write (a non-GET fetch, of this tab or — localStorage 'practical.write' — another one) and the live
   stream closing drop all entries, those still in flight included. Only while the stream is open
   (practical.stream) and never on Save-Data. The server answers a prefetch (Purpose: prefetch) without touching
   the session's data and with 204 while a flash is pending (sessions.rs): only a 200 is kept. */
(function () {
  const P = window.practical, f0 = window.fetch.bind(window), TTL = 30000, MAX = 8, HOVER_MS = 65;
  const m = new Map(); let gen = 0, live = false;
  const drop = () => { gen++; m.clear(); };
  const wrote = () => { drop(); try { localStorage['practical.write'] = gen + ' ' + Math.random(); } catch (_) {} };
  window.fetch = (input, init) => { if (!init || !init.method || /^(GET|HEAD)$/i.test(init.method)) return f0(input, init); wrote(); return f0(input, init).finally(wrote); };
  addEventListener('storage', e => { if (e.key === 'practical.write') drop(); });
  document.addEventListener('rustical:change', drop);
  const on = () => live && !(navigator.connection && navigator.connection.saveData);
  P.stream = open => { live = open; if (!open) drop(); };
  P.prefetch = url => {
    const k = String(url), e = m.get(k);
    if (!on() || (e && Date.now() - e.t < TTL)) return;
    if (m.size >= MAX) m.delete(m.keys().next().value);
    m.set(k, { t: Date.now(), g: gen, p: f0(k, { headers: { Accept: 'text/html', Purpose: 'prefetch' } }).then(r => r.status === 200 ? r.text() : null).catch(() => null) });
  };
  P.prefetched = async url => {
    const k = String(url), e = m.get(k); if (!e) return null;
    m.delete(k);
    if (e.g !== gen || Date.now() - e.t > TTL || !on()) return null;
    const html = await e.p; return e.g === gen ? html : null;
  };
  // `urls()`: what a click on `el` would fetch, asked at the moment of intent; `hovered`: a pointer already
  // resting on `el` counts (a control that replaced the one under it, which gets no pointerenter)
  P.intent = (el, urls, hovered) => {
    let t = 0; const now = () => { clearTimeout(t); t = 0; if (el.isConnected) urls().forEach(P.prefetch); }, rest = () => { if (!t) t = setTimeout(now, HOVER_MS); };
    if (hovered && el.matches(':hover')) rest();
    el.addEventListener('pointerenter', rest);
    el.addEventListener('pointerleave', () => { clearTimeout(t); t = 0; });
    el.addEventListener('pointerdown', now); el.addEventListener('focus', now); el.addEventListener('touchstart', now, { passive: true });
  };
})();
