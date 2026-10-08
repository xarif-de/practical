// Navigation of the Markdown docs. The documentation table of docs/index.md (the site's start page, between the
// docs-index markers) is the one list of pages; every page it links to gets a "back to the start page" link at the
// top and at the bottom, and a copy of that table at the end (links rewritten relative to the page; absolute URLs
// such as the API doc stay). On the site (site/build.mjs) `index.md` becomes index.html; the build drops the bottom
// block there (its footer carries the same index). Generated blocks sit between <!-- docs-nav:top --> /
// <!-- docs-nav:bottom --> and <!-- /docs-nav -->; anything else in a page is left alone.
// Usage: node e2e/demo/docs-nav.mjs [--check]   (--check: exit 1 and name the pages that are out of date)
import fs from 'node:fs';
import path from 'node:path';
import { REPO } from '../lib/env.mjs';

const INDEX = 'docs/index.md';
const check = process.argv.includes('--check');
const src = fs.readFileSync(path.join(REPO, INDEX), 'utf8');
const m = src.match(/<!-- docs-index:start -->\n([\s\S]*?)<!-- docs-index:end -->/);
if (!m) throw new Error(`${INDEX}: docs-index markers missing`);
const table = m[1].trim();
// a link target of the table (relative to docs/) as a repo path
const repoPath = t => path.posix.normalize(path.posix.join(path.posix.dirname(INDEX), t));
const pages = [...new Set([...table.matchAll(/\]\(([^)#]+\.md)\)/g)].filter(x => !/^[a-z]+:/.test(x[1])).map(x => repoPath(x[1])))];

// a repo path as seen from `page`
const rel = (page, target) => path.posix.relative(path.posix.dirname(page), target) || path.posix.basename(target);
const block = (tag, body) => `<!-- docs-nav:${tag} -->\n${body}\n<!-- /docs-nav -->`;
const strip = s => s.replace(/<!-- docs-nav:(top|bottom) -->\n[\s\S]*?<!-- \/docs-nav -->\n*/g, '');

const stale = [];
for (const page of pages) {
  const file = path.join(REPO, page);
  const back = `<p><a href="https://xarif.gitlab.io/docker/practical/"><b>← Back to gallery and documentation</b></a></p>`; // absolute: on gitlab.com a relative .md link opens the source explorer; the site build makes it relative
  const index = table.replace(/\]\(([^)]+)\)/g, (_, t) => {
    if (/^[a-z]+:/.test(t)) return `](${t})`;
    const [p, frag] = t.split('#');
    return `](${rel(page, repoPath(p))}${frag !== undefined ? '#' + frag : ''})`;
  });
  const body = strip(fs.readFileSync(file, 'utf8')).trim();
  const next = `${block('top', back)}\n\n${body}\n\n${block('bottom', `---\n\n### Documentation\n\n${index}\n\n${back}`)}\n`;
  if (next !== fs.readFileSync(file, 'utf8')) { stale.push(page); if (!check) fs.writeFileSync(file, next); }
}
if (check && stale.length) { console.error(`docs-nav out of date (run node e2e/demo/docs-nav.mjs): ${stale.join(', ')}`); process.exit(1); }
console.log(check ? `docs-nav: ${pages.length} pages up to date` : `docs-nav: ${stale.length} of ${pages.length} pages updated`);
