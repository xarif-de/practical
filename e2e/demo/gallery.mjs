// README gallery pages from e2e/demo/shots.json: docs/gallery/<NN>-<slug>.md, one screenshot per page with
// previous/next links (wrapping), because GitLab renders no script or carousel in Markdown. The README's thumbnails
// link to these pages. Usage: node e2e/demo/gallery.mjs (run by `mise run readme-shots` after shots.mjs).
import fs from 'node:fs';
import path from 'node:path';
import { E2E, REPO } from '../lib/env.mjs';

const shots = JSON.parse(fs.readFileSync(path.join(E2E, 'demo', 'shots.json'), 'utf8'));
const dir = path.join(REPO, 'docs', 'gallery');
fs.rmSync(dir, { recursive: true, force: true });
fs.mkdirSync(dir, { recursive: true });
const page = s => s.file.replace(/\.webp$/, '.md');
shots.forEach((s, i) => {
  const prev = shots[(i - 1 + shots.length) % shots.length], next = shots[(i + 1) % shots.length];
  const nav = `<p align="center"><a href="${page(prev)}">◀ ${prev.title}</a> · <b>${i + 1} / ${shots.length}</b> · <a href="${page(next)}">${next.title} ▶</a><br><a href="../../README.md#gallery">all screenshots</a></p>`;
  fs.writeFileSync(path.join(dir, page(s)), `${nav}\n\n## ${s.title}\n\n${s.caption}\n\n<a href="${page(next)}"><img src="../images/${s.file}" alt="${s.title}" width="100%"></a>\n\n${nav}\n`);
});
console.log(`gallery: ${shots.length} pages in ${path.relative(REPO, dir)}`);
