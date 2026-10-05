/* PractiCal — shared behaviour (vanilla ES2020, no framework). */
/* i18n: the page ships its js-* strings (layout.html, from locales/*.ftl; placeholders %name);
   month and weekday names and date formats come from Intl in the page's locale */
(function () {
  let S = {};
  try { S = JSON.parse(document.getElementById('i18n').textContent); } catch (_) {}
  const LOC = window.LOC = document.body.dataset.locale || 'de-DE';
  window.tr = (k, v) => { let s = S[k] ?? k; if (v) for (const [n, x] of Object.entries(v)) s = s.split('%' + n).join(x); return s; };
  window.trs = (prefix, names) => Object.fromEntries(names.map(n => [n, tr(prefix + n)]));
  const f = o => new Intl.DateTimeFormat(LOC, o);
  window.dfmt = (d, o) => f(o).format(d);
  const days = style => Array.from({ length: 7 }, (_, i) => f({ weekday: style }).format(new Date(2024, 0, 1 + i)).replace(/\.$/, '')); // 2024-01-01 was a Monday
  window.MONTHS = Array.from({ length: 12 }, (_, i) => f({ month: 'long' }).format(new Date(2024, i, 1)));
  window.WEEKDAYS = days('short'); window.WEEKDAYS_LONG = days('long');
})();

(function () {
  const $ = (s, r = document) => r.querySelector(s);
  const $$ = (s, r = document) => [...r.querySelectorAll(s)];
  const csrf = () => ($('meta[name=csrf]') || {}).content || '';

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
    const watch = () => { ro.disconnect(); ['.topbar', '.toolbar'].forEach(s => { const e = $(s); if (e) ro.observe(e); }); setBars(); };
    watch(); const mainEl = $('.main'); if (mainEl) new MutationObserver(watch).observe(mainEl, { childList: true, subtree: true });
  } else { setBars(); addEventListener('resize', setBars); }

  /* segmented switch (`.seg`): exactly one button pressed. Every page switches through this one
     function — the look keys on aria-pressed="true", so a bare attribute ("") shows nothing */
  /* a minimal yes/no question over the page (the editor dialog's backdrop). Resolves true for "yes";
     Escape, "no" and a click on the backdrop resolve false. "Yes" has the focus, so Enter confirms.
     `modal`: the backdrop does not answer — a question whose "no" still changes something (a stray click
     must not decide it); Escape stays the explicit "no". */
  window.askYesNo = (text, yes, no, modal) => new Promise(done => {
    const d = document.createElement('div');
    d.className = 'ask'; d.setAttribute('role', 'alertdialog'); d.setAttribute('aria-modal', 'true');
    d.innerHTML = '<div class="ask-box"><p></p><div class="ask-acts"><button class="btn icon-btn" type="button" data-a="0"><svg class="ico" aria-hidden="true"><use href="#i-x"/></svg></button><button class="btn btn-primary icon-btn" type="button" data-a="1"><svg class="ico" aria-hidden="true"><use href="#i-check"/></svg></button></div></div>';
    d.querySelector('p').textContent = text;
    for (const [a, t] of [['0', no], ['1', yes]]) { const b = d.querySelector(`[data-a="${a}"]`); b.title = t; b.setAttribute('aria-label', t); }
    const prev = document.activeElement;
    const end = v => { removeEventListener('keydown', key, true); d.remove(); if (prev && prev.focus) prev.focus(); done(v); };
    const key = e => { if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); end(false); } };
    addEventListener('keydown', key, true);
    d.addEventListener('click', e => { const b = e.target.closest('[data-a]'); if (b) end(b.dataset.a === '1'); else if (e.target === d && !modal) end(false); });
    document.body.appendChild(d);
    d.querySelector('[data-a="1"]').focus();
  });

  // the app's own question instead of confirm(): same look everywhere, and a browser that
  // suppresses native dialogs (automation, some embedded views) cannot swallow a delete
  window.ask = text => askYesNo(text, tr('js-yes'), tr('js-no'));

  // the bottom toast (layout.html renders the server's one the same way): a success fades, an error stays until closed
  window.toast = (text, err) => {
    $$('.toast').forEach(t => t.remove());
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
    const r = await fetch('/settings/scheme', { method: 'POST', headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf() }, body: JSON.stringify({ scheme: v }) }).catch(() => null);
    if (!r || !r.ok) toast(tr('js-set-err'), true);
  });

  // confirmations on destructive forms
  $$('[data-confirm]').forEach(b => b.addEventListener('click', async e => { if (b.dataset.asked) { delete b.dataset.asked; return; } e.preventDefault(); if (await ask(b.dataset.confirm)) { b.dataset.asked = '1'; b.click(); } })); // the repeated click submits with this button

  // copy-to-clipboard
  $$('[data-copy]').forEach(b => b.addEventListener('click', async () => {
    const el = $(b.dataset.copy); if (!el) return;
    try { await navigator.clipboard.writeText(el.textContent.trim()); toast(tr('js-copied')); } catch (_) { const r = document.createRange(); r.selectNodeContents(el); getSelection().removeAllRanges(); getSelection().addRange(r); }
  }));

  // toast: a success fades by itself, an error stays until closed
  $$('.toast').forEach(t => { const bye = () => { t.classList.add('gone'); setTimeout(() => t.remove(), 400); }; $('.toast-x', t).onclick = bye; if (!t.classList.contains('error')) setTimeout(bye, 3000); });

  // settings: grouped lists with drill-in pages, one <section class="page"> per URL hash (without JS all are shown)
  const settings = $('.settings');
  if (settings) {
    const csrfTok = ($('meta[name=csrf]') || {}).content || '';
    // old hashes: the tabs of the previous layout and the server's redirect anchors
    const OLD = { profile: 'account', tokens: 'account', prefs: 'home', '': 'home' };
    let show = () => {
      let id = decodeURIComponent(location.hash.slice(1)); id = OLD[id] ?? id;
      const pg = $('#' + CSS.escape(id), settings);
      if (!pg || !pg.matches('.page')) id = 'home'; // empty or unknown hash; '#' alone is not a selector
      settings.classList.add('paged');
      $$('.page', settings).forEach(p => p.classList.toggle('on', p.id === id));
      settings.closest('.main').scrollTop = 0; // the anchor jump would land mid-page
    };
    addEventListener('hashchange', show); show();
    // a toast survives the reload that some saves need (language, a renamed calendar)
    if (sessionStorage.toast) { toast(sessionStorage.toast); delete sessionStorage.toast; }
    // settings → search: the history list (from /api/search/history); a row goes one by one, the button all at once
    const hp = $('#search', settings);
    if (hp) {
      const D = hp.dataset, list = $('#hist-list', hp), clr = $('#hist-clear', hp);
      const e2 = s => String(s ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
      const when = t => { const d = new Date(String(t).replace(' ', 'T') + 'Z'); return isNaN(d) ? '' : dfmt(d, { day: 'numeric', month: 'short', year: 'numeric', hour: '2-digit', minute: '2-digit' }); }; // stored in UTC
      const drawHist = async () => {
        let H = []; try { H = await (await fetch('/api/search/history')).json(); } catch (_) {}
        $('#hist-n', hp).textContent = H.length ? `(${H.length})` : ''; clr.disabled = !H.length;
        list.innerHTML = H.length ? H.map(e => `<div class="cell noic" data-id="${e.id}"><span>${e2(e.q)}<span class="sub">${e2([when(e.used_at), e.uses > 1 ? D.tUses.replace('{n}', e.uses) : '', e.target && e.target.label ? '→ ' + e.target.label : ''].filter(Boolean).join(' · '))}</span></span><span class="val"><button type="button" class="btn icon-btn" data-hrm title="${e2(D.tRm)}" aria-label="${e2(D.tRm)}"><svg class="ico" aria-hidden="true"><use href="#i-x"/></svg></button></span></div>`).join('')
          : `<div class="cell noic"><span class="muted">${e2(D.tEmpty)}</span></div>`;
      };
      list.addEventListener('click', async e => { const b = e.target.closest('[data-hrm]'); if (!b) return; await fetch('/api/search/history/' + b.closest('[data-id]').dataset.id, { method: 'DELETE', headers: { 'X-CSRF-Token': csrfTok } }); drawHist(); });
      clr.addEventListener('click', async () => { if (!await ask(D.tAsk)) return; await fetch('/api/search/history', { method: 'DELETE', headers: { 'X-CSRF-Token': csrfTok } }); drawHist(); });
      const onShow = () => { if (location.hash === '#search') drawHist(); };
      addEventListener('hashchange', onShow); onShow();
      // the home row names the choice; a shorter one drops old entries (the list shows the result after the save)
      $('#hist-days', hp).addEventListener('change', e => { $('#v-search').textContent = e.target.selectedOptions[0].textContent; setTimeout(drawHist, 600); });
    }
    // autosave: a changed field of a [data-autosave] form posts the whole form; the server answers JSON
    const save = async form => {
      const r = await fetch(form.action, { method: 'POST', headers: { Accept: 'application/json' }, body: new URLSearchParams(new FormData(form)) });
      const d = r.ok ? await r.json().catch(() => ({ ok: false, msg: String(r.status) })) : { ok: false, msg: String(r.status) };
      if (!d.ok) { toast(d.msg || String(r.status), true); return false; }
      return d.msg || settings.dataset.tSaved;
    };
    $$('form[data-autosave]', settings).forEach(form => {
      form.addEventListener('submit', e => e.preventDefault()); // Enter in a field saves through change, never navigates
      form.addEventListener('change', async e => {
        if (!e.target.name || e.target.type === 'file' || !form.reportValidity()) return;
        const msg = await save(form); if (!msg) return;
        if (e.target.name === 'scheme') applyScheme(e.target.value);
        if (form.hasAttribute('data-reload') || e.target.hasAttribute('data-reload')) { sessionStorage.toast = msg; location.reload(); } else toast(msg);
      });
    });
    // region lists (#regions-public, #regions-school): a hidden field each, a comma list of region codes in the
    // user's order; the first public one is the user's own (vacation days and bridge days count by it).
    // Pickers (#pick-country-KIND, #pick-region-KIND-CODE, #pick-tz): one page with a filter field over a list;
    // a region ticks on or off and the list returns, a region with subregions opens them as a further step
    const prefsForm = $('#prefs-form'), pickData = (() => { try { return JSON.parse($('#pick-data').textContent); } catch (_) { return { countries: [], zones: [], names: {} }; } })();
    const hid = n => $(`input[type=hidden][name="${n}"]`, prefsForm);
    const names = pickData.names || {}; // code → [name, path above it]
    let subs = null, subsFor = '';
    const loadSubs = async c => { if (subsFor !== c) { const r = await fetch('/api/holidays/subdivisions?country=' + encodeURIComponent(c)); subs = r.ok ? await r.json() : []; subsFor = c; } return subs; };
    const esc2 = x => String(x).replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
    const commit = (name, value) => { const i = hid(name); i.value = value; i.dispatchEvent(new Event('change', { bubbles: true })); };
    const pathOf = (list, r) => { const out = []; let x = r; while (x && out.length < 5) { out.unshift(x.name); x = list.find(y => y.code === x.parent); } return out; };
    const countryName = c => (pickData.countries.find(x => x.code === c) || { name: c }).name;
    const listOf = kind => hid(kind + '_regions').value.split(',').filter(Boolean);
    const nameOf = c => (names[c] || [c])[0];
    const summary = kind => { const l = listOf(kind); $('#v-regions-' + kind).textContent = !l.length ? tr('js-regions-none') : l.length < 3 ? l.map(nameOf).join(', ') : tr('js-regions-n', { n: l.length }); };
    const setList = (kind, l) => { commit(kind + '_regions', l.join(',')); summary(kind); $('#v-sources').hidden = true; /* the sources line is the server's; it returns with the next visit */ };
    const icoBtn = (attr, ico, label) => `<button type="button" class="btn icon-btn" ${attr} title="${esc2(label)}" aria-label="${esc2(label)}"><svg class="ico" aria-hidden="true"><use href="#${ico}"/></svg></button>`;
    const drawRegions = kind => {
      const l = listOf(kind);
      $(`#regions-${kind} .rlist`).innerHTML = l.length ? l.map((c, i) => `<div class="cell noic" data-code="${esc2(c)}"><span>${esc2(nameOf(c))}${(names[c] || [])[1] ? `<span class="sub">${esc2(names[c][1])}</span>` : ''}</span><span class="val">${kind === 'public' ? (i ? icoBtn('data-first', 'i-star', tr('js-region-first')) : `<span class="badge">${esc2(tr('js-region-own'))}</span>`) : ''}${icoBtn('data-rm', 'i-x', tr('js-region-remove'))}</span></div>`).join('')
        : `<div class="cell noic"><span class="muted">${esc2(tr('js-regions-none'))}</span></div>`;
    };
    $$('.page.regions', settings).forEach(pg => $('.rlist', pg).addEventListener('click', e => {
      const b = e.target.closest('button'); if (!b) return;
      const kind = pg.dataset.kind, c = b.closest('[data-code]').dataset.code, l = listOf(kind).filter(x => x !== c);
      if (b.hasAttribute('data-first')) l.unshift(c);
      setList(kind, l); drawRegions(kind);
    }));
    async function pick(route) {
      const title = $('#pick-title'), q = $('#pick-q'), box = $('#pick-list'), back = $('#pick .back');
      const backTo = (href, text) => { back.setAttribute('href', href); back.textContent = '‹ ' + text; };
      let rows = [], search = null, kind = '', on = () => false, count = () => 0;
      const rm = route.match(/^region-(public|school)-([A-Z0-9-]+)$/), cm = route.match(/^country-(public|school)$/);
      if (route === 'tz') {
        backTo('#', $('#home h2').textContent);
        title.textContent = tr('js-pick-tz'); const cur = hid('timezone').value; on = v => v === cur;
        rows = [{ v: '', name: tr('js-pick-default', { tz: prefsForm.dataset.defaultTz }) }];
        pickData.zones.forEach(([area, zs]) => { rows.push({ head: area }); zs.forEach(([id, name]) => rows.push({ v: id, name, sub: id })); });
      } else if (cm) {
        kind = cm[1]; backTo('#regions-' + kind, $(`#regions-${kind} h2`).textContent);
        title.textContent = tr('js-pick-country');
        count = v => listOf(kind).filter(x => x.slice(0, 2) === v).length;
        rows = pickData.countries.filter(c => kind !== 'school' || c.school).map(c => ({ v: c.code, name: c.name, more: true }));
      } else if (rm) {
        kind = rm[1]; const parent = rm[2], country = parent.slice(0, 2), list = await loadSubs(country), cname = countryName(country);
        const p = list.find(x => x.code === parent), full = x => [cname].concat(pathOf(list, x).slice(0, -1)).join(' › ');
        title.textContent = p ? p.name : cname;
        if (p) backTo('#pick-region-' + kind + '-' + p.parent, (list.find(x => x.code === p.parent) || { name: cname }).name); else backTo('#pick-country-' + kind, tr('js-pick-country'));
        on = v => listOf(kind).includes(v);
        const kids = code => list.filter(x => x.parent === code);
        rows = [p ? { v: p.code, name: tr('js-pick-all', { name: p.name }), nm: [p.name, full(p)] } : { v: country, name: tr('js-pick-whole'), nm: [cname, ''] }]
          .concat(kids(parent).map(x => ({ v: x.code, name: x.name, more: list.some(y => y.parent === x.code), nm: [x.name, full(x)] })));
        search = list.map(x => ({ v: x.code, name: x.name, sub: pathOf(list, x).slice(0, -1).join(' › '), nm: [x.name, full(x)] }));
      } else { location.hash = ''; return; } // an old or broken address
      const draw = () => {
        const f = q.value.trim().toLowerCase(), norm = x => x.normalize('NFD').replace(/[\u0300-\u036f]/g, '').toLowerCase();
        const src = f && search ? search : rows, nf = norm(f);
        const hit = r => r.head || !f || norm(r.name).includes(nf) || (r.sub && norm(r.sub).includes(nf));
        const shown = src.filter(hit).filter((r, i, a) => !r.head || (a[i + 1] && !a[i + 1].head));
        box.innerHTML = shown.length ? shown.map(r => { if (r.head) return `<div class="cell noic head"><span class="label">${esc2(r.head)}</span></div>`;
          const sel = on(r.v), n = count(r.v);
          return `<button type="button" class="cell noic link${sel ? ' on' : ''}" data-v="${esc2(r.v)}"${r.more ? ' data-more="1"' : ''} role="option" aria-selected="${sel}"><span>${esc2(r.name)}${r.sub && (f || route !== 'tz') ? `<span class="sub">${esc2(r.sub)}</span>` : ''}</span><span class="val">${n ? `<span class="badge">${n}</span>` : ''}${sel ? '<svg class="ico" aria-hidden="true"><use href="#i-check"/></svg>' : ''}${r.more ? '<span class="chev">›</span>' : ''}</span></button>`; }).join('')
          : `<div class="cell noic"><span class="muted">${esc2(tr('js-pick-none'))}</span></div>`;
      };
      q.value = ''; q.oninput = draw; draw();
      if (matchMedia('(pointer: fine)').matches) q.focus();
      box.onclick = e => {
        const b = e.target.closest('button[data-v]'); if (!b) return;
        const v = b.dataset.v;
        if (b.dataset.more) { location.hash = '#pick-region-' + kind + '-' + v; return; }
        if (route === 'tz') { commit('timezone', v); $('#v-tz').textContent = v ? v.replace(/_/g, ' ') : b.textContent; location.hash = ''; return; }
        // a region ticks on or off; the list shows the result
        const r = (search || []).concat(rows).find(x => x.v === v); if (r && r.nm) names[v] = r.nm;
        const l = listOf(kind); setList(kind, l.includes(v) ? l.filter(x => x !== v) : l.concat(v));
        location.hash = '#regions-' + kind;
      };
    }
    const baseShow = show;
    show = () => {
      const id = decodeURIComponent(location.hash.slice(1));
      if (id.startsWith('pick-') && $('#pick')) { settings.classList.add('paged'); $$('.page', settings).forEach(p => p.classList.toggle('on', p.id === 'pick')); pick(id.slice(5)); settings.closest('.main').scrollTop = 0; return; }
      if (id.startsWith('regions-') && $('#' + CSS.escape(id))) drawRegions(id.slice(8));
      baseShow();
    };
    removeEventListener('hashchange', baseShow); addEventListener('hashchange', show); show();
    // a segment stands for one hidden field: pressing sets it and saves like any other change
    $$('.seg[data-field]', settings).forEach(sg => sg.addEventListener('click', e => {
      const b = e.target.closest('button[data-v]'); if (!b || b.getAttribute('aria-pressed') === 'true') return;
      segPress(b); const inp = sg.closest('.val').querySelector(`input[name="${sg.dataset.field}"]`); inp.value = b.dataset.v; inp.dispatchEvent(new Event('change', { bubbles: true }));
    }));
    // a flash after a failed confirmation should keep the details open
    // a danger zone (delete/purge a group) asks in a dialog: its text, the value to type, cancel or go.
    // Without JS the <details> opens inline and works the same.
    $$('details.danger-zone', settings).forEach(d => $('summary', d).addEventListener('click', e => {
      e.preventDefault();
      const form = $('form', d), inp = $('input[name=confirm]', form), go = $('button[type=submit]', form), want = inp.placeholder;
      const w = document.createElement('div');
      w.className = 'ask'; w.setAttribute('role', 'alertdialog'); w.setAttribute('aria-modal', 'true');
      w.innerHTML = '<form class="ask-box"><p></p><label class="ask-type"><span></span> <b></b><input type="text" autocomplete="off"></label><div class="ask-acts"><button class="btn icon-btn" type="button"><svg class="ico" aria-hidden="true"><use href="#i-x"/></svg></button><button class="btn danger icon-btn" type="submit" disabled><svg class="ico" aria-hidden="true"><use href="#i-trash-2"/></svg></button></div></form>';
      $('p', w).textContent = ($('p', form) || {}).textContent || ''; $('.ask-type span', w).textContent = form.dataset.tType; $('.ask-type b', w).textContent = want;
      const [no, yes] = $$('.ask-acts button', w); const goText = go.textContent.trim(); no.title = form.dataset.tCancel; no.setAttribute('aria-label', no.title); yes.title = goText; yes.setAttribute('aria-label', goText);
      const typed = $('.ask-type input', w); // no placeholder: a greyed copy of the answer reads as already filled in
      const end = () => { removeEventListener('keydown', key, true); w.remove(); };
      const key = ev => { if (ev.key === 'Escape') { ev.preventDefault(); ev.stopPropagation(); end(); } };
      addEventListener('keydown', key, true);
      typed.oninput = () => { yes.disabled = typed.value.trim() !== want; };
      no.onclick = end; w.onclick = ev => { if (ev.target === w) end(); };
      $('form', w).onsubmit = ev => { ev.preventDefault(); if (yes.disabled) return; inp.value = typed.value.trim(); end(); form.submit(); };
      document.body.appendChild(w); typed.focus();
    }));
    // reminder strip snap points (#snaps): the two hidden fields keep the lists as the server writes them
    // ("2 min, 1 h, 1 d"). The strip draws them in the order of the event editor's strip (start points far to
    // near, the start, end points, the end); a point opens an edit row below it, a "+" in a gap offers the
    // round value between its neighbours. An empty start list would mean the built-in one: the last start point stays.
    const snapBox = $('#snaps');
    if (snapBox) {
      const UNITS = [[1, 'min'], [60, 'h'], [1440, 'd'], [10080, 'w']], MAXM = 366 * 1440;
      const NICE = [1, 2, 3, 4, 5, 10, 15, 20, 25, 30, 40, 45, 60, 90, 120, 150, 180, 240, 300, 360, 480, 600, 720, 960, 1080, 1440, 2160, 2880, 4320, 5760, 7200, 8640, 10080, 14400, 20160, 30240, 40320, 60480, 80640, 120960, 241920, 524160];
      const unitOf = m => [...UNITS].reverse().find(([u]) => m % u === 0);
      const parseL = v => v.split(',').map(t => { const x = t.trim().match(/^(\d+)\s*(min|h|d|w)$/); return x ? +x[1] * UNITS.find(u => u[1] === x[2])[0] : 0; }).filter(m => m > 0 && m <= MAXM);
      const fmtL = l => l.map(m => { const [u, k] = unitOf(m); return `${m / u} ${k}`; }).join(', ');
      const label = m => { const [u, k] = unitOf(m), n = m / u; return k === 'min' || k === 'h' ? tr('js-snp-' + k, { n }) : n === 1 ? tr(`js-snp-${k}1`) : tr('js-snp-' + k, { n }); };
      const side = k => tr(k === 'start' ? 'js-snp-start' : 'js-snp-end');
      const sortU = l => [...new Set(l)].sort((a, b) => a - b), same = (a, b) => a.length === b.length && a.every((v, i) => v === b[i]);
      let L = { start: sortU(parseL(hid('alarm_snaps_start').value)), end: sortU(parseL(hid('alarm_snaps_end').value)) };
      const DEF = { start: sortU(parseL(snapBox.dataset.defaultStart)), end: sortU(parseL(snapBox.dataset.defaultEnd)) };
      const resetBtn = $('#snaps-reset');
      let edit = null; // { key, m } of the point in the edit row
      const canRm = k => k !== 'start' || L.start.length > 1;
      const store = () => {
        L = { start: sortU(L.start), end: sortU(L.end) };
        hid('alarm_snaps_start').value = fmtL(L.start); commit('alarm_snaps_end', fmtL(L.end)); // one change posts the whole form
        $('#v-snaps').textContent = L.start.length === 1 ? tr('js-snaps-one') : tr('js-snaps-n', { n: L.start.length });
      };
      const draw = focus => {
        const o = [...L.start].reverse().map(m => ({ key: 'start', m })).concat([{ fix: 'start' }], [...L.end].reverse().map(m => ({ key: 'end', m })), [{ fix: 'end' }]);
        o.forEach((p, i) => { p.x = (i + 1) / (o.length + 1) * 100; });
        const b = o.find(p => p.fix === 'start').x, e = o[o.length - 1].x;
        let h = `<div class="snp-ax"></div><div class="snp-ev" style="left:${b}%;width:${e - b}%"></div>`, alt = 0;
        for (const p of o) {
          alt ^= 1; // labels alternate above and below, the start and the end included, so neighbours never share a row
          if (p.fix) { h += `<span class="snp-pt fix ${alt ? 'up' : 'dn'}" style="left:${p.x}%">${esc2(tr(p.fix === 'start' ? 'js-snp-begin' : 'js-snp-finish'))}</span>`; continue; }
          const on = !!edit && edit.key === p.key && edit.m === p.m;
          h += `<button type="button" class="snp-pt ${alt ? 'up' : 'dn'}${on ? ' on' : ''}" data-key="${p.key}" data-m="${p.m}" style="left:${p.x}%" aria-label="${esc2(label(p.m) + ' ' + side(p.key))}" aria-expanded="${on}">${esc2(label(p.m))}</button>`;
        }
        // a gap belongs to the side of the point on its right (the start counts as a start point, the end as an end
        // point); its "+" offers the round value nearest to the geometric middle of its neighbours
        for (let i = 0; i < o.length; i++) {
          const left = o[i - 1], right = o[i], key = right.key === 'start' || right.fix === 'start' ? 'start' : 'end';
          const hi = left && left.key === key ? left.m : null, lo = right.key ? right.m : 0;
          const top = hi ?? (key === 'start' ? Math.min(lo * 4, MAXM) : lo ? Math.min(lo * 2, 1440) : 60), mid = Math.sqrt(Math.max(lo, 1) * top);
          const v = NICE.filter(n => n > lo && n <= top && n !== hi && !L[key].includes(n)).sort((x, y) => Math.abs(Math.log(x / mid)) - Math.abs(Math.log(y / mid)))[0];
          if (v) { const t = tr('js-snp-add', { what: label(v) + ' ' + side(key) }); h += `<button type="button" class="snp-plus" data-add="${key}" data-m="${v}" style="left:${((left ? left.x : 0) + right.x) / 2}%" title="${esc2(t)}" aria-label="${esc2(t)}">+</button>`; }
        }
        let row = '';
        if (edit) {
          const [u] = unitOf(edit.m), last = !canRm(edit.key);
          row = `<div class="cell noic snp-edit"><span>${esc2(label(edit.m) + ' ' + side(edit.key))}<span class="sub snp-err" role="alert"></span></span><span class="val"><input type="number" min="1" step="1" value="${edit.m / u}" aria-label="${esc2(tr('js-snp-amount'))}"><span class="seg">${UNITS.map(([uu, k]) => `<button type="button" data-u="${uu}" aria-pressed="${uu === u}">${esc2(tr('js-snp-u-' + k))}</button>`).join('')}</span><button type="button" class="btn" data-ok><svg class="ico" aria-hidden="true"><use href="#i-pencil"/></svg><span>${esc2(tr('js-snp-change'))}</span></button><button type="button" class="btn danger" data-del${last ? ` disabled title="${esc2(tr('js-snp-last'))}"` : ''}><svg class="ico" aria-hidden="true"><use href="#i-trash"/></svg><span>${esc2(tr('js-snp-remove'))}</span></button><button type="button" class="btn icon-btn" data-close title="${esc2(tr('js-snp-close'))}" aria-label="${esc2(tr('js-snp-close'))}"><svg class="ico" aria-hidden="true"><use href="#i-x"/></svg></button></span></div>`;
        }
        const sc = $('.snp-scroll', snapBox), keep = sc ? sc.scrollLeft : 0; // redrawing must not jump a scrolled strip back
        snapBox.innerHTML = `<div class="snp-scroll"><div class="snp-es" style="min-width:${(o.length + 1) * 31}px">${h}</div></div>${row}`;
        $('.snp-scroll', snapBox).scrollLeft = keep;
        resetBtn.hidden = same(L.start, DEF.start) && same(L.end, DEF.end);
        if (focus === 'edit') $('.snp-edit input', snapBox).select();
        else if (focus) { const f = $(`.snp-pt[data-key="${focus.key}"][data-m="${focus.m}"]`, snapBox); if (f) f.focus(); }
      };
      const apply = () => {
        const inp = $('.snp-edit input', snapBox), err = $('.snp-err', snapBox), n = parseFloat(inp.value), m = Math.round(n * +$('.snp-edit .seg [aria-pressed=true]', snapBox).dataset.u);
        const bad = !(n > 0) ? 'js-snp-bad' : m > MAXM ? 'js-snp-max' : m !== edit.m && L[edit.key].includes(m) ? 'js-snp-dup' : '';
        if (bad) { err.textContent = tr(bad); inp.focus(); return; }
        const key = edit.key; if (m !== edit.m) { L[key] = L[key].filter(x => x !== edit.m).concat(m); store(); }
        edit = null; draw({ key, m });
      };
      snapBox.addEventListener('click', e => {
        const t = e.target.closest('button'); if (!t) return;
        if (t.dataset.add) { const key = t.dataset.add, m = +t.dataset.m; L[key].push(m); edit = null; store(); draw({ key, m }); }
        else if (t.matches('.snp-pt[data-m]')) { const k = { key: t.dataset.key, m: +t.dataset.m }; const was = edit && edit.key === k.key && edit.m === k.m; edit = was ? null : k; draw(was ? k : 'edit'); }
        else if (t.dataset.u) segPress(t);
        else if (t.hasAttribute('data-ok')) apply();
        else if (t.hasAttribute('data-del')) { const k = edit; L[k.key] = L[k.key].filter(x => x !== k.m); edit = null; store(); draw(); }
        else if (t.hasAttribute('data-close')) { const k = edit; edit = null; draw(k); }
      });
      snapBox.addEventListener('keydown', e => {
        if (!edit || !e.target.closest('.snp-edit')) return;
        if (e.key === 'Enter') { e.preventDefault(); apply(); } else if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); const k = edit; edit = null; draw(k); }
      });
      resetBtn.addEventListener('click', () => { L = { start: [...DEF.start], end: [...DEF.end] }; edit = null; store(); draw(); });
      draw();
    }
    // vacation rule (#vacation): one kind (category · calendar · title) and one field per kind; the server
    // reads the field of the chosen kind. Categories come from /api/categories; the title pattern is tested
    // live against every event title (/api/titles). The server matches with Rust's regex crate, any case:
    // the test translates \b and (?P< for JS and refuses what the crate lacks (look-around, back-references),
    // and an invalid pattern blocks the autosave through the field's validity.
    const vacKind = hid('vacation_kind');
    if (vacKind && $('#vac-re')) {
      const catSel = $('#vac-cat'), calSel = $('#vac-cal'), re = $('#vac-re'), tools = $('#vac-tools'), test = $('#vac-test');
      let titles = null, cats = null, shown = 'match';
      const summary = () => { const k = vacKind.value; $('#v-vacation').textContent = k === 'title' ? `/${re.value}/` : `„${k === 'calendar' ? (calSel.selectedOptions[0] || {}).textContent || '' : catSel.value}“`; };
      const loadCats = async () => {
        if (cats) return; cats = [];
        try { const r = await fetch('/api/categories?scope=event'); if (r.ok) cats = await r.json(); } catch (_) {}
        const cur = catSel.value, list = cats.filter(c => c.count || c.name === cur);
        if (cur && !list.some(c => c.name === cur)) list.unshift({ name: cur, count: 0 });
        catSel.innerHTML = (cur ? '' : '<option value="">–</option>') + list.map(c => `<option value="${esc2(c.name)}">${esc2(c.name)} (${c.count || tr('js-vac-no-events')})</option>`).join('');
        catSel.value = cur;
      };
      // a switch to "category" with none chosen yet: the category the title pattern matches, else the most used
      const pickCat = () => { const c = re.value.trim() ? compile(re.value.trim()) : {}, list = (cats || []).filter(x => x.count); const hit = c.rx && list.find(x => c.rx.test(x.name)); return (hit || list[0] || {}).name || ''; };
      // Rust's \b and \w are Unicode-aware, JS's are ASCII even with /u: \b and \B become look-arounds over letters,
      // digits, marks and connectors (outside character classes)
      const W = '[\\p{L}\\p{N}\\p{M}\\p{Pc}]', WB = `(?:(?<=${W})(?!${W})|(?<!${W})(?=${W}))`, NWB = `(?:(?<=${W})(?=${W})|(?<!${W})(?!${W}))`;
      const toJs = p => { let out = '', cls = false; for (let i = 0; i < p.length; i++) { const c = p[i];
        if (c === '\\' && i + 1 < p.length) { const n = p[i + 1]; out += !cls && n === 'b' ? WB : !cls && n === 'B' ? NWB : c + n; i++; continue; }
        if (c === '[') cls = true; else if (c === ']') cls = false; out += c; } return out.replace(/\(\?P</g, '(?<'); };
      const compile = p => { if (/\(\?<?[=!]|\\[1-9]|\\k</.test(p)) return { err: tr('js-vac-unsup') }; try { return { rx: new RegExp(toJs(p), 'iu') }; } catch (e) { return { err: tr('js-vac-bad', { msg: e.message.replace(/^Invalid regular expression: \/.*\/[a-z]*: /s, '') }) }; } }; // the engine's text quotes the translated pattern: keep the reason only
      const mark = (t, rx) => { const g = new RegExp(rx.source, 'giu'); let out = '', last = 0, m; while ((m = g.exec(t))) { if (!m[0].length) { g.lastIndex++; continue; } out += esc2(t.slice(last, m.index)) + `<mark>${esc2(m[0])}</mark>`; last = m.index + m[0].length; } return out + esc2(t.slice(last)); };
      const run = () => {
        const p = re.value.trim(), c = p ? compile(p) : {};
        re.setCustomValidity(c.err || ''); re.classList.toggle('bad', !!c.err);
        if (c.err) { test.innerHTML = `<p class="vac-err">${esc2(c.err)}</p>`; return; }
        if (!p) { test.innerHTML = `<p class="vac-note">${esc2(tr('js-vac-empty'))}</p>`; return; }
        if (!titles) { test.innerHTML = `<p class="vac-note">${esc2(tr('js-vac-loading'))}</p>`; return; }
        const hit = titles.filter(([t]) => c.rx.test(t)), miss = titles.filter(([t]) => !c.rx.test(t)), list = shown === 'match' ? hit : miss, MAX = 40;
        test.innerHTML = `<div class="vac-th"><span>${esc2(hit.length ? tr('js-vac-hits', { hits: hit.length, all: titles.length, events: hit.reduce((s, [, n]) => s + n, 0) }) : tr('js-vac-none'))}</span><span class="seg"><button type="button" data-show="match" aria-pressed="${shown === 'match'}">${esc2(tr('js-vac-match'))} · ${hit.length}</button><button type="button" data-show="miss" aria-pressed="${shown === 'miss'}">${esc2(tr('js-vac-miss'))} · ${miss.length}</button></span></div>`
          + `<ul class="vac-list">${list.slice(0, MAX).map(([t, n]) => `<li class="${shown}"><span>${shown === 'match' ? mark(t, c.rx) : esc2(t)}</span><small>${n}×</small></li>`).join('')}</ul>${list.length > MAX ? `<p class="vac-note">${esc2(tr('js-vac-more', { n: list.length - MAX }))}</p>` : ''}`;
      };
      const loadTitles = async () => { if (titles) return; run(); try { const r = await fetch('/api/titles'); titles = r.ok ? await r.json() : []; } catch (_) { titles = []; } run(); };
      // building blocks: each inserts at the cursor (a selection is wrapped where that makes sense)
      const TOOLS = [['word', '\\b', '\\b'], ['begin', '^', ''], ['end', '', '$'], ['or', '|', ''], ['any', '.*', '']];
      tools.innerHTML = TOOLS.map(([k, a, b]) => `<button type="button" class="chip" data-tool="${k}"><code>${esc2(k === 'word' ? '\\b…\\b' : a + b)}</code> ${esc2(tr('js-vac-' + k))}</button>`).join('');
      tools.addEventListener('click', e => {
        const b = e.target.closest('[data-tool]'); if (!b) return; const [k, a, z] = TOOLS.find(t => t[0] === b.dataset.tool);
        let v = re.value, s = re.selectionStart ?? v.length, en = re.selectionEnd ?? v.length;
        if (k === 'begin') { if (!v.startsWith('^')) { v = '^' + v; s++; en++; } s = en = s; }
        else if (k === 'end') { if (!v.endsWith('$')) v += '$'; s = en = v.length; }
        else { v = v.slice(0, s) + a + v.slice(s, en) + z + v.slice(en); s += a.length; en += a.length; if (k !== 'word') s = en; }
        re.value = v; re.focus(); re.setSelectionRange(s, en); run(); re.dispatchEvent(new Event('change', { bubbles: true })); summary();
      });
      test.addEventListener('click', e => { const b = e.target.closest('[data-show]'); if (b) { shown = b.dataset.show; run(); } });
      let tm = 0; re.addEventListener('input', () => { clearTimeout(tm); tm = setTimeout(run, 120); });
      re.addEventListener('change', summary); catSel.addEventListener('change', summary); calSel.addEventListener('change', summary);
      const showKind = () => {
        const k = vacKind.value; $$('.vac-k', settings).forEach(r => { r.hidden = r.dataset.k !== k; });
        if (k === 'title') loadTitles();
      };
      // the kind switch fires `change` on the hidden field before the form's autosave sees it: an empty list
      // gets its first entry here, so the switch saves a rule that counts something
      vacKind.addEventListener('change', () => {
        if (vacKind.value === 'calendar' && !calSel.value) { const o = [...calSel.options].find(x => x.value); if (o) calSel.value = o.value; }
        if (vacKind.value === 'category' && !catSel.value) { const n = pickCat(); if (n) catSel.value = n; }
        showKind(); summary();
      });
      loadCats(); // before any switch, so the switch finds its list
      showKind(); if (vacKind.value === 'title') run();
    }
    // categories: the index with counts; a click opens the category's rule in the calendar's (or tasks') editor
    const catPanel = $('#categories');
    if (catPanel) {
      const t = catPanel.dataset, list = $('#cat-list'), h = s => String(s).replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
      let scope = 'event';
      const ruleUrl = n => (scope === 'task' ? '/tasks' : '/calendar') + '?rule=' + encodeURIComponent(n) + '&back=' + encodeURIComponent('/settings#categories');
      const load = async () => {
        const r = await fetch('/api/categories?scope=' + scope); const d = r.ok ? await r.json() : [];
        if (scope === 'event') $('#cat-total').textContent = d.length;
        list.innerHTML = d.length ? d.map(c => `<a class="cell noic link" href="${ruleUrl(c.name)}"><span>${h(c.name)}</span><span class="val">${c.rule ? `<span class="badge">${h(t.tRule)}</span>` : ''}${c.count ? h((scope === 'task' ? t.tCountTask : t.tCount).replace('{n}', c.count)) : ''}<span class="chev">›</span></span></a>`).join('') : `<div class="cell noic"><span class="muted">${h(t.tNone)}</span></div>`;
      };
      $$('#cat-scope button').forEach(b => b.onclick = () => { scope = b.dataset.scope; segPress(b); load(); });
      $('#cat-new').addEventListener('submit', e => { e.preventDefault(); const n = $('#cat-new-name').value.trim(); if (n) location.href = ruleUrl(n); });
      load();
    }
  }

  // user menu: click toggles (hover works via CSS, touch needs the click)
  $$('.filter > button').forEach(b => b.addEventListener('click', e => { e.stopPropagation(); b.parentElement.classList.toggle('open'); }));
  document.addEventListener('click', e => { if (!e.target.closest('.filter')) $$('.filter.open').forEach(f => f.classList.remove('open')); });
  // keep every hover/click popover inside the screen. Its CSS anchor (right: 0, or -60px on a phone) is
  // right for some toolbar positions only: the contacts column picker sits at the left of a wrapped
  // toolbar row at 390px (ran 148px off the left edge) and at the right at 600px. Measured while hidden
  // too — visibility keeps the layout — so a closed popover never widens the page.
  const fitPop = pop => {
    pop.style.translate = '';
    const r = pop.getBoundingClientRect();
    if (!r.width) return;
    let dx = Math.min(0, innerWidth - 8 - r.right);
    if (r.left + dx < 8) dx = 8 - r.left;
    if (dx) pop.style.translate = dx + 'px 0';
  };
  const fitPops = () => $$('.filter > .pop, .more > .pop').forEach(fitPop);
  fitPops(); addEventListener('resize', fitPops);
  $$('.filter, .more').forEach(f => ['pointerenter', 'focusin', 'click'].forEach(t => f.addEventListener(t, () => { const p = f.querySelector(':scope > .pop'); if (p) fitPop(p); })));

  // "/" focuses the search
  const q = $('#q');
  if (q) addEventListener('keydown', e => { if (e.key === '/' && document.activeElement !== q && !/input|textarea|select/i.test(document.activeElement.tagName)) { e.preventDefault(); q.focus(); } });

  // live updates: server-sent changes → custom event for the views
  if (document.body.dataset.user && 'EventSource' in window) {
    let es; const connect = () => {
      es = new EventSource('/api/changes');
      es.addEventListener('change', ev => { try { document.dispatchEvent(new CustomEvent('rustical:change', { detail: JSON.parse(ev.data) })); } catch (_) {} });
      es.onerror = () => { es.close(); setTimeout(connect, 5000 + Math.random() * 5000); };
    };
    connect();
  }

  window.practical = { csrf, $, $$ };
})();

/* ===== colour picker (round 13): one picker for every colour in the app. The popover offers the colours
   already in use (GET /api/colors), each named by its sources, plus "like the calendar" or "standard"
   where the element has such a fallback; ＋ opens the dialog for a new colour: a fan deck or a text
   field with three sliders, the distance to the colours in use, and a light and dark preview. ===== */
(function () {
  const $ = (s, r = document) => r.querySelector(s), $$ = (s, r = document) => [...r.querySelectorAll(s)];
  const csrf = () => ($('meta[name=csrf]') || {}).content || '';
  const esc = s => String(s).replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
  const clamp = (x, a = 0, b = 1) => Math.min(b, Math.max(a, x));
  const num = new Intl.NumberFormat(window.LOC, { minimumFractionDigits: 1, maximumFractionDigits: 1 });

  // ---- colour math: sRGB, HSV/HSL, OKLab/OKLCH (perceptual: equal steps look equal), WCAG contrast ----
  const C = {};
  C.rgb = hex => { const n = parseInt(hex.slice(1), 16); return [(n >> 16 & 255) / 255, (n >> 8 & 255) / 255, (n & 255) / 255]; };
  C.hex = rgb => '#' + rgb.map(v => Math.round(clamp(v) * 255).toString(16).padStart(2, '0')).join('');
  C.lin = c => c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  C.gam = c => c <= 0.0031308 ? 12.92 * c : 1.055 * c ** (1 / 2.4) - 0.055;
  C.hsv = ([r, g, b]) => { const mx = Math.max(r, g, b), mn = Math.min(r, g, b), d = mx - mn; let h = 0; if (d) h = mx === r ? ((g - b) / d) % 6 : mx === g ? (b - r) / d + 2 : (r - g) / d + 4; return [(h * 60 + 360) % 360, mx ? d / mx : 0, mx]; };
  C.fromHsv = (h, s, v) => { const f = n => { const k = (n + h / 60) % 6; return v - v * s * Math.max(0, Math.min(k, 4 - k, 1)); }; return [f(5), f(3), f(1)]; };
  C.hsl = rgb => { const [h, s, v] = C.hsv(rgb), l = v * (1 - s / 2); return [h, l === 0 || l === 1 ? 0 : (v - l) / Math.min(l, 1 - l), l]; };
  C.fromHsl = (h, s, l) => { const v = l + s * Math.min(l, 1 - l); return C.fromHsv(h, v ? 2 * (1 - l / v) : 0, v); };
  C.lab = rgb => { const [r, g, b] = rgb.map(C.lin);
    const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b), m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b), s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
    return [0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s, 1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s, 0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s]; };
  const labLin = ([L, a, b]) => { const l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3, m = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3, s = (L - 0.0894841775 * a - 1.2914855480 * b) ** 3;
    return [4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s, -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s, -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s]; };
  const lchLin = (L, c, h) => labLin([L, c * Math.cos(h * Math.PI / 180), c * Math.sin(h * Math.PI / 180)]);
  C.lch = hex => { const [L, a, b] = C.lab(C.rgb(hex)); return [L, Math.hypot(a, b), (Math.atan2(b, a) * 180 / Math.PI + 360) % 360]; };
  C.maxC = (L, h) => { let lo = 0, hi = 0.4; for (let i = 0; i < 18; i++) { const m = (lo + hi) / 2; if (lchLin(L, m, h).every(v => v >= -1e-4 && v <= 1 + 1e-4)) lo = m; else hi = m; } return lo; };
  // OKLCH to hex; a chroma outside sRGB is pulled in at the same lightness and hue
  C.fromLch = (L, c, h) => { L = clamp(L); c = Math.min(Math.max(c, 0), C.maxC(L, h)); return C.hex(lchLin(L, c, h).map(v => C.gam(clamp(v)))); };
  C.lum = hex => { const [r, g, b] = C.rgb(hex).map(C.lin); return 0.2126 * r + 0.7152 * g + 0.0722 * b; };
  C.contrast = (a, b) => { const x = C.lum(a), y = C.lum(b); return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05); };
  C.dist = (a, b) => { const p = C.lab(C.rgb(a)), q = C.lab(C.rgb(b)); return Math.hypot(p[0] - q[0], p[1] - q[1], p[2] - q[2]); };
  // anything CSS understands (#abc, a name, rgb(), var(--c3)) → #rrggbb, '' when it is no colour
  const toHex = v => {
    v = String(v || '').trim(); if (/^[0-9a-f]{6}$/i.test(v) || /^[0-9a-f]{3}$/i.test(v)) v = '#' + v;
    if (/^#[0-9a-f]{3}$/i.test(v)) v = '#' + [...v.slice(1)].map(x => x + x).join('');
    if (/^#[0-9a-f]{6}$/i.test(v)) return v.toLowerCase();
    const el = document.createElement('i'); el.style.color = v; if (!el.style.color) return '';
    document.body.appendChild(el); const m = getComputedStyle(el).color.match(/[\d.]+/g); el.remove();
    return m ? C.hex(m.slice(0, 3).map(n => +n / 255)) : '';
  };
  // the page and dark backgrounds of the preview; a colour needs 3:1 on both to be seen
  const LIGHT = '#ffffff', DARK = '#0f1115';
  const visible = x => C.contrast(x, LIGHT) >= 3 && C.contrast(x, DARK) >= 3;
  const ink = x => C.contrast(x, '#ffffff') >= 2.2 ? '#fff' : '#1d1d1f'; // the tick on a circle

  // ---- the colours in use ----
  let cache = null;
  const load = force => { if (!cache || force) cache = fetch('/api/colors').then(r => r.ok ? r.json() : []).catch(() => []); return cache; };
  addEventListener('rustical:change', () => { cache = null; });
  const THEME = { holiday: 'var(--c3)', school: 'var(--c4)' };
  const hexOf = s => toHex(s.color || THEME[s.kind] || '');
  const kindText = k => tr('js-color-k-' + k);
  const label = s => THEME[s.kind] ? kindText(s.kind) + (s.color ? '' : ' (' + tr('js-color-std') + ')') : tr('js-color-src', { kind: kindText(s.kind), name: s.name || '–' });
  // one entry per colour with its sources, in the server's order (collections first)
  const palette = list => { const m = new Map(); for (const s of list) { const x = hexOf(s); if (!x) continue; if (!m.has(x)) m.set(x, []); m.get(x).push(s); } return [...m].map(([hex, src]) => ({ hex, src })); };
  // equal labels once, with a count ("Termin „Urlaub“ 26×"); after two names only how many more
  const srcText = src => { const m = new Map(); src.forEach(x => { const l = label(x); m.set(l, (m.get(l) || 0) + 1); }); const u = [...m].map(([l, n]) => n > 1 ? l + ' ' + n + '×' : l);
    return u.length <= 2 ? u.join(' · ') : u[0] + ' · ' + u[1] + ' ' + tr('js-color-more', { n: u.length - 2 }); };
  const isSelf = (s, self) => self && (self.list ? self.list.includes(s) : s.kind === self.kind && (self.keys ? self.keys.includes(s.key) : s.key === self.key));

  // ---- the preview: the colour where the app shows it, on the light and the dark page at once ----
  const preview = title => {
    const el = document.createElement('div'); el.className = 'cpk-pv';
    const panel = dark => `<div class="cpk-pvp${dark ? ' dark' : ''}"><span class="sq"><svg class="ico" aria-hidden="true"><use href="#i-calendar"/></svg></span><span class="evs"><span class="t"><b>10:00</b> ${esc(title)}</span><span class="ad">${esc(tr('js-color-pv-allday'))}</span></span><span class="hd">3</span></div>`;
    el.innerHTML = panel(false) + panel(true) + '<div class="meta"></div>';
    return { el, set: x => { el.style.setProperty('--x', x); const r = C.contrast(x, LIGHT), d = C.contrast(x, DARK);
      $('.meta', el).innerHTML = `<code>${x}</code><span class="${r < 3 ? 'warn' : ''}">${esc(tr('js-color-on-light', { r: num.format(r) }))}</span><span class="${d < 3 ? 'warn' : ''}">${esc(tr('js-color-on-dark', { r: num.format(d) }))}</span>`; } };
  };

  // ---- drag on a track or area: fn(x, y) in 0..1 of its box ----
  const drag = (el, fn) => el.addEventListener('pointerdown', e => {
    e.preventDefault(); el.setPointerCapture(e.pointerId);
    const mv = ev => { const r = el.getBoundingClientRect(); fn(clamp((ev.clientX - r.left) / r.width), clamp((ev.clientY - r.top) / r.height)); };
    mv(e); el.onpointermove = mv; el.onpointerup = el.onpointercancel = () => { el.onpointermove = null; };
  });
  const check = (list, on) => list.forEach(b => b.setAttribute('aria-checked', b === on ? 'true' : 'false'));

  // ---- new colour, picker A: a fan deck of 14 colour families, five tones each ----
  const FAM = [['red', '#e5372b'], ['coral', '#ff6f59'], ['orange', '#ff9500'], ['mustard', '#d9a520'], ['lime', '#8cc63f'], ['green', '#30b46c'], ['teal', '#1e8a8a'], ['sky', '#32ade6'], ['blue', '#0a84ff'], ['indigo', '#5856d6'], ['violet', '#af52de'], ['berry', '#d3245c'], ['earth', '#a2845e'], ['slate', '#5f6b7a']];
  const TONES = [0.84, 0.74, 0.64, 0.54, 0.43]; // OKLCH lightness, light to dark
  let DECK = null;
  const deck = () => DECK || (DECK = FAM.map(([k, x]) => { const [, c, h] = C.lch(x); return [tr('js-color-fam-' + k), TONES.map(L => C.fromLch(L, c, h))]; }));
  function fanPicker(onPick) {
    const D = deck(); let act = 8;
    const el = document.createElement('div'); el.className = 'cpk-fanbox';
    el.innerHTML = `<div class="cpk-fan">${D.map(([nm, ts]) => `<div class="cpk-card">${ts.map((t, j) => `<button type="button" data-v="${t}" style="background:${t}" title="${esc(nm)} ${j + 1}" aria-checked="false"></button>`).join('')}<span class="nm">${esc(nm)}</span></div>`).join('')}</div>
      <div class="cpk-nav"><button class="btn icon-btn" type="button" data-d="-1" aria-label="${esc(tr('js-color-prev'))}">‹</button><span></span><button class="btn icon-btn" type="button" data-d="1" aria-label="${esc(tr('js-color-next'))}">›</button></div>`;
    const fan = $('.cpk-fan', el), cards = $$('.cpk-card', fan), lab = $('.cpk-nav span', el);
    const lay = () => cards.forEach((c, i) => { const d = i - act; c.style.transform = `rotate(${d * 11}deg) translateY(${Math.abs(d) * 2}px)`; c.style.zIndex = 50 - Math.abs(d); c.style.opacity = Math.abs(d) > 5 ? .25 : 1; });
    const go = d => { act = clamp(act + d, 0, D.length - 1); lay(); };
    fan.addEventListener('click', e => { const b = e.target.closest('button'); if (!b) return; const i = cards.indexOf(b.closest('.cpk-card')); if (i !== act) { act = i; lay(); } onPick(b.dataset.v); });
    fan.addEventListener('wheel', e => { e.preventDefault(); go(Math.sign(e.deltaY || e.deltaX)); }, { passive: false });
    let sx = null; fan.addEventListener('pointerdown', e => { sx = e.clientX; });
    fan.addEventListener('pointermove', e => { if (sx == null || e.buttons !== 1) return; const d = Math.round((sx - e.clientX) / 28); if (d) { go(d); sx = e.clientX; } });
    fan.addEventListener('pointerup', () => { sx = null; }); fan.addEventListener('pointerleave', () => { sx = null; });
    $$('.cpk-nav button', el).forEach(b => b.onclick = () => go(+b.dataset.d));
    // a colour from outside: its tone if the deck has it, else the card with the nearest tone
    const set = x => { let bi = 0, bj = 0, bd = 9; D.forEach(([, ts], i) => ts.forEach((t, j) => { const d = C.dist(t, x); if (d < bd) { bd = d; bi = i; bj = j; } }));
      act = bi; lay(); const exact = bd < 0.005; check($$('button', fan), exact ? $$('button', cards[bi])[bj] : null);
      lab.textContent = tr(exact ? 'js-color-fan-tone' : 'js-color-fan-near', { fam: D[bi][0], n: bj + 1 }); };
    return { el, set };
  }

  // ---- new colour, picker B: a text field (any CSS colour), a dice, three sliders — one colour ----
  const MODES = {
    simple: { get: x => C.hsl(C.rgb(x)).map((n, i) => i ? n * 100 : n), to: ([a, b, c]) => C.hex(C.fromHsl(a, b / 100, c / 100)), rows: [['hue', 0, 359, '°'], ['sat', 0, 100, ''], ['light', 0, 100, '']] },
    rgb: { get: x => C.rgb(x).map(n => n * 255), to: v => C.hex(v.map(n => n / 255)), rows: [['red', 0, 255, ''], ['green', 0, 255, ''], ['blue', 0, 255, '']] },
    oklch: { get: x => { const [L, c, h] = C.lch(x); return [L * 100, c * 250, h]; }, to: ([L, c, h]) => C.fromLch(L / 100, c / 250, h), rows: [['light', 0, 100, ''], ['chroma', 0, 100, ''], ['hue', 0, 359, '°']] },
  };
  const SUGG = ['#b60205', '#d93f0b', '#fbca04', '#0e8a16', '#006b75', '#1d76db', '#0052cc', '#5319e7', '#e99695', '#f9d0c4', '#fef2c0', '#c2e0c6', '#bfdadc', '#c5def5', '#bfd4f2', '#d4c5f9'];
  function fieldPicker(onPick) {
    let hex = '#000000', mode = MODES[localStorage.cpkMode] ? localStorage.cpkMode : 'simple', vals = [0, 0, 0], rows = [];
    const el = document.createElement('div'); el.className = 'cpk-field';
    el.innerHTML = `<div class="cpk-in"><span class="cpk-chip"></span><input type="text" spellcheck="false" autocomplete="off" aria-label="${esc(tr('js-color-text'))}"><button class="btn icon-btn" type="button" title="${esc(tr('js-color-dice'))}" aria-label="${esc(tr('js-color-dice'))}"><svg class="ico" aria-hidden="true"><use href="#i-dice"/></svg></button></div><div class="cpk-err" hidden></div>
      <span class="seg">${Object.keys(MODES).map(m => `<button type="button" data-m="${m}" aria-pressed="${m === mode}">${esc(tr('js-color-m-' + m))}</button>`).join('')}</span><div class="cpk-sl"></div>
      <div class="cpk-cap">${esc(tr('js-color-sugg'))}</div><div class="cpk-sugg">${SUGG.map(x => `<button type="button" data-v="${x}" style="background:${x}" title="${x}" aria-checked="false"></button>`).join('')}</div>`;
    const box = $('.cpk-in', el), chip = $('.cpk-chip', el), inp = $('input', el), err = $('.cpk-err', el), sl = $('.cpk-sl', el), sugg = $$('.cpk-sugg button', el);
    // `from` keeps the source of a change as it is: the field what is being typed, a slider its exact value
    const paint = from => { const M = MODES[mode]; if (from !== 'slider') vals = M.get(hex); chip.style.background = hex;
      if (from !== 'text') { inp.value = hex; box.classList.remove('bad'); err.hidden = true; }
      rows.forEach((r, i) => { const stops = [0, .17, .33, .5, .67, .83, 1].map(t => { const w = vals.slice(); w[i] = r.lo + t * (r.hi - r.lo); return M.to(w); });
        r.tr.style.background = `linear-gradient(to right, ${stops.join(',')})`; r.kn.style.left = (vals[i] - r.lo) / (r.hi - r.lo) * 100 + '%'; r.kn.style.background = hex; r.out.textContent = Math.round(vals[i]) + r.u; });
      check(sugg, sugg.find(b => b.dataset.v === hex)); };
    const build = () => { const M = MODES[mode];
      sl.innerHTML = M.rows.map(([k]) => `<div class="row"><span>${esc(tr('js-color-' + k))}</span><div class="track"><i class="knob"></i></div><output></output></div>`).join('');
      rows = $$('.row', sl).map((row, i) => { const [, lo, hi, u] = M.rows[i], tr_ = $('.track', row); drag(tr_, x => { vals[i] = lo + x * (hi - lo); hex = M.to(vals); paint('slider'); onPick(hex); }); return { tr: tr_, kn: $('.knob', row), out: $('output', row), lo, hi, u }; });
      paint(); };
    $$('.seg button', el).forEach(b => b.onclick = () => { mode = localStorage.cpkMode = b.dataset.m; segPress(b); build(); });
    inp.oninput = () => { const x = toHex(inp.value); if (x) { hex = x; paint('text'); onPick(hex); } else { box.classList.add('bad'); err.hidden = !inp.value; err.textContent = tr('js-color-bad'); } };
    inp.onblur = () => { if (!box.classList.contains('bad')) inp.value = hex; };
    $('.cpk-in .btn', el).onclick = () => { hex = C.fromLch(0.55 + Math.random() * 0.2, 0.13 + Math.random() * 0.07, Math.random() * 360); paint(); onPick(hex); };
    sugg.forEach(b => b.onclick = () => { hex = b.dataset.v; paint(); onPick(hex); });
    build();
    return { el, set: x => { hex = x; paint(); } };
  }

  // ---- the dialog for a new colour ----
  function openNew(o, pal, done) {
    const others = pal.filter(p => !p.src.every(s => isSelf(s, o.self)));
    // start on the deck tone that stands out most from the colours in use and is seen on light and dark
    let hex = deck().flatMap(d => d[1]).filter(visible).map(t => [Math.min(9, ...others.map(p => C.dist(t, p.hex))), t]).sort((a, b) => b[0] - a[0])[0][1];
    let tab = localStorage.cpkTab === 'field' ? 'field' : 'fan';
    const w = document.createElement('div'); w.className = 'ask cpk-ask'; w.setAttribute('role', 'dialog'); w.setAttribute('aria-modal', 'true'); w.setAttribute('aria-label', tr('js-color-new'));
    w.innerHTML = `<div class="ask-box cpk-dlg"><div class="cpk-dh"><span><b>${esc(tr('js-color-new'))}</b><small>${esc(tr('js-color-for', { what: o.what || o.title }))}</small></span>
      <span class="seg"><button type="button" data-t="fan"><svg class="ico" aria-hidden="true"><use href="#i-palette"/></svg><span>${esc(tr('js-color-tab-fan'))}</span></button><button type="button" data-t="field"><svg class="ico" aria-hidden="true"><use href="#i-eyedropper"/></svg><span>${esc(tr('js-color-tab-field'))}</span></button></span>
      <button class="btn icon-btn" type="button" data-x aria-label="${esc(tr('js-color-close'))}"><svg class="ico" aria-hidden="true"><use href="#i-x"/></svg></button></div>
      <div class="cpk-body"></div><div class="cpk-near"></div><div class="ask-acts"><button class="btn icon-btn" type="button" data-x title="${esc(tr('js-ct-cancel'))}" aria-label="${esc(tr('js-ct-cancel'))}"><svg class="ico" aria-hidden="true"><use href="#i-x"/></svg></button><button class="btn btn-primary icon-btn" type="button" data-ok title="${esc(tr('js-color-apply'))}" aria-label="${esc(tr('js-color-apply'))}"><svg class="ico" aria-hidden="true"><use href="#i-check"/></svg></button></div></div>`;
    const pv = preview(o.title); $('.cpk-near', w).after(pv.el);
    const body = $('.cpk-body', w), near = $('.cpk-near', w);
    const showNear = () => {
      const rows = others.map(p => ({ p, d: C.dist(hex, p.hex) })).sort((a, b) => a.d - b.d), same = rows.find(r => r.d < 0.03);
      const shown = rows.filter((r, i) => r.d < 0.15 || i < 6), rest = rows.length - shown.length;
      near.innerHTML = `<div class="cpk-cap">${esc(tr('js-color-dist'))}</div>`
        + (same ? `<div class="same"><i style="background:${same.p.hex}"></i><span>${esc(tr('js-color-same', { src: srcText(same.p.src) }))}</span><button class="link" type="button" data-take="${same.p.hex}">${esc(tr('js-color-take'))}</button></div>` : '')
        + `<div class="vrd">${shown.map(({ p, d }) => `<span class="${d < 0.09 ? 'bad' : d < 0.15 ? 'meh' : ''}"><i style="background:${p.hex}"></i>${esc(srcText(p.src))}${d < 0.09 ? ' – ' + esc(tr('js-color-near')) : d < 0.15 ? ' – ' + esc(tr('js-color-similar')) : ' ✓'}</span>`).join('')}${rest > 0 ? `<span class="muted">${esc(tr('js-color-more', { n: rest }))} ✓</span>` : ''}</div>`;
    };
    const pick = x => { hex = x; pv.set(x); showNear(); };
    const A = fanPicker(x => { pick(x); A.set(x); }), B = fieldPicker(pick);
    const show = () => { body.replaceChildren(tab === 'fan' ? A.el : B.el); (tab === 'fan' ? A : B).set(hex); $$('.cpk-dh .seg button', w).forEach(b => b.setAttribute('aria-pressed', b.dataset.t === tab ? 'true' : 'false')); };
    $$('.cpk-dh .seg button', w).forEach(b => b.onclick = () => { tab = localStorage.cpkTab = b.dataset.t; show(); });
    const prev = document.activeElement;
    const end = v => { removeEventListener('keydown', key, true); w.remove(); if (prev && prev.focus) prev.focus(); if (v) done(v); };
    const key = e => { if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); end(null); } else if (e.key === 'Enter' && e.target.tagName === 'INPUT') { e.preventDefault(); end(hex); } };
    addEventListener('keydown', key, true);
    // modal: only ×, Cancel and Escape cancel, the backdrop does not
    w.addEventListener('click', e => { if (e.target.closest('[data-x]')) end(null); else if (e.target.closest('[data-ok]')) end(hex); else if (e.target.closest('[data-take]')) end(e.target.closest('[data-take]').dataset.take); });
    document.body.appendChild(w); show(); pick(hex); $('[data-ok]', w).focus();
  }

  // ---- the popover: the colours in use ----
  let pop = null, lastRect = null;
  const close = () => { if (!pop) return; pop.remove(); pop = null; removeEventListener('pointerdown', outside, true); removeEventListener('keydown', key, true); };
  const outside = e => { if (pop && !pop.contains(e.target) && !e.target.closest('.cpk-ask')) close(); };
  const key = e => {
    if ($('.cpk-ask')) return;
    if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); close(); return; }
    const b = e.target.closest && e.target.closest('.cpk-sw'); if (!b || !pop.contains(b)) return;
    const all = $$('.cpk-sw', pop), i = all.indexOf(b), n = { ArrowRight: 1, ArrowLeft: -1, ArrowDown: 8, ArrowUp: -8 }[e.key];
    if (n) { e.preventDefault(); all[clamp(i + n, 0, all.length - 1)].focus(); }
  };
  /* o: { title (the element's name), what (for the dialog: "Termin „Zahnarzt“"), sub (its kind),
     value ('' = the fallback), inherit ({ hex, text } or null), self ({ kind, key } of the element),
     onPick(value), once (true: a new colour does not bring the popover back) } */
  async function open(anchor, o) {
    close();
    if (!o.title) { o.title = o.sub || ''; o.sub = ''; } if (o.sub === o.title) o.sub = ''; // a new task has no name yet; a holiday row is its kind
    const list = await load(), pal = palette(list), cur = () => o.value ? toHex(o.value) : '';
    const extra = cur() && !pal.some(p => p.hex === cur()) ? cur() : ''; // a colour nothing else has yet (a form not yet saved)
    pop = document.createElement('div'); pop.className = 'cpk'; pop.setAttribute('role', 'dialog'); pop.setAttribute('aria-label', o.title);
    const circle = (v, hex, text, cls = '') => `<button type="button" class="cpk-sw${cls}" data-v="${v}" style="--c:${hex};--ink:${ink(hex)}" title="${esc(text)}" aria-label="${esc(text)}" aria-checked="false"></button>`;
    pop.innerHTML = `<div class="cpk-h"><span class="sq"><svg class="ico" aria-hidden="true"><use href="#i-calendar"/></svg></span><span><b>${esc(o.title)}</b><small>${esc(o.sub || '')}</small></span></div>
      <div class="cpk-sws" role="radiogroup" aria-label="${esc(o.title)}">${o.inherit ? circle('', o.inherit.hex, o.inherit.text, ' inh') : ''}${pal.map(p => circle(p.hex, p.hex, srcText(p.src))).join('')}${extra ? circle(extra, extra, tr('js-color-current')) : ''}<button type="button" class="cpk-sw cpk-add" data-new title="${esc(tr('js-color-add'))}" aria-label="${esc(tr('js-color-add'))}"><svg class="ico" aria-hidden="true"><use href="#i-plus"/></svg></button></div>
      <div class="cpk-name"></div>`;
    const pv = preview(o.title); pop.appendChild(pv.el);
    const name = $('.cpk-name', pop), sq = $('.cpk-h .sq', pop), sws = $$('.cpk-sw:not(.cpk-add)', pop);
    const shownHex = () => cur() || (o.inherit ? o.inherit.hex : '#8e8e93');
    let shown = '';
    const mark = () => { const on = sws.find(b => b.dataset.v === cur()); check(sws, on); shown = on ? on.title : ''; name.textContent = shown; sq.style.background = shownHex(); pv.set(shownHex()); };
    pop.addEventListener('pointerover', e => { const b = e.target.closest('.cpk-sw'); if (!b) return; name.textContent = b.title; if (!b.hasAttribute('data-new')) pv.set(b.dataset.v || o.inherit.hex); });
    pop.addEventListener('pointerout', e => { if (e.target.closest('.cpk-sw') && !(e.relatedTarget && e.relatedTarget.closest && e.relatedTarget.closest('.cpk-sw'))) { name.textContent = shown; pv.set(shownHex()); } });
    pop.addEventListener('click', e => {
      const b = e.target.closest('.cpk-sw'); if (!b) return;
      // ＋: the popover comes back after the save and any question the save asks
      if (b.hasAttribute('data-new')) { openNew(o, pal, async v => { o.value = v; cache = null; await o.onPick(v); if (!o.once) open(anchor, o); }); return; }
      if (b.dataset.v === cur()) return;
      o.value = b.dataset.v; cache = null; mark(); o.onPick(o.value);
    });
    document.body.appendChild(pop); mark();
    // below the anchor, else above it; always inside the window. A list redrawn meanwhile (settings "Farben")
    // detached the anchor: the popover stays where it was
    const r = anchor.isConnected || !lastRect ? anchor.getBoundingClientRect() : lastRect, pw = pop.offsetWidth, ph = pop.offsetHeight; lastRect = r;
    const top = r.bottom + 6 + ph <= innerHeight ? r.bottom + 6 : Math.max(8, r.top - ph - 6);
    pop.style.left = clamp(r.right - pw, 8, innerWidth - pw - 8) + 'px'; pop.style.top = top + 'px';
    addEventListener('pointerdown', outside, true); addEventListener('keydown', key, true);
    (sws.find(b => b.getAttribute('aria-checked') === 'true') || $('.cpk-sw', pop)).focus();
  }

  window.colorPicker = { open, close, load, invalidate: () => { cache = null; }, toHex, palette, label, srcText, hexOf };

  /* ---- a colour field in a form: <input type=hidden name=…> + <button class="cpk-btn" data-cpk-…>.
     A pick writes the field and fires `change` (the settings' autosave). data-cpk-kind/-key name the
     element (its own colour then counts as its), data-cpk-std a fallback ("standard"; the field's "" ) ---- */
  const paintBtn = (b, v) => { const x = toHex(v) || toHex(b.dataset.cpkStd || ''); b.style.setProperty('--c', x); b.querySelector('span').textContent = v ? '' : tr('js-color-std'); };
  $$('button.cpk-btn[data-cpk-for]').forEach(b => {
    const field = b.form ? b.form.elements[b.dataset.cpkFor] : document.getElementsByName(b.dataset.cpkFor)[0];
    if (!field) return;
    paintBtn(b, field.value);
    b.addEventListener('click', e => {
      e.preventDefault();
      const std = b.dataset.cpkStd ? { hex: toHex(b.dataset.cpkStd), text: tr('js-color-std') } : null;
      open(b, { title: b.dataset.cpkTitle || '', sub: b.dataset.cpkSub || '', what: b.dataset.cpkWhat, value: field.value, inherit: std, self: b.dataset.cpkKind ? { kind: b.dataset.cpkKind, key: b.dataset.cpkKey || '' } : null,
        onPick: v => { field.value = v; paintBtn(b, v); themeVar(b.dataset.cpkVar, v); field.dispatchEvent(new Event('change', { bubbles: true })); } });
    });
  });
  // holiday colours show at once: --hol/--fer on <html>; "" drops the user's value, also from the page's own
  // <style> (render.rs holiday_css), so the theme's colour shows again
  function themeVar(prop, v) {
    if (!prop) return; const root = document.documentElement;
    if (v) root.style.setProperty(prop, v); else { root.style.removeProperty(prop); $$('style').forEach(s => { s.textContent = s.textContent.replace(new RegExp(prop + ':#[0-9a-f]{6};', 'g'), ''); }); }
  }

  /* ---- settings page "Farben" (#colors): every element with a colour, grouped; a row opens the picker and
     saves at once (PUT /api/colors/…); holiday colours go through their prefs fields, a category's into its rule
     (GET + PUT /api/category-rules, the setting keeps its mode). At the end what the picker offers. ---- */
  const page = $('#colors'), list = page && $('#colors-list', page);
  if (!page) return;
  const putRule = async (scope, name, color) => {
    const q = '?scope=' + scope, rules = await (await fetch('/api/category-rules' + q)).json(), rule = rules[name];
    if (!rule || !rule.when || !rule.when.color) return { ok: false, status: 404 };
    rule.when.color.value = color;
    return fetch('/api/category-rules/' + encodeURIComponent(name) + q, { method: 'PUT', headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf() }, body: JSON.stringify(rule) });
  };
  // a category's new colour: existing events/tasks with the category that still carry the old one may follow
  const recolor = async (scope, name, from, to) => {
    const post = apply => fetch('/api/colors/category', { method: 'POST', headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf() }, body: JSON.stringify({ scope, name, from, to, apply }) }).then(r => r.ok ? r.json() : null).catch(() => null);
    const c = await post(false); if (!c || !c.count) return;
    close(); // the question needs the page, not the popover over it
    if (!await askYesNo(tr('js-color-recolor-' + scope, { n: c.count, cat: name }), tr('js-color-recolor-yes'), tr('js-color-recolor-no'), true)) return;
    const d = await post(true); if (d) toast(tr('js-color-recolored', { n: d.count })); else toast(tr('js-set-err'), true);
  };
  const put = (kind, key, color) => fetch('/api/colors/' + kind + '/' + key.split('/').map(encodeURIComponent).join('/'), { method: 'PUT', headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf() }, body: JSON.stringify({ color }) });
  const ICON = { calendar: 'i-calendar', addressbook: 'i-contacts', holiday: 'i-party', school: 'i-grad', category: 'i-tag', 'category-task': 'i-tag', event: 'i-calendar', task: 'i-tasks' };
  // two blocks whose colours are seen together (calendar view, task list), each with "Farben optimieren"; address books apart
  const BLOCKS = [['event', ['calendar', 'holiday', 'category', 'event']], ['task', ['category-task', 'task']], ['', ['addressbook']]];
  const VAR = { holiday: ['holiday_color', '--hol'], school: ['school_color', '--fer'] };
  const field = n => $(`#prefs-form input[type=hidden][name="${n}"]`);
  let rows = [];
  // the same colour elsewhere on the settings page: the collection lists and the collection's own page
  const syncPage = (kind, key, v) => {
    const b = $(`button.cpk-btn[data-cpk-kind="${kind}"][data-cpk-key="${CSS.escape(key)}"]`);
    if (b) { const f = b.form.elements[b.dataset.cpkFor]; f.value = v; paintBtn(b, v); }
    $$(`[data-cpk-swatch="${CSS.escape(kind + ':' + key)}"]`).forEach(s => { s.style.background = v; });
  };
  // identical rows (kind, colour, name and origin alike: 26 × "Urlaub" in one calendar) are one row with a count
  const merge = src => { const m = new Map(); for (const s of src) { const hex = hexOf(s), k = [s.kind, hex, s.name, s.cal || '', s.editable].join('\u0001'); if (!m.has(k)) m.set(k, { kind: s.kind, hex, name: s.name, cal: s.cal, cal_color: s.cal_color, color: s.color, editable: s.editable, key: s.key, keys: [] }); m.get(k).keys.push(s.key); } return [...m.values()]; };
  // colour order: hue round the circle (OKLCH), lighter first; greys after the colours
  const hueKey = hex => { const [L, c, h] = C.lch(hex); return c < 0.03 ? [1, 0, -L] : [0, h, -L]; };
  const coll = new Intl.Collator(window.LOC, { sensitivity: 'base', numeric: true });
  const byHue = (a, b) => { const x = hueKey(a), y = hueKey(b); for (let i = 0; i < 3; i++) if (Math.abs(x[i] - y[i]) > 1e-9) return x[i] - y[i]; return 0; };
  // rows of a group: origin, name, count (most first), colour
  const order = (a, b) => coll.compare(a.cal || '', b.cal || '') || coll.compare(a.name || '', b.name || '') || b.keys.length - a.keys.length || byHue(a.hex, b.hex);
  const draw = async force => {
    const src = await load(force), pal = palette(src);
    rows = merge(src);
    // what the picker offers: most used first, then name, colour; a row recolours everything of its colour
    offers = pal.map(p => ({ ...p, text: srcText(p.src) })).sort((a, b) => b.src.length - a.src.length || coll.compare(a.text, b.text) || byHue(a.hex, b.hex));
    const row = (s, i) => {
      const sw = `<span class="swatch" style="background:${s.hex}"><svg class="ico" aria-hidden="true"><use href="#${ICON[s.kind]}"/></svg></span>`;
      const name = VAR[s.kind] ? kindText(s.kind) : s.name || '–';
      const sub = VAR[s.kind] ? (s.color ? tr('js-color-own') : tr('js-color-std')) : s.cal ? tr('js-color-src', { kind: kindText('calendar'), name: s.cal }) + (s.editable ? '' : ' · ' + tr('js-color-ro')) : s.kind.startsWith('category') ? tr('js-color-in-rule') : '';
      const n = s.keys.length > 1 ? `<span class="badge" title="${esc(tr('js-color-count', { n: s.keys.length }))}">${s.keys.length}×</span>` : '';
      const body = `${sw}<span>${esc(name)}<span class="sub">${esc(sub)}</span></span><span class="val">${n}<code>${s.hex}</code><span class="chev">›</span></span>`;
      if (!s.editable) return `<div class="cell">${body.replace('<span class="chev">›</span>', '')}</div>`;
      return `<button type="button" class="cell link" data-i="${i}">${body}</button>`;
    };
    const group = k => { const idx = rows.map((s, i) => [s, i]).filter(([s]) => s.kind === k || (k === 'holiday' && s.kind === 'school'));
      if (k !== 'holiday') idx.sort((x, y) => order(x[0], y[0]));
      return idx.length ? `<div class="label">${esc(tr('js-color-g-' + k))}</div><div class="group">${idx.map(([s, i]) => row(s, i)).join('')}</div>` : ''; };
    list.innerHTML = BLOCKS.map(([view, kinds]) => { const inner = kinds.map(group).join('');
        return inner && view ? `<div class="cfo-block"><h3>${esc(tr('js-cfo-block-' + view))}</h3><button type="button" class="cfo-opt" data-opt="${view}">✦ ${esc(tr('js-cfo-btn'))}</button></div>${inner}` : inner; }).join('')
      + `<div class="label">${esc(tr('js-color-g-offer', { n: pal.length }))}</div><div class="group">${offers.map((p, i) => { const body = `<span class="swatch" style="background:${p.hex}"></span><span>${esc(p.text)}</span><span class="val"><code>${p.hex}</code>`;
        return p.src.some(x => x.editable) ? `<button type="button" class="cell link" data-o="${i}">${body}<span class="chev">›</span></span></button>` : `<div class="cell">${body}</span></div>`; }).join('')}</div>`;
    const v = $('#v-colors'); if (v) v.textContent = pal.length;
  };
  let offers = [];
  // one element's colour: collections, events and tasks through /api/colors, a category into its rule
  // (the setting keeps its mode), a holiday colour into its prefs field (autosave)
  const setOne = async (kind, key, v) => {
    const vr = VAR[kind];
    if (vr) { const f = field(vr[0]); f.value = v; themeVar(vr[1], v); f.dispatchEvent(new Event('change', { bubbles: true })); return true; }
    const r = await (kind.startsWith('category') ? putRule(kind === 'category' ? 'event' : 'task', key, v) : put(kind, key, v)).catch(() => null);
    if (!r || !r.ok) { toast(tr('js-set-err') + (r ? ' ' + r.status : ''), true); return false; }
    if (kind === 'calendar' || kind === 'addressbook') syncPage(kind, key, v);
    return true;
  };
  list.addEventListener('click', e => {
    const v = e.target.closest('button[data-opt]'); if (v) return optimise(v.dataset.opt);
    const o = e.target.closest('button[data-o]');
    if (o) {
      // a colour of the offer: every element that carries it gets the new one (read-only ones stay)
      const p = offers[+o.dataset.o], els = p.src.filter(x => x.editable);
      open(o, { title: p.text, sub: tr('js-color-count', { n: els.length }), what: p.text, value: p.hex, inherit: null, self: { list: p.src }, onPick: async v => {
        let n = 0; for (const x of els) { if (!await setOne(x.kind, x.key, v)) break; n++; }
        if (n === els.length) toast(tr('js-color-changed', { n })); draw(true);
      } });
      return;
    }
    const b = e.target.closest('button[data-i]'); if (!b) return;
    const s = rows[+b.dataset.i], vr = VAR[s.kind];
    const inherit = s.cal ? { hex: toHex(s.cal_color), text: tr('js-color-inherit-cal', { cal: s.cal }) } : vr ? { hex: toHex(THEME[s.kind]), text: tr('js-color-std') } : null;
    const title = vr ? kindText(s.kind) : s.name || '–';
    open(b, { title, sub: kindText(s.kind) + (s.keys.length > 1 ? ' · ' + tr('js-color-count', { n: s.keys.length }) : ''), what: vr ? title : label(s), value: s.color, inherit, self: { kind: s.kind, keys: s.keys }, onPick: async v => {
      if (vr) await setOne(s.kind, s.key, v);
      else {
        // a merged row changes all its elements
        for (const key of s.keys) if (!await setOne(s.kind, key, v)) { draw(true); return; }
        toast(settingsSaved());
        if (s.kind.startsWith('category') && s.color && s.color !== v) await recolor(s.kind === 'category' ? 'event' : 'task', s.key, s.color, v);
      }
      s.color = v; draw(true);
    } });
  });

  /* ---- "Farben optimieren" (round 14): one dialog per view whose colours meet — the calendar (calendars,
     events, their categories, holidays) and the task list (calendars, tasks, their categories). Elements
     that belong together (same title, word stem, abbreviation, category) form a family and get one colour;
     families whose colours look alike are pulled apart. A flow chart shows element → action → colour after;
     the user picks colours on the right, leaves elements out and drags them between actions. The plan is
     recomputed from scratch on every change, with the user's choices pinned. POST /api/colors/batch. ---- */
  const VIEW = { event: ['calendar', 'event', 'category', 'holiday', 'school'], task: ['calendar', 'task', 'category-task'] };
  const SHOWN_T = 0.09; // "too similar" in the numbers on top, whatever the level separates at
  // the one slider: every level adds to the one before; T = pull apart below this OKLab distance
  const S3 = { stem: true, abbr: true }, S5 = { stem: true, abbr: true, series: true };
  const LEVELS = [{ title: {}, T: 0 }, { title: S3, T: 0.04 }, { title: S3, anchor: true, T: 0.06 }, { title: S3, anchor: true, T: 0.09, rec: true }, { title: S5, anchor: true, T: 0.12 }, { title: S5, anchor: true, T: 0.09, repaint: true }];
  // who leads a family: holidays first (their default colours are kept), then a category, then a calendar
  const ANCH = { holiday: 4, school: 4, category: 3, 'category-task': 3, calendar: 1 };
  const num2 = new Intl.NumberFormat(window.LOC, { minimumFractionDigits: 2, maximumFractionDigits: 2 }), dec = x => num2.format(x);

  // titles: "Urlaub" ~ "Urlaubstag" (stem), "RB" ~ "Rufbereitschaft" (abbreviation); "Regel-Demo: x" is a series
  const STOP = new Set('der die das den dem des ein eine und oder mit für fuer im in am an zu zum zur von vom bei auf aus nach the of and a le la les de du et el los y'.split(' '));
  const split = t => (t || '').split(/[\s:;,.()[\]/!?"„“”']+/).map(w => w.replace(/^-+|-+$/g, '')).filter(Boolean);
  const words = t => split(t).map(w => w.toLowerCase()).filter(w => w.length >= 2 && !STOP.has(w));
  const series = t => { const m = /^([^:]{3,}):\s*\S/.exec(t || ''); return m ? m[1].trim().toLowerCase() : null; };
  const body = t => (series(t) ? t.slice(t.indexOf(':') + 1) : t);
  const stem = (x, y) => x === y || (Math.min(x.length, y.length) >= 5 && (x.startsWith(y) || y.startsWith(x)));
  const abbr = (tok, w) => { if (!tok || !w || !/^[A-ZÄÖÜ]{2,4}$/.test(tok) || w.length < 6) return false; const t = tok.toLowerCase(); if (w[0] !== t[0]) return false; let i = 1; for (const ch of t.slice(1)) { i = w.indexOf(ch, i) + 1; if (!i) return false; } return true; };
  const alike = (a, b, o) => {
    const ba = words(body(a)), bb = words(body(b));
    // the leading word carries the meaning; a one-word title may sit anywhere in the other
    if (o.stem && ba.length && bb.length && (stem(ba[0], bb[0]) || (ba.length === 1 && bb.some(w => stem(w, ba[0]))) || (bb.length === 1 && ba.some(w => stem(w, bb[0]))))) return true;
    if (o.abbr && (abbr(split(body(a))[0], bb[0]) || abbr(split(body(b))[0], ba[0]))) return true;
    return !!(o.series && series(a) && series(a) === series(b));
  };

  // new colours come from professional palettes, in tiers: Apple's system colours (the app's own look,
  // developer.apple.com HIG "Color"), then Google Calendar's event and calendar colours, then an even OKLCH
  // ring; browns last, they read as dull. A tier is used until nothing in it keeps the distance T to what is
  // taken; within a tier the farthest colour wins.
  const TIERS = [
    ['#ff3b30', '#ff9500', '#ffcc00', '#34c759', '#00c7be', '#30b0c7', '#32ade6', '#007aff', '#5856d6', '#af52de', '#ff2d55'],
    ['#d50000', '#e67c73', '#f4511e', '#f6bf26', '#33b679', '#0b8043', '#039be5', '#3f51b5', '#7986cb', '#8e24aa', '#4285f4', '#ef6c00', '#ad1457', '#f09300', '#009688', '#7cb342', '#c0ca33', '#e4c441'],
  ];
  let RING = null;
  const ring = () => RING || (RING = [0.7, 0.6].flatMap(L => [15, 40, 65, 90, 115, 140, 165, 190, 215, 240, 265, 290, 315, 340].map(h => C.fromLch(L, Math.min(0.19, C.maxC(L, h) * 0.95), h))));
  const BROWN = ['#a2845e', '#795548'];
  const dist = (a, b) => (a === b ? 0 : C.dist(a, b));
  const room = (x, taken) => Math.min(1, ...taken.map(a => dist(a, x)));
  const pickNew = (taken, T) => {
    let best = null;
    for (const tier of [...TIERS, ring(), BROWN]) {
      for (const x of tier) { if (taken.includes(x)) continue; const m = room(x, taken); if (!best || m > best.m) best = { x, m }; }
      if (best && best.m >= T) break;
    }
    return best ? best.x : '#8e8e93';
  };
  // a whole new palette for `n` families: farthest-point picks around what stays, then each family (heaviest
  // first) takes the picked colour nearest to the one it has, so it keeps its character where it can
  const palette_ = (fams, taken, T) => {
    const set = [], all = [...taken];
    for (let i = 0; i < fams.length; i++) { const x = pickNew(all, Math.min(T, SHOWN_T)); set.push(x); all.push(x); }
    const out = new Map();
    for (const F of fams) { let bi = 0; set.forEach((x, i) => { if (dist(x, F.to) < dist(set[bi], F.to)) bi = i; }); out.set(F, set.splice(bi, 1)[0]); }
    return out;
  };

  // families: categories first (an event joins the first coloured category it carries), then equal titles,
  // then alike titles; two different categories or calendars never become one family
  function families(its, L) {
    const par = new Map(its.map(x => [x.i, x.i])), find = k => { while (par.get(k) !== k) k = par.get(k); return k; };
    const anc = new Map(its.map(x => [x.i, ANCH[x.kind] ? [x] : []]));
    const join = (x, y) => { const a = find(x.i), b = find(y.i); if (a === b || anc.get(a).some(p => anc.get(b).some(q => p.kind === q.kind && p.name !== q.name))) return; par.set(b, a); anc.set(a, [...anc.get(a), ...anc.get(b)]); };
    for (const x of its) if (x.cats.length) { const c = its.find(k => k.kind === (x.kind === 'task' ? 'category-task' : 'category') && x.cats.includes(k.name)); if (c) join(c, x); }
    const by = new Map(); for (const x of its) { const k = words(x.name).join(' '); if (!k) continue; if (!by.has(k)) by.set(k, []); by.get(k).push(x); }
    for (const xs of by.values()) xs.slice(1).forEach(x => join(xs[0], x));
    const reps = [...by.values()].map(xs => xs[0]);
    if (L.title.stem || L.title.abbr || L.title.series) for (let i = 0; i < reps.length; i++) for (let j = i + 1; j < reps.length; j++) if (alike(reps[i].name, reps[j].name, L.title)) join(reps[i], reps[j]);
    const m = new Map(); for (const x of its) { const r = find(x.i); if (!m.has(r)) m.set(r, []); m.get(r).push(x); }
    return m;
  }

  function plan(src, view, st) {
    const L = LEVELS[st.level];
    const its = src.map((s, i) => ({ s, i, kind: s.kind, key: s.key, hex: hexOf(s), raw: s.color || '', std: !s.color && !!THEME[s.kind], name: THEME[s.kind] ? kindText(s.kind) : s.name || '–', cats: s.cats || [], cal: s.cal, editable: s.editable !== false }))
      .filter(x => x.hex && VIEW[view].includes(x.kind));
    const fams = new Map(), G = new Map();
    for (const [root, members] of families(its, L)) {
      const F = { key: String(root), groups: [] }, m = new Map();
      for (const x of members) {
        const k = [x.name, x.kind, x.hex, root].join('\u0001');
        if (!m.has(k)) { const g = { key: k, home: F.key, name: x.std ? kindText(x.kind) + ' (' + tr('js-color-std') + ')' : x.name, kind: x.kind, hex: x.hex, raw: x.raw, items: [], editable: true }; m.set(k, g); G.set(k, g); }
        const g = m.get(k); g.items.push(x); if (!x.editable) g.editable = false;
      }
      F.groups = [...m.values()]; fams.set(F.key, F);
    }
    // what the user dragged: into another action, or out into one of its own
    for (const [gk, to] of st.moves) {
      const g = G.get(gk); if (!g) continue;
      for (const F of fams.values()) F.groups = F.groups.filter(x => x !== g);
      if (to === 'split') fams.set('x:' + gk, { key: 'x:' + gk, groups: [g], split: true });
      else (fams.get(to) || fams.get(g.home)).groups.push(g);
      g.moved = to;
    }
    const list = [...fams.values()].filter(F => F.groups.length);
    for (const F of list) {
      F.inc = F.groups.filter(g => !st.excl.has(g.key));
      const xs = F.inc.flatMap(g => g.items);
      F.n = xs.length;
      F.lead = xs.filter(x => ANCH[x.kind]).sort((a, b) => ANCH[b.kind] - ANCH[a.kind])[0] || null;
      const nm = new Map(); for (const g of F.groups) nm.set(g.name, (nm.get(g.name) || 0) + g.items.length);
      F.head = F.lead ? F.lead.name : [...nm.entries()].sort((a, b) => b[1] - a[1])[0][0];
      F.weight = F.n + (F.lead ? (F.lead.kind === 'calendar' ? 500 : 60) : 0);
      const cm = new Map(); for (const g of F.inc) { const k = cm.get(g.hex) || { hex: g.hex, raw: g.raw, n: 0 }; k.n += g.items.length; cm.set(g.hex, k); }
      F.colors = [...cm.values()].sort((a, b) => b.n - a.n);
      F.movable = F.inc.some(g => g.editable);
      F.cats = new Set(xs.filter(x => x.kind.startsWith('category')).map(x => x.name));
      F.cal = F.lead && F.lead.kind === 'calendar';
      F.hol = F.lead && THEME[F.lead.kind];
    }
    // the target: the user's pick, else the lead's colour (holidays always, others from "Ordnen" on) or the most used one
    for (const F of list) {
      if (!F.inc.length) { F.to = null; F.why = tr('js-cfo-why-none'); continue; }
      if (st.pins.has(F.key)) { F.to = st.pins.get(F.key); F.pinned = true; F.why = tr('js-cfo-why-pin'); }
      else if (F.split) { F.to = F.colors[0].hex; F.why = tr('js-cfo-why-split'); }
      else if ((L.anchor || F.hol) && F.lead) { F.to = F.lead.hex; F.why = F.lead.std ? tr('js-cfo-why-std', { kind: kindText(F.lead.kind) }) : tr(F.cal ? 'js-cfo-why-cal' : 'js-cfo-why-cat', { name: F.lead.name }); }
      else { F.to = F.colors[0].hex; F.why = tr('js-cfo-why-most', { n: F.colors[0].n }); }
    }
    // "Neue Palette": everything but calendars, holidays and the user's picks gets a colour of the palette
    if (L.repaint) {
      const byW = list.filter(F => F.to).sort((a, b) => b.weight - a.weight);
      const keep = byW.filter(F => F.pinned || F.cal || F.hol || !F.movable);
      const taken = [...keep.map(F => F.to), ...list.flatMap(F => F.groups.filter(g => st.excl.has(g.key) || !g.editable).map(g => g.hex))];
      for (const [F, x] of palette_(byW.filter(F => !keep.includes(F)), taken, L.T)) { F.to = x; F.why = tr('js-cfo-why-pal'); }
    }
    // colours on screen afterwards: an active family shows its target, whatever is left out keeps its own
    const points = () => list.flatMap(F => F.groups.map(g => { const keeps = st.off.has(F.key) || st.excl.has(g.key) || !g.editable || !F.to; return { hex: keeps ? g.hex : F.to, F, g, fixed: keeps }; }));
    const clash = (T, only) => { const p = points(), out = []; for (let i = 0; i < p.length; i++) for (let j = i + 1; j < p.length; j++) { const a = p[i], b = p[j]; if (a.F === b.F) continue; const d = dist(a.hex, b.hex); if (d < T && (!only || only(a, b))) out.push({ a, b, d }); } return out.sort((x, y) => x.d - y.d); };
    // pull apart: picked colours, calendars, holidays and left-out elements stay; the lighter family gives way
    const notes = [], skip = new Set();
    if (L.T) for (let guard = 0; guard < 60; guard++) {
      const can = x => !x.fixed && !x.F.pinned && !x.F.cal && !x.F.hol && x.F.movable && (x.F.movedN || 0) < 2;
      const k = clash(L.T).find(k => (can(k.a) || can(k.b)) && !skip.has(k.a.F.key + '|' + k.b.F.key));
      if (!k) break;
      const [mv, ot] = can(k.a) && (!can(k.b) || k.a.F.weight <= k.b.F.weight) ? [k.a, k.b] : [k.b, k.a], F = mv.F;
      const avoid = [...new Set(points().filter(x => x.F !== F).map(x => x.hex))];
      // always the professional colour farthest from everything that stays
      const to = pickNew(avoid, L.T);
      if (to === F.to) { skip.add(k.a.F.key + '|' + k.b.F.key); continue; }
      F.movedN = (F.movedN || 0) + 1; F.moved = true;
      F.why = tr(k.d < 0.005 ? 'js-cfo-why-same' : 'js-cfo-why-near', { other: ot.F.head, d: dec(k.d) });
      F.to = to;
      if (ot.F.pinned && !ot.fixed) notes.push({ warn: false, F, text: tr('js-cfo-n-moved', { a: ot.F.head, b: F.head }) });
    }
    // hints about the user's own colours
    for (const F of list.filter(F => F.pinned)) {
      const seen = new Set();
      for (const k of clash(SHOWN_T, (a, b) => a.F === F || b.F === F)) {
        const o = k.a.F === F ? k.b : k.a; if (seen.has(o.F.key)) continue; seen.add(o.F.key);
        const who = o.F.head + (o.fixed && st.excl.has(o.g.key) ? ' ' + tr('js-cfo-n-excl') : '');
        const why = o.F.pinned ? 'js-cfo-n-both' : o.F.cal ? 'js-cfo-n-cal' : o.F.hol ? 'js-cfo-n-hol' : o.fixed ? 'js-cfo-n-fixed' : 'js-cfo-n-full';
        notes.push({ warn: true, F, text: tr(k.d < 0.005 ? 'js-cfo-n-same' : 'js-cfo-n-near', { a: F.head, b: who, d: dec(k.d) }) + ' ' + tr(why) });
      }
      if (C.contrast(F.to, LIGHT) < 1.6) notes.push({ warn: true, F, text: tr('js-cfo-n-light', { a: F.head }) });
      if (C.contrast(F.to, DARK) < 1.8) notes.push({ warn: true, F, text: tr('js-cfo-n-dark', { a: F.head }) });
      if (F.inc.some(g => g.kind === 'calendar' && g.editable && g.hex !== F.to)) notes.push({ warn: false, F, text: tr('js-cfo-n-cal-' + view, { a: F.head }) });
    }
    for (const F of list) if (F.cats.size > 1 && F.groups.some(g => g.moved)) notes.push({ warn: false, F, text: tr('js-cfo-n-cats', { list: [...F.cats].map(c => '„' + c + '“').join(', ') }) });
    // lanes: every family that would change something, or that the user touched
    const lanes = [];
    for (const F of list) {
      const on = !st.off.has(F.key);
      F.manual = !!(F.split || F.pinned || !on || F.groups.some(g => g.moved || st.excl.has(g.key)));
      const changes = on && F.to ? F.inc.filter(g => g.editable).flatMap(g => g.items.filter(x => x.hex !== F.to).map(x => ({ item: x, to: F.cal && (x.kind === 'event' || x.kind === 'task') && x.cal === F.lead.name ? '' : F.to }))) : [];
      if (!F.manual && !(F.to && F.inc.some(g => g.editable && g.hex !== F.to))) continue;
      F.type = F.colors.length > 1 ? 'unify' : F.moved || F.split ? 'separate' : L.repaint ? 'repaint' : 'unify';
      lanes.push({ F, changes, on });
    }
    // the order the user saw stays; new lanes go to the end (a split: where it was dropped)
    const T3 = ['unify', 'separate', 'repaint'], def = [...lanes].sort((a, b) => T3.indexOf(a.F.type) - T3.indexOf(b.F.type) || b.F.n - a.F.n);
    const idx = k => { const i = st.order.indexOf(k); return i < 0 ? 1e6 + def.findIndex(l => l.F.key === k) : i; };
    lanes.sort((a, b) => idx(a.F.key) - idx(b.F.key));
    st.order = lanes.map(l => l.F.key);
    const changes = lanes.flatMap(l => l.changes), after = new Map(changes.map(c => [c.item.i, c.to]));
    const shown = x => { const t = after.get(x.i); return t === undefined ? x.hex : t || toHex(x.s.cal_color) || x.hex; };
    const pairs = pts => { const s = new Set(); for (let i = 0; i < pts.length; i++) for (let j = i + 1; j < pts.length; j++) if (pts[i].F !== pts[j].F && dist(pts[i].hex, pts[j].hex) < SHOWN_T) s.add([pts[i].F.key, pts[j].F.key].sort().join('|')); return s.size; };
    const p0 = list.flatMap(F => F.groups.map(g => ({ F, hex: g.hex })));
    return { L, list, lanes, notes, changes, before: new Set(its.map(x => x.hex)).size, after: new Set(its.map(shown)).size, sim0: pairs(p0), sim1: pairs(points()) };
  }

  function optimise(view) {
    const fresh = level => ({ level, pins: new Map(), excl: new Set(), moves: new Map(), off: new Set(), order: [] });
    const manual = st => st.pins.size + st.excl.size + st.moves.size + st.off.size;
    let st = fresh(3), P = null, src = [], ro = null, preview = null;
    const w = document.createElement('div'); w.className = 'ask cfo-wrap'; w.setAttribute('role', 'dialog'); w.setAttribute('aria-modal', 'true'); w.setAttribute('aria-label', tr('js-cfo-title'));
    w.innerHTML = `<div class="ask-box cfo-dlg"><div class="cfo-dh"><span class="cfo-spark">✦</span><span><b>${esc(tr('js-cfo-title'))}</b><small>${esc(tr('js-cfo-sub-' + view))}</small></span><button class="btn icon-btn" type="button" data-x aria-label="${esc(tr('js-color-close'))}"><svg class="ico" aria-hidden="true"><use href="#i-x"/></svg></button></div><div class="cfo-body"></div><div class="ask-acts"><button class="btn icon-btn" type="button" data-x title="${esc(tr('js-ct-cancel'))}" aria-label="${esc(tr('js-ct-cancel'))}"><svg class="ico" aria-hidden="true"><use href="#i-x"/></svg></button><button class="btn btn-primary icon-btn" type="button" data-apply><svg class="ico" aria-hidden="true"><use href="#i-check"/></svg><b class="n" hidden></b></button></div></div>`;
    const bodyEl = $('.cfo-body', w), applyBtn = $('[data-apply]', w);
    const sw = (hex, t) => `<i class="cfo-sw" style="--x:${hex}"${t ? ` title="${esc(t)}"` : ''}></i>`;
    const cname = (hex, raw) => (raw && raw[0] !== '#' ? raw : hex);
    const src1 = (g, F, li) => {
      const ex = st.excl.has(g.key), stay = !ex && g.editable && F.to && g.hex === F.to, n = g.items.length;
      const sub = [kindText(g.kind) + (n > 1 ? ' · ' + n + '×' : ''), cname(g.hex, g.raw), ex ? tr('js-cfo-excl-state') : !g.editable ? tr('js-cfo-ro') : stay ? tr('js-cfo-stay') : ''].filter(Boolean).join(' · ');
      return `<div class="cfo-src${ex ? ' excl' : ''}${g.moved ? ' moved' : ''}${stay ? ' stay' : ''}${g.editable ? '' : ' ro'}" data-n="g:${esc(g.key)}" data-g="${esc(g.key)}" data-l="${li}" style="--x:${g.hex}">`
        + `<span class="cfo-grip" title="${esc(tr('js-cfo-grip'))}">⠿</span>${sw(g.hex)}<span class="cfo-t"><b>${esc(g.name)}</b><small>${esc(sub)}</small>${g.moved ? `<span class="cfo-man"><svg class="ico" aria-hidden="true"><use href="#i-hand"/></svg>${esc(tr(g.moved === 'split' ? 'js-cfo-man-split' : 'js-cfo-man-moved'))}</span>` : ''}</span>`
        + (g.moved ? `<button type="button" class="cfo-mini" data-unmove="${esc(g.key)}" title="${esc(tr('js-cfo-unmove'))}">↺</button>` : '')
        + (g.editable ? `<button type="button" class="cfo-mini${ex ? ' on' : ''}" data-excl="${esc(g.key)}" title="${esc(tr(ex ? 'js-cfo-incl-t' : 'js-cfo-excl'))}">${ex ? esc(tr('js-cfo-incl')) : '⊘'}</button>` : '') + '</div>';
    };
    const draw = () => {
      const keep = bodyEl.scrollTop;
      P = plan(src, view, st);
      const n = manual(st), edges = [];
      const lanes = P.lanes.map(({ F, changes, on }, li) => {
        const groups = [...F.groups].sort((a, b) => (F.to && a.hex === F.to) - (F.to && b.hex === F.to) || b.items.length - a.items.length);
        const left = groups.map(g => { if (!st.excl.has(g.key) && g.editable && F.to) edges.push(g.hex === F.to ? ['g:' + g.key, 'r:' + F.key, 'stay', li, g.hex] : ['g:' + g.key, 'm:' + F.key, on ? 'in' : 'off', li, g.hex]); return src1(g, F, li); }).join('');
        if (F.to) edges.push(['m:' + F.key, 'r:' + F.key, on ? 'out' : 'off', li, F.to]);
        const tag = F.split ? 'js-cfo-tag-split' : F.groups.some(g => g.moved) ? 'js-cfo-tag-moved' : F.pinned ? 'js-cfo-tag-pin' : '';
        const warn = P.notes.some(x => x.warn && x.F === F);
        const mid = `<button type="button" class="cfo-mid${on ? '' : ' off'}${F.manual ? ' manual' : ''}" data-n="m:${esc(F.key)}" data-off="${esc(F.key)}" data-l="${li}" title="${esc(tr(on ? 'js-cfo-off-t' : 'js-cfo-on-t'))}"><span class="cfo-b ${F.type}">${esc(tr('js-cfo-t-' + F.type))}</span>${tag ? `<span class="cfo-man"><svg class="ico" aria-hidden="true"><use href="#i-hand"/></svg>${esc(tr(tag))}</span>` : ''}<small>${esc(F.why)}</small><span class="cfo-ck">${esc(on ? (changes.length ? tr('js-cfo-on', { n: changes.length }) : tr('js-cfo-none')) : tr('js-cfo-off'))}</span></button>`;
        const inh = F.cal && F.inc.some(g => g.kind !== 'calendar');
        const res = F.to ? `<button type="button" class="cfo-res${on ? '' : ' off'}${F.pinned ? ' pinned' : ''}${warn ? ' warn' : ''}" data-n="r:${esc(F.key)}" data-pick="${esc(F.key)}" data-l="${li}" title="${esc(tr('js-cfo-pick'))}">${sw(F.to)}<span class="cfo-t"><b>${esc(cname(F.to, (F.colors.find(c => c.hex === F.to) || {}).raw))}</b><small>${esc(tr('js-cfo-entries', { n: F.n }) + (inh ? ' · ' + tr('js-color-inherit-cal', { cal: F.lead.name }) : '') + (F.moved && !F.pinned ? ' · ' + tr('js-cfo-new') : ''))}</small>${F.pinned ? `<span class="cfo-man"><svg class="ico" aria-hidden="true"><use href="#i-hand"/></svg>${esc(tr('js-cfo-tag-pin'))}${warn ? ' · ⚠' : ''}</span>` : ''}</span>${F.pinned ? `<span class="cfo-mini" role="button" tabindex="0" data-unpin="${esc(F.key)}" title="${esc(tr('js-cfo-unpin'))}">↺</span>` : `<span class="cfo-edit">${esc(tr('js-cfo-edit'))}</span>`}</button>` : '';
        return `<div class="cfo-lane${F.manual ? ' manual' : ''}" data-l="${li}" data-f="${esc(F.key)}"><div class="cfo-lc">${left}</div><div class="cfo-lc">${mid}</div><div class="cfo-lc">${res}</div></div>`;
      }).join('');
      const L = P.L, lv = st.level;
      bodyEl.innerHTML = `<div class="cfo-level"><div class="cfo-lh"><span>${esc(tr('js-cfo-scope'))}</span><b>${esc(tr('js-cfo-l' + lv))}</b>${L.rec ? `<em>${esc(tr('js-cfo-rec'))}</em>` : ''}</div>
        <input type="range" min="0" max="${LEVELS.length - 1}" step="1" value="${lv}" data-level aria-label="${esc(tr('js-cfo-scope'))}">
        <div class="cfo-ticks">${LEVELS.map((x, i) => `<button type="button" data-lv="${i}" aria-pressed="${i === lv}">${esc(tr('js-cfo-l' + i))}</button>`).join('')}</div><p class="cfo-ltext">${esc(tr('js-cfo-l' + lv + '-text'))}</p></div>
        <div class="cfo-sum"><span class="cfo-m good">${esc(tr('js-cfo-m-colors', { a: P.before, b: P.after }))}</span><span class="cfo-m${P.sim1 < P.sim0 ? ' good' : ''}">${esc(tr('js-cfo-m-similar', { a: P.sim0, b: P.sim1 }))}</span><span class="cfo-m">${esc(tr('js-cfo-m-changes', { n: P.changes.length }))}</span>${n ? `<span class="cfo-manbar"><svg class="ico" aria-hidden="true"><use href="#i-hand"/></svg>${esc(tr('js-cfo-manual', { n }))} <button type="button" class="link" data-discard>${esc(tr('js-cfo-discard'))}</button></span>` : ''}</div>
        ${P.notes.length ? `<div class="cfo-notes">${P.notes.map(x => `<p class="${x.warn ? 'warn' : 'info'}">${x.warn ? '⚠' : 'ℹ'} ${esc(x.text)}</p>`).join('')}</div>` : ''}
        ${P.lanes.length ? `<div class="cfo-head"><span>${esc(tr('js-cfo-h-now'))}</span><span>${esc(tr('js-cfo-h-act'))}</span><span>${esc(tr('js-cfo-h-after'))}</span></div><div class="cfo-fd"><svg class="cfo-svg" aria-hidden="true"></svg>${lanes}</div><p class="cfo-help">${esc(tr('js-cfo-help'))}</p>`
          : `<div class="cfo-empty"><b>${esc(tr('js-cfo-empty'))}</b><span>${esc(tr('js-cfo-empty-hint'))}</span></div>`}`;
      const aT = tr('js-cfo-apply') + (P.changes.length ? ' · ' + P.changes.length : ''); applyBtn.title = aT; applyBtn.setAttribute('aria-label', aT); const nB = $('.n', applyBtn); nB.textContent = P.changes.length; nB.hidden = !P.changes.length; applyBtn.disabled = !P.changes.length;
      bodyEl.scrollTop = keep;
      const box = $('.cfo-fd', bodyEl); if (ro) ro.disconnect(); if (!box) return;
      const lines = () => {
        const svg = $('.cfo-svg', box), B = box.getBoundingClientRect();
        svg.setAttribute('width', box.scrollWidth); svg.setAttribute('height', box.scrollHeight);
        const at = id => { const el = box.querySelector(`[data-n="${CSS.escape(id)}"]`); if (!el) return null; const q = el.getBoundingClientRect(); return { l: q.left - B.left, r: q.right - B.left, t: q.top - B.top, b: q.bottom - B.top }; };
        svg.innerHTML = edges.map(([a, b, k, li, hex]) => {
          const p = at(a), q = at(b); if (!p || !q) return '';
          // side by side: right edge to left edge; stacked (phones: action above its colour): bottom to top
          const d = q.l > p.r + 8 ? (() => { const y1 = (p.t + p.b) / 2, y2 = (q.t + q.b) / 2, dx = Math.max(24, (q.l - p.r) / 2); return `M${p.r} ${y1} C${p.r + dx} ${y1},${q.l - dx} ${y2},${q.l} ${y2}`; })()
            : (() => { const x1 = (p.l + p.r) / 2, x2 = (q.l + q.r) / 2, dy = Math.max(12, (q.t - p.b) / 2); return `M${x1} ${p.b} C${x1} ${p.b + dy},${x2} ${q.t - dy},${x2} ${q.t}`; })();
          return `<path d="${d}" class="${k}" data-l="${li}" style="--x:${hex}"/>`;
        }).join('');
      };
      lines(); ro = new ResizeObserver(lines); ro.observe(box);
    };
    const ask = (text, yes, then) => askYesNo(text, yes, tr('js-ct-cancel'), true).then(ok => { if (ok) then(); else draw(); });
    const setLevel = v => {
      if (v === st.level) return draw();
      const n = manual(st);
      if (!n) { st = fresh(v); return draw(); }
      ask(tr('js-cfo-ask-level', { n, level: tr('js-cfo-l' + v) }), tr('js-cfo-ask-yes'), () => { st = fresh(v); draw(); });
    };
    // while the slider moves only its words follow; the plan is recomputed on release
    bodyEl.addEventListener('input', e => { if (!e.target.matches('[data-level]')) return; const v = preview = +e.target.value; $('.cfo-lh b', bodyEl).textContent = tr('js-cfo-l' + v); $('.cfo-ltext', bodyEl).textContent = tr('js-cfo-l' + v + '-text'); });
    bodyEl.addEventListener('change', e => { if (e.target.matches('[data-level]') && preview != null) { const v = preview; preview = null; setLevel(v); } });
    const end = () => { if (ro) ro.disconnect(); removeEventListener('keydown', key, true); close(); w.remove(); };
    const key = e => { if (e.key === 'Escape' && !$('.cpk') && !$('.cpk-ask') && !$('.ask:not(.cfo-wrap)')) { e.preventDefault(); end(); } };
    const applyAll = async () => {
      const list = P.changes.map(c => ({ kind: c.item.kind, key: c.item.kind.startsWith('category') ? c.item.name : c.item.key, color: c.to }));
      applyBtn.disabled = true;
      const r = await fetch('/api/colors/batch', { method: 'POST', headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf() }, body: JSON.stringify({ changes: list }) }).then(x => x.ok ? x.json() : null).catch(() => null);
      if (!r) { applyBtn.disabled = false; toast(tr('js-set-err'), true); return; }
      // holiday colours live in the prefs form too (autosave would write the old value back); calendars elsewhere on the page
      for (const c of list) { const vr = VAR[c.kind]; if (vr) { const f = field(vr[0]); if (f) f.value = c.color; themeVar(vr[1], c.color); } else if (c.kind === 'calendar') syncPage('calendar', c.key, c.color); }
      toast(tr('js-color-changed', { n: r.count }) + (r.failed ? ' ' + tr('js-cfo-failed', { n: r.failed }) : ''), !!r.failed);
      cache = null; end(); draw_(true);
    };
    w.addEventListener('click', e => {
      const t = e.target; let b;
      if (t.closest('[data-x]')) return end();
      if (t.closest('[data-apply]')) return applyAll();
      if ((b = t.closest('[data-lv]'))) return setLevel(+b.dataset.lv);
      if (t.closest('[data-discard]')) return ask(tr('js-cfo-ask-discard'), tr('js-cfo-discard-yes'), () => { st = fresh(st.level); draw(); });
      if ((b = t.closest('[data-unpin]'))) { e.stopPropagation(); st.pins.delete(b.dataset.unpin); return draw(); }
      if ((b = t.closest('[data-unmove]'))) { st.moves.delete(b.dataset.unmove); return draw(); }
      if ((b = t.closest('[data-excl]'))) { const k = b.dataset.excl; st.excl.has(k) ? st.excl.delete(k) : st.excl.add(k); return draw(); }
      if ((b = t.closest('[data-off]'))) { const k = b.dataset.off; st.off.has(k) ? st.off.delete(k) : st.off.add(k); return draw(); }
      if ((b = t.closest('[data-pick]'))) {
        const F = P.list.find(f => f.key === b.dataset.pick);
        open(b, { once: true, title: F.head, sub: tr('js-cfo-entries', { n: F.n }), what: F.head, value: F.to, inherit: null, self: { list: F.inc.flatMap(g => g.items.map(x => x.s)) }, onPick: v => { close(); if (v) { st.pins.set(F.key, toHex(v)); draw(); } } });
      }
    });
    w.addEventListener('keydown', e => { const u = e.target.closest && e.target.closest('[data-unpin]'); if (u && (e.key === 'Enter' || e.key === ' ')) { e.preventDefault(); e.stopPropagation(); st.pins.delete(u.dataset.unpin); draw(); } });

    // ---- dragging an element up or down: onto an action = join it, onto a gap between two = its own action ----
    let dg = null;
    const hit = y => {
      const ls = $$('.cfo-lane', bodyEl);
      for (let i = 0; i < ls.length; i++) {
        const r = ls[i].getBoundingClientRect();
        if (y < r.top + 14 && y >= r.top - 8) return { split: i, y: r.top };
        if (y > r.bottom - 14 && y <= r.bottom + 8) return { split: i + 1, y: r.bottom };
        if (y >= r.top && y <= r.bottom) return { lane: ls[i] };
      }
      if (!ls.length) return null;
      const first = ls[0].getBoundingClientRect(), last = ls[ls.length - 1].getBoundingClientRect();
      return y < first.top ? { split: 0, y: first.top } : { split: ls.length, y: last.bottom };
    };
    const laneOf = gk => { const s = $(`.cfo-src[data-g="${CSS.escape(gk)}"]`, bodyEl); return s && s.closest('.cfo-lane').dataset.f; };
    const unmark = () => { $$('.cfo-lane.drop', bodyEl).forEach(x => x.classList.remove('drop')); const s = $('.cfo-sep', bodyEl); if (s) s.remove(); };
    bodyEl.addEventListener('pointerdown', e => {
      const s = e.target.closest('.cfo-src'); if (!s || e.button || e.target.closest('button') || s.classList.contains('excl') || s.classList.contains('ro')) return;
      dg = { gk: s.dataset.g, s, y0: e.clientY, on: false }; s.setPointerCapture(e.pointerId);
    });
    bodyEl.addEventListener('pointermove', e => {
      if (!dg) return;
      if (!dg.on) {
        if (Math.abs(e.clientY - dg.y0) < 5) return;
        const r = dg.s.getBoundingClientRect(); dg.on = true; dg.dy = dg.y0 - r.top;
        dg.ghost = dg.s.cloneNode(true); dg.ghost.classList.add('cfo-ghost'); Object.assign(dg.ghost.style, { left: r.left + 'px', width: r.width + 'px' });
        document.body.appendChild(dg.ghost); dg.s.classList.add('dragging');
      }
      dg.ghost.style.top = e.clientY - dg.dy + 'px';
      const sr = bodyEl.getBoundingClientRect();
      if (e.clientY < sr.top + 40) bodyEl.scrollTop -= 14; else if (e.clientY > sr.bottom - 40) bodyEl.scrollTop += 14;
      unmark();
      const h = dg.hit = hit(e.clientY); if (!h) return;
      if (h.lane) { if (h.lane.dataset.f !== laneOf(dg.gk)) { h.lane.classList.add('drop'); h.lane.dataset.hint = tr('js-cfo-drop-join'); } }
      else { const fd = $('.cfo-fd', bodyEl), d = document.createElement('div'); d.className = 'cfo-sep'; d.style.top = h.y - fd.getBoundingClientRect().top - 2 + 'px'; d.innerHTML = `<span>${esc(tr('js-cfo-drop-split'))}</span>`; fd.appendChild(d); }
    });
    const drop = () => {
      if (!dg) return; const d = dg; dg = null; unmark();
      if (!d.on) return;
      d.ghost.remove(); d.s.classList.remove('dragging');
      const h = d.hit, g = d.gk, cur = laneOf(g), home = (P.list.flatMap(F => F.groups).find(x => x.key === g) || {}).home;
      if (!h) return;
      if (h.lane) { const to = h.lane.dataset.f; if (to === cur) return; if (to === home) st.moves.delete(g); else st.moves.set(g, to); }
      else if (cur === 'x:' + g) { const o = st.order.filter(k => k !== cur); o.splice(Math.min(h.split, o.length), 0, cur); st.order = o; }
      else { st.moves.set(g, 'split'); st.order.splice(h.split, 0, 'x:' + g); }
      draw();
    };
    bodyEl.addEventListener('pointerup', drop); bodyEl.addEventListener('pointercancel', drop);

    addEventListener('keydown', key, true);
    document.body.appendChild(w);
    load(true).then(list => { src = list; draw(); $('[data-level]', w).focus(); });
  }
  const draw_ = f => draw(f);
  const settingsSaved = () => ($('.settings') || {}).dataset?.tSaved || '';
  draw(false);
  addEventListener('hashchange', () => { if (location.hash === '#colors') draw(true); });
})();

/* ===== calendar page ===== */
(function () {
  const root = document.getElementById('viewroot');
  if (!root) return;
  const $ = (s, r = document) => r.querySelector(s), $$ = (s, r = document) => [...r.querySelectorAll(s)];
  const csrf = document.querySelector('meta[name=csrf]').content;
  const lang = document.body.dataset.lang || 'de';
  const MON = MONTHS;
  const DOW = WEEKDAYS;
  const T = trs('js-ed-', ['suggNone', 'saved', 'err', 'del', 'delRule', 'series', 'newEv', 'editEv', 'conflict', 'copied', 'delTask']);
  const iso = d => d.toISOString().slice(0, 10);
  const esc = s => String(s).replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]);
  const edOpen = (key, date) => window.rusticalEd && window.rusticalEd.open(key, date);
  const parse = s => { const [y, m, d] = s.split('-').map(Number); return new Date(y, m - 1, d); };
  const pad = n => String(n).padStart(2, '0');
  // date = the picked day, which every view shows as selected; month (YYYY-MM) / week (its Monday) = what the
  // month / week view has on screen once scrolling left the picked day's — scrolling never changes the picked day
  const q0 = new URL(location.href).searchParams;
  let state = { view: root.dataset.view, date: root.dataset.date, cals: root.dataset.cals || '', month: q0.get('month'), week: q0.get('week') };
  const shown = () => state.view === 'month' && state.month ? state.month + '-01' : state.view === 'week' && state.week ? state.week : state.date; // the date whose month or week is on screen
  root.dataset.date = shown();
  let loading = null; // before the filter setup below: a filter kept in localStorage calls load() right away
  const today = root.dataset.today;

  // ---- filter (localStorage-persisted set of hidden calendars) ----
  const filterBox = $('#calfilter');
  const hidden = new Set(JSON.parse(localStorage.calHidden || '[]'));
  function calsParam() { const all = $$('input[data-cal]', filterBox).map(i => i.dataset.cal); const on = all.filter(c => !hidden.has(c)); return on.length === all.length ? '' : on.join(',') || '-'; } // '-' matches no calendar: an empty list means all of them
  if (filterBox) {
    $$('input[data-cal]', filterBox).forEach(i => { i.checked = !hidden.has(i.dataset.cal); i.addEventListener('change', () => { if (i.checked) hidden.delete(i.dataset.cal); else hidden.add(i.dataset.cal); persistFilter(); load(); }); });
    $$('[data-filter]', filterBox).forEach(a => a.addEventListener('click', e => { e.preventDefault(); const all = $$('input[data-cal]', filterBox); all.forEach(i => { const mine = !i.parentElement.querySelector('small'); i.checked = a.dataset.filter === 'all' || (a.dataset.filter === 'mine' && mine); if (i.checked) hidden.delete(i.dataset.cal); else hidden.add(i.dataset.cal); }); persistFilter(); load(); }));
    state.cals = calsParam();
    if (state.cals !== (root.dataset.cals || '')) load(false);
  }
  function persistFilter() { localStorage.calHidden = JSON.stringify([...hidden]); state.cals = calsParam(); dropCarried(); }
  // holidays: a setting of the user (not of this browser), saved at once, then the view redraws
  $$('input[data-hol]', filterBox || document.createElement('div')).forEach(i => i.addEventListener('change', async () => {
    const body = {}; $$('input[data-hol]', filterBox).forEach(x => { body[x.dataset.hol] = x.checked; }); // no school switch → the setting stays
    await fetch('/settings/holiday-display', { method: 'POST', headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf }, body: JSON.stringify(body) });
    dropCarried(); load(false);
  }));

  // ---- navigation / fragment loading ----
  function url(fragment) { const u = new URL('/calendar', location.origin); u.searchParams.set('view', state.view); u.searchParams.set('date', state.date); if (state.view === 'month' && state.month && state.month !== state.date.slice(0, 7)) u.searchParams.set('month', state.month); if (state.view === 'week' && state.week && state.week !== localIso(weekStart(parse(state.date)))) u.searchParams.set('week', state.week); if (state.cals) u.searchParams.set('cals', state.cals); if (fragment) u.searchParams.set('fragment', '1'); return u; }
  async function load(push = true) {
    const u = url(true);
    loading = u;
    // a month-to-month change glides (wheel, rail, orbit picker, today alike); the months in between are fetched alongside the target
    const turnOf = from => root.dataset.view === 'month' && state.view === 'month' && from.slice(0, 7) !== shown().slice(0, 7) ? (shown() > from ? 1 : -1) : 0;
    const from0 = root.dataset.date, between0 = turnOf(from0) ? monthsBetween(from0, shown()) : [];
    const midsP = between0.length && between0.length <= FLIP_FETCH && !reducedMotion() ? fetchMonths(between0) : Promise.resolve(null);
    try {
      const r = await fetch(u, { headers: { Accept: 'text/html' } });
      if (loading !== u) return;
      if (!r.ok) throw new Error(r.status);
      const html = await r.text(), mids = await midsP; tlScroll = $('.tl', root) ? $('.tl', root).scrollTop : null;
      ledScroll = ledNode ? ledNode.scrollTop : null; // the ledger is carried over, and a detached node forgets where it stood
      if (flip) flip.end(); const from = root.dataset.date, turn = turnOf(from), oldMonth = $('.month', root);
      const between = !turn ? [] : from === from0 ? between0 : monthsBetween(from, shown()); // another load swapped meanwhile: the prefetched months no longer fit
      root.innerHTML = html;
      root.dataset.view = state.view; root.dataset.date = shown();
      const h1 = root.querySelector('#title'); if (h1) document.title = h1.textContent.trim() + ' · ' + document.title.split(' · ').slice(1).join(' · ');
      if (push) history.pushState(state, '', url(false).pathname + url(false).search);
      wire(); if (turn) monthFlip(oldMonth, turn, between, from === from0 ? mids : null);
    } catch (e) { console.error(e); }
  }
  addEventListener('popstate', e => { if (e.state) { state = e.state; load(false); } });
  function shift(dir) {
    const d = parse(state.date);
    if (state.view === 'day') d.setDate(d.getDate() + dir);
    else if (state.view === 'week') d.setDate(d.getDate() + 7 * dir);
    else if (state.view === 'month') { const m = parse(shown()); m.setDate(1); m.setMonth(m.getMonth() + dir); state.month = localIso(m).slice(0, 7); return load(); }
    else d.setFullYear(d.getFullYear() + dir);
    state.date = iso(new Date(d.getTime() - d.getTimezoneOffset() * 60000));
    load();
  }
  function go(view, date) { if (date || (view && view !== state.view)) state.month = state.week = null; if (view) state.view = view; if (date) state.date = date; load(); } // a picked day or another view shows the picked day's month again
  function pickDay(date) { // a click picks the day; the month or week on screen stays
    if (state.view === 'month') { state.month = shown().slice(0, 7); state.date = date; load(false); } // the agenda beside the grid shows it
    else { state.date = date; markPicked(); }
    history.replaceState(state, '', url(false).pathname + url(false).search); }
  function markPicked() { $$('.stream .dayblock, .planner .c[data-date]', root).forEach(b => b.classList.toggle('sel', b.dataset.date === state.date)); } // hoisted: wireItems calls it
  // the month grid owns the wheel: one gesture turns exactly one page, however far it goes. A new
  // gesture is told by speed, not by a pause: a trackpad's momentum tail only ever slows down, while
  // a fresh swipe arrives faster than the tail it interrupts — the speed must have fallen below half
  // the gesture's peak and then jump above twice the recent mean (a finger still accelerating after
  // the page turned is neither). Time gaps proved unusable: the lift-off before the tail and Chrome
  // coalescing events while the page swaps both open gaps that look like rests (two pages per swipe),
  // and the rest between two swipes can be shorter than either (no page at all). Speed is taken over
  // the event's own time, so a coalesced event (one big delta over a long gap) keeps its true speed.
  // State lives outside the fragment so the events that arrive after the swap still count as the
  // same gesture.
  const MONTH_FLIP = 60, MONTH_REST = 150, MONTH_JUMP = 2, MONTH_MINV = 0.25; // px that turn the page; ms of rest that end a gesture for sure; speed ratio and floor (px/ms) of a fresh swipe
  let mwLast = 0, mwAcc = 0, mwDone = false, mwVs = [], mwPeak = 0;
  function monthWheel(e) {
    e.preventDefault(); const now = e.timeStamp, dy = e.deltaY * (e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? e.currentTarget.clientHeight : 1);
    const gap = now - mwLast, v = Math.abs(dy) / Math.max(1, gap), ref = mwVs.reduce((a, b) => a + b, 0) / (mwVs.length || 1);
    const fresh = gap > MONTH_REST || (mwDone && v >= MONTH_MINV && ref < mwPeak / 2 && v > MONTH_JUMP * ref);
    if (fresh) { mwAcc = 0; mwDone = false; mwVs = []; mwPeak = 0; } mwLast = now; mwVs = mwVs.concat(v).slice(-3); mwPeak = Math.max(mwPeak, v);
    if (mwDone) return;
    mwAcc += dy;
    if (Math.abs(mwAcc) >= MONTH_FLIP) { mwDone = true; shift(mwAcc > 0 ? 1 : -1); }
  }
  // page turn: the leaving grid, every month in between and the arriving grid form one column that
  // glides past, eased in and out — the same glide whatever changed the month, and a far jump passes
  // every month on the way. The months in between are fetched with the target (up to FLIP_FETCH of
  // them); beyond that they are drawn as bare day grids, at that speed only the shape is visible.
  // A turn that starts mid-glide ends the running one first (load()).
  const FLIP_MS = 480, FLIP_STEP = 90, FLIP_MAX = 1500, FLIP_EASE = 'cubic-bezier(.65,0,.35,1)', FLIP_GAP = 24, FLIP_FETCH = 12; // ms for one page, ms per further page, cap; px between pages
  let flip = null;
  const reducedMotion = () => matchMedia('(prefers-reduced-motion: reduce)').matches;
  const localIso = d => iso(new Date(d.getTime() - d.getTimezoneOffset() * 60000));
  function monthsBetween(a, b) { const out = [], d = parse(a.slice(0, 7) + '-01'), dir = b > a ? 1 : -1; for (;;) { d.setMonth(d.getMonth() + dir); const m = localIso(d); if (m.slice(0, 7) === b.slice(0, 7)) return out; out.push(m); } } // first days of the months strictly between, in travel order
  const fetchMonths = list => Promise.all(list.map(async m => { try { const u = url(true); u.searchParams.set('month', m.slice(0, 7)); const r = await fetch(u, { headers: { Accept: 'text/html' } }); return r.ok ? new DOMParser().parseFromString(await r.text(), 'text/html').querySelector('.month') : null; } catch { return null; } }));
  function skeleton(m, dows) { const d = parse(m), y = d.getFullYear(), mo = d.getMonth(), first = new Date(y, mo, 1), c = new Date(y, mo, 1 - ((first.getDay() + 6) % 7)); let h = dows.map(x => `<div class="dow">${esc(x)}</div>`).join('');
    for (let i = 0; ; i++, c.setDate(c.getDate() + 1)) { if (i && i % 7 === 0 && (c.getMonth() !== mo || c.getFullYear() !== y)) break; h += `<div class="day${c.getMonth() !== mo ? ' other' : ''}"><span class="n">${c.getDate()}</span></div>`; }
    const el = document.createElement('div'); el.className = 'month'; el.innerHTML = h; return el; } // same week rule as render.rs month_weeks
  function monthFlip(old, dir, between, mids) {
    const cur = $('.month', root); if (!old || !cur || reducedMotion()) return;
    const dows = $$('.dow', cur).map(x => x.textContent), pages = between.map((m, i) => (mids && mids[i]) || skeleton(m, dows));
    const H = cur.getBoundingClientRect().height, step = H + FLIP_GAP, n = pages.length + 1;
    const wrap = document.createElement('div'); wrap.className = 'mflip'; wrap.style.height = `${H}px`; cur.replaceWith(wrap);
    const col = document.createElement('div'); col.className = 'mcol'; col.style.gridAutoRows = `${H}px`; col.style.gap = `${FLIP_GAP}px`;
    col.append(...(dir > 0 ? [old, ...pages, cur] : [cur, ...pages.reverse(), old])); wrap.append(col); // top to bottom = earlier to later
    wrap.addEventListener('wheel', monthWheel, { passive: false }); // the wheel keeps working over the moving column
    const a = col.animate([{ transform: `translateY(${dir > 0 ? 0 : -n * step}px)` }, { transform: `translateY(${dir > 0 ? -n * step : 0}px)` }], { duration: Math.min(FLIP_MAX, FLIP_MS + FLIP_STEP * (n - 1)), easing: FLIP_EASE, fill: 'forwards' });
    const end = () => { if (!flip) return; flip = null; a.cancel(); if (wrap.parentNode) wrap.replaceWith(cur); };
    flip = { end }; a.onfinish = end;
  }

  // ---- timeline (right rail): client-rendered, endless in both directions; the SVG node and the
  // scroll position survive fragment swaps of the same kind, so selecting never re-centres ----
  const tip = document.createElement('div'); tip.className = 'tip'; tip.hidden = true; document.body.appendChild(tip);
  // `per` = items a year takes (weeks: 52-53 dots + 12 month lines; months: 12 dots + 1 year line).
  // The step sits halfway between the loose `step` and the one at which a whole year would fit the
  // rail — dense enough to show a good stretch, loose enough to read. A year marker is always on
  // screen regardless, because the top one sticks (`tlYearSync`). The chunk follows the density, or
  // a short rail would never fill its own height.
  const TLK = { weeks: { step: 30, y0: 16, r: 3.5, chunk: 26, per: 65 }, months: { step: 24, y0: 14, r: 3.5, chunk: 24, per: 13 } };
  const TL_LABEL = 16, TL_MID = 8; // height of the sticky year label and the height its line sits at
  let tlStep = 0, tlR = 0, tlRing = 0, tlChunk = 0;
  function tlMetrics(box, kind) { const k = TLK[kind], h = Math.max(200, box.clientHeight);
    const step = Math.max(6, Math.min(k.step, Math.round((k.step + h / k.per) / 2)));
    return { step, r: Math.min(k.r, step * 0.3), ring: Math.max(4.5, Math.min(7.5, step * 0.62)), chunk: Math.max(k.chunk, Math.ceil(h / step)) }; }
  const fmt = d => d.getFullYear() + '-' + String(d.getMonth() + 1).padStart(2, '0') + '-' + String(d.getDate()).padStart(2, '0');
  const dm = d => dfmt(d, { day: 'numeric', month: 'numeric' });
  const isoWeek = d => { const t = new Date(Date.UTC(d.getFullYear(), d.getMonth(), d.getDate())); const day = t.getUTCDay() || 7; t.setUTCDate(t.getUTCDate() + 4 - day); return Math.ceil(((t - Date.UTC(t.getUTCFullYear(), 0, 1)) / 864e5 + 1) / 7); };
  const weekStart = d => { const w = new Date(d); w.setDate(w.getDate() - ((w.getDay() + 6) % 7)); return w; };
  const stepDate = (kind, d, n) => { const x = new Date(d); if (kind === 'weeks') x.setDate(x.getDate() + 7 * n); else x.setMonth(x.getMonth() + n); return x; };
  const periodOf = (kind, d) => kind === 'weeks' ? weekStart(d) : new Date(d.getFullYear(), d.getMonth(), 1);
  let tlSvg = null, tlKind = null, tlFirst = null, tlLast = null, tlItems = [], tlScroll = null;
  // items: one per period, optional line before it (month start in weeks, January in months)
  function tlBuild(kind) {
    const k = { ...TLK[kind], step: tlStep, r: tlR }, todayD = parse(today), nowP = fmt(periodOf(kind, todayD));
    let y = k.y0, out = '', items = [];
    for (let d = new Date(tlFirst); d <= tlLast; d = stepDate(kind, d, 1)) {
      if (kind === 'weeks') {
        const first = [0, 1, 2, 3, 4, 5, 6].map(i => { const x = new Date(d); x.setDate(x.getDate() + i); return x; }).find(x => x.getDate() === 1);
        if (first) { const lbl = dfmt(first, { day: 'numeric', month: 'long', year: 'numeric' }), isY = first.getMonth() === 0; out += `<g class="mline${isY ? ' y' : ''}" data-y="${y}" data-date="${fmt(first)}" data-label="${lbl}"><line x1="18" x2="46" y1="${y}" y2="${y}"/><text x="50" y="${y + 4}">${isY ? first.getFullYear() : MON[first.getMonth()].slice(0, 3)}</text></g>`; items.push({ y, date: fmt(first), label: lbl, year: isY ? first.getFullYear() : 0 }); y += k.step; }
        const e = new Date(d); e.setDate(e.getDate() + 6);
        const lbl = `${tr('js-week-short', { w: isoWeek(d) })} · ${dm(d)} – ${dm(e)}`, ds = fmt(d);
        out += `<g class="wdot${ds === nowP ? ' now' : ''}" data-y="${y}" data-date="${ds}" data-label="${lbl}"><circle cx="32" cy="${y}" r="${k.r}"/></g>`;
        items.push({ y, date: ds, label: lbl, period: ds }); y += k.step;
      } else {
        if (d.getMonth() === 0) { out += `<g class="mline y" data-y="${y}" data-date="${fmt(d)}" data-label="${d.getFullYear()}"><line x1="18" x2="46" y1="${y}" y2="${y}"/><text x="50" y="${y + 4}">${d.getFullYear()}</text></g>`; items.push({ y, date: fmt(d), label: String(d.getFullYear()), year: d.getFullYear() }); y += k.step; }
        const lbl = `${MON[d.getMonth()]} ${d.getFullYear()}`, ds = fmt(d);
        out += `<g class="wdot${ds === nowP ? ' now' : ''}" data-y="${y}" data-date="${ds}" data-label="${lbl}"><circle cx="32" cy="${y}" r="${k.r}"/></g>`;
        items.push({ y, date: ds, label: lbl, period: ds }); y += k.step;
      }
    }
    tlSvg.setAttribute('height', y); tlSvg.setAttribute('viewBox', `0 0 88 ${y}`);
    tlSvg.innerHTML = out + `<circle class="sel" cx="32" r="${tlRing}" hidden/><circle class="pre" cx="32" r="${tlRing}" hidden/>`;
    tlItems = items; tlMark();
    return y;
  }
  function tlYearSync(box) { // the top year marker stays put until the next one pushes it out
    const el = $('.tly', box); if (!el || !tlItems.length) return;
    // The label draws its line at its own middle, so it stands for a marker at TL_MID. Handover
    // happens exactly when the arriving marker reaches that height: the old label is pushed out by
    // then and the new one takes its place with both lines at the same height. The year comes from
    // the last marker crossed, never from the item at the top — in the week rail the dot after the
    // January line carries its Monday, which usually still lies in December.
    const top = box.scrollTop + TL_MID;
    let cur = 0; for (let i = tlItems.length - 1; i >= 0; i--) if (tlItems[i].year && tlItems[i].y <= top) { cur = tlItems[i].year; break; }
    const next = tlItems.find(i => i.year && i.y > top);
    if (!cur) cur = next ? next.year - 1 : parse(tlItems[0].date).getFullYear();
    el.firstElementChild.textContent = cur;
    el.style.transform = next && next.y - top < TL_LABEL ? `translateY(${next.y - top - TL_LABEL}px)` : '';
  }
  const svgShow = (el, on) => on ? el.removeAttribute('hidden') : el.setAttribute('hidden', ''); // SVG elements have no .hidden property
  function tlMark() { // selection ring on the viewed period
    if (!tlSvg) return; const sel = tlSvg.querySelector('.sel'); const p = fmt(periodOf(tlKind, parse(shown())));
    const it = tlItems.find(i => i.period === p); if (it) sel.setAttribute('cy', it.y); svgShow(sel, !!it);
  }
  const TL_EDGE = 14; // px the ring keeps from the rail's edge — its own radius plus a little air
  function tlFollow() { // one rule for every rail (weeks in day/week, months elsewhere): it only moves when the
    const rail = $('.tl', root); if (!rail || !tlSvg) return; // ring would leave it, and then just far enough to let the ring hang at that edge
    const it = tlItems.find(i => i.period === fmt(periodOf(tlKind, parse(shown())))); if (!it) return;
    const y = it.y - rail.scrollTop, h = rail.clientHeight, edge = TL_EDGE + TL_LABEL; // the sticky year label sits on the top edge
    if (y < edge) rail.scrollTop = Math.max(0, it.y - edge);
    else if (y > h - TL_EDGE) rail.scrollTop = it.y - (h - TL_EDGE);
  }
  const tlSelect = () => { tlMark(); tlFollow(); }; // the viewed period changed: mark it and keep it in sight
  let tlHome = false; // the today button is the one jump that centres the rail instead of letting the ring hang at an edge
  const tlCentre = box => { tlMark(); const it = tlItems.find(i => i.period === fmt(periodOf(tlKind, parse(shown())))); box.scrollTop = it ? Math.max(0, it.y - box.clientHeight / 2) : 0; };
  function tlExtend(box) { // keep two chunks of slack above and below the viewport
    let changed = false;
    if (box.scrollTop < 300) { tlFirst = stepDate(tlKind, tlFirst, -tlChunk); const before = tlSvg.getBoundingClientRect().height; const h = tlBuild(tlKind); box.scrollTop += h - before; changed = true; }
    if (box.scrollTop + box.clientHeight > box.scrollHeight - 300) { tlLast = stepDate(tlKind, tlLast, tlChunk); tlBuild(tlKind); changed = true; }
    return changed;
  }
  function mountTimeline(box) {
    if (!box) return;
    const kind = box.dataset.kind, center = periodOf(kind, parse(box.dataset.center)), m = tlMetrics(box, kind);
    // a rail of the same kind is carried over with its scroll position — but only while it reaches
    // the period we are moving to; a jump past its ends (today from another year, the orbit picker)
    // gets a fresh rail around the target instead of one whose ring is nowhere
    if (tlSvg && tlKind === kind && tlStep === m.step && tlItems.some(i => i.period === fmt(center))) { box.appendChild(tlSvg); if (tlScroll != null) box.scrollTop = tlScroll; if (tlHome) tlCentre(box); else tlSelect(); }
    else { // a resized window changes the step, and a rail built at the old one has to go
      tlKind = kind; tlStep = m.step; tlR = m.r; tlRing = m.ring; tlChunk = m.chunk;
      tlFirst = stepDate(kind, center, -tlChunk); tlLast = stepDate(kind, center, tlChunk);
      tlSvg = document.createElementNS('http://www.w3.org/2000/svg', 'svg'); tlSvg.setAttribute('width', 88); box.appendChild(tlSvg); tlBuild(kind);
      const it = tlItems.find(i => i.period === fmt(center)); box.scrollTop = it ? Math.max(0, it.y - box.clientHeight / 2) : 0;
    }
    if (!$('.tly', box)) { const y = document.createElement('div'); y.className = 'tly'; y.innerHTML = '<b></b>'; box.insertBefore(y, box.firstChild); }
    tlYearSync(box);
    tlHome = false;
    const nearest = e => { const top = tlSvg.getBoundingClientRect().top; const y = e.clientY - top; let best = null; for (const i of tlItems) if (!best || Math.abs(i.y - y) < Math.abs(best.y - y)) best = i; return best; };
    box.onmousemove = e => { const it = nearest(e); if (!it) return; const pre = tlSvg.querySelector('.pre'); pre.setAttribute('cy', it.y); svgShow(pre, true); tip.textContent = it.label; tip.hidden = false; tip.style.left = e.clientX + 'px'; tip.style.top = e.clientY + 'px'; };
    box.onmouseleave = () => { tip.hidden = true; const pre = tlSvg.querySelector('.pre'); if (pre) svgShow(pre, false); };
    box.onclick = e => { const it = nearest(e); if (it) { tip.hidden = true; if (state.view === 'week') goWeek(it.date); else if (state.view === 'month') { state.month = it.date.slice(0, 7); load(); } else go(null, it.date); } };
    box.onscroll = () => { tlYearSync(box); if (!box._busy) { box._busy = true; requestAnimationFrame(() => { tlExtend(box); tlYearSync(box); box._busy = false; }); } };
    tlExtend(box);
  }
  // ---- week stream: endless in both directions, the week in focus drives outline, title and rail ----
  // No prev/next buttons: the week changes by scrolling alone. Focus = the week covering the middle
  // of the free viewport, which is also the one taking up most of it. It is tracked per frame, so
  // the title, the rail's ring, the heat panel and the URL follow the scroll without reloading.
  const WK_CHUNK = 8, WK_MAX = 80, WK_SLACK = 1200; // weeks per fetch, weeks kept in the DOM, px of slack before the next fetch
  let wkOff = null;
  const isoYear = d => { const t = new Date(d.getFullYear(), d.getMonth(), d.getDate()); t.setDate(t.getDate() + 3 - ((t.getDay() + 6) % 7)); return t.getFullYear(); }; // the Thursday's year
  const scrollerOf = el => { for (let p = el.parentElement; p; p = p.parentElement) { const o = getComputedStyle(p).overflowY; if ((o === 'auto' || o === 'scroll') && p.scrollHeight > p.clientHeight + 1) return p; } return document.scrollingElement; };
  const centreY = () => { const t = $('.toolbar', root); return ((t ? t.getBoundingClientRect().bottom : 0) + innerHeight) / 2; };
  function goWeek(date) { // a rail or heat click scrolls there when the week is loaded, and reloads around it when it is not
    const stream = $('.stream', root), mon = localIso(weekStart(parse(date))), b = stream && $(`.week[data-week="${mon}"]`, stream);
    if (!b) return go(null, date);
    const box = scrollerOf(stream), r = b.getBoundingClientRect();
    box.scrollTo({ top: box.scrollTop + r.top - (centreY() - r.height / 2), behavior: reducedMotion() ? 'auto' : 'smooth' });
  }
  function mountWeeks(stream) {
    if (wkOff) { wkOff(); wkOff = null; } // the listeners sit on .main, which survives the fragment swap
    if (!stream || state.view !== 'week') return;
    const box = scrollerOf(stream), target = box === document.scrollingElement ? window : box;
    let focus = null, tick = 0, busy = false;
    const blocks = () => $$('.week', stream);
    const pick = () => { const y = centreY(); let best = null, bestD = Infinity;
      for (const w of blocks()) { const r = w.getBoundingClientRect(); if (r.top <= y && r.bottom >= y) return w; const d = Math.min(Math.abs(r.top - y), Math.abs(r.bottom - y)); if (d < bestD) { bestD = d; best = w; } }
      return best; };
    function setFocus(w) {
      if (!w || w === focus) return;
      if (focus) focus.classList.remove('focus');
      focus = w; w.classList.add('focus');
      const d = parse(w.dataset.week);
      state.week = w.dataset.week; // the week on screen; the picked day stays
      const h1 = $('#title', root); if (h1) { h1.textContent = tr('js-week', { w: isoWeek(d), y: isoYear(d) }); document.title = h1.textContent + ' · ' + document.title.split(' · ').slice(1).join(' · '); }
      history.replaceState(state, '', url(false).pathname + url(false).search);
      tlSelect(); heatSync(d);
    }
    function heatSync(d) { // the panel names its month, so it must not keep showing the one we scrolled away from
      const heat = $('.heat', root); if (!heat) return;
      const y = d.getFullYear(), m = d.getMonth(), key = `${y}-${pad(m + 1)}`;
      if (heat.dataset.month !== key) {
        const cells = []; const c = weekStart(new Date(y, m, 1));
        for (let i = 0; ; i++, c.setDate(c.getDate() + 1)) { if (i && i % 7 === 0 && (c.getMonth() !== m || c.getFullYear() !== y)) break; cells.push(new Date(c)); } // same week rule as render.rs month_weeks
        const dayOf = x => $(`.dayblock[data-date="${x}"]`, stream);
        if (cells.some(x => !dayOf(fmt(x)))) return; // a day of that month is not loaded: leave the panel alone rather than show a wrong count
        heat.dataset.month = key; const hm = $('.hm', heat); if (hm) hm.textContent = `${MON[m]} ${y}`;
        $('.grid', heat).innerHTML = Array.from({ length: cells.length / 7 }, (_, r) => { const w = cells.slice(r * 7, r * 7 + 7);
          return `<div class="hw" data-week="${fmt(w[0])}"><b>${isoWeek(w[0])}</b>` + w.map(x => { const n = Math.min(4, $$('li', dayOf(fmt(x))).filter(li => !li.classList.contains('none')).length);
            return `<i class="${x.getMonth() !== m ? 'o' : ''}${fmt(x) === today ? ' t' : ''}" style="--n:${n}" data-date="${fmt(x)}" title="${esc(dm(x))}"></i>`; }).join('') + '</div>'; }).join('');
        wireItems(heat);
      }
      $$('.hw', heat).forEach(r => r.classList.toggle('focus', r.dataset.week === state.week)); }
    async function extend(dir) {
      if (busy) return; const bs = blocks(); if (!bs.length) return;
      const edge = dir > 0 ? bs[bs.length - 1] : bs[0];
      const from = localIso(stepDate('weeks', parse(edge.dataset.week), dir > 0 ? 1 : -WK_CHUNK));
      busy = true;
      try {
        const u = new URL('/api/weeks', location.origin); u.searchParams.set('start', from); u.searchParams.set('count', WK_CHUNK); if (state.cals) u.searchParams.set('cals', state.cals);
        const r = await fetch(u, { headers: { Accept: 'text/html' } }); if (!r.ok) return;
        const tmp = document.createElement('div'); tmp.innerHTML = await r.text();
        const fresh = [...tmp.children].filter(w => !$(`.week[data-week="${w.dataset.week}"]`, stream));
        if (!fresh.length) return;
        // put a block that stays back where it was: measuring the displacement afterwards also absorbs
        // the browser's own scroll anchoring, which a scrollHeight delta would have added a second time
        const anchor = focus && focus.isConnected ? focus : bs[0], y0 = anchor.getBoundingClientRect().top;
        if (dir > 0) stream.append(...fresh); else stream.prepend(...fresh);
        fresh.forEach(wireItems); markPicked();
        const over = blocks().length - WK_MAX;
        if (over > 0) (dir > 0 ? blocks().slice(0, over) : blocks().slice(-over)).forEach(b => b.remove());
        box.scrollTop += anchor.getBoundingClientRect().top - y0;
      } catch (e) { console.error(e); } finally { busy = false; }
    }
    const onScroll = () => { if (tick) return; tick = requestAnimationFrame(() => { tick = 0;
      setFocus(pick()); const bs = blocks(); if (!bs.length) return;
      if (bs[0].getBoundingClientRect().top > -WK_SLACK) extend(-1);
      if (bs[bs.length - 1].getBoundingClientRect().bottom < innerHeight + WK_SLACK) extend(1); }); };
    target.addEventListener('scroll', onScroll, { passive: true }); addEventListener('resize', onScroll);
    wkOff = () => { target.removeEventListener('scroll', onScroll); removeEventListener('resize', onScroll); };
    const cal = $('.cal', root), first = (cal && $(`.week[data-week="${cal.dataset.focus}"]`, stream)) || blocks()[0];
    if (first) { const r = first.getBoundingClientRect(); box.scrollTop += r.top - (centreY() - r.height / 2); } // land with the week in focus centred
    markPicked(); setFocus(pick());
  }
  // ---- day ledger: endless in both directions. Picking a day in it must not move it — only a day
  // that arrives some other way (the today button, the orbit picker, the rail) is scrolled to the
  // middle. The node itself is carried across fragment swaps, so the loaded days survive a pick. ----
  const LED_CHUNK = 30, LED_MAX = 400, LED_SLACK = 600; // days per fetch, days kept, px of slack before the next fetch
  let ledNode = null, ledScroll = null, ledPin = false, ledOff = null;
  // a filter change (calendars, holidays) makes the carried ledger stale: the next swap takes the server's
  function dropCarried() { ledNode = null; ledScroll = null; }
  function mountLedger(box) {
    if (ledOff) { ledOff(); ledOff = null; }
    if (!box || state.view !== 'day') { ledNode = null; ledPin = false; return; }
    const keep = ledNode && ledNode !== box && $(`.lrow[data-date="${state.date}"]`, ledNode); // a day outside the loaded days starts from the page's own ledger
    if (keep) { box.replaceWith(ledNode); if (ledScroll != null) ledNode.scrollTop = ledScroll; } else ledNode = box;
    const led = ledNode, list = $('.lrows', led), head = $('.lhd', led), rows = () => $$('.lrow', led);
    $$('.lrow.sel', led).forEach(r => r.classList.remove('sel'));
    const sel = $(`.lrow[data-date="${state.date}"]`, led); if (sel) sel.classList.add('sel');
    if (sel && !(keep && ledPin)) led.scrollTop += sel.getBoundingClientRect().top - led.getBoundingClientRect().top - (led.clientHeight - sel.offsetHeight) / 2;
    ledPin = false;
    wireItems(led);
    const headSync = () => { if (!head) return; // the heading names the month with the most days in sight (row middle visible); a tie goes to the one covering more pixels
      const top = led.getBoundingClientRect().top + head.offsetHeight, bottom = Math.min(led.getBoundingClientRect().bottom, innerHeight), seen = new Map();
      for (const x of rows()) { const r = x.getBoundingClientRect(); if (r.bottom <= top) continue; if (r.top >= bottom) break;
        const m = x.dataset.date.slice(0, 7), s = seen.get(m) || { n: 0, px: 0 }, mid = (r.top + r.bottom) / 2; if (mid >= top && mid <= bottom) s.n++; s.px += Math.min(r.bottom, bottom) - Math.max(r.top, top); seen.set(m, s); }
      let best = null; for (const [m, s] of seen) if (!best || s.n > best[1].n || (s.n === best[1].n && s.px > best[1].px)) best = [m, s]; if (!best) return;
      const d = parse(best[0] + '-01'); head.textContent = `${MON[d.getMonth()]} ${d.getFullYear()}`; };
    headSync();
    if (led.scrollHeight <= led.clientHeight + 1) return; // narrow layout: the page scrolls, not the ledger, and an endless list would never stop growing
    let busy = false, tick = 0;
    async function extend(dir) {
      if (busy) return; const rs = rows(); if (!rs.length) return;
      const d0 = parse((dir > 0 ? rs[rs.length - 1] : rs[0]).dataset.date); d0.setDate(d0.getDate() + (dir > 0 ? 1 : -LED_CHUNK));
      busy = true;
      try {
        const u = new URL('/api/days', location.origin); u.searchParams.set('start', localIso(d0)); u.searchParams.set('count', LED_CHUNK); if (state.cals) u.searchParams.set('cals', state.cals);
        const r = await fetch(u, { headers: { Accept: 'text/html' } }); if (!r.ok) return;
        const tmp = document.createElement('div'); tmp.innerHTML = await r.text();
        const fresh = [...tmp.children].filter(x => !$(`.lrow[data-date="${x.dataset.date}"]`, led));
        if (!fresh.length) return;
        const top = led.getBoundingClientRect().top, anchor = rows().find(x => x.getBoundingClientRect().bottom > top) || rs[0], y0 = anchor.getBoundingClientRect().top; // a visible row is never the one that gets trimmed
        if (dir > 0) list.append(...fresh); else list.prepend(...fresh);
        wireItems(list); // the fetched nodes ARE the rows: wiring each one would search inside it and find nothing, and the untouched <a href> would reload the page
        const over = rows().length - LED_MAX;
        if (over > 0) (dir > 0 ? rows().slice(0, over) : rows().slice(-over)).forEach(x => x.remove());
        led.scrollTop += anchor.getBoundingClientRect().top - y0;
      } catch (e) { console.error(e); } finally { busy = false; }
    }
    const onScroll = () => { if (tick) return; tick = requestAnimationFrame(() => { tick = 0; headSync();
      const rs = rows(); if (!rs.length) return; const r = led.getBoundingClientRect();
      if (rs[0].getBoundingClientRect().bottom > r.top - LED_SLACK) extend(-1);
      if (rs[rs.length - 1].getBoundingClientRect().top < r.bottom + LED_SLACK) extend(1); }); };
    led.addEventListener('scroll', onScroll, { passive: true });
    ledOff = () => led.removeEventListener('scroll', onScroll);
    onScroll();
  }
  function wireItems(scope) { // everything a fetched week block or a rebuilt heat grid brings along; assignments only, so a second call is harmless
    $$('.heat i[data-date], .we[data-date]', scope).forEach(el => el.onclick = () => state.view === 'week' ? goWeek(el.dataset.date) : go('day', el.dataset.date));
    $$('.planner .c[data-date]', scope).forEach(el => el.onclick = () => { if (el.dataset.date !== state.date) pickDay(el.dataset.date); }); // the year planner picks the day; the view stays
    $$('.lrow[data-date]', scope).forEach(el => el.onclick = e => { if (e.target.classList.contains('add')) return; e.preventDefault(); ledPin = true; go('day', el.dataset.date); }); // picking in the ledger leaves it exactly where it is
    $$('.more[data-date]', scope).forEach(el => el.onclick = e => { e.stopPropagation(); go('day', el.closest('[data-date]').dataset.date); });
    $$('.v-month > .month > .day[data-date]', scope).forEach(el => el.onclick = e => { if (!e.defaultPrevented && !e.target.closest('a, button, .ev, .more') && el.dataset.date !== state.date) pickDay(el.dataset.date); });
    $$('.dayblock[data-date]', scope).forEach(el => el.onclick = e => { if (!e.defaultPrevented && !e.target.closest('a, button, .ev') && el.dataset.date !== state.date) pickDay(el.dataset.date); }); // the date label picks too, it no longer opens the day view
    markPicked();
    $$('[data-key]', scope).forEach(el => el.onclick = e => { e.stopPropagation(); edOpen(el.dataset.key, el.dataset.date); });
    // the day document's details sit inside the clickable entry, so each of them stops the click
    $$('.doc [data-copytext]', scope).forEach(b => b.onclick = e => { e.stopPropagation(); e.preventDefault(); // not data-copy: that one takes a selector (top of this file)
      navigator.clipboard.writeText(b.dataset.copytext).then(() => { b.classList.add('ok'); b.title = T.copied; setTimeout(() => b.classList.remove('ok'), 1200); }); });
    $$('.doc [data-map]', scope).forEach(b => b.onclick = e => { e.stopPropagation(); e.preventDefault(); const q = encodeURIComponent(b.dataset.q);
      window.open(b.dataset.map === 'osm' ? 'https://www.openstreetmap.org/search?query=' + q : 'https://www.google.com/maps/search/?api=1&query=' + q, '_blank', 'noopener'); });
    $$('.doc a.lk', scope).forEach(a => a.onclick = e => e.stopPropagation());
    $$('.add', scope).forEach(b => b.onclick = e => { e.stopPropagation(); e.preventDefault(); const d = b.closest('[data-date]'); edOpen(null, d ? d.dataset.date : state.date); });
  }
  function wire() {
    $$('[data-nav]', root).forEach(b => b.onclick = () => { if (b.dataset.nav === 'today') { tlHome = true; go(null, today); } else shift(b.dataset.nav === 'next' ? 1 : -1); });
    $$('.toolbar [data-view]', root).forEach(b => b.onclick = e => { e.preventDefault(); go(b.dataset.view, null); });
    const more = $('.more', root); if (more) { $('[data-more]', more).onclick = e => { e.stopPropagation(); more.classList.toggle('open'); }; }
    mountTimeline($('.tl', root));
    if (state.view === 'month') { const m = $('.month', root); if (m) m.addEventListener('wheel', monthWheel, { passive: false }); }
    wireItems(root); mountWeeks($('.stream', root)); mountLedger($('.ledger', root));
    const title = $('#title', root); if (title) title.onclick = openPicker;
  }
  document.addEventListener('click', e => { if (!e.target.closest('.more')) $$('.more.open').forEach(m => m.classList.remove('open')); });
  document.addEventListener('rustical:change', () => { clearTimeout(load._t); load._t = setTimeout(() => load(false), 800); });
  $('#fab').onclick = () => edOpen(null, state.date);

  // ---- orbit picker ----
  const pk = $('#picker');
  let pkYear = +pk.dataset.year, pkMonth = +pk.dataset.month, pkBase = +pk.dataset.year;
  const yn = $$('.ynode', pk);
  function py() { yn.forEach((g, i) => { const y = pkBase + (8 - i); g.dataset.year = y; g.querySelector('text').textContent = y; g.classList.toggle('cur', y === pkYear); }); }
  function pm() { $$('.mnode', pk).forEach(g => g.classList.toggle('cur', +g.dataset.month === pkMonth)); $('#pk-title').textContent = MON[pkMonth - 1] + ' ' + pkYear; grid(); }
  function grid() {
    const g = $('#pk-grid'); let h = DOW.map(d => `<div class="dow">${d}</div>`).join('');
    const first = new Date(pkYear, pkMonth - 1, 1); const start = new Date(first); start.setDate(1 - ((first.getDay() + 6) % 7));
    for (let i = 0; i < 42; i++) { const d = new Date(start); d.setDate(start.getDate() + i); if (i >= 35 && d.getMonth() !== pkMonth - 1) break; const s = `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`; h += `<div class="day${d.getMonth() !== pkMonth - 1 ? ' other' : ''}${s === today ? ' today' : ''}" data-date="${s}"><span class="n">${d.getDate()}</span></div>`; }
    g.innerHTML = h;
    $$('.day', g).forEach(d => d.onclick = () => { pk.hidden = true; go(state.view === 'week' || state.view === 'day' ? state.view : (['planner','weekends','vacation','countdown','numbers'].includes(state.view) ? state.view : 'month'), d.dataset.date); });
  }
  async function density() { try { const r = await fetch(`/api/density?year=${pkYear}${state.cals ? '&cals=' + encodeURIComponent(state.cals) : ''}`); const d = await r.json(); const mx = Math.max(1, ...d); $$('.mnode', pk).forEach(g => { const m = +g.dataset.month; if (!g.classList.contains('cur')) g.querySelector('circle').setAttribute('r', 14 + 10 * d[m - 1] / mx); }); } catch (_) {} }
  function openPicker() { const d = parse(state.date); pkYear = d.getFullYear(); pkMonth = d.getMonth() + 1; pkBase = pkYear; py(); pm(); density(); pk.hidden = false; }
  yn.forEach(g => g.onclick = () => { pkYear = +g.dataset.year; py(); pm(); density(); });
  $$('.mnode', pk).forEach(g => g.onclick = () => { pkMonth = +g.dataset.month; pm(); });
  $('.ybtn.prev', pk).onclick = () => { pkBase -= 7; py(); }; $('.ybtn.next', pk).onclick = () => { pkBase += 7; py(); };
  $('.ybtn.reset', pk).onclick = () => { const d = parse(today); pkYear = d.getFullYear(); pkMonth = d.getMonth() + 1; pkBase = pkYear; py(); pm(); density(); };
  $('#pk-close').onclick = () => pk.hidden = true; pk.addEventListener('click', e => { if (e.target === pk) pk.hidden = true; });


  wire();
  window.rusticalCal = { go, view: () => state.view, openEditor: edOpen };
  // the editor module asks the page where "now" is and how to redraw after a save
  window.rusticalHost = { kind: root.dataset.kind || 'calendar', date: () => state.date, cals: () => state.cals, reload: (push) => load(push) };
})();

// ---- editor: the card dialog. One implementation for THREE consumers — the calendar's events,
// the category rules and the tasks page; the markup comes from templates/editor.html, which the
// calendar and the tasks page both include. The page it runs on publishes `window.rusticalHost`
// (where "now" is and how to reload); everything else lives here. ----
(function () {
  if (!document.getElementById('edform')) return;
  const $ = (s, r = document) => r.querySelector(s), $$ = (s, r = document) => [...r.querySelectorAll(s)];
  const csrf = document.querySelector('meta[name=csrf]').content;
  const lang = document.body.dataset.lang || 'de';
  const MON = MONTHS;
  const DOW = WEEKDAYS;
  const T = trs('js-ed-', ['suggNone', 'saved', 'err', 'del', 'delRule', 'series', 'newEv', 'editEv', 'conflict', 'copied', 'delTask']);
  const iso = d => d.toISOString().slice(0, 10);
  const parse = s => { const [y, m, d] = s.split('-').map(Number); return new Date(y, m - 1, d); };
  const pad = n => String(n).padStart(2, '0');
  const fmt = d => d.getFullYear() + '-' + String(d.getMonth() + 1).padStart(2, '0') + '-' + String(d.getDate()).padStart(2, '0');
  const HOST = () => window.rusticalHost || {};
  let solo = null, soloSigs = {}; // one-card mode (see soloStart); declared early, cards() reads it from the start
  const root = document.getElementById('viewroot') || document.body;
  const today = root.dataset.today || fmt(new Date());
  // the host owns the current date and the calendar filter; without one the editor still works
  const state = { get date() { const h = HOST(); return (h.date && h.date()) || today; }, get cals() { const h = HOST(); return (h.cals && h.cals()) || ''; } };
  // `saved`: the key just written, so a page that keeps the editor open can show it again after the reload
  const load = (push, saved) => { const h = HOST(); if (h.reload) h.reload(push, saved); };
  const pk = document.getElementById('picker') || {};
  // which component this dialog edits: an event or a task (templates/editor.html, `editor_kind`)
  const ED_SCOPE = document.getElementById('ed').classList.contains('taskmode') ? 'task' : 'event';
  const TASK = ED_SCOPE === 'task';
  const API = TASK ? '/api/tasks' : '/api/events';
  // a card this consumer does not render: the stand-in swallows the wiring instead of throwing
  const NIL = document.createElement('div');
  const $x = (sel, r = document) => r.querySelector(sel) || NIL;

  // ---- editor: a card form like the contact editor — view mode behind the lock, cards shown only
  // when they hold data, "add card" menu in edit mode, icon buttons, dirty tracking ----
  const ed = $('#ed'), form = $('#edform'), f = form.elements, D = form.dataset;
  let current = null, openDate = null, baseline = '';
  function setAllDay(on) { if (!f.all_day) return; f.all_day.checked = on; $('#ed-allday').classList.toggle('on', on); f.from.disabled = on; f.to.disabled = on; $('#ed-times').hidden = on; }
  $x('#ed-allday').addEventListener('click', e => { e.preventDefault(); setAllDay(!f.all_day.checked); refresh(); });
  function syncDot() { const o = f.calendar.selectedOptions[0]; $('#ed-dot').style.background = f.color_cal.checked ? (o ? o.dataset.color : '') : f.color.value; $('#ed-sub').textContent = o ? o.textContent : ''; if (f.color_cal.checked && o) f.color.value = toHex(o.dataset.color); paintColor(); }
  const tl = $x('#ed-tl'); function setTl(on) { ed.classList.toggle('no-tl', !on); tl.setAttribute('aria-pressed', on ? 'true' : 'false'); localStorage.edtl = on ? '1' : '0'; }
  tl.onclick = () => setTl(ed.classList.contains('no-tl')); setTl(localStorage.edtl !== '0');
  // the dialog closes only through its buttons; unsaved edits ask first (a click on the backdrop does nothing)
  function close() { ed.hidden = true; tplClose(); $('#ed-confirm').hidden = true; if (ruleMode) ruleLeave(); soloEnd(); const h = HOST(); if (h.closed) h.closed(); }
  const dirty = () => form.classList.contains('mode-edit') && sig() !== baseline;
  function requestClose() { if (dirty()) { $('#ed-confirm').hidden = false; $('#ed-back').focus(); } else close(); }
  $('#ed-close').onclick = requestClose; $('#ed-back').onclick = () => { $('#ed-confirm').hidden = true; }; $('#ed-discard').onclick = close;
  addEventListener('keydown', e => { if (e.key === 'Escape') { if (!ed.hidden) requestClose(); pk.hidden = true; } });
  const fmtD = s => dfmt(parse(s.slice(0, 10)), { weekday: 'short', day: '2-digit', month: '2-digit', year: 'numeric' });
  const fmtDT = s => fmtD(s) + ' ' + s.slice(11, 16);
  // ---- categories and category rules (round 17): `CATS` is the ordered list mirrored into `cats_json`. A rule
  // (`RULES`, per user, see domain::catrules) puts values on the event as a template ("Vorlage": prefilled,
  // editable) or a lock ("Vorgabe": fixed here). Where and repeat carry one mode per card; when and status one
  // per setting; reminders and links one per entry. Order matters: per setting the first category wins, the
  // others are struck through. `rulesApply` sets and locks the fields; every card lists the templates and locks
  // that touch it in a footer (`.rfoot`, `rulesMark`). The same dialog edits a rule (rule mode): the cards look
  // the same, the footer chips switch each setting between "beliebig" (disabled), template and lock. Opened
  // from an event the event is snapshotted and restored afterwards. ----
  const CT = $('#ed-cats-card'), CTT = CT.dataset, ctRows = $('#ed-ct-rows'), ctIn = $('#ed-ct-in'), ctCloud = $('#ed-ct-cloud');
  const RULES = JSON.parse(($('#category-rules-data') || {}).textContent || '{}');
  let CATS = [], CT_IDX = null, ctPop = null, RFX = {}, RFXL = { alarms: [], links: [] }, RLOCK = new Set(), ruleMode = null, ruleBack = null, ruleReturn = null, RM = {};
  const CTF = CTT.tFields.split('|');
  const FLD = { summary: ['title', 11], all_day: ['when', 0], calendar: ['when', 1], color: ['when', 2], location: ['where', 3], rrule: ['repeat', 5], status: ['status', 6], transp: ['status', 7], class: ['status', 8], priority: ['status', 9] };
  const SCALARS = Object.keys(FLD), WHEN_KEYS = ['all_day', 'calendar', 'color'], STATUS_KEYS = ['status', 'transp', 'class', 'priority'], RULE_CARDS = ['when', 'where', 'alarm', 'repeat', 'status', 'links'];
  const ruleSetting = (r, fld) => { if (fld === 'summary') { const st = r.title; return st && st.value ? { mode: st.mode, value: st.value } : null; } if (fld === 'location') { const p = r.where; return p && p.mode !== 'off' && (p.location || p.geo) ? { mode: p.mode, value: p.location || '', geo: p.geo } : null; } if (fld === 'rrule') { const p = r.repeat; return p && p.mode !== 'off' && p.rrule ? { mode: p.mode, value: p.rrule } : null; } const st = (r[FLD[fld][0]] || {})[fld]; return st && st.value != null && st.value !== '' ? { mode: st.mode, value: st.value } : null; };
  const alarmKey = a => [a.action || 'DISPLAY', a.related_end ? 1 : 0, a.at || '', a.minutes ?? ''].join('|');
  const chips = () => CATS.slice();
  const ctMirror = () => { f.cats_json.value = CATS.length ? JSON.stringify(CATS) : ''; };
  const optText = (sel, v) => ([...sel.options].find(o => o.value === String(v)) || {}).textContent || String(v);
  const fxVal = (fld, v) => fld === 'all_day' ? esc(v ? CTT.tYes : CTT.tNo) : fld === 'calendar' ? esc(optText(f.calendar, v)) : fld === 'color' ? `<i class="ct-sw" style="background:${esc(v)}"></i>` : fld === 'rrule' ? esc(String(v).replace(/^FREQ=/, '').toLowerCase().slice(0, 20)) : STATUS_KEYS.includes(fld) ? esc(optText(f[fld], v)) : esc(String(v).slice(0, 24));
  const curVal = fld => fld === 'summary' ? f.summary.value.trim() : fld === 'all_day' ? !!(f.all_day && f.all_day.checked) : fld === 'calendar' ? f.calendar.value : fld === 'color' ? (colorOf() || '').toLowerCase() : fld === 'location' ? f.location.value.trim() : fld === 'rrule' ? (rrBuild() || '') : f[fld].value;
  const sameVal = (fld, v) => fld === 'all_day' ? !!(f.all_day && f.all_day.checked) === !!v : fld === 'color' ? (colorOf() || '').toLowerCase() === String(v).toLowerCase() : String(curVal(fld)) === String(v);
  // effective rules of the event's categories: per setting (scalars by field, reminders and links by key) a lock
  // beats a template, among equals the upper category wins; every loser is recorded as overridden by the winner
  function effective() { const fx = {}, over = {}, alarms = [], links = [], cand = {};
    const push = (key, c) => { (cand[key] ||= []).push(c); };
    CATS.forEach(c => { const r = RULES[c]; if (!r) return;
      SCALARS.forEach(fld => { const st = ruleSetting(r, fld); if (st) push(fld, { cat: c, ...st }); });
      ((r.alarm || {}).alarms || []).forEach(a => push('alarm:' + alarmKey(a), { cat: c, mode: a.mode, entry: a }));
      ((r.links || {}).links || []).forEach(l => { if (l.uri) push('link:' + l.uri, { cat: c, mode: l.mode, entry: l }); }); });
    Object.entries(cand).forEach(([key, list]) => { const win = list.find(x => x.mode === 'lock') || list[0]; list.forEach(x => { if (x !== win) (over[x.cat] ||= {})[key] = win.cat; }); if (key.startsWith('alarm:')) alarms.push(win); else if (key.startsWith('link:')) links.push(win); else fx[key] = win; });
    return { fx, over, alarms, links }; }
  // effect line of a category row: one item per setting, struck through when an upper category wins ("überstimmt");
  // deviation from a template is shown in the card footers, not here
  const ctFx = (c, over) => { const r = RULES[c]; if (!r) return `<div class="ct-l"><span class="ct-t full">${esc(CTT.tNorule)}</span></div>`; const out = [];
    const item = (key, mode, label) => { const by = over[c] && over[c][key]; out.push(`<div class="ct-l${mode === 'lock' ? ' lock' : ''}${by ? ' over' : ''}" title="${esc(mode === 'lock' ? CTT.tLock : CTT.tDefault)}"><svg class="ico"><use href="#${mode === 'lock' ? 'i-lock' : 'i-tag'}"/></svg><span class="ct-t">${label}</span>${by ? `<span class="ct-by">${esc(CTT.tOver.replace('{n}', by))}</span>` : ''}</div>`); }; /* one rule per line: icon | text, "überschrieben von" below the text */
    SCALARS.forEach(fld => { const st = ruleSetting(r, fld); if (st) item(fld, st.mode, `${esc(CTF[FLD[fld][1]])}: ${fxVal(fld, st.value)}`); });
    ((r.alarm || {}).alarms || []).forEach(a => item('alarm:' + alarmKey(a), a.mode, `${esc(CTF[4])}: ${esc(alarmText(a))}`));
    ((r.links || {}).links || []).forEach(l => item('link:' + l.uri, l.mode, `${esc(CTF[10])}: ${esc(l.label || lkHost(l.uri) || l.uri)}`));
    return out.join(''); };
  function ctRender() {
    ctMirror();
    const { over } = effective();
    ctRows.innerHTML = CATS.map((c, i) => `<div class="ct-row" data-i="${i}" draggable="true"><span class="ct-grip edits" title="${esc(CTT.tUp)} / ${esc(CTT.tDown)}">⋮⋮</span><span class="ct-head"><span class="chip">${esc(c)}</span><button class="add edits ct-rule" type="button" data-ct-rule="${esc(c)}"><svg class="ico"><use href="#i-settings"/></svg> ${esc(CTT.tRule)}</button></span><button class="ct-x edits" type="button" data-ct-rm="${i}">×</button><span class="ct-fx">${ctFx(c, over)}</span></div>`).join('');
    ctCloudRender();
  }
  const ctIndex = () => CT_IDX ? Promise.resolve(CT_IDX) : fetch('/api/categories?scope=' + ED_SCOPE).then(r => r.ok ? r.json() : []).then(d => (CT_IDX = d)).catch(() => []);
  const ctInvalidate = () => { CT_IDX = null; };
  async function ctCloudRender() { const idx = await ctIndex(); ctCloud.innerHTML = idx.filter(c => !CATS.includes(c.name)).slice(0, 24).map(c => `<button class="ct-tag" type="button" data-ct-add="${esc(c.name)}">${esc(c.name)}${c.count ? ` <small>${c.count}</small>` : ''}${c.rule ? ' <svg class="ico" style="width:10px;height:10px"><use href="#i-tag"/></svg>' : ''}</button>`).join(''); }
  function catsInit(list) { CATS = (list || []).map(c => String(c).trim()).filter(Boolean); ctRender(); }
  function catAdd(v) { v = String(v).replace(/,/g, '').trim(); if (!v || CATS.includes(v)) return; addCard('cats'); CATS.push(v); ctRender(); rulesApply('add'); refresh(); }
  const addChip = catAdd;
  const ctClosePop = () => { if (ctPop) { ctPop.remove(); ctPop = null; } };
  ctIn.addEventListener('input', async () => { const q = ctIn.value.replace(/,/g, '').trim().toLowerCase(); ctClosePop(); if (!q) return; const idx = await ctIndex(); if (ctIn.value.replace(/,/g, '').trim().toLowerCase() !== q) return; const hits = idx.filter(c => c.name.toLowerCase().includes(q) && !CATS.includes(c.name)).slice(0, 5); const exact = idx.some(c => c.name.toLowerCase() === q);
    ctPop = document.createElement('div'); ctPop.className = 'rrpop'; ctPop.innerHTML = hits.map(c => `<button type="button" class="ct-hit" data-v="${esc(c.name)}">${esc(c.name)}<small>${esc(CTT.tExisting)}${c.count ? ' · ' + c.count : ''}</small></button>`).join('') + (exact ? '' : `<button type="button" class="ct-hit new" data-v="${esc(ctIn.value.replace(/,/g, '').trim())}">${esc(CTT.tNew.replace('{n}', ctIn.value.replace(/,/g, '').trim()))}</button>`);
    document.body.appendChild(ctPop); const r = ctIn.getBoundingClientRect(), box = ctIn.closest('.box'), maxRight = (box ? box.getBoundingClientRect().right : innerWidth) - 8; ctPop.style.left = `${Math.max(8, Math.min(r.left, maxRight - ctPop.offsetWidth)) + scrollX}px`; ctPop.style.top = `${r.bottom + scrollY + 4}px`;
    ctPop.addEventListener('mousedown', e => e.preventDefault()); ctPop.addEventListener('click', e => { const b = e.target.closest('[data-v]'); if (b) { catAdd(b.dataset.v); ctIn.value = ''; ctClosePop(); } }); });
  ctIn.addEventListener('keydown', e => { if (e.key === 'Enter' || e.key === ',') { e.preventDefault(); const first = ctPop && $('.ct-hit:not(.new)', ctPop); if (first && e.key === 'Enter') catAdd(first.dataset.v); else catAdd(ctIn.value); ctIn.value = ''; ctClosePop(); } else if (e.key === 'Escape') ctClosePop(); }); /* Enter takes the first existing suggestion; a new name is created via the last entry or a comma */
  ctIn.addEventListener('blur', () => { setTimeout(ctClosePop, 150); if (ctIn.value.trim()) { catAdd(ctIn.value); ctIn.value = ''; } });
  CT.addEventListener('click', e => { const rm = e.target.closest('[data-ct-rm]'); if (rm) { CATS.splice(+rm.dataset.ctRm, 1); ctRender(); rulesApply('change'); refresh(); return; }
    const add = e.target.closest('[data-ct-add]'); if (add) { catAdd(add.dataset.ctAdd); return; }
    const rl = e.target.closest('[data-ct-rule]'); if (rl) { openRuleEditor(rl.dataset.ctRule, true); return; }
    const grip = e.target.closest('.ct-grip'); if (grip) { const i = +grip.closest('.ct-row').dataset.i; rrPop(grip, [{ v: 'up', label: CTT.tUp }, { v: 'down', label: CTT.tDown }], null, v => { const j = v === 'up' ? i - 1 : i + 1; if (j < 0 || j >= CATS.length) return; [CATS[i], CATS[j]] = [CATS[j], CATS[i]]; ctRender(); rulesApply('change'); refresh(); }); } });
  let ctDrag = null;
  ctRows.addEventListener('dragstart', e => { const row = e.target.closest('.ct-row'); if (!row) return; ctDrag = +row.dataset.i; row.classList.add('dragging'); e.dataTransfer.effectAllowed = 'move'; });
  ctRows.addEventListener('dragover', e => { const row = e.target.closest('.ct-row'); if (!row || ctDrag == null) return; e.preventDefault(); $$('.ct-row.over', ctRows).forEach(x => x.classList.remove('over')); row.classList.add('over'); });
  ctRows.addEventListener('drop', e => { const row = e.target.closest('.ct-row'); if (!row || ctDrag == null) return; e.preventDefault(); const to = +row.dataset.i; const [m] = CATS.splice(ctDrag, 1); CATS.splice(to, 0, m); ctDrag = null; ctRender(); rulesApply('change'); refresh(); });
  ctRows.addEventListener('dragend', () => { ctDrag = null; $$('.ct-row.dragging, .ct-row.over', ctRows).forEach(x => x.classList.remove('dragging', 'over')); });
  // ---- applying the rules to the fields ----
  const modeIcon = m => m === 'lock' ? 'i-lock' : m === 'default' ? 'i-tag' : 'i-dots';
  const modeText = m => m === 'lock' ? CTT.tLock : m === 'default' ? CTT.tDefault : CTT.tAny;
  // badge "Vorlage · Kat" / "Vorgabe · Kat"; `extra` appended with " · "; `changed` = the event deviates from a template → orange, " (geändert)"
  const ruleBadgeHtml = (e, extra, changed) => `<span class="rb${changed ? ' changed' : ''}"><span class="rbadge ${e.mode}"><svg class="ico"><use href="#${modeIcon(e.mode)}"/></svg>${esc(modeText(e.mode))} · ${esc(e.cat)}</span>${extra ? `<span class="rb-x">${esc(extra)}</span>` : ''}${changed ? `<span class="rb-x">(${esc(CTT.tChanged)})</span>` : ''}</span>`;
  const ruleBadge = (fld, cur) => { const e = RFX[fld]; if (!e) return ''; const same = cur === undefined || sameVal(fld, e.value); return ruleBadgeHtml(e, fld === 'calendar' && !form.classList.contains('new') ? CTT.tNewonly : '', !same && e.mode === 'default'); }; /* a lock shows no "adjusted": the forced value is visible in the field */
  // disabled looks for locked settings (event: locks; rule editor: settings on "beliebig")
  function applyLocks() {
    f.summary.readOnly = !ruleMode && RLOCK.has('summary'); f.summary.disabled = !!ruleMode && RLOCK.has('summary'); /* event: a locked title; rule editor: "beliebig" */
    if (f.all_day) f.all_day.disabled = RLOCK.has('all_day');
    f.calendar.disabled = RLOCK.has('calendar');
    f.color.disabled = f.color_cal.disabled = $('#ed-color-btn').disabled = RLOCK.has('color');
    $('.card[data-card="where"]', form).classList.toggle('locked-where', RLOCK.has('location'));
    rrLocked = RLOCK.has('rrule'); $x('#ed-repeat-card').classList.toggle('locked', rrLocked);
  }
  function rulesApply(reason) {
    if (ruleMode) { RFX = {}; RFXL = { alarms: [], links: [] }; RLOCK = new Set(SCALARS.filter(k => (RM[k] || 'off') === 'off')); ALARMS.forEach(a => { a._mode ||= 'default'; a._locked = a._mode === 'off'; }); LK.links.forEach(l => { l._mode ||= 'default'; l._locked = l._mode === 'off'; }); applyLocks(); rulesMark(); stRender(); alRender(); lkRender(); cards(); return false; }
    const { fx, alarms, links } = effective(); RFX = fx; RFXL = { alarms, links }; RLOCK = new Set(Object.keys(fx).filter(k => fx[k].mode === 'lock'));
    const isNew = form.classList.contains('new'), prefill = reason === 'add' || reason === 'new';
    let adjusted = false;
    const set = (fld, differs, apply) => { const e = fx[fld]; if (!e) return; if (e.mode === 'lock' ? differs() : (prefill && differs())) { apply(e.value); if (e.mode === 'lock' && reason === 'open') { adjusted = true; e.adjusted = true; } } };
    set('summary', () => fx.summary.mode === 'lock' ? f.summary.value.trim() !== String(fx.summary.value).trim() : !f.summary.value.trim(), v => { f.summary.value = v; });
    if (f.all_day) set('all_day', () => fx.all_day.mode === 'lock' ? f.all_day.checked !== !!fx.all_day.value : !f.all_day.checked && !!fx.all_day.value, v => setAllDay(!!v));
    if (isNew) set('calendar', () => f.calendar.value !== fx.calendar.value && [...f.calendar.options].some(o => o.value === fx.calendar.value), v => { f.calendar.value = v; syncDot(); }); else RLOCK.delete('calendar'); /* an existing event cannot move */
    set('color', () => fx.color.mode === 'lock' ? (colorOf() || '').toLowerCase() !== String(fx.color.value).toLowerCase() : !colorOf(), v => { setColor(v); syncDot(); });
    set('location', () => fx.location.mode === 'lock' ? f.location.value.trim() !== String(fx.location.value).trim() : !f.location.value.trim(), v => { f.location.value = v; const g = fx.location.geo; if (g) { f.lat.value = g[0]; f.lon.value = g[1]; } locInit(); });
    ALARMS.forEach(a => { delete a._rule; delete a._locked; delete a._mode; });
    alarms.forEach(e => { let a = ALARMS.find(x => alarmKey(x) === alarmKey(e.entry)); if (!a && (e.mode === 'lock' || prefill)) { const { mode, ...ra } = e.entry; a = newAlarm({ ...ra, attendees: [...(ra.attendees || [])] }); ALARMS.push(a); if (e.mode === 'lock' && reason === 'open') { adjusted = true; e.adjusted = true; } } if (a) { a._rule = e; a._locked = e.mode === 'lock'; } });
    LK.links.forEach(l => { delete l._rule; delete l._locked; delete l._mode; });
    links.forEach(e => { let l = LK.links.find(x => x.uri === e.entry.uri); if (!l && (e.mode === 'lock' || prefill)) { const { mode, ...rl } = e.entry; l = { ...rl, embedded: false, keep: false, features: rl.features || [], extra: rl.extra || [] }; LK.links.push(l); if (e.mode === 'lock' && reason === 'open') { adjusted = true; e.adjusted = true; } } if (l) { l._rule = e; l._locked = e.mode === 'lock'; } });
    if (fx.rrule) { const e = fx.rrule; const cur = rrBuild() || ''; if (e.mode === 'lock' ? cur !== e.value : (prefill && !cur)) { rrInit(e.value, null, [], []); if (e.mode === 'lock' && reason === 'open') { adjusted = true; e.adjusted = true; } } }
    STATUS_KEYS.filter(fld => f[fld]).forEach(fld => set(fld, () => fx[fld].mode === 'lock' ? f[fld].value !== String(fx[fld].value) : f[fld].value === '', v => { if (fld === 'priority') setSel(f.priority, v); else f[fld].value = String(v); }));
    applyLocks(); rulesMark(); stRender(); alRender(); lkRender(); cards(); /* cards that got rule entries must show */
    return adjusted;
  }
  // card footers: in an event every template/lock that touches the card (name · badge, "geändert" when the event
  // deviates from a template); in rule mode every setting of the card with its mode chip. Re-run on every change.
  function rulesMark() {
    const foot = {}; const add = (card, name, chip) => { (foot[card] ||= []).push(`<div class="rf-row"><span class="rf-name">${esc(name)}</span>${chip}</div>`); };
    if (ruleMode) {
      const chip = (key, mode) => `<button type="button" class="rbadge ${mode}" data-rmode="${esc(key)}" title="${esc(CTT.tRulemode)}"><svg class="ico"><use href="#${modeIcon(mode)}"/></svg>${esc(modeText(mode))}</button>`;
      add('title', CTF[11], chip('summary', RM.summary || 'off'));
      WHEN_KEYS.filter(k => k === 'color' || f[k]).forEach(k => add('when', CTF[FLD[k][1]], chip(k, RM[k] || 'off')));
      add('where', CTF[3], chip('location', RM.location || 'off'));
      ALARMS.forEach(a => add('alarm', alarmText(a), chip('alarm:' + a._id, a._mode || 'off')));
      add('repeat', CTF[5], chip('rrule', RM.rrule || 'off'));
      STATUS_KEYS.forEach(k => add('status', CTF[FLD[k][1]], chip(k, RM[k] || 'off')));
      LK.links.forEach((l, i) => { if (!l.embedded) add('links', lkTitle(l), chip('link:' + i, l._mode || 'off')); });
    } else {
      const fx = RFX, entry = (e, present) => ruleBadgeHtml(e, '', !present && e.mode === 'default');
      if (fx.summary) add('title', CTF[11], ruleBadge('summary', curVal('summary')));
      WHEN_KEYS.forEach(k => { if (fx[k]) add('when', CTF[FLD[k][1]], ruleBadge(k, curVal(k))); });
      if (fx.location) add('where', CTF[3], ruleBadge('location', f.location.value.trim()));
      RFXL.alarms.forEach(e => add('alarm', alarmText(e.entry), entry(e, ALARMS.some(x => alarmKey(x) === alarmKey(e.entry)))));
      if (fx.rrule) add('repeat', CTF[5], ruleBadge('rrule', rrBuild() || ''));
      STATUS_KEYS.forEach(k => { if (fx[k]) add('status', CTF[FLD[k][1]], ruleBadge(k, f[k].value)); });
      RFXL.links.forEach(e => add('links', e.entry.label || lkHost(e.entry.uri) || e.entry.uri, entry(e, LK.links.some(x => x.uri === e.entry.uri))));
    }
    ['title', ...RULE_CARDS].forEach(c => { $x(`.rfoot[data-rfoot="${c}"]`, form).innerHTML = (foot[c] || []).join(''); });
    if (!ruleMode) ctRender();
  }
  // ---- rule editor: the dialog in rule mode. The cards are the event cards; the footer chips carry the mode per
  // setting or entry (`RM`, `_mode`); "beliebig" disables the setting. `sig()` extends the dirty check by the modes. ----
  const RULE_NEW = { color: () => '#3478f6', status: () => 'CONFIRMED', transp: () => 'OPAQUE', class: () => 'PUBLIC', priority: () => 5 };
  function setRuleMode(key, v) {
    if (key.startsWith('alarm:')) { const a = ALARMS.find(x => String(x._id) === key.slice(6)); if (a) a._mode = v; }
    else if (key.startsWith('link:')) { const l = LK.links[+key.slice(5)]; if (l) l._mode = v; }
    else { RM[key] = v; if (v !== 'off') { if (key === 'color' && !colorOf()) { setColor(RULE_NEW.color()); syncDot(); } else if (STATUS_KEYS.includes(key) && f[key].value === '') { if (key === 'priority') setSel(f.priority, RULE_NEW.priority()); else f[key].value = RULE_NEW[key](); } } }
    rulesApply('change'); refresh();
  }
  form.addEventListener('click', e => { const b = e.target.closest('[data-rmode]'); if (!b || !ruleMode) return; e.stopPropagation(); const key = b.dataset.rmode; const cur = key.startsWith('alarm:') ? (ALARMS.find(x => String(x._id) === key.slice(6)) || {})._mode : key.startsWith('link:') ? (LK.links[+key.slice(5)] || {})._mode : RM[key];
    rrPop(b, ['off', 'default', 'lock'].map(m => ({ v: m, label: modeText(m) })), cur || 'off', v => setRuleMode(key, v)); });
  const eventBody = () => { const [principal, cal_id] = f.calendar.value.split('/'); const allDay = f.all_day.checked; const start = allDay ? f.date.value + 'T00:00:00' : f.date.value + 'T' + (f.from.value || '09:00') + ':00'; const endDate = f.end_date.value || f.date.value; const end = allDay ? nextDay(endDate) + 'T00:00:00' : endDate + 'T' + (f.to.value || f.from.value || '10:00') + ':00';
    return { principal, cal_id, summary: f.summary.value, location: f.location.value, description: f.description.value, start, end, all_day: allDay, rrule: rrBuild(), exdates: rrDates('exdates'), rdates: rrDates('rdates'), alarms: alarmsOf(), organizer: PEOPLE.organizer, attendees: PEOPLE.attendees, categories: chips(), status: f.status.value, transp: f.transp.value, class: f.class.value, priority: f.priority.value === '' ? null : +f.priority.value, color: colorOf(), geo: geoOf(), ...linksOf(), timezone: root.dataset.tz, etag: f.etag.value }; };
  const sig = () => serialize() + (ruleMode ? '|' + JSON.stringify(ruleOf()) : '');
  function openRuleEditor(name, stack) {
    if (stack && !ruleMode) ruleReturn = { detail: { ...eventBody(), object_id: f.key.value.split('/')[2] || '', uid: (current || {}).uid || '', rrule_until: RR.end.kind === 'until' ? RR.end.until : null, has_overrides: !!(current && current.has_overrides), related: LK.related }, baseline, isNew: form.classList.contains('new'), summaryPh: f.summary.placeholder, date: openDate };
    const r = RULES[name] || {}, today = state.date || new Date().toISOString().slice(0, 10), cal = ((f.calendar.options[0] || {}).value || '/').split('/');
    const st = k => ((r[FLD[k][0]] || {})[k]) || null, where = r.where || { mode: 'off' }, repeat = r.repeat || { mode: 'off' }, ralarms = (r.alarm || {}).alarms || [], rlinks = (r.links || {}).links || [];
    const calv = st('calendar') && [...f.calendar.options].some(o => o.value === st('calendar').value) ? st('calendar').value.split('/') : cal;
    ruleMode = name; form.classList.add('rulemode'); ed.classList.add('rulemode');
    RM = {}; WHEN_KEYS.concat(STATUS_KEYS).forEach(k => { RM[k] = st(k) ? st(k).mode : 'off'; }); RM.summary = r.title ? r.title.mode : 'off'; RM.location = where.mode || 'off'; RM.rrule = repeat.mode || 'off';
    // a rule is edited through the same cards, so it needs a detail of the shape this consumer fills
    if (TASK) {
      fill({ principal: calv[0], cal_id: calv[1], object_id: '', uid: '', summary: r.title ? r.title.value : '', description: '',
        location: where.mode !== 'off' ? where.location || '' : '', start: null, start_all_day: true, due: null, due_all_day: true,
        status: st('status') ? st('status').value : '', percent: null, completed_at: null,
        class: st('class') ? st('class').value : '', priority: st('priority') ? st('priority').value : null, categories: [],
        color: st('color') ? st('color').value : '', geo: where.mode !== 'off' ? where.geo || null : null,
        alarms: ralarms.map(({ mode, ...a }) => a), links: rlinks.map(({ mode, ...l }) => l), related: [], etag: '', has_rrule: false });
    } else
    fill({ principal: calv[0], cal_id: calv[1], object_id: '', uid: '', summary: r.title ? r.title.value : '', location: where.mode !== 'off' ? where.location || '' : '', description: '', start: today + 'T09:00:00', end: today + 'T10:00:00', all_day: st('all_day') ? !!st('all_day').value : false, rrule: repeat.mode !== 'off' ? repeat.rrule || null : null, rrule_until: null, exdates: [], rdates: [], organizer: null, attendees: [], alarms: ralarms.map(({ mode, ...a }) => a), categories: [], status: st('status') ? st('status').value : '', transp: st('transp') ? st('transp').value : '', class: st('class') ? st('class').value : '', priority: st('priority') ? st('priority').value : null, color: st('color') ? st('color').value : '', geo: where.mode !== 'off' ? where.geo || null : null, links: rlinks.map(({ mode, ...l }) => l), related: [], etag: '', has_overrides: false });
    ralarms.forEach((a, i) => { if (ALARMS[i]) ALARMS[i]._mode = a.mode; }); rlinks.forEach((l, i) => { if (LK.links[i]) LK.links[i]._mode = l.mode; });
    const has = { when: true, where: RM.location !== 'off', alarm: ALARMS.length > 0, repeat: RM.rrule !== 'off', status: STATUS_KEYS.some(k => RM[k] !== 'off'), links: LK.links.length > 0 };
    RULE_CARDS.forEach(n => { const c = $(`.card[data-card="${n}"]`, form); if (!c) return; if (has[n]) c.dataset.added = '1'; else delete c.dataset.added; });
    mode('edit'); rulesApply('open'); f.summary.placeholder = D.tTitle; f.summary.required = false; $('#ed-sub').textContent = D.tRuleTitle.replace('{name}', name); $x('#ed-hint').textContent = ''; /* the title field holds the title template; the rule name goes to the subtitle; no series hint */
    ed.hidden = false; baseline = sig(); updateButtons(); $('#ed-delete').disabled = !RULES[name];
  }
  function ruleOf() {
    const setting = (k, v) => (RM[k] || 'off') !== 'off' && v !== '' && v != null ? { mode: RM[k], value: v } : null;
    const alarms = alarmsOf().map((a, i) => ({ mode: ALARMS[i]._mode || 'default', ...a })).filter(a => a.mode !== 'off'), plain = LK.links.filter(l => !l.embedded), links = linksOf().links.filter(l => !l.embedded).map((l, i) => ({ mode: (plain[i] || {})._mode || 'default', ...l })).filter(l => l.mode !== 'off');
    return { title: setting('summary', f.summary.value.trim()), when: { all_day: f.all_day ? setting('all_day', f.all_day.checked) : null, calendar: setting('calendar', f.calendar.value), color: setting('color', colorOf()) }, where: { mode: RM.location || 'off', location: f.location.value.trim(), geo: geoOf() }, alarm: { alarms }, repeat: { mode: TASK ? 'off' : (RM.rrule || 'off'), rrule: TASK ? '' : (rrBuild() || '') }, status: Object.fromEntries(STATUS_KEYS.filter(k => f[k]).map(k => [k, setting(k, k === 'priority' ? (f.priority.value === '' ? null : +f.priority.value) : f[k].value)])), links: { links } }; }
  // leaving rule mode: back to the event it was opened from (rules re-applied by `fill`), or to the page it
  // was called from (`?back=`), or simply closed
  function ruleLeave() { if (!ruleMode) return false; ruleMode = null; RM = {}; form.classList.remove('rulemode'); ed.classList.remove('rulemode'); f.summary.readOnly = false; f.summary.disabled = false; f.summary.required = true; $$('.rfoot', form).forEach(el => { el.innerHTML = ''; });
    if (ruleReturn) { const r = ruleReturn; ruleReturn = null; fill(r.detail, r.date); if (r.isNew) { form.classList.add('new'); f.key.value = ''; f.etag.value = ''; f.summary.placeholder = r.summaryPh; const b = $('#ed-save'); b.title = D.tCreate; b.setAttribute('aria-label', b.title); } mode('edit'); rulesApply('open'); baseline = r.baseline; updateButtons(); ed.hidden = false; return true; }
    if (ruleBack) { const to = ruleBack; ruleBack = null; location.href = to; return true; }
    return false; }
  async function ruleSave() { const rule = ruleOf(), name = ruleMode; const r = await fetch('/api/category-rules/' + encodeURIComponent(name) + '?scope=' + ED_SCOPE, { method: 'PUT', headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf }, body: JSON.stringify(rule) }); if (!r.ok) { $('#ed-error').textContent = T.err + ' ' + r.status + ': ' + (await r.text()).slice(0, 200); return; } RULES[name] = await r.json(); ctInvalidate(); if (!ruleLeave()) close(); }
  async function ruleDelete() { const name = ruleMode; const r = await fetch('/api/category-rules/' + encodeURIComponent(name) + '?scope=' + ED_SCOPE, { method: 'DELETE', headers: { 'X-CSRF-Token': csrf } }); if (r.ok) { delete RULES[name]; ctInvalidate(); if (!ruleLeave()) close(); } else $('#ed-error').textContent = T.err + ' ' + r.status; }
  // ---- list rows from <template>: reminders, links, exceptions, extra dates ----
  const rows = list => $$(`[data-list="${list}"] .row`, form);
  const addRow = list => { const box = $(`[data-list="${list}"]`, form); box.appendChild($('template', box).content.cloneNode(true)); return box.lastElementChild; };
  const w = (row, k) => $(`[data-w=${k}]`, row);
  const setSel = (sel, v, unit = '') => { v = String(v); if (![...sel.options].some(o => o.value === v)) sel.add(new Option(v + unit, v)); sel.value = v; };
  // ---- reminders (design K): a model of alarms rendered as a timeline strip (markers snap to the
  // usual offsets), a bar of per-user preset chips (a click adds or removes the matching alarm) and
  // one expandable entry per alarm whose settings follow the chosen kind and trigger. The model is
  // mirrored into the hidden `alarms_json` input so dirty tracking and reset work like for any field. ----
  const AL = $('#ed-alarm-card'), AT = AL.dataset, alBox = $('[data-list="alarms"]', AL), tline = $('#ed-tline'), apre = $('#ed-apre');
  let ALARMS = [], alSeq = 0;
  const AL_ACTS = ['DISPLAY', 'AUDIO', 'EMAIL'], AL_ICO = { DISPLAY: '#i-monitor-dot', AUDIO: '#i-bell', EMAIL: '#i-mail' }, AL_PROX = ['ARRIVE', 'DEPART', 'CONNECT', 'DISCONNECT'];
  const SNAPS = (() => { try { const j = JSON.parse($('#alarm-snaps-data').textContent); if (Array.isArray(j.start) && j.start.length) return j; } catch (_) {} return { start: [2, 5, 15, 30, 60, 180, 480, 1440, 2880, 4320, 10080, 20160], end: [5, 10, 15, 30, 60] }; })(); // per-user snap points of the strip (settings)
  const SNAP_START = SNAPS.start, SNAP_END_ALL = SNAPS.end, AL_UNITS = [1, 60, 1440, 10080, 43200]; // a "month" is 30 days: iCalendar durations have no months
  const DEFAULT_PRESETS = [0, 10].map(m => ({ action: 'DISPLAY', minutes: m, related_end: false, repeat: 0, duration_minutes: 0, attendees: [], attach: '', proximity: '' }));
  let PRESETS = (() => { try { const p = JSON.parse($('#alarm-presets-data').textContent || 'null'); return Array.isArray(p) ? p : DEFAULT_PRESETS; } catch (_) { return DEFAULT_PRESETS; } })();
  const newAlarm = o => ({ idx: null, action: 'DISPLAY', minutes: 15, related_end: false, at: null, description: '', summary: '', repeat: 0, duration_minutes: 0, attendees: [], attach: '', proximity: '', acknowledged: null, ...o, _id: ++alSeq });
  // a preset is the setting without text, subject and acknowledgement; an absolute trigger cannot be one
  const presetSig = a => a.at ? null : JSON.stringify([!!a.related_end, a.minutes, a.action, a.repeat, a.duration_minutes, a.proximity || '', a.attach || '', [...(a.attendees || [])].sort()]);
  const fromPreset = p => newAlarm({ action: p.action, minutes: p.minutes, related_end: !!p.related_end, repeat: p.repeat || 0, duration_minutes: p.duration_minutes || 0, attendees: [...(p.attendees || [])], attach: p.attach || '', proximity: p.proximity || '' });
  const matchesPreset = a => !a.at && PRESETS.some(p => presetSig(p) === presetSig(a));
  async function savePresets() { try { const r = await fetch('/api/alarm-presets', { method: 'PUT', headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf }, body: JSON.stringify(PRESETS) }); if (!r.ok) throw new Error(r.status); } catch (e) { $('#ed-error').textContent = T.err + ': ' + e.message; } }
  const amount = m => { m = Math.abs(m); if (m && m % 10080 === 0) return m === 10080 ? AT.tWeek : AT.tWeeks.replace('{n}', m / 10080); if (m && m % 1440 === 0) return m === 1440 ? D.tDay : D.tDays.replace('{n}', m / 1440); if (m && m % 60 === 0) return m === 60 ? D.tHour : D.tHours.replace('{n}', m / 60); return D.tMin.replace('{n}', m); };
  const whenText = a => { if (a.at) return fmtDT(a.at); if (a.minutes === 0) return a.related_end ? AT.tAtEndTxt : AT.tAtStart; return amount(a.minutes) + ' ' + (a.minutes > 0 ? (a.related_end ? D.tBeforeEnd : D.tBefore) : (a.related_end ? AT.tAfterEnd : AT.tAfter)); };
  const proxText = a => a.proximity ? AT.tProxVals.split('|')[AL_PROX.indexOf(a.proximity)] || a.proximity : '';
  const alarmText = a => { let t = whenText(a); const acts = D.tActs.split('|'); if (a.action === 'AUDIO') t += ' · ' + acts[1]; if (a.action === 'EMAIL') t += ' · ' + acts[2]; if (a.repeat > 0) t += ' · ' + D.tRepeat.replace('{n}', a.repeat) + (a.duration_minutes ? ' ' + AT.tRepeatEvery + ' ' + amount(a.duration_minutes) : ''); if (a.proximity) t += ' · ' + proxText(a); if (a.action === 'EMAIL' && a.attendees.length) t += ' · ' + AT.tToTxt + ' ' + a.attendees.join(', '); if (a.description && a.description !== f.summary.value) t += ' – ' + a.description; return t; };
  const extrasOf = a => [a.repeat > 0 ? D.tRepeat.replace('{n}', a.repeat) + (a.duration_minutes ? ' ' + AT.tRepeatEvery + ' ' + amount(a.duration_minutes) : '') : '', proxText(a), a.action === 'EMAIL' ? (a.attendees.length ? AT.tToTxt + ' ' + a.attendees.join(', ') : AT.tToMissing) : '', a.description ? '„' + a.description + '“' : ''].filter(Boolean).join(' · ');
  const esc = s => String(s).replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]);
  const alIco = id => `<svg class="ico" aria-hidden="true"><use href="${id}"/></svg>`;
  const unitOf = m => { m = Math.abs(m); for (const u of [...AL_UNITS].reverse()) if (m && m % u === 0) return u; return 1; };
  const fmtNum = n => String(Math.round(n * 100) / 100).replace('.', ',');
  // the event on the strip: start and length in minutes from the "when" card
  const evStart = () => {
    if (TASK) { const [due] = getStamp('due'), [st] = getStamp('start'); return new Date(due || st || (today + 'T00:00:00')); }
    return new Date((f.date.value || today) + 'T' + (f.all_day.checked ? '00:00' : (f.from.value || '00:00')) + ':00');
  };
  const evLen = () => {
    if (TASK) { const [due] = getStamp('due'), [st] = getStamp('start'); return due && st ? Math.max(1, Math.round((new Date(due) - new Date(st)) / 60000)) : 1; }
    const s = evStart(); const e = f.all_day.checked ? new Date(nextDay(f.end_date.value || f.date.value || today) + 'T00:00:00') : new Date((f.end_date.value || f.date.value || today) + 'T' + (f.to.value || f.from.value || '00:00') + ':00'); return Math.max(1, Math.round((e - s) / 60000)); };
  const minutesBefore = a => a.at ? Math.round((evStart() - new Date(a.at)) / 60000) : a.related_end ? a.minutes - evLen() : a.minutes;
  // axis: every snap point owns a zone of the same width (the strip is ordinal, not to scale) — the
  // marker follows the pointer and the value changes at regular intervals; an alarm off the grid sits
  // between its neighbours
  const SPAN = SNAP_START[SNAP_START.length - 1];
  const alCands = () => { const len = evLen(), ends = SNAP_END_ALL.filter(m => m < len);
    const order = [...SNAP_START].reverse().map(m => ({ related_end: false, minutes: m })).concat([{ related_end: false, minutes: 0 }], [...ends].reverse().map(m => ({ related_end: true, minutes: m })), [{ related_end: true, minutes: 0 }]);
    const step = 1 / (order.length + 1); return order.map((c, i) => ({ ...c, x: step * (i + 1), mb: c.related_end ? c.minutes - len : c.minutes })); };
  const evEdges = () => { const c = alCands(); return [c.find(k => !k.related_end && k.minutes === 0).x, c[c.length - 1].x]; };
  const xa = a => { if (a._dragX != null) return a._dragX; const mb = minutesBefore(a), pts = alCands(); if (mb >= pts[0].mb) return pts[0].x; if (mb <= pts[pts.length - 1].mb) return pts[pts.length - 1].x; for (let i = 0; i < pts.length - 1; i++) { const p = pts[i], q = pts[i + 1]; if (mb <= p.mb && mb >= q.mb) return p.mb === q.mb ? p.x : p.x + (q.x - p.x) * (p.mb - mb) / (p.mb - q.mb); } return pts[0].x; };
  const snapX = f => alCands().reduce((b, c) => Math.abs(c.x - f) < Math.abs(b.x - f) ? c : b);
  function alTimeline() {
    if (!form.classList.contains('mode-edit') || AL.hidden) return;
    const color = $('#ed-dot').style.background || '';
    const [EV0, EV1] = evEdges(), cx = alCands();
    let h = `<div class="axis"></div><div class="evb" style="left:${EV0 * 100}%;width:${(EV1 - EV0) * 100}%;--evc:${color}"></div>`;
    h += [60, 1440, 10080].map(m => cx.find(c => !c.related_end && c.minutes === m)).filter(Boolean).map(c => `<span class="tick" style="left:${c.x * 100}%">${esc(amount(c.minutes))}</span>`).join('');
    tline.innerHTML = h;
    { let prev = null; for (const t of $$('.tick', tline).sort((a, b) => a.getBoundingClientRect().left - b.getBoundingClientRect().left)) { const r = t.getBoundingClientRect(); if (prev !== null && r.left < prev + 4) { t.remove(); continue; } prev = r.right; } } // left to right: a label that would run into its kept neighbour is dropped (narrow card)
    const groups = new Map();
    for (const a of ALARMS) { const k = Math.round(xa(a) * 1000); if (!groups.has(k)) groups.set(k, []); groups.get(k).push(a); }
    for (const [k, list] of groups) {
      const g = document.createElement('div'); g.className = 'mkg'; g.style.left = `${k / 10}%`;
      for (const a of list) {
        const mk = document.createElement('div'); mk.className = `mk${a._open || a._drag ? ' sel' : ''}${a.acknowledged ? ' ack' : ''}`; mk.title = alarmText(a); mk.innerHTML = `<span class="lab">${esc(whenText(a))}</span>${alIco(AL_ICO[a.action] || AL_ICO.DISPLAY)}`;
        // a click on the marker opens or closes the entry; a drag only moves the alarm and leaves the entries as they are
        mk.onpointerdown = e => { e.preventDefault(); if (a._locked) { ALARMS.forEach(x => x._open = x === a ? !x._open : false); alRender(); return; } const rect = tline.getBoundingClientRect(); let moved = false;
          const move = ev => { const fr = Math.min(1, Math.max(0, (ev.clientX - rect.left) / rect.width)); const c = snapX(fr); a._dragX = fr; moved = true; if (!(c.related_end === a.related_end && c.minutes === a.minutes && !a.at)) { a.related_end = c.related_end; a.minutes = c.minutes; a.at = null; } a._drag = true; alRender(); refresh(); };
          const up = () => { document.removeEventListener('pointermove', move); document.removeEventListener('pointerup', up); delete a._drag; delete a._dragX; if (!moved) ALARMS.forEach(x => x._open = x === a ? !x._open : false); alRender(); refresh(); };
          document.addEventListener('pointermove', move); document.addEventListener('pointerup', up); };
        g.appendChild(mk);
      }
      tline.appendChild(g);
    }
  }
  const presetLabel = p => { const acts = D.tActs.split('|'); let t = !p.related_end && p.minutes > 0 ? amount(p.minutes) : whenText({ ...p, at: null, attendees: p.attendees || [] }); if (p.action === 'AUDIO') t += ' · ' + acts[1]; if (p.action === 'EMAIL') t += ' · ' + acts[2]; if (p.repeat) t += ` · ${p.repeat}×`; if (p.proximity) t += ' · ' + proxText(p); return t; };
  function alPresets() {
    const order = [...PRESETS].sort((p, q) => (!!p.related_end) - (!!q.related_end) || q.minutes - p.minutes);
    apre.innerHTML = `<span class="lbl">${esc(AT.tPresets)}</span>` + order.map(p => { const sig = presetSig(p); return `<button type="button" data-sig="${esc(sig)}" aria-pressed="${ALARMS.some(a => presetSig(a) === sig)}">${p.action !== 'DISPLAY' ? alIco(AL_ICO[p.action]) : ''}${esc(presetLabel(p))}</button>`; }).join('');
  }
  apre.addEventListener('click', e => { const b = e.target.closest('button[data-sig]'); if (!b) return; const sig = b.dataset.sig; const ex = ALARMS.find(a => presetSig(a) === sig); if (ex) ALARMS = ALARMS.filter(a => a !== ex); else { const p = PRESETS.find(q => presetSig(q) === sig); if (p) ALARMS.push(fromPreset(p)); } alRender(); refresh(); });
  // the settings of one entry: the fields follow the kind (text | sound | subject, message, recipients)
  // and the trigger (number + unit for a relative one, date and time for an absolute one); "after …"
  // only shows when the stored alarm already fires after its anchor (a valid but foreign setting)
  function alDetails(a, row) {
    const rels = AT.tRels.split('|'), units = AT.tUnits.split('|'), acts = D.tActs.split('|');
    const relv = a.at ? 'at' : (a.minutes < 0 ? 'after-' : 'before-') + (a.related_end ? 'end' : 'start');
    const REL_ICO = { 'before-start': '#i-al-bs', 'before-end': '#i-al-be', 'after-start': '#i-al-as', 'after-end': '#i-al-ae', at: '#i-today' };
    const relOpts = [['before-start', rels[0]], ['before-end', rels[1]]].concat(a.minutes < 0 ? [['after-start', rels[2]], ['after-end', rels[3]]] : []).concat([['at', rels[4]]]);
    const u = unitOf(a.minutes), isPreset = matchesPreset(a);
    const d = $('.aldet', row);
    const relLabel = a.at ? rels[4] : a.minutes < 0 ? (a.related_end ? rels[3] : rels[2]) : (a.related_end ? rels[1] : rels[0]);
    const field = (label, inner) => `<div class="field"><span>${esc(label)}</span>${inner}</div>`;
    const seg = (name, opts, cur, icons) => `<div class="segs"><span class="seg" data-w="${name}">${opts.map(([v, l]) => `<button type="button" data-v="${v}" aria-pressed="${v === cur}" title="${esc(l)}" aria-label="${esc(l)}">${alIco(icons[v])}</button>`).join('')}</span></div>`;
    d.innerHTML = `${seg('rel', relOpts, relv, REL_ICO)}
      ${a.at ? field(relLabel, `<input type="datetime-local" data-w="at" value="${esc((a.at || '').slice(0, 16))}">`) : field(relLabel, `<div class="inl"><input type="text" inputmode="decimal" data-w="n" value="${fmtNum(Math.abs(a.minutes) / u)}"><select data-w="unit">${AL_UNITS.map((v, i) => `<option value="${v}" ${v === u ? 'selected' : ''}>${esc(units[i])}</option>`).join('')}</select></div>`)}
      ${field(AT.tRepeat, `<div class="inl"><select data-w="repeat">${[0, 1, 2, 3, 5, 10].map(n => `<option value="${n}" ${n === a.repeat ? 'selected' : ''}>${n ? n + '×' : esc(AT.tNever)}</option>`).join('')}</select>${a.repeat > 0 ? `<span class="muted small">${esc(AT.tEvery)}</span><select data-w="every">${[1, 5, 10, 15, 30, 60].map(n => `<option value="${n}" ${n === a.duration_minutes ? 'selected' : ''}>${esc(amount(n))}</option>`).join('')}</select>` : ''}</div>`)}
      ${seg('act', AL_ACTS.map((v, i) => [v, acts[i]]), a.action, AL_ICO)}
      ${a.action === 'DISPLAY' ? field(AT.tText, `<input type="text" data-w="text" value="${esc(a.description)}" placeholder="${esc(AT.tTextHint)}">`) : ''}
      ${a.action === 'AUDIO' ? field(AT.tSound, `<input type="url" data-w="attach" value="${esc(a.attach)}" placeholder="${esc(AT.tSoundHint)}">`) : ''}
      ${a.action === 'EMAIL' ? field(AT.tSubject, `<input type="text" data-w="subject" value="${esc(a.summary)}" placeholder="${esc(AT.tTextHint)}">`) + field(AT.tBody, `<input type="text" data-w="text" value="${esc(a.description)}" placeholder="${esc(AT.tTextHint)}">`) + field(AT.tTo, `<input type="text" data-w="to" value="${esc(a.attendees.join(', '))}" placeholder="${esc(AT.tToHint)}">`) : ''}
      <div class="foot"><button class="pset" type="button" data-w="pset" aria-pressed="${isPreset}" ${a.at ? 'disabled' : ''} title="${esc(a.at ? AT.tPresetAt : isPreset ? AT.tPresetRm : AT.tPresetAdd)}" aria-label="${esc(a.at ? AT.tPresetAt : isPreset ? AT.tPresetRm : AT.tPresetAdd)}">${alIco('#i-star-h')}</button><span>${esc(a.at ? AT.tPresetAt : AT.tPresetHint)}</span></div>`;
    const w = k => $(`[data-w=${k}]`, d);
    w('rel').onclick = e => { const b = e.target.closest('button'); if (!b) return; const v = b.dataset.v; if (v === 'at') { a.at = a.at || (TASK ? (getStamp('due')[0] || today + 'T09:00:00') : (f.date.value || today) + 'T' + (f.all_day.checked ? '09:00' : (f.from.value || '09:00')) + ':00'); } else { const [dir, anc] = v.split('-'); a.related_end = anc === 'end'; a.at = null; a.minutes = (Math.abs(a.minutes) || 15) * (dir === 'after' ? -1 : 1); } alRender(); refresh(); };
    if (w('n')) { const apply = () => { const n = parseFloat(w('n').value.replace(',', '.')); if (isNaN(n) || n < 0) { w('n').classList.add('err'); return; } a.minutes = Math.sign(a.minutes || 1) * Math.round(n * +w('unit').value); alRender(); refresh(); }; w('n').onchange = apply; w('unit').onchange = apply; w('n').onkeydown = e => { if (e.key === 'Enter') { e.preventDefault(); apply(); } }; }
    if (w('at')) w('at').onchange = () => { if (w('at').value) { a.at = w('at').value + ':00'; alRender(); refresh(); } };
    w('act').onclick = e => { const b = e.target.closest('button'); if (b) { a.action = b.dataset.v; alRender(); refresh(); } };
    if (w('text')) w('text').oninput = () => { a.description = w('text').value; alSync(row, a); };
    if (w('subject')) w('subject').oninput = () => { a.summary = w('subject').value; alSync(row, a); };
    if (w('to')) w('to').onchange = () => { a.attendees = w('to').value.split(/[,;\s]+/).filter(Boolean); alRender(); refresh(); };
    if (w('attach')) w('attach').onchange = () => { a.attach = w('attach').value.trim(); alRender(); refresh(); };
    w('repeat').onchange = () => { a.repeat = +w('repeat').value; a.duration_minutes = a.repeat ? (a.duration_minutes || 10) : 0; alRender(); refresh(); };
    if (w('every')) w('every').onchange = () => { a.duration_minutes = +w('every').value; alRender(); refresh(); };
    w('pset').onclick = () => { if (a.at) return; if (matchesPreset(a)) PRESETS = PRESETS.filter(p => presetSig(p) !== presetSig(a)); else { const { action, minutes, related_end, repeat, duration_minutes, attendees, attach, proximity } = a; PRESETS.push({ action, minutes, related_end, repeat, duration_minutes, attendees: [...attendees], attach, proximity }); } savePresets(); alRender(); };
  }
  // text edits update the row's summary and the mirror without rebuilding the entry (the field keeps its focus)
  const alMirror = () => { f.alarms_json.value = JSON.stringify(ALARMS.map(({ _id, _open, _drag, _dragX, ...a }) => a)); };
  function alSync(row, a) { $('.txt', row).textContent = alarmText(a); $('.extra', row).textContent = extrasOf(a); alMirror(); refresh(); }
  let alBusy = false;
  function alRender() {
    if (alBusy) return; alBusy = true; // a field inside a row commits on blur when its row is rebuilt; that nested render is skipped, the model is current anyway
    try { alRenderNow(); } finally { alBusy = false; }
  }
  function alRenderNow() {
    if (alBox.contains(document.activeElement)) document.activeElement.blur();
    $$('.row', alBox).forEach(r => r.remove());
    for (const a of ALARMS) {
      const r = addRow('alarms'); r.classList.toggle('open', !!a._open); r._alarm = a;
      const tx = $('.txt', r); tx.textContent = alarmText(a); tx.classList.toggle('ack', !!a.acknowledged); $('.sum use', r).setAttribute('href', AL_ICO[a.action] || AL_ICO.DISPLAY); const t = $('.sum .t', r); t.textContent = whenText(a); t.classList.toggle('ack', !!a.acknowledged); $('.extra', r).textContent = extrasOf(a); r.classList.toggle('locked', !!a._locked); if (a._locked && !ruleMode) $$('[data-rm]', r).forEach(x => x.hidden = true); /* a rule entry on "beliebig" is greyed but removable */
      $('.pdot', r).classList.toggle('on', matchesPreset(a)); $('.act', r).setAttribute('aria-pressed', a._open ? 'true' : 'false');
      const d = $('.aldet', r); d.hidden = !a._open; if (a._open) alDetails(a, r);
    }
    $('#ed-alarm-n').textContent = ALARMS.length ? `(${ALARMS.length})` : '';
    alPresets(); alTimeline(); alMirror();
  }
  alBox.addEventListener('click', e => {
    const b = e.target.closest('[data-al-open], [data-rm]'); if (!b) return; e.stopPropagation(); const row = b.closest('.row'), a = row && row._alarm; if (!a) return;
    if (b.hasAttribute('data-rm')) ALARMS = ALARMS.filter(x => x !== a); else ALARMS.forEach(x => x._open = x === a ? !x._open : false);
    alRender(); refresh();
  });
  function alarmsInit(list) { ALARMS = list.map(a => newAlarm({ ...a, attendees: [...(a.attendees || [])] })); alRender(); }
  const alarmAdd = () => { const a = newAlarm(); if (ruleMode) a._mode = 'default'; ALARMS.push(a); alRender(); return $$('.row', alBox).pop(); }; // the new entry stays collapsed; the row header, the dots or the marker open it
  const alarmsOf = () => ALARMS.map(({ _id, _open, _drag, _dragX, _rule, _locked, _mode, ...a }) => ({ ...a, description: a.description.trim(), summary: a.summary.trim(), attach: a.attach.trim() }));
  // people: the organizer is the row with the ORGANIZER role
  // ---- status card (design K): the four hidden selects stay the model (dirty tracking, reset, submit work as
  // before); the tiles are their face. Each select carries its label and one icon per option (`data-icons`),
  // so the tile's icon follows the value; a priority outside 1/5/9 (Outlook writes 2) maps by the RFC bands. ----
  const SC = $x('#ed-status-card'), SSEL = ['status', 'transp', 'class', 'priority'].map(n => f[n]).filter(Boolean);
  const stIcon = sel => { const ic = sel.dataset.icons.split('|'); if (sel.name === 'priority' && sel.value !== '') { const n = +sel.value; return n === 0 ? ic[0] : n <= 4 ? ic[1] : n === 5 ? ic[2] : ic[3]; } return ic[Math.max(0, sel.selectedIndex)] || ic[0]; };
  const stText = sel => sel.selectedOptions[0] ? sel.selectedOptions[0].textContent : sel.value;
  function stRender() {
    $$('.stiles', form).forEach(box => { const sels = $$('select.dat', box.closest('.card')); box.innerHTML = sels.map(sel => `<button class="stile${sel.value === '' ? ' off' : ''}${RLOCK.has(sel.name) ? ' locked' : ''}" type="button" data-sel="${sel.name}" title="${esc(sel.dataset.label)}"><svg class="ico" aria-hidden="true"><use href="#${stIcon(sel)}"/></svg><small>${esc(sel.dataset.label)}</small><b>${esc(stText(sel))}</b></button>`).join('') });
    if (TASK) return;
    const t = SC.dataset, parts = [];
    if (f.status.value) parts.push(stText(f.status));
    if (f.transp.value) parts.push(f.transp.value === 'OPAQUE' ? t.tViewOpaque : t.tViewTransparent);
    if (f.class.value) parts.push(stText(f.class));
    if (f.priority.value) parts.push(t.tViewPrio.replace('{n}', stText(f.priority)));
    $('#ed-status-text').textContent = parts.join(' · ');
  }
  form.addEventListener('click', e => { const b = e.target.closest('.stile'); if (!b || !form.contains(b)) return; if (!b || b.classList.contains('locked')) return; const sel = f[b.dataset.sel];
    rrPop(b, [...sel.options].map(o => ({ v: o.value, label: o.textContent })), sel.value, v => { sel.value = v; stRender(); refresh(); }, `<div class="q">${esc(sel.dataset.label)}</div>`); b.setAttribute('aria-expanded', 'true'); });
  // ---- note (round 17): the textarea stays plain text (DESCRIPTION). Suggestions while typing come from a
  // lazily loaded index (contacts, categories, events, tasks) even without a prefix; picking one inserts
  // @Name / #Category / !Title. Detected persons, links and categories are offered as chips that create the
  // entry in the matching card. The view mode renders links, mentions and "- " lists. ----
  const NC = $('#ed-note-card'), NT = NC.dataset, ntView = $('#ed-note-view'), ntFound = $('#ed-note-found');
  let NTX = null, ntLoading = null, ntPop = null, ntTimer = 0;
  const ntIndex = () => NTX ? Promise.resolve(NTX) : (ntLoading ||= fetch('/api/mentions').then(r => r.ok ? r.json() : { contacts: [], categories: [], events: [], tasks: [] }).then(d => (NTX = d)).catch(() => { ntLoading = null; return { contacts: [], categories: [], events: [], tasks: [] }; }));
  const ntInvalidate = () => { NTX = null; ntLoading = null; };
  const RE_URL = /\bhttps?:\/\/[^\s<>"')\]]+/gi, RE_MAIL = /(?<![\w.+-])[\w.+-]+@[\w-]+(?:\.[\w-]+)+\b/g, RE_TEL = /(?<![\w@.+-])\+?\d[\d ()\/-]{6,}\d(?![\w@])/g, RE_MENTION = /(?<![\w.])([@#!])(\p{L}[\p{L}\p{N}.-]*(?: \p{Lu}[\p{L}\p{N}.-]*){0,2})/gu;
  const ntWordAtCaret = () => { const t = f.description, v = t.value.slice(0, t.selectionStart); const m = /(^|\s)([@#!]?)([^\s@#!]{1,60})$/.exec(v); return m ? { prefix: m[2], word: m[3], start: v.length - m[2].length - m[3].length } : null; };
  const ntClosePop = () => { if (ntPop) { ntPop.remove(); ntPop = null; } };
  async function ntSuggest() {
    const w = ntWordAtCaret();
    if (!w || (w.prefix ? w.word.length < 1 : w.word.length < 3)) { ntClosePop(); return; }
    const ix = await ntIndex(), q = w.word.toLowerCase(), has = s => s.toLowerCase().includes(q);
    const hits = [];
    if (!w.prefix || w.prefix === '@') ix.contacts.filter(c => has(c.name)).slice(0, 4).forEach(c => hits.push({ p: '@', v: c.name, sub: NT.tContact + (c.email ? ' · ' + c.email : '') }));
    if (!w.prefix || w.prefix === '#') ix.categories.filter(c => has(c.name)).slice(0, 3).forEach(c => hits.push({ p: '#', v: c.name, sub: NT.tCat + ' · ' + c.count }));
    if (!w.prefix || w.prefix === '!') { ix.events.filter(e => has(e.summary)).slice(0, 3).forEach(e => hits.push({ p: '!', v: e.summary, sub: NT.tEvent + (e.date ? ' · ' + fmtD(e.date) : '') })); ix.tasks.filter(e => has(e.summary)).slice(0, 2).forEach(e => hits.push({ p: '!', v: e.summary, sub: NT.tTask })); }
    ntClosePop();
    if (!hits.length || ntWordAtCaret()?.word !== w.word) return;
    ntPop = document.createElement('div'); ntPop.className = 'rrpop';
    ntPop.innerHTML = hits.slice(0, 7).map((h, i) => `<button type="button" class="lk-hit" data-h="${i}"><span class="nt-pre">${h.p}</span><span>${esc(h.v)}</span><small>${esc(h.sub)}</small></button>`).join('');
    document.body.appendChild(ntPop);
    const r = f.description.getBoundingClientRect(), box = f.description.closest('.box'), maxRight = (box ? box.getBoundingClientRect().right : innerWidth) - 8;
    ntPop.style.left = `${Math.max(8, Math.min(r.left, maxRight - ntPop.offsetWidth)) + scrollX}px`; ntPop.style.top = `${r.bottom + scrollY + 4}px`;
    ntPop.addEventListener('mousedown', e => e.preventDefault()); // keep the textarea focused
    ntPop.addEventListener('click', e => { const b = e.target.closest('.lk-hit'); if (b) ntPick(hits[+b.dataset.h]); });
    ntPop._pick = () => ntPick(hits[0]);
  }
  function ntPick(h) { const w = ntWordAtCaret(); if (!w) return; const t = f.description, v = t.value, ins = h.p + h.v + ' '; t.value = v.slice(0, w.start) + ins + v.slice(t.selectionStart); const c = w.start + ins.length; t.setSelectionRange(c, c); ntClosePop(); ntChanged(); }
  f.description.addEventListener('keydown', e => { if (ntPop && (e.key === 'Tab' || e.key === 'Enter') && e.target === f.description) { e.preventDefault(); ntPop._pick(); } else if (e.key === 'Escape' && ntPop) { e.stopPropagation(); ntClosePop(); } });
  f.description.addEventListener('blur', () => setTimeout(ntClosePop, 150));
  f.description.addEventListener('input', () => { ntSuggest(); clearTimeout(ntTimer); ntTimer = setTimeout(ntDetect, 300); });
  const ntChanged = () => { grow(); refresh(); ntDetect(); };
  // detection: what the note mentions that the other cards do not have yet
  async function ntDetect() {
    const text = f.description.value; ntFound.innerHTML = '';
    if (!text.trim() || !form.classList.contains('mode-edit')) return;
    const ix = await ntIndex(); if (f.description.value !== text) return;
    const chips = [];
    const present = new Set(peAll().map(p => p.email.toLowerCase()));
    const mentioned = new Set([...text.matchAll(RE_MENTION)].filter(m => m[1] === '@').map(m => m[2].replace(/[.,;:!?]+$/, '').toLowerCase()));
    ix.contacts.filter(c => c.email && !present.has(c.email.toLowerCase())).filter(c => mentioned.has(c.name.toLowerCase()) || (c.name.length >= 5 && new RegExp('(?<![\\p{L}])' + c.name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&') + '(?![\\p{L}])', 'iu').test(text))).slice(0, 4).forEach(c => chips.push({ k: 'person', icon: 'i-users', text: c.name, sub: NT.tPeople, data: c }));
    [...new Set((text.match(RE_URL) || []).map(u => u.replace(/[.,;:!?]+$/, '')))].filter(u => !LK.links.some(l => l.uri === u)).slice(0, 4).forEach(u => chips.push({ k: 'link', icon: 'i-link', text: u.replace(/^https?:\/\/(www\.)?/i, ''), sub: NT.tLinks, data: u }));
    const have = new Set(chips2().map(c => c.toLowerCase()));
    const tags = new Set([...text.matchAll(RE_MENTION)].filter(m => m[1] === '#').map(m => m[2].replace(/[.,;:!?]+$/, '')));
    ix.categories.filter(c => c.name.length >= 4 && new RegExp('(?<![\\p{L}#])' + c.name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&') + '(?![\\p{L}])', 'iu').test(text)).forEach(c => tags.add(c.name));
    [...tags].filter(t => !have.has(t.toLowerCase())).slice(0, 4).forEach(t => chips.push({ k: 'cat', icon: 'i-tag', text: t, sub: NT.tCats, data: t }));
    [...new Set(text.match(RE_TEL) || [])].slice(0, 2).forEach(t => chips.push({ k: 'tel', icon: 'i-phone', text: t.trim(), sub: NT.tCall, data: 'tel:' + t.replace(/[^\d+]/g, ''), ghost: true }));
    [...new Set(text.match(RE_MAIL) || [])].filter(m => !present.has(m.toLowerCase())).slice(0, 2).forEach(m => chips.push({ k: 'mail', icon: 'i-mail', text: m, sub: NT.tMail, data: 'mailto:' + m, ghost: true }));
    ntFound.innerHTML = chips.map((c, i) => `<button class="pillbtn${c.ghost ? ' ghost' : ''}" type="button" data-nt="${i}"><svg class="ico"><use href="#${c.icon}"/></svg><span>${esc(c.text)}</span><small>${esc(c.sub)}</small></button>`).join('');
    ntFound._chips = chips;
  }
  const chips2 = () => chips(); // categories currently on the event (the chips widget)
  ntFound.addEventListener('click', e => { const b = e.target.closest('[data-nt]'); if (!b) return; const c = ntFound._chips[+b.dataset.nt];
    if (c.k === 'person') { addCard('people'); PEOPLE.attendees.push({ name: c.data.name, email: c.data.email, role: 'REQ-PARTICIPANT', status: '', cutype: '', extra: [], contacts: [{ key: c.data.key, name: c.data.name, sub: c.data.email, initials: c.data.name.split(/\s+/).map(w => w[0]).slice(0, 2).join('').toUpperCase(), color_index: 1 }] }); peopleRender(); }
    else if (c.k === 'link') { addCard('links'); lkAddUri(c.data); }
    else if (c.k === 'cat') { catAdd(c.data); }
    else { location.href = c.data; return; }
    refresh(); ntDetect(); });
  // view mode: links, phone numbers, e-mail addresses and @ # ! mentions become clickable; "- " lines are bullets
  function ntRender() {
    const t = f.description.value;
    let h = esc(t).replace(RE_URL, u => `<a href="${u}" target="_blank" rel="noopener">${u}</a>`).replace(RE_MAIL, m => `<a href="mailto:${m}">${m}</a>`).replace(RE_TEL, m => `<a href="tel:${m.replace(/[^\d+]/g, '')}">${m}</a>`);
    h = h.replace(RE_MENTION, (_, p, v) => { const name = v.replace(/[.,;:!?]+$/, ''), rest = v.slice(name.length); return `<span class="nt-m" data-k="${p}" data-v="${esc(name)}">${esc(name)}</span>${rest}`; }).replace(/^- (.*)$/gm, '• $1');
    ntView.innerHTML = h;
  }
  const ntSearch = v => { const q = $('#q'); if (q) { q.value = v; q.focus(); q.dispatchEvent(new Event('input', { bubbles: true })); } };
  ntView.addEventListener('click', async e => { const m = e.target.closest('.nt-m'); if (!m) return; const k = m.dataset.k, v = m.dataset.v.toLowerCase();
    if (k === '#') { ntSearch(m.dataset.v); return; }
    const ix = await ntIndex();
    if (k === '@') { const c = ix.contacts.find(x => x.name.toLowerCase() === v); location.href = c ? '/contacts#' + c.key : '/contacts'; }
    else { const ev = ix.events.find(x => x.summary.toLowerCase() === v) || ix.events.find(x => x.summary.toLowerCase().startsWith(v)); const tk = !ev && (ix.tasks.find(x => x.summary.toLowerCase() === v) || ix.tasks.find(x => x.summary.toLowerCase().startsWith(v))); if (ev) openEditor(ev.key); else if (tk) location.href = '/tasks#' + tk.key; else ntSearch(m.dataset.v); } });
  // ---- people (design C, display only): ORGANIZER/ATTENDEE are shown as chips whose ring colour is the
  // reply, a click opens a detail card with role, reply, delegation, address and the actions. Nothing is
  // editable: RustiCal does no scheduling, so the list is a record, not an invitation tool; the persons are
  // sent back unchanged on save (with every preserved parameter). `contacts` per person come from the
  // server (exact e-mail, else similar name); one hit jumps to the address book, several open a chooser. ----
  const PC = $x('#ed-people-card'), PT = PC.dataset; let PEOPLE = { organizer: null, attendees: [] };
  const statusText = p => { const s = D.tStatus.split('|'); return { 'NEEDS-ACTION': s[0], ACCEPTED: s[1], DECLINED: s[2], TENTATIVE: s[3], DELEGATED: s[4] }[p.status] || ''; };
  const peStatus = p => ({ ACCEPTED: 'acc', DECLINED: 'dec', TENTATIVE: 'ten', DELEGATED: 'del' })[p.status] || 'nee';
  const peRole = p => { const r = PT.tRoles.split('|'); return p.role === 'ORGANIZER' ? r[0] : { CHAIR: r[1], 'REQ-PARTICIPANT': r[2], 'OPT-PARTICIPANT': r[3], 'NON-PARTICIPANT': r[4] }[p.role] || r[2]; };
  const peType = p => { const t = PT.tTypes.split('|'); return { GROUP: t[0], ROOM: t[1], RESOURCE: t[2] }[p.cutype] || ''; };
  const peAll = () => [...(PEOPLE.organizer ? [{ ...PEOPLE.organizer, role: 'ORGANIZER' }] : []), ...PEOPLE.attendees];
  const peName = p => p.name || p.email;
  const peShort = uri => { const e = uri.replace(/^mailto:/i, ''); const q = peAll().find(x => x.email === e); return q ? peName(q).split(' ')[0] : e; };
  const peDeleg = p => { const f = p.extra.find(x => x[0] === 'DELEGATED-FROM'), t = p.extra.find(x => x[0] === 'DELEGATED-TO'); return f ? PT.tFrom.replace('{n}', peShort(f[1])) : t ? PT.tTo.replace('{n}', peShort(t[1])) : ''; };
  const peAv = p => { const ic = p.cutype === 'GROUP' ? 'i-users' : p.cutype === 'ROOM' ? 'i-room' : p.cutype === 'RESOURCE' ? 'i-res' : null; const ini = peName(p).split(/\s+/).filter(Boolean).map(w => w[0]).slice(0, 2).join('').toUpperCase() || '?'; const c = p.contacts && p.contacts.length === 1 ? p.contacts[0] : null;
    return ic ? `<span class="pav none" title="${esc(peType(p))}"><svg class="ico"><use href="#${ic}"/></svg></span>` : c ? `<span class="pav" style="background:var(--c${c.color_index})" title="${esc(PT.tKnown)}">${esc(ini)}</span>` : `<span class="pav none" title="${esc(PT.tUnknown)}">${esc(ini)}</span>`; };
  function peopleInit(organizer, attendees) { PEOPLE = { organizer: organizer || null, attendees: attendees || [] }; peopleRender(); }
  function peopleRender() { const all = peAll(); $('#ed-people-n').textContent = all.length || ''; $x('#ed-people-detail').innerHTML = ''; f.people_json.value = all.length ? JSON.stringify(all.map(p => [p.role, p.email])) : '';
    $x('#ed-people').innerHTML = all.map((p, i) => `<button class="pchip ${p.role === 'ORGANIZER' ? '' : peStatus(p)}" type="button" data-i="${i}" title="${esc(p.email)}">${peAv(p)}<span>${esc(peName(p))}</span>${p.role === 'ORGANIZER' ? `<small>${esc(PT.tOrgShort)}</small>` : p.role === 'OPT-PARTICIPANT' ? `<small>${esc(PT.tOptShort)}</small>` : ''}</button>`).join(''); }
  function peopleDetail(i) { const p = peAll()[i]; const n = (p.contacts || []).length; const status = p.role === 'ORGANIZER' ? '' : `<span class="pst ${peStatus(p)}">${esc(statusText(p) || D.tStatus.split('|')[0])}</span>`;
    $x('#ed-people-detail').innerHTML = `<div class="pdetail">${peAv(p)}<div><div class="nm">${esc(peName(p))}</div><div class="ml">${esc(p.email)}</div></div><span class="acts">${n ? `<button class="act" type="button" data-pact="book" ${n > 1 ? `data-n="${n}"` : ''} title="${esc(n > 1 ? PT.tBookN.replace('{n}', n) : PT.tBook)}"><svg class="ico"><use href="#i-user"/></svg></button>` : ''}<button class="act" type="button" data-pact="mail" title="${esc(PT.tMail)}"><svg class="ico"><use href="#i-mail"/></svg></button><button class="act" type="button" data-pact="copy" title="${esc(PT.tCopy)}"><svg class="ico"><use href="#i-copy"/></svg></button></span><div class="kv"><span>${esc(PT.tRole)}</span><span>${esc(peRole(p))}${peType(p) ? ' · ' + esc(peType(p)) : ''}</span>${status ? `<span>${esc(PT.tReply)}</span><span>${status}${peDeleg(p) ? ' · ' + esc(peDeleg(p)) : ''}</span>` : ''}${p.extra.some(x => x[0] === 'RSVP' && /true/i.test(x[1])) ? `<span>${esc(PT.tRsvp)}</span><span>${esc(PT.tYes)}</span>` : ''}</div></div>`;
    $$('.pchip', PC).forEach(b => b.setAttribute('aria-expanded', b.dataset.i === String(i))); }
  PC.addEventListener('click', e => { const chip = e.target.closest('.pchip'); const act = e.target.closest('[data-pact]');
    if (chip) { const i = +chip.dataset.i; const open = chip.getAttribute('aria-expanded') === 'true'; if (open) { $x('#ed-people-detail').innerHTML = ''; chip.setAttribute('aria-expanded', 'false'); } else peopleDetail(i); return; }
    if (!act) return; const p = peAll()[+$('.pchip[aria-expanded=true]', PC).dataset.i];
    if (act.dataset.pact === 'mail') location.href = 'mailto:' + p.email;
    else if (act.dataset.pact === 'copy') navigator.clipboard.writeText(p.email).then(() => { act.title = D.tCopied; act.style.color = 'var(--accent)'; setTimeout(() => act.style.color = '', 800); });
    else if (p.contacts.length === 1) location.href = '/contacts#' + p.contacts[0].key;
    else rrPop(act, [], null, key => { location.href = '/contacts#' + key; }, `<div class="q">${esc(PT.tPick)}</div>` + p.contacts.map(c => `<button type="button" class="pk" data-v="${esc(c.key)}"><span class="pav" style="background:var(--c${c.color_index})">${esc(c.initials)}</span><b>${esc(c.name)}</b><small>${esc(c.sub)}</small></button>`).join('')); });
  // links: URL / attachment / conference; an embedded (binary) attachment is shown read-only and never re-sent
  // ---- links & attachments (round 17, design K): one model `LK` for URL/ATTACH/CONFERENCE (`links`) and
  // RELATED-TO (`related`), mirrored into the hidden `links_json` for dirty tracking. Adding: a paste field that
  // detects the kind, a search over events and tasks, a drop zone that embeds files as base64 (2 MiB each).
  // Rows in three groups; a conference gets a join card with provider, meeting id and dial-in numbers. ----
  const LKC = $('#ed-links-card'), LKT = LKC.dataset, lkList = $('#ed-lk-list'), lkDetail = $('#ed-lk-detail'), lkMsg = $('#ed-lk-msg');
  let LK = { links: [], related: [] }, lkSel = null, lkPop = null;
  const CONF_HOSTS = [[/(^|\.)zoom\.us$/, 'Zoom'], [/teams\.(microsoft|live)\.com$/, 'Microsoft Teams'], [/^meet\.google\.com$/, 'Google Meet'], [/jit\.?si/, 'Jitsi'], [/webex\.com$/, 'Webex'], [/bigbluebutton|(^|\.)bbb\./, 'BigBlueButton']];
  const ATTACH_EXT = /\.(pdf|jpe?g|png|gif|webp|heic|docx?|xlsx?|pptx?|odt|ods|zip|ics|txt)(\?.*)?$/i;
  const lkHost = u => { try { return new URL(u).hostname.replace(/^www\./, ''); } catch { return ''; } };
  const lkProvider = u => { const h = lkHost(u); const p = CONF_HOSTS.find(([re]) => re.test(h)); return p ? p[1] : ''; };
  const lkKind = u => /^tel:/i.test(u) ? 'conference' : lkProvider(u) ? 'conference' : ATTACH_EXT.test((() => { try { return new URL(u).pathname; } catch { return u; } })()) ? 'attach' : 'url';
  const lkSize = n => n == null ? '' : n >= 1048576 ? (n / 1048576).toLocaleString(lang, { maximumFractionDigits: 1 }) + ' MB' : n >= 1024 ? Math.round(n / 1024) + ' KB' : n + ' B';
  const lkTitle = l => l.label || l.filename || (l.embedded ? LKT.tAttach + (l.bin_index != null ? ' ' + (l.bin_index + 1) : '') : lkHost(l.uri) || l.uri);
  const lkSub = l => [l.embedded ? LKT.tEmbedded : (l.label || l.filename ? lkHost(l.uri) : l.uri.replace(/^https?:\/\/(www\.)?/i, '').replace(/\/$/, '')), l.fmt && l.embedded ? l.fmt : '', l.size ? lkSize(l.size) : '', l.kind === 'conference' && l.features && l.features.length ? l.features.map(x => x.toLowerCase()).join(', ') : ''].filter(Boolean).join(' · ');
  const lkIcon = l => l.embedded || l.kind === 'attach' ? ((l.fmt || '').startsWith('image/') || /\.(jpe?g|png|gif|webp|heic)$/i.test(l.filename || l.uri) ? 'i-image' : 'i-clip') : l.kind === 'conference' ? 'i-chat' : 'i-link';
  const lkMirror = () => { f.links_json.value = LK.links.length || LK.related.length ? JSON.stringify([LK.links.map(l => [l.kind, l.uri, l.label, l.filename, l.keep, !!l.data]), LK.related.map(r => r.uid)]) : ''; };
  function linksInit(links, related) { LK = { links: (links || []).map(l => ({ ...l, keep: l.embedded ? true : false })), related: (related || []).map(r => ({ ...r })) }; lkSel = null; lkMsg.textContent = ''; lkRender(); }
  const lkSync = () => { lkMirror(); lkRender(); refresh(); };
  const linksOf = () => ({ links: LK.links.map(({ _rule, _mode, ...l }) => ({ ...l, features: l.features || [], extra: l.extra || [] })), related: LK.related.map(({ target, ...r }) => ({ ...r, extra: r.extra || [] })) });
  const lkMeeting = u => { const p = lkProvider(u); let id = '', pwd = false; try { const x = new URL(u); pwd = x.searchParams.has('pwd') || x.searchParams.has('passcode'); const m = x.pathname.match(/\/j\/(\d{9,11})/) || x.pathname.match(/\/([a-z]{3}-[a-z]{4}-[a-z]{3})$/) || x.pathname.match(/\/([A-Za-z0-9._-]{6,})$/); if (m) id = m[1].replace(/^(\d{3})(\d{4})(\d{4})$/, '$1 $2 $3'); } catch {} return { provider: p || LKT.tConf, id, pwd }; };
  const lkActs = (i, kind) => { const l = LK.links[i]; const locked = !!l._locked && !ruleMode; const view = l.embedded ? (f.key.value && l.bin_index != null ? [['download', 'i-download', LKT.tDownload]] : []) : [['open', 'i-ext', LKT.tOpen], ['copy', 'i-copy', LKT.tCopy]]; return `<span class="lk-acts">${view.map(([a, ic, t]) => `<button class="act views" type="button" data-lact="${a}" data-i="${i}" title="${esc(t)}"><svg class="ico"><use href="#${ic}"/></svg></button>`).join('')}${locked ? '' : `<button class="act edits${lkSel === i ? ' on' : ''}" type="button" data-lact="edit" data-i="${i}" title="${esc(LKT.tEdit)}"><svg class="ico"><use href="#i-dots"/></svg></button><button class="act edits" type="button" data-lact="rm" data-i="${i}" title="${esc(LKT.tRemove)}"><svg class="ico"><use href="#i-x"/></svg></button>`}</span>`; };
  const lkRow = i => { const l = LK.links[i]; return `<div class="lk-row${lkSel === i ? ' sel' : ''}${l._locked ? ' locked' : ''}"><svg class="ico"><use href="#${lkIcon(l)}"/></svg><span class="lk-t" title="${esc(l.uri || l.filename)}">${esc(lkTitle(l))}</span><span class="lk-s">${esc(lkSub(l))}</span>${lkActs(i)}</div>`; };
  const lkRelRow = i => { const r = LK.related[i], t = r.target; return `<div class="lk-row"><svg class="ico"><use href="#${t && t.kind === 'task' ? 'i-tasks' : 'i-calendar'}"/></svg><span class="lk-t">${esc(t ? t.summary || r.uid : r.uid)}</span><span class="lk-s">${esc(t ? [t.kind === 'task' ? LKT.tTask : LKT.tEvent, t.date ? fmtD(t.date) : '', t.done ? LKT.tDone : ''].filter(Boolean).join(' · ') : LKT.tNotfound)}</span><span class="lk-acts">${t ? `<button class="act views" type="button" data-lact="goto" data-i="${i}" title="${esc(LKT.tOpen)}"><svg class="ico"><use href="#i-ext"/></svg></button>` : ''}<button class="act edits" type="button" data-lact="rmrel" data-i="${i}" title="${esc(LKT.tRemove)}"><svg class="ico"><use href="#i-x"/></svg></button></span></div>`; };
  const lkSec = (icon, title, body) => body ? `<div class="lk-sec"><h4><svg class="ico"><use href="#${icon}"/></svg> ${esc(title)}</h4>${body}</div>` : '';
  function lkRender() {
    lkMirror();
    const idx = LK.links.map((l, i) => i);
    const conf = idx.filter(i => LK.links[i].kind === 'conference'), web = conf.filter(i => /^https?:/i.test(LK.links[i].uri)), tel = conf.filter(i => /^tel:/i.test(LK.links[i].uri));
    let c = '';
    if (web.length) { const l = LK.links[web[0]], m = lkMeeting(l.uri); c += `<div class="lk-join"><button class="btn" type="button" data-lact="open" data-i="${web[0]}"><svg class="ico" aria-hidden="true"><use href="#i-headset"/></svg><span>${esc(LKT.tJoin)}</span></button><b>${esc(l.label && l.label !== m.provider ? l.label : LKT.tMeeting.replace('{provider}', m.provider))}</b><small>${esc([m.id ? LKT.tId + ' ' + m.id : '', m.pwd ? LKT.tPwd : '', (l.features || []).map(x => x.toLowerCase()).join(' + ')].filter(Boolean).join(' · ') || lkHost(l.uri))}</small></div>`; const kv = [[LKT.tLink, l.uri.replace(/^https?:\/\//, '').slice(0, 40) + (l.uri.length > 48 ? '…' : '')], ...tel.map(i => [LKT.tPhone, LK.links[i].uri.replace(/^tel:/i, '') + (LK.links[i].label ? ' · ' + LK.links[i].label : '')])]; c += `<div class="lk-kv">${kv.map(([k, v]) => `<span>${esc(k)}</span><span>${esc(v)}</span>`).join('')}</div>`; }
    c += conf.filter(i => i !== web[0] && !tel.includes(i)).map(lkRow).join('') + (web.length ? lkRow(web[0]) .replace('class="lk-row', 'class="lk-row edits') : '');
    if (!conf.length) c = `<button class="lk-pill edits" type="button" data-lact="jitsi"><svg class="ico"><use href="#i-plus"/></svg> ${esc(LKT.tJitsi)}</button>`;
    const internal = LK.related.map((r, i) => lkRelRow(i)).join('');
    const internet = idx.filter(i => LK.links[i].kind !== 'conference').map(lkRow).join('');
    const total = LK.links.filter(l => l.embedded && l.data).reduce((a, l) => a + (l.size || 0), 0);
    lkList.innerHTML = (conf.length || form.classList.contains('mode-edit') ? lkSec('i-chat', LKT.tConf, c) : '') + lkSec('i-calendar', LKT.tInternal, internal) + lkSec('i-globe', LKT.tInternet, internet);
    if (!conf.length && form.classList.contains('mode-edit')) $('.lk-sec', lkList).classList.add('edits'); /* the empty conference section is edit-only; in view mode it is not rendered, so the first section is a real one */
    $('#ed-lk-drop').title = total ? LKT.tTotal.replace('{n}', lkSize(total)) : '';
    lkDetailRender();
  }
  function lkDetailRender() {
    const l = lkSel != null ? LK.links[lkSel] : null;
    lkDetail.hidden = !l;
    if (!l) { lkDetail.innerHTML = ''; return; }
    lkDetail.innerHTML = `<div class="lk-dh"><span>${esc(LKT.tEdit)}: ${esc(lkTitle(l))}</span><button type="button" data-lact="rm" data-i="${lkSel}">${esc(LKT.tRemove)}</button></div>` + (l.embedded ? `<div class="lk-kv" style="margin:0"><span>${esc(LKT.tAttach)}</span><span>${esc(l.filename || '')}</span><span>${esc(l.fmt ? 'Typ' : '')}</span><span>${esc(l.fmt || '')}</span></div>` : `<div class="field"><input type="url" class="aux mirror" data-lk="uri" value="${esc(l.uri)}"><span>${esc(LKT.tAddress)}</span></div><div class="field"><input type="text" class="aux mirror" data-lk="label" value="${esc(l.label || '')}" placeholder="${esc(lkHost(l.uri))}"><span>${esc(LKT.tLabel)}</span></div><span class="seg">${[['url', 'i-ext', LKT.tUrl], ['attach', 'i-download', LKT.tAttach], ['conference', 'i-chat', LKT.tConfk]].map(([k, ic, t]) => `<button type="button" data-lk-kind="${k}" aria-pressed="${l.kind === k}"><svg class="ico"><use href="#${ic}"/></svg> ${esc(t)}</button>`).join('')}</span>`);
  }
  const lkAddUri = (uri, label) => { uri = uri.trim(); if (!uri) return; if (!/^[a-z][a-z0-9+.-]*:/i.test(uri)) uri = 'https://' + uri; const kind = lkKind(uri); if (LK.links.some(l => l.uri === uri)) return; LK.links.push({ kind, uri, label: label || '', embedded: false, filename: '', fmt: '', size: null, features: kind === 'conference' && /^https?:/i.test(uri) ? ['AUDIO', 'VIDEO'] : [], extra: [], keep: false }); lkSel = null; lkSync(); };
  const lkAddFiles = files => { let total = LK.links.filter(l => l.embedded && l.data).reduce((a, l) => a + (l.size || 0), 0); const errs = []; [...files].forEach(file => { if (file.size > 2 * 1024 * 1024 || total + file.size > 6 * 1024 * 1024) { errs.push(LKT.tToobig.replace('{name}', file.name)); return; } total += file.size; const rd = new FileReader(); rd.onload = () => { LK.links.push({ kind: 'attach', uri: '', label: '', embedded: true, filename: file.name, fmt: file.type || 'application/octet-stream', size: file.size, features: [], extra: [], keep: false, data: String(rd.result).replace(/^data:[^,]*,/, '') }); lkSync(); }; rd.readAsDataURL(file); }); lkMsg.className = 'lk-msg' + (errs.length ? ' err' : ''); lkMsg.textContent = errs.join(' · '); };
  const lkClosePop = () => { if (lkPop) { lkPop.remove(); lkPop = null; } };
  let lkSearchT = 0;
  $('#ed-lk-search').addEventListener('input', e => { clearTimeout(lkSearchT); const q = e.target.value.trim(); if (q.length < 2) { lkClosePop(); return; } lkSearchT = setTimeout(async () => { const r = await fetch('/api/search?q=' + encodeURIComponent(q)); if (!r.ok) return; const d = await r.json(); const seen = new Set([current && current.uid, ...LK.related.map(x => x.uid)]); const hits = [...d.events.map(x => ({ kind: 'event', key: x.key, uid: x.uid, summary: x.summary, date: x.date, done: false })), ...d.tasks.map(x => ({ kind: 'task', key: x.key, uid: x.uid, summary: x.summary, date: x.due, done: x.completed }))].filter(h => h.uid && !seen.has(h.uid) && seen.add(h.uid)).slice(0, 8); lkClosePop(); lkPop = document.createElement('div'); lkPop.className = 'rrpop'; lkPop.innerHTML = hits.length ? hits.map((h, i) => `<button type="button" class="lk-hit" data-h="${i}"><svg class="ico"><use href="#${h.kind === 'task' ? 'i-tasks' : 'i-calendar'}"/></svg><span>${esc(h.summary)}</span><small>${esc([h.kind === 'task' ? LKT.tTask : LKT.tEvent, h.date ? fmtD(h.date) : ''].filter(Boolean).join(' · '))}</small></button>`).join('') : `<div class="lk-none">${esc(LKT.tNothing)}</div>`; const box = e.target.closest('.box'); document.body.appendChild(lkPop); const r2 = e.target.getBoundingClientRect(), maxRight = (box ? box.getBoundingClientRect().right : innerWidth) - 8; lkPop.style.left = `${Math.max(8, Math.min(r2.left, maxRight - lkPop.offsetWidth)) + scrollX}px`; lkPop.style.top = `${r2.bottom + scrollY + 4}px`; lkPop.addEventListener('click', ev => { const b = ev.target.closest('.lk-hit'); if (!b) return; const h = hits[+b.dataset.h]; LK.related.push({ uid: h.uid, reltype: '', extra: [], target: h }); e.target.value = ''; lkClosePop(); lkSync(); }); }, 250); });
  $('#ed-lk-search').addEventListener('blur', () => setTimeout(lkClosePop, 150));
  $('#ed-lk-addbtn').onclick = () => { lkAddUri($('#ed-lk-url').value); $('#ed-lk-url').value = ''; };
  $('#ed-lk-url').addEventListener('keydown', e => { if (e.key === 'Enter') { e.preventDefault(); $('#ed-lk-addbtn').click(); } });
  const lkDrop = $('#ed-lk-drop'), lkFile = $('#ed-lk-file');
  lkDrop.addEventListener('dragover', e => { e.preventDefault(); lkDrop.classList.add('over'); });
  lkDrop.addEventListener('dragleave', () => lkDrop.classList.remove('over'));
  lkDrop.addEventListener('drop', e => { e.preventDefault(); lkDrop.classList.remove('over'); if (e.dataTransfer.files.length) lkAddFiles(e.dataTransfer.files); else { const t = e.dataTransfer.getData('text/uri-list') || e.dataTransfer.getData('text/plain'); if (t) lkAddUri(t.split('\n')[0]); } });
  lkDrop.addEventListener('click', e => { if (e.target !== lkFile) lkFile.click(); });
  lkDrop.addEventListener('keydown', e => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); lkFile.click(); } });
  lkFile.addEventListener('change', () => { lkAddFiles(lkFile.files); lkFile.value = ''; });
  lkDetail.addEventListener('input', e => { const l = LK.links[lkSel]; if (!l || !e.target.dataset.lk) return; l[e.target.dataset.lk] = e.target.value.trim(); lkMirror(); $$('.lk-row', lkList)[0] && lkRenderRowsOnly(); refresh(); });
  const lkRenderRowsOnly = () => { const d = lkDetail.innerHTML; lkRender(); lkDetail.innerHTML = d; };
  lkDetail.addEventListener('click', e => { const k = e.target.closest('[data-lk-kind]'); if (k && lkSel != null) { LK.links[lkSel].kind = k.dataset.lkKind; if (k.dataset.lkKind === 'conference' && !(LK.links[lkSel].features || []).length) LK.links[lkSel].features = ['AUDIO', 'VIDEO']; lkSync(); } });
  LKC.addEventListener('click', e => { const b = e.target.closest('[data-lact]'); if (!b) return; const a = b.dataset.lact, i = +b.dataset.i;
    if (a === 'open') window.open(LK.links[i].uri, '_blank', 'noopener');
    else if (a === 'copy') navigator.clipboard.writeText(LK.links[i].uri).then(() => { b.title = D.tCopied; b.style.color = 'var(--accent)'; setTimeout(() => b.style.color = '', 800); });
    else if (a === 'download') window.open(`/api/events/${f.key.value}/attach/${LK.links[i].bin_index}`, '_blank', 'noopener');
    else if (a === 'edit') { lkSel = lkSel === i ? null : i; lkRender(); }
    else if (a === 'rm') { LK.links.splice(i, 1); lkSel = null; lkSync(); }
    else if (a === 'rmrel') { LK.related.splice(i, 1); lkSync(); }
    else if (a === 'goto') { const t = LK.related[i].target; if (t.kind === 'task') location.href = '/tasks#' + t.key; else if (serialize() !== baseline) { $('#ed-error').textContent = LKT.tSaveFirst; } else openEditor(t.key); }
    else if (a === 'jitsi') { const slug = (ruleMode || f.summary.value || 'raum') /* a rule's room is named after the category */.toLowerCase().normalize('NFD').replace(/[̀-ͯ]/g, '').replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '').slice(0, 40) || 'raum'; lkAddUri(`https://meet.jit.si/${slug}-${Math.random().toString(36).slice(2, 8)}`, 'Jitsi'); } });
  // colour: the colour picker; "like the calendar" means no COLOR property (color_cal ticked)
  const toHex = c => { if (/^#[0-9a-f]{6}$/i.test(c)) return c.toLowerCase(); const el = document.createElement('i'); el.style.color = c; document.body.appendChild(el); const rgb = getComputedStyle(el).color.match(/\d+/g); el.remove(); return rgb && rgb.length >= 3 ? '#' + rgb.slice(0, 3).map(n => (+n).toString(16).padStart(2, '0')).join('') : '#3478f6'; };
  const colorBtn = $('#ed-color-btn');
  function paintColor() { const b = $('#ed-color-btn'); b.style.setProperty('--c', f.color.value); b.querySelector('span').textContent = f.color_cal.checked ? b.dataset.tCal : ''; } // hoisted: syncDot calls it
  const setColor = c => { const own = !!c; f.color_cal.checked = !own; f.color.value = toHex(own ? c : (f.calendar.selectedOptions[0] || {}).dataset?.color || '#3478f6'); f.color.disabled = !own; paintColor(); };
  colorBtn.addEventListener('click', () => {
    const o = f.calendar.selectedOptions[0], kind = ruleMode ? (TASK ? 'category-task' : 'category') : TASK ? 'task' : 'event', title = ruleMode || f.summary.value.trim() || (TASK ? T.newTask || '' : T.newEv || '');
    window.colorPicker.open(colorBtn, { title, sub: tr('js-color-k-' + kind), value: colorOf(), inherit: o ? { hex: toHex(o.dataset.color), text: tr('js-color-inherit-cal', { cal: o.textContent }) } : null,
      self: { kind, key: ruleMode || f.key.value }, onPick: v => { setColor(v); syncDot(); refresh(); } });
  });
  const colorOf = () => f.color_cal.checked ? '' : f.color.value;
  const geoOf = () => { const a = f.lat.value.trim(), b = f.lon.value.trim(); return a !== '' && b !== '' && !isNaN(+a) && !isNaN(+b) ? [+a, +b] : null; };
  // ---- location (design G): ONE editable field — the address or the coordinates — the other part is
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
    if (LOC[O]) { const e = O === 'addr' ? LOC.errAddr : LOC.errCoords, v = O === 'addr' ? f.location.value : coordsText(); h = `<span class="chip${e ? ' err' : ''}" title="${O === 'addr' ? LT.tAddr : LT.tGeo}"><svg class="ico" aria-hidden="true"><use href="#${O === 'addr' ? 'i-pin' : 'i-target'}"/></svg><span class="txt">${(v || (LOC.pending ? '…' : '–')).replace(/</g, '&lt;')}</span><b data-locrm="${O}" title="${O === 'addr' ? LT.tRmAddr : LT.tRmCoords}">×</b></span>` + (e ? `<span class="errtxt">${O === 'addr' ? LT.tErrAddr : LT.tErrCoords}</span>` : ''); }
    else h = `<button class="chip ghost" type="button" data-locadd="${O}"><svg class="ico" aria-hidden="true"><use href="#i-plus"/></svg> ${O === 'addr' ? LT.tAddAddr : LT.tAddCoords}</button>`;
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
  // ---- map dialog: the crosshair is the position; with the address part in use the address must be
  // determined for a moved map before "apply" — apply writes both parts at once ----
  const gm = $('#geomap'); let map = null, gmSeq = 0, gmMoved = false, gmAddrKnown = false, gmAddrErr = false, gmAddr = '', gmMoving = false;
  const assetV = (($('script[src*="app.js"]') || {}).src || '').split('?')[1] || '';
  const loadLeaflet = () => new Promise((res, rej) => { if (window.L) return res(); const css = document.createElement('link'); css.rel = 'stylesheet'; css.href = '/assets/leaflet.css?' + assetV; document.head.appendChild(css); const js = document.createElement('script'); js.src = '/assets/leaflet.js?' + assetV; js.onload = res; js.onerror = () => rej(new Error('leaflet')); document.head.appendChild(js); });
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
    $('#gm-ok-ico').setAttribute('href', determine ? '#i-pin' : '#i-check');
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
  // ---- recurrence (design C): the rule is a model painted on a scrollable week strip — the month in full
  // with a neighbouring week on each side and separators around it; a click on a day toggles the pattern
  // (weekday, day of month) or, in the exception mode, strikes an occurrence or adds an extra date. Interval
  // and end are two pills with popovers; the end date is picked on the strip. Rules the strip cannot paint
  // (BYSETPOS, several ordinals, BYHOUR …) stay a raw text that is saved as typed. The strip owns the wheel:
  // one gesture moves at most to the neighbouring month, the next needs a short pause. ----
  const RC = $x('#ed-repeat-card'), RT = RC.dataset, rrBox = $x('#ed-rr');
  const DAYS = ['MO', 'TU', 'WE', 'TH', 'FR', 'SA', 'SU'], RR_FREQS = ['DAILY', 'WEEKLY', 'MONTHLY', 'YEARLY'];
  const DOWL = WEEKDAYS_LONG;
  const blankRR = () => ({ freq: '', interval: 1, byday: [], mode: 'date', bymonthday: null, ordinal: 1, ordday: 'TU', bymonth: null, end: { kind: 'never', count: 10, until: '' }, exdates: [], rdates: [], raw: '' });
  let RR = blankRR(), rrMode = 'pattern', rrPicking = false, rrFocus = null, rrLocked = false;
  const rrIso = d => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
  const rrWd = d => (d.getDay() + 6) % 7; // 0 = Monday
  const rrStart = () => parse((f.date.value || today).slice(0, 10));
  const ordN = () => Math.ceil(rrStart().getDate() / 7);
  // a stored rule → the model; parts the strip cannot paint keep the rule as raw text
  function rrParse(r, untilLocal) {
    const m = blankRR(); if (!r) return m;
    const kv = {}; r.replace(/^RRULE:/i, '').split(';').forEach(p => { const [k, v] = p.split('='); if (k) kv[k.toUpperCase()] = (v || '').toUpperCase(); });
    const raw = () => Object.assign(blankRR(), { freq: 'RAW', raw: r });
    if (!RR_FREQS.includes(kv.FREQ)) return raw();
    if (Object.keys(kv).some(k => !['FREQ', 'INTERVAL', 'BYDAY', 'BYMONTHDAY', 'BYMONTH', 'COUNT', 'UNTIL', 'WKST'].includes(k))) return raw();
    m.freq = kv.FREQ; m.interval = +(kv.INTERVAL || 1) || 1;
    if (kv.BYDAY) { const one = kv.BYDAY.match(/^([+-]?[1-4]|-1)([A-Z]{2})$/); if (one && m.freq !== 'WEEKLY') { m.mode = 'ordinal'; m.ordinal = +one[1]; m.ordday = one[2]; } else if (m.freq === 'WEEKLY' && /^([A-Z]{2})(,[A-Z]{2})*$/.test(kv.BYDAY)) m.byday = DAYS.filter(d => kv.BYDAY.split(',').includes(d)); else return raw(); }
    if (kv.BYMONTHDAY) { if (!/^\d{1,2}$/.test(kv.BYMONTHDAY) || m.mode === 'ordinal') return raw(); m.bymonthday = +kv.BYMONTHDAY; }
    if (kv.BYMONTH) { if (!/^\d{1,2}$/.test(kv.BYMONTH) || m.freq !== 'YEARLY') return raw(); m.bymonth = +kv.BYMONTH; }
    if (kv.COUNT) m.end = { ...m.end, kind: 'count', count: +kv.COUNT || 1 };
    if (kv.UNTIL) m.end = { ...m.end, kind: 'until', until: untilLocal || `${kv.UNTIL.slice(0, 4)}-${kv.UNTIL.slice(4, 6)}-${kv.UNTIL.slice(6, 8)}` };
    return m;
  }
  const rrBuild = () => { const r = RR; if (!r.freq) return null; if (r.freq === 'RAW') return r.raw.trim() || null; const p = ['FREQ=' + r.freq]; if (r.interval > 1) p.push('INTERVAL=' + r.interval);
    if (r.freq === 'WEEKLY' && r.byday.length) p.push('BYDAY=' + r.byday.join(','));
    if ((r.freq === 'MONTHLY' || r.freq === 'YEARLY') && r.mode === 'ordinal') p.push('BYDAY=' + r.ordinal + r.ordday);
    if ((r.freq === 'MONTHLY' || r.freq === 'YEARLY') && r.mode === 'date' && r.bymonthday) p.push('BYMONTHDAY=' + r.bymonthday);
    if (r.freq === 'YEARLY' && r.bymonth) p.push('BYMONTH=' + r.bymonth);
    if (r.end.kind === 'count') p.push('COUNT=' + r.end.count); if (r.end.kind === 'until' && r.end.until) p.push('UNTIL=' + r.end.until.replace(/-/g, '')); // the server turns a bare date into the local end of day
    return p.join(';'); };
  const rrUnit = () => RT.tUnits.split('|')[RR_FREQS.indexOf(RR.freq)] || '';
  const rrOrdTxt = () => RT.tOrd.split('|')[RR.ordinal === -1 ? 4 : RR.ordinal - 1];
  // the sentence of the view mode: "Wöchentlich am Dienstag, 6-mal"
  function rrSentence() { const r = RR; if (!r.freq) return ''; if (r.freq === 'RAW') return r.raw; const longs = RT.tFreqsLong.split('|'), s0 = rrStart();
    let t = r.interval > 1 ? D.tEvery.replace('{n}', r.interval) + ' ' + rrUnit() : longs[RR_FREQS.indexOf(r.freq) + 1];
    if (r.freq === 'WEEKLY' && r.byday.length) t += ' ' + D.tOn + ' ' + r.byday.map(d => DOWL[DAYS.indexOf(d)]).join(', ');
    if (r.freq === 'MONTHLY' || r.freq === 'YEARLY') { t += ' ' + D.tOn + ' ' + (r.mode === 'ordinal' ? `${rrOrdTxt()} ${DOWL[DAYS.indexOf(r.ordday)]}` : `${r.bymonthday || s0.getDate()}.`); if (r.freq === 'YEARLY') t += ' ' + RT.tIn + ' ' + MON[(r.bymonth || s0.getMonth() + 1) - 1]; }
    if (r.end.kind === 'count') t += ', ' + D.tCount.replace('{n}', r.end.count); if (r.end.kind === 'until' && r.end.until) t += ', ' + D.tUntil + ' ' + fmtD(r.end.until);
    return t.charAt(0).toUpperCase() + t.slice(1); }
  // occurrences for the strip (the server's caldata is authoritative; this covers what the strip can paint)
  function rrOccurrences(limit) { const r = RR, s0 = rrStart(), out = new Set(); if (!r.freq || r.freq === 'RAW') return out;
    const max = r.end.kind === 'count' ? r.end.count : Infinity, until = r.end.kind === 'until' && r.end.until ? r.end.until : null;
    const nth = (d, n, day) => { if (DAYS[rrWd(d)] !== day) return false; const dom = d.getDate(); if (n > 0) return Math.ceil(dom / 7) === n; const last = new Date(d.getFullYear(), d.getMonth() + 1, 0).getDate(); return Math.ceil((last - dom + 1) / 7) === -n; };
    const week0 = Math.floor((s0.getTime() / 864e5 + 3) / 7); let n = 0;
    for (let i = 0; i < limit && n < max; i++) { const d = new Date(s0); d.setDate(s0.getDate() + i); const iso = rrIso(d); if (until && iso > until) break; let ok = false;
      if (r.freq === 'DAILY') ok = i % r.interval === 0;
      else if (r.freq === 'WEEKLY') { const days = r.byday.length ? r.byday : [DAYS[rrWd(s0)]]; ok = days.includes(DAYS[rrWd(d)]) && (Math.floor((d.getTime() / 864e5 + 3) / 7) - week0) % r.interval === 0; }
      else if (r.freq === 'MONTHLY') { const mi = (d.getFullYear() - s0.getFullYear()) * 12 + d.getMonth() - s0.getMonth(); ok = mi % r.interval === 0 && (r.mode === 'ordinal' ? nth(d, r.ordinal, r.ordday) : d.getDate() === (r.bymonthday || s0.getDate())); }
      else if (r.freq === 'YEARLY') { ok = (d.getFullYear() - s0.getFullYear()) % r.interval === 0 && d.getMonth() + 1 === (r.bymonth || s0.getMonth() + 1) && (r.mode === 'ordinal' ? nth(d, r.ordinal, r.ordday) : d.getDate() === (r.bymonthday || s0.getDate())); }
      if (ok) { out.add(iso); n++; } }
    return out; }
  const rrMirror = () => { f.rrule_json.value = RR.freq ? JSON.stringify(RR) : (RR.exdates.length || RR.rdates.length ? JSON.stringify(RR) : ''); };
  function rrSync() { if (TASK) return; rrMirror(); $('#ed-rrule-text').textContent = rrSentence(); $('#ed-exdates').classList.toggle('empty', !RR.exdates.length); $('#ed-rdates').classList.toggle('empty', !RR.rdates.length); } // the view lists show only when set
  // popovers for the pills: a list of choices or a number entry
  let rrOpenPop = null; const rrClosePop = () => { if (rrOpenPop) { rrOpenPop.remove(); rrOpenPop = null; document.removeEventListener('click', rrOutside, true); } };
  function rrOutside(e) { if (rrOpenPop && !rrOpenPop.contains(e.target)) rrClosePop(); }
  function rrPop(anchor, items, cur, onpick, extra) { rrClosePop(); const p = document.createElement('div'); p.className = 'rrpop'; p.innerHTML = (extra || '') + items.map(i => `<button type="button" data-v="${esc(i.v)}"${String(i.v) === String(cur) ? ' aria-current="true"' : ''}>${esc(i.label)}</button>`).join(''); const r = anchor.getBoundingClientRect(); document.body.appendChild(p); const box = anchor.closest('.box'); const maxRight = (box ? box.getBoundingClientRect().right : innerWidth) - 8; p.style.left = `${Math.max(8, Math.min(r.left, maxRight - p.offsetWidth)) + scrollX}px`; p.style.top = `${r.bottom + scrollY + 4}px`; rrOpenPop = p; /* kept inside the dialog box */ p.addEventListener('click', e => { const b = e.target.closest('button[data-v]'); if (b) { rrClosePop(); onpick(b.dataset.v); } }); setTimeout(() => document.addEventListener('click', rrOutside, true), 0); return p; }
  function rrAskNumber(anchor, label, value, on) { const p = rrPop(anchor, [], null, () => {}, `<div class="q">${esc(label)}</div><input type="number" min="1" max="999" value="${value}"><button type="button" class="ok">${esc(RT.tApply)}</button>`); const i = $('input', p); i.focus(); i.select(); const ok = () => { const n = +i.value; if (n >= 1) on(n); rrClosePop(); }; i.onkeydown = e => { if (e.key === 'Enter') { e.preventDefault(); ok(); } }; $('.ok', p).onclick = ok; }
  const rrSet = () => { rrRender(); refresh(); };
  // a short eased glide of a scroll container (200–420 ms); any input stops it
  function rrGlide(el) { let raf = 0, on = false; const stop = () => { cancelAnimationFrame(raf); on = false; };
    const to = target => { stop(); const from = el.scrollTop, dist = target - from; if (Math.abs(dist) < 1) return; const t0 = performance.now(), ms = Math.min(420, 200 + Math.abs(dist) * 0.8); on = true;
      const step = now => { const k = Math.min(1, (now - t0) / ms), e = k < .5 ? 4 * k * k * k : 1 - Math.pow(-2 * k + 2, 3) / 2; el.scrollTop = from + dist * e; if (k < 1) raf = requestAnimationFrame(step); else on = false; }; raf = requestAnimationFrame(step); };
    return { to, stop, active: () => on }; }
  // ---- the strip: NWEEKS rows of ROWH px, starting a year before the event on a Monday; the separators are the
  // only variable spacing (a GAP margin above / below at most three rows), so every row position is arithmetic ----
  const ROWH = 32, GAP = 16, NWEEKS = 130;
  const GESTURE_PAUSE = 40; // ms without wheel input that end a gesture — the next one may reach the next month
  const SETTLE_MS = 120;    // ms after the last input before the strip glides to the nearest month
  const WHEEL_DRAG = 0.8;   // share of the wheel distance the strip follows
  const rrW0 = () => { const d = rrStart(); d.setDate(d.getDate() - rrWd(d) - 364); return d; };
  function rrRender() {
    if (TASK) return; // a task has no recurrence card in this editor
    rrBox.innerHTML = ''; ed.classList.toggle('picking', rrPicking);
    const s0 = rrStart(), freqs = RT.tFreqs.split('|'), longs = RT.tFreqsLong.split('|');
    const isRaw = RR.freq === 'RAW';
    rrBox.insertAdjacentHTML('beforeend', `<div class="rrsegs"><span class="seg" data-w="freq">${[''].concat(RR_FREQS).map((v, i) => `<button type="button" data-v="${v}" aria-pressed="${RR.freq === v}" title="${esc(longs[i])}">${esc(freqs[i])}</button>`).join('')}${isRaw ? `<button type="button" data-v="RAW" aria-pressed="true" title="${esc(RT.tCustom)}">${esc(RT.tCustomShort)}</button>` : ''}</span></div>`);
    $('[data-w=freq]', rrBox).onclick = e => { const b = e.target.closest('button'); if (!b) return; RR.freq = b.dataset.v; if (RR.freq === 'WEEKLY' && !RR.byday.length) RR.byday = [DAYS[rrWd(s0)]]; rrSet(); };
    if (!RR.freq) rrBox.insertAdjacentHTML('beforeend', `<div class="rrbottom"><button class="pillbtn ghost" type="button" disabled><svg class="ico" aria-hidden="true"><use href="#i-ellipsis"/></svg><span>${esc(RT.tInterval)}</span></button><button class="pillbtn ghost" type="button" disabled><svg class="ico" aria-hidden="true"><use href="#i-to-line"/></svg><span>${esc(RT.tEnd)}</span></button></div><hr class="rrsep">`); // no frequency yet: the row stays, inert
    if (RR.freq && !isRaw) {
      const u = rrUnit(), each = RT.tEach.split('|')[RR_FREQS.indexOf(RR.freq)];
      const shortD = d => `${d.slice(8, 10)}.${d.slice(5, 7)}.${d.slice(2, 4)}`;
      const endLabel = () => RR.end.kind === 'count' ? D.tCount.replace('{n}', RR.end.count) : RR.end.kind === 'until' && RR.end.until ? D.tUntil + ' ' + shortD(RR.end.until) : RT.tNoEnd;
      rrBox.insertAdjacentHTML('beforeend', `<div class="rrbottom"><button class="pillbtn${RR.interval > 1 ? '' : ' ghost'}" type="button" data-s="i"><svg class="ico" aria-hidden="true"><use href="#i-ellipsis"/></svg><span>${esc(RR.interval > 1 ? D.tEvery.replace('{n}', RR.interval) + ' ' + u : each)}</span></button><button class="pillbtn${RR.end.kind === 'never' ? ' ghost' : ''}" type="button" data-s="e"><svg class="ico" aria-hidden="true"><use href="#i-to-line"/></svg><span>${esc(endLabel())}</span></button></div><hr class="rrsep">`);
      $('.rrbottom', rrBox).onclick = e => { const b = e.target.closest('.pillbtn'); if (!b) return;
        if (b.dataset.s === 'i') rrPop(b, [1, 2, 3, 4, 6].map(n => ({ v: n, label: n === 1 ? each : D.tEvery.replace('{n}', n) + ' ' + u })).concat({ v: 'n', label: RT.tN }), RR.interval, v => { if (v === 'n') rrAskNumber(b, RT.tIntQ.replace('{u}', u), RR.interval, n => { RR.interval = n; rrSet(); }); else { RR.interval = +v; rrSet(); } });
        else rrPop(b, [{ v: 'never', label: RT.tNoEnd }, { v: 'count', label: RT.tEndCount + ' …' }, { v: 'until', label: RT.tEndUntil + ' …' }], RR.end.kind, v => { if (v === 'count') rrAskNumber(b, RT.tCountQ, RR.end.count, n => { RR.end = { ...RR.end, kind: 'count', count: n }; rrSet(); }); else if (v === 'until') { rrPicking = true; rrRender(); } else { RR.end = { ...RR.end, kind: 'never' }; rrSet(); } }); };
    }
    if (isRaw) { rrBox.insertAdjacentHTML('beforeend', `<div class="rrhint">${esc(RT.tCustomHint)}</div><div class="rrraw"><input type="text" value="${esc(RR.raw)}" placeholder="FREQ=…"></div>`); $('.rrraw input', rrBox).onchange = e => { RR.raw = e.target.value.trim(); rrSet(); }; }
    if (rrPicking) { rrBox.insertAdjacentHTML('beforeend', `<div class="pickmsg"><svg class="ico" aria-hidden="true"><use href="#i-today"/></svg><span>${esc(RT.tPick)}</span><button class="btn" type="button">${esc(RT.tCancel)}</button></div>`); $('.pickmsg .btn', rrBox).onclick = () => { rrPicking = false; rrRender(); }; }
    else rrBox.insertAdjacentHTML('beforeend', `<div class="rrhint">${esc(rrMode === 'pattern' ? RT.tHintPattern : RT.tHintEx)}</div>`);
    // week rows
    const occ = rrOccurrences(1100), ex = new Set(RR.exdates), rd = new Set(RR.rdates), untilD = RR.end.kind === 'until' ? RR.end.until : null, W0 = rrW0();
    if (rrFocus == null) rrFocus = s0.getFullYear() * 12 + s0.getMonth();
    const cal = document.createElement('div'); cal.className = 'calstrip'; cal.innerHTML = `<div class="calhead"><b></b><div class="dows">${DOW.map(d => `<span>${d}</span>`).join('')}</div></div><div class="calscroll"><div class="weeks${rrPicking ? ' pick' : ''}"></div></div>`;
    const weeks = $('.weeks', cal), scroller = $('.calscroll', cal), head = $('.calhead b', cal); let html = ''; const startIso = rrIso(s0);
    for (let i = 0; i < NWEEKS; i++) { const ws = new Date(W0); ws.setDate(W0.getDate() + i * 7); let row = '';
      for (let k = 0; k < 7; k++) { const d = new Date(ws); d.setDate(ws.getDate() + k); const iso = rrIso(d), mi = d.getFullYear() * 12 + d.getMonth();
        const cls = [iso === startIso ? 'start' : '', ex.has(iso) ? 'ex' : rd.has(iso) ? 'rd' : occ.has(iso) ? 'occ' : '', iso === untilD ? 'until' : '', iso < startIso ? 'past' : '', mi !== rrFocus ? 'dim' : '', k >= 5 ? 'we' : ''].filter(Boolean).join(' ');
        row += `<b data-d="${iso}" data-m="${mi}" class="${cls}">${d.getDate()}</b>`; }
      html += `<div class="wrow">${row}</div>`; }
    weeks.innerHTML = html;
    // geometry without layout reads
    const rowOf = d => Math.floor(Math.round((d - W0) / 864e5) / 7); // rounding absorbs the DST hour
    const monthRows = mi => { const y = Math.floor(mi / 12), m = mi % 12; return [rowOf(new Date(y, m, 1)), rowOf(new Date(y, m + 1, 0))]; }; // first and last week holding a day of the month
    const inRange = mi => { const [a, b] = monthRows(mi); return a >= 0 && b < NWEEKS; };
    const rowMonth = i => { const t = new Date(W0); t.setDate(W0.getDate() + i * 7 + 3); return t.getFullYear() * 12 + t.getMonth(); }; // the heading follows the week's Thursday
    let seps = { before: [], after: [] }; // rows carrying the GAP margin above / below
    const rowTop = i => i * ROWH + GAP * (seps.before.filter(r => r <= i).length + seps.after.filter(r => r < i).length);
    const rowAt = y => { let i = 0; while (i + 1 < NWEEKS && rowTop(i + 1) <= y) i++; return i; };
    const setSeps = (mi, nextM) => { const [a, b] = monthRows(mi); seps = { before: [a], after: [b] }; if (nextM != null && inRange(nextM)) { const [na, nb] = monthRows(nextM); if (nextM > mi) seps.after.push(nb); else seps.before.push(na); } // while scrolling only the far edge of the neighbour is pre-drawn
      $$('.wrow.sep-before, .wrow.sep-after', weeks).forEach(el => el.classList.remove('sep-before', 'sep-after')); seps.before.forEach(r => weeks.children[r].classList.add('sep-before')); seps.after.forEach(r => weeks.children[r].classList.add('sep-after')); };
    const snapTop = mi => Math.max(0, rowTop(monthRows(mi)[0]) - GAP - ROWH);
    const glide = rrGlide(scroller);
    const focusMonth = (mi, animate, nextM) => { if (!inRange(mi)) return; rrFocus = mi;
      const anchor = rowAt(scroller.scrollTop + scroller.clientHeight / 2), before = rowTop(anchor);
      setSeps(mi, nextM); if (animate) scroller.scrollTop += rowTop(anchor) - before; // the separators moved the rows: keep the row under the centre in place
      const top = snapTop(mi); scroller.style.height = `${rowTop(monthRows(mi)[1]) + ROWH + GAP + ROWH - top}px`;
      head.textContent = `${MON[mi % 12]} ${Math.floor(mi / 12)}`; $$('b[data-m]', weeks).forEach(el => el.classList.toggle('dim', +el.dataset.m !== mi));
      if (!animate || Math.abs(scroller.scrollTop - top) <= GAP) scroller.scrollTop = top; else glide.to(top); }; // a gap-sized rest is set at once: a glide would fight an ongoing gesture
    const nearestMonth = () => { const st = scroller.scrollTop; let best = rrFocus, dist = Math.abs(snapTop(rrFocus) - st); for (const mi of [rrFocus - 1, rrFocus + 1]) if (inRange(mi) && Math.abs(snapTop(mi) - st) < dist) { best = mi; dist = Math.abs(snapTop(mi) - st); } return best; }; // past half the way the neighbour wins
    const settle = () => focusMonth(nearestMonth(), true, null);
    // the strip owns the wheel: one gesture reaches at most the neighbouring month and snaps in on arrival
    let lastWheel = 0, gestureFrom = null, gestureDir = 0, settleT = 0;
    scroller.addEventListener('wheel', e => { e.preventDefault(); glide.stop(); const now = e.timeStamp; // the input's own time: a busy main thread must not split a gesture
      if (now - lastWheel > GESTURE_PAUSE || gestureFrom == null) { gestureFrom = rrFocus; gestureDir = 0; } lastWheel = now;
      const dir = Math.sign(e.deltaY); if (dir && dir !== gestureDir) { gestureDir = dir; setSeps(rrFocus, rrFocus + dir); }
      const lo = snapTop(inRange(gestureFrom - 1) ? gestureFrom - 1 : gestureFrom), hi = snapTop(inRange(gestureFrom + 1) ? gestureFrom + 1 : gestureFrom);
      const delta = e.deltaY * (e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? scroller.clientHeight : 1) * WHEEL_DRAG;
      const target = Math.min(hi, Math.max(lo, scroller.scrollTop + delta)); scroller.scrollTop = target;
      clearTimeout(settleT); if ((target === hi && dir > 0) || (target === lo && dir < 0)) settle(); else settleT = setTimeout(settle, SETTLE_MS); }, { passive: false });
    scroller.addEventListener('pointerdown', glide.stop); scroller.addEventListener('touchstart', glide.stop, { passive: true });
    scroller.addEventListener('scroll', () => { const mi = rowMonth(rowAt(scroller.scrollTop + scroller.clientHeight / 2)); head.textContent = `${MON[mi % 12]} ${Math.floor(mi / 12)}`; /* the heading follows at once */ if (glide.active() || performance.now() - lastWheel < SETTLE_MS) return; clearTimeout(settleT); settleT = setTimeout(settle, SETTLE_MS); });
    weeks.addEventListener('click', e => { const b = e.target.closest('b[data-d]'); if (!b) return; const iso = b.dataset.d, d = parse(iso);
      if (rrPicking) { if (iso < startIso) return; RR.end = { ...RR.end, kind: 'until', until: iso }; rrPicking = false; rrSet(); return; }
      if (isRaw) return;
      if (rrMode === 'pattern') { if (!RR.freq) RR.freq = 'WEEKLY'; if (RR.freq === 'WEEKLY') { const day = DAYS[rrWd(d)]; RR.byday = RR.byday.includes(day) ? RR.byday.filter(x => x !== day) : DAYS.filter(x => RR.byday.includes(x) || x === day); if (!RR.byday.length) RR.byday = [day]; } else if (RR.freq === 'MONTHLY' || RR.freq === 'YEARLY') { RR.mode = 'date'; RR.bymonthday = d.getDate(); if (RR.freq === 'YEARLY') RR.bymonth = d.getMonth() + 1; } }
      else { if (occ.has(iso)) RR.exdates = ex.has(iso) ? RR.exdates.filter(x => x !== iso) : RR.exdates.concat(iso); else if (ex.has(iso)) RR.exdates = RR.exdates.filter(x => x !== iso); else RR.rdates = rd.has(iso) ? RR.rdates.filter(x => x !== iso) : RR.rdates.concat(iso); }
      rrSet(); });
    rrBox.appendChild(cal); focusMonth(rrFocus, false, null);
    const lg = RT.tLegend.split('|');
    rrBox.insertAdjacentHTML('beforeend', `<div class="rrlegend"><span><i style="background:var(--accent)"></i>${esc(lg[0])}</span><span><i style="background:var(--bg-3)"></i>${esc(lg[1])}</span><span><i style="background:var(--c2)"></i>${esc(lg[2])}</span><span><i style="outline:2px solid var(--c5);outline-offset:-2px"></i>${esc(lg[3])}</span>${untilD ? `<span><i style="background:var(--c4)"></i>${esc(lg[4])}</span>` : ''}</div>`);
    const md = RT.tMode.split('|');
    rrBox.insertAdjacentHTML('beforeend', `<hr class="rrsep"><div class="rrmode"><span>${esc(md[0])}</span><span class="seg" data-w="mode"><button type="button" data-v="pattern" aria-pressed="${rrMode === 'pattern'}">${esc(md[1])}</button><button type="button" data-v="ex" aria-pressed="${rrMode === 'ex'}">${esc(md[2])}</button></span></div>`);
    $('[data-w=mode]', rrBox).onclick = e => { const b = e.target.closest('button'); if (b) { rrMode = b.dataset.v; rrRender(); } };
    rrMirror();
  }
  // view mode rows for exceptions and extra dates (edited on the strip)
  const addDate = (list, iso) => { const r = addRow(list); $('.txt', r).textContent = iso ? fmtD(iso) : ''; return r; };
  function rrInit(rule, untilLocal, exdates, rdates) { RR = rrParse(rule, untilLocal); RR.exdates = (exdates || []).map(x => x.slice(0, 10)); RR.rdates = (rdates || []).map(x => x.slice(0, 10)); rrMode = 'pattern'; rrPicking = false; rrFocus = null; rrClosePop(); RR.exdates.forEach(x => addDate('exdates', x)); RR.rdates.forEach(x => addDate('rdates', x)); rrRender(); }
  const rrDates = list => RR[list].map(d => f.all_day.checked ? d : `${d}T${f.from.value || '00:00'}:00`);
  addEventListener('keydown', e => { if (e.key === 'Escape' && rrPicking && !ed.hidden) { e.stopImmediatePropagation(); rrPicking = false; rrRender(); } }, true);
  // cards: the "when" card always, the others when they hold data or were added in edit mode
  // `.aux` inputs (rule interval, count, until, raw rule) only qualify the rule and never count as data on their own
  const hasData = card => $$('input:not([type=hidden]):not(.aux):not(.mirror), textarea, select.dat', card).some(i => i.value.trim() !== '') || !!$('.pchips .pchip', card) || $$('.rows .row', card).length > 0;
  function cards() { const edit = form.classList.contains('mode-edit'); $$('.card', form).forEach(c => { c.hidden = !(c.hasAttribute('data-always') || (edit && c.hasAttribute('data-always-edit')) || hasData(c) || (edit && c.dataset.added)); });
    // a task that is simply open has no progress to show: STATUS:NEEDS-ACTION is the absence of one
    const pc = TASK && $('.card[data-card="progress"]', form);
    if (pc && !pc.dataset.added) pc.hidden = !(f.status.value && f.status.value !== 'NEEDS-ACTION') && !(+f.percent.value > 0) && !f.completed_date.value; /* data-always-edit: the categories card is the quick pick while editing, in view mode only with data */ $$('.addmenu [data-card-add]', form).forEach(b => b.disabled = !$(`.card[data-card="${b.dataset.cardAdd}"]`, form).hidden); soloCards(); }
  function addCard(name) { const c = $(`.card[data-card="${name}"]`, form); c.dataset.added = '1'; c.hidden = false; cards(); return c; }
  const serialize = () => $$('input:not([type=hidden]):not(.mirror), select, textarea', form).map(e => e.type === 'checkbox' ? (e.checked ? '1' : '0') : e.value).join('|') + '|' + chips().join(',');
  function updateButtons() { const changed = sig() !== baseline; $('#ed-save').disabled = !changed; $('#ed-reset').disabled = !changed && form.classList.contains('new'); $('#ed-delete').disabled = form.classList.contains('mode-view') || form.classList.contains('new'); }
  function taskLine() {
    const [start] = getStamp('start'), [due] = getStamp('due');
    const box = $('#ed-whenline');
    const one = v => v ? (v.slice(11, 16) === '00:00' ? fmtD(v) : fmtDT(v)) : '';
    box.textContent = due ? (start ? one(start) + ' – ' + one(due) : one(due)) : start ? one(start) : '';
    const pt = $('#ed-progress-text');
    if (pt) {
      const P = $('#ed-progress-card').dataset, parts = [];
      if (f.status.value) parts.push(f.status.selectedOptions[0].textContent);
      if (+f.percent.value > 0 && +f.percent.value < 100) parts.push(P.tViewPct.replace('{n}', f.percent.value));
      const [done] = getStamp('completed');
      if (done) parts.push(P.tViewDone.replace('{at}', fmtDT(done)));
      pt.textContent = parts.join(' · ');
    }
    pctOut();
  }
  function whenLine() { if (TASK) return taskLine(); const s = f.date.value; if (!s) return; const e = f.end_date.value || s, multi = e > s; const t = f.all_day.checked ? (multi ? `${fmtD(s)} – ${fmtD(e)}` : fmtD(s)) + ' · ' + D.tAllday : multi ? `${fmtD(s)} ${f.from.value} – ${fmtD(e)} ${f.to.value}` : `${fmtD(s)} · ${f.from.value} – ${f.to.value}`; $('#ed-whenline').textContent = t; }
  const grow = () => { const t = f.description; t.style.height = 'auto'; t.style.height = (t.scrollHeight + 2) + 'px'; }; // the note shows its whole text in both modes
  function refresh() { whenLine(); rrSync(); grow(); alTimeline(); if (ruleMode || Object.keys(RFX).length || RFXL.alarms.length || RFXL.links.length) { if (ruleMode) { ALARMS.forEach(a => { a._mode ||= 'default'; }); LK.links.forEach(l => { l._mode ||= 'default'; }); } rulesMark(); } /* footers follow added or removed entries */ soloCards(); updateButtons(); }
  function mode(m) { form.classList.toggle('mode-view', m === 'view'); form.classList.toggle('mode-edit', m === 'edit'); ed.classList.toggle('mode-edit', m === 'edit'); cards(); lkRender(); ntClosePop(); if (m === 'view') ntRender(); else { grow(); ntDetect(); } if (m === 'edit') rrRender(); /* the strip measures itself: it must be visible */ refresh(); requestAnimationFrame(fitAddrLines); }
  // all-day: the API end is the exclusive DTEND; the editor shows the last day (empty = one day)
  const addDays = (s, n) => { const e = new Date(s + 'T00:00:00Z'); e.setUTCDate(e.getUTCDate() + n); return e.toISOString().slice(0, 10); }; // UTC arithmetic: no zone shift
  const lastDay = d => { const s = addDays(d.end.slice(0, 10), -1); return s > d.start.slice(0, 10) ? s : ''; };
  const nextDay = s => addDays(s, 1);
  // a task carries an optional start and an optional due, each with an optional time
  function setStamp(name, v, allDay) {
    const dEl = f[name + '_date'], tEl = f[name + '_time'];
    if (!dEl) return;
    dEl.value = v ? String(v).slice(0, 10) : '';
    tEl.value = v && !allDay ? String(v).slice(11, 16) : '';
  }
  function getStamp(name) {
    const dEl = f[name + '_date'], tEl = f[name + '_time'];
    if (!dEl || !dEl.value) return [null, true];
    return [dEl.value + (tEl.value ? 'T' + tEl.value + ':00' : 'T00:00:00'), !tEl.value];
  }
  const pctOut = () => { const o = $('#ed-pct-out'); if (o && f.percent) o.textContent = f.percent.value + ' %'; };
  function taskBody() {
    const [principal, cal_id] = f.calendar.value.split('/');
    const [start, startAll] = getStamp('start'), [due, dueAll] = getStamp('due'), [done] = getStamp('completed');
    return {
      principal, cal_id, summary: f.summary.value, description: f.description.value, location: f.location.value,
      start, start_all_day: startAll, due, due_all_day: dueAll,
      status: f.status ? f.status.value : '', percent: f.percent ? +f.percent.value : null, completed_at: done,
      priority: f.priority && f.priority.value ? +f.priority.value : null, class: f.class ? f.class.value : '',
      categories: chips(), color: colorOf(), geo: geoOf(),
      alarms: alarmsOf(), ...linksOf(), etag: f.etag.value,
    };
  }
  // `asNew`: `d` is a template (`tplUse`) — its attributes fill a new event
  function fill(d, date, asNew) {
    const old = d && !asNew; current = old ? d : null; openDate = date; form.reset(); $('#ed-error').textContent = '';
    $$('.card[data-added]', form).forEach(x => x.removeAttribute('data-added')); catsInit([]); $$('.rows .row', form).forEach(x => x.remove());
    f.key.value = old ? `${d.principal}/${d.cal_id}/${d.object_id}` : ''; f.etag.value = old ? d.etag : '';
    form.classList.toggle('new', !old); f.summary.placeholder = old ? D.tTitle : D.tNewtitle;
    const b = $('#ed-save'); b.title = old ? D.tSave : D.tCreate; b.setAttribute('aria-label', b.title);
    if (d && TASK) {
      f.summary.value = d.summary; f.location.value = d.location; f.description.value = d.description; catsInit(d.categories);
      f.calendar.value = `${d.principal}/${d.cal_id}`;
      setStamp('start', d.start, d.start_all_day); setStamp('due', d.due, d.due_all_day); setStamp('completed', d.completed_at, false);
      f.status.value = d.status; f.percent.value = d.percent == null ? 0 : d.percent; pctOut();
      if (d.priority != null) setSel(f.priority, d.priority); f.class.value = d.class;
      alarmsInit(d.alarms); linksInit(d.links, d.related); setColor(d.color); stRender();
    } else if (d) {
      f.summary.value = d.summary; f.location.value = d.location; f.description.value = d.description; catsInit(d.categories);
      f.calendar.value = `${d.principal}/${d.cal_id}`; f.date.value = d.start.slice(0, 10); f.end_date.value = d.all_day ? lastDay(d) : d.end.slice(0, 10);
      f.from.value = d.start.slice(11, 16); f.to.value = d.end.slice(11, 16); setAllDay(d.all_day);
      alarmsInit(d.alarms); peopleInit(d.organizer, d.attendees);
      rrInit(d.rrule, d.rrule_until, d.exdates, d.rdates);
      f.status.value = d.status; f.transp.value = d.transp; f.class.value = d.class; if (d.priority != null) setSel(f.priority, d.priority); stRender(); linksInit(d.links, d.related); setColor(d.color); if (d.geo) { f.lat.value = d.geo[0]; f.lon.value = d.geo[1]; }
      $('#ed-hint').textContent = d.rrule ? D.tSeries : '';
    } else if (TASK) {
      f.status.value = 'NEEDS-ACTION'; f.percent.value = 0; pctOut(); setColor('');
      if (state.cals) { const first = state.cals.split(',')[0]; if ([...f.calendar.options].some(o => o.value === first)) f.calendar.value = first; }
      stRender();
    } else {
      f.date.value = date || state.date; f.end_date.value = f.date.value; f.from.value = '09:00'; f.to.value = '10:00'; setAllDay(false); rrInit(null, null, [], []); setColor(''); alarmsInit([]); peopleInit(null, []); stRender(); linksInit([], []); $('#ed-hint').textContent = '';
      if (state.cals) { const first = state.cals.split(',')[0]; if ([...f.calendar.options].some(o => o.value === first)) f.calendar.value = first; }
    }
    locInit(); syncDot(); mode(old ? 'view' : 'edit'); const pre = serialize(); const adjusted = rulesApply(old ? 'open' : 'new'); baseline = adjusted ? pre : serialize(); updateButtons();
  }
  // `quiet`: a new form that is always on screen (the tasks page) must not take the focus when it appears
  // `only`: open a single card of an existing entry (a table cell that cannot be edited in place)
  async function openEditor(key, date, quiet, only) {
    soloEnd();
    if (key) {
      try { const r = await fetch(API + '/' + key); if (!r.ok) throw new Error(r.status); fill(await r.json()); } catch (e) { alert(T.err + ': ' + e.message); return; }
    } else fill(null, date);
    if (key && only) soloStart(only);
    ed.hidden = false; if (!key && !quiet) f.summary.focus(); dayline();
  }
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
    tplPop.innerHTML = `<div class="q">${esc(D.tTplHead)}</div>` + hits.map((h, i) => `<button type="button" class="lk-hit" role="option" data-h="${i}" aria-selected="false"><svg class="ico" style="color:${esc(h.color)}"><use href="#i-calendar"/></svg><span>${esc(h.summary)}</span><small>${esc([D.tTplCount.replace('{n}', h.count), D.tTplLast.replace('{date}', fmtD(h.last) + (h.time ? ' ' + h.time : '')), h.cal_name].filter(Boolean).join(' · '))}</small></button>`).join('');
    document.body.appendChild(tplPop);
    const r = f.summary.getBoundingClientRect(), box = f.summary.closest('.box'), maxRight = (box ? box.getBoundingClientRect().right : innerWidth) - 8;
    tplPop.style.left = `${Math.max(8, Math.min(r.left, maxRight - tplPop.offsetWidth)) + scrollX}px`; tplPop.style.top = `${r.bottom + scrollY + 4}px`;
    tplPop.addEventListener('mousedown', e => e.preventDefault()); // the title keeps the focus
    tplPop.addEventListener('click', e => { const b = e.target.closest('.lk-hit'); if (b) tplUse(tplHits[+b.dataset.h]); });
  }
  f.summary.addEventListener('input', () => {
    const q = f.summary.value.trim(); tplClose(); if (!tplOn() || q.length < 2) return;
    const n = tplReq; tplTimer = setTimeout(async () => { let hits = []; try { const r = await fetch('/api/events/suggest?q=' + encodeURIComponent(q)); if (r.ok) hits = await r.json(); } catch (_) {} if (n === tplReq && tplOn() && document.activeElement === f.summary) tplShow(hits); }, 200);
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
    let d; try { const r = await fetch(API + '/' + h.key); if (!r.ok) throw new Error(r.status); d = await r.json(); } catch (e) { $('#ed-error').textContent = T.err + ': ' + e.message; return; }
    const day = f.date.value || state.date, delta = Math.round((parse(day) - parse(d.start.slice(0, 10))) / 864e5), move = s => s && addDays(s.slice(0, 10), delta) + s.slice(10);
    const links = await Promise.all(d.links.map(async l => { if (!l.embedded) return l; try { return { ...l, data: await b64Of(`${API}/${h.key}/attach/${l.bin_index}`), bin_index: null, keep: false }; } catch (_) { return null; } }));
    const b0 = baseline, cal = f.calendar.value;
    fill({ ...d, start: move(d.start), end: move(d.end), rrule: d.rrule ? d.rrule.split(';').filter(p => !/^UNTIL=/i.test(p)).join(';') : null, rrule_until: null, exdates: [], rdates: [],
      attendees: d.attendees.map(p => ({ ...p, status: p.status ? 'NEEDS-ACTION' : '' })), alarms: d.alarms.map(a => ({ ...a, idx: null, at: move(a.at), acknowledged: null })), links: links.filter(Boolean) }, day, true);
    if (!f.calendar.value) { f.calendar.value = cal; syncDot(); } // the old calendar is gone: the one chosen stays
    baseline = b0; updateButtons(); f.summary.focus(); dayline();
  }
  /* solo: the dialog shows one card, already in edit mode, and saves the whole entry as usual. A
     category rule can fill other cards (location, reminders …): every card whose content changed
     since the dialog opened is shown too, so nothing is written unseen. */
  const cardSig = c => $$('input:not([type=hidden]), select, textarea', c).map(e => e.type === 'checkbox' ? String(e.checked) : e.value).join('|') + '|' + $$('.chip, .pchip, .rows .row', c).map(x => x.textContent).join(',');
  function soloCards() {
    if (!solo) return;
    $$('.card', form).forEach(c => {
      const own = c.dataset.card === solo, moved = soloSigs[c.dataset.card] !== undefined && cardSig(c) !== soloSigs[c.dataset.card];
      if (own || moved) c.hidden = false;
      c.classList.toggle('solo-on', own || moved);
    });
  }
  function soloStart(card) {
    solo = card; mode('edit');
    const c = $(`.card[data-card="${card}"]`, form); if (c) { c.dataset.added = '1'; c.hidden = false; }
    soloSigs = {}; $$('.card', form).forEach(x => { soloSigs[x.dataset.card] = cardSig(x); });
    f.summary.readOnly = true; ed.classList.add('solo'); soloCards();
    const first = c && ($('input:not([type=hidden]):not([readonly]), textarea', c) || $('select', c)); if (first) setTimeout(() => first.focus(), 0);
  }
  function soloEnd() { if (!solo) return; solo = null; f.summary.readOnly = false; ed.classList.remove('solo'); $$('.card.solo-on', form).forEach(c => c.classList.remove('solo-on')); }
  form.addEventListener('input', refresh);
  form.addEventListener('change', e => { const t = e.target; if (t === f.calendar) syncDot(); if (t === f.date) { if (f.end_date.value < f.date.value) f.end_date.value = f.date.value; dayline(); } if (t === f.date || t === f.from || t === f.all_day) { rrFocus = null; rrRender(); } if (!ruleMode && CATS.length && !t.dataset.rv) rulesMark(); refresh(); });
  form.addEventListener('click', async e => {
    const b = e.target.closest('button'); if (!b || !form.contains(b)) return;
    if (b.dataset.add) { const l = b.dataset.add; const r = l === 'alarms' ? alarmAdd() : addDate(l, ''); const i = $('input:not([hidden]), select', r); if (i) i.focus(); refresh(); return; }
    if (b.hasAttribute('data-rm')) { b.closest('.row').remove(); refresh(); return; }
    if (b.dataset.cardAdd) { const c = addCard(b.dataset.cardAdd); b.closest('.addmenu').classList.remove('open'); if (ruleMode) { if (b.dataset.cardAdd === 'where') RM.location = 'default'; if (b.dataset.cardAdd === 'repeat') RM.rrule = 'default'; rulesApply('change'); } /* a one-setting rule card starts as template */ if (b.dataset.cardAdd === 'alarm') $('[data-add=alarms]', c).click(); else if (b.dataset.cardAdd === 'links') $('#ed-lk-url').focus(); else if (b.dataset.cardAdd === 'where') { locInit(); locin.focus(); } else if (b.dataset.cardAdd === 'repeat') { rrFocus = null; rrRender(); } else { const i = $('input:not([type=hidden]), select, textarea', c); if (i) i.focus(); } refresh(); return; }
    if (b.hasAttribute('data-card-rm')) { if (!await ask(D.tRmcard)) return; const c = b.closest('.card'); $$('input:not([type=hidden]), textarea', c).forEach(i => i.value = ''); $$('select.dat', c).forEach(s => s.value = ''); $$('.rows .row, .tags .chip', c).forEach(x => x.remove()); if (c.dataset.card === 'where') locInit(); if (c.dataset.card === 'alarm') alarmsInit([]); if (c.dataset.card === 'repeat') rrInit(null, null, [], []); if (c.dataset.card === 'status') stRender(); if (c.dataset.card === 'links') linksInit([], []); if (c.dataset.card === 'note') { ntRender(); ntFound.innerHTML = ''; } if (c.dataset.card === 'cats') { catsInit([]); rulesApply('change'); } if (ruleMode) { SCALARS.filter(k => FLD[k][0] === c.dataset.card).forEach(k => { RM[k] = 'off'; }); rulesApply('change'); } /* a removed rule card is off */ delete c.dataset.added; c.hidden = true; cards(); refresh(); return; }
    if (b.dataset.geo === 'swap') { locSwap(); return; }
    if (b.dataset.geo === 'map') { openMap(); return; }
    if (b.dataset.act) { const row = b.closest('.row'); const v = row ? (w(row, 'email') || w(row, 'uri')).value : f.location.value, open_ = u => window.open(u, '_blank', 'noopener'), g = geoOf(); if (b.dataset.act === 'osm') open_(g ? `https://www.openstreetmap.org/?mlat=${g[0]}&mlon=${g[1]}#map=17/${g[0]}/${g[1]}` : 'https://www.openstreetmap.org/search?query=' + encodeURIComponent(v)); else if (b.dataset.act === 'gmaps') open_('https://www.google.com/maps/search/?api=1&query=' + encodeURIComponent(g ? g.join(',') : v)); else if (b.dataset.act === 'mailto') location.href = 'mailto:' + v; else if (b.dataset.act === 'open') open_(/^[a-z]+:/i.test(v) ? v : 'https://' + v); else navigator.clipboard.writeText(v).then(() => { b.title = D.tCopied; b.style.color = 'var(--accent)'; setTimeout(() => b.style.color = '', 800); }); }
  });
  form.addEventListener('click', e => { const x = e.target.closest('.chip b'); if (x && form.contains(x)) { x.closest('.chip').remove(); refresh(); } });
  $('#ed-lock').onclick = () => { mode('edit'); f.summary.focus(); };
  $('#ed-reset').onclick = () => { if (ruleMode) openRuleEditor(ruleMode); else fill(current, openDate); };
  async function dayline() {
    const box = $('#ed-dayline'); const day = f.date ? f.date.value : ''; if (!box || !day) return;
    let h = ''; for (let i = 0; i < 7; i++) h += `<span class="h" style="top:${(i * 14.3).toFixed(1)}%">${7 + i * 2}:00</span>`;
    try {
      const nd = parse(day); nd.setDate(nd.getDate() + 1); const end = `${nd.getFullYear()}-${pad(nd.getMonth() + 1)}-${pad(nd.getDate())}`;
      const r = await fetch(`/api/events?start=${day}&end=${end}`); const evs = await r.json();
      for (const ev of evs) { if (ev.all_day || ev.start.slice(0, 10) !== day) continue; const s = ev.start.slice(11, 16).split(':'), e2 = ev.end.slice(11, 16).split(':'); const top = ((+s[0] + s[1] / 60) - 7) / 14 * 100, hh = Math.max(3, ((+e2[0] + e2[1] / 60) - (+s[0] + s[1] / 60)) / 14 * 100); const me = f.key.value === `${ev.principal}/${ev.cal_id}/${ev.object_id}`; h += `<div class="blk${me ? ' me' : ''}" style="top:${top.toFixed(1)}%;height:${hh.toFixed(1)}%;--c:${ev.color}">${ev.summary}</div>`; }
    } catch (_) {}
    box.innerHTML = h;
  }
  form.addEventListener('submit', async e => {
    e.preventDefault();
    if (ruleMode) { await ruleSave(); return; }
    const body = TASK ? taskBody() : eventBody();
    const key = f.key.value;
    try {
      const r = await fetch(key ? API + '/' + key : API, { method: key ? 'PUT' : 'POST', headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf }, body: JSON.stringify(body) });
      if (!r.ok) { $('#ed-error').textContent = (r.status === 409 ? T.conflict : T.err + ' ' + r.status) + ': ' + (await r.text()).slice(0, 200); return; }
      const res = await r.json().catch(() => ({}));
      const saved = key || (res.id && body.principal && body.cal_id ? `${body.principal}/${body.cal_id}/${res.id}` : null);
      ntInvalidate(); ctInvalidate(); close(); load(false, saved);
    } catch (e2) { $('#ed-error').textContent = T.err + ': ' + e2.message; }
  });
  $('#ed-delete').onclick = async () => { if (ruleMode) { if (await ask(T.delRule)) await ruleDelete(); return; } if (!await ask(TASK ? T.delTask : T.del)) return; const r = await fetch(API + '/' + f.key.value, { method: 'DELETE', headers: { 'X-CSRF-Token': csrf } }); if (r.ok) { close(); load(false); } else $('#ed-error').textContent = T.err + ' ' + r.status; };

  window.rusticalEd = { open: openEditor, openRule: openRuleEditor };
  { const sp = new URLSearchParams(location.search), rp = sp.get('rule'); if (rp) { ruleBack = sp.get('back') || null; openRuleEditor(rp); } }
})();


/* ===== universal search =====
   The drop-down shows at most 5 hits per category and 10 in all, shared by each category's share of
   the matches (the server ranks them, domain::search); "all results" (Enter without a pick) opens a
   dialog with every hit, filtered by category. Finished searches — a result opened, or Enter — go to
   the history (settings → search), with the result last opened: the drop-down offers matching ones at
   its foot, and completes the query from them (grey behind the caret; Tab goes straight to that
   result). A pick keeps the view on screen where it can show it (calendar, contacts, tasks). */
(function () {
  const q = document.getElementById('q'), box = document.getElementById('sres'), ghost = document.getElementById('q-ghost');
  if (!q) return;
  const $ = (s, r = document) => r.querySelector(s), $$ = (s, r = document) => [...r.querySelectorAll(s)];
  const csrf = (document.querySelector('meta[name=csrf]') || {}).content || '';
  const T = trs('js-search-', ['jump', 'ev', 'ct', 'tk', 'none', 'all', 'of', 'hist', 'histRm', 'dlg', 'tabAll', 'close', 'keys', 'only', 'series', 'why']);
  const esc = s => String(s ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
  const ico = n => `<svg class="ico" aria-hidden="true"><use href="#i-${n}"/></svg>`;
  // the server's fold (domain::search::fold): case and diacritics off, for highlighting and history matches
  const FOLD = { 'ß': 'ss', 'æ': 'ae', 'œ': 'oe', 'ø': 'o', 'ł': 'l' };
  const foldc = c => FOLD[c] ?? c.normalize('NFD').replace(/[\u0300-\u036f]/g, '');
  const fold = s => [...String(s).toLowerCase()].map(foldc).join('');
  const SCOPE = /^\s*[^\s:]+:\s*/;
  const wordsOf = (v, scoped) => fold(scoped ? v.replace(SCOPE, '') : v).split(/[^\p{L}\p{N}]+/u).filter(w => w.length >= 2);
  function marksOf(text, ws) { // which characters of `text` the query's words match (typos are not marked)
    const on = new Array(text.length).fill(false); if (!ws.length) return on;
    let f = '', g = ''; const map = [], gmap = []; // g: letters and digits only, for words typed without spaces
    for (let i = 0; i < text.length; i++) for (const x of foldc(text[i].toLowerCase())) { f += x; map.push(i); if (/[\p{L}\p{N}]/u.test(x)) { g += x; gmap.push(i); } }
    const mark = (hay, m, w) => { let hit = false; for (let p = hay.indexOf(w); p >= 0; p = hay.indexOf(w, p + w.length)) { hit = true; for (let j = p; j < p + w.length; j++) on[m[j]] = true; } return hit; };
    // starts of consecutive words ("dietstein"): the server's starts_chain, marking each part
    const W = []; for (const m of f.matchAll(/[\p{L}\p{N}]+/gu)) W.push({ s: m[0], at: m.index });
    const chain = (w, k, first) => { const x = W[k]; if (!x) return null; let c = 0; while (c < w.length && c < x.s.length && w[c] === x.s[c]) c++;
      for (let l = c; l >= 2; l--) { if (l === w.length) { if (!first && l >= 3) return [[x.at, l]]; continue; } const r = chain(w.slice(l), k + 1, false); if (r) return [[x.at, l], ...r]; } return null; };
    // letters left out ("chrstinbach"): the server's gaps_span — in order from a word's start, over at most twice the word
    const gaps = w => { let best = null, pos = 0; const gs = W.map(x => { const at = pos; pos += x.s.length; return at; });
      for (const st of gs) { if (g[st] !== w[0]) continue; const hit = []; let i = st; for (const ch of w) { while (i < g.length && g[i] !== ch) i++; if (i >= g.length) break; hit.push(i++); }
        if (hit.length === w.length && hit[hit.length - 1] - st < 2 * w.length && (!best || hit[hit.length - 1] - st < best[best.length - 1] - best[0])) best = hit; }
      if (best) best.forEach(j => { on[gmap[j]] = true; }); return !!best; };
    for (const w of ws) { if (mark(f, map, w) || mark(g, gmap, w)) continue; let done = false; if (w.length >= 5) for (let k = 0; k < W.length - 1 && !done; k++) { const r = chain(w, k, true); if (r) { r.forEach(([at, l]) => { for (let j = at; j < at + l; j++) on[map[j]] = true; }); done = true; } } if (!done && w.length >= 6) gaps(w); }
    return on;
  }
  function paintMarks(text, on, a = 0, b = text.length) {
    let h = '', open = false;
    for (let i = a; i < b; i++) { if (on[i] !== open) { h += on[i] ? '<mark>' : '</mark>'; open = on[i]; } h += esc(text[i]); }
    return h + (open ? '</mark>' : '');
  }
  const hl = (text, ws) => { text = String(text ?? ''); return paintMarks(text, marksOf(text, ws)); };
  // a long text cut to about `max` characters around its first match
  function snip(text, ws, max = 90) {
    text = String(text ?? '').replace(/\s+/g, ' ').trim(); const on = marksOf(text, ws);
    if (text.length <= max) return paintMarks(text, on);
    const first = Math.max(0, on.indexOf(true)), a = Math.max(0, Math.min(first - 30, text.length - max)), b = Math.min(text.length, a + max);
    return (a ? '…' : '') + paintMarks(text, on, a, b) + (b < text.length ? '…' : '');
  }
  /* scrolling a list moves its rows under a resting pointer: every row it passes would repaint its hover
     tint and move the pick — measured as nearly all the paint work while scrolling. While a list scrolls
     its rows ignore the pointer (`.scrolling`), 150 ms after the last scroll they react again. */
  function calmScroll(list) { let t; list.addEventListener('scroll', () => { list.classList.add('scrolling'); clearTimeout(t); t = setTimeout(() => list.classList.remove('scrolling'), 150); }, { passive: true }); }
  // the pick moves between two rows only; the keyboard's pick scrolls it into view, the pointer's does not
  function movePick(list, i, scroll) { const old = $('.r.on', list); if (old) old.classList.remove('on'); const el = $(`[data-i="${i}"]`, list); if (el) { el.classList.add('on'); if (scroll) el.scrollIntoView({ block: 'nearest' }); } }
  const day = iso => { const [y, m, d] = iso.split('-').map(Number); return dfmt(new Date(y, m - 1, d), { weekday: 'short', day: 'numeric', month: 'short', year: 'numeric' }); };

  // ---- fetching: debounced, the stale request aborted, answers kept 30 s for back-typing ----
  const cache = new Map();
  let ctl = null;
  async function fetchQ(v, all, signal) {
    const k = (all ? '1' : '0') + v, c = cache.get(k);
    if (c && Date.now() - c.t < 30000) return c.d;
    const r = await fetch('/api/search?q=' + encodeURIComponent(v) + (all ? '&all=1' : ''), { signal });
    const d = await r.json(); cache.set(k, { t: Date.now(), d }); if (cache.size > 40) cache.delete(cache.keys().next().value);
    return d;
  }

  // ---- history (domain::search_history): loaded once, then kept here ----
  let H = null;
  const loadH = async () => { if (H) return H; try { H = await (await fetch('/api/search/history')).json(); } catch (_) { H = []; } return H; };
  function record(v, target) {
    v = v.trim(); if (fold(v).length < 2) return;
    fetch('/api/search/history', { method: 'POST', keepalive: true, headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf }, body: JSON.stringify({ q: v, target: target || null }) })
      .then(r => r.json()).then(on => { if (!on || !H) return; const n = fold(v), old = H.find(e => fold(e.q) === n); H = H.filter(e => e !== old); H.unshift({ id: old ? old.id : 0, q: v, target: target || (old && old.target) || null, uses: (old ? old.uses : 0) + 1 }); if (!old) H = null; }).catch(() => {});
  }
  async function forget(id) { await fetch('/api/search/history/' + id, { method: 'DELETE', headers: { 'X-CSRF-Token': csrf } }); if (H) H = H.filter(e => e.id !== id); }
  const histFor = v => { const n = fold(v.trim()); return (H || []).filter(e => { const f = fold(e.q); return !n || (f !== n && (f.startsWith(n) || f.includes(n))); }); };

  // ---- one hit → markup + what a pick does ----
  const KEEP = ['day', 'week', 'month', 'planner'];
  function targetOf(kind, x) {
    if (kind === 'jump') return { kind, date: x.date, view: x.view, label: x.label };
    if (kind === 'event') return { kind, key: x.key, date: x.date, label: x.summary };
    if (kind === 'contact') return { kind, key: x.key, label: x.name };
    return { kind: 'task', key: x.key, done: !!x.completed, label: x.summary };
  }
  function open(t) { // a result: the view on screen keeps it where it can (calendar/contacts/tasks), else its page
    if (t.kind === 'jump' || t.kind === 'event') {
      const view = t.kind === 'jump' ? t.view : 'day';
      if (window.rusticalCal) { const cur = window.rusticalCal.view(); window.rusticalCal.go(KEEP.includes(cur) ? cur : view, t.date); } else location.href = `/calendar?view=${view}&date=${t.date}`;
    } else if (t.kind === 'contact') { if (window.rusticalContacts) { history.replaceState(null, '', '#' + t.key); window.rusticalContacts.reveal(t.key); } else location.href = '/contacts#' + t.key; }
    else if (window.rusticalTasks) window.rusticalTasks.reveal(t.key); else location.href = `/tasks?view=groups${t.done ? '&back=all' : ''}#${encodeURIComponent(t.key)}`;
  }
  // why a hit was found (domain::search Why): per field its text around the match and how each word matched
  const how = h => h.k === 'typo' ? tr('js-search-k-typo', { m: h.m }) : tr('js-search-k-' + h.k);
  const typos = x => (x.why || []).flatMap(w => w.hits.filter(h => h.m).map(h => fold(h.m)));
  const why = (x, ws) => x.why && x.why.length ? `<span class="why"><b>${esc(T.why)}</b>${x.why.map(w => `<span><em>${esc(tr('js-search-f-' + w.f))}:</em> ${snip(w.s, ws.concat(typos(x)))} <i>(${esc(w.hits.map(h => (h.k === 'date' ? '' : '„' + h.w + '“ ') + how(h)).join(', '))})</i></span>`).join('')}</span>` : '';
  // the drop-down's short form: only what the row does not show — a field it hides, a typo
  const whySm = x => {
    const loose = h => h.k === 'typo' || h.k === 'gaps';
    const v = (x.why || []).filter(w => (!w.shown || w.hits.some(loose)) && !w.hits.some(h => h.k === 'date'))
      .map(w => { const t = w.hits.find(h => h.k === 'typo'); return esc(tr('js-search-f-' + w.f)) + (t ? ' ≈ ' + esc(t.m) : w.hits.some(loose) ? ' ≈' : ''); });
    return v.length ? `<span class="why sm">${ico('info')}${v.join(' · ')}</span>` : '';
  };
  function row(kind, x, ws) {
    if (kind === 'jump') return `<span class="ic">⇢</span><b>${esc(T.jump)} ${esc(x.label)}</b><small>${day(x.date)}</small>`;
    if (kind === 'event') return `<span class="sw" style="--c:${esc(x.color)}"></span><b>${hl(x.summary, ws)}</b><small>${day(x.date)}${x.time ? ' · ' + esc(x.time) : ''} · ${esc(x.cal_name)}${x.count > 1 ? ' · ' + ico('repeat') + esc(tr('js-search-series', { n: x.count })) : ''}</small>`;
    if (kind === 'contact') return `<span class="av" style="background:var(--c${x.color_index})">${esc(x.initials)}</span><b>${hl(x.name, ws)}</b><small>${x.bday ? ico('cake') : ''}${hl(x.sub, ws)}</small>`;
    return `<span class="sw" style="--c:${esc(x.color)}"></span><b>${hl(x.summary, ws)}</b><small>${x.due ? day(x.due) + ' · ' : ''}${esc(x.cal_name)}</small>`;
  }
  const HREF = t => t.kind === 'jump' || t.kind === 'event' ? `/calendar?view=${t.kind === 'jump' ? t.view : 'day'}&date=${t.date}` : t.kind === 'contact' ? '/contacts#' + t.key : '/tasks#' + t.key;
  const CATS = [['event', 'events', T.ev], ['contact', 'contacts', T.ct], ['task', 'tasks', T.tk]];
  // the drop-down's share: at most `cap` per category and `max` in all, each category by its share of the matches (at least one)
  function alloc(tot, cap = 5, max = 10) {
    const ks = Object.keys(tot), want = Object.fromEntries(ks.map(k => [k, Math.min(cap, tot[k])]));
    if (ks.reduce((a, k) => a + want[k], 0) <= max) return want;
    const all = ks.reduce((a, k) => a + tot[k], 0), out = {}; let used = 0;
    ks.forEach(k => { out[k] = tot[k] ? Math.min(want[k], Math.max(1, Math.floor(max * tot[k] / all))) : 0; used += out[k]; });
    while (used < max) { let best = null, bd = -Infinity; for (const k of ks) if (out[k] < want[k]) { const d = max * tot[k] / all - out[k]; if (d > bd) { bd = d; best = k; } } if (!best) break; out[best]++; used++; }
    return out;
  }

  // ---- the drop-down ----
  let items = [], sel = -1, last = null, data = null;
  const setSel = (i, scroll = true) => { sel = i; movePick(box, i, scroll); };
  calmScroll(box);
  function draw() {
    const v = q.value, d = data, ws = d ? wordsOf(v, d.scope) : [];
    items = []; let h = '', allSum = null;
    const add = (it, inner, cls = '') => { items.push(it); return `<a class="r${cls}" href="${esc(it.href || '#')}" data-i="${items.length - 1}" role="option">${inner}</a>`; };
    if (d && v.trim()) {
      if (d.scope) { const c = CATS.find(c => c[0] === d.scope); h += `<div class="sscope">${ico('filter')}${esc(tr('js-search-only', { what: c[2] }))}</div>`; }
      if (d.jump) { const t = targetOf('jump', d.jump); h += add({ t, href: HREF(t) }, row('jump', d.jump, ws), ' jump'); }
      const tot = Object.fromEntries(CATS.map(([, k]) => [k, d.totals[k]])), n = alloc(tot);
      for (const [kind, k, label] of CATS) {
        if (!n[k]) continue;
        h += `<h5>${esc(label)}<span>${n[k] < tot[k] ? esc(tr('js-search-of', { n: n[k], total: tot[k] })) : tot[k]}</span></h5>`;
        d[k].slice(0, n[k]).forEach(x => { const t = targetOf(kind, x); h += add({ t, href: HREF(t) }, row(kind, x, ws.concat(typos(x))) + whySm(x), x.completed ? ' done' : ''); });
      }
      const sum = CATS.reduce((a, [, k]) => a + tot[k], 0);
      if (!sum && !d.jump) h += `<div class="none">${esc(T.none)}</div>`;
      allSum = sum;
    }
    const hs = histFor(v).slice(0, v.trim() ? 3 : 6);
    if (hs.length) {
      h += `<h5>${esc(T.hist)}</h5>`;
      hs.forEach(e => { h += add({ hist: e }, `<span class="ic">${ico('history')}</span><b>${hl(e.q, wordsOf(v))}</b><small>${e.target ? esc(e.target.label || '') : ''}</small><button type="button" class="hx" data-hdel="${e.id}" title="${esc(T.histRm)}" aria-label="${esc(T.histRm)}">${ico('x')}</button>`, ' hist'); });
    }
    // always there with a query, pinned to the foot: the dialog also says why each hit was found
    if (allSum !== null) h += add({ all: true }, `<span class="ic">${ico('list')}</span><b>${esc(T.all)} (${allSum})</b><small>Enter</small>`, ' more');
    box.innerHTML = h; box.hidden = !h; sel = -1;
    drawGhost();
  }
  function drawGhost() { // the newest history entry the input is the start of
    const v = q.value, at = q.selectionStart === v.length;
    const e = v && at && (H || []).find(e => e.q.length > v.length && e.q.toLowerCase().startsWith(v.toLowerCase()));
    ghost.innerHTML = e ? `<span class="t">${esc(v)}</span>${esc(e.q.slice(v.length))}` : ''; ghost.dataset.i = e ? H.indexOf(e) : '';
  }
  async function refresh() {
    const v = q.value.trim(); await loadH();
    if (!v) { data = null; last = null; draw(); return; }
    if (v === last) { draw(); return; }
    if (ctl) ctl.abort(); ctl = new AbortController(); const mine = ctl;
    try { const d = await fetchQ(v, false, mine.signal); if (mine !== ctl) return; data = d; last = v; draw(); } catch (_) { /* aborted or offline: keep what is shown */ }
  }
  function pick(it, from) {
    if (!it) return;
    if (it.all) { openAll(from); return; }
    if (it.hist) { if (it.hist.target) { record(it.hist.q, it.hist.target); close(); q.value = it.hist.q; open(it.hist.target); } else { q.value = it.hist.q; q.focus(); refresh(); } return; }
    record(from, it.t); close(); open(it.t);
  }
  const close = () => { box.hidden = true; ghost.innerHTML = ''; };
  let t;
  q.addEventListener('input', () => { drawGhost(); clearTimeout(t); t = setTimeout(refresh, 180); });
  q.addEventListener('focus', () => { refresh(); });
  box.addEventListener('mousedown', e => e.preventDefault()); // a click keeps the focus in the field
  box.addEventListener('click', async e => {
    const x = e.target.closest('[data-hdel]'); if (x) { e.preventDefault(); e.stopPropagation(); await forget(+x.dataset.hdel); draw(); return; }
    const a = e.target.closest('[data-i]'); if (!a) return; e.preventDefault(); pick(items[+a.dataset.i], q.value);
  });
  box.addEventListener('mousemove', e => { const a = e.target.closest('[data-i]'); if (a && +a.dataset.i !== sel) setSel(+a.dataset.i, false); });
  q.addEventListener('keydown', e => {
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') { if (box.hidden) { refresh(); return; } e.preventDefault(); const n = items.length; if (n) setSel(e.key === 'ArrowDown' ? (sel + 1) % n : (sel <= 0 ? n : sel) - 1); }
    else if (e.key === 'Enter') { e.preventDefault(); if (sel >= 0) pick(items[sel], q.value); else if (q.value.trim()) { record(q.value); openAll(q.value); } }
    else if (e.key === 'Tab' && !e.shiftKey && ghost.dataset.i !== '' && ghost.innerHTML) { e.preventDefault(); const h = H[+ghost.dataset.i]; if (h) pick({ hist: h }, h.q); }
    else if (e.key === 'ArrowRight' && ghost.innerHTML && q.selectionStart === q.value.length) { e.preventDefault(); q.value = H[+ghost.dataset.i].q; refresh(); } // → takes the completion without going
    else if (e.key === 'Escape') { if (!box.hidden) { e.stopPropagation(); close(); } else q.blur(); }
  });
  q.addEventListener('blur', () => setTimeout(close, 120));
  document.addEventListener('click', e => { if (!e.target.closest('.usearch')) close(); });
  // / and Ctrl/⌘ K put the cursor in the field
  addEventListener('keydown', e => { if ((e.key === 'k' && (e.metaKey || e.ctrlKey)) && !e.altKey) { e.preventDefault(); q.focus(); q.select(); } });

  // ---- "all results": every hit, filtered by category ----
  function openAll(v) {
    close();
    const w = document.createElement('div'); w.className = 'ask sdlg-wrap';
    w.innerHTML = `<div class="ask-box sdlg" role="dialog" aria-modal="true" aria-label="${esc(T.dlg)}"><div class="sd-head"><div class="sfield"><input type="search" autocomplete="off" spellcheck="false" aria-label="${esc(T.dlg)}"></div><button class="btn icon-btn" type="button" data-x title="${esc(T.close)}" aria-label="${esc(T.close)}">${ico('x')}</button></div><div class="seg sd-tabs" role="tablist"></div><div class="sres sd-list" role="listbox"></div><p class="sd-foot">${esc(T.keys)}</p></div>`;
    document.body.appendChild(w);
    const inp = $('input', w), tabs = $('.sd-tabs', w), list = $('.sd-list', w);
    let d = null, cat = 'all', its = [], s = -1, t2, c2 = null;
    inp.value = v;
    const mark = (i, scroll = true) => { s = i; movePick(list, i, scroll); };
    calmScroll(list);
    function paint() {
      if (!d) return; const ws = wordsOf(inp.value, d.scope); its = []; let h = '';
      const add = (it, inner, cls = '') => { its.push(it); return `<a class="r${cls}" href="${esc(it.href)}" data-i="${its.length - 1}" role="option">${inner}</a>`; };
      const sum = CATS.reduce((a, [, k]) => a + d.totals[k], 0);
      tabs.innerHTML = `<button type="button" data-cat="all" aria-pressed="${cat === 'all'}">${esc(T.tabAll)} <span class="n">${sum}</span></button>` + CATS.map(([kind, k, label]) => `<button type="button" data-cat="${kind}" aria-pressed="${cat === kind}"${d.totals[k] ? '' : ' disabled'}>${esc(label)} <span class="n">${d.totals[k]}</span></button>`).join('');
      if (d.jump && cat === 'all') { const t = targetOf('jump', d.jump); h += add({ t, href: HREF(t) }, row('jump', d.jump, ws), ' jump'); }
      for (const [kind, k, label] of CATS) {
        if ((cat !== 'all' && cat !== kind) || !d[k].length) continue;
        h += `<h5>${esc(label)}<span>${d[k].length < d.totals[k] ? esc(tr('js-search-of', { n: d[k].length, total: d.totals[k] })) : d.totals[k]}</span></h5>`;
        d[k].forEach(x => { const t = targetOf(kind, x); h += add({ t, href: HREF(t) }, row(kind, x, ws.concat(typos(x))) + why(x, ws), x.completed ? ' done' : ''); });
      }
      list.innerHTML = h || `<div class="none">${esc(T.none)}</div>`; s = -1;
    }
    async function load() {
      const v2 = inp.value.trim(); if (!v2) { d = { totals: { events: 0, contacts: 0, tasks: 0 }, events: [], contacts: [], tasks: [] }; paint(); return; }
      if (c2) c2.abort(); c2 = new AbortController(); const mine = c2;
      try { const r = await fetchQ(v2, true, mine.signal); if (mine !== c2) return; d = r; if (cat !== 'all' && !d.totals[CATS.find(c => c[0] === cat)[1]]) cat = 'all'; paint(); } catch (_) {}
    }
    const end = () => { removeEventListener('keydown', key, true); w.remove(); q.focus(); };
    const choose = it => { if (!it) return; record(inp.value, it.t); end(); q.value = inp.value; close(); open(it.t); };
    const key = e => {
      if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); end(); }
      else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') { e.preventDefault(); const n = its.length; if (n) mark(e.key === 'ArrowDown' ? (s + 1) % n : (s <= 0 ? n : s) - 1); }
      else if (e.key === 'Enter' && document.activeElement === inp) { e.preventDefault(); choose(its[s >= 0 ? s : 0]); }
    };
    addEventListener('keydown', key, true);
    inp.addEventListener('input', () => { clearTimeout(t2); t2 = setTimeout(load, 180); });
    tabs.addEventListener('click', e => { const b = e.target.closest('[data-cat]'); if (!b || b.disabled) return; cat = b.dataset.cat; paint(); inp.focus(); });
    list.addEventListener('mousedown', e => e.preventDefault());
    list.addEventListener('click', e => { const a = e.target.closest('[data-i]'); if (!a) return; e.preventDefault(); choose(its[+a.dataset.i]); });
    list.addEventListener('mousemove', e => { const a = e.target.closest('[data-i]'); if (a && +a.dataset.i !== s) mark(+a.dataset.i, false); });
    w.addEventListener('click', e => { if (e.target === w || e.target.closest('[data-x]')) end(); });
    inp.focus(); inp.select(); load();
  }
})();

/* ===== contacts page ===== */
(function () {
  const wrap = document.getElementById('contacts');
  if (!wrap) return;
  const $ = (s, r = document) => r.querySelector(s), $$ = (s, r = document) => [...r.querySelectorAll(s)];
  const csrf = document.querySelector('meta[name=csrf]').content;
  const lang = document.body.dataset.lang || 'de';
  const T = { ...trs('js-ct-', ['del', 'err', 'phone', 'email', 'create', 'save', 'reset', 'cancel', 'delete']), kinds: ['home', 'work', 'cell', 'other'] };
  // view switch; the picked contact (activeKey) stays picked and in view
  let activeKey = null;
  $$('[data-ctview]').forEach(b => b.onclick = () => { const table = b.dataset.ctview === 'table'; wrap.classList.toggle('view-table', table); $$('#colpop [data-for]').forEach(g => g.hidden = g.dataset.for !== (table ? 'table' : 'list')); segPress(b); localStorage.ctview = b.dataset.ctview; if (activeKey) reveal(activeKey); });
  if (localStorage.ctview === 'table' && !wrap.classList.contains('view-table') && location.hash.length <= 1) $('[data-ctview=table]').click(); // a link to one contact (#key) opens the list
  // columns
  function applyCols() { const on = $$('#colpop input[data-col]').filter(i => i.checked).map(i => i.dataset.col); $$('.sheet [data-c]').forEach(el => el.classList.toggle('off', !on.includes(el.dataset.c))); return on; }
  $$('#colpop input[data-col]').forEach(i => i.addEventListener('change', () => { const on = applyCols(); fetch('/settings/columns', { method: 'POST', headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf }, body: JSON.stringify(on) }); }));
  applyCols();
  // list view: the same picker toggles the lines of every entry
  function applyListFields() { const on = $$('#colpop input[data-lf]').filter(i => i.checked).map(i => i.dataset.lf); $$('.panels [data-lf]').forEach(el => el.classList.toggle('off', !on.includes(el.dataset.lf))); return on; }
  $$('#colpop input[data-lf]').forEach(i => i.addEventListener('change', () => { const on = applyListFields(); fetch('/settings/list-fields', { method: 'POST', headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf }, body: JSON.stringify(on) }); }));
  applyListFields(); $$('#colpop [data-for]').forEach(g => g.hidden = g.dataset.for !== (wrap.classList.contains('view-table') ? 'table' : 'list'));
  // filter by addressbook / category
  const fb = $('#abfilter');
  function applyFilter() {
    const abs = $$('input[data-ab]', fb).filter(i => !i.checked).map(i => i.dataset.ab); const cats = $$('input[data-cat]', fb).filter(i => !i.checked).map(i => i.dataset.cat);
    $$('[data-key][data-ab]', wrap).forEach(el => el.classList.toggle('hide', abs.includes(el.dataset.ab) || cats.includes(el.dataset.cat)));
    $$('.lpanel', wrap).forEach(p => p.classList.toggle('hide', !$$('li:not(.hide)', p).length));
    $$('.ct-rail [data-jump]', wrap).forEach(b => { const p = $(`.lpanel[data-letter="${CSS.escape(b.dataset.jump)}"]`, wrap); b.classList.toggle('hide', !p || p.classList.contains('hide')); });
    if (typeof railMark === 'function') setTimeout(railMark, 0);
  }
  if (fb) $$('input', fb).forEach(i => i.addEventListener('change', applyFilter));
  if (fb) $$('[data-filter]', fb).forEach(a => a.addEventListener('click', e => { e.preventDefault(); $$('input[data-ab]', fb).forEach(i => { i.checked = a.dataset.filter === 'all' || (a.dataset.filter === 'mine' && !i.parentElement.querySelector('small')); }); applyFilter(); })); // as the calendar filter
  // quick-jump rail: scroll the letter's panel (list) or first row (table) to the top of its scroller
  const scrollerOf = el => { for (let p = el.parentElement; p; p = p.parentElement) { const o = getComputedStyle(p).overflowY; if (o === 'auto' || o === 'scroll') return p; } return document.querySelector('.main'); };
  $('.ct-rail', wrap).addEventListener('click', e => { // delegated: the sort rebuilds the buttons
    const b = e.target.closest('[data-jump]'); if (!b) return;
    const L = CSS.escape(b.dataset.jump);
    const target = wrap.classList.contains('view-table') ? $(`tr[data-letter="${L}"]:not(.hide)`, wrap) : $(`.lpanel[data-letter="${L}"]`, wrap);
    if (!target) return; const sc = scrollerOf(target); const head = sc.classList.contains('main') ? ($('.toolbar') || {}).offsetHeight || 0 : ($('thead', sc) || {}).offsetHeight || 0;
    sc.scrollTop += target.getBoundingClientRect().top - sc.getBoundingClientRect().top - head - 6;
  });
  // rail: ring on the letter whose group is at the top of the scroller (list: .main, table: .sheetwrap)
  function railMark() {
    const table = wrap.classList.contains('view-table'); const sc = table ? $('.sheetwrap', wrap) : $('.main');
    const line = sc.getBoundingClientRect().top + (table ? ($('thead', sc) || {}).offsetHeight || 0 : ($('.toolbar') || {}).offsetHeight || 0) + 8;
    let cur = null;
    for (const el of $$(table ? 'tr[data-letter]:not(.hide)' : '.lpanel:not(.hide)', wrap)) { if (el.getBoundingClientRect().top <= line) cur = el.dataset.letter; else break; }
    $$('.ct-rail [data-jump]', wrap).forEach(b => b.classList.toggle('cur', b.dataset.jump === cur));
  }
  const railTick = () => { if (!railTick._q) { railTick._q = true; requestAnimationFrame(() => { railTick._q = false; railMark(); }); } };
  $('.main').addEventListener('scroll', railTick); $('.sheetwrap', wrap).addEventListener('scroll', railTick);
  $$('[data-ctview]').forEach(b => b.addEventListener('click', () => setTimeout(railMark, 0)));
  /* sort field + direction, the same box as on the tasks page. The list regroups (initial letter, or
     birth month), the table reorders, the rail follows; entries without the value stay at the end. */
  const SL = trs('js-ctsort-', ['given', 'family', 'bday', 'email', 'none']), TS = trs('js-tk-', ['sort', 'asc', 'desc']);
  const FIELDS = ['given', 'family', 'bday', 'email'];
  let sort = FIELDS.includes(localStorage.ctSort) ? localStorage.ctSort : 'given', desc = localStorage.ctDesc === '1';
  const hesc = v => String(v).replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
  const coll = new Intl.Collator(lang, { sensitivity: 'base', numeric: true });
  const MONTHS = Array.from({ length: 12 }, (_, i) => dfmt(new Date(2000, i, 1), { month: 'long' }));
  const initial = v => { const c = v.normalize('NFD').charAt(0).toUpperCase(); return /\p{L}/u.test(c) ? c : '#'; };
  const recs = $$('.panels li[data-key]', wrap).map(li => {
    const d = li.dataset, name = d.name.trim(), md = d.bday.match(/(\d{2})-(\d{2})$/);
    return { li, tr: $(`tr[data-key="${CSS.escape(d.key)}"]`, wrap), given: `${d.given || name} ${d.family}`.trim(), family: `${d.family || name} ${d.given}`.trim(), bday: md ? `${md[1]}-${md[2]}` : '', email: d.email.trim().toLowerCase() };
  });
  const groupOf = r => !r[sort] ? '-' : sort === 'bday' ? r.bday.slice(0, 2) : initial(r[sort]);
  const label = g => g === '-' ? SL.none : sort === 'bday' ? MONTHS[+g - 1] : g;
  const short = g => g === '-' ? '–' : sort === 'bday' ? MONTHS[+g - 1].slice(0, 3) : g;
  function resort() {
    const has = recs.filter(r => r[sort]), none = recs.filter(r => !r[sort]);
    has.sort((a, b) => coll.compare(a[sort], b[sort]) || coll.compare(a.given, b.given));
    if (desc) has.reverse();
    none.sort((a, b) => coll.compare(a.given, b.given));
    const groups = new Map();
    for (const r of has.concat(none)) { const g = groupOf(r); if (!groups.has(g)) groups.set(g, []); groups.get(g).push(r); }
    const panels = $('.panels', wrap), tbody = $('.sheet tbody', wrap), ph = $('.placeholder', panels);
    $$('.lpanel', panels).forEach(p => p.remove());
    let rail = '';
    for (const [g, rs] of groups) {
      const sec = document.createElement('section'); sec.className = 'lpanel'; sec.dataset.letter = g;
      sec.innerHTML = `<h3>${hesc(label(g))}</h3><ul class="list"></ul>`;
      const ul = sec.lastElementChild;
      rs.forEach(r => { ul.appendChild(r.li); if (r.tr) { r.tr.dataset.letter = g; tbody.appendChild(r.tr); } });
      panels.insertBefore(sec, ph);
      rail += `<button type="button" data-jump="${hesc(g)}" title="${hesc(label(g))}">${hesc(short(g))}</button>`;
    }
    $('.ct-rail', wrap).innerHTML = rail;
    sortSel.value = sort;
    sortDir.innerHTML = `<svg class="ico s" aria-hidden="true"><use href="#i-${desc ? 'sort-desc' : 'sort-asc'}"/></svg>`;
    sortDir.title = desc ? TS.desc : TS.asc; sortDir.setAttribute('aria-label', sortDir.title);
    applyFilter();
  }
  const sortBox = $('#ct-sortbox');
  sortBox.innerHTML = `<span class="sel" title="${hesc(TS.sort)}"><svg class="ico s" aria-hidden="true"><use href="#i-swap"/></svg><select id="ct-sort" aria-label="${hesc(TS.sort)}">${FIELDS.map(v => `<option value="${v}">${hesc(SL[v])}</option>`).join('')}</select></span><button class="btn icon-btn" id="ct-dir" type="button"></button>`;
  const sortSel = $('#ct-sort'), sortDir = $('#ct-dir');
  const resorted = () => { resort(); if (activeKey) reveal(activeKey); };
  sortSel.onchange = () => { sort = sortSel.value; localStorage.ctSort = sort; resorted(); };
  sortDir.onclick = () => { desc = !desc; localStorage.ctDesc = desc ? '1' : '0'; resorted(); };
  resort();
  railMark();
  // ---- contact editor: cards, view/edit mode (lock), per-row actions ----
  const det = $('#detail'), form = $('#ctform'), f = form.elements, D = form.dataset;
  const KEY = c => `${c.principal}/${c.ab_id}/${c.object_id}`;
  let current = null, baseline = '', fnAuto = true;
  const wide = () => matchMedia('(min-width: 1100px)').matches;
  // partial dates: "YYYY-MM-DD" | "--MM-DD" <-> three inputs
  const setDate = (p, iso) => { const m = (iso || '').match(/^(?:(\d{4})|-)-(\d{2})-(\d{2})$/); $('[data-p=d]', p).value = m ? m[3] : ''; $('[data-p=m]', p).value = m ? m[2] : ''; $('[data-p=y]', p).value = m && m[1] ? m[1] : ''; };
  const getDate = p => { const d = $('[data-p=d]', p).value.trim(), m = $('[data-p=m]', p).value.trim(), y = $('[data-p=y]', p).value.trim(); if (!d || !m) return ''; const dd = d.padStart(2, '0'), mm = m.padStart(2, '0'); return (y ? y.padStart(4, '0') : '-') + `-${mm}-${dd}`; };
  const dateInfo = p => { const d = +$('[data-p=d]', p).value, m = +$('[data-p=m]', p).value, y = +$('[data-p=y]', p).value; const info = $('.dateinfo', p); if (!info) return; if (!d || !m) { info.textContent = ''; return; } const now = new Date(), t0 = new Date(now.getFullYear(), now.getMonth(), now.getDate()); let next = new Date(now.getFullYear(), m - 1, d); if (next < t0) next = new Date(now.getFullYear() + 1, m - 1, d); const days = Math.round((next - t0) / 864e5); info.textContent = (days === 0 ? D.tToday : D.tIn.replace('{n}', days)) + (y ? ' · ' + D.tTurns.replace('{n}', next.getFullYear() - y) : ''); };
  // list rows from <template>
  const addRow = (list, item) => { const box = $(`[data-list="${list}"]`, form); const tpl = $('template', box); box.appendChild(tpl.content.cloneNode(true)); const row = box.lastElementChild; if (item) { const sel = $('select.typ', row); if (sel && item.kind) { if (![...sel.options].some(o => o.value === item.kind)) sel.add(new Option(item.kind, item.kind)); sel.value = item.kind; } if (row.classList.contains('adr')) { ['street', 'code', 'city', 'country', 'region'].forEach(k => $(`[data-w=${k}]`, row).value = item[k] || ''); } else if (row.classList.contains('xd')) { $('.lbl', row).value = item.label || ''; setDate($('.pdate', row), item.date); dateInfo($('.pdate', row)); } else { $('input', row).value = item.value || ''; } const pref = $('.pref', row); if (pref) pref.setAttribute('aria-pressed', item.pref ? 'true' : 'false'); } return row; };
  const rows = list => [...$$(`[data-list="${list}"] .row`, form)];
  const rowVals = list => rows(list).map(r => { const p = $('.pref', r); return { kind: $('select.typ', r).value, value: $('input:not([type=hidden])', r).value.trim(), pref: !!p && p.getAttribute('aria-pressed') === 'true' }; }).filter(v => v.value);
  const chips = () => $$('.tags .chip', form).map(c => c.firstChild.textContent.trim());
  const addChip = v => { v = v.replace(/,/g, '').trim(); if (!v || chips().includes(v)) return; $('.tags input', form).insertAdjacentHTML('beforebegin', `<span class="chip">${v.replace(/</g, '&lt;')}<b class="edits">×</b></span>`); };
  const hasData = card => $$('input:not([type=hidden]), textarea', card).some(i => !i.closest('template') && i.value.trim() !== '') || !!$('.chip', card);
  function markEmpty() { $$('.field', form).forEach(fl => { const p = $('.pdate', fl); const v = p ? getDate(p) : ($('input, select, textarea', fl) || {}).value; fl.classList.toggle('empty', !(v || '').trim()); }); const mn = $('.morenames', form); mn.classList.toggle('filled', $$('input', mn).some(i => i.value.trim())); }
  function cards() { const edit = form.classList.contains('mode-edit'); $$('.card', form).forEach(c => { c.hidden = !(c.hasAttribute('data-always') || hasData(c) || (edit && c.dataset.added)); }); $$('.addmenu [data-card-add]', form).forEach(b => b.disabled = !$(`.card[data-card="${b.dataset.cardAdd}"]`, form).hidden); }
  function mode(m) { form.classList.toggle('mode-view', m === 'view'); form.classList.toggle('mode-edit', m === 'edit'); cards(); markEmpty(); $$('.pdate', form).forEach(dateInfo); updateButtons(); }
  const serialize = () => [...form.querySelectorAll('input:not([type=hidden]), select, textarea')].filter(e => !e.closest('template')).map(e => e.value).join('|') + '|' + chips().join(',') + '|' + $$('.pref[aria-pressed=true]', form).length;
  function updateButtons() { const changed = serialize() !== baseline; $('#ct-save').disabled = !changed; $('#ct-reset').disabled = !changed && form.classList.contains('new'); $('#ct-delete').disabled = form.classList.contains('mode-view') || form.classList.contains('new'); }
  const initials = n => n.split(/\s+/).map(w => w[0]).slice(0, 2).join('').toUpperCase();
  function show(c) {
    current = c; form.reset(); form.classList.remove('solo'); wrap.classList.remove('solo'); f.full_name.readOnly = false; $('#ct-error').textContent = '';
    f.key.value = c ? KEY(c) : ''; f.etag.value = c ? c.etag : '';
    ['given', 'family', 'nickname', 'prefix', 'additional', 'suffix', 'org', 'department', 'title', 'note', 'full_name'].forEach(k => f[k].value = c ? (c[k] || '') : '');
    fnAuto = !c || !c.full_name || c.full_name === [c.given, c.family].filter(Boolean).join(' ');
    f.ab.value = c ? `${c.principal}/${c.ab_id}` : f.ab.options[0].value;
    setDate($('[data-date=birthday]', form), c ? c.birthday : ''); setDate($('[data-date=anniversary]', form), c ? c.anniversary : '');
    ['phones', 'emails', 'addresses', 'urls', 'impp', 'related', 'dates'].forEach(l => { rows(l).forEach(r => r.remove()); (c ? c[l] : []).forEach(it => addRow(l, it)); });
    if (!c) addRow('phones', { kind: 'cell', value: '' });
    $$('.tags .chip', form).forEach(x => x.remove()); (c ? c.categories : []).forEach(addChip);
    $('.morenames', form).hidden = true; $('[data-more]', form).hidden = false;
    $$('.card[data-added]', form).forEach(x => x.removeAttribute('data-added'));
    $('#ct-av').textContent = c ? initials(c.full_name || '?') : '?'; $('#ct-av').style.background = c ? `var(--c${1 + (c.uid.length % 6)})` : 'var(--accent)';
    $('#ct-sub').textContent = c ? [c.org, c.categories.join(', ')].filter(Boolean).join(' · ') : '';
    form.classList.toggle('new', !c); f.full_name.placeholder = c ? D.tDisplay : D.tNewtitle;
    const b = $('#ct-save'); b.title = c ? D.tSave : D.tCreate; b.setAttribute('aria-label', b.title);
    mode(c ? 'view' : 'edit'); baseline = serialize(); updateButtons();
    det.hidden = false; det.classList.add('on');
    activeKey = c ? KEY(c) : null; markRow(activeKey);
    $$('.list li.on').forEach(l => l.classList.remove('on')); if (c) { const li = $(`.list li[data-key="${f.key.value}"]`); if (li) li.classList.add('on'); }
  }
  async function open(key) { try { const r = await fetch('/api/contacts/' + key); if (!r.ok) throw new Error(r.status); show(await r.json()); } catch (e) { alert(T.err + ': ' + e.message); } }
  $$('.list li[data-key]').forEach(li => li.onclick = () => open(li.dataset.key));
  $$('[data-open]').forEach(b => b.onclick = () => { wrap.classList.remove('view-table'); $('[data-ctview=list]').click(); open(b.dataset.open); });
  // editing: dirty tracking, display name follows given + family, dates re-evaluate
  form.addEventListener('input', e => { if (e.target === f.given || e.target === f.family) { if (fnAuto) f.full_name.value = [f.given.value, f.family.value].filter(Boolean).join(' '); } else if (e.target === f.full_name) fnAuto = !f.full_name.value; const p = e.target.closest('.pdate'); if (p) dateInfo(p); updateButtons(); });
  form.addEventListener('change', updateButtons);
  form.addEventListener('click', async e => {
    const b = e.target.closest('button'); if (!b || !form.contains(b)) return;
    if (b.dataset.add) { const row = addRow(b.dataset.add, b.dataset.add === 'phones' ? { kind: 'cell' } : b.dataset.add === 'emails' || b.dataset.add === 'addresses' ? { kind: 'home' } : null); const i = $('input:not([type=hidden])', row); if (i) i.focus(); updateButtons(); return; }
    if (b.hasAttribute('data-rm')) { b.closest('.row').remove(); updateButtons(); return; }
    if (b.classList.contains('pref')) { const list = b.closest('[data-list]'); const on = b.getAttribute('aria-pressed') === 'true'; $$('.pref', list).forEach(p => p.setAttribute('aria-pressed', p === b && !on ? 'true' : 'false')); updateButtons(); return; }
    if (b.hasAttribute('data-more')) { $('.morenames', form).hidden = false; b.hidden = true; return; }
    if (b.dataset.cardAdd) { const c = $(`.card[data-card="${b.dataset.cardAdd}"]`, form); c.dataset.added = '1'; c.hidden = false; b.disabled = true; b.closest('.addmenu').classList.remove('open'); if (b.dataset.cardAdd === 'email') addRow('emails', { kind: 'home' }); if (b.dataset.cardAdd === 'adr') addRow('addresses', { kind: 'home' }); const i = $('input:not([type=hidden]), textarea', c); if (i && !i.closest('template')) i.focus(); return; }
    if (b.hasAttribute('data-card-rm')) { if (!await ask(D.tRmcard)) return; const c = b.closest('.card'); $$('input:not([type=hidden]), textarea', c).forEach(i => { if (!i.closest('template')) i.value = ''; }); $$('.rows .row', c).forEach(x => x.remove()); $$('.chip', c).forEach(x => x.remove()); delete c.dataset.added; c.hidden = true; cards(); updateButtons(); return; }
    if (b.dataset.act) { action(b); return; }
  });
  form.addEventListener('click', e => { const x = e.target.closest('.chip b'); if (x && form.contains(x)) { x.closest('.chip').remove(); updateButtons(); } });
  const tagIn = $('.tags input', form);
  tagIn.addEventListener('keydown', e => { if (e.key === 'Enter' || e.key === ',') { e.preventDefault(); addChip(tagIn.value); tagIn.value = ''; updateButtons(); } else if (e.key === 'Backspace' && !tagIn.value) { const c = tagIn.previousElementSibling; if (c && c.classList.contains('chip')) { c.remove(); updateButtons(); } } });
  tagIn.addEventListener('blur', () => { if (tagIn.value.trim()) { addChip(tagIn.value); tagIn.value = ''; updateButtons(); } });
  // view-mode actions
  function action(b) {
    const row = b.closest('.row'); const val = row ? ($('input:not([type=hidden])', row) || {}).value || '' : ''; const digits = val.replace(/[^0-9+]/g, ''); const open_ = u => window.open(u, '_blank', 'noopener');
    const adrText = () => ['street', 'code', 'city', 'country'].map(k => ($(`[data-w=${k}]`, row) || {}).value || '').filter(Boolean).join(', ');
    switch (b.dataset.act) {
      case 'tel': location.href = 'tel:' + digits; break;
      case 'sms': location.href = 'sms:' + digits; break;
      case 'wa': open_('https://wa.me/' + digits.replace(/^\+/, '').replace(/^00/, '').replace(/^0/, '49')); break;
      case 'mailto': location.href = 'mailto:' + val; break;
      case 'open': open_(/^https?:/.test(val) ? val : 'https://' + val); break;
      case 'osm': open_('https://www.openstreetmap.org/search?query=' + encodeURIComponent(adrText())); break;
      case 'gmaps': open_('https://www.google.com/maps/search/?api=1&query=' + encodeURIComponent(adrText())); break;
      case 'copy': navigator.clipboard.writeText(row.classList.contains('adr') ? adrText() : val).then(() => { b.title = D.tCopied; b.style.color = 'var(--accent)'; setTimeout(() => b.style.color = '', 800); }); break;
      case 'im': { const svc = $('select.typ', row).value; const u = { Signal: 'https://signal.me/#p/' + digits, WhatsApp: 'https://wa.me/' + digits.replace(/^\+/, ''), Telegram: 'https://t.me/' + val.replace(/^@/, ''), Matrix: 'https://matrix.to/#/' + val, XMPP: 'xmpp:' + val, Threema: 'https://threema.id/' + val }[svc]; if (u) open_(u); break; }
      case 'goto': { const li = $$('.list li[data-key]').find(l => (l.dataset.name || '').toLowerCase() === val.toLowerCase()); if (li) { open(li.dataset.key); const sc = $('.main'); sc.scrollTop += li.getBoundingClientRect().top - sc.getBoundingClientRect().top - 120; } else alert(D.tMissing); break; }
    }
  }
  $('#ct-lock').onclick = () => mode('edit');
  $('#ct-reset').onclick = () => show(current);
  $('#ct-cancel').onclick = () => { if (wrap.classList.contains('dlg')) dlgClose(); else if (wide()) show(null); else { det.classList.remove('on'); det.hidden = true; } };
  $('#ct-vcf').onclick = () => { if (f.key.value) location.href = '/api/contacts/' + f.key.value + '/vcf'; };
  // reveal a contact (a search hit, a #key link): the table marks and scrolls to its row, the list opens it and scrolls its entry into view
  function markRow(key) { let hit = null; $$('.sheet tr[data-key]', wrap).forEach(r => { const on = r.dataset.key === key; r.classList.toggle('on', on); if (on) hit = r; }); return hit; }
  async function reveal(key) {
    activeKey = key;
    if (wrap.classList.contains('view-table')) { const tr = markRow(key); if (tr) tr.scrollIntoView({ block: 'center' }); return; }
    if (!current || KEY(current) !== key) await open(key); const li = $(`.list li[data-key="${CSS.escape(key)}"]`, wrap); if (li) li.scrollIntoView({ block: 'center' });
  }
  // the table picks a contact with a click on its row; the list opens it (above)
  $$('.sheet tr[data-key]', wrap).forEach(tr => tr.addEventListener('click', () => { activeKey = tr.dataset.key; markRow(activeKey); }));
  window.rusticalContacts = { reveal };
  if (location.hash.length > 1) reveal(decodeURIComponent(location.hash.slice(1))); else if (wide()) show(null);
  function collect() {
    const [principal, ab_id] = f.ab.value.split('/');
    const g = k => f[k].value.trim();
    return { principal, ab_id, full_name: g('full_name'), given: g('given'), family: g('family'), nickname: g('nickname'), prefix: g('prefix'), additional: g('additional'), suffix: g('suffix'), org: g('org'), department: g('department'), title: g('title'), note: g('note'),
      phones: rowVals('phones'), emails: rowVals('emails'), urls: rowVals('urls'), impp: rowVals('impp'), related: rowVals('related'),
      addresses: rows('addresses').map(r => { const o = { kind: $('select.typ', r).value, pref: $('.pref', r).getAttribute('aria-pressed') === 'true' }; ['street', 'code', 'city', 'country', 'region'].forEach(k => o[k] = $(`[data-w=${k}]`, r).value.trim()); return o; }).filter(a => a.street || a.city || a.code || a.country),
      dates: rows('dates').map(r => ({ label: $('.lbl', r).value.trim(), date: getDate($('.pdate', r)) })).filter(d => d.date),
      birthday: getDate($('[data-date=birthday]', form)), anniversary: getDate($('[data-date=anniversary]', form)), categories: chips() };
  }
  form.addEventListener('submit', async e => {
    e.preventDefault(); const key = f.key.value; const body = collect();
    const r = await fetch(key ? '/api/contacts/' + key : '/api/contacts', { method: key ? 'PUT' : 'POST', headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf }, body: JSON.stringify(key ? { ...body, etag: f.etag.value } : body) });
    if (!r.ok) { $('#ct-error').textContent = T.err + ' ' + r.status + ': ' + (await r.text()).slice(0, 200); return; }
    const { id } = await r.json(); location.hash = `${body.principal}/${body.ab_id}/${id}`; location.reload();
  });
  $('#ct-delete').onclick = async () => { if (!await ask(T.del)) return; const r = await fetch('/api/contacts/' + f.key.value, { method: 'DELETE', headers: { 'X-CSRF-Token': csrf } }); if (r.ok) { location.hash = ''; location.reload(); } };
  // table inline edit: fetch → patch one field → PUT
  $$('.sheet td[contenteditable][data-f], .sheet .pin [contenteditable][data-f]').forEach(cell => {
    let before = '';
    cell.addEventListener('focus', () => before = cell.textContent);
    cell.addEventListener('keydown', e => { if (e.key === 'Enter') { e.preventDefault(); cell.blur(); } });
    cell.addEventListener('blur', async () => {
      const v = cell.textContent.trim(); if (v === before.trim()) return;
      const tr = cell.closest('tr'); const key = tr.dataset.key;
      try {
        const c = await (await fetch('/api/contacts/' + key)).json();
        const body = { ...c }; // the contact JSON is a superset of the input: nothing gets dropped
        switch (cell.dataset.f) {
          case 'full_name': { body.full_name = v; const parts = v.split(/\s+/); body.given = parts.slice(0, -1).join(' '); body.family = parts.slice(-1)[0] || ''; break; }
          case 'phone': body.phones = v ? [{ kind: (c.phones[0] || {}).kind || 'cell', value: v }, ...c.phones.slice(1)] : c.phones.slice(1); break;
          case 'email': body.emails = v ? [{ kind: (c.emails[0] || {}).kind || 'home', value: v }, ...c.emails.slice(1)] : c.emails.slice(1); break;
          case 'birthday': { const m = v.match(/^(\d{1,2})\.(\d{1,2})\.(\d{4})?$/); body.birthday = m ? (m[3] ? `${m[3]}-${m[2].padStart(2, '0')}-${m[1].padStart(2, '0')}` : `--${m[2].padStart(2, '0')}-${m[1].padStart(2, '0')}`) : v; break; }
          case 'org': body.org = v; break;
        }
        const r = await fetch('/api/contacts/' + key, { method: 'PUT', headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf }, body: JSON.stringify(body) });
        if (!r.ok) throw new Error(r.status);
        cell.style.outline = '2px solid var(--c2)'; setTimeout(() => cell.style.outline = '', 800);
      } catch (e) { cell.textContent = before; alert(T.err + ': ' + e.message); }
    });
  });
  // table view: + opens the editor as a dialog over the table (the list view has it beside the list)
  const dlgClose = () => { wrap.classList.remove('dlg', 'solo'); form.classList.remove('solo'); f.full_name.readOnly = false; det.classList.remove('on'); if (!wide()) det.hidden = true; };
  $('#ct-fab').onclick = () => { show(null); wrap.classList.add('dlg'); f.full_name.focus(); };
  /* a cell the table cannot edit (categories, address, note) opens just that card of the editor,
     already unlocked, as a dialog; saving writes the whole contact as the full editor does */
  async function openSolo(key, card) {
    await open(key);
    const c = $(`.card[data-card="${card}"]`, form); if (!c) return;
    c.dataset.added = '1'; mode('edit');
    $$('.card', form).forEach(x => x.classList.toggle('solo-on', x === c)); c.hidden = false;
    form.classList.add('solo'); wrap.classList.add('dlg', 'solo'); f.full_name.readOnly = true;
    // an empty list card (addresses …) starts with one empty row, as a new contact does with its phone
    const add = $('[data-add]', c); if (add && !$('.rows .row', c)) add.click();
    const first = $('input:not([type=hidden]):not([readonly]), textarea', c) || $('select', c); if (first) first.focus();
  }
  $$('.sheet td.cardcell').forEach(td => td.onclick = () => openSolo(td.closest('tr').dataset.key, td.dataset.card));
  addEventListener('keydown', e => { if (e.key === 'Escape' && wrap.classList.contains('dlg')) dlgClose(); });
})();

/* ===== tasks page: six views over one list, the shared card editor beside them =====
   The server puts the tasks it wants shown into `#tasks-data` (the lookback decides how far
   back finished ones reach); everything here is drawing and picking. The editor is the same
   dialog the calendar uses (templates/editor.html) — on a wide window it is docked to the
   right instead of opening over the page. */
(function () {
  const root = document.getElementById('taskroot');
  if (!root) return;
  const $ = (s, r = document) => r.querySelector(s), $$ = (s, r = document) => [...r.querySelectorAll(s)];
  const csrf = document.querySelector('meta[name=csrf]').content;
  const lang = document.body.dataset.lang || 'de';
  const T = trs('js-tk-', ['open', 'doing', 'done', 'cancelled', 'nodate', 'overdue', 'today', 'week', 'later', 'del', 'err', 'sort', 'askDone', 'yes', 'no', 'asc', 'desc', 'done_at', 'due', 'wasdue', 'empty', 'prio', 'pct', 'count', 'keys', 'hist', 'histNone', 'nodateBox', 'loading', 'start', 'cats', 'edit']);
  const SORTL = trs('js-sort-', ['due', 'done', 'prio', 'pct', 'title', 'cal', 'created']);

  // the page ships every open task (the working set, bounded) and the first page of the finished
  // ones (unbounded by the lookback); the rest arrives as a view scrolls towards it
  const DATA = (() => { try { return JSON.parse($('#tasks-data').textContent); } catch (_) { return { open: [], done: [], done_total: 0, per_day: {} }; } })();
  const OPEN = DATA.open || [], PER_DAY = DATA.per_day || {};
  const COMPLETED_TOTAL = DATA.completed_total || 0, CANCELLED_TOTAL = DATA.cancelled_total || 0;
  let DONE = DATA.done || [], DONE_TOTAL = DATA.done_total || 0;
  let TASKS = OPEN.concat(DONE);
  const rebuild = () => { TASKS = OPEN.concat(DONE); };
  /* a page of the finished list is a valid prefix of these two sorts only; every other one
     needs the whole list, so it is fetched before the view draws */
  const PREFIX_SAFE = ['due', 'done'];
  let loading = null;
  async function loadMore(all) {
    if (loading) return loading;
    if (DONE.length >= DONE_TOTAL) return null;
    const limit = all ? Math.min(500, DONE_TOTAL - DONE.length) : +root.dataset.chunk || 120;
    const u = `/api/tasks/done?back=${encodeURIComponent(root.dataset.back)}&offset=${DONE.length}&limit=${limit}` + (root.dataset.cals ? '&cals=' + encodeURIComponent(root.dataset.cals) : '');
    loading = fetch(u).then(r => r.ok ? r.json() : null).then(d => {
      loading = null;
      if (!d) return null;
      DONE = DONE.concat(d.items);
      DONE_TOTAL = d.total;
      rebuild();
      return d;
    }).catch(() => { loading = null; return null; });
    return loading;
  }
  async function ensureAll() { while (DONE.length < DONE_TOTAL) { if (!await loadMore(true)) break; } }
  /* the finished list runs newest first, so a day is fully loaded once the tail reaches past it */
  async function ensureUntil(day) {
    const oldest = () => { const l = DONE[DONE.length - 1]; return l && l.completed_at ? String(l.completed_at).slice(0, 10) : '9999-99-99'; };
    while (DONE.length < DONE_TOTAL && oldest() > day) { if (!await loadMore()) break; }
  }
  const main = $('#tkmain'), ed = $('#ed');
  const TODAY = parse(root.dataset.today);
  let view = root.dataset.view, sort = localStorage.tkSort || 'due', desc = localStorage.tkDesc === '1', cur = null, act = null; // cur: the task in the editor; act: the picked one (marked in every view that shows it)

  function parse(s) { const [y, m, d] = String(s).slice(0, 10).split('-').map(Number); return new Date(y, m - 1, d); }
  const dt = s => s ? new Date(String(s).replace(' ', 'T')) : null;
  const day = d => Math.round((d - TODAY) / 864e5);
  const esc = s => String(s == null ? '' : s).replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
  const ic = (n, cl) => `<svg class="ico ${cl || 's'}" aria-hidden="true"><use href="#i-${n}"/></svg>`;
  const L = LOC;
  const de = d => d ? d.toLocaleDateString(L, { day: '2-digit', month: '2-digit', year: 'numeric' }) : '';
  const deS = d => d ? d.toLocaleDateString(L, { day: '2-digit', month: '2-digit' }) : '';
  const deT = d => d ? d.toLocaleString(L, { day: '2-digit', month: '2-digit', year: '2-digit', hour: '2-digit', minute: '2-digit' }) : '';
  const iso = d => d.getFullYear() + '-' + String(d.getMonth() + 1).padStart(2, '0') + '-' + String(d.getDate()).padStart(2, '0');
  const key = t => `${t.principal}/${t.cal_id}/${t.object_id}`;

  const isDone = t => t.status === 'COMPLETED' || (!t.status && !!t.completed_at);
  const isOpen = t => !isDone(t) && t.status !== 'CANCELLED';
  const overdue = t => isOpen(t) && t.due && day(parse(t.due)) < 0;
  const pctOf = t => t.percent == null ? (isDone(t) ? 100 : 0) : t.percent;
  const dueTxt = t => t.due ? (t.due_all_day ? de(parse(t.due)) : deT(dt(t.due))) : '';

  // ---- sorting: the completed ones drop to the end, unless that is what we sort by ----
  function skey(t, k) {
    switch (k) {
      case 'due': return t.due ? parse(t.due).getTime() : 9e15;
      case 'done': return t.completed_at ? -dt(t.completed_at).getTime() : 9e15;
      case 'prio': return t.priority ? t.priority : 99;
      case 'pct': return -pctOf(t);
      case 'title': return t.summary.toLowerCase();
      case 'cal': return t.cal_name;
      case 'created': return t.created ? -dt(t.created).getTime() : 9e15;
    }
  }
  function sortList(list, k, d) {
    const s = list.slice().sort((a, b) => { const x = skey(a, k), y = skey(b, k); return x < y ? -1 : x > y ? 1 : a.summary.localeCompare(b.summary); });
    return d ? s.reverse() : s;
  }
  function sortView(list) {
    if (sort === 'done') return sortList(list.filter(t => t.completed_at), sort, desc).concat(sortList(list.filter(t => !t.completed_at), 'due', false));
    return sortList(list.filter(isOpen), sort, desc).concat(sortList(list.filter(t => !isOpen(t)), 'done', false));
  }

  // ---- shared atoms ----
  function chips(t, short) {
    const out = [];
    if (t.due) out.push(`<span class="tchip${overdue(t) ? ' warn' : ''}">${ic('clock', 'xs')}${t.start && !short ? deS(parse(t.start)) + '–' : ''}${dueTxt(t)}</span>`);
    if (t.completed_at) out.push(`<span class="tchip good">${ic('check', 'xs')}${T.done_at} ${deT(dt(t.completed_at))}</span>`);
    if (t.priority) out.push(`<span class="tchip">${ic(t.priority <= 4 ? 'prio-high' : t.priority === 5 ? 'prio-mid' : 'prio-low', 'xs')}${T.prio} ${t.priority}</span>`);
    const p = pctOf(t);
    if (p > 0 && p < 100) out.push(`<span class="tchip">${ic('target', 'xs')}${p} %</span>`);
    (t.categories || []).forEach(c => out.push(`<span class="tchip">${ic('tag', 'xs')}${esc(c)}</span>`));
    if (t.location) out.push(`<span class="tchip">${ic('pin', 'xs')}${esc(t.location.split(',')[0])}</span>`);
    if (t.alarms) out.push(`<span class="tchip">${ic('bell', 'xs')}${t.alarms}</span>`);
    if (t.links) out.push(`<span class="tchip">${ic('ext', 'xs')}${t.links}</span>`);
    if (t.class === 'PRIVATE' || t.class === 'CONFIDENTIAL') out.push(`<span class="tchip">${ic('lock', 'xs')}</span>`);
    if (t.status === 'CANCELLED') out.push(`<span class="tchip warn">${ic('thumbs-down', 'xs')}${T.cancelled}</span>`);
    if (t.status === 'IN-PROCESS') out.push(`<span class="tchip">${ic('dots', 'xs')}${T.doing}</span>`);
    return out.join('');
  }
  const cbox = t => `<button class="tcb${t.status === 'IN-PROCESS' ? ' pp' : ''}" role="checkbox" aria-checked="${isDone(t)}" data-tog="${esc(key(t))}" title="${T.done}">${ic(t.status === 'IN-PROCESS' ? 'dots' : 'check', 'xs')}</button>`;

  /* sort field and direction live in the page toolbar, between lookback and view switch; the
     direction button always shows the current order. The history lists by completion only, so
     there it keeps just the direction. */
  const sortBox = $('#tk-sortbox');
  sortBox.innerHTML = `<span class="sel" title="${esc(T.sort)}">${ic('swap')}<select id="tk-sort" aria-label="${esc(T.sort)}">${Object.entries(SORTL).map(([v, l]) => `<option value="${v}">${esc(l)}</option>`).join('')}</select></span><button class="btn icon-btn" id="tk-dir" type="button"></button>`;
  const sortSel = $('#tk-sort'), sortDir = $('#tk-dir');
  function syncSort() {
    sortSel.value = sort;
    sortSel.closest('.sel').hidden = view === 'history';
    sortDir.innerHTML = ic(desc ? 'sort-desc' : 'sort-asc');
    sortDir.title = desc ? T.desc : T.asc; sortDir.setAttribute('aria-label', sortDir.title);
  }
  sortSel.onchange = () => { sort = sortSel.value; localStorage.tkSort = sort; render(); };
  sortDir.onclick = () => { desc = !desc; localStorage.tkDesc = desc ? '1' : '0'; render(); };

  // ---- A: groups ----
  function bucket(t) {
    if (isDone(t)) return T.done;
    if (t.status === 'CANCELLED') return T.cancelled;
    if (!t.due) return T.nodate;
    const n = day(parse(t.due));
    return n < 0 ? T.overdue : n === 0 ? T.today : n <= 7 ? T.week : T.later;
  }
  const GICON = { [T.overdue]: 'prio-high', [T.today]: 'today', [T.week]: 'dayline', [T.later]: 'calendar', [T.nodate]: 'note', [T.done]: 'check', [T.cancelled]: 'thumbs-down' };
  function vGroups() {
    let h = '';
    const order = [T.overdue, T.today, T.week, T.later, T.nodate, T.done, T.cancelled];
    order.forEach(g => {
      const items = sortList(TASKS.filter(t => bucket(t) === g), g === T.done && sort === 'due' ? 'done' : sort, desc);
      if (!items.length) return;
      const n = g === T.done ? COMPLETED_TOTAL : g === T.cancelled ? CANCELLED_TOTAL : items.length;
      h += `<div class="tkg"><div class="tkgh${g === T.overdue ? ' due' : ''}">${ic(GICON[g] || 'list')}${esc(g)}<span class="n">${n}</span></div>`;
      items.forEach(t => {
        h += `<div class="tkcard${isDone(t) ? ' off' : ''}" data-key="${esc(key(t))}">${cbox(t)}
          <div><div class="ti">${esc(t.summary)}</div><div class="meta">${chips(t, true)}</div></div>
          <span class="sw" style="--c:${esc(t.color)}"></span></div>`;
      });
      h += '</div>';
    });
    return h;
  }

  // ---- B: table — the sheet is the editor: each cell saves on its own, like the contacts table ----
  const STL = { 'NEEDS-ACTION': T.open, 'IN-PROCESS': T.doing, COMPLETED: T.done, CANCELLED: T.cancelled, '': '—' };
  // the choices come from the editor's own selects, so both speak the same words
  const optsOf = name => $$(`#ed select[name=${name}] option`).map(o => [o.value, o.textContent]);
  const shownCols = () => $$('#tk-colpop input[data-col]').filter(i => i.checked).map(i => i.dataset.col);
  const colLabel = c => { const i = $(`#tk-colpop input[data-col="${c}"]`); return i ? i.dataset.label : c; };
  const SORTABLE = { cal: 'cal', due: 'due', percent: 'pct', priority: 'prio', completed: 'done', created: 'created' };
  // a date input stays a date, a time stays a time: switching between the two is the dialog's job
  function dtIn(t, f) {
    const v = t[f] ? String(t[f]) : '';
    const allDay = f === 'completed_at' ? false : (v ? t[f + '_all_day'] : true);
    return `<input type="${allDay ? 'date' : 'datetime-local'}" data-f="${f}" value="${esc(allDay ? v.slice(0, 10) : v.slice(0, 16))}"${f === 'due' && overdue(t) ? ' class="late"' : ''}${v ? '' : ' data-empty'}>`;
  }
  function selIn(f, cur, opts) {
    const list = opts.some(([v]) => v === cur) ? opts : opts.concat([[cur, cur]]);
    return `<select data-f="${f}">${list.map(([v, l]) => `<option value="${esc(v)}"${v === cur ? ' selected' : ''}>${esc(l)}</option>`).join('')}</select>`;
  }
  const cardCell = (card, html, cl) => `<td class="cardcell${cl ? ' ' + cl : ''}" data-card="${card}" title="${esc(T.edit)}">${html || '<span class="mut">–</span>'}</td>`;
  const edTxt = (f, v) => `<span contenteditable="plaintext-only" data-f="${f}">${esc(v)}</span>`;
  // the calendar of a task is fixed (a task cannot move)
  const CELL = {
    cal: t => `<td class="ro"><span class="sw" style="--c:${esc(t.color)}"></span> ${esc(t.cal_name)}</td>`,
    start: t => `<td>${dtIn(t, 'start')}</td>`,
    due: t => `<td>${dtIn(t, 'due')}</td>`,
    status: t => `<td>${selIn('status', t.status || '', Object.entries(STL).filter(([v]) => v).map(([v, l]) => [v, l]).concat(t.status ? [] : [['', '—']]))}</td>`,
    percent: t => `<td class="tkc"><input type="number" min="0" max="100" step="5" data-f="percent" value="${t.percent == null ? '' : t.percent}" placeholder="—"></td>`,
    priority: t => `<td>${selIn('priority', t.priority ? String(t.priority) : '', optsOf('priority'))}</td>`,
    completed: t => `<td>${dtIn(t, 'completed_at')}</td>`,
    // what a cell cannot hold opens the editor's own card for it (cardCell)
    categories: t => cardCell('cats', (t.categories || []).map(c => `<span class="tchip">${esc(c)}</span>`).join(' ')),
    location: t => cardCell('where', esc(t.location)),
    class: t => `<td>${selIn('class', t.class || '', optsOf('class'))}</td>`,
    created: t => `<td class="ro tnum">${t.created ? deT(dt(t.created)) : '—'}</td>`,
    description: t => cardCell('note', esc(t.description), 'desc'),
    alarms: t => cardCell('alarm', t.alarms ? `${ic('bell', 'xs')} ${t.alarms}` : ''),
    links: t => cardCell('links', t.links ? `${ic('ext', 'xs')} ${t.links}` : ''),
  };
  const rowHtml = (t, cols) => `<tr class="${isDone(t) ? 'off' : ''}" data-key="${esc(key(t))}"><td class="pin"><div class="pinw">${cbox(t)}${edTxt('summary', t.summary)}</div></td>` +
    cols.map(c => CELL[c](t)).join('') + '</tr>';
  function vTable() {
    const cols = shownCols().filter(c => CELL[c]);
    const th = (label, sk, cl) => `<th${cl ? ` class="${cl}"` : ''}${sk ? ` data-sk="${sk}"` : ''}>${esc(label)}${sk && sort === sk ? ` <span class="a ${desc ? 'desc' : 'asc'}">${ic('sort', 'xs')}</span>` : ''}</th>`;
    return `<div class="sheetwrap"><table class="sheet"><thead><tr>${th(SORTL.title, 'title', 'pin')}${cols.map(c => th(colLabel(c), SORTABLE[c])).join('')}</tr></thead><tbody>` +
      sortView(TASKS).map(t => rowHtml(t, cols)).join('') + `</tbody></table>${SENTINEL()}</div>`;
  }
  const find = k => TASKS.find(x => key(x) === k);
  // a row with the cursor in it is updated cell by cell, leaving the focused one alone —
  // tabbing on from a saved cell never loses the cursor and still sees the derived values
  function redrawRow(k) {
    const tr = $$('.sheet tbody tr', main).find(r => r.dataset.key === k), t = find(k);
    if (!tr || !t) return;
    const tmp = document.createElement('tbody');
    tmp.innerHTML = rowHtml(t, shownCols().filter(c => CELL[c]));
    const fresh = tmp.firstElementChild;
    if (!tr.contains(document.activeElement)) { fresh.classList.toggle('sel', tr.classList.contains('sel')); tr.replaceWith(fresh); return; }
    tr.classList.toggle('off', fresh.classList.contains('off'));
    $('.tcb', tr).replaceWith($('.tcb', fresh));
    $$('[data-f]', fresh).forEach(n => {
      const o = $(`[data-f="${n.dataset.f}"]`, tr);
      if (!o || o === document.activeElement) return;
      o.replaceWith(n);
    });
  }
  // after a save the server's answer wins: it keeps status, progress and completion consistent
  async function refresh(k) {
    const d = await (await fetch('/api/tasks/' + k)).json(), t = find(k);
    if (!t) return;
    ['summary', 'description', 'location', 'start', 'start_all_day', 'due', 'due_all_day', 'status', 'percent', 'completed_at', 'class', 'categories'].forEach(f => { t[f] = d[f]; });
    t.priority = d.priority || 0;
    t.alarms = (d.alarms || []).length; t.links = (d.links || []).length;
    $('#task-count').textContent = $('#task-count').textContent.replace(/^\d+/, TASKS.filter(isOpen).length);
    redrawRow(k);
  }
  async function saveCell(el) {
    const k = el.closest('tr').dataset.key, f = el.dataset.f;
    const v = el.isContentEditable ? el.textContent.trim() : el.value;
    const d = await (await fetch('/api/tasks/' + k)).json();
    switch (f) {
      case 'summary': if (!v) { refresh(k); return; } d.summary = v; break;
      case 'start': case 'due': d[f] = v || null; d[f + '_all_day'] = el.type === 'date'; break;
      case 'class': d.class = v; break;
      case 'priority': d.priority = v ? +v : null; break;
      case 'status': d.status = v; if (v !== 'COMPLETED') d.completed_at = null; break;
      // the three progress fields move together, the same rule the editor applies
      case 'percent': { const n = v === '' ? null : Math.max(0, Math.min(100, Math.round(+v)));
        d.percent = n; d.status = n === 100 ? 'COMPLETED' : n ? 'IN-PROCESS' : (d.status === 'COMPLETED' || d.status === 'IN-PROCESS' ? 'NEEDS-ACTION' : d.status); break; }
      case 'completed_at': d.completed_at = v || null; d.status = v ? 'COMPLETED' : (d.status === 'COMPLETED' ? 'NEEDS-ACTION' : d.status); break;
      default: return;
    }
    if (!await put(k, d)) { refresh(k); return; }
    const td = el.closest('td'); td.classList.add('saved'); setTimeout(() => td.classList.remove('saved'), 800);
    await refresh(k);
  }
  main.addEventListener('focusin', e => { const el = e.target.closest('.sheet [contenteditable][data-f]'); if (el) el.dataset.before = el.textContent; });
  main.addEventListener('focusout', e => {
    const el = e.target.closest('.sheet tbody [contenteditable][data-f]');
    if (el && el.textContent.trim() !== (el.dataset.before || '').trim()) saveCell(el);
  });
  main.addEventListener('change', e => { const el = e.target.closest('.sheet tbody input[data-f], .sheet tbody select[data-f]'); if (el) saveCell(el); });
  main.addEventListener('keydown', e => {
    const el = e.target.closest('.sheet [contenteditable]');
    if (!el) return;
    if (e.key === 'Escape' && el.dataset.f) { el.textContent = el.dataset.before || ''; el.blur(); }
    if (e.key === 'Enter') { e.preventDefault(); el.blur(); }
  });
  $$('#tk-colpop input[data-col]').forEach(i => i.addEventListener('change', () => {
    render();
    fetch('/settings/task-columns', { method: 'POST', headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf }, body: JSON.stringify(shownCols()) });
  }));

  // ---- C: board ----
  const BCOLS = [['NEEDS-ACTION', T.open, 'list'], ['IN-PROCESS', T.doing, 'dots'], ['COMPLETED', T.done, 'check'], ['CANCELLED', T.cancelled, 'thumbs-down']];
  function vBoard() {
    let h = '<div class="tkboard">';
    BCOLS.forEach(([st, label, icon]) => {
      const items = sortList(TASKS.filter(t => (st === 'COMPLETED' ? isDone(t) : t.status === st && !isDone(t))), st === 'COMPLETED' && sort === 'due' ? 'done' : sort, desc);
      const n = st === 'COMPLETED' ? COMPLETED_TOTAL : st === 'CANCELLED' ? CANCELLED_TOTAL : items.length;
      h += `<div class="tkcol${st === 'COMPLETED' ? ' done' : ''}" data-st="${st}"><div class="chd">${ic(icon, 'xs')}${esc(label)}<span class="n">${n}</span></div>`;
      items.forEach(t => {
        const p = pctOf(t);
        h += `<div class="tkkc" draggable="true" data-key="${esc(key(t))}" style="--c:${esc(t.color)}"><div class="ti">${esc(t.summary)}</div>
          ${p > 0 && p < 100 ? `<div class="pb"><i style="width:${p}%"></i></div>` : ''}<div class="ft">${chips(t, true)}</div></div>`;
      });
      h += '</div>';
    });
    return h + '</div>';
  }

  // ---- D: timeline ----
  function vTimeline() {
    const LO = -25, HI = 75, span = HI - LO;
    const x = n => (Math.max(LO, Math.min(HI, n)) - LO) / span * 100;
    let ax = '';
    for (let m = -1; m <= 2; m++) {
      const d = new Date(TODAY.getFullYear(), TODAY.getMonth() + m, 1), n = day(d);
      if (n < LO || n > HI) continue;
      ax += `<span style="left:${x(n)}%">${d.toLocaleDateString(L, { month: 'short', year: '2-digit' })}</span><i style="left:${x(n)}%"></i>`;
    }
    let h = `<div class="tktl"><div class="axis">${ax}</div><div class="now" style="left:${x(0)}%"><b>${T.today}</b></div>`;
    sortList(TASKS.filter(t => t.due || t.completed_at), sort, desc).forEach(t => {
      const anchor = t.completed_at ? day(dt(t.completed_at)) : day(parse(t.due));
      const from = (!t.completed_at && t.start) ? day(parse(t.start)) : anchor;
      const l = x(from), r = x(anchor), w = Math.max(r - l, 0), wide = w > 3.5;
      h += `<div class="lane"><div class="bar2${wide ? '' : ' dot'}${isDone(t) || t.status === 'CANCELLED' ? ' off' : ''}" data-key="${esc(key(t))}"
        style="--c:${esc(t.color)};left:calc(${l}% - ${wide ? 0 : 8}px)${wide ? `;width:${w}%` : ''}" title="${esc(t.summary)}">${isDone(t) ? ic('check', 'xs') : ''}${wide ? esc(t.summary) : ''}</div>
        ${wide ? '' : `<div class="lbl" style="left:calc(${l}% + 14px)">${esc(t.summary)}</div>`}</div>`;
    });
    h += '</div>';
    const nd = TASKS.filter(t => !t.due && !t.completed_at);
    if (nd.length) h += `<div class="muted small" style="margin-top:14px">${T.nodateBox}</div><div class="tknd">${nd.map(t => `<span class="tchip" data-key="${esc(key(t))}">${ic('note', 'xs')}${esc(t.summary)}</span>`).join('')}</div>`;
    return h;
  }

  // ---- E: deck ----
  let deckI = 0;
  function deckGo(i) { const list = sortView(TASKS); deckI = i >= list.length ? 0 : i; if (list[deckI]) act = key(list[deckI]); } // the card on top is the picked task
  function vDeck() {
    const list = sortView(TASKS);
    if (deckI >= list.length) deckI = 0;
    let h = '<div class="tkdeck" tabindex="0">';
    for (let k = Math.min(list.length - 1, deckI + 2); k >= deckI; k--) {
      const t = list[k], o = k - deckI;
      const p = pctOf(t);
      h += `<div class="cd" data-key="${esc(key(t))}" style="transform:translateY(${-o * 14}px) scale(${1 - o * 0.05});opacity:${o === 0 ? 1 : 0.85 - o * 0.25};z-index:${10 - o}${o ? ';height:120px;overflow:hidden' : ''}">
        <h3${isDone(t) ? ' class="off"' : ''}>${esc(t.summary)}</h3>${o ? '' :
        `${t.description ? `<p class="dsc">${esc(t.description)}</p>` : ''}<div class="fx">${chips(t)}<span class="tchip">${ic('calendar', 'xs')}${esc(t.cal_name)}</span></div>
         ${p > 0 && p < 100 ? `<div class="pb"><i style="width:${p}%"></i></div>` : ''}
         <div class="act"><button class="btn" data-tog="${esc(key(t))}">${isDone(t) ? '↺' : '✓'}</button><button class="btn icon-btn" data-open title="${esc(T.edit)}" aria-label="${esc(T.edit)}">${ic('pencil')}</button><button class="btn" data-next>→</button></div>`}</div>`;
    }
    return h + `</div><div class="tkcnt">${list.length ? deckI + 1 : 0} ${T.count} ${list.length}</div><div class="tkkeys">${esc(T.keys)}</div>`;
  }

  // ---- F: history ----
  let pick = null;
  function vHistory() {
    const done = TASKS.filter(t => t.completed_at);
    const per = PER_DAY; // the server counted the whole window; the list below is what is loaded
    const start = new Date(TODAY); start.setDate(start.getDate() - 364 - ((start.getDay() + 6) % 7));
    let cols = '';
    for (let w = 0; w < 53; w++) {
      let cells = '';
      for (let d = 0; d < 7; d++) {
        const c = new Date(start); c.setDate(c.getDate() + w * 7 + d);
        if (c > TODAY) { cells += '<div class="dy" style="visibility:hidden"></div>'; continue; }
        const k = iso(c), n = Math.min(3, per[k] || 0);
        cells += `<div class="dy${pick === k ? ' on' : ''}" data-n="${n}" data-d="${k}" title="${de(c)}: ${per[k] || 0}"></div>`;
      }
      cols += `<div class="wk">${cells}</div>`;
    }
    let h = `<div class="tkhm">${cols}</div><div class="muted small">${T.hist}</div>`;
    const shown = sortList(pick ? done.filter(t => String(t.completed_at).slice(0, 10) === pick) : done, 'done', desc);
    h += '<div class="tkdl">';
    if (!shown.length) h += `<p class="muted">${T.histNone}</p>`;
    let last = null;
    shown.forEach(t => {
      const k = String(t.completed_at).slice(0, 10);
      if (k !== last) { last = k; h += `<div class="dd">${de(parse(k))}</div>`; }
      h += `<div class="ri" data-key="${esc(key(t))}"><span class="muted tnum">${String(t.completed_at).slice(11, 16)}</span><span class="sw" style="--c:${esc(t.color)}"></span>${esc(t.summary)}
        ${t.due ? `<span class="tchip">${ic('clock', 'xs')}${T.wasdue} ${deS(parse(t.due))}</span>` : ''}</div>`;
    });
    return h + '</div>';
  }

  const VIEWS = { groups: vGroups, table: vTable, board: vBoard, timeline: vTimeline, deck: vDeck, history: vHistory };
  /* the sentinel sits at the end of the finished tasks; the observer reaches far enough ahead
     (a screenful and a half) that the next page is there before the sentinel is on screen */
  const SENTINEL = () => DONE.length < DONE_TOTAL && view !== 'deck' ? `<div class="tksent" id="tk-sent">${esc(T.loading)}</div>` : '';
  let obs = null;
  /* the page scrolls inside <main>, not the window: an observer left on the viewport sees the
     sentinel only once it is clipped in, which is too late to be a prefetch */
  function scrollParent(el) {
    for (let p = el.parentElement; p; p = p.parentElement) {
      const o = getComputedStyle(p).overflowY;
      if ((o === 'auto' || o === 'scroll') && p.scrollHeight > p.clientHeight) return p;
    }
    return null;
  }
  function watchSentinel() {
    if (obs) obs.disconnect();
    const el = $('#tk-sent');
    if (!el) return;
    obs = new IntersectionObserver(async es => { if (es.some(e => e.isIntersecting) && !loading) { if (await loadMore()) renderIdle(); } },
      { root: scrollParent(el), rootMargin: '1200px 0px' });
    obs.observe(el);
  }

  // a redraw would take the cursor out of a cell being typed in: wait until the focus leaves the view
  function renderIdle() {
    const a = document.activeElement;
    if (a && main.contains(a) && a.matches('input, select, [contenteditable]')) { main.addEventListener('focusout', () => setTimeout(renderIdle, 0), { once: true }); return; }
    render();
  }
  function render() {
    if (!PREFIX_SAFE.includes(sort) && DONE.length < DONE_TOTAL) {
      main.innerHTML = `<div class="tksent">${esc(T.loading)}</div>`;
      ensureAll().then(render);
      return;
    }
    // the table scrolls inside its own box (sticky head and new row): keep its place across a redraw
    const sw = $('.sheetwrap', main), top = sw ? sw.scrollTop : 0;
    main.classList.toggle('view-table', view === 'table');
    main.innerHTML = (VIEWS[view] || vGroups)() + (view === 'table' ? '' : SENTINEL());
    const sw2 = $('.sheetwrap', main); if (sw2) sw2.scrollTop = top;
    watchSentinel();
    syncSort();
    alignEd();
    $$('[data-sk]', main).forEach(th => th.onclick = () => { if (sort === th.dataset.sk) desc = !desc; else { sort = th.dataset.sk; desc = false; } localStorage.tkSort = sort; localStorage.tkDesc = desc ? '1' : '0'; render(); });
    $$('[data-d]', main).forEach(el => el.onclick = async () => { pick = pick === el.dataset.d ? null : el.dataset.d; if (pick) await ensureUntil(pick); render(); });
    $$('.tkkc[draggable]', main).forEach(c => c.ondragstart = e => e.dataTransfer.setData('text/plain', c.dataset.key));
    $$('.tkcol', main).forEach(col => {
      col.ondragover = e => e.preventDefault();
      col.ondrop = async e => { e.preventDefault(); await setStatus(e.dataTransfer.getData('text/plain'), col.dataset.st); };
    });
    const deck = $('.tkdeck', main);
    if (deck) deck.onkeydown = e => {
      const list = sortView(TASKS);
      if (e.key === 'j' || e.key === 'ArrowDown') { deckGo(Math.min(list.length - 1, deckI + 1)); if (deckI > list.length - 6) loadMore().then(d => d && render()); render(); $('.tkdeck', main).focus(); }
      if (e.key === 'k' || e.key === 'ArrowUp') { deckGo(Math.max(0, deckI - 1)); render(); $('.tkdeck', main).focus(); }
      if (e.key === ' ') { e.preventDefault(); const t = list[deckI]; if (t) toggle(key(t)); }
      if (e.key === 'e') { e.preventDefault(); const t = list[deckI]; if (t) open(key(t)); }
    };
    markAct();
    if (deck && view === 'deck' && !document.activeElement.closest('input, select, textarea')) deck.focus();
  }

  main.addEventListener('click', e => {
    const tog = e.target.closest('[data-tog]');
    if (tog) { e.stopPropagation(); toggle(tog.dataset.tog); return; }
    if (e.target.closest('[data-next]')) { deckGo(deckI + 1); render(); return; }
    const holder = e.target.closest('[data-key]');
    if (!holder) return;
    // the table is its own editor; a card cell opens that one card of the editor
    if (view === 'table') { const cc = e.target.closest('.cardcell'); if (cc) openCard(holder.dataset.key, cc.dataset.card); else { act = holder.dataset.key; markAct(); } return; } // a row click picks
    open(holder.dataset.key);
  });

  // ---- writing ----
  async function put(k, body) {
    const r = await fetch('/api/tasks/' + k, { method: 'PUT', headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf }, body: JSON.stringify(body) });
    if (!r.ok) { alert(T.err + ' ' + r.status); return false; }
    return true;
  }
  // one click or key press is easily an accident: finishing a task asks first (reopening does not)
  async function toggle(k) {
    const t = TASKS.find(x => key(x) === k); if (!t) return;
    if (!isDone(t) && !await askYesNo(T.askDone.replace('%t', t.summary), T.yes, T.no)) return;
    if (await put(k, { principal: t.principal, cal_id: t.cal_id, summary: t.summary, completed: !isDone(t) })) { if (view === 'table') refresh(k); else reload(); }
  }
  async function setStatus(k, st) {
    const t = TASKS.find(x => key(x) === k); if (!t || t.status === st) return;
    const d = await (await fetch('/api/tasks/' + k)).json();
    d.status = st;
    if (st !== 'COMPLETED') { d.completed_at = null; if (d.percent === 100) d.percent = 0; } else d.percent = 100;
    if (await put(k, d)) reload();
  }
  function reload() { location.href = url(); }
  // ---- calendar filter: the calendar page's hidden set (localStorage.calHidden), sent as ?cals= like there ----
  const cf = $('#calfilter');
  if (cf) {
    const hid = new Set(JSON.parse(localStorage.calHidden || '[]')), boxes = $$('input[data-cal]', cf);
    const param = () => { const on = boxes.filter(i => i.checked).map(i => i.dataset.cal); return on.length === boxes.length ? '' : on.join(',') || '-'; };
    const apply = () => { boxes.forEach(i => { if (i.checked) hid.delete(i.dataset.cal); else hid.add(i.dataset.cal); }); localStorage.calHidden = JSON.stringify([...hid]); root.dataset.cals = param(); reload(); };
    boxes.forEach(i => { i.checked = !hid.has(i.dataset.cal); i.addEventListener('change', apply); });
    $$('[data-filter]', cf).forEach(a => a.addEventListener('click', e => { e.preventDefault(); boxes.forEach(i => { i.checked = a.dataset.filter === 'all' || (a.dataset.filter === 'mine' && !i.parentElement.querySelector('small')); }); apply(); }));
    // a filter set on the calendar page applies here too; a rule link (?rule=) keeps its page
    if (param() !== (root.dataset.cals || '') && !new URL(location.href).searchParams.has('rule')) { root.dataset.cals = param(); location.href = url() + location.hash; }
  }
  function url(v) { const u = new URL('/tasks', location.origin); u.searchParams.set('view', v || view); u.searchParams.set('back', root.dataset.back); if (root.dataset.cals) u.searchParams.set('cals', root.dataset.cals); return u.toString(); }

  // ---- the editor, placed the way the contacts page places its own: the groups view on a wide
  // window is list | editor and the editor is always there — with nothing picked it is the form for
  // a new task. Every other view (and a narrow window) opens it as the calendar's dialog. ----
  const MIN_DOCK = 1100;
  let held;       // what the editor holds: a task key, null = the new-task form, undefined = nothing
  let wantNew = false; // a new task asked for through + (outside the docked pane it is a dialog)
  const docked = () => view === 'groups' && innerWidth >= MIN_DOCK;
  function load(k, quiet) { held = k; window.rusticalEd.open(k, undefined, quiet); }
  /* the round + (bottom left) adds a task where nothing else can: table, board, timeline and cards,
     and the groups view when its editor is not docked; the history shows finished work only */
  const FAB_VIEWS = ['table', 'board', 'timeline', 'deck'];
  function fit() {
    const dock = docked();
    const fab = $('#fab'), withFab = FAB_VIEWS.includes(view) || (view === 'groups' && !dock);
    fab.hidden = !withFab; fab.classList.toggle('fab-add', withFab);
    root.classList.toggle('dock', dock);
    ed.classList.toggle('docked', dock);
    if (dock) { ed.hidden = false; if (held === undefined) load(null, true); }
    else ed.hidden = !(cur || wantNew);
    alignEd();
  }
  addEventListener('resize', fit);
  // the docked pane starts level with the first task (below the first group heading)
  function alignEd() {
    const c = docked() && $('.tkcard', main);
    ed.style.setProperty('--ed-off', c ? Math.round(c.getBoundingClientRect().top - main.getBoundingClientRect().top) + 'px' : '0px');
  }
  function open(k) {
    cur = act = k;
    render();
    load(k);
    fit();
  }
  function openCard(k, card) { cur = act = k; held = k; markAct(); window.rusticalEd.open(k, undefined, false, card); fit(); }
  function newTask() {
    cur = null; wantNew = true; render(); load(null); fit();
  }
  $('#fab').onclick = newTask;

  // what the editor module asks of the page it runs on
  window.rusticalHost = {
    kind: 'task',
    date: () => iso(TODAY),
    cals: () => root.dataset.cals || '',
    // after a save the docked pane shows the saved task again (the contacts page keeps it in the hash)
    reload: (push, saved) => {
      if (saved && view === 'table' && find(saved)) { refresh(saved); return; } // a card saved from a cell: redraw that row only
      if (saved && docked()) sessionStorage.tkSel = saved;
      reload();
    },
    // closing the docked pane falls back to the new-task form; a dialog just goes away
    closed: () => { if (docked()) act = null; cur = null; wantNew = false; held = undefined; render(); fit(); }, // the docked pane falls back to the new-task form: nothing is picked
  };

  $('#tk-back').onchange = e => { document.cookie = `tk_back=${e.target.value}; path=/tasks; max-age=31536000; SameSite=Lax`; const u = new URL(url()); u.searchParams.set('back', e.target.value); location.href = u.toString(); }; // the cookie: the page reached from the navigation keeps the choice (routes/tasks.rs)
  // ---- the picked task: a search hit, a click or a view switch keeps it marked and in view where the view shows it ----
  function markAct() { $$('.sel[data-key]', main).forEach(el => el.classList.remove('sel')); if (act) $$(`[data-key="${CSS.escape(act)}"]`, main).forEach(el => el.classList.add('sel')); }
  function deckTo() { if (!act) return; const i = sortView(TASKS).findIndex(t => key(t) === act); if (i >= 0) deckI = i; }
  function showAct() { // false: this view has no place for it (a history day, a finished task on the deck …)
    if (!act) return false;
    if (docked() && held !== act && find(act)) open(act); // the docked pane shows the picked task
    const el = $(`[data-key="${CSS.escape(act)}"]`, main); if (!el) return false;
    el.scrollIntoView({ block: 'center' }); return true;
  }
  function setView(v) {
    view = v;
    segPress($(`#tk-views button[data-tkview="${v}"]`));
    $('#tk-colpick').hidden = view !== 'table';
    history.replaceState(null, '', url());
    // the editor does not travel between views (the dialog would pop up in the next one); the pick does
    cur = null; wantNew = false; held = undefined;
    deckTo(); render(); fit();
  }
  $$('#tk-views button').forEach(b => b.onclick = () => { setView(b.dataset.tkview); showAct(); });
  async function reveal(k) {
    act = k;
    if (!find(k)) await ensureAll();
    if (!find(k) && root.dataset.back !== 'all') { const u = new URL(url()); u.searchParams.set('back', 'all'); location.href = u + '#' + encodeURIComponent(k); return; } // finished before the lookback: widen it, same view
    if (!find(k)) { cur = k; load(k); fit(); return; } // hidden by the calendar filter: the editor still shows it
    deckTo(); render();
    if (!showAct()) { setView('groups'); showAct(); } // this view cannot show it: the list can
  }
  window.rusticalTasks = { reveal };
  addEventListener('popstate', () => location.reload());

  // back from a save in the docked pane: show what was saved
  const sel = sessionStorage.tkSel; delete sessionStorage.tkSel;
  if (sel && docked() && find(sel)) open(sel); else { fit(); render(); }
  // a link to one task (#key, e.g. a search hit from another page)
  const hk = decodeURIComponent(location.hash.slice(1)); if (hk) { history.replaceState(null, '', url()); reveal(hk); }
})();

/* ===== settings: import files, browser push ===== */
(function () {
  const settings = document.querySelector('.settings');
  if (!settings) return;
  const csrf = document.querySelector('meta[name=csrf]').content;
  const lang = document.body.dataset.lang || 'de';
  const T = { ...trs('js-set-', ['err', 'active', 'denied', 'unsupported']), done: n => tr('js-set-imported', { n }) };
  // import: read the chosen file, POST its bytes (no multipart)
  settings.querySelectorAll('input[type=file][data-import]').forEach(inp => inp.addEventListener('change', async () => {
    const file = inp.files[0]; if (!file) return;
    const r = await fetch(inp.dataset.import, { method: 'POST', headers: { 'Content-Type': 'application/octet-stream', 'X-CSRF-Token': csrf }, body: await file.arrayBuffer() });
    if (r.ok) { const d = await r.json(); sessionStorage.toast = T.done(d.imported); location.reload(); } else toast(T.err + ' ' + r.status + ': ' + (await r.text()).slice(0, 300), true);
    inp.value = '';
  }));
  // import into a NEW calendar or address book (.imp-form, data-kind = the route's collection kind): the page
  // reads the file first — for a calendar its calendar-level X-WR-* lines (the ones RustiCal's own IMPORT
  // reads, plus Apple's colour) prefill the form and the types it holds are switched on and locked; a .vcf
  // carries no book metadata, so its name comes from the file name. The server creates the collection
  // and its objects in one transaction.
  const unfold = text => text.replace(/\r?\n[ \t]/g, '').split(/\r?\n/);
  const READ = {
    calendars: text => {
      const head = [], uids = { VEVENT: new Set(), VTODO: new Set(), VJOURNAL: new Set() };
      let cur = null, body = false;
      for (const l of unfold(text)) {
        const u = l.toUpperCase(), b = u.match(/^BEGIN:(VEVENT|VTODO|VJOURNAL|VTIMEZONE)$/);
        if (b) { body = true; if (b[1] !== 'VTIMEZONE') cur = b[1]; continue; }
        if (/^END:(VEVENT|VTODO|VJOURNAL)$/.test(u)) { cur = null; continue; }
        if (!body) head.push(l); else if (cur && /^UID[:;]/.test(u)) uids[cur].add(l.slice(l.indexOf(':') + 1).trim()); // overrides share their series' UID: one object
      }
      const prop = n => { const l = head.find(x => x.toUpperCase().startsWith(n + ':') || x.toUpperCase().startsWith(n + ';')); return l ? l.slice(l.indexOf(':') + 1).replace(/\\[nN]/g, '\n').replace(/\\([,;\\])/g, '$1').trim() : ''; };
      return { name: prop('X-WR-CALNAME'), desc: prop('X-WR-CALDESC'), color: prop('X-WR-CALCOLOR') || prop('X-APPLE-CALENDAR-COLOR'), tz: prop('X-WR-TIMEZONE'), n: { ev: uids.VEVENT.size, td: uids.VTODO.size, jr: uids.VJOURNAL.size }, types: { ev: 'VEVENT', td: 'VTODO', jr: 'VJOURNAL' } };
    },
    addressbooks: text => {
      let cards = 0, groups = 0, card = false, grp = false;
      for (const l of unfold(text)) {
        const u = l.toUpperCase();
        if (u === 'BEGIN:VCARD') { card = true; grp = false; } else if (u === 'END:VCARD' && card) { card = false; if (grp) groups++; else cards++; }
        else if (card && /^(KIND|X-ADDRESSBOOKSERVER-KIND):GROUP$/.test(u)) grp = true;
      }
      return { n: { ct: cards, gr: groups } };
    },
  };
  const hexOf = v => { const m = v.match(/^#?([0-9a-f]{6})([0-9a-f]{2})?$/i); return m ? '#' + m[1].toLowerCase() : (window.colorPicker && window.colorPicker.toHex(v)) || ''; }; // #rrggbbaa (Apple) loses its alpha
  const count = (k, n) => n === 1 ? tr(`js-imp-${k}1`) : tr(`js-imp-${k}`, { n });
  settings.querySelectorAll('.imp-form').forEach(form => {
    const kind = form.dataset.kind, q = s => form.querySelector(s), el = form.elements, fields = q('.imp-fields'), info = q('.imp-info'), go = q('.imp-go'), goText = go.textContent;
    let buf = null;
    q('.imp-file').addEventListener('change', async e => {
      const f = e.target.files[0]; if (!f) return;
      buf = await f.arrayBuffer(); const d = READ[kind](new TextDecoder().decode(buf)), taken = [];
      const found = Object.keys(d.n).filter(k => d.n[k]);
      q('.imp-pick span').textContent = tr('js-imp-other');
      if (!found.length) { buf = null; fields.hidden = true; info.textContent = ''; info.append(tr('js-imp-file', { file: f.name }) + ' · ', Object.assign(document.createElement('span'), { className: 'danger', textContent: tr(kind === 'calendars' ? 'js-imp-empty' : 'js-imp-ab-empty') })); return; }
      el.name.value = d.name || f.name.replace(/\.(ics|ical|ifb|icalendar|vcf|vcard)$/i, ''); if (d.name) taken.push(tr('js-imp-f-name'));
      el.description.value = d.desc || ''; if (d.desc) taken.push(tr('js-imp-f-desc'));
      const hex = d.color && hexOf(d.color);
      el.color.value = hex || form.dataset.defaultColor; q('.cpk-btn').style.setProperty('--c', el.color.value); if (hex) taken.push(tr('js-imp-f-color'));
      if (el.timezone) { el.timezone.value = d.tz || form.dataset.defaultTz; if (d.tz) taken.push(tr('js-imp-f-tz')); }
      form.querySelectorAll('input[name=comp]').forEach(c => { const has = found.some(k => d.types[k] === c.value); c.checked = has; c.disabled = has; c.closest('label').title = has ? tr('js-imp-in-file') : ''; });
      info.textContent = [tr('js-imp-file', { file: f.name }), ...found.map(k => count(k, d.n[k]))].join(' · ') + (taken.length ? ' · ' + tr('js-imp-taken', { what: taken.join(', ') }) : '');
      fields.hidden = false; el.name.focus(); el.name.select();
    });
    form.addEventListener('submit', async e => {
      e.preventDefault(); if (!buf || !form.reportValidity()) return;
      go.disabled = true; go.textContent = tr('js-imp-busy');
      const p = new URLSearchParams({ owner: el.owner.value, name: el.name.value.trim(), color: el.color.value, description: el.description.value.trim() });
      if (kind === 'calendars') { p.set('timezone', el.timezone.value.trim()); p.set('components', [...form.querySelectorAll('input[name=comp]')].filter(c => c.checked).map(c => c.value).join(',')); }
      const r = await fetch(`/settings/import/${kind}?${p}`, { method: 'POST', headers: { 'Content-Type': kind === 'calendars' ? 'text/calendar' : 'text/vcard', 'X-CSRF-Token': csrf }, body: buf });
      if (r.ok) { const d = await r.json(); sessionStorage.toast = tr('js-imp-done', { title: el.name.value.trim(), count: d.imported }); location.hash = `#${kind === 'calendars' ? 'cal' : 'ab'}-${d.owner}-${d.id}`; location.reload(); return; }
      toast(T.err + ' ' + r.status + ': ' + (await r.text()).slice(0, 300), true); go.disabled = false; go.textContent = goText;
    });
  });
  // push
  const en = document.getElementById('push-enable'), dis = document.getElementById('push-disable'), st = document.getElementById('push-status'), list = document.getElementById('push-devices');
  if (!en) return;
  const b64 = s => { const p = '='.repeat((4 - s.length % 4) % 4); const raw = atob((s + p).replace(/-/g, '+').replace(/_/g, '/')); return Uint8Array.from(raw, c => c.charCodeAt(0)); };
  async function refresh() {
    try { const subs = await (await fetch('/api/push/subscriptions')).json(); list.innerHTML = subs.map(s => `<li><small class="muted">${(s.user_agent || '').slice(0, 60)} · ${s.created_at}</small></li>`).join('') || ''; } catch (_) {}
    if (!('serviceWorker' in navigator) || !('PushManager' in window)) { st.textContent = T.unsupported; en.disabled = true; return; }
    const reg = await navigator.serviceWorker.getRegistration('/'); const sub = reg && await reg.pushManager.getSubscription();
    en.hidden = !!sub; dis.hidden = !sub; st.textContent = sub ? T.active : '';
  }
  en.onclick = async () => {
    try {
      const perm = await Notification.requestPermission(); if (perm !== 'granted') { st.textContent = T.denied; return; }
      const reg = await navigator.serviceWorker.register('/assets/sw.js', { scope: '/' });
      const key = (await (await fetch('/api/push/key')).json()).key;
      const sub = await reg.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: b64(key) });
      const j = sub.toJSON();
      const r = await fetch('/api/push/subscriptions', { method: 'POST', headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf }, body: JSON.stringify({ endpoint: j.endpoint, keys: j.keys }) });
      if (!r.ok) throw new Error(r.status);
      refresh();
    } catch (e) { st.textContent = T.err + ': ' + e.message; }
  };
  dis.onclick = async () => {
    const reg = await navigator.serviceWorker.getRegistration('/'); const sub = reg && await reg.pushManager.getSubscription();
    if (sub) { await fetch('/api/push/subscriptions', { method: 'DELETE', headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf }, body: JSON.stringify({ endpoint: sub.endpoint }) }); await sub.unsubscribe(); }
    refresh();
  };
  refresh();
})();
