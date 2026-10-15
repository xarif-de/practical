// js/shell/settings-connect.js — part of the shell IIFE (src/bundle.rs; shares its scope with settings-pages.js): "add a
// device" (settings.html #add…, routes::connect) — which path shows, the pairing with its QR codes, the live state
  if (settings && $('#add', settings)) {
    /* The Android path when the browser names Android (Chrome's reduced User-Agent keeps "Android"); when it names no
       system at all, a touch screen counts as the device itself. Anything else (desktop, iPhone) is "another device". */
    const ua = navigator.userAgent || '', uad = navigator.userAgentData;
    const named = /Android|Windows|Macintosh|Mac OS X|iPhone|iPad|CrOS|X11|Linux x86_64/i.test(ua);
    const android = /Android/i.test(ua) || /Android/i.test((uad && uad.platform) || '') || (!named && matchMedia('(pointer: coarse)').matches);
    settings.dataset.android = android ? '1' : '0';
    /* An iPad's Safari says "Macintosh" (desktop sites, iPadOS 13+); a touch screen tells it from a Mac. */
    const ios = /iPhone|iPad|iPod/.test(ua) || (/Macintosh/.test(ua) && navigator.maxTouchPoints > 1);
    settings.dataset.ios = ios ? '1' : '0';
    let pair = '', codes = null, timer = 0, tries = 0, landed = false, linkAt = 0, manual = null;
    const device = ios && !/iPhone|iPod/.test(ua) ? 'iPad' : 'iPhone';
    // a pairing for this run of the wizard and the QR codes (POST: the pairing is this user's); `loud`: say why not
    const ensure = async loud => {
      if (codes) return codes;
      const r = await practical.req('/settings/connect/pair', { method: 'POST' });
      if (!r.ok) { if (loud) practical.tell(r); return null; }
      codes = r.data; pair = codes.pair;
      $$('[data-qr]', settings).forEach(el => {
        const svg = typeof codes[el.dataset.qr] === 'string' ? codes[el.dataset.qr] : codes.stores[el.dataset.qr];
        if (svg) el.innerHTML = svg; // the server's own SVG; the frame carries role and label (settings.html)
      });
      return codes;
    };
    /* the iPhone path's profile link (routes::connect `apple_link`): its QR code, and on the device the download.
       Only Safari and Chrome install a profile (domain::connect `installs_profiles`); a web app on the Home Screen
       is no browser at all: there the device is told to open the link's page in Safari. */
    const link = async loud => {
      const r = await practical.req('/settings/connect/apple' + (ios ? '?name=' + device : ''), { method: 'POST' });
      if (!r.ok) { if (loud) practical.tell(r); return false; }
      linkAt = Date.now();
      $('[data-qr="apple"]', settings).innerHTML = r.data.qr;
      $('[data-apple]', settings).href = r.data.file;
      $('#io-url', settings).textContent = r.data.url;
      $('[data-io-other]', settings).hidden = !(navigator.standalone || /FxiOS|DuckDuckGo|EdgiOS|OPiOS|OPT\/|GSA\//.test(ua));
      return true;
    };
    /* the iPhone by hand (#add-ios-values, -more): its password, an app token made once per run of the wizard (routes::connect
       `manual_password`); the pages show dots, the copy buttons take it from the hidden [data-io-pw] elements */
    const password = async () => {
      if (manual) return;
      const r = await practical.req('/settings/connect/manual?name=' + device, { method: 'POST' });
      if (!r.ok) { practical.tell(r); return; }
      manual = r.data;
      $$('[data-io-pw]', settings).forEach(e => { e.textContent = manual.password; });
      $$('[data-device]', settings).forEach(e => { e.textContent = manual.name; });
    };
    // the pages a device can get connected from: there the wizard waits for a device of this user that fetched its
    // app password (Android) or its profile (iPhone) → done, or the iPhone's install step. Every 2 s for two
    // minutes, then every 10 s; not while the page is hidden. The profile link is renewed after 8 minutes: it lives 10,
    // and a downloaded one answers its device for 8 more (domain::connect LINK_SECS, LINK_AGAIN_SECS).
    const WAIT = /^#add-(android(-connect)?|ios)$/;
    const poll = async () => {
      clearTimeout(timer);
      if (!WAIT.test(location.hash) || document.hidden) return;
      if (pair) {
        const r = await practical.req('/settings/connect/status?pair=' + encodeURIComponent(pair));
        if (!WAIT.test(location.hash)) return; // left meanwhile
        if (r.ok && r.data.device) {
          $$('[data-device]', settings).forEach(e => { e.textContent = r.data.device; });
          codes = null; pair = ''; // Back starts a new wait instead of landing here again
          landed = true; location.replace(location.hash === '#add-ios' ? '#add-ios-install' : '#add-done');
          return;
        }
        if (r.kind === 'notfound') { codes = null; pair = ''; } // expired: a new pairing
      }
      if (!pair) await ensure(false);
      if (location.hash === '#add-ios' && Date.now() - linkAt > 8 * 60e3) await link(false);
      timer = setTimeout(poll, ++tries < 60 ? 2000 : 10000);
    };
    // each path has its own step 4: a link to the other one is sent on
    const OTHER = android ? { '#add-android': '#add-android-connect' } : { '#add-android-connect': '#add-android' };
    const step = async () => {
      const h = location.hash;
      if (OTHER[h]) { location.replace(OTHER[h]); return; }
      clearTimeout(timer);
      // #add-done's text: Android, the iPhone's profile, the iPhone by hand
      if (/^#add-(ios|android)/.test(h)) settings.dataset.path = /^#add-ios-(manual|values|more)$/.test(h) ? 'iosm' : h.startsWith('#add-ios') ? 'ios' : 'android';
      if ((h === '#add-done' || h === '#add-ios-install') && landed) { landed = false; $(h + ' h1').focus(); return; } // the page changed by itself: say so
      if (h === '#add') { codes = null; pair = ''; linkAt = 0; manual = null; return; } // a new run: a new pairing (a device of the last one would end it at once), link and password
      if (h === '#add-ios-values' || h === '#add-ios-more') { password(); return; }
      if (h === '#add-ios-manual' && !ios) { ensure(true); return; } // the code that opens this on the iPhone
      if (h === '#add-android-install' && !android) ensure(true); // the stores' codes
      // Back from the install step keeps the link: the same device may load its profile again (domain::connect Links)
      else if (WAIT.test(h) && await ensure(true) && (h !== '#add-ios' || Date.now() - linkAt < 8 * 60e3 || await link(true))) { tries = 0; poll(); }
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
