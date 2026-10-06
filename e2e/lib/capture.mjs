// One captured UI state = <OUT>/<project>/<name>.png + .dom.html.gz (normalised outerHTML) +
// .style.json.gz (computed-style digest of every rendered element). compare.mjs diffs two OUT dirs.
import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import { OUT } from './env.mjs';
import { settle } from './test.mjs';
import { recordThemeRules } from './theme-cover.mjs';
import { clock, mark } from './timing.mjs';

// the visual properties digested per element (and per ::before/::after with content)
export const STYLE_PROPS = [
  'display', 'position', 'top', 'right', 'bottom', 'left', 'z-index', 'float', 'box-sizing',
  'width', 'height', 'margin-top', 'margin-right', 'margin-bottom', 'margin-left',
  'padding-top', 'padding-right', 'padding-bottom', 'padding-left',
  'border-top', 'border-right', 'border-bottom', 'border-left', 'border-radius', 'outline',
  'color', 'background-color', 'background-image', 'box-shadow', 'opacity', 'visibility', 'transform', 'filter',
  'font-family', 'font-size', 'font-weight', 'font-style', 'line-height', 'letter-spacing', 'text-align', 'text-decoration-line',
  'text-transform', 'white-space', 'overflow-x', 'overflow-y', 'text-overflow',
  'flex-direction', 'flex-wrap', 'justify-content', 'align-items', 'gap', 'grid-template-columns', 'grid-template-rows',
  'fill', 'stroke', 'cursor', 'pointer-events', 'content',
];

