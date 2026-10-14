// js/shell/settings-connect.js — part of the shell IIFE (src/bundle.rs; shares its scope with settings-pages.js): "add a
// device" (settings.html #add…, routes::connect) — which path shows, the pairing with its QR codes, the live state
  if (settings && $('#add', settings)) {
    /* The Android path when the browser names Android (Chrome's reduced User-Agent keeps "Android"); when it names no
       system at all, a touch screen counts as the device itself. Anything else (desktop, iPhone) is "another device". */
    const ua = navigator.userAgent || '', uad = navigator.userAgentData;
    const named = /Android|Windows|Macintosh|Mac OS X|iPhone|iPad|CrOS|X11|Linux x86_64/i.test(ua);
    const android = /Android/i.test(ua) || /Android/i.test((uad && uad.platform) || '') || (!named && matchMedia('(pointer: coarse)').matches);
    settings.dataset.android = android ? '1' : '0';
    let pair = new URLSearchParams(location.search).get('pair') || '', codes = null, timer = 0;
    // the pairing and its codes come from the server once per run of the wizard (POST: it is made for this user)
    const ensure = async () => {
      if (codes) return codes;
      const r = await practical.req('/settings/connect/pair', { method: 'POST', body: new URLSearchParams({ pair }) });
      if (!r.ok) return null;
      codes = r.data; pair = codes.pair;
      $$('[data-code]', settings).forEach(el => {
        const k = el.dataset.code, s = k.startsWith('store-') && codes.stores.find(x => 'store-' + x.key === k);
        const svg = k === 'install' ? codes.install.svg : k === 'connect' ? codes.connect.svg : s && s.svg;
        if (svg) el.innerHTML = svg; // the server's own SVG; the frame carries role and label (settings.html)
      });
      return codes;
    };
    // the live state while a step waits for the other device: opened there (step 2), a device connected (step 4 → 5)
    const poll = async () => {
      clearTimeout(timer);
      if (!/^#add-android/.test(location.hash) || document.hidden || !pair) return;
      const r = await practical.req('/settings/connect/status?pair=' + encodeURIComponent(pair));
      if (r.ok && r.data) {
        const o = $('[data-live="opened"]', settings); if (o) o.hidden = !r.data.opened;
        if (r.data.device) { $('[data-device]', settings).textContent = r.data.device; location.hash = '#add-done'; return; }
      } else if (r.kind === 'notfound') { codes = null; pair = ''; await ensure(); } // expired: a new pairing (new codes)
      timer = setTimeout(poll, 2000);
    };
    const step = async () => {
      const h = location.hash;
      if (h === '#add') { clearTimeout(timer); codes = null; pair = ''; return; } // a new run: a new pairing (a device of the last one would end it at once)
      if (!h.startsWith('#add-android')) { clearTimeout(timer); return; }
      if (android && h === '#add-android') { location.replace('#add-android-install'); return; } // that step is for another device
      if (await ensure()) poll();
    };
    addEventListener('hashchange', step);
    document.addEventListener('visibilitychange', () => { if (!document.hidden) poll(); }); // back from DAVx⁵: ask at once
    step();
    // step 3 on another device: a store row picks the QR code beside the list instead of leaving the page
    $$('[data-store]', settings).forEach(a => a.addEventListener('click', e => {
      if (android) return;
      e.preventDefault();
      $$('[data-store]', settings).forEach(x => { x.classList.toggle('sel', x === a); x.setAttribute('aria-current', x === a ? 'true' : 'false'); });
      $$('[data-store-qr]', settings).forEach(x => { x.hidden = x.dataset.storeQr !== a.dataset.store; });
    }));
  }
