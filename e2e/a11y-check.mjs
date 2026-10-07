#!/usr/bin/env node
// a11y-check.mjs: after the a11y suite (E2E_SUITE=a11y, same E2E_PORT) — every line of a11y-allow.txt was shown by a
// state of the run (else it is STALE: the finding is fixed or gone, delete the line: the list only shrinks), and no
// state showed a finding the list does not hold (the spec already failed on it; listed again here). Exit 1 on either,
// 2 without findings of a run (the suite did not run on this port).
import fs from 'node:fs';
import path from 'node:path';
import { FINDINGS, allowList } from './lib/axe.mjs';

if (!fs.existsSync(FINDINGS)) { console.error(`a11y-check: no ${FINDINGS} (run the a11y suite first: E2E_SUITE=a11y npx playwright test)`); process.exit(2); }
const allow = allowList(), seen = new Map();
let states = 0;
for (const p of fs.readdirSync(FINDINGS)) for (const f of fs.readdirSync(path.join(FINDINGS, p))) {
  states++;
  for (const k of JSON.parse(fs.readFileSync(path.join(FINDINGS, p, f), 'utf8'))) { if (!seen.has(k)) seen.set(k, []); seen.get(k).push(`${p}/${f.replace(/\.json$/, '')}`); }
}
const fresh = [...seen.keys()].filter(k => !allow.has(k)), stale = [...allow.keys()].filter(k => !seen.has(k));
console.log(`a11y-check: ${states} states, ${seen.size} distinct findings, ${seen.size - fresh.length} accepted (a11y-allow.txt, ${allow.size} lines), ${fresh.length} new, ${stale.length} stale`);
for (const k of fresh) console.log(`  NEW ${k.replace('\t', ' ')}  (${seen.get(k).slice(0, 3).join(', ')})`);
for (const k of stale) console.log(`  STALE allow entry (no state shows it): ${k.replace('\t', ' ')}`);
process.exit(fresh.length || stale.length ? 1 : 0);
