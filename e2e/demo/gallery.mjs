// README gallery from e2e/demo/shots.json. GitLab's Markdown keeps no script, style, form input or element id, so
// nothing can switch images in place; <details> survives (checked via POST /api/v4/markdown, 2026-10-08). This writes
// the README section between the gallery markers: one collapsed block per screenshot, grouped (GROUPS). GitLab
// lazy-loads images, so a closed block costs no download.
// Usage: node e2e/demo/gallery.mjs (run by `mise run readme-shots` after shots.mjs).
import fs from 'node:fs';
import path from 'node:path';
import { E2E, REPO } from '../lib/env.mjs';

// [heading, number of consecutive shots.json entries]; entries with `readme: false` are left out (docs-only images)
const GROUPS = [['Calendar', 8], ['Tasks', 6], ['Contacts', 2], ['Editors — view and edit mode', 6], ['Colours, search, dark mode', 4]];
const shots = JSON.parse(fs.readFileSync(path.join(E2E, 'demo', 'shots.json'), 'utf8')).filter(s => s.readme !== false);
if (GROUPS.reduce((n, [, k]) => n + k, 0) !== shots.length) throw new Error('gallery.mjs GROUPS do not cover shots.json');

let md = 'Click a title to open the screenshot right here (demo data).\n', at = 0;
for (const [head, n] of GROUPS) {
  md += `\n### ${head}\n\n`;
  for (const s of shots.slice(at, at + n)) md += `<details>\n<summary><b>${s.title}</b></summary>\n\n${s.caption}\n\n<img src="docs/images/${s.file}" alt="${s.title}" width="100%">\n\n</details>\n`;
  at += n;
}
const readme = path.join(REPO, 'README.md'), START = /<!-- gallery:start[^>]*-->\n/, END = '<!-- gallery:end -->';
const r = fs.readFileSync(readme, 'utf8'), m = r.match(START), e = r.indexOf(END);
if (!m || e < m.index) throw new Error('README.md: gallery markers missing');
fs.writeFileSync(readme, r.slice(0, m.index + m[0].length) + md + r.slice(e));
console.log(`gallery: README section with ${shots.length} screenshots`);
