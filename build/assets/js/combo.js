// js/combo.js — app.js part (classic script, after req.js): practical.combo, the one suggestion list under a text
// field (sharing, a group's new member, the remote server address)
/* combo(input, list, o) wires a listbox of our own (not a <datalist>: its dropdown triangle came back on hover even
   hidden) to `input`: o.fetch(q) → the items (an array or a promise of one; only the newest answer is drawn),
   o.label(item) → [text, sub] (sub optional; both set as text, never as markup), o.value(item) → what goes into the
   field, o.take(item) after it went there, o.delay (ms after a keystroke, default 0), o.focus (open on focus too).
   ↑/↓ walk it (↓ on a closed list opens it), Enter takes the marked one (else closes it and goes on: a form submits),
   Escape closes it, a click takes one without the field losing the focus.
   The list is `position: fixed` (chrome.css `.combo`) and placed here under the FIELD (above it without room below),
   inside the window: an `overflow: hidden` ancestor (settings.css `.set .group`) would clip an absolute one. A fixed
   box does not travel with the page, so a scroll anywhere but in the list, or a resize, closes every open one (one
   shared pair of listeners). Returns { close, load }. */
(function () {
  const P = window.practical, OPEN = new Set();
  const closeAll = e => OPEN.forEach(c => { if (!(e && e.target instanceof Node && c.list.contains(e.target))) c.close(); });
  addEventListener('scroll', closeAll, { passive: true, capture: true });
  addEventListener('resize', () => closeAll());
  let n = 0;
  P.combo = (input, list, o) => {
    if (!list.id) list.id = `combo-${++n}`;
    input.setAttribute('aria-controls', list.id);
    let items = [], rows = [], at = -1, seq = 0, timer = 0;
    const self = { list };
    const mark = i => { at = i; rows.forEach((r, k) => r.setAttribute('aria-selected', String(k === i))); if (rows[i]) { const r = rows[i]; input.setAttribute('aria-activedescendant', r.id); if (r.offsetTop < list.scrollTop) list.scrollTop = r.offsetTop; else if (r.offsetTop + r.offsetHeight > list.scrollTop + list.clientHeight) list.scrollTop = r.offsetTop + r.offsetHeight - list.clientHeight; } /* not scrollIntoView: it may scroll the page, which closes the list */ else input.removeAttribute('aria-activedescendant'); };
    const close = self.close = () => { seq++; clearTimeout(timer); list.hidden = true; list.replaceChildren(); items = []; rows = []; at = -1; input.setAttribute('aria-expanded', 'false'); input.removeAttribute('aria-activedescendant'); OPEN.delete(self); };
    const place = () => {
      const f = input.getBoundingClientRect();
      list.style.width = `${Math.max(f.width, o.minWidth || 190)}px`;
      list.style.left = `${Math.max(8, Math.min(f.left, innerWidth - list.offsetWidth - 8))}px`;
      const below = innerHeight - f.bottom;
      list.style.top = `${below < list.offsetHeight + 12 && f.top > list.offsetHeight + 12 ? f.top - list.offsetHeight - 4 : f.bottom + 4}px`;
    };
    const draw = got => {
      items = Array.isArray(got) ? got : [];
      if (!items.length) { close(); return; }
      rows = items.map((it, i) => {
        const [text, sub] = o.label(it), r = document.createElement('div');
        r.className = 'r'; r.id = `${list.id}-o${i}`; r.setAttribute('role', 'option'); r.setAttribute('aria-selected', 'false');
        r.append(String(text));
        if (sub) { const s = document.createElement('small'); s.textContent = sub; r.append(s); }
        return r;
      });
      list.replaceChildren(...rows); list.hidden = false; input.setAttribute('aria-expanded', 'true'); OPEN.add(self);
      place(); mark(-1);
    };
    const load = self.load = async () => { const s = ++seq, got = await o.fetch(input.value); if (s === seq) draw(got); };
    const take = i => { const it = items[i]; input.value = o.value(it); close(); input.focus(); if (o.take) o.take(it); };
    input.addEventListener('input', () => { clearTimeout(timer); if (o.delay) timer = setTimeout(load, o.delay); else load(); });
    if (o.focus) input.addEventListener('focus', load);
    input.addEventListener('keydown', e => {
      const open = !list.hidden && rows.length;
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        e.preventDefault();
        if (!open) { load(); return; }
        const k = rows.length;
        mark(e.key === 'ArrowDown' ? (at + 1) % k : (at <= 0 ? k : at) - 1);
      } else if (e.key === 'Enter' && open) {
        if (at >= 0) { e.preventDefault(); take(at); } else close();
      } else if (e.key === 'Escape' && open) { e.preventDefault(); e.stopPropagation(); close(); }
    });
    input.addEventListener('blur', close);
    list.addEventListener('mousedown', e => e.preventDefault()); // the focus stays in the field
    list.addEventListener('click', e => { const r = e.target.closest('.r'); if (r) take(rows.indexOf(r)); });
    list.addEventListener('mousemove', e => { const r = e.target.closest('.r'); if (r && rows.indexOf(r) !== at) mark(rows.indexOf(r)); });
    return self;
  };
})();
