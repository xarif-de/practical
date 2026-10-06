#!/usr/bin/env node
// compare.mjs <dirA> <dirB> [--report <dir>]
// Diffs two artifact trees of the visual suite (e2e-baseline): per state the PNG (pixelmatch,
// threshold 0 → number of differing pixels), the normalised DOM (.dom.html.gz, line diff) and the
// computed-style digest (.style.json.gz, per element/property). Writes report.md + report.html (+ diff
// PNGs) into the report dir (default <dirB>/compare-report) and exits 1 on ANY difference, 2 on usage errors.
// A state whose files are byte-identical in A and B is equal without decoding (same bytes → same pixels, same
// text); the others are diffed in COMPARE_THREADS worker threads (default 4).
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { Worker, isMainThread, parentPort, workerData } from 'node:worker_threads';
import zlib from 'node:zlib';
import pixelmatch from 'pixelmatch';
import { PNG } from 'pngjs';

const EXT = ['.png', '.dom.html.gz', '.style.json.gz'];
function states(root, reportDir) {
  const out = new Map();
  const walk = d => {
    for (const e of fs.readdirSync(d, { withFileTypes: true })) {
      const p = path.join(d, e.name);
      if (e.isDirectory()) { if (p !== reportDir && e.name !== 'compare-report') walk(p); continue; }
      const ext = EXT.find(x => e.name.endsWith(x));
      if (!ext) continue;
      const key = path.relative(root, p).slice(0, -ext.length);
      if (!out.has(key)) out.set(key, new Set());
      out.get(key).add(ext);
    }
  };
  walk(root);
  return out;
}
const gunzip = f => zlib.gunzipSync(fs.readFileSync(f)).toString('utf8');

/** The page shell moved what it carried inline into cached files (2026-10, package build): the icon sprite
 *  (`<svg class="sprite">` → assets/icons.svg, `href="#i-x"` → `href="/assets/icons.svg?v=…#i-x"` + <meta
 *  name=icons>), app.js' strings (`<script id="i18n">` → /assets/i18n-<lang>.js) and the colour-scheme script
 *  (inline → /assets/theme.js). Both forms normalise to one, so a baseline from before compares with one
 *  from after; their contents are pinned by cargo tests (it/assets.rs) and the functional suite. */
