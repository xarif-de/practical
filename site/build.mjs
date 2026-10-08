// PractiCal's GitLab Pages site: docs/index.md (start page: the screenshot gallery + the documentation index),
// docs/*.md and CONTRIBUTING.md as static HTML pages, flat at the site root, every link RELATIVE (the site works
// under any base URL: gitlab.io project path, a local `python3 -m http.server`). The Rust API doc (rustdoc) is copied
// into api/ by the Pages job / `mise run site`, not by this script.
// Links, rewritten at build time and validated (any failure = exit 1, nothing written):
//   docs/<page>.md / ../CONTRIBUTING.md (#anchor)  → <page>.html / contributing.html (#anchor must be a heading id)
//   SITE_URL + <path>                              → <path> (relative; Markdown that must also work on gitlab.com)
//   SITE_URL + api/…                               → api/… in a new tab (rel=noopener, external hint); not checked
//   ../README.md#x                                 → the project page on gitlab.com (#x kept)
//   docs/images/…                                  → images/… (copied)
//   any other repo path                            → its gitlab.com blob/tree URL (must exist in the repo)
//   #anchor on the same page                       → must be a heading id of that page
// README.md is checked too (not built): its SITE_URL links must hit a page and anchor of the site, its relative
// links an existing repo file.
// Heading ids follow GitLab (lib/banzai/filter/table_of_contents_filter.rb): lowercase, everything but letters,
// digits, `_`, `-` and space dropped, space → `-`, runs of `-` squeezed, duplicates get -1, -2 … — so an anchor
// that works on gitlab.com works here.
// Usage: node site/build.mjs [--out <dir>] [--check]   (default out: e2e/.work/site; --check: validate only)
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { Marked, Renderer } from 'marked';

const SITE_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.dirname(SITE_DIR);
export const SITE_URL = 'https://xarif.gitlab.io/docker/practical/';
const REPO_URL = 'https://gitlab.com/xarif/docker/practical';
const args = process.argv.slice(2);
const CHECK = args.includes('--check');
const OUT = path.resolve(args.includes('--out') ? args[args.indexOf('--out') + 1] : path.join(REPO, 'e2e', '.work', 'site'));

// ---------------------------------------------------------------- pages
const pages = new Map(); // repo path → site file
pages.set('docs/index.md', 'index.html');
for (const f of fs.readdirSync(path.join(REPO, 'docs')).filter(f => f.endsWith('.md')).sort()) if (f !== 'index.md') pages.set(`docs/${f}`, f.replace(/\.md$/, '.html'));
pages.set('CONTRIBUTING.md', 'contributing.html');
const siteToRepo = new Map([...pages].map(([r, s]) => [s, r]));

