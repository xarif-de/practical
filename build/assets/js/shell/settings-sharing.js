// js/shell/settings-sharing.js — part of the shell IIFE (src/bundle.rs; shares its scope): sharing on a collection's settings
// page (routes/settings/share_forms.rs), the people suggestions of its field and of a group's new-member field, and the
// visibility levels of a calendar
  /* sharing takes two steps: GET /api/sharee says whom the typed name or address means (a group with its note,
     and its members for a member; someone found by an address who keeps name and id out of the suggestions comes
     back as that address, without an id), the user confirms, then the form posts that id, else the address (the
     store resolves it) */
  $$('form.share-add').forEach(form => form.addEventListener('submit', async e => {
    if (form.dataset.confirmed) { delete form.dataset.confirmed; return; }
    e.preventDefault();
    const input = form.elements.sharee, t = form.dataset;
    const r = await practical.req('/api/sharee?q=' + encodeURIComponent(input.value.trim()), { headers: { Accept: 'application/json' } });
    if (!r.ok) { toast(r.msg || tr('js-req-server')); return; }
    const s = r.data;
    let text = t.tConfirm.replace('%name', s.id ? `${s.name} (${s.id})` : s.name);
    if (s.group) text += ' ' + t.tGroup + (s.members ? ' ' + t.tMembers.replace('%list', s.members.join(', ')) : '');
    if (!(await ask(text))) return;
    input.value = s.id || input.value.trim();
    form.dataset.confirmed = '1';
    form.requestSubmit();
  }));
  /* suggestions from two typed characters on (GET /api/sharee/suggest): only people who let others find them by
     what was typed (settings #privacy); a pick puts the id, or the address, into the field (practical.combo). A
     group's member field leaves out who is in it already (`data-members`). */
  const people = (input, list, skip = []) => practical.combo(input, list, {
    delay: 150,
    fetch: async v => {
      const q = v.trim();
      if ([...q].length < 2) return [];
      const r = await practical.req('/api/sharee/suggest?q=' + encodeURIComponent(q), { headers: { Accept: 'application/json' } });
      return r.ok && Array.isArray(r.data) ? r.data.filter(s => !s.id || !skip.includes(s.id)) : [];
    },
    label: s => [s.name, [s.id, s.email].filter(Boolean).join(' · ')],
    value: s => s.value,
  });
  $$('form.share-add').forEach(form => people(form.elements.sharee, $('.combo', form)));
  $$('form.member-add').forEach(form => people(form.elements.member, $('.combo', form), (form.dataset.members || '').split(',')));
  /* CONFIDENTIAL never shows more than PRIVATE: a lower PRIVATE takes CONFIDENTIAL down with it before the save */
  $$('form.vis-form').forEach(form => form.addEventListener('change', e => {
    if (e.target.name !== 'private') return;
    const p = +e.target.value, c = form.elements.confidential;
    [...c.options].forEach(o => { o.disabled = +o.value > p; });
    if (+c.value > p) c.value = String(p);
  }, true));
