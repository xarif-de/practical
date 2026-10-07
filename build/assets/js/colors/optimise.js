// js/colors/optimise.js — part of the colour IIFE (src/bundle.rs; shares its scope): "Farben optimieren" (families, palette plan, the dialog)
  /* ---- "Farben optimieren": one dialog per view whose colours meet — the calendar (calendars,
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
    w.innerHTML = `<div class="ask-box cfo-dlg"><div class="cfo-dh"><span class="cfo-spark">✦</span><span><b>${esc(tr('js-cfo-title'))}</b><small>${esc(tr('js-cfo-sub-' + view))}</small></span><button class="btn icon-btn" type="button" data-x aria-label="${esc(tr('js-color-close'))}">${ico('x')}</button></div><div class="cfo-body"></div><div class="ask-acts"><button class="btn icon-btn" type="button" data-x title="${esc(tr('js-ct-cancel'))}" aria-label="${esc(tr('js-ct-cancel'))}">${ico('x')}</button><button class="btn btn-primary icon-btn" type="button" data-apply>${ico('check')}<b class="n" hidden></b></button></div></div>`;
    const bodyEl = $('.cfo-body', w), applyBtn = $('[data-apply]', w);
    const sw = (hex, t) => `<i class="cfo-sw" style="--x:${hex}"${t ? ` title="${esc(t)}"` : ''}></i>`;
    const cname = (hex, raw) => (raw && raw[0] !== '#' ? raw : hex);
    const src1 = (g, F, li) => {
      const ex = st.excl.has(g.key), stay = !ex && g.editable && F.to && g.hex === F.to, n = g.items.length;
      const sub = [kindText(g.kind) + (n > 1 ? ' · ' + n + '×' : ''), cname(g.hex, g.raw), ex ? tr('js-cfo-excl-state') : !g.editable ? tr('js-cfo-ro') : stay ? tr('js-cfo-stay') : ''].filter(Boolean).join(' · ');
      return `<div class="cfo-src${ex ? ' excl' : ''}${g.moved ? ' moved' : ''}${stay ? ' stay' : ''}${g.editable ? '' : ' ro'}" data-n="g:${esc(g.key)}" data-g="${esc(g.key)}" data-l="${li}" style="--x:${g.hex}">`
        + `<span class="cfo-grip" title="${esc(tr('js-cfo-grip'))}">⠿</span>${sw(g.hex)}<span class="cfo-t"><b>${esc(g.name)}</b><small>${esc(sub)}</small>${g.moved ? `<span class="cfo-man">${ico('hand')}${esc(tr(g.moved === 'split' ? 'js-cfo-man-split' : 'js-cfo-man-moved'))}</span>` : ''}</span>`
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
        const mid = `<button type="button" class="cfo-mid${on ? '' : ' off'}${F.manual ? ' manual' : ''}" data-n="m:${esc(F.key)}" data-off="${esc(F.key)}" data-l="${li}" title="${esc(tr(on ? 'js-cfo-off-t' : 'js-cfo-on-t'))}"><span class="cfo-b ${F.type}">${esc(tr('js-cfo-t-' + F.type))}</span>${tag ? `<span class="cfo-man">${ico('hand')}${esc(tr(tag))}</span>` : ''}<small>${esc(F.why)}</small><span class="cfo-ck">${esc(on ? (changes.length ? tr('js-cfo-on', { n: changes.length }) : tr('js-cfo-none')) : tr('js-cfo-off'))}</span></button>`;
        const inh = F.cal && F.inc.some(g => g.kind !== 'calendar');
        const res = F.to ? `<button type="button" class="cfo-res${on ? '' : ' off'}${F.pinned ? ' pinned' : ''}${warn ? ' warn' : ''}" data-n="r:${esc(F.key)}" data-pick="${esc(F.key)}" data-l="${li}" title="${esc(tr('js-cfo-pick'))}">${sw(F.to)}<span class="cfo-t"><b>${esc(cname(F.to, (F.colors.find(c => c.hex === F.to) || {}).raw))}</b><small>${esc(tr('js-cfo-entries', { n: F.n }) + (inh ? ' · ' + tr('js-color-inherit-cal', { cal: F.lead.name }) : '') + (F.moved && !F.pinned ? ' · ' + tr('js-cfo-new') : ''))}</small>${F.pinned ? `<span class="cfo-man">${ico('hand')}${esc(tr('js-cfo-tag-pin'))}${warn ? ' · ⚠' : ''}</span>` : ''}</span>${F.pinned ? `<span class="cfo-mini" role="button" tabindex="0" data-unpin="${esc(F.key)}" title="${esc(tr('js-cfo-unpin'))}">↺</span>` : `<span class="cfo-edit">${esc(tr('js-cfo-edit'))}</span>`}</button>` : '';
        return `<div class="cfo-lane${F.manual ? ' manual' : ''}" data-l="${li}" data-f="${esc(F.key)}"><div class="cfo-lc">${left}</div><div class="cfo-lc">${mid}</div><div class="cfo-lc">${res}</div></div>`;
      }).join('');
      const L = P.L, lv = st.level;
      bodyEl.innerHTML = `<div class="cfo-level"><div class="cfo-lh"><span>${esc(tr('js-cfo-scope'))}</span><b>${esc(tr('js-cfo-l' + lv))}</b>${L.rec ? `<em>${esc(tr('js-cfo-rec'))}</em>` : ''}</div>
        <input type="range" min="0" max="${LEVELS.length - 1}" step="1" value="${lv}" data-level aria-label="${esc(tr('js-cfo-scope'))}">
        <div class="cfo-ticks">${LEVELS.map((x, i) => `<button type="button" data-lv="${i}" aria-pressed="${i === lv}">${esc(tr('js-cfo-l' + i))}</button>`).join('')}</div><p class="cfo-ltext">${esc(tr('js-cfo-l' + lv + '-text'))}</p></div>
        <div class="cfo-sum"><span class="cfo-m good">${esc(tr('js-cfo-m-colors', { a: P.before, b: P.after }))}</span><span class="cfo-m${P.sim1 < P.sim0 ? ' good' : ''}">${esc(tr('js-cfo-m-similar', { a: P.sim0, b: P.sim1 }))}</span><span class="cfo-m">${esc(tr('js-cfo-m-changes', { n: P.changes.length }))}</span>${n ? `<span class="cfo-manbar">${ico('hand')}${esc(tr('js-cfo-manual', { n }))} <button type="button" class="link" data-discard>${esc(tr('js-cfo-discard'))}</button></span>` : ''}</div>
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
      const x = practical.tell(await practical.send('/api/colors/batch', 'POST', { changes: list })), r = x.ok ? x.data : null;
      if (!r) { applyBtn.disabled = false; return; }
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
