// js/filter-pins.js — quick switches: a filter row's star pins the row as a button left of the filter (macros.html `pin_btn`)
/* A quick switch is the row's colour with the row's eye. A click on it is one on the row's checkbox, so each page's own
   filter code hides and remembers as before; it follows the checkbox back on every change and after the "all / none /
   mine" links, which set the boxes without one. The star stores the pin per user (POST /settings/filter-pin {key, on}:
   one key, so the calendar page and the contacts page — each knows only its own rows — keep each other's pins). */
(function () {
  const { $, $$ } = window.practical, bar = $('.topbar .qsbar');
  if (!bar) return;
  const keyOf = i => i.dataset.cal ? 'cal:' + i.dataset.cal : i.dataset.ab ? 'ab:' + i.dataset.ab : i.dataset.hol ? 'hol:' + i.dataset.hol : '';
  const boxOf = k => $$('.filter input.eyed').find(i => keyOf(i) === k);
  const sync = () => $$('.qs', bar).forEach(b => { const i = boxOf(b.dataset.pin); if (i) b.setAttribute('aria-pressed', i.checked); });
  // the switch as pin_btn draws it: the row's colour, name (its label's own text, not the owner) and eyes
  const switchOf = star => {
    const row = star.closest('.frow'), label = $('label', row), b = document.createElement('button');
    const name = [...label.childNodes].filter(n => n.nodeType === 3).map(n => n.textContent).join('').trim();
    Object.assign(b, { type: 'button', className: 'qs', title: name });
    b.dataset.pin = star.dataset.pin; b.setAttribute('aria-label', name);
    b.style.setProperty('--c', $('.sw', row).style.getPropertyValue('--c'));
    b.append(...$$('.eye', label).map(e => e.cloneNode(true)));
    return b;
  };
  const redraw = () => { bar.replaceChildren(...$$('.filter .pinstar[aria-pressed=true]').map(switchOf)); sync(); };
  document.addEventListener('change', e => { if (e.target.matches && e.target.matches('.filter input.eyed')) sync(); });
  document.addEventListener('click', async e => {
    const b = e.target.closest('.qs[data-pin]');
    if (b) { const i = boxOf(b.dataset.pin); if (i) i.click(); return; }
    if (e.target.closest('.filter [data-filter]')) { sync(); return; } // its own handler ran first: the boxes are set
    const s = e.target.closest('.filter .pinstar');
    if (!s) return;
    const on = s.getAttribute('aria-pressed') !== 'true';
    s.setAttribute('aria-pressed', on); redraw();
    const r = await window.practical.send('/settings/filter-pin', 'POST', { key: s.dataset.pin, on });
    if (!r.ok) { s.setAttribute('aria-pressed', !on); redraw(); window.practical.tell(r); }
  });
  sync();
})();
