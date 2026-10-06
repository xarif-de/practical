// Builds this checkout's server (build-server.sh; E2E_SKIP_BUILD=1 keeps an existing BIN) and, for the visual and
// perf suites, starts the one server pair all their workers share (lib/servers.mjs). The functional suite's workers
// start pairs of their own (lib/test.mjs). Only the PIDs started for this run are stopped (global-teardown.mjs).
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { BASE, BIN, E2E, FIRST_PORT, FIXTURE, LOGIN_BASE, PER_WORKER, REPO, RUN_DIR, WORK } from './lib/env.mjs';
import { startPair } from './lib/servers.mjs';

/** The run dirs this run may use: one per worker slot (functional) or the shared pair's. */
export const runDirs = config => (PER_WORKER ? [...Array(config.workers).keys()] : [0]).map(i => path.join(WORK, `run-${FIRST_PORT + 2 * i}`));

export default async function globalSetup(config) {
  if (!fs.existsSync(FIXTURE)) throw new Error(`no fixture at ${FIXTURE} — run: mise run e2e-seed`);
  for (const d of runDirs(config)) if (fs.existsSync(path.join(d, 'pids'))) throw new Error(`${d}/pids exists: a run on port ${FIRST_PORT} is active or was not torn down (stop those PIDs, delete the file)`);
  fs.mkdirSync(path.dirname(BIN), { recursive: true });
  if (!process.env.E2E_SKIP_BUILD || !fs.existsSync(BIN) || !fs.existsSync(`${BIN}.assets`)) {
    const t0 = Date.now();
    execFileSync(path.join(E2E, 'build-server.sh'), [BIN], { stdio: 'inherit' });
    console.log(`[e2e] server built in ${((Date.now() - t0) / 1000).toFixed(1)} s`);
  }
  const rev = execFileSync('git', ['-C', REPO, 'rev-parse', '--short', 'HEAD']).toString().trim();
  const dirty = execFileSync('git', ['-C', REPO, 'status', '--porcelain', '--', 'build']).toString().trim() !== '';
  fs.writeFileSync(path.join(path.dirname(BIN), 'rev'), `${rev}${dirty ? '+dirty' : ''}\n`);
  if (PER_WORKER) {
    console.log(`[e2e] ${rev}${dirty ? '+dirty' : ''}: ${config.workers} worker(s), each with its own servers on ports ${FIRST_PORT}…${FIRST_PORT + 2 * config.workers - 1}`);
    return;
  }
  await startPair({ bin: BIN, runDir: RUN_DIR, port: FIRST_PORT, fixture: FIXTURE });
  console.log(`[e2e] ${rev}${dirty ? '+dirty' : ''}: dev server ${BASE}, login server ${LOGIN_BASE}`);
}
