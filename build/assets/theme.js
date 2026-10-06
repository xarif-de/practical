/* The colour scheme before the first paint (layout.html <head>, parser-blocking; was an inline script,
   which the Content-Security-Policy forbids). "auto" follows the system, also while the page is open;
   the old per-browser choice (localStorage) is gone. */
(function () {
  const h = document.documentElement, m = matchMedia('(prefers-color-scheme: dark)');
  const f = () => { if (h.dataset.prefScheme === 'auto') h.dataset.scheme = m.matches ? 'dark' : 'light'; };
  f();
  m.addEventListener('change', f);
  try { localStorage.removeItem('scheme'); } catch (e) {}
})();
