  /* solo: the dialog shows one card, already in edit mode, and saves the whole entry as usual. A
     category rule can fill other cards (location, reminders …): every card whose content changed
     since the dialog opened is shown too, so nothing is written unseen. */
  const cardSig = c => $$('input:not([type=hidden]), select, textarea', c).map(e => e.type === 'checkbox' ? String(e.checked) : e.value).join('|') + '|' + $$('.chip, .pchip, .rows .row', c).map(x => x.textContent).join(',');
  function soloCards() {
    if (!solo) return;
    $$('.card', form).forEach(c => {
      const own = c.dataset.card === solo, moved = soloSigs[c.dataset.card] !== undefined && cardSig(c) !== soloSigs[c.dataset.card];
      if (own || moved) c.hidden = false;
      c.classList.toggle('solo-on', own || moved);
    });
  }
  function soloStart(card) {
    solo = card; mode('edit');
    const c = $(`.card[data-card="${card}"]`, form); if (c) { c.dataset.added = '1'; c.hidden = false; }
    soloSigs = {}; $$('.card', form).forEach(x => { soloSigs[x.dataset.card] = cardSig(x); });
    f.summary.readOnly = true; ed.classList.add('solo'); soloCards();
    const first = c && ($('input:not([type=hidden]):not([readonly]), textarea', c) || $('select', c)); if (first) setTimeout(() => first.focus(), 0);
  }
  function soloEnd() { if (!solo) return; solo = null; f.summary.readOnly = false; ed.classList.remove('solo'); $$('.card.solo-on', form).forEach(c => c.classList.remove('solo-on')); }