/** In the page: normalised DOM + style digest. */
function dump(props) {
  const UUID = /\b(?!5eed0000-)[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b/gi;
  const hash = s => { let h = 0x811c9dc5; for (let i = 0; i < s.length; i++) h = Math.imul(h ^ s.charCodeAt(i), 0x01000193); return (h >>> 0).toString(16); };
  const t0 = performance.now();
  // ---- DOM: live form values as attributes, volatile values masked, big constant blocks hashed
  const clone = document.documentElement.cloneNode(true);
  const live = document.querySelectorAll('input,textarea,select');
  const copies = clone.querySelectorAll('input,textarea,select');
  live.forEach((el, i) => {
    const c = copies[i];
    if (!c) return;
    if (el.type === 'checkbox' || el.type === 'radio') c.setAttribute('data-e2e-checked', String(el.checked));
    else c.setAttribute('data-e2e-value', el.value);
  });
  clone.querySelectorAll('#e2e-freeze').forEach(e => e.remove());
  clone.querySelectorAll('meta[name=csrf]').forEach(e => e.setAttribute('content', '<csrf>'));
  clone.querySelectorAll('input[name=csrf]').forEach(e => { e.setAttribute('value', '<csrf>'); e.setAttribute('data-e2e-value', '<csrf>'); });
  for (const sel of ['svg.sprite', 'script#i18n']) clone.querySelectorAll(sel).forEach(e => { e.innerHTML = `<!-- e2e: content hash ${hash(e.innerHTML)} -->`; });
  let html = '<!doctype html>\n' + clone.outerHTML;
  html = html.replace(/([?&])v=[0-9a-zA-Z_-]+/g, '$1v=<v>').replace(UUID, '<uuid>').replace(/></g, '>\n<');
  const t1 = performance.now();
  // ---- style digest: tag path (nth-of-type) → props + box
  const out = {};
  const pathOf = el => {
    const parts = [];
    for (let e = el; e && e.nodeType === 1; e = e.parentElement) {
      let i = 1;
      for (let s = e.previousElementSibling; s; s = s.previousElementSibling) if (s.tagName === e.tagName) i++;
      parts.unshift(`${e.tagName.toLowerCase()}${e.id ? '#' + e.id : ''}:${i}`);
    }
    return parts.join('>');
  };
  const pick = cs => props.map(p => cs.getPropertyValue(p));
  const r2 = v => Math.round(v * 100) / 100;
  for (const el of document.querySelectorAll('body *')) {
    if (el.id === 'e2e-freeze' || el.closest('svg.sprite')) continue;
    const rects = el.getClientRects();
    if (!rects.length) continue;
    const cs = getComputedStyle(el);
    if (cs.visibility === 'hidden') continue;
    const b = el.getBoundingClientRect();
    const k = pathOf(el);
    out[k] = { s: pick(cs), b: [r2(b.x + scrollX), r2(b.y + scrollY), r2(b.width), r2(b.height)] };
    for (const pe of ['::before', '::after']) {
      const ps = getComputedStyle(el, pe);
      if (ps.content && ps.content !== 'none' && ps.content !== 'normal') out[k + pe] = { s: pick(ps) };
    }
  }
  // the scroll positions of scrolled boxes change what a screenshot shows
  const scroll = {};
  for (const el of document.querySelectorAll('*')) if (el.scrollTop || el.scrollLeft) scroll[pathOf(el)] = [Math.round(el.scrollLeft), Math.round(el.scrollTop)];
  // the digest leaves the page as one JSON string: Playwright's value serialisation of the object (~60 values per
  // element) took ~80 ms per state, more than DOM + digest themselves
  return { html, style: JSON.stringify({ props, url: location.pathname + location.search + location.hash, scroll: { win: [scrollX, scrollY], ...scroll }, el: out }), t: [t1 - t0, performance.now() - t1, Object.keys(out).length] };
}

/** In the page: EVERY standard computed property (custom properties excluded) of html, body and each rendered
 *  element (+ ::before/::after with content, ::placeholder of inputs, ::marker of list items); identical
 *  property vectors are stored once. Opt-in (E2E_FULL_STYLE=1): a stricter gate than STYLE_PROPS for CSS refactors. */
function fullDump() {
  const cs0 = getComputedStyle(document.documentElement);
  const names = [];
  for (let i = 0; i < cs0.length; i++) if (!cs0[i].startsWith('--')) names.push(cs0[i]);
  names.sort();
  const pathOf = el => {
    const parts = [];
    for (let e = el; e && e.nodeType === 1; e = e.parentElement) {
      let i = 1;
      for (let s = e.previousElementSibling; s; s = s.previousElementSibling) if (s.tagName === e.tagName) i++;
      parts.unshift(`${e.tagName.toLowerCase()}${e.id ? '#' + e.id : ''}:${i}`);
    }
    return parts.join('>');
  };
  const vecs = [], index = new Map(), el = {};
  const put = (k, cs) => {
    const v = names.map(n => cs.getPropertyValue(n));
    const j = v.join('\u0001');
    let id = index.get(j);
    if (id === undefined) { id = vecs.length; vecs.push(v); index.set(j, id); }
    el[k] = id;
  };
  for (const e of [document.documentElement, document.body, ...document.querySelectorAll('body *')]) {
    if (e.id === 'e2e-freeze' || e.closest('svg.sprite')) continue;
    if (e !== document.documentElement && e !== document.body && !e.getClientRects().length) continue;
    const k = pathOf(e);
    put(k, getComputedStyle(e));
    for (const pe of ['::before', '::after']) {
      const ps = getComputedStyle(e, pe);
      if (ps.content && ps.content !== 'none' && ps.content !== 'normal') put(k + pe, ps);
    }
    if ((e.tagName === 'INPUT' || e.tagName === 'TEXTAREA') && e.placeholder) put(k + '::placeholder', getComputedStyle(e, '::placeholder'));
    if (getComputedStyle(e).display === 'list-item') put(k + '::marker', getComputedStyle(e, '::marker'));
  }
  return { names, vecs, el };
}

// E2E_SHOT_REF (gate.sh: the reference baseline): a first screenshot byte-identical to the reference's PNG of the same
// state is taken as stable without a second one. The reference was itself confirmed by two agreeing screenshots;
// equal bytes = the look the state settled to there. Any other first shot is confirmed as before.
const SHOT_REF = process.env.E2E_SHOT_REF || '';
async function stableShot(page, name, opts, t = {}, ref = null) {
  // the screenshot is taken until two agree: a late repaint must not land in the baseline
  let png = await page.screenshot(opts);
  t.shots = (t.shots || 0) + 1;
  if (ref) {
    let want = null;
    try { want = fs.readFileSync(ref); } catch (_) { /* a state the reference does not have */ }
    if (want && png.equals(want)) { t.refHits = (t.refHits || 0) + 1; return png; }
  }
  for (let i = 0; i < 5; i++) {
    const again = await page.screenshot(opts);
    t.shots++;
    if (again.equals(png)) return png;
    png = again;
    await settle(page);
  }
  throw new Error(`${name}: screenshot did not stabilise`);
}

/** Captures the current state as `name` (file-safe) for the running project. The page scrolls inside
 *  `main.main` (not the document): `segments` more screenshots `<name>~sN.png` follow its content down
 *  (0 for endless lists whose length depends on scrolling). The pointer stays where the test left it. */
export async function capture(page, testInfo, name, { fullPage = true, mask = [], segments = 3 } = {}) {
  const t = { k: 'capture', p: testInfo.project.name, n: name }, c0 = clock();
  await settle(page);
  const c1 = clock();
  const dir = path.join(OUT, testInfo.project.name);
  fs.mkdirSync(dir, { recursive: true });
  const base = path.join(dir, name);
  const shotOpts = { fullPage, animations: 'disabled', caret: 'hide', scale: 'device', mask, maskColor: '#ff00ff' };
  const refOf = n => SHOT_REF ? path.join(SHOT_REF, testInfo.project.name, n + '.png') : null;
  const png = await stableShot(page, name, shotOpts, t, refOf(name));
  fs.writeFileSync(base + '.png', png);
  const c2 = clock();
  const { html, style, t: inPage } = await page.evaluate(dump, STYLE_PROPS);
  [t.domIn, t.styleIn, t.els] = inPage;
  const c3 = clock();
  fs.writeFileSync(base + '.dom.html.gz', zlib.gzipSync(html, { level: 9 }));
  fs.writeFileSync(base + '.style.json.gz', zlib.gzipSync(style, { level: 9 }));
  if (process.env.E2E_FULL_STYLE === '1') fs.writeFileSync(base + '.fullstyle.json.gz', zlib.gzipSync(JSON.stringify(await page.evaluate(fullDump)), { level: 6 }));
  await recordThemeRules(page, testInfo, name);
  const c4 = clock();
  Object.assign(t, { settle: c1 - c0, shot: c2 - c1, dump: c3 - c2, write: c4 - c3, segs: 0, segSettle: 0, segShot: 0, restore: 0 });
  const main = page.locator('main.main');
  if (segments > 0 && (await main.count()) === 1) {
    const start = await main.evaluate(m => m.scrollTop);
    for (let i = 1; i <= segments; i++) {
      const more = await main.evaluate(m => {
        if (m.scrollTop + m.clientHeight >= m.scrollHeight - 2) return false;
        m.scrollTop += m.clientHeight;
        return true;
      });
      if (!more) break;
      const s0 = clock();
      await settle(page);
      const s1 = clock();
      fs.writeFileSync(`${base}~s${i}.png`, await stableShot(page, `${name}~s${i}`, shotOpts, t, refOf(`${name}~s${i}`)));
      t.segs++; t.segSettle += s1 - s0; t.segShot += clock() - s1;
    }
    // back where the test left it (nothing to undo when no segment scrolled)
    if (t.segs) {
      const r0 = clock();
      await main.evaluate((m, t) => { m.scrollTop = t; }, start);
      await settle(page);
      t.restore = clock() - r0;
    }
  }
  t.total = clock() - c0;
  mark(t);
}
