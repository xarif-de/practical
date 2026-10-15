// js/shell/settings-connect.js — part of the shell IIFE (src/bundle.rs; shares its scope with settings-pages.js): "add a
// device" (settings.html #add…, routes::connect) — which path shows, the pairing with its QR codes, the live state
  if (settings && $('#add', settings)) {
    /* The Android path when the browser names Android (Chrome's reduced User-Agent keeps "Android"); when it names no
       system at all, a touch screen counts as the device itself. Anything else (desktop, iPhone) is "another device". */
    const ua = navigator.userAgent || '', uad = navigator.userAgentData;
    const named = /Android|Windows|Macintosh|Mac OS X|iPhone|iPad|CrOS|X11|Linux x86_64/i.test(ua);
    const android = /Android/i.test(ua) || /Android/i.test((uad && uad.platform) || '') || (!named && matchMedia('(pointer: coarse)').matches);
    settings.dataset.android = android ? '1' : '0';
    let pair = '', codes = null, timer = 0, tries = 0, landed = false;
    // a pairing for this run of the wizard and the QR codes (POST: the pairing is this user's); `loud`: say why not
    const ensure = async loud => {
      if (codes) return codes;
      const r = await practical.req('/settings/connect/pair', { method: 'POST' });
      if (!r.ok) { if (loud) practical.tell(r); return null; }
      codes = r.data; pair = codes.pair;
      $$('[data-qr]', settings).forEach(el => {
        const svg = el.dataset.qr === 'connect' ? codes.connect : codes.stores[el.dataset.qr];
        if (svg) el.innerHTML = svg; // the server's own SVG; the frame carries role and label (settings.html)
      });
      return codes;
    };
    // the pages a device can get connected from: there the wizard waits for a device of this user that fetched its
    // app password → done. Every 2 s for two minutes, then every 10 s; not while the page is hidden.
    const WAIT = /^#add-android(-connect)?$/;
    const poll = async () => {
      clearTimeout(timer);
      if (!WAIT.test(location.hash) || document.hidden) return;
      if (pair) {
        const r = await practical.req('/settings/connect/status?pair=' + encodeURIComponent(pair));
        if (!WAIT.test(location.hash)) return; // left meanwhile
        if (r.ok && r.data.device) {
          $('[data-device]', settings).textContent = r.data.device;
          codes = null; pair = ''; // Back starts a new wait instead of landing here again
          landed = true; location.replace('#add-done');
          return;
        }
        if (r.kind === 'notfound') { codes = null; pair = ''; } // expired: a new pairing
      }
      if (!pair) await ensure(false);
      timer = setTimeout(poll, ++tries < 60 ? 2000 : 10000);
    };
    // each path has its own step 4: a link to the other one is sent on
    const OTHER = android ? { '#add-android': '#add-android-connect' } : { '#add-android-connect': '#add-android' };
    const step = async () => {
      const h = location.hash;
      if (OTHER[h]) { location.replace(OTHER[h]); return; }
      clearTimeout(timer);
      if (h === '#add-done' && landed) { landed = false; $('#add-done h1').focus(); return; } // the page changed by itself: say so
      if (h === '#add') { codes = null; pair = ''; return; } // a new run: a new pairing (a device of the last one would end it at once)
      if (h === '#add-android-install' && !android) ensure(true); // the stores' codes
      else if (WAIT.test(h) && await ensure(true)) { tries = 0; poll(); }
    };
    addEventListener('hashchange', step);
    document.addEventListener('visibilitychange', () => { if (!document.hidden) poll(); }); // back from DAVx⁵: ask at once
    step();
    // step 3 on another device: a store row picks the QR code beside the list instead of leaving the page
    if (!android) {
      const rows = $$('[data-store]', settings), pick = a => {
        rows.forEach(x => { x.classList.toggle('sel', x === a); x.setAttribute('aria-current', x === a); });
        $$('[data-store-qr]', settings).forEach(x => { x.hidden = x.dataset.storeQr !== a.dataset.store; });
      };
      rows.forEach(a => a.addEventListener('click', e => { e.preventDefault(); pick(a); }));
      if (rows[0]) pick(rows[0]);
    }
  }
