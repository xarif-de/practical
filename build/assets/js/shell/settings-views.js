// js/shell/settings-views.js — part of the shell IIFE (src/bundle.rs; shares its scope): settings #views-year,
// #views-vacation, #views-countdown (domain::views). One GET /api/views fills all three; every change sends the whole
// configuration (PUT /api/views, one at a time, the newest last) and takes the ids the server made.
  const vwRoots = $$('.settings .vw[data-view]');
  if (vwRoots.length) {
    const h = practical.escQ, setEl = $('.settings');
    let D = null, loading = null, chain = Promise.resolve(), cats = [], open = null, iconsFor = null, timer = 0;
    const cfgOf = v => v === 'vacation' ? D.config.vacation : D.config[v];
    const names = () => Object.fromEntries(D.calendars.map(c => [c.key, c.name]));
    const calName = k => names()[k] || tr('js-vw-cal-gone');
    const cs = r => r.categories || [];
    const label = r => r.name || cs(r).join(', ') || (r.cals || []).map(calName).join(', ');
    const q = c => tr('js-vw-q', { c });
    const catText = r => { const l = cs(r); return !l.length ? (r._pc ? tr('js-vw-cats-none') : tr('js-vw-cat-any')) : tr(l.length > 1 ? 'js-vw-cats' : 'js-vw-cat', { c: l.map(q).join(', ') }); };
    // `theirs`: an offered rule, whose "all" means the sharer's calendars the user sees
    const scope = (r, theirs) => [catText(r), r.cals ? (r.cals.length ? r.cals.map(calName).join(', ') : tr('js-vw-cals-none')) : tr(theirs ? 'js-vw-cals-theirs' : 'js-vw-cals-all')].join(' · ');
    const valid = r => !!(cs(r).length || (r.cals && r.cals.length));
    const sw = (color, icon) => `<span class="swatch vw-sw"${color ? ` style="background:${h(color)}"` : ''}>${icon ? ico(icon) : ''}</span>`;
    const toggle = (attr, on, lbl) => `<span class="toggle"><input type="checkbox" ${attr}${on ? ' checked' : ''} aria-label="${h(lbl)}"><i></i></span>`;
    const icoName = i => tr('js-vw-ico-' + i);

    // the rows' counts on #views and on the settings' start page
    const counts = () => {
      const n = v => cfgOf(v).rules.length;
      ['year', 'vacation', 'countdown'].forEach(v => { const el = $('#v-views-' + v); if (el) el.textContent = tr('js-vw-rules', { n: n(v) }); });
      const all = $('#v-views'); if (all) all.textContent = tr('js-vw-rules', { n: n('year') + n('vacation') + n('countdown') });
    };

    const save = (delay = 0) => {
      clearTimeout(timer);
      // a rule without a category and without a calendar would mark every event: not sent until it names one
      const bad = ['year', 'vacation', 'countdown'].some(v => cfgOf(v).rules.some(r => !valid(r)));
      $$('.vw-err').forEach(e => { e.hidden = !bad || !e.closest('.vw-ed'); });
      if (bad) return;
      timer = setTimeout(() => {
        const body = JSON.parse(JSON.stringify(D.config));
        chain = chain.then(async () => {
          const r = practical.tell(await practical.req('/api/views', { method: 'PUT', json: body }));
          if (!r.ok) return;
          // (a new rule carries the id made here, so no answer has to be matched back to a rule that may have moved)
          D.people = r.data.people; counts(); toast(setEl.dataset.tSaved);
        });
      }, delay);
    };

    const iconGrid = (cur, none) => `<div class="vw-icons" role="group" aria-label="${h(tr('js-vw-icon'))}">${none ? `<button type="button" data-ic="" aria-pressed="${!cur}" title="${h(tr('js-vw-icon-none'))}" aria-label="${h(tr('js-vw-icon-none'))}">–</button>` : ''}${D.icons.map(i => `<button type="button" data-ic="${i}" aria-pressed="${cur === i}" title="${h(icoName(i))}" aria-label="${h(icoName(i))}">${ico(i)}</button>`).join('')}</div>`;

    // one own rule open for editing below its row
    const editor = (v, r) => {
      // categories: "any" or a choice (r._pc: chosen "choice" with none ticked yet; the server never sees it)
      const pick = !!r.cals, pickC = !!(cs(r).length || r._pc), lower = new Set(cs(r).map(c => c.toLowerCase()));
      const catList = [...cs(r), ...cats.filter(c => !lower.has(c.toLowerCase()))];
      const known = new Set(D.calendars.map(c => c.key)), gone = (r.cals || []).filter(k => !known.has(k));
      return `<div class="vw-ed">
        <label class="cell noic"><span>${h(tr('js-vw-name'))}</span><span class="val"><input type="text" data-f="name" maxlength="60" value="${h(r.name)}" placeholder="${h(label({ ...r, name: '' }) || tr('js-vw-optional'))}"></span></label>
        <div class="cell noic"><span>${h(tr('js-vw-categories'))}</span><span class="val"><span class="seg"><button type="button" data-catm="any" aria-pressed="${!pickC}">${h(tr('js-vw-cat-any-short'))}</button><button type="button" data-catm="pick" aria-pressed="${pickC}">${h(tr('js-vw-cals-pick'))}</button></span></span></div>
        ${pickC ? `<div class="vw-cats" role="group" aria-label="${h(tr('js-vw-categories'))}">${catList.map(c => `<button type="button" class="chip" data-cat="${h(c)}" aria-pressed="${lower.has(c.toLowerCase())}">${h(c)}</button>`).join('')}<input type="text" class="chip vw-newcat" data-newcat maxlength="100" placeholder="＋ ${h(tr('js-vw-cat-new'))}" aria-label="${h(tr('js-vw-cat-new'))}"></div>` : ''}
        <div class="cell noic"><span>${h(tr('js-vw-calendars'))}</span><span class="val"><span class="seg"><button type="button" data-cals="all" aria-pressed="${!pick}">${h(tr('js-vw-cals-all'))}</button><button type="button" data-cals="pick" aria-pressed="${pick}">${h(tr('js-vw-cals-pick'))}</button></span></span></div>
        ${pick ? `<div class="vw-cals">${D.calendars.map(c => `<label class="cell noic"><span><i class="vw-dot" style="background:${h(c.color)}"></i>${h(c.name)}${c.by ? `<span class="sub">${h(c.by)}</span>` : ''}</span><span class="val">${toggle(`data-cal="${h(c.key)}"`, r.cals.includes(c.key), c.name)}</span></label>`).join('')}${gone.map(k => `<label class="cell noic"><span class="muted">${h(tr('js-vw-cal-gone'))}</span><span class="val">${toggle(`data-cal="${h(k)}"`, true, tr('js-vw-cal-gone'))}</span></label>`).join('')}</div>` : ''}
        ${v === 'year' ? `<div class="cell noic"><span>${h(tr('js-vw-color'))}</span><span class="val"><button type="button" class="vw-color" data-color style="--c:${h(r.color || '#32ade6')}" aria-label="${h(tr('js-vw-color'))}"></button></span></div>` : ''}
        ${v !== 'vacation' ? `<div class="cell noic"><span>${h(tr('js-vw-icon'))}</span><span class="val"><button type="button" class="vw-icbtn" data-icons aria-expanded="false" aria-label="${h(tr('js-vw-icon'))}">${r.icon ? ico(r.icon) : '–'}</button></span></div>` : ''}
        <p class="vw-err" role="alert" hidden>${h(tr('js-vw-invalid'))}</p>
        <div class="vw-acts dlg-acts"><button type="button" class="dbtn ghost danger" data-del>${ico('trash')}<span>${h(tr('js-ct-delete'))}</span></button><span class="grow"></span><button type="button" class="dbtn second" data-done>${h(tr('js-vw-done'))}</button></div>
      </div>`;
    };

    const draw = root => {
      const v = root.dataset.view, c = cfgOf(v), out = [];
      if (v === 'vacation') out.push(`<div class="group"><label class="cell noic"><span>${h(tr('js-vw-days'))}</span><span class="val"><input type="number" min="0" max="366" data-days value="${c.days ?? ''}" placeholder="–"></span></label></div>`);
      out.push(`<div class="label">${h(tr('js-vw-own'))}</div><div class="group">`);
      c.rules.forEach((r, i) => {
        const isOpen = open && open.v === v && open.i === i;
        out.push(`<div class="cell vw-rule${valid(r) ? '' : ' bad'}" data-i="${i}">${sw(v === 'year' ? r.color : null, r.icon || (v === 'vacation' ? 'palm' : null))}<button type="button" class="vw-open" aria-expanded="${!!isOpen}"><span>${h(label(r) || tr('js-vw-new'))}<span class="sub">${h(scope(r))}</span></span></button><span class="val">${i ? `<button type="button" class="btn icon-btn" data-up title="${h(tr('js-vw-up'))}" aria-label="${h(tr('js-vw-up'))}">↑</button>` : ''}<span class="chev">${isOpen ? '⌄' : '›'}</span></span></div>`);
        if (isOpen) out.push(editor(v, r));
      });
      if (!c.rules.length) out.push(`<div class="cell noic"><span class="muted">${h(tr('js-vw-none'))}</span></div>`);
      out.push(`<button type="button" class="cell btnrow" data-add>${ico('plus')}<span>${h(tr('js-vw-add'))}</span></button></div>`);
      if (v !== 'vacation') out.push(`<p class="foot">${h(tr('js-vw-first-wins'))}</p>`);
      // offered to the people who see the user's calendars
      out.push(`<div class="label">${h(tr('js-vw-share'))}</div><div class="group"><label class="cell noic"><span>${h(tr(v === 'vacation' ? 'js-vw-share-vac' : 'js-vw-share-rules'))}</span><span class="val">${toggle('data-share', c.share, tr('js-vw-share'))}</span></label></div><p class="foot">${h(tr(v === 'vacation' ? 'js-vw-share-hint-vac' : 'js-vw-share-hint'))}</p>`);
      // what others offer
      const people = D.people.filter(p => v === 'vacation' ? p.offer.vacation : p.offer[v].length);
      out.push(`<div class="label">${h(tr('js-vw-from-others'))}</div>`);
      if (!people.length) out.push(`<div class="group"><div class="cell noic"><span class="muted">${h(tr(D.people.length ? 'js-vw-others-none' : 'js-vw-others-nobody'))}</span></div></div>`);
      else if (v === 'vacation') {
        out.push('<div class="group">' + people.map(p => { const [days, rules] = p.offer.vacation;
          return `<label class="cell noic"><span>${h(p.label)}<span class="sub">${h([days != null ? tr('js-vw-days-n', { n: days }) : '', rules.map(label).join(', ')].filter(Boolean).join(' · '))}</span></span><span class="val">${toggle(`data-person="${h(p.id)}"`, !!c.people[p.id], p.label)}</span></label>`; }).join('') + '</div>');
      } else for (const p of people) {
        const mine = c.adopt[p.id] || {};
        out.push(`<div class="label sub-label">${h(p.label)}</div><div class="group">` + p.offer[v].map(r => {
          const a = mine[r.id] || {}, color = a.color || r.color, icon = a.icon || r.icon, ic = iconsFor === p.id + '/' + r.id;
          return `<div class="cell vw-rule" data-p="${h(p.id)}" data-r="${h(r.id)}">${sw(v === 'year' ? color : null, icon)}<span>${h(label(r))}<span class="sub">${h(scope(r, true))}</span></span><span class="val">${v === 'year' ? `<button type="button" class="vw-color" data-acolor style="--c:${h(color || '#32ade6')}" title="${h(tr('js-vw-color-mine'))}" aria-label="${h(tr('js-vw-color-mine'))}"></button>` : ''}<button type="button" class="vw-icbtn" data-aicons aria-expanded="${ic}" title="${h(tr('js-vw-icon-mine'))}" aria-label="${h(tr('js-vw-icon-mine'))}">${icon ? ico(icon) : '–'}</button>${toggle('data-adopt', !!a.on, tr('js-vw-adopt', { what: label(r) }))}</span></div>${ic ? iconGrid(icon, v === 'year') : ''}`;
        }).join('') + '</div>');
      }
      out.push(`<p class="foot">${h(tr(v === 'vacation' ? 'js-vw-others-hint-vac' : 'js-vw-others-hint'))}</p>`);
      root.innerHTML = out.join('');
      root.removeAttribute('aria-busy');
    };
    const drawAll = () => { if (D) vwRoots.forEach(draw); };
    const redraw = (root, focusSel) => { draw(root); if (focusSel) { const f = $(focusSel, root); if (f) f.focus(); } };

    const load = () => loading || (loading = Promise.all([practical.req('/api/views'), practical.req('/api/categories?scope=event')]).then(([a, b]) => {
      practical.tell(a); if (!a.ok) { loading = null; return; }
      const seen = new Set();
      D = a.data; cats = b.ok ? b.data.filter(c => c.count).sort((x, y) => y.count - x.count).map(c => c.name).filter(n => !seen.has(n.toLowerCase()) && seen.add(n.toLowerCase())) : []; drawAll(); counts();
    }));
    const onShow = () => { if (/^#(views-|vacation$)/.test(location.hash)) load(); }; // #vacation: the former page's hash (settings-pages.js OLD)
    addEventListener('hashchange', onShow); onShow();

    vwRoots.forEach(root => {
      const v = root.dataset.view, c = () => cfgOf(v);
      // the open rule's row follows its fields
      const rowText = r => { const row = $(`[data-i="${open.i}"]`, root); if (row) { $('.vw-open > span', row).firstChild.textContent = label(r) || tr('js-vw-new'); $('.sub', row).textContent = scope(r); } };
      const ruleOf = el => { const row = el.closest('[data-i]') || el.closest('.vw-ed') && el.closest('.vw-ed').previousElementSibling; return row ? +row.dataset.i : -1; };
      root.addEventListener('click', e => {
        const t = e.target;
        if (t.closest('[data-add]')) {
          const r = { id: [...crypto.getRandomValues(new Uint8Array(4))].map(b => b.toString(16).padStart(2, '0')).join(''), name: '', categories: cats.length ? [cats[0]] : [], cals: cats.length ? null : [], color: null, icon: v === 'countdown' ? 'star' : null };
          if (v === 'year') { const used = new Set(c().rules.map(x => x.color)); r.color = ['#32ade6', '#30b46c', '#ff9f0a', '#af52de', '#ff2d55', '#0a84ff', '#5856d6', '#a2845e', '#ff3b30'].find(x => !used.has(x)) || '#32ade6'; }
          c().rules.push(r); open = { v, i: c().rules.length - 1 }; redraw(root, '.vw-ed [data-f=name]'); save(); return;
        }
        if (t.closest('[data-up]')) { const i = ruleOf(t), R = c().rules; [R[i - 1], R[i]] = [R[i], R[i - 1]]; if (open && open.v === v) open = null; redraw(root); save(); return; }
        if (t.closest('.vw-open')) { const i = ruleOf(t); open = open && open.v === v && open.i === i ? null : { v, i }; redraw(root, open ? '.vw-ed [data-f=name]' : `[data-i="${i}"] .vw-open`); return; }
        if (t.closest('[data-done]')) { const i = open && open.i; open = null; redraw(root, `[data-i="${i}"] .vw-open`); return; }
        if (t.closest('[data-del]')) { const i = ruleOf(t); askDelete(tr('js-vw-del-ask', { what: label(c().rules[i]) || tr('js-vw-new') })).then(ok => { if (!ok) return; c().rules.splice(i, 1); open = null; redraw(root); save(); }); return; }
        const cb = t.closest('[data-cals]');
        if (cb) { const r = c().rules[ruleOf(t)]; r.cals = cb.dataset.cals === 'all' ? null : (r.cals || []); redraw(root, `.vw-ed [data-cals="${cb.dataset.cals}"]`); save(); return; }
        const chip = t.closest('[data-cat]');
        if (chip) { const r = c().rules[ruleOf(chip.closest('.vw-ed'))], k = chip.dataset.cat, on = chip.getAttribute('aria-pressed') !== 'true';
          r.categories = on ? [...cs(r), k] : cs(r).filter(x => x.toLowerCase() !== k.toLowerCase()); chip.setAttribute('aria-pressed', on); rowText(r); save(); return; }
        const cm = t.closest('[data-catm]');
        if (cm) { const r = c().rules[ruleOf(t)]; if (cm.dataset.catm === 'any') { r.categories = []; delete r._pc; } else r._pc = true; redraw(root, `.vw-ed [data-catm="${cm.dataset.catm}"]`); save(); return; }
        if (t.closest('[data-color]')) { const r = c().rules[ruleOf(t)], b = t.closest('[data-color]');
          window.colorPicker.open(b, { title: label(r), sub: tr('js-vw-color'), value: r.color, onPick: x => { if (!x) return; r.color = x; b.style.setProperty('--c', x); const s = $(`[data-i="${ruleOf(b)}"] .vw-sw`, root); if (s) s.style.background = x; save(); } }); return; }
        if (t.closest('[data-icons]')) { const b = t.closest('[data-icons]'), cell = b.closest('.cell'), nx = cell.nextElementSibling;
          if (nx && nx.matches('.vw-icons')) { nx.remove(); b.setAttribute('aria-expanded', 'false'); return; }
          cell.insertAdjacentHTML('afterend', iconGrid(c().rules[ruleOf(t)].icon, v === 'year')); b.setAttribute('aria-expanded', 'true'); return; }
        const ic = t.closest('[data-ic]');
        if (ic) {
          const grid = ic.closest('.vw-icons'), row = grid.previousElementSibling, val = ic.dataset.ic || null;
          if (row && row.dataset.p) { const m = (c().adopt[row.dataset.p] ||= {}), a = (m[row.dataset.r] ||= { on: false, color: null, icon: null }); a.icon = val; iconsFor = null; redraw(root); save(); return; }
          const r = c().rules[ruleOf(row)]; r.icon = v === 'countdown' ? (val || 'star') : val; redraw(root, '.vw-ed [data-icons]'); save(); return;
        }
        if (t.closest('[data-acolor]')) { const b = t.closest('[data-acolor]'), row = b.closest('[data-p]'), m = (c().adopt[row.dataset.p] ||= {}), a = (m[row.dataset.r] ||= { on: false, color: null, icon: null });
          window.colorPicker.open(b, { title: $('span', row.children[1]) ? row.children[1].firstChild.textContent : '', sub: tr('js-vw-color-mine'), value: b.style.getPropertyValue('--c'), onPick: x => { if (!x) return; a.color = x; redraw(root); save(); } }); return; }
        if (t.closest('[data-aicons]')) { const row = t.closest('[data-p]'), k = row.dataset.p + '/' + row.dataset.r; iconsFor = iconsFor === k ? null : k; redraw(root, `[data-p="${CSS.escape(row.dataset.p)}"][data-r="${CSS.escape(row.dataset.r)}"] [data-aicons]`); }
      });
      root.addEventListener('change', e => {
        const t = e.target;
        if (t.matches('[data-days]')) { const n = t.value.trim() === '' ? null : Math.max(0, Math.min(366, Math.round(+t.value))); c().days = Number.isFinite(n) ? n : null; save(); return; }
        if (t.matches('[data-share]')) { c().share = t.checked; save(); return; }
        if (t.matches('[data-person]')) { c().people[t.dataset.person] = t.checked; save(); return; }
        if (t.matches('[data-adopt]')) { const row = t.closest('[data-p]'), m = (c().adopt[row.dataset.p] ||= {}); (m[row.dataset.r] ||= { on: false, color: null, icon: null }).on = t.checked; save(); return; }
        if (t.matches('[data-cal]')) { const r = c().rules[ruleOf(t.closest('.vw-ed'))], k = t.dataset.cal; r.cals = t.checked ? [...new Set([...(r.cals || []), k])] : (r.cals || []).filter(x => x !== k); const row = $(`[data-i="${open.i}"] .sub`, root); if (row) row.textContent = scope(r); save(); return; }
        if (t.matches('[data-newcat]')) { const r = c().rules[ruleOf(t.closest('.vw-ed'))], k = t.value.trim(); if (!k) return; if (!cs(r).some(x => x.toLowerCase() === k.toLowerCase())) r.categories = [...cs(r), k]; redraw(root, '.vw-ed [data-newcat]'); save(); }
      });
      root.addEventListener('input', e => {
        if (!e.target.matches('[data-f=name]')) return;
        const r = c().rules[open.i]; r.name = e.target.value; const row = $(`[data-i="${open.i}"] .vw-open > span`, root); if (row) row.firstChild.textContent = label(r) || tr('js-vw-new'); save(500);
      });
    });
  }
