// js/colors/picker.js — the colour IIFE starts here (src/bundle.rs: its parts share one function scope): colour maths, the picker popover, the new-colour dialog; window.colorPicker
/* ===== colour picker: one picker for every colour in the app. The popover offers the colours
   already in use (GET /api/colors), each named by its sources, plus "like the calendar" or "standard"
   where the element has such a fallback; ＋ opens the dialog for a new colour: a fan deck or a text
   field with three sliders, the distance to the colours in use, and a light and dark preview. ===== */
  const { $, $$, esc, ico } = practical;
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
    const panel = dark => `<div class="cpk-pvp${dark ? ' dark' : ''}"><span class="sq">${ico('calendar')}</span><span class="evs"><span class="t"><b>10:00</b> ${esc(title)}</span><span class="ad">${esc(tr('js-color-pv-allday'))}</span></span><span class="hd">3</span></div>`;
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
    el.innerHTML = `<div class="cpk-fan">${D.map(([nm, ts]) => `<div class="cpk-card">${ts.map((t, j) => `<button type="button" data-v="${t}" style="background:${t}" title="${esc(nm)} ${j + 1}" role="radio" aria-checked="false"></button>`).join('')}<span class="nm">${esc(nm)}</span></div>`).join('')}</div>
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
    el.innerHTML = `<div class="cpk-in"><span class="cpk-chip"></span><input type="text" spellcheck="false" autocomplete="off" aria-label="${esc(tr('js-color-text'))}"><button class="btn icon-btn" type="button" title="${esc(tr('js-color-dice'))}" aria-label="${esc(tr('js-color-dice'))}">${ico('dice')}</button></div><div class="cpk-err" hidden></div>
      <span class="seg">${Object.keys(MODES).map(m => `<button type="button" data-m="${m}" aria-pressed="${m === mode}">${esc(tr('js-color-m-' + m))}</button>`).join('')}</span><div class="cpk-sl"></div>
      <div class="cpk-cap">${esc(tr('js-color-sugg'))}</div><div class="cpk-sugg">${SUGG.map(x => `<button type="button" data-v="${x}" style="background:${x}" title="${x}" role="radio" aria-checked="false"></button>`).join('')}</div>`;
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
      <span class="seg"><button type="button" data-t="fan">${ico('palette')}<span>${esc(tr('js-color-tab-fan'))}</span></button><button type="button" data-t="field">${ico('eyedropper')}<span>${esc(tr('js-color-tab-field'))}</span></button></span>
      <button class="btn icon-btn" type="button" data-x aria-label="${esc(tr('js-color-close'))}">${ico('x')}</button></div>
      <div class="cpk-body"></div><div class="cpk-near"></div><div class="ask-acts"><button class="btn icon-btn" type="button" data-x title="${esc(tr('js-ct-cancel'))}" aria-label="${esc(tr('js-ct-cancel'))}">${ico('x')}</button><button class="btn btn-primary icon-btn" type="button" data-ok title="${esc(tr('js-color-apply'))}" aria-label="${esc(tr('js-color-apply'))}">${ico('check')}</button></div></div>`;
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
    const circle = (v, hex, text, cls = '') => `<button type="button" class="cpk-sw${cls}" data-v="${v}" style="--c:${hex};--ink:${ink(hex)}" title="${esc(text)}" aria-label="${esc(text)}" role="radio" aria-checked="false"></button>`;
    pop.innerHTML = `<div class="cpk-h"><span class="sq">${ico('calendar')}</span><span><b>${esc(o.title)}</b><small>${esc(o.sub || '')}</small></span></div>
      <div class="cpk-sws" role="radiogroup" aria-label="${esc(o.title)}">${o.inherit ? circle('', o.inherit.hex, o.inherit.text, ' inh') : ''}${pal.map(p => circle(p.hex, p.hex, srcText(p.src))).join('')}${extra ? circle(extra, extra, tr('js-color-current')) : ''}<button type="button" class="cpk-sw cpk-add" data-new title="${esc(tr('js-color-add'))}" aria-label="${esc(tr('js-color-add'))}">${ico('plus')}</button></div>
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

  window.colorPicker = { open, toHex };
