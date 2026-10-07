// js/shell/base.js — the shell IIFE starts here (src/bundle.rs: its parts share one function scope): dev reload, sticky header, .seg, ask/askYesNo, toast,
// colour scheme, confirmations, clipboard; window.segPress ask askYesNo toast applyScheme
  const { $, $$, ico } = practical;

  // development only (body[data-dev]): reload after a rebuild (new boot id) or an asset edit
  if (document.body.dataset.dev) {
    let boot = sessionStorage.devBoot; const connect = () => {
      const es = new EventSource('/dev/reload');
      es.addEventListener('boot', ev => { if (boot && boot !== ev.data) location.reload(); boot = sessionStorage.devBoot = ev.data; });
      es.addEventListener('reload', () => location.reload());
      es.onerror = () => { es.close(); setTimeout(connect, 700); };
    };
    connect();
  }

  // fixed page header: the toolbar sticks under the topbar, sidebars under both — measured heights
  // (both bars wrap on narrow screens; the calendar toolbar is replaced with every fragment swap)
  const setBars = () => { const st = document.documentElement.style, tb = $('.topbar'), tl = $('.toolbar'); if (tb) st.setProperty('--topbar-h', tb.offsetHeight + 'px'); if (tl) st.setProperty('--toolbar-h', tl.offsetHeight + 'px'); };
  if ('ResizeObserver' in window) {
    const ro = new ResizeObserver(setBars);
    // re-observed only when the toolbar element itself is another one (a fragment swap): its size changes are the ResizeObserver's
    let tl0;
    const watch = () => { tl0 = $('.toolbar'); ro.disconnect(); ['.topbar', '.toolbar'].forEach(s => { const e = $(s); if (e) ro.observe(e); }); setBars(); };
    watch(); const mainEl = $('.main'); if (mainEl) new MutationObserver(() => { if ($('.toolbar') !== tl0) watch(); }).observe(mainEl, { childList: true, subtree: true });
  } else { setBars(); addEventListener('resize', setBars); }

  /* segmented switch (`.seg`): exactly one button pressed. Every page switches through this one
     function — the look keys on aria-pressed="true", so a bare attribute ("") shows nothing */
  /* a minimal yes/no question over the page (the editor dialog's backdrop). Resolves true for "yes", false for
     "no"; Escape and a click on the backdrop resolve null (also a "no"; the conflict question tells it apart:
     back to the editing). "Yes" has the focus, so Enter confirms.
     `modal`: the backdrop does not answer — a question whose "no" still changes something (a stray click
     must not decide it); Escape stays the explicit "no". */
  window.askYesNo = (text, yes, no, modal) => new Promise(done => {
    const d = document.createElement('div');
    d.className = 'ask'; d.setAttribute('role', 'alertdialog'); d.setAttribute('aria-modal', 'true');
    d.innerHTML = '<div class="ask-box"><p></p><div class="ask-acts"><button class="btn icon-btn" type="button" data-a="0"><svg class="ico" aria-hidden="true"><use href="' + practical.sprite + '#i-x"/></svg></button><button class="btn btn-primary icon-btn" type="button" data-a="1"><svg class="ico" aria-hidden="true"><use href="' + practical.sprite + '#i-check"/></svg></button></div></div>';
    d.querySelector('p').textContent = text;
    for (const [a, t] of [['0', no], ['1', yes]]) { const b = d.querySelector(`[data-a="${a}"]`); b.title = t; b.setAttribute('aria-label', t); }
    const prev = document.activeElement;
    const end = v => { removeEventListener('keydown', key, true); d.remove(); if (prev && prev.focus) prev.focus(); done(v); };
    const key = e => { if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); end(null); } };
    addEventListener('keydown', key, true);
    d.addEventListener('click', e => { const b = e.target.closest('[data-a]'); if (b) end(b.dataset.a === '1'); else if (e.target === d && !modal) end(null); });
    document.body.appendChild(d);
    d.querySelector('[data-a="1"]').focus();
  });

  // the app's own question instead of confirm(): same look everywhere, and a browser that
  // suppresses native dialogs (automation, some embedded views) cannot swallow a delete
  window.ask = text => askYesNo(text, tr('js-yes'), tr('js-no'));

  // the bottom toast (layout.html renders the server's one the same way): a success fades, an error stays until closed
  window.toast = (text, err) => {
    $$('.toast:not(.net)').forEach(t => t.remove()); // the offline notice stays (practical.online)
    const t = document.createElement('div'); t.className = 'toast' + (err ? ' error' : ''); t.setAttribute('role', err ? 'alert' : 'status');
    t.innerHTML = '<span></span><button type="button" class="toast-x" aria-label="×">×</button>'; t.firstChild.textContent = text;
    const bye = () => { t.classList.add('gone'); setTimeout(() => t.remove(), 400); };
    t.lastChild.onclick = bye; if (!err) setTimeout(bye, 3000);
    document.body.appendChild(t);
  };

  window.segPress = btn => { const seg = btn.closest('.seg'); if (seg) seg.querySelectorAll('button').forEach(x => x.setAttribute('aria-pressed', x === btn ? 'true' : 'false')); };

  // colour scheme: the user's setting (auto | light | dark, stored on the server, rendered into <html>);
  // the button in the bar steps through the three and saves, the settings' segment follows
  const h = document.documentElement;
  const sb = $('#scheme');
  window.applyScheme = v => {
    h.dataset.prefScheme = v; h.dataset.scheme = v === 'auto' ? (matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light') : v;
    if (sb) sb.title = sb.dataset['t' + v[0].toUpperCase() + v.slice(1)];
  };
  if (sb) sb.addEventListener('click', async () => {
    const v = { auto: 'light', light: 'dark' }[h.dataset.prefScheme] || 'auto';
    applyScheme(v);
    const seg = $('.seg[data-field=scheme]');
    if (seg) { segPress($(`button[data-v="${v}"]`, seg)); seg.closest('.val').querySelector('input[name=scheme]').value = v; }
    if (!document.body.dataset.user) return; // the login page: for this visit only
    practical.tell(await practical.send('/settings/scheme', 'POST', { scheme: v }));
  });

  // confirmations on destructive forms
  $$('[data-confirm]').forEach(b => b.addEventListener('click', async e => { if (b.dataset.asked) { delete b.dataset.asked; return; } e.preventDefault(); if (await ask(b.dataset.confirm)) { b.dataset.asked = '1'; b.click(); } })); // the repeated click submits with this button

  // logout ends this browser's push subscription too: a shared device must not keep showing the user's reminders (at most 1.5 s)
  $$('form[action="/logout"]').forEach(f => f.addEventListener('submit', e => {
    if (f.dataset.done || !('serviceWorker' in navigator)) return;
    e.preventDefault(); f.dataset.done = '1';
    const off = navigator.serviceWorker.getRegistration('/').then(reg => reg && reg.pushManager.getSubscription()).then(sub => sub && Promise.all([practical.send('/api/push/subscriptions', 'DELETE', { endpoint: sub.endpoint }), sub.unsubscribe()]));
    Promise.race([off, new Promise(r => setTimeout(r, 1500))]).catch(() => {}).finally(() => f.submit());
  }));

  // copy-to-clipboard
  $$('[data-copy]').forEach(b => b.addEventListener('click', async () => {
    const el = $(b.dataset.copy); if (!el) return;
    try { await navigator.clipboard.writeText(el.textContent.trim()); toast(tr('js-copied')); } catch (_) { const r = document.createRange(); r.selectNodeContents(el); getSelection().removeAllRanges(); getSelection().addRange(r); }
  }));

  // toast: a success fades by itself, an error stays until closed
  $$('.toast').forEach(t => { const bye = () => { t.classList.add('gone'); setTimeout(() => t.remove(), 400); }; $('.toast-x', t).onclick = bye; if (!t.classList.contains('error')) setTimeout(bye, 3000); });
