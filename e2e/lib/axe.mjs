// axe-core (pinned devDependency) in a page: the WCAG 2.2 A/AA rules + axe's best practices. The source is evaluated
// through Playwright (Runtime.evaluate), not a <script>: the server's CSP forbids inline scripts, and an evaluation is
// exempt from it. Used by the a11y suite (e2e/a11y, gated against e2e/a11y-allow.txt) and, with E2E_AXE=1, by every
// visual state (lib/capture.mjs: <state>.axe.json next to the PNG — an audit of all of them).
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { FIRST_PORT, WORK } from './env.mjs';

const SOURCE = fs.readFileSync(createRequire(import.meta.url).resolve('axe-core/axe.min.js'), 'utf8');
export const AXE_TAGS = ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22a', 'wcag22aa', 'best-practice'];
// the a11y suite's findings of this run, one file per project and state (a11y-check.mjs: stale allow-list entries)
export const FINDINGS = path.join(WORK, `a11y-${FIRST_PORT}`);
export const ALLOW_FILE = new URL('../a11y-allow.txt', import.meta.url);

/** Violations of the page as [{ id, impact, help, nodes: [{ target, html, summary }] }] (+ `incomplete`: the
 *  contrast checks axe could not decide, e.g. text over an image or gradient). */
export async function axe(page) {
  if (!(await page.evaluate(() => !!window.axe))) await page.evaluate(SOURCE);
  return page.evaluate(async tags => {
    const r = await window.axe.run(document, { runOnly: { type: 'tag', values: tags }, resultTypes: ['violations', 'incomplete'] });
    const nodes = ns => ns.map(n => ({ target: n.target.join(' '), html: n.html.slice(0, 200), summary: (n.failureSummary || '').slice(0, 400) }));
    return {
      violations: r.violations.map(v => ({ id: v.id, impact: v.impact, help: v.help, nodes: nodes(v.nodes) })),
      incomplete: r.incomplete.filter(v => v.id === 'color-contrast').map(v => ({ id: v.id, nodes: nodes(v.nodes) })),
    };
  }, AXE_TAGS);
}

/** The key a finding is accepted under: for color-contrast the colour pair in the scheme ("dark #7d858f on #413626"
 *  — an element that shows an accepted pair is no new finding), else the target with its volatile parts dropped
 *  (positions, attribute values: dates, keys, labels). */
export function keyOf(rule, node, scheme) {
  if (rule === 'color-contrast') {
    const m = node.summary.match(/foreground color: (#\w+), background color: (#\w+)/);
    if (m) return `${scheme} ${m[1]} on ${m[2]}`;
  }
  let t = node.target.replace(/:nth-child\(\d+\)|:nth-of-type\(\d+\)/g, '').replace(/\[([\w-]+)="[^"]*"\]/g, (_, a) => /^(data-|aria-|title|value|href|style)/.test(a) ? '' : `[${a}]`).replace(/\s+/g, ' ').trim();
  // a bare tag ("div") would accept any element of it: the element's own classes name it
  const cls = (node.html.match(/^<[\w-]+[^>]*?\sclass="([^"]*)"/) || [])[1];
  if (!/[.#]/.test(t.split(' ').pop()) && cls && cls.trim()) t += '.' + cls.trim().split(/\s+/).join('.');
  return t;
}

/** e2e/a11y-allow.txt: `<rule>⇥<key>⇥<reason>` per line ('# ' starts a comment) → Map "rule⇥key" → reason. */
export function allowList() {
  return new Map(fs.readFileSync(ALLOW_FILE, 'utf8').split('\n').filter(l => l.trim() && !/^#\s/.test(l)).map(l => { const [r, k, why] = l.split('\t'); return [`${r}\t${k}`, why]; }));
}

/** The findings of one state as unique "rule⇥key" (+ an example target each). */
export function findings(result, scheme) {
  const out = new Map();
  for (const v of result.violations) for (const n of v.nodes) {
    const k = `${v.id}\t${keyOf(v.id, n, scheme)}`;
    if (!out.has(k)) out.set(k, { impact: v.impact, help: v.help, target: n.target, html: n.html, summary: n.summary });
  }
  return out;
}
