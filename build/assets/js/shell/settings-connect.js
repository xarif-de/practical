// js/shell/settings-connect.js — part of the shell IIFE (src/bundle.rs; shares its scope with settings-pages.js): "add a
// device" (settings.html #add…, routes::connect) — which path shows, the pairing with its QR codes, the live state
  /* the iPhone by hand: the password's buttons in `scope` (the wizard; the profile link's manual page, connect.html). It is
     made only when first asked for — `get()` → {password, name?}, once —, dots until then. Copy: the clipboard takes a
     promise (Safari keeps the tap's permission while the token is made), else the text after it; the eye shows it. */
  const ioPassword = (scope, get) => {
    let p = null;
    const failed = e => { if (!(e && e.kind)) toast(tr('js-io-pw-failed')); }; // a practical.req answer was told already
    const once = () => p ||= get().then(m => {
      $$('[data-io-pw]', scope).forEach(e => { e.textContent = m.password; });
      if (m.name) $$('[data-device]', scope).forEach(e => { e.textContent = m.name; });
      return m;
    }, e => { p = null; throw e; });
    $$('[data-io-pw-copy]', scope).forEach(b => b.addEventListener('click', async () => {
      const got = once();
      try {
        if (window.ClipboardItem) await navigator.clipboard.write([new ClipboardItem({ 'text/plain': got.then(m => new Blob([m.password], { type: 'text/plain' })) })]);
        else await navigator.clipboard.writeText((await got).password);
        toast(tr('js-copied'));
      } catch (_) { // not made (one message, no second request), or the clipboard refused: the text after it, else shown
        got.then(m => navigator.clipboard.writeText(m.password).then(() => toast(tr('js-copied')), () => { show(true); toast(tr('js-copy-failed')); }), failed);
      }
    }));
    // the password instead of its dots (`on`), in every row of `row`
    const show = (on, row = scope) => {
      $$('[data-io-pw]', row).forEach(e => { e.hidden = !on; });
      $$('[data-io-dots]', row).forEach(e => { e.hidden = on; });
      $$('[data-io-pw-show]', row).forEach(b => b.setAttribute('aria-pressed', on));
    };
    $$('[data-io-pw-show]', scope).forEach(b => b.addEventListener('click', async () => {
      try { await once(); } catch (e) { failed(e); return; }
      show(b.getAttribute('aria-pressed') !== 'true', b.closest('div'));
    }));
    return () => { // a new run: a new password, shown as dots again
      p = null;
      $$('[data-io-pw]', scope).forEach(e => { e.textContent = ''; });
      show(false);
    };
  };
  const cp = $('.connect-page [data-pw-url]');
  if (cp) ioPassword($('.connect-page'), () => fetch(cp.dataset.pwUrl).then(r => r.ok ? r.json() : Promise.reject(r)));
  // the end of the phone's guide: say that the page has done its job
  $$('[data-io-finish]').forEach(b => b.addEventListener('click', () => askChoice(b.dataset.ioFinish, [['1', tr('js-ok')]], tr('js-close'))));
  // `/settings?add=ios-manual` (a profile link's "expired" page): a login keeps the query but drops a #hash
  const addq = settings && new URLSearchParams(location.search).get('add');
  if (addq && /^[a-z-]+$/.test(addq) && $('#add-' + addq, settings)) location.replace(location.pathname + '#add-' + addq);
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
    let pair = '', codes = null, timer = 0, tries = 0, landed = false, linkAt = 0, known = false, profiled = false;
    const device = ios && !/iPhone|iPod/.test(ua) ? 'iPad' : 'iPhone';
    // the device of this run as the server named it ("iPhone (2)"); `known` until a new run
    const name = n => { known = true; $$('[data-device]', settings).forEach(e => { e.textContent = n; }); };
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
      const r = await practical.req('/settings/connect/apple', { method: 'POST' });
      if (!r.ok) { if (loud) practical.tell(r); return false; }
      linkAt = Date.now();
      $('[data-qr="apple"]', settings).innerHTML = r.data.qr;
      $('[data-qr="apple-manual"]', settings).innerHTML = r.data.manual_qr; // the manual guide on the iPhone, from a computer
      $('[data-apple]', settings).href = r.data.file;
      $('#io-url', settings).textContent = r.data.url;
      $('[data-io-other]', settings).hidden = !(navigator.standalone || r.data.other);
      return true;
    };
    // the iPhone by hand (#add-ios-values, -more): its password (routes::connect `manual_password`)
    const newPassword = ioPassword(settings, () => practical.req('/settings/connect/manual?name=' + device, { method: 'POST' }).then(r => r.ok ? (known ||= ios, r.data) : Promise.reject(practical.tell(r)))); // shown on a computer: no device yet
    // the pages a device can get connected from: there the wizard waits for a device of this user that fetched its
    // app password (Android) or its profile (iPhone) → done, or the iPhone's install step; on a computer the iPhone's
    // values page waits too (its code is a link) and only names the device, and its install step for a phone that
    // refuses the profile and goes on by hand (→ the manual end). Every 2 s for two minutes, then every 10 s
    // for 13 more; not while the page is hidden. Then a tap, a key or the page shown again waits on: a code left open
    // on a screen stays no lasting key (review 2026-10-17). The link is renewed after 5 minutes: it lives 10, so a
    // code scanned late still leaves 5; a used one answers its device for 8 more (domain::connect LINK_SECS,
    // LINK_AGAIN_SECS). The device's own download button renews a stale link before it goes.
    const WAIT = ios ? /^#add-(android(-connect)?|ios)$/ : /^#add-(android(-connect)?|ios|ios-values|ios-install)$/, LINKED = /^#add-ios(-values)?$/;
    const RENEW = 5 * 60e3, TRIES = 60 + 78;
    const poll = async () => {
      clearTimeout(timer);
      if (!WAIT.test(location.hash) || document.hidden || tries >= TRIES) return;
      if (pair) {
        const r = await practical.req('/settings/connect/status?pair=' + encodeURIComponent(pair));
        if (!WAIT.test(location.hash)) return; // left meanwhile
        const h = location.hash, manual = r.ok && r.data.manual;
        if (r.ok && r.data.device && (h !== '#add-ios-install' || manual)) {
          name(r.data.device);
          if (h === '#add-ios' && !manual) { landed = profiled = true; location.replace('#add-ios-install'); return; } // the pairing waits on there
          codes = null; pair = ''; // Back starts a new wait instead of landing here again
          if (h === '#add-ios-values') return; // the iPhone copies from its own page; this one goes on by hand
          if (manual) settings.dataset.path = 'iosm'; // the phone refused the profile and went on by hand: its end
          landed = true; location.replace('#add-done');
          return;
        }
        if (r.kind === 'notfound') { codes = null; pair = ''; } // expired: a new pairing
      }
      if (!pair) await ensure(false);
      if (LINKED.test(location.hash) && Date.now() - linkAt > RENEW) await link(false);
      timer = setTimeout(poll, ++tries < 60 ? 2000 : 10000);
    };
    const wake = () => { if (tries >= TRIES && WAIT.test(location.hash)) step(); };
    // #add-done's row before a device was named (Android's text; on the iPhone paths its kind)
    const unnamed = $('#add-done [data-device]', settings).textContent;
    // each path has its own step 4: a link to the other one is sent on
    const OTHER = android ? { '#add-android': '#add-android-connect' } : { '#add-android-connect': '#add-android' };
    const step = async () => {
      const h = location.hash;
      if (OTHER[h]) { location.replace(OTHER[h]); return; }
      clearTimeout(timer);
      // #add-done's text: Android, the iPhone's profile, the iPhone by hand
      if (/^#add-(ios|android)/.test(h)) settings.dataset.path = /^#add-ios-(manual|values|more)$/.test(h) ? 'iosm' : h.startsWith('#add-ios') ? 'ios' : 'android';
      if (h === '#add-done') { // the iPhone by hand may end here before the phone fetched its password: no "connected" yet
        const d = $('#add-done', settings);
        if (!known) $('[data-device]', d).textContent = settings.dataset.path === 'android' ? unnamed : device;
        $('.sub', d).hidden = !known && settings.dataset.path === 'iosm';
      }
      if ((h === '#add-done' || h === '#add-ios-install') && landed) { // the page changed by itself: say so
        landed = false; $(h + ' h1').focus();
        if (WAIT.test(h)) { tries = 0; poll(); }
        return;
      }
      if (h === '#add') { codes = null; pair = ''; linkAt = 0; known = profiled = false; newPassword(); return; } // a new run: a new pairing (a device of the last one would end it at once), link and password
      if (h === '#add-ios' && profiled) { codes = null; pair = ''; profiled = false; } // Back from the install step: a new wait
      if (h === '#add-android-install' && !android) ensure(true); // the stores' codes
      // Back from the install step keeps the link: the same device may load its profile again (domain::connect Links).
      // A failed request was said once; the wait retries it quietly.
      else if (WAIT.test(h)) {
        tries = 0;
        if (await ensure(true) && LINKED.test(h) && Date.now() - linkAt >= RENEW) await link(true);
        poll();
      }
    };
    addEventListener('hashchange', step);
    document.addEventListener('visibilitychange', () => { if (!document.hidden) { if (tries >= TRIES) wake(); else poll(); } }); // back from DAVx⁵: ask at once
    ['pointerdown', 'keydown'].forEach(t => settings.addEventListener(t, wake));
    const dl = $('[data-apple]', settings);
    dl.addEventListener('click', async e => {
      if (Date.now() - linkAt < RENEW) return;
      e.preventDefault();
      if (await link(true)) location.href = dl.href;
    });
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
