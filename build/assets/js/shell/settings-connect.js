// js/shell/settings-connect.js — part of the shell IIFE (src/bundle.rs; shares its scope with settings-pages.js): "add a
// device" (settings.html #add…, routes::connect) — which path shows, the pairing with its QR codes, the live state
  /* the iPhone by hand: the password's buttons in `scope` (the wizard; the profile link's manual page, connect.html). It is
     made only when first asked for — `get()` → {password, name?}, once —, dots until then. Copy: the clipboard takes a
     promise (Safari keeps the tap's permission while the token is made), else the text after it; the eye shows it. */
  const ioPassword = (scope, get) => {
    let p = null;
    const once = () => p ||= get().then(m => {
      $$('[data-io-pw]', scope).forEach(e => { e.textContent = m.password; });
      if (m.name) $$('[data-device]', scope).forEach(e => { e.textContent = m.name; });
      return m;
    }, e => { p = null; throw e; });
    $$('[data-io-pw-copy]', scope).forEach(b => b.addEventListener('click', async () => {
      try {
        if (window.ClipboardItem) await navigator.clipboard.write([new ClipboardItem({ 'text/plain': once().then(m => new Blob([m.password], { type: 'text/plain' })) })]);
        else await navigator.clipboard.writeText((await once()).password);
        toast(tr('js-copied'));
      } catch (_) { once().then(m => navigator.clipboard.writeText(m.password)).then(() => toast(tr('js-copied')), () => {}); }
    }));
    $$('[data-io-pw-show]', scope).forEach(b => b.addEventListener('click', async () => {
      await once().catch(() => {});
      const on = b.getAttribute('aria-pressed') !== 'true', row = b.closest('div');
      b.setAttribute('aria-pressed', on);
      $('[data-io-pw]', row).hidden = !on; $('[data-io-dots]', row).hidden = on;
    }));
    return () => { p = null; $$('[data-io-pw]', scope).forEach(e => { e.textContent = ''; }); }; // a new run: a new password
  };
  const cp = $('.connect-page [data-pw-url]');
  if (cp) ioPassword($('.connect-page'), () => fetch(cp.dataset.pwUrl).then(r => r.ok ? r.json() : Promise.reject(r)));
  // the end of the phone's guide: say that the page has done its job
  $$('[data-io-finish]').forEach(b => b.addEventListener('click', () => askChoice(b.dataset.ioFinish, [['1', 'OK']], tr('js-close'))));
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
    let pair = '', codes = null, timer = 0, tries = 0, landed = false, linkAt = 0;
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
      $('[data-qr="apple-manual"]', settings).innerHTML = r.data.manual_qr; // the manual guide on the iPhone, from a computer
      $('[data-apple]', settings).href = r.data.file;
      $('#io-url', settings).textContent = r.data.url;
      $('[data-io-other]', settings).hidden = !(navigator.standalone || /FxiOS|DuckDuckGo|EdgiOS|OPiOS|OPT\/|GSA\//.test(ua));
      return true;
    };
    // the iPhone by hand (#add-ios-values, -more): its password (routes::connect `manual_password`)
    const newPassword = ioPassword(settings, () => practical.req('/settings/connect/manual?name=' + device, { method: 'POST' }).then(r => r.ok ? r.data : Promise.reject(practical.tell(r))));
    // the pages a device can get connected from: there the wizard waits for a device of this user that fetched its
    // app password (Android) or its profile (iPhone) → done, or the iPhone's install step; on a computer the iPhone's
    // values page waits too (its code is a link) and only names the device. Every 2 s for two minutes, then every 10 s;
    // not while the page is hidden. The link is renewed after 8 minutes: it lives 10, and a used one answers its device
    // for 8 more (domain::connect LINK_SECS, LINK_AGAIN_SECS).
    const WAIT = ios ? /^#add-(android(-connect)?|ios)$/ : /^#add-(android(-connect)?|ios|ios-values)$/, LINKED = /^#add-ios(-values)?$/;
    const poll = async () => {
      clearTimeout(timer);
      if (!WAIT.test(location.hash) || document.hidden) return;
      if (pair) {
        const r = await practical.req('/settings/connect/status?pair=' + encodeURIComponent(pair));
        if (!WAIT.test(location.hash)) return; // left meanwhile
        if (r.ok && r.data.device) {
          $$('[data-device]', settings).forEach(e => { e.textContent = r.data.device; });
          codes = null; pair = ''; // Back starts a new wait instead of landing here again
          if (location.hash === '#add-ios-values') return; // the iPhone copies from its own page; this one goes on by hand
          landed = true; location.replace(location.hash === '#add-ios' ? '#add-ios-install' : '#add-done');
          return;
        }
        if (r.kind === 'notfound') { codes = null; pair = ''; } // expired: a new pairing
      }
      if (!pair) await ensure(false);
      if (LINKED.test(location.hash) && Date.now() - linkAt > 8 * 60e3) await link(false);
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
      if (h === '#add') { codes = null; pair = ''; linkAt = 0; newPassword(); return; } // a new run: a new pairing (a device of the last one would end it at once), link and password
      if (h === '#add-android-install' && !android) ensure(true); // the stores' codes
      // Back from the install step keeps the link: the same device may load its profile again (domain::connect Links)
      else if (WAIT.test(h) && await ensure(true) && (!LINKED.test(h) || Date.now() - linkAt < 8 * 60e3 || await link(true))) { tries = 0; poll(); }
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
