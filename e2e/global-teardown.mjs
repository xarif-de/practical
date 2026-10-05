// Stops exactly the servers global-setup.mjs started (RUN_DIR/pids): SIGINT, then SIGKILL after 5 s.
import fs from 'node:fs';
import path from 'node:path';
import { RUN_DIR } from './lib/env.mjs';

const alive = pid => { try { process.kill(pid, 0); return true; } catch { return false; } };

export default async function globalTeardown() {
  const f = path.join(RUN_DIR, 'pids');
  if (!fs.existsSync(f)) return;
  const pids = fs.readFileSync(f, 'utf8').split('\n').filter(Boolean).map(Number);
  for (const pid of pids) if (alive(pid)) process.kill(pid, 'SIGINT');
  for (let i = 0; i < 50 && pids.some(alive); i++) await new Promise(r => setTimeout(r, 100));
  for (const pid of pids) if (alive(pid)) process.kill(pid, 'SIGKILL');
  fs.rmSync(f);
}
