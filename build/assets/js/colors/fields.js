// js/colors/fields.js — part of the colour IIFE (src/bundle.rs; shares its scope): colour fields in forms, the settings page "Farben"
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
    const q = '?scope=' + scope, got = await practical.req('/api/category-rules' + q); if (!got.ok) return got;
    const rule = got.data[name];
    if (!rule || !rule.when || !rule.when.color) return { ok: false, kind: 'notfound', status: 404, msg: tr('js-req-notfound') };
    rule.when.color.value = color;
    return practical.send('/api/category-rules/' + encodeURIComponent(name) + q, 'PUT', rule);
  };
  // a category's new colour: existing events/tasks with the category that still carry the old one may follow
  const recolor = async (scope, name, from, to) => {
    const post = apply => practical.send('/api/colors/category', 'POST', { scope, name, from, to, apply }).then(r => r.ok ? r.data : null);
    const c = await post(false); if (!c || !c.count) return;
    close(); // the question needs the page, not the popover over it
    if (!await askYesNo(tr('js-color-recolor-' + scope, { n: c.count, cat: name }), tr('js-color-recolor-yes'), tr('js-color-recolor-no'), true)) return;
    const d = await post(true); if (d) toast(tr('js-color-recolored', { n: d.count })); else toast(tr('js-set-err'), true);
  };
  const put = (kind, key, color) => practical.send('/api/colors/' + kind + '/' + key.split('/').map(encodeURIComponent).join('/'), 'PUT', { color });
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
      const sw = `<span class="swatch" style="background:${s.hex}"><svg class="ico" aria-hidden="true"><use href="${practical.sprite}#${ICON[s.kind]}"/></svg></span>`;
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
        return inner && view ? `<div class="cfo-block"><h2>${esc(tr('js-cfo-block-' + view))}</h2><button type="button" class="cfo-opt" data-opt="${view}">✦ ${esc(tr('js-cfo-btn'))}</button></div>${inner}` : inner; }).join('')
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
    const r = practical.tell(await (kind.startsWith('category') ? putRule(kind === 'category' ? 'event' : 'task', key, v) : put(kind, key, v)));
    if (!r.ok) return false;
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
