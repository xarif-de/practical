// js/editor/form.js — part of the editor IIFE (src/bundle.rs; shares its scope): the card form, view/edit mode, dirty tracking
  // ---- editor: a card form like the contact editor — view mode behind the lock, cards shown only
  // when they hold data, "add card" menu in edit mode, icon buttons, dirty tracking ----
  const ed = $('#ed'), form = $('#edform'), f = form.elements, D = form.dataset;
  let current = null, openDate = null, baseline = '', opener = null; // opener: what had the focus before the dialog opened
  // the user worked in the dialog since it was filled: the leave-page guard and the work kept over a new login ask for it
  // (a rule editor opened by a link can differ from its baseline untouched; nothing to lose there)
  let touched = false;
  ['pointerdown', 'keydown', 'input', 'change', 'drop'].forEach(t => form.addEventListener(t, e => { if (e.isTrusted) touched = true; }, true));
  function setAllDay(on) { if (!f.all_day) return; f.all_day.checked = on; $('#ed-allday').classList.toggle('on', on); f.from.disabled = on; f.to.disabled = on; $('#ed-times').hidden = on; }
  $x('#ed-allday').addEventListener('click', e => { e.preventDefault(); setAllDay(!f.all_day.checked); refresh(); });
  function syncDot() { const o = f.calendar.selectedOptions[0]; $('#ed-dot').style.background = f.color_cal.checked ? (o ? o.dataset.color : '') : f.color.value; $('#ed-sub').textContent = o ? o.textContent : ''; if (f.color_cal.checked && o) f.color.value = toHex(o.dataset.color); paintColor(); }
  const tl = $x('#ed-tl'); function setTl(on) { ed.classList.toggle('no-tl', !on); tl.setAttribute('aria-pressed', on ? 'true' : 'false'); localStorage.edtl = on ? '1' : '0'; }
  tl.onclick = () => setTl(ed.classList.contains('no-tl')); setTl(localStorage.edtl !== '0');
  // the dialog closes only through its buttons; unsaved edits ask first (a click on the backdrop does nothing)
  function close() { ed.hidden = true; tplClose(); $('#ed-confirm').hidden = true; if (ruleMode) ruleLeave(); soloEnd(); const back = opener; opener = null; const a = document.activeElement; if (back && back.isConnected && (!a || a === document.body || ed.contains(a))) back.focus(); const h = HOST(); if (h.closed) h.closed(); }
  const dirty = () => { flush(); return form.classList.contains('mode-edit') && sig() !== baseline; };
  // while a write is pending the dialog stays (its answer closes it, or shows why not): ×, Escape and "discard" wait
  function requestClose() { if (W.busy) return; if (dirty()) { $('#ed-confirm').hidden = false; $('#ed-back').focus(); } else close(); }
  /* writes (save, delete, a rule's save and delete): one at a time, Save shows the progress, a 409 asks whether the
     own version overwrites the other one (the server's ETag of now) or the other one is loaded (practical.writer) */
  const W = practical.writer(form, { buttons: () => [$('#ed-save'), $('#ed-delete'), $('#ed-reset')], error: $('#ed-error'), idle: () => updateButtons(),
    conflict: { overwrite: async () => { const r = await practical.req(API + '/' + f.key.value); if (!r.ok) return r; f.etag.value = r.data.etag; return put(); },
      reload: () => openEditor(f.key.value, openDate) } });
  $('#ed-close').onclick = requestClose; $('#ed-back').onclick = () => { $('#ed-confirm').hidden = true; }; $('#ed-discard').onclick = () => { if (!W.busy) close(); };
  addEventListener('keydown', e => { if (e.key === 'Escape') { if (!ed.hidden) requestClose(); pk.hidden = true; } });
  const fmtD = s => dfmt(parse(s.slice(0, 10)), { weekday: 'short', day: '2-digit', month: '2-digit', year: 'numeric' });
  const fmtDT = s => fmtD(s) + ' ' + s.slice(11, 16);