const errors = [];
const anchorChecks = []; // [from, site file, fragment]
const assets = new Set(); // repo paths of copied files (images)
const esc = s => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
const unesc = s => s.replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&quot;/g, '"').replace(/&#39;/g, "'").replace(/&amp;/g, '&');

// GitLab's heading id (see the header comment); `seen` = the ids of one page
export function slug(text, seen) {
  let s = text.toLowerCase().replace(/[^\p{L}\p{M}\p{N}\p{Pc}\- ]/gu, '').replace(/ /g, '-').replace(/-{2,}/g, '-');
  if (seen) { const n = seen.get(s); seen.set(s, (n ?? -1) + 1); if (n !== undefined) s = `${s}-${n + 1}`; }
  return s;
}

// width × height of a WebP (VP8 / VP8L / VP8X header), for the img attributes (no layout shift)
function webpSize(file) {
  const b = fs.readFileSync(file);
  if (b.toString('ascii', 0, 4) !== 'RIFF' || b.toString('ascii', 8, 12) !== 'WEBP') throw new Error(`${file}: not a WebP`);
  const kind = b.toString('ascii', 12, 16);
  if (kind === 'VP8 ') return [b.readUInt16LE(26) & 0x3fff, b.readUInt16LE(28) & 0x3fff];
  if (kind === 'VP8L') return [1 + (((b[22] & 0x3f) << 8) | b[21]), 1 + (((b[24] & 0x0f) << 10) | (b[23] << 2) | ((b[22] & 0xc0) >> 6))];
  if (kind === 'VP8X') return [1 + b.readUIntLE(24, 3), 1 + b.readUIntLE(27, 3)];
  throw new Error(`${file}: unknown WebP chunk ${kind}`);
}
const sizeAttrs = repoPath => (repoPath.endsWith('.webp') ? (([w, h]) => ` width="${w}" height="${h}"`)(webpSize(path.join(REPO, repoPath))) : '');

// one link target of `from` (a repo path) → { href, api } or null after recording an error
function resolve(from, href, kind = 'link') {
  const fail = why => { errors.push(`${from}: ${kind} ${href} — ${why}`); return null; };
  if (href.startsWith('#')) { anchorChecks.push([from, pages.get(from), href.slice(1)]); return { href }; }
  if (href.startsWith(SITE_URL)) {
    const rest = href.slice(SITE_URL.length);
    if (rest.startsWith('api/')) return { href: rest, api: true };
    const [file, frag] = rest.split('#');
    const target = file || 'index.html';
    if (!siteToRepo.has(target)) return fail(`no site page ${target}`);
    if (frag) anchorChecks.push([from, target, frag]);
    return { href: target + (frag ? `#${frag}` : '') };
  }
  if (/^([a-z][a-z0-9+.-]*:|\/\/)/i.test(href)) return { href };
  const [rel, frag] = href.split('#');
  const p = path.posix.normalize(path.posix.join(path.posix.dirname(from), decodeURI(rel)));
  if (p.startsWith('../')) return fail('points outside the repo');
  if (p === 'README.md') return { href: REPO_URL + (frag ? `#${frag}` : '') };
  if (pages.has(p)) { if (frag) anchorChecks.push([from, pages.get(p), frag]); return { href: pages.get(p) + (frag ? `#${frag}` : '') }; }
  const abs = path.join(REPO, p);
  if (!fs.existsSync(abs)) return fail(`${p} does not exist`);
  if (p.startsWith('docs/images/')) { assets.add(p); return { href: p.slice('docs/'.length), img: p }; }
  return { href: `${REPO_URL}/-/${fs.statSync(abs).isDirectory() ? 'tree' : 'blob'}/main/${p}${frag ? `#${frag}` : ''}` };
}
const NEW_TAB = ' target="_blank" rel="noopener" class="ext"';
const NEW_TAB_HINT = '<span class="ext-ico" aria-hidden="true">↗</span><span class="vh"> (opens in a new tab)</span>';

// raw HTML inside Markdown: rewrite href/src, give images their size and lazy loading
function rewriteHtml(from, html) {
  return html
    .replace(/<a\s([^>]*?)href="([^"]+)"([^>]*)>/g, (m, a, href, b) => {
      const r = resolve(from, href); if (!r) return m;
      return `<a ${a}href="${esc(r.href)}"${b}${r.api ? NEW_TAB : ''}>`;
    })
    .replace(/<img\s([^>]*?)src="([^"]+)"([^>]*?)\s*\/?>/g, (m, a, src, b) => {
      const r = resolve(from, src, 'image'); if (!r) return m;
      const rest = `${a}${b}`.replace(/\s*\b(width|height|loading|decoding)="[^"]*"/g, '');
      return `<img src="${esc(r.href)}"${r.img ? sizeAttrs(r.img) : ''} ${rest.trim()} loading="lazy" decoding="async">`.replace(/ +>/, '>').replace(/  +/g, ' ');
    });
}

