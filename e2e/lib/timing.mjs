// E2E_TIMING=1: durations of the harness' own phases (settle, screenshots, DOM/style dump, file writes, scrolled
// segments) as JSON lines in .work/timing-<E2E_PORT>/<pid>.jsonl. Measurement only: nothing captured depends on it.
import fs from 'node:fs';
import path from 'node:path';
import { FIRST_PORT, WORK } from './env.mjs';

const dir = process.env.E2E_TIMING === '1' ? path.join(WORK, `timing-${FIRST_PORT}`) : null;
let file = null;
export const clock = () => performance.now();
export function mark(rec) {
  if (!dir) return;
  if (!file) { fs.mkdirSync(dir, { recursive: true }); file = path.join(dir, `${process.pid}.jsonl`); }
  fs.appendFileSync(file, JSON.stringify(rec) + '\n');
}
