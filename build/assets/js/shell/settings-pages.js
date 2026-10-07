// js/shell/settings-pages.js — part of the shell IIFE (src/bundle.rs; shares its scope): the settings pages — drill-in per hash, autosave, region
// pickers, segments, danger zones, reminder snap points, vacation rule, categories index
  // settings: grouped lists with drill-in pages, one <section class="page"> per URL hash (without JS all are shown)
  const settings = $('.settings');
  if (settings) {
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
      const e2 = practical.escQ;
      const when = t => { const d = new Date(String(t).replace(' ', 'T') + 'Z'); return isNaN(d) ? '' : dfmt(d, { day: 'numeric', month: 'short', year: 'numeric', hour: '2-digit', minute: '2-digit' }); }; // stored in UTC
      const drawHist = async () => {
        const r = await practical.req('/api/search/history'), H = r.ok ? r.data : [];
        $('#hist-n', hp).textContent = H.length ? `(${H.length})` : ''; clr.disabled = !H.length;
        list.innerHTML = H.length ? H.map(e => `<div class="cell noic" data-id="${e.id}"><span>${e2(e.q)}<span class="sub">${e2([when(e.used_at), e.uses > 1 ? D.tUses.replace('{n}', e.uses) : '', e.target && e.target.label ? '→ ' + e.target.label : ''].filter(Boolean).join(' · '))}</span></span><span class="val"><button type="button" class="btn icon-btn" data-hrm title="${e2(D.tRm)}" aria-label="${e2(D.tRm)}">${ico('x')}</button></span></div>`).join('')
          : `<div class="cell noic"><span class="muted">${e2(D.tEmpty)}</span></div>`;
      };
      list.addEventListener('click', async e => { const b = e.target.closest('[data-hrm]'); if (!b) return; practical.tell(await practical.req('/api/search/history/' + b.closest('[data-id]').dataset.id, { method: 'DELETE' })); drawHist(); });
      clr.addEventListener('click', async () => { if (!await ask(D.tAsk)) return; practical.tell(await practical.req('/api/search/history', { method: 'DELETE' })); drawHist(); });
      const onShow = () => { if (location.hash === '#search') drawHist(); };
      addEventListener('hashchange', onShow); onShow();
      // the home row names the choice; a shorter one drops old entries (the list shows the result after the save)
      $('#hist-days', hp).addEventListener('change', e => { $('#v-search').textContent = e.target.selectedOptions[0].textContent; setTimeout(drawHist, 600); });
    }
    // autosave: a changed field of a [data-autosave] form posts the whole form; the server answers JSON. One post at a
    // time per form, the newest state last (two at once could each carry one change only: the later answer would win)
    const save = (form, w) => w.run(async () => {
      const r = await practical.req(form.action, { method: 'POST', headers: { Accept: 'application/json' }, body: new URLSearchParams(new FormData(form)) });
      return !r.ok || (r.data && r.data.ok) ? r : { ok: false, kind: 'invalid', msg: (r.data && r.data.msg) || tr('js-req-server') };
    }, true).then(r => r && r.ok && (r.data.msg || settings.dataset.tSaved));
    $$('form[data-autosave]', settings).forEach(form => {
      const w = practical.writer(form);
      form.addEventListener('submit', e => e.preventDefault()); // Enter in a field saves through change, never navigates
      form.addEventListener('change', async e => {
        if (!e.target.name || e.target.type === 'file' || !form.reportValidity()) return;
        const msg = await save(form, w); if (!msg) return;
        if (e.target.name === 'scheme') applyScheme(e.target.value);
        if (form.hasAttribute('data-reload') || e.target.hasAttribute('data-reload')) { sessionStorage.toast = msg; practical.leaving = true; location.reload(); } else toast(msg);
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
    const loadSubs = async c => { if (subsFor !== c) { const r = practical.tell(await practical.req('/api/holidays/subdivisions?country=' + encodeURIComponent(c))); if (!r.ok) return []; subs = r.data; subsFor = c; } return subs; };
    const esc2 = practical.esc;
    const commit = (name, value) => { const i = hid(name); i.value = value; i.dispatchEvent(new Event('change', { bubbles: true })); };
    const pathOf = (list, r) => { const out = []; let x = r; while (x && out.length < 5) { out.unshift(x.name); x = list.find(y => y.code === x.parent); } return out; };
    const countryName = c => (pickData.countries.find(x => x.code === c) || { name: c }).name;
    const listOf = kind => hid(kind + '_regions').value.split(',').filter(Boolean);
    const nameOf = c => (names[c] || [c])[0];
    const summary = kind => { const l = listOf(kind); $('#v-regions-' + kind).textContent = !l.length ? tr('js-regions-none') : l.length < 3 ? l.map(nameOf).join(', ') : tr('js-regions-n', { n: l.length }); };
    const setList = (kind, l) => { commit(kind + '_regions', l.join(',')); summary(kind); $('#v-sources').hidden = true; /* the sources line is the server's; it returns with the next visit */ };
    const icoBtn = (attr, ico, label) => `<button type="button" class="btn icon-btn" ${attr} title="${esc2(label)}" aria-label="${esc2(label)}"><svg class="ico" aria-hidden="true"><use href="${practical.sprite}#${ico}"/></svg></button>`;
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
        backTo('#', $('#home h1').textContent);
        title.textContent = tr('js-pick-tz'); const cur = hid('timezone').value; on = v => v === cur;
        rows = [{ v: '', name: tr('js-pick-default', { tz: prefsForm.dataset.defaultTz }) }];
        pickData.zones.forEach(([area, zs]) => { rows.push({ head: area }); zs.forEach(([id, name]) => rows.push({ v: id, name, sub: id })); });
      } else if (cm) {
        kind = cm[1]; backTo('#regions-' + kind, $(`#regions-${kind} h1`).textContent);
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
          return `<button type="button" class="cell noic link${sel ? ' on' : ''}" data-v="${esc2(r.v)}"${r.more ? ' data-more="1"' : ''} role="option" aria-selected="${sel}"><span>${esc2(r.name)}${r.sub && (f || route !== 'tz') ? `<span class="sub">${esc2(r.sub)}</span>` : ''}</span><span class="val">${n ? `<span class="badge">${n}</span>` : ''}${sel ? `<svg class="ico" aria-hidden="true"><use href="${practical.sprite}#i-check"/></svg>` : ''}${r.more ? '<span class="chev">›</span>' : ''}</span></button>`; }).join('')
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
      w.innerHTML = '<form class="ask-box"><p></p><label class="ask-type"><span></span> <b></b><input type="text" autocomplete="off"></label><div class="ask-acts"><button class="btn icon-btn" type="button"><svg class="ico" aria-hidden="true"><use href="' + practical.sprite + '#i-x"/></svg></button><button class="btn danger icon-btn" type="submit" disabled><svg class="ico" aria-hidden="true"><use href="' + practical.sprite + '#i-trash-2"/></svg></button></div></form>';
      $('p', w).textContent = ($('p', form) || {}).textContent || ''; w.setAttribute('aria-label', $('p', w).textContent || go.textContent.trim()); $('.ask-type span', w).textContent = form.dataset.tType; $('.ask-type b', w).textContent = want;
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
      const label = m => { const [u, k] = unitOf(m), n = m / u; return tr('js-snp-' + k, { n }); };
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
        $('#v-snaps').textContent = tr('js-snaps-n', { n: L.start.length });
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
          row = `<div class="cell noic snp-edit"><span>${esc2(label(edit.m) + ' ' + side(edit.key))}<span class="sub snp-err" role="alert"></span></span><span class="val"><input type="number" min="1" step="1" value="${edit.m / u}" aria-label="${esc2(tr('js-snp-amount'))}"><span class="seg">${UNITS.map(([uu, k]) => `<button type="button" data-u="${uu}" aria-pressed="${uu === u}">${esc2(tr('js-snp-u-' + k))}</button>`).join('')}</span><button type="button" class="btn" data-ok>${ico('pencil')}<span>${esc2(tr('js-snp-change'))}</span></button><button type="button" class="btn danger" data-del${last ? ` disabled title="${esc2(tr('js-snp-last'))}"` : ''}>${ico('trash')}<span>${esc2(tr('js-snp-remove'))}</span></button><button type="button" class="btn icon-btn" data-close title="${esc2(tr('js-snp-close'))}" aria-label="${esc2(tr('js-snp-close'))}">${ico('x')}</button></span></div>`;
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
    // the vacation rule and the categories page both start with the event categories: one request for both
    let catsFirst = vacKind && $('#vac-re') && $('#categories') ? practical.req('/api/categories?scope=event').then(r => r.ok ? r.data : []) : null;
    if (vacKind && $('#vac-re')) {
      const catSel = $('#vac-cat'), calSel = $('#vac-cal'), re = $('#vac-re'), tools = $('#vac-tools'), test = $('#vac-test');
      let titles = null, cats = null, shown = 'match';
      const summary = () => { const k = vacKind.value; $('#v-vacation').textContent = k === 'title' ? `/${re.value}/` : `„${k === 'calendar' ? (calSel.selectedOptions[0] || {}).textContent || '' : catSel.value}“`; };
      const loadCats = async () => {
        if (cats) return; cats = [];
        try { if (catsFirst) cats = await catsFirst; else { const r = await fetch('/api/categories?scope=event'); if (r.ok) cats = await r.json(); } } catch (_) { cats = []; }
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
    /* categories: the index with counts; a click (or the new-category form) opens the category's rule in the editor over
       this page. The dialog (GET /editor?kind=, routes/editor.rs) is loaded on first use and again when the scope changes:
       one #ed per page (its ids and JSON islands are the editor's globals), the other kind's leaves the page first, hidden
       (its window listeners — Escape, the map's and the strip's keys, the kept work, the leave-page question — all ask
       `ed.hidden` and do nothing). The link's href stays the calendar's or tasks' deep link (`?rule=&back=`): a new tab
       or a middle click lands there, as before. */
    const catPanel = $('#categories');
    if (catPanel) {
      const t = catPanel.dataset, list = $('#cat-list'), h = practical.esc, newName = $('#cat-new-name');
      let scope = 'event', edKind = null, edChain = Promise.resolve();
      const ruleUrl = n => (scope === 'task' ? '/tasks' : '/calendar') + '?rule=' + encodeURIComponent(n) + '&back=' + encodeURIComponent('/settings#categories');
      const edUrl = k => '/editor?kind=' + k;
      const load = async () => {
        const first = scope === 'event' && catsFirst; catsFirst = null;
        let d; if (first) d = await first; else { const r = practical.tell(await practical.req('/api/categories?scope=' + scope)); d = r.ok ? r.data : []; }
        if (scope === 'event') $('#cat-total').textContent = d.length;
        const a = document.activeElement, back = list.contains(a) && a.firstElementChild ? a.firstElementChild.textContent : null; // the row the closed editor gave the focus back to
        list.innerHTML = d.length ? d.map(c => `<a class="cell noic link" href="${ruleUrl(c.name)}"><span>${h(c.name)}</span><span class="val">${c.rule ? `<span class="badge">${h(t.tRule)}</span>` : ''}${c.count ? h(plural(scope === 'task' ? t.tCountTask : t.tCount, c.count).replace('{n}', c.count)) : ''}<span class="chev">›</span></span></a>`).join('') : `<div class="cell noic"><span class="muted">${h(t.tNone)}</span></div>`;
        if (back != null) { const row = $$('a', list).find(x => x.firstElementChild.textContent === back); if (row) row.focus(); }
      };
      // the editor of kind `k` in the page (true), one load at a time: a later one waits, so the last scope asked for wins
      const editor = k => (edChain = edChain.catch(() => false).then(async () => {
        if (edKind === k) return true;
        const cur = $('#ed-host'), ed = cur && $('#ed', cur); if (ed && !ed.hidden) return false; // never under an open dialog
        let html = await practical.prefetched(edUrl(k));
        if (html == null) { const r = practical.tell(await practical.req(edUrl(k), { headers: { Accept: 'text/html' }, as: 'text', wait: 300 })); if (!r.ok) return false; html = r.data; }
        const box = document.createElement('div'); box.innerHTML = html; const host = $('#ed-host', box); if (!host) return false;
        const old = $('#ed-host'); if (old) { $$('#ed, #geomap', old).forEach(x => { x.hidden = true; }); old.remove(); }
        settings.closest('.main').appendChild(host); edKind = k; practical.editorInit();
        return true;
      }));
      const openRule = async n => { const k = scope; if (await editor(k) && scope === k) rusticalEd.rule(n); };
      // the editor's host hooks (js/editor/setup.js HOST): a saved or deleted rule redraws the list (badge, counts)
      window.rusticalHost = { rulesChanged: n => { if (newName.value.trim() === n) newName.value = ''; load(); } };
      list.addEventListener('click', e => { const a = e.target.closest('a'); if (!a || e.button || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return; e.preventDefault(); openRule(a.firstElementChild.textContent); });
      practical.intentIn(list, 'a', () => edKind === scope ? [] : [edUrl(scope)]);
      $$('#cat-scope button').forEach(b => b.onclick = () => { scope = b.dataset.scope; segPress(b); load(); });
      $('#cat-new').addEventListener('submit', e => { e.preventDefault(); const n = newName.value.trim(); if (n) openRule(n); });
      // a rule kept over a lost session (the editor's practical.keep → stash on this path): its editor loads at once and takes
      // the rule back itself (js/editor/wiring.js, unstash)
      const kept = k => { try { const s = JSON.parse(sessionStorage['practical.stash.ed-' + k] || 'null'); return !!s && s.path === location.pathname; } catch (_) { return false; } };
      const k0 = ['event', 'task'].find(kept);
      if (k0) { scope = k0; segPress($(`#cat-scope button[data-scope="${k0}"]`)); if (location.hash !== '#categories') location.hash = '#categories'; editor(k0); }
      load();
    }
  }
