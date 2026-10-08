// README gallery from e2e/demo/shots.json. GitLab's Markdown keeps no script, style, form input or element id, so
// nothing can switch images in place; <details> survives (checked via POST /api/v4/markdown, 2026-10-08):
//  - README.md between the gallery markers: one collapsible block per screenshot, grouped (GROUPS), the first one
//    open; GitLab lazy-loads images, so a closed block costs no download;
//  - docs/gallery/<NN>-<slug>.md: one screenshot per page with wrapping ◀ ▶ links; the README images link there.
// Usage: node e2e/demo/gallery.mjs (run by `mise run readme-shots` after shots.mjs).
import fs from 'node:fs';
import path from 'node:path';
import { E2E, REPO } from '../lib/env.mjs';

// [heading, number of consecutive shots.json entries]
const GROUPS = [['Calendar', 8], ['Tasks', 6], ['Contacts', 2], ['Editors — view and edit mode', 6], ['Colours, search, dark mode', 4]];
const shots = JSON.parse(fs.readFileSync(path.join(E2E, 'demo', 'shots.json'), 'utf8'));
if (GROUPS.reduce((n, [, k]) => n + k, 0) !== shots.length) throw new Error('gallery.mjs GROUPS do not cover shots.json');
const page = s => s.file.replace(/\.webp$/, '.md');

const dir = path.join(REPO, 'docs', 'gallery');
fs.rmSync(dir, { recursive: true, force: true });
fs.mkdirSync(dir, { recursive: true });
shots.forEach((s, i) => {
  const prev = shots[(i - 1 + shots.length) % shots.length], next = shots[(i + 1) % shots.length];
  const nav = `<p align="center"><a href="${page(prev)}">◀ ${prev.title}</a> · <b>${i + 1} / ${shots.length}</b> · <a href="${page(next)}">${next.title} ▶</a><br><a href="../../README.md#gallery">all screenshots</a></p>`;
  fs.writeFileSync(path.join(dir, page(s)), `${nav}\n\n## ${s.title}\n\n${s.caption}\n\n<a href="${page(next)}"><img src="../images/${s.file}" alt="${s.title}" width="100%"></a>\n\n${nav}\n`);
});

let md = 'Click a title to open the screenshot right here; click the picture to page through all of them with ◀ ▶ (demo data).\n', at = 0;
for (const [head, n] of GROUPS) {
  md += `\n### ${head}\n\n`;
  for (const s of shots.slice(at, at + n)) {
    md += `<details${at === 0 && s === shots[0] ? ' open' : ''}>\n<summary><b>${s.title}</b></summary>\n\n${s.caption}\n\n<a href="docs/gallery/${page(s)}"><img src="docs/images/${s.file}" alt="${s.title}" width="100%"></a>\n\n</details>\n`;
  }
  at += n;
}
const readme = path.join(REPO, 'README.md'), START = /<!-- gallery:start[^>]*-->\n/, END = '<!-- gallery:end -->';
const r = fs.readFileSync(readme, 'utf8'), m = r.match(START), e = r.indexOf(END);
if (!m || e < m.index) throw new Error('README.md: gallery markers missing');
fs.writeFileSync(readme, r.slice(0, m.index + m[0].length) + md + r.slice(e));
console.log(`gallery: README section + ${shots.length} pages in ${path.relative(REPO, dir)}`);
