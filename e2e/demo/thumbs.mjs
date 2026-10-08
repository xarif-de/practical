// Gallery thumbnails for the site (site/build.mjs): docs/images/thumbs/NN-*.webp, 240 px wide (the strip shows them
// at 120 CSS px, 2× for HiDPI), q 70, one per shots.json entry; thumbs of shots that no longer exist are removed.
// Committed, not made by the Pages job: the CI image has no cwebp, and the full pictures as thumbnails would put all
// 34 (2.3 MB, 2026-10-08) on the start page instead of ~0.2 MB.
// Usage: node e2e/demo/thumbs.mjs (run by `mise run readme-shots` after shots.mjs; cwebp from Homebrew or PATH).
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { E2E, REPO } from '../lib/env.mjs';

const IMG = path.join(REPO, 'docs', 'images'), OUT = path.join(IMG, 'thumbs');
const cwebp = fs.existsSync('/opt/homebrew/bin/cwebp') ? '/opt/homebrew/bin/cwebp' : 'cwebp';
const shots = JSON.parse(fs.readFileSync(path.join(E2E, 'demo', 'shots.json'), 'utf8'));
fs.mkdirSync(OUT, { recursive: true });
const want = new Set(shots.map(s => s.file));
for (const f of fs.readdirSync(OUT)) if (!want.has(f)) fs.rmSync(path.join(OUT, f));
let bytes = 0;
for (const { file } of shots) {
  const src = path.join(IMG, file), dst = path.join(OUT, file);
  if (!fs.existsSync(src)) throw new Error(`thumbs: ${src} missing (run mise run readme-shots)`);
  execFileSync(cwebp, ['-quiet', '-q', '70', '-resize', '240', '0', '-m', '6', '-sharp_yuv', src, '-o', dst]);
  bytes += fs.statSync(dst).size;
}
console.log(`thumbs: ${shots.length} in docs/images/thumbs, ${(bytes / 1024).toFixed(0)} KB`);
