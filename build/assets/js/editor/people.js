// js/editor/people.js — part of the editor IIFE (src/bundle.rs; shares its scope): the people card
  // ---- people (display only): ORGANIZER/ATTENDEE are shown as chips whose ring colour is the
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
    return ic ? `<span class="pav none" title="${esc(peType(p))}"><svg class="ico"><use href="${practical.sprite}#${ic}"/></svg></span>` : c ? `<span class="pav" style="background:var(--c${c.color_index})" title="${esc(PT.tKnown)}">${esc(ini)}</span>` : `<span class="pav none" title="${esc(PT.tUnknown)}">${esc(ini)}</span>`; };
  function peopleInit(organizer, attendees) { PEOPLE = { organizer: organizer || null, attendees: attendees || [] }; peopleRender(); }
  function peopleRender() { const all = peAll(); $('#ed-people-n').textContent = all.length || ''; $x('#ed-people-detail').innerHTML = ''; f.people_json.value = all.length ? JSON.stringify(all.map(p => [p.role, p.email])) : '';
    $x('#ed-people').innerHTML = all.map((p, i) => `<button class="pchip ${p.role === 'ORGANIZER' ? '' : peStatus(p)}" type="button" data-i="${i}" title="${esc(p.email)}">${peAv(p)}<span>${esc(peName(p))}</span>${p.role === 'ORGANIZER' ? `<small>${esc(PT.tOrgShort)}</small>` : p.role === 'OPT-PARTICIPANT' ? `<small>${esc(PT.tOptShort)}</small>` : ''}</button>`).join(''); }
  function peopleDetail(i) { const p = peAll()[i]; const n = (p.contacts || []).length; const status = p.role === 'ORGANIZER' ? '' : `<span class="pst ${peStatus(p)}">${esc(statusText(p) || D.tStatus.split('|')[0])}</span>`;
    $x('#ed-people-detail').innerHTML = `<div class="pdetail">${peAv(p)}<div><div class="nm">${esc(peName(p))}</div><div class="ml">${esc(p.email)}</div></div><span class="acts">${n ? `<button class="act" type="button" data-pact="book" ${n > 1 ? `data-n="${n}"` : ''} title="${esc(n > 1 ? PT.tBookN.replace('{n}', n) : PT.tBook)}"><svg class="ico"><use href="${practical.sprite}#i-user"/></svg></button>` : ''}<button class="act" type="button" data-pact="mail" title="${esc(PT.tMail)}"><svg class="ico"><use href="${practical.sprite}#i-mail"/></svg></button><button class="act" type="button" data-pact="copy" title="${esc(PT.tCopy)}"><svg class="ico"><use href="${practical.sprite}#i-copy"/></svg></button></span><div class="kv"><span>${esc(PT.tRole)}</span><span>${esc(peRole(p))}${peType(p) ? ' · ' + esc(peType(p)) : ''}</span>${status ? `<span>${esc(PT.tReply)}</span><span>${status}${peDeleg(p) ? ' · ' + esc(peDeleg(p)) : ''}</span>` : ''}${p.extra.some(x => x[0] === 'RSVP' && /true/i.test(x[1])) ? `<span>${esc(PT.tRsvp)}</span><span>${esc(PT.tYes)}</span>` : ''}</div></div>`;
    $$('.pchip', PC).forEach(b => b.setAttribute('aria-expanded', b.dataset.i === String(i))); }
  PC.addEventListener('click', e => { const chip = e.target.closest('.pchip'); const act = e.target.closest('[data-pact]');
    if (chip) { const i = +chip.dataset.i; const open = chip.getAttribute('aria-expanded') === 'true'; if (open) { $x('#ed-people-detail').innerHTML = ''; chip.setAttribute('aria-expanded', 'false'); } else peopleDetail(i); return; }
    if (!act) return; const p = peAll()[+$('.pchip[aria-expanded=true]', PC).dataset.i];
    if (act.dataset.pact === 'mail') location.href = 'mailto:' + p.email;
    else if (act.dataset.pact === 'copy') practical.copy(p.email).then(ok => { if (!ok) return; act.title = D.tCopied; act.style.color = 'var(--accent)'; setTimeout(() => act.style.color = '', 800); });
    else if (p.contacts.length === 1) location.href = '/contacts#' + p.contacts[0].key;
    else rrPop(act, [], null, key => { location.href = '/contacts#' + key; }, `<div class="q">${esc(PT.tPick)}</div>` + p.contacts.map(c => `<button type="button" class="pk" data-v="${esc(c.key)}"><span class="pav" style="background:var(--c${c.color_index})">${esc(c.initials)}</span><b>${esc(c.name)}</b><small>${esc(c.sub)}</small></button>`).join('')); });
  // links: URL / attachment / conference; an embedded (binary) attachment is shown read-only and never re-sent
