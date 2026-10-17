// The server pair of a run: `serve` of this checkout's build on fresh COPIES of the fixture DB —
//  - dev (port): development mode (DEV_ASSETS_DIR = the minified assets this build embeds, from build-server.sh;
//    E2E_ASSETS=source serves build/assets as written instead), DEV_LOGIN=alice, DEV_NOW = NOW — every test;
//  - login (port + 1): no development mode, password login, PRACTICAL__UPLOADER — the login page, the password login
//    test and the uploader's.
// Egress of both is blocked (HTTP(S)_PROXY → 127.0.0.1:9). Every PID started is listed in <runDir>/pids until it
// is stopped, so global-teardown.mjs can stop what a crashed worker left behind (and nothing else).
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { NOW, POLL_MS, REPO } from './env.mjs';

const BLOCK = 'http://127.0.0.1:9';
const alive = pid => { try { process.kill(pid, 0); return true; } catch { return false; } };

function freshDb(runDir, name, fixture) {
  const db = path.join(runDir, name);
  for (const s of ['', '-wal', '-shm']) fs.rmSync(db + s, { force: true });
  fs.copyFileSync(fixture, db);
  return db;
}

async function waitHealthy(base, child, log) {
  for (let i = 0; i < 1500; i++) {
    if (child.exitCode !== null) throw new Error(`server ${base} exited with ${child.exitCode}, see ${log}`);
    try {
      const r = await fetch(base + '/healthz');
      if (r.ok && (await r.text()) === 'ok') return;
    } catch {}
    await new Promise(r => setTimeout(r, 20));
  }
  throw new Error(`server ${base} did not become healthy, see ${log}`);
}

function start(bin, runDir, port, db, dev, tag) {
  const log = path.join(runDir, `${tag}.log`);
  const env = {
    PATH: process.env.PATH, HOME: process.env.HOME, RUST_LOG: 'info,tower_http=warn',
    PRACTICAL__DB_PATH: db, PRACTICAL__RUSTICAL_URL: '', PRACTICAL__BIND: `127.0.0.1:${port}`,
    PRACTICAL__PUBLIC_URL: `http://127.0.0.1:${port}`, PRACTICAL__ALLOW_PASSWORD_LOGIN: 'true',
    PRACTICAL__HOLIDAYS_COUNTRY: 'DE',
    PRACTICAL__PUBLIC_DAV_URL: 'https://dav.example.org', PRACTICAL__DEFAULT_TIMEZONE: 'Europe/Berlin',
    PRACTICAL__DEFAULT_LANG: 'de',
    HTTP_PROXY: BLOCK, HTTPS_PROXY: BLOCK, http_proxy: BLOCK, https_proxy: BLOCK, ALL_PROXY: BLOCK, NO_PROXY: '', no_proxy: '',
  };
  // PractiCalUploader (PRACTICAL__UPLOADER) only on the login server: the dev server's pages, which the visual and a11y
  // states show, stay as they are without it; functional/uploader.spec.mjs covers both
  if (!dev) env.PRACTICAL__UPLOADER = 'true';
  if (dev) Object.assign(env, { PRACTICAL__DEV_ASSETS_DIR: process.env.E2E_ASSETS === 'source' ? path.join(REPO, 'build', 'assets') : `${bin}.assets`, PRACTICAL__DEV_LOGIN: 'alice', PRACTICAL__DEV_NOW: NOW, PRACTICAL__DEV_POLL_MS: String(POLL_MS) });
  const out = fs.openSync(log, 'w');
  const child = spawn(bin, ['serve'], { env, stdio: ['ignore', out, out] });
  fs.closeSync(out);
  fs.appendFileSync(path.join(runDir, 'pids'), `${child.pid}\n`);
  return { child, log, base: `http://127.0.0.1:${port}` };
}

/** Stops the PIDs listed in <runDir>/pids (SIGINT, SIGKILL after 5 s) and removes the list. */
export async function stopPids(runDir) {
  const f = path.join(runDir, 'pids');
  if (!fs.existsSync(f)) return;
  const pids = fs.readFileSync(f, 'utf8').split('\n').filter(Boolean).map(Number);
  for (const pid of pids) if (alive(pid)) process.kill(pid, 'SIGINT');
  for (let i = 0; i < 250 && pids.some(alive); i++) await new Promise(r => setTimeout(r, 20));
  for (const pid of pids) if (alive(pid)) process.kill(pid, 'SIGKILL');
  fs.rmSync(f);
}

/** Starts the pair on `port`/`port + 1` with fresh DB copies in `runDir`; resolves once both answer /healthz and the
 *  dev server proved it is this checkout's build. Returns `stop()`. */
export async function startPair({ bin, runDir, port, fixture }) {
  fs.mkdirSync(runDir, { recursive: true });
  const a = start(bin, runDir, port, freshDb(runDir, 'db.sqlite3', fixture), true, 'server');
  const b = start(bin, runDir, port + 1, freshDb(runDir, 'login.sqlite3', fixture), false, 'login-server');
  const stop = async () => {
    for (const s of [a, b]) if (s.child.exitCode === null) s.child.kill('SIGINT');
    const exited = s => s.child.exitCode !== null || s.child.signalCode !== null;
    for (let i = 0; i < 250 && ![a, b].every(exited); i++) await new Promise(r => setTimeout(r, 20));
    for (const s of [a, b]) if (!exited(s)) s.child.kill('SIGKILL');
    await Promise.all([a, b].map(s => exited(s) ? null : new Promise(r => s.child.once('exit', r))));
    fs.rmSync(path.join(runDir, 'pids'), { force: true });
  };
  try {
    await Promise.all([waitHealthy(a.base, a.child, a.log), waitHealthy(b.base, b.child, b.log)]);
    // the binary must be this checkout's: its dev server logs the fixed clock (clock.rs)
    if (!fs.readFileSync(a.log, 'utf8').includes('development clock')) throw new Error(`${a.log}: no "development clock" line — the server binary ignores PRACTICAL__DEV_NOW (wrong build?)`);
  } catch (e) {
    await stop();
    throw e;
  }
  return { stop };
}
