// Builds this checkout's server (release), starts it twice on fresh COPIES of the fixture DB:
//  - BASE: development mode (DEV_ASSETS_DIR = the minified assets this build embeds, from
//    build-server.sh; E2E_ASSETS=source serves build/assets as written instead), DEV_LOGIN=alice,
//    DEV_NOW = NOW) — every test;
//  - LOGIN_BASE: no development mode, password login — the login page and the password login test.
// Egress of both is blocked (HTTP(S)_PROXY → 127.0.0.1:9). Only the PIDs started here are stopped
// (global-teardown.mjs, from RUN_DIR/pids).
import { execFileSync, spawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { BASE, E2E, FIXTURE, LOGIN_BASE, LOGIN_PORT, NOW, PORT, REPO, RUN_DIR } from './lib/env.mjs';

const BLOCK = 'http://127.0.0.1:9';

function freshDb(name) {
  const db = path.join(RUN_DIR, name);
  for (const s of ['', '-wal', '-shm']) fs.rmSync(db + s, { force: true });
  fs.copyFileSync(FIXTURE, db);
  return db;
}

async function waitHealthy(base, child, log) {
  for (let i = 0; i < 300; i++) {
    if (child.exitCode !== null) throw new Error(`server ${base} exited with ${child.exitCode}, see ${log}`);
    try {
      const r = await fetch(base + '/healthz');
      if (r.ok && (await r.text()) === 'ok') return;
    } catch {}
    await new Promise(r => setTimeout(r, 100));
  }
  throw new Error(`server ${base} did not become healthy, see ${log}`);
}

function start(bin, port, db, dev, tag) {
  const log = path.join(RUN_DIR, `${tag}.log`);
  const env = {
    PATH: process.env.PATH, HOME: process.env.HOME, RUST_LOG: 'info,tower_http=warn',
    PRACTICAL__DB_PATH: db, PRACTICAL__RUSTICAL_URL: '', PRACTICAL__BIND: `127.0.0.1:${port}`,
    PRACTICAL__PUBLIC_URL: `http://127.0.0.1:${port}`, PRACTICAL__ALLOW_PASSWORD_LOGIN: 'true',
    PRACTICAL__HOLIDAYS_COUNTRY: 'DE', PRACTICAL__HOLIDAYS_SUBDIVISION: 'DE-SN',
    PRACTICAL__PUBLIC_DAV_URL: 'https://dav.example.org', PRACTICAL__DEFAULT_TIMEZONE: 'Europe/Berlin',
    PRACTICAL__DEFAULT_LANG: 'de',
    HTTP_PROXY: BLOCK, HTTPS_PROXY: BLOCK, http_proxy: BLOCK, https_proxy: BLOCK, ALL_PROXY: BLOCK, NO_PROXY: '', no_proxy: '',
  };
  if (dev) Object.assign(env, { PRACTICAL__DEV_ASSETS_DIR: process.env.E2E_ASSETS === 'source' ? path.join(REPO, 'build', 'assets') : `${bin}.assets`, PRACTICAL__DEV_LOGIN: 'alice', PRACTICAL__DEV_NOW: NOW });
  const out = fs.openSync(log, 'w');
  const child = spawn(bin, ['serve'], { env, stdio: ['ignore', out, out] });
  fs.appendFileSync(path.join(RUN_DIR, 'pids'), `${child.pid}\n`);
  return { child, log };
}

export default async function globalSetup() {
  if (!fs.existsSync(FIXTURE)) throw new Error(`no fixture at ${FIXTURE} — run: mise run e2e-seed`);
  fs.mkdirSync(RUN_DIR, { recursive: true });
  if (fs.existsSync(path.join(RUN_DIR, 'pids'))) throw new Error(`${RUN_DIR}/pids exists: a run on port ${PORT} is active or was not torn down (stop those PIDs, delete the file)`);
  const bin = path.join(RUN_DIR, 'practical');
  if (!process.env.E2E_SKIP_BUILD || !fs.existsSync(bin) || !fs.existsSync(`${bin}.assets`)) {
    const t0 = Date.now();
    execFileSync(path.join(E2E, 'build-server.sh'), [bin], { stdio: 'inherit' });
    console.log(`[e2e] server built in ${((Date.now() - t0) / 1000).toFixed(1)} s`);
  }
  const rev = execFileSync('git', ['-C', REPO, 'rev-parse', '--short', 'HEAD']).toString().trim();
  const dirty = execFileSync('git', ['-C', REPO, 'status', '--porcelain', '--', 'build']).toString().trim() !== '';
  fs.writeFileSync(path.join(RUN_DIR, 'rev'), `${rev}${dirty ? '+dirty' : ''}\n`);
  const a = start(bin, PORT, freshDb('db.sqlite3'), true, 'server');
  const b = start(bin, LOGIN_PORT, freshDb('login.sqlite3'), false, 'login-server');
  try {
    await Promise.all([waitHealthy(BASE, a.child, a.log), waitHealthy(LOGIN_BASE, b.child, b.log)]);
  } catch (e) {
    for (const c of [a.child, b.child]) if (c.exitCode === null) c.kill('SIGKILL');
    fs.rmSync(path.join(RUN_DIR, 'pids'), { force: true });
    throw e;
  }
  // the binary must be this checkout's: its dev server logs the fixed clock (clock.rs)
  if (!fs.readFileSync(a.log, 'utf8').includes('development clock')) {
    for (const c of [a.child, b.child]) c.kill('SIGKILL');
    fs.rmSync(path.join(RUN_DIR, 'pids'), { force: true });
    throw new Error(`${a.log}: no "development clock" line — the server binary ignores PRACTICAL__DEV_NOW (wrong build?)`);
  }
  console.log(`[e2e] ${rev}${dirty ? '+dirty' : ''}: dev server ${BASE}, login server ${LOGIN_BASE}`);
}