// ---------------------------------------------------------------- Markdown → HTML
const ALERTS = { note: 'Note', tip: 'Tip', important: 'Important', warning: 'Warning', caution: 'Caution' };
function render(from, md) {
  const seen = new Map(), toc = [];
  let title = '', lastHeading = '';
  const r = new Renderer();
  r.heading = function ({ tokens, depth }) {
    const inner = this.parser.parseInline(tokens);
    const text = unesc(inner.replace(/<[^>]+>/g, '')).trim();
    const id = slug(text, seen);
    if (depth === 1 && !title) title = text;
    lastHeading = text;
    if (depth === 2) toc.push([id, inner.replace(/<a\b[^>]*>|<\/a>/g, '')]);
    const anchor = depth > 1 ? ` <a class="h-anchor" href="#${id}" aria-label="Link to this section: ${esc(text)}">#</a>` : '';
    return `<h${depth} id="${id}">${inner}${anchor}</h${depth}>\n`;
  };
  // a table scrolls inside a focusable region (keyboard scrolling); its name = the heading above it, unique per page
  const tableNames = new Map();
  r.table = function (token) {
    const base = `Table: ${lastHeading || title || 'page'}`, k = (tableNames.get(base) || 0) + 1;
    tableNames.set(base, k);
    return `<div class="table-wrap" tabindex="0" role="region" aria-label="${esc(k > 1 ? `${base} (${k})` : base)}">${Renderer.prototype.table.call(this, token)}</div>\n`;
  };
  r.blockquote = function ({ tokens }) {
    const body = this.parser.parse(tokens);
    const m = body.match(/^<p>\[!(note|tip|important|warning|caution)\][ \t]*([^\n<]*)\n?/i);
    if (!m) return `<blockquote>\n${body}</blockquote>\n`;
    const kind = m[1].toLowerCase(), head = m[2].trim() || ALERTS[kind];
    let rest = body.slice(m[0].length);
    rest = rest.startsWith('</p>') ? rest.slice(4).replace(/^\n/, '') : `<p>${rest}`; // the marker line alone, or the first paragraph goes on
    return `<div class="callout callout-${kind}" role="note"><p class="callout-title"><span class="callout-ico" aria-hidden="true"></span>${esc(head)}</p>\n${rest}</div>\n`;
  };
  r.link = function ({ href, title: t, tokens }) {
    const text = this.parser.parseInline(tokens);
    const res = resolve(from, href);
    const h = res ? res.href : href;
    return `<a href="${esc(h)}"${t ? ` title="${esc(t)}"` : ''}${res?.api ? NEW_TAB : ''}>${text}${res?.api ? NEW_TAB_HINT : ''}</a>`;
  };
  r.image = function ({ href, title: t, text }) {
    const res = resolve(from, href, 'image');
    return `<img src="${esc(res ? res.href : href)}"${res?.img ? sizeAttrs(res.img) : ''} alt="${esc(text)}"${t ? ` title="${esc(t)}"` : ''} loading="lazy" decoding="async">`;
  };
  r.html = function ({ text }) { return rewriteHtml(from, text); };
  const marked = new Marked({ gfm: true, renderer: r });
  const html = marked.parse(md);
  return { html, title, toc };
}
// every id a page really has: headings (with the -N duplicates) and explicit id="" attributes (the gallery's slides)
function idsOf(file, html) {
  const ids = [...html.matchAll(/\sid="([^"]+)"/g)].map(m => m[1]), set = new Set(ids);
  if (set.size !== ids.length) errors.push(`${file}: duplicate id ${ids.find((x, i) => ids.indexOf(x) !== i)}`);
  return set;
}

// ---------------------------------------------------------------- the documentation index (docs/index.md table)
const indexMd = fs.readFileSync(path.join(REPO, 'docs/index.md'), 'utf8');
const im = indexMd.match(/<!-- docs-index:start -->\n([\s\S]*?)<!-- docs-index:end -->/);
if (!im) throw new Error('docs/index.md: docs-index markers missing');
const inline = new Marked({ gfm: true });
const docIndex = im[1].trim().split('\n').slice(2).map(line => {
  const m = line.match(/^\|\s*\[([^\]]+)\]\(([^)]+)\)\s*\|\s*(.*?)\s*\|$/);
  if (!m) { errors.push(`docs/index.md: docs-index row not understood: ${line}`); return null; }
  const res = resolve('docs/index.md', m[2]);
  return res && { title: m[1], href: res.href, api: !!res.api, desc: inline.parseInline(m[3]) };
}).filter(Boolean);
const linkAttrs = d => `href="${esc(d.href)}"${d.api ? NEW_TAB : ''}`;
const indexCards = `<ul class="doc-cards">\n${docIndex.map(d => `<li><a ${linkAttrs(d)}><span class="doc-title">${esc(d.title)}${d.api ? NEW_TAB_HINT : ''}</span><span class="doc-desc">${d.desc}</span></a></li>`).join('\n')}\n</ul>\n`;
const footerIndex = `<ul class="foot-index">${docIndex.map(d => `<li><a ${linkAttrs(d)}>${esc(d.title)}${d.api ? NEW_TAB_HINT : ''}</a></li>`).join('')}</ul>`;

