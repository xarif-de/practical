// js/shell/settings-sharing.js — part of the shell IIFE (src/bundle.rs; shares its scope): sharing on a collection's settings
// page (routes/settings/share_forms.rs), its suggestions and the visibility levels of a calendar
  /* sharing takes two steps: GET /api/sharee says whom the typed name or address means (a group with its note,
     and its members for a member), the user confirms, then the form posts that principal's id */
  $$('form.share-add').forEach(form => form.addEventListener('submit', async e => {
    if (form.dataset.confirmed) { delete form.dataset.confirmed; return; }
    e.preventDefault();
    const input = form.elements.sharee, t = form.dataset;
    const r = await practical.req('/api/sharee?q=' + encodeURIComponent(input.value.trim()), { headers: { Accept: 'application/json' } });
    if (!r.ok) { toast(r.msg || tr('js-req-server')); return; }
    const s = r.data;
    let text = t.tConfirm.replace('%name', `${s.name} (${s.id})`);
    if (s.group) text += ' ' + t.tGroup + (s.members ? ' ' + t.tMembers.replace('%list', s.members.join(', ')) : '');
    if (!(await ask(text))) return;
    input.value = s.id;
    form.dataset.confirmed = '1';
    form.requestSubmit();
  }));
  /* suggestions from two typed characters on (GET /api/sharee/suggest): only people who let others find them by
     what was typed (settings #privacy). ↑/↓ move, Enter or a click puts one into the field, Escape closes */
  $$('form.share-add').forEach(form => {
    const input = form.elements.sharee, box = $('.share-sugg', form), esc = practical.esc;
    let seq = 0, timer = 0, items = [], at = -1;
    const close = () => { seq++; box.hidden = true; box.innerHTML = ''; items = []; at = -1; input.setAttribute('aria-expanded', 'false'); input.removeAttribute('aria-activedescendant'); };
    const mark = i => { at = i; items.forEach((b, k) => b.setAttribute('aria-selected', String(k === i))); if (items[i]) input.setAttribute('aria-activedescendant', items[i].id); else input.removeAttribute('aria-activedescendant'); };
    const take = b => { input.value = b.dataset.v; close(); input.focus(); };
    const load = async () => {
      const q = input.value.trim(), n = ++seq;
      if ([...q].length < 2) { close(); return; }
      const r = await practical.req('/api/sharee/suggest?q=' + encodeURIComponent(q), { headers: { Accept: 'application/json' } });
      if (n !== seq) return; // a newer keystroke asked meanwhile
      if (!r.ok || !Array.isArray(r.data) || !r.data.length) { close(); return; }
      box.innerHTML = r.data.map((s, i) => `<button type="button" role="option" tabindex="-1" id="${box.id}-${i}" data-v="${esc(s.value)}">${esc(s.name)}<small>${esc([s.id, s.email].filter(Boolean).join(' · '))}</small></button>`).join('');
      items = $$('button', box); box.hidden = false; input.setAttribute('aria-expanded', 'true'); mark(-1);
    };
    input.addEventListener('input', () => { clearTimeout(timer); timer = setTimeout(load, 150); });
    input.addEventListener('keydown', e => {
      if (box.hidden) return;
      const d = e.key === 'ArrowDown' ? 1 : e.key === 'ArrowUp' ? -1 : 0;
      if (d) { e.preventDefault(); mark(at < 0 ? (d > 0 ? 0 : items.length - 1) : (at + d + items.length) % items.length); }
      else if (e.key === 'Enter' && at >= 0) { e.preventDefault(); take(items[at]); }
      else if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); close(); }
    });
    box.addEventListener('mousedown', e => e.preventDefault()); // the focus stays in the field
    box.addEventListener('click', e => { const b = e.target.closest('button[data-v]'); if (b) take(b); });
    input.addEventListener('blur', () => { clearTimeout(timer); close(); });
  });
  /* CONFIDENTIAL never shows more than PRIVATE: a lower PRIVATE takes CONFIDENTIAL down with it before the save */
  $$('form.vis-form').forEach(form => form.addEventListener('change', e => {
    if (e.target.name !== 'private') return;
    const p = +e.target.value, c = form.elements.confidential;
    [...c.options].forEach(o => { o.disabled = +o.value > p; });
    if (+c.value > p) c.value = String(p);
  }, true));
