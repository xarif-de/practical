// site/gallery.js — the start page's screenshot gallery (markup: site/build.mjs galleryHtml). Without this script
// every figure stays listed (no-JS fallback); with it one screenshot shows at a time: previous/next buttons, arrow
// keys / Home / End while the stage is focused, touch swipe, group chips, a thumbnail strip (roving tabindex, arrow
// keys), "n / N", the URL hash (#shot-<slug>) as a deep link. Hidden slides are display:none (from the first layout:
// html.js, set in <head>), so their lazy images load only when shown; once the user switches, the two neighbours of
// the shown one load eagerly (ready when you get there).
(function () {
  'use strict';
  var g = document.querySelector('.gallery');
  if (!g) return;
  var slides = [].slice.call(g.querySelectorAll('.g-slide'));
  var thumbs = [].slice.call(g.querySelectorAll('.g-thumb'));
  var chips = [].slice.call(g.querySelectorAll('.g-chip'));
  var stage = g.querySelector('.g-stage');
  var posN = g.querySelector('.g-pos-n');
  var live = g.querySelector('[aria-live]');
  var n = slides.length, cur = -1;
  if (!n) return;

  g.classList.add('is-js');
  [].forEach.call(g.querySelectorAll('[hidden]'), function (el) { el.hidden = false; });

  function show(i, opts) {
    i = (i + n) % n;
    opts = opts || {};
    if (i === cur && !opts.force) return;
    if (cur >= 0) { slides[cur].classList.remove('is-active'); thumbs[cur].removeAttribute('aria-current'); thumbs[cur].tabIndex = -1; }
    cur = i;
    var s = slides[i];
    s.classList.add('is-active');
    // after the first switch by the user: the neighbours load eagerly (not on page load: two pictures nobody may want)
    if (!opts.quiet) [i - 1, i + 1].forEach(function (k) { var img = slides[(k + n) % n].querySelector('img'); if (img) img.loading = 'eager'; });
    var t = thumbs[i];
    t.setAttribute('aria-current', 'true');
    t.tabIndex = 0;
    // keep the current thumbnail in view inside the strip only (never scroll the page)
    var strip = t.parentNode, sr = strip.getBoundingClientRect(), tr = t.getBoundingClientRect();
    if (tr.left < sr.left || tr.right > sr.right) strip.scrollLeft += tr.left - sr.left - (sr.width - tr.width) / 2;
    var group = s.getAttribute('data-group');
    chips.forEach(function (c) {
      var first = slides[+c.getAttribute('data-to')];
      if (first && first.getAttribute('data-group') === group) c.setAttribute('aria-current', 'true'); else c.removeAttribute('aria-current');
    });
    posN.textContent = String(i + 1);
    if (!opts.quiet) live.textContent = s.querySelector('strong').textContent + ', ' + (i + 1) + ' of ' + n;
    if (!opts.keepHash && history.replaceState) history.replaceState(null, '', '#' + s.id);
  }

  g.querySelector('.g-prev').addEventListener('click', function () { show(cur - 1); });
  g.querySelector('.g-next').addEventListener('click', function () { show(cur + 1); });
  chips.forEach(function (c) { c.addEventListener('click', function () { show(+c.getAttribute('data-to')); }); });
  thumbs.forEach(function (t, i) {
    t.addEventListener('click', function () { show(i); });
    t.addEventListener('keydown', function (e) {
      var k = { ArrowLeft: cur - 1, ArrowRight: cur + 1, Home: 0, End: n - 1 }[e.key];
      if (k === undefined) return;
      e.preventDefault();
      show(k);
      thumbs[cur].focus();
    });
  });
  stage.addEventListener('keydown', function (e) {
    if (e.target !== stage && e.target.closest('.g-nav')) { if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return; }
    var k = { ArrowLeft: cur - 1, ArrowRight: cur + 1, Home: 0, End: n - 1 }[e.key];
    if (k === undefined || e.altKey || e.ctrlKey || e.metaKey) return;
    e.preventDefault();
    show(k);
  });

  // touch swipe: a mostly horizontal move of at least 40 px (vertical moves keep scrolling the page: touch-action pan-y)
  var sx = 0, sy = 0, tracking = false, swiped = false;
  stage.addEventListener('pointerdown', function (e) {
    if (e.pointerType === 'mouse') return;
    tracking = true; swiped = false; sx = e.clientX; sy = e.clientY;
  });
  stage.addEventListener('pointerup', function (e) {
    if (!tracking) return;
    tracking = false;
    var dx = e.clientX - sx, dy = e.clientY - sy;
    if (Math.abs(dx) >= 40 && Math.abs(dx) > 1.5 * Math.abs(dy)) { swiped = true; show(dx < 0 ? cur + 1 : cur - 1); }
  });
  stage.addEventListener('pointercancel', function () { tracking = false; });
  // a swipe that ends on the picture's link must not open it
  stage.addEventListener('click', function (e) { if (swiped) { swiped = false; e.preventDefault(); } }, true);

  function fromHash() {
    var id = decodeURIComponent(location.hash.slice(1));
    for (var i = 0; i < n; i++) if (slides[i].id === id) return i;
    return -1;
  }
  var start = fromHash();
  show(start < 0 ? 0 : start, { quiet: true, keepHash: true });
  if (start >= 0) g.scrollIntoView();
  window.addEventListener('hashchange', function () { var i = fromHash(); if (i >= 0) show(i, { keepHash: true }); });
})();