// ---------------------------------------------------------------- gallery (e2e/demo/shots.json)
const shots = JSON.parse(fs.readFileSync(path.join(REPO, 'e2e/demo/shots.json'), 'utf8'));
const groups = [];
for (const s of shots) {
  if (!s.group) errors.push(`e2e/demo/shots.json: ${s.file} has no group`);
  let g = groups.find(x => x.name === s.group); if (!g) groups.push(g = { name: s.group, shots: [] });
  g.shots.push(s);
}
const ordered = groups.flatMap(g => g.shots); // grouped order: a chip jumps to a contiguous run
function galleryHtml() {
  const slides = ordered.map((s, i) => {
    const img = `docs/images/${s.file}`;
    if (fs.existsSync(path.join(REPO, img))) assets.add(img); else errors.push(`gallery: ${img} missing (mise run readme-shots)`);
    if (errors.length) return '';
    const id = `shot-${s.file.replace(/^\d+-|\.webp$/g, '')}`;
    return `<li class="g-slide" id="${id}" data-group="${esc(s.group)}" aria-roledescription="slide" aria-label="${i + 1} of ${ordered.length}: ${esc(s.title)}">
<figure><a class="g-img" href="images/${s.file}"><img src="images/${s.file}"${sizeAttrs(img)} alt="Screenshot: ${esc(s.title)}" loading="lazy" decoding="async"></a>
<figcaption><span class="g-group">${esc(s.group)}</span> <strong>${esc(s.title)}</strong> <span class="g-cap">${esc(s.caption)}</span></figcaption></figure></li>`;
  }).join('\n');
  const chips = groups.map(g => `<button type="button" class="g-chip" data-to="${ordered.indexOf(g.shots[0])}">${esc(g.name)} <span class="g-n">${g.shots.length}</span></button>`).join('');
  return `<section class="gallery" aria-label="Screenshot gallery" data-count="${ordered.length}">
<div class="g-chips" role="group" aria-label="Jump to a group" hidden>${chips}</div>
<div class="g-stage" tabindex="0" aria-roledescription="carousel" aria-label="Screenshots: left and right arrow keys switch">
<ol class="g-slides">
${slides}
</ol>
<button type="button" class="g-nav g-prev" aria-label="Previous screenshot" hidden><span aria-hidden="true">‹</span></button>
<button type="button" class="g-nav g-next" aria-label="Next screenshot" hidden><span aria-hidden="true">›</span></button>
</div>
<p class="g-pos" hidden><span class="g-pos-n">1</span> / ${ordered.length}</p>
<p class="vh" aria-live="polite" aria-atomic="true"></p>
</section>
`;
}

// ---------------------------------------------------------------- pages
const template = fs.readFileSync(path.join(SITE_DIR, 'template.html'), 'utf8');
const built = new Map(); // site file → html
const idSets = new Map();
for (const [repoPath, file] of pages) {
  let md = fs.readFileSync(path.join(REPO, repoPath), 'utf8').replace(/<!-- docs-nav:(top|bottom) -->\n[\s\S]*?<!-- \/docs-nav -->\n*/g, '');
  const isIndex = file === 'index.html';
  if (isIndex) {
    const g = md.match(/<!-- gallery:start[^>]*-->\n[\s\S]*?<!-- gallery:end -->/);
    if (!g) errors.push('docs/index.md: gallery markers missing');
    else md = md.replace(g[0], '<!--SITE:GALLERY-->');
    md = md.replace(im[0], '<!--SITE:INDEX-->');
  }
  let { html, title, toc } = render(repoPath, md);
  if (isIndex) html = html.replace('<!--SITE:GALLERY-->', galleryHtml()).replace('<!--SITE:INDEX-->', indexCards);
  if (!title) errors.push(`${repoPath}: no # heading (page title)`);
  // "On this page": pages with at least three ## sections (a sticky sidebar on wide screens)
  const tocHtml = !isIndex && toc.length >= 3
    ? `<nav class="toc" aria-label="On this page"><p class="toc-h">On this page</p><ol>${toc.map(([id, h]) => `<li><a href="#${id}">${h}</a></li>`).join('')}</ol></nav>` : '';
  // narrow screens: the same list, collapsed, below the back link (the sidebar is display:none there, and vice versa)
  const tocMobile = tocHtml.replace(/^<nav class="toc" aria-label="On this page"><p class="toc-h">On this page<\/p>(.*)<\/nav>$/, '<details class="toc-m"><summary>On this page</summary>$1</details>');
  const back = isIndex ? '' : '<p class="back"><a href="index.html">← Gallery and documentation</a></p>';
  const out = template
    .replaceAll('{{title}}', esc(isIndex ? 'PractiCal — gallery and documentation' : `${title} · PractiCal`))
    .replaceAll('{{description}}', esc(isIndex ? 'Screenshots and documentation of PractiCal, a fast self-hosted web app for calendars, contacts and tasks on RustiCal.' : `${title} — PractiCal documentation`))
    .replaceAll('{{bodyClass}}', isIndex ? 'page-index' : `page-doc${tocHtml ? ' has-toc' : ''}`)
    .replaceAll('{{script}}', isIndex ? '<script>document.documentElement.classList.add("js")</script>\n<script src="gallery.js" defer></script>' : '')
    .replaceAll('{{source}}', `${REPO_URL}/-/blob/main/${repoPath}`)
    .replaceAll('{{repo}}', REPO_URL)
    .replace('{{toc}}', tocHtml)
    .replace('{{backTop}}', back + tocMobile)
    .replace('{{backBottom}}', back)
    .replace('{{footerIndex}}', isIndex ? '' : `<h2 class="foot-h">Documentation</h2>${footerIndex}`)
    .replace('{{content}}', html);
  built.set(file, out);
  idSets.set(file, idsOf(file, out));
}
for (const [from, file, frag] of anchorChecks) if (!idSets.get(file)?.has(decodeURIComponent(frag))) errors.push(`${from}: anchor #${frag} not found in ${file}`);