function normDom(html) {
  return html
    .replace(/href="\/assets\/icons\.svg\?v=<v>#/g, 'href="#')
    .replace(/^<meta name="icons" content="\/assets\/icons\.svg\?v=<v>">\n/m, '')
    .replace(/^<script src="\/assets\/i18n-[a-z]+\.js\?v=<v>" defer="">\n<\/script>\n/m, '')
    .replace(/^<script src="\/assets\/theme\.js\?v=<v>">\n<\/script>\n/m, '')
    .replace(/^<script>\(function\(\)\{const h=document\.documentElement,m=matchMedia[^\n]*<\/script>\n/m, '')
    .replace(/^<script type="application\/json" id="i18n">\n<!-- e2e: content hash [0-9a-f]+ -->\n<\/script>\n/m, '')
    .replace(/^<svg class="sprite" aria-hidden="true" xmlns="http:\/\/www\.w3\.org\/2000\/svg">\n<!-- e2e: content hash [0-9a-f]+ -->\n<\/svg>\n/m, '');
}

/** Line diff: common prefix/suffix trimmed, then an LCS on the (bounded) middle → hunks of -/+ lines. */
function lineDiff(a, b, max = 80) {
  const x = a.split('\n'), y = b.split('\n');
  let p = 0;
  while (p < x.length && p < y.length && x[p] === y[p]) p++;
  let s = 0;
  while (s < x.length - p && s < y.length - p && x[x.length - 1 - s] === y[y.length - 1 - s]) s++;
  const xm = x.slice(p, x.length - s), ym = y.slice(p, y.length - s);
  const out = [];
  if (xm.length * ym.length <= 4e6) {
    const n = xm.length, m = ym.length, L = Array.from({ length: n + 1 }, () => new Uint32Array(m + 1));
    for (let i = n - 1; i >= 0; i--) for (let j = m - 1; j >= 0; j--) L[i][j] = xm[i] === ym[j] ? L[i + 1][j + 1] + 1 : Math.max(L[i + 1][j], L[i][j + 1]);
    let i = 0, j = 0;
    while (i < n || j < m) {
      if (i < n && j < m && xm[i] === ym[j]) { i++; j++; continue; }
      if (j < m && (i === n || L[i][j + 1] >= L[i + 1][j])) out.push(`@${p + i + 1} + ${ym[j++]}`);
      else out.push(`@${p + i + 1} - ${xm[i++]}`);
    }
  } else {
    xm.forEach((l, i) => out.push(`@${p + i + 1} - ${l}`));
    ym.forEach(l => out.push(`@${p + 1} + ${l}`));
  }
  return { count: out.length, lines: out.slice(0, max) };
}

function styleDiff(a, b, max = 60) {
  const ja = JSON.parse(a), jb = JSON.parse(b);
  const out = [];
  if (JSON.stringify(ja.props) !== JSON.stringify(jb.props)) out.push('property list differs (different capture.mjs) — compare per element unreliable');
  if (ja.url !== jb.url) out.push(`url: ${ja.url} → ${jb.url}`);
  if (JSON.stringify(ja.scroll) !== JSON.stringify(jb.scroll)) out.push(`scroll: ${JSON.stringify(ja.scroll)} → ${JSON.stringify(jb.scroll)}`);
  let n = out.length;
  const ka = Object.keys(ja.el), kb = new Set(Object.keys(jb.el));
  for (const k of ka) {
    if (!kb.has(k)) { n++; if (out.length < max) out.push(`- element ${k}`); continue; }
    const ea = ja.el[k], eb = jb.el[k];
    if (JSON.stringify(ea.b) !== JSON.stringify(eb.b)) { n++; if (out.length < max) out.push(`box ${k}: ${JSON.stringify(ea.b)} → ${JSON.stringify(eb.b)}`); }
    ea.s.forEach((v, i) => {
      if (v !== eb.s[i]) { n++; if (out.length < max) out.push(`${ja.props[i]} ${k}: ${v} → ${eb.s[i]}`); }
    });
  }
  for (const k of kb) if (!(k in ja.el)) { n++; if (out.length < max) out.push(`+ element ${k}`); }
  return { count: n, lines: out };
}

/** The differences of one state (`ea`/`eb`: the extensions present in A/B). */
function compareState(A, B, reportDir, key, ea, eb) {
  ea = new Set(ea); eb = new Set(eb);
  const r = { key, png: null, dom: null, style: null, missing: [] };
  // `<name>~sN`: a scrolled screenshot of a state, PNG only (capture.mjs segments)
  for (const ext of key.includes('~s') ? ['.png'] : EXT) {
    if (!ea.has(ext)) r.missing.push(`${ext} missing in A`);
    if (!eb.has(ext)) r.missing.push(`${ext} missing in B`);
  }
  if (ea.has('.png') && eb.has('.png')) {
    const pa = PNG.sync.read(fs.readFileSync(path.join(A, key + '.png')));
    const pb = PNG.sync.read(fs.readFileSync(path.join(B, key + '.png')));
    if (pa.width !== pb.width || pa.height !== pb.height) {
      r.png = { size: `${pa.width}×${pa.height} → ${pb.width}×${pb.height}`, pixels: -1 };
    } else {
      const diff = new PNG({ width: pa.width, height: pa.height });
      const n = pixelmatch(pa.data, pb.data, diff.data, pa.width, pa.height, { threshold: 0, includeAA: true });
      r.png = { pixels: n, total: pa.width * pa.height };
      if (n) {
        const f = key.replace(/[\\/]/g, '__');
        for (const [tag, img] of [['a', pa], ['b', pb], ['diff', diff]]) fs.writeFileSync(path.join(reportDir, 'img', `${f}.${tag}.png`), PNG.sync.write(img));
        r.png.img = f;
      }
    }
  }
  if (ea.has('.dom.html.gz') && eb.has('.dom.html.gz')) {
    const a = normDom(gunzip(path.join(A, key + '.dom.html.gz'))), b = normDom(gunzip(path.join(B, key + '.dom.html.gz')));
    r.dom = a === b ? { count: 0, lines: [] } : lineDiff(a, b);
  }
  if (ea.has('.style.json.gz') && eb.has('.style.json.gz')) {
    const a = gunzip(path.join(A, key + '.style.json.gz')), b = gunzip(path.join(B, key + '.style.json.gz'));
    r.style = a === b ? { count: 0, lines: [] } : styleDiff(a, b);
  }
  r.bad = r.missing.length > 0 || (r.png && r.png.pixels !== 0) || (r.dom && r.dom.count > 0) || (r.style && r.style.count > 0);
  return r;
}

/** Every expected file present on both sides with the same bytes: equal, nothing to decode. */
function identical(A, B, key, ea, eb) {
  const exts = key.includes('~s') ? ['.png'] : EXT;
  if (ea.size !== exts.length || eb.size !== exts.length || !exts.every(x => ea.has(x) && eb.has(x))) return false;
  return exts.every(x => fs.readFileSync(path.join(A, key + x)).equals(fs.readFileSync(path.join(B, key + x))));
}

if (!isMainThread) {
  const { A, B, reportDir } = workerData;
  parentPort.on('message', ({ key, ea, eb }) => parentPort.postMessage(compareState(A, B, reportDir, key, ea, eb)));
} else {
  await main();
}

async function main() {
  const args = process.argv.slice(2);
  const ri = args.indexOf('--report');
  const reportDir0 = ri >= 0 ? args.splice(ri, 2)[1] : null;
  if (args.length !== 2) {
    console.error('usage: compare.mjs <dirA> <dirB> [--report <dir>]');
    process.exit(2);
  }
  const [A, B] = args.map(a => path.resolve(a));
  for (const d of [A, B]) if (!fs.existsSync(d)) { console.error(`no such dir: ${d}`); process.exit(2); }
  const reportDir = path.resolve(reportDir0 || path.join(B, 'compare-report'));
  fs.rmSync(reportDir, { recursive: true, force: true });
  fs.mkdirSync(path.join(reportDir, 'img'), { recursive: true });
  const SA = states(A, reportDir), SB = states(B, reportDir);
  const keys = [...new Set([...SA.keys(), ...SB.keys()])].sort();

  const byKey = new Map(), todo = [];
  for (const key of keys) {
    const ea = SA.get(key) || new Set(), eb = SB.get(key) || new Set();
    if (identical(A, B, key, ea, eb)) byKey.set(key, { key, png: ea.has('.png') ? { pixels: 0 } : null, dom: ea.has('.dom.html.gz') ? { count: 0, lines: [] } : null, style: ea.has('.style.json.gz') ? { count: 0, lines: [] } : null, missing: [], bad: false });
    else todo.push({ key, ea: [...ea], eb: [...eb] });
  }
  const threads = Math.max(1, Math.min(todo.length, Number(process.env.COMPARE_THREADS || Math.min(4, os.availableParallelism()))));
  await Promise.all(Array.from({ length: threads }, () => new Promise((resolve, reject) => {
    const w = new Worker(new URL(import.meta.url), { workerData: { A, B, reportDir } });
    const next = () => { const job = todo.shift(); if (job) w.postMessage(job); else w.terminate().then(resolve); };
    w.on('message', r => { byKey.set(r.key, r); next(); });
    w.on('error', reject);
    next();
  })));
  const rows = keys.map(k => byKey.get(k));
  const failed = rows.filter(r => r.bad).length;

  // ---------------------------------------------------------------- report
  const esc = s => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
  const pngTxt = r => !r.png ? '–' : r.png.pixels < 0 ? `size ${r.png.size}` : `${r.png.pixels}`;
  const md = [`# e2e compare`, '', `A: \`${A}\`  `, `B: \`${B}\``, '',
    `States: ${rows.length}, with differences: ${failed}`, '',
    '| state | pixels | DOM lines | style diffs | missing |', '|---|---:|---:|---:|---|'];
  for (const r of rows.filter(r => r.bad)) md.push(`| ${r.key} | ${pngTxt(r)} | ${r.dom ? r.dom.count : '–'} | ${r.style ? r.style.count : '–'} | ${r.missing.join(', ')} |`);
  for (const r of rows.filter(r => r.bad)) {
    md.push('', `## ${r.key}`);
    if (r.png && r.png.img) md.push(`![diff](img/${r.png.img}.diff.png)`);
    if (r.dom && r.dom.count) md.push('', 'DOM:', '```diff', ...r.dom.lines, r.dom.count > r.dom.lines.length ? `… ${r.dom.count - r.dom.lines.length} more` : '', '```');
    if (r.style && r.style.count) md.push('', 'Style:', '```', ...r.style.lines, r.style.count > r.style.lines.length ? `… ${r.style.count - r.style.lines.length} more` : '', '```');
  }
  fs.writeFileSync(path.join(reportDir, 'report.md'), md.join('\n') + '\n');
  const html = [`<!doctype html><meta charset=utf-8><title>e2e compare</title><style>body{font:14px system-ui;margin:20px}table{border-collapse:collapse}td,th{border:1px solid #ccc;padding:3px 8px}pre{background:#f6f6f6;padding:8px;overflow:auto;max-height:400px}img{max-width:32%;border:1px solid #ccc;margin-right:4px;vertical-align:top}.ok{color:#080}.bad{color:#c00}</style>`,
    `<h1>e2e compare</h1><p>A: <code>${esc(A)}</code><br>B: <code>${esc(B)}</code></p>`,
    `<p class="${failed ? 'bad' : 'ok'}">States: ${rows.length}, with differences: ${failed}</p>`,
    '<table><tr><th>state</th><th>pixels</th><th>DOM lines</th><th>style diffs</th><th>missing</th></tr>',
    ...rows.filter(r => r.bad).map(r => `<tr><td><a href="#${esc(r.key)}">${esc(r.key)}</a></td><td>${esc(pngTxt(r))}</td><td>${r.dom ? r.dom.count : '–'}</td><td>${r.style ? r.style.count : '–'}</td><td>${esc(r.missing.join(', '))}</td></tr>`), '</table>'];
  for (const r of rows.filter(r => r.bad)) {
    html.push(`<h2 id="${esc(r.key)}">${esc(r.key)}</h2>`);
    if (r.png && r.png.img) html.push(['a', 'b', 'diff'].map(t => `<img src="img/${esc(r.png.img)}.${t}.png" alt="${t}">`).join(''));
    if (r.dom && r.dom.count) html.push(`<h3>DOM (${r.dom.count})</h3><pre>${esc(r.dom.lines.join('\n'))}</pre>`);
    if (r.style && r.style.count) html.push(`<h3>Style (${r.style.count})</h3><pre>${esc(r.style.lines.join('\n'))}</pre>`);
  }
  fs.writeFileSync(path.join(reportDir, 'report.html'), html.join('\n'));

  for (const r of rows.filter(r => r.bad)) console.log(`DIFF ${r.key}: pixels=${pngTxt(r)} dom=${r.dom ? r.dom.count : '–'} style=${r.style ? r.style.count : '–'}${r.missing.length ? ' ' + r.missing.join(', ') : ''}`);
  console.log(`${rows.length} states compared, ${failed} with differences; report: ${path.join(reportDir, 'report.md')}`);
  process.exit(failed ? 1 : 0);
}
