/* rustical-ui — shared behaviour (vanilla ES2020, no framework). */
(function () {
  const $ = (s, r = document) => r.querySelector(s);
  const $$ = (s, r = document) => [...r.querySelectorAll(s)];
  const csrf = () => ($('meta[name=csrf]') || {}).content || '';

  // light/dark toggle (choice persisted in the browser; server pref = initial default)
  const h = document.documentElement;
  const sb = $('#scheme');
  if (sb) sb.addEventListener('click', () => { h.dataset.scheme = h.dataset.scheme === 'dark' ? 'light' : 'dark'; localStorage.scheme = h.dataset.scheme; });

  // confirmations on destructive forms
  $$('[data-confirm]').forEach(b => b.addEventListener('click', e => { if (!confirm(b.dataset.confirm)) e.preventDefault(); }));

  // copy-to-clipboard
  $$('[data-copy]').forEach(b => b.addEventListener('click', async () => {
    const el = $(b.dataset.copy); if (!el) return;
    try { await navigator.clipboard.writeText(el.textContent.trim()); b.textContent = '✓'; } catch (_) { const r = document.createRange(); r.selectNodeContents(el); getSelection().removeAllRanges(); getSelection().addRange(r); }
  }));

  // settings tabs (hash-driven; without JS every panel is visible)
  const settings = $('.settings');
  if (settings) {
    const tabs = $$('.tabs a');
    const show = id => {
      settings.classList.add('tabbed');
      $$('.panel', settings).forEach(p => p.classList.toggle('on', p.id === id));
      tabs.forEach(a => { if (a.getAttribute('href') === '#' + id) a.setAttribute('aria-current', 'page'); else a.removeAttribute('aria-current'); });
    };
    const fromHash = () => { const id = location.hash.slice(1); show(($('#' + id, settings)) ? id : 'profile'); };
    tabs.forEach(a => a.addEventListener('click', e => { e.preventDefault(); history.replaceState(null, '', a.getAttribute('href')); fromHash(); }));
    addEventListener('hashchange', fromHash); fromHash();
    // a flash after a failed confirmation should keep the details open
    $$('.flash.error').forEach(() => $$('details.danger-zone', settings).forEach(d => d.open = true));
  }

  // user menu: click toggles (hover works via CSS, touch needs the click)
  $$('.filter > button').forEach(b => b.addEventListener('click', e => { e.stopPropagation(); b.parentElement.classList.toggle('open'); }));
  document.addEventListener('click', e => { if (!e.target.closest('.filter')) $$('.filter.open').forEach(f => f.classList.remove('open')); });

  // "/" focuses the search
  const q = $('#q');
  if (q) addEventListener('keydown', e => { if (e.key === '/' && document.activeElement !== q && !/input|textarea|select/i.test(document.activeElement.tagName)) { e.preventDefault(); q.focus(); } });

  // live updates: server-sent changes → custom event for the views
  if (document.body.dataset.user && 'EventSource' in window) {
    let es; const connect = () => {
      es = new EventSource('/api/changes');
      es.addEventListener('change', ev => { try { document.dispatchEvent(new CustomEvent('rustical:change', { detail: JSON.parse(ev.data) })); } catch (_) {} });
      es.onerror = () => { es.close(); setTimeout(connect, 5000 + Math.random() * 5000); };
    };
    connect();
  }

  window.rusticalUi = { csrf, $, $$ };
})();
