// Navigation of the Markdown docs. The README's documentation table (between the docs-index markers) is the one
// list of pages; every page it links to gets a "back to the README" link at the top and at the bottom, and a copy
// of that table at the end (links rewritten relative to the page). Generated blocks sit between
// <!-- docs-nav:top --> / <!-- docs-nav:bottom --> and <!-- /docs-nav -->; anything else in a page is left alone.
// Usage: node e2e/demo/docs-nav.mjs [--check]   (--check: exit 1 and name the pages that are out of date)
import fs from 'node:fs';
import path from 'node:path';
import { REPO } from '../lib/env.mjs';

const check = process.argv.includes('--check');
const readme = fs.readFileSync(path.join(REPO, 'README.md'), 'utf8');
const m = readme.match(/<!-- docs-index:start -->\n([\s\S]*?)<!-- docs-index:end -->/);
if (!m) throw new Error('README.md: docs-index markers missing');
const table = m[1].trim();
const pages = [...new Set([...table.matchAll(/\]\(([^)#]+\.md)\)/g)].map(x => x[1]))];

// a README-relative link target as seen from `page`
const rel = (page, target) => path.relative(path.dirname(page), target).split(path.sep).join('/') || path.basename(target);
const block = (tag, body) => `<!-- docs-nav:${tag} -->\n${body}\n<!-- /docs-nav -->`;
const strip = s => s.replace(/<!-- docs-nav:(top|bottom) -->\n[\s\S]*?<!-- \/docs-nav -->\n*/g, '');

const stale = [];
for (const page of pages) {
  const file = path.join(REPO, page);
  const back = `<p><a href="${rel(page, 'README.md')}"><b>← Back to the README</b></a></p>`;
  const index = table.replace(/\]\(([^)]+)\)/g, (_, t) => `](${t.startsWith('http') ? t : rel(page, t.split('#')[0]) + (t.includes('#') ? '#' + t.split('#')[1] : '')})`);
  const body = strip(fs.readFileSync(file, 'utf8')).trim();
  const next = `${block('top', back)}\n\n${body}\n\n${block('bottom', `---\n\n### Documentation\n\n${index}\n\n${back}`)}\n`;
  if (next !== fs.readFileSync(file, 'utf8')) { stale.push(page); if (!check) fs.writeFileSync(file, next); }
}
if (check && stale.length) { console.error(`docs-nav out of date (run node e2e/demo/docs-nav.mjs): ${stale.join(', ')}`); process.exit(1); }
console.log(check ? `docs-nav: ${pages.length} pages up to date` : `docs-nav: ${stale.length} of ${pages.length} pages updated`);
