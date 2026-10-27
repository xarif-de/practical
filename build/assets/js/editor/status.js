// js/editor/status.js — part of the editor IIFE (src/bundle.rs; shares its scope): the status card
  // ---- status card: the hidden selects stay the model (dirty tracking, reset, submit work as
  // before); the tiles are their face. Each select carries its label and one icon per option (`data-icons`),
  // so the tile's icon follows the value; a priority outside 1/5/9 (Outlook writes 2) maps by the RFC bands. ----
  const SC = $x('#ed-status-card');
  // a task without STATUS is shown as open (tasks.rs shown_status); the select keeps "", so a save writes none (not in rule mode: "" = the rule leaves it)
  const stVal = sel => TASK && !ruleMode && sel.name === 'status' && sel.value === '' ? 'NEEDS-ACTION' : sel.value;
  const stOpt = sel => [...sel.options].find(o => o.value === stVal(sel));
  const stIcon = sel => { const ic = sel.dataset.icons.split('|'); if (sel.name === 'priority' && sel.value !== '') { const n = +sel.value; return n === 0 ? ic[0] : n <= 4 ? ic[1] : n === 5 ? ic[2] : ic[3]; } return ic[Math.max(0, [...sel.options].indexOf(stOpt(sel)))] || ic[0]; };
  const stText = sel => stOpt(sel) ? stOpt(sel).textContent : sel.value;
  function stRender() {
    visRender(); // the CLASS select left the card for its own row (visibility.js); it renders with the others
    $$('.stiles', form).forEach(box => { const sels = $$('select.dat', box.closest('.card')); box.innerHTML = sels.map(sel => `<button class="stile${stVal(sel) === '' ? ' off' : ''}${RLOCK.has(sel.name) ? ' locked' : ''}" type="button" data-sel="${sel.name}" title="${esc(sel.dataset.label)}"><svg class="ico" aria-hidden="true"><use href="${practical.sprite}#${stIcon(sel)}"/></svg><small>${esc(sel.dataset.label)}</small><b>${esc(stText(sel))}</b></button>`).join('') });
    if (TASK) return;
    const t = SC.dataset, parts = [];
    if (f.status.value) parts.push(stText(f.status));
    if (f.transp.value) parts.push(f.transp.value === 'OPAQUE' ? t.tViewOpaque : t.tViewTransparent);
    if (f.priority.value) parts.push(t.tViewPrio.replace('{n}', stText(f.priority)));
    $('#ed-status-text').textContent = parts.join(' · ');
  }
  form.addEventListener('click', e => { const b = e.target.closest('.stile'); if (!b || !form.contains(b)) return; if (!b || b.classList.contains('locked')) return; const sel = f[b.dataset.sel];
    rrPop(b, [...sel.options].map(o => ({ v: o.value, label: o.textContent })), stVal(sel), v => { if (v !== stVal(sel)) sel.value = v; stRender(); refresh(); }, `<div class="q">${esc(sel.dataset.label)}</div>`); b.setAttribute('aria-expanded', 'true'); });
