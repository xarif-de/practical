#!/usr/bin/env node
// theme-cover.mjs <baseline-dir>: every colour rule of style.css and history.css (lib/theme-cover.mjs) is shown by at
// least one state of a dark project — otherwise no dark capture can see a wrong dark colour there.
// Exit 1 lists the rules no dark state shows (add a state that shows each, or — when no state can — an entry in
// theme-cover-allow.txt: the selector exactly as listed, a tab, the reason). Exit 2: no data in the baseline.
import fs from 'node:fs';
import path from 'node:path';

const dir = process.argv[2];
const src = dir && path.join(dir, '.theme-cover');
if (!src || !fs.existsSync(src)) { console.error(`theme-cover: no ${src || '<baseline-dir>'}/.theme-cover (a baseline of this harness?)`); process.exit(2); }
// '# ' starts a comment line ('#title:hover' is a selector)
const allow = new Map(fs.readFileSync(new URL('./theme-cover-allow.txt', import.meta.url), 'utf8').split('\n')
  .filter(l => l.trim() && !/^#\s/.test(l)).map(l => l.split('\t')));
const tables = {}, hits = {}, states = new Set();
for (const f of fs.readdirSync(src)) for (const line of fs.readFileSync(path.join(src, f), 'utf8').split('\n').filter(Boolean)) {
  const j = JSON.parse(line);
  states.add(f.replace(/-\d+\.jsonl$/, '') + '/' + j.n);
  // a page's own sheets can add scheme-dependent properties (history.css): the tables of a sheet are merged
  for (const [p, t] of Object.entries(j.tables || {})) for (const [i, sel, cond] of t) {
    const m = (tables[p] ||= new Map()), was = m.get(i);
    if (was && was[0] !== sel) { console.error(`theme-cover: ${p} rule ${i} is ${was[0]} and ${sel} (two versions of the sheet)`); process.exit(2); }
    m.set(i, [sel, cond]);
  }
  for (const [p, is] of Object.entries(j.hit)) for (const i of is) (hits[p] ||= new Set()).add(i);
}
const gaps = [], allowed = [];
let n = 0;
for (const [p, t] of Object.entries(tables)) for (const [i, [sel, cond]] of [...t].sort((a, b) => a[0] - b[0])) {
  n++;
  if (hits[p]?.has(i)) continue;
  if (allow.has(sel)) { allowed.push(sel); continue; }
  gaps.push(`${path.basename(p)} #${i}${cond ? ` @${cond}` : ''}: ${sel}`);
}
const stale = [...allow.keys()].filter(sel => !allowed.includes(sel));
console.log(`theme-cover: ${n} colour rules, ${n - gaps.length - allowed.length} shown by ${states.size} dark states, ${allowed.length} allowed (theme-cover-allow.txt), ${gaps.length} shown by none`);
for (const g of gaps) console.log('  GAP ' + g);
for (const s of stale) console.log('  STALE allow entry (now shown or gone): ' + s);
process.exit(gaps.length || stale.length ? 1 : 0);
