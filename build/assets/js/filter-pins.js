// js/filter-pins.js — quick switches: a filter row's star pins the row as a button left of the filter (macros.html `pin_btn`)
/* A quick switch is a ribbon in the row's colour, drawn for every row and hidden while not pinned. A click on it is one
   on the row's checkbox, so each page's own filter code hides and remembers as before; it follows the checkbox back on
   every change and after the "all / none / mine" links, which set the boxes without one. The star stores the pin per
   user (POST /settings/filter-pin {key, on}: one key, so the calendar page and the contacts page — each knows only its
   own rows — keep each other's pins). Saves run one after another; when the last one is answered, stars and switches
   show the server's list (a failed save falls back to the last list it answered). */
(function () {
  const { $, $$, send, tell } = window.practical, bar = $('.topbar .qsbar');
  if (!bar) return;
  const starOf = k => $$('.filter .pinstar').find(s => s.dataset.pin === k);
  const sync = () => $$('.qs', bar).forEach(b => { const s = starOf(b.dataset.pin); if (s) b.setAttribute('aria-pressed', $('input.eyed', s.closest('.frow')).checked); });
  const show = pins => $$('.qs', bar).forEach(b => { const on = pins.includes(b.dataset.pin); b.hidden = !on; const s = starOf(b.dataset.pin); if (s) s.setAttribute('aria-pressed', on); });
  let known = $$('.qs:not([hidden])', bar).map(b => b.dataset.pin), queue = Promise.resolve(), pending = 0;
  document.addEventListener('change', e => { if (e.target.matches?.('.filter input.eyed')) sync(); });
  document.addEventListener('click', e => {
    const b = e.target.closest('.qs[data-pin]');
    if (b) { const s = starOf(b.dataset.pin); if (s) $('input.eyed', s.closest('.frow')).click(); return; }
    if (e.target.closest('.filter [data-filter]')) { sync(); return; } // its own handler ran first: the boxes are set
    const s = e.target.closest('.filter .pinstar');
    if (!s) return;
    const key = s.dataset.pin, on = s.getAttribute('aria-pressed') !== 'true';
    show($$('.filter .pinstar').filter(x => x === s ? on : x.getAttribute('aria-pressed') === 'true').map(x => x.dataset.pin));
    pending++;
    queue = queue.then(async () => {
      const r = await send('/settings/filter-pin', 'POST', { key, on });
      if (r.ok) known = r.data; else tell(r);
      if (!--pending || !r.ok) show(known);
    });
  });
  sync();
})();
