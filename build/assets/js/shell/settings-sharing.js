// js/shell/settings-sharing.js — part of the shell IIFE (src/bundle.rs; shares its scope): sharing on a collection's settings
// page (routes/settings/share_forms.rs) and the visibility levels of #sharing
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
  /* CONFIDENTIAL never shows more than PRIVATE: a lower PRIVATE takes CONFIDENTIAL down with it before the save */
  $$('form.vis-form').forEach(form => form.addEventListener('change', e => {
    if (e.target.name !== 'private') return;
    const p = +e.target.value, c = form.elements.confidential;
    [...c.options].forEach(o => { o.disabled = +o.value > p; });
    if (+c.value > p) c.value = String(p);
  }, true));