// every relative href/src the pages really carry (template included) must hit a page + id, a copied file or api/
const statics = new Set(['style.css', 'gallery.js', 'icon.svg', 'logo.svg', ...[...assets].map(p => p.slice('docs/'.length))]);
for (const [file, html] of built) {
  for (const [, u] of html.matchAll(/\s(?:href|src)="([^"]*)"/g)) {
    const t = unesc(u);
    if (/^([a-z][a-z0-9+.-]*:|\/\/)/i.test(t) || t.startsWith('api/')) continue;
    const [f, frag] = t.split('#'), target = f || file;
    if (built.has(target)) { if (frag && !idSets.get(target).has(decodeURIComponent(frag))) errors.push(`${file}: #${frag} not found in ${target}`); }
    else if (!statics.has(target)) errors.push(`${file}: ${t} is not part of the site`);
  }
}

// README.md (gitlab.com project page, not part of the site): links into the site and into the repo must resolve
{
  const readme = fs.readFileSync(path.join(REPO, 'README.md'), 'utf8').replace(/```[\s\S]*?```/g, '');
  const targets = [...readme.matchAll(/\]\(([^)\s]+)\)|(?:href|src)="([^"]+)"/g)].map(m => m[1] || m[2]);
  for (const t of targets) {
    if (t.startsWith(SITE_URL)) {
      const rest = t.slice(SITE_URL.length); if (rest.startsWith('api/')) continue;
      const [f, frag] = rest.split('#'), file = f || 'index.html';
      if (!idSets.has(file)) errors.push(`README.md: link ${t} — no site page ${file}`);
      else if (frag && !idSets.get(file).has(frag)) errors.push(`README.md: link ${t} — anchor #${frag} not found in ${file}`);
    } else if (!/^([a-z][a-z0-9+.-]*:|\/\/|#)/i.test(t) && !fs.existsSync(path.join(REPO, t.split('#')[0]))) errors.push(`README.md: link ${t} — file does not exist`);
  }
}

if (errors.length) { console.error(`site: ${errors.length} broken link(s) or input error(s):\n  ${errors.join('\n  ')}`); process.exit(1); }
if (CHECK) { console.log(`site: ${pages.size} pages, ${anchorChecks.length} anchors and every link resolve (--check: nothing written)`); process.exit(0); }

// ---------------------------------------------------------------- write
fs.rmSync(OUT, { recursive: true, force: true });
fs.mkdirSync(path.join(OUT, 'images'), { recursive: true });
for (const [file, html] of built) fs.writeFileSync(path.join(OUT, file), html);
for (const p of assets) fs.copyFileSync(path.join(REPO, p), path.join(OUT, p.slice('docs/'.length)));
for (const f of ['style.css', 'gallery.js']) fs.copyFileSync(path.join(SITE_DIR, f), path.join(OUT, f));
for (const f of ['icon.svg', 'logo.svg']) fs.copyFileSync(path.join(REPO, 'build/assets', f), path.join(OUT, f));
console.log(`site: ${pages.size} pages, ${assets.size} images, links checked → ${OUT}`);
