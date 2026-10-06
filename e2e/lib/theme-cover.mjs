// Which colour rules a dark capture shows. A colour rule names `data-scheme` in its selector or reads a colour custom
// property: one set in an `html[data-scheme=dark]` rule, one whose value is a colour, or one defined from those. It is
// shown when its conditions hold (@media, @supports; @container is taken as true) and its selector — pseudo-elements
// stripped, pseudo-classes such as :hover kept, i.e. as the page is now — matches a rendered element (client rects,
// not visibility: hidden): the elements the style digest covers. Per capture of a dark project one line in <OUT>/.theme-cover/<project>-<pid>.jsonl;
// theme-cover.mjs checks that every such rule of style.css and history.css is shown by at least one dark state.
// Why: 3 of 13 random dark-only colour mutants (mutation study 2026-10-06: D5, D7, D8) sat on rules no dark state
// showed, so the dark projects could not see them.
import fs from 'node:fs';
import path from 'node:path';
import { OUT } from './env.mjs';

/** In the page: { sheets: {path: [[index, selector, conditions]]} of every scheme-dependent rule, hit: {path: [index]} }. */
function themeRules() {
  const SHEETS = /\/assets\/(style|history)\.css$/;
  const flat = []; // [sheetPath, index, rule, conditions]
  for (const sh of document.styleSheets) {
    const p = sh.href ? new URL(sh.href).pathname : '';
    if (!SHEETS.test(p)) continue;
    let i = 0;
    const walk = (rules, conds) => {
      for (const r of rules) {
        if (r instanceof CSSStyleRule) { flat.push([p, i++, r, conds]); if (r.cssRules && r.cssRules.length) walk(r.cssRules, conds); }
        else if (r instanceof CSSMediaRule) walk(r.cssRules, [...conds, ['media', r.media.mediaText]]);
        else if (r instanceof CSSSupportsRule) walk(r.cssRules, [...conds, ['supports', r.conditionText]]);
        else if (r.cssRules) walk(r.cssRules, [...conds, [r.constructor.name, r.conditionText || '']]);
      }
    };
    walk(sh.cssRules, []);
  }
  const dark = s => /data-scheme/.test(s);
  // the scheme-dependent custom properties: set in a data-scheme rule, or defined from one of those (fixpoint)
  const defs = [];
  for (const [, , r] of flat) for (const prop of r.style) if (prop.startsWith('--')) defs.push([prop, r.style.getPropertyValue(prop), dark(r.selectorText)]);
  const reads = v => [...v.matchAll(/var\(\s*(--[\w-]+)/g)].map(m => m[1]);
  const closure = set => { for (let grew = true; grew;) { grew = false; for (const [n, v] of defs) if (!set.has(n) && reads(v).some(x => set.has(x))) { set.add(n); grew = true; } } return set; };
  // colour properties too (a literal colour, or defined from one): a light-equal twin swapped in (--ok → --c2) changes dark only
  const S = closure(new Set(defs.filter(d => d[2] || (!/var\(/.test(d[1]) && CSS.supports('color', d[1].trim()))).map(d => d[0])));
  // cssText, not the longhands: a shorthand that reads var() leaves its longhands empty in the CSSOM
  const themed = r => dark(r.selectorText) || reads(r.style.cssText).some(x => S.has(x));
  const holds = conds => conds.every(([k, c]) => k === 'media' ? (c === 'print' ? false : matchMedia(c).matches) : k === 'supports' ? CSS.supports(c) : true);
  const shown = el => el.getClientRects().length > 0 && getComputedStyle(el).visibility !== 'hidden';
  // a pseudo-element goes; where it was the whole compound (`::-webkit-scrollbar`), `*` takes its place
  const strip = s => s.replace(/::?(before|after|placeholder|marker|selection|backdrop|first-line|first-letter|file-selector-button|-webkit-[\w-]+|-moz-[\w-]+)(\([^)]*\))?/g, (m, _a, _b, at) => at === 0 || /[\s>+~,(]/.test(s[at - 1]) ? '*' : '');
  const sheets = {}, hit = {};
  for (const [p, i, r, conds] of flat) {
    if (!themed(r)) continue;
    (sheets[p] ||= []).push([i, r.selectorText, conds.map(c => c.join(' ')).join(' & ')]);
    if (!holds(conds)) continue;
    let els;
    try { els = document.querySelectorAll(strip(r.selectorText)); } catch (_) { continue; }
    for (const el of els) if (shown(el)) { (hit[p] ||= []).push(i); break; }
  }
  return { sheets, hit };
}

const seen = new Set(); // the rule tables this worker wrote already (same CSS → same table)
export async function recordThemeRules(page, testInfo, name) {
  if (testInfo.project.use.colorScheme !== 'dark') return;
  const t0 = Date.now(), { sheets, hit } = await page.evaluate(themeRules), ms = Date.now() - t0;
  const dir = path.join(OUT, '.theme-cover');
  fs.mkdirSync(dir, { recursive: true });
  const file = path.join(dir, `${testInfo.project.name}-${process.pid}.jsonl`);
  const tables = {};
  for (const [p, t] of Object.entries(sheets)) { const k = p + ' ' + t.length; if (!seen.has(k)) { seen.add(k); tables[p] = t; } }
  fs.appendFileSync(file, JSON.stringify({ n: name, ms, hit, ...(Object.keys(tables).length ? { tables } : {}) }) + '\n');
}
