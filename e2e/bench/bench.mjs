#!/usr/bin/env node
// Performance regression bench (docs/MAP.md "bench"): a small set of key metrics of this checkout's server
// build on a large synthetic DB; with --ref, A/B against a reference build, interleaved on this machine in this run.
//   node e2e/bench/bench.mjs [--out <dir>]              measure this checkout → <dir>/result.json, table.txt, the binary
//   node e2e/bench/bench.mjs --ref <dir> [--out <dir>]  A/B against <dir> (a former --out; default out bench-<sha>-vs-<ref sha>); exit 1 on a regression,
//                                                       4 when the power state changed during the run (no verdict)
// The verdict is never drawn from a stored number of another run (the clock speed differs between power modes and
// even within a run): both binaries run in the same session, alternating per round/sample, and every time metric
// is judged as the median of per-round cand/ref ratios. Stored results are for information (and the build-free
// source sizes). Next to wall time it counts what does not depend on the clock: instructions and CPU time per
// request, SQL statements per request, bytes, RSS/peak footprint, DOM nodes, requests per interaction.
// Env: E2E_PORT (uses PORT … PORT+2), E2E_FIXTURE, E2E_PROFILE (cargo profile, default e2e), BENCH_ROUNDS (5),
// BENCH_UI_SAMPLES (5), BENCH_IDLE_S (60), BENCH_SKIP_BUILD=1 (reuse this run dir's build), BENCH_PHASES
// (server,count,idle,client: a subset for a quick look).
import { execFileSync, spawn } from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import readline from 'node:readline';
import zlib from 'node:zlib';
import { E2E, FIXTURE, REPO } from '../lib/env.mjs';
import { runClient } from './client.mjs';

const HERE = path.join(E2E, 'bench');
const args = process.argv.slice(2);
const opt = k => { const i = args.indexOf(k); return i >= 0 ? args[i + 1] : undefined; };
const REF = opt('--ref');
const PORT = Number(process.env.E2E_PORT || 4101);
const ROUNDS = Number(process.env.BENCH_ROUNDS || 5);
const UI_SAMPLES = Number(process.env.BENCH_UI_SAMPLES || 5);
const IDLE_S = Number(process.env.BENCH_IDLE_S || 60);
const IDLE_WINDOWS = 5;
const PROFILE = process.env.E2E_PROFILE || 'e2e';
const PHASES = (process.env.BENCH_PHASES || 'server,count,idle,client').split(',');
const WORK = path.join(E2E, '.work', `bench-${PORT}`);
const CACHE = path.join(os.homedir(), '.cache', 'practical-e2e');
const BLOCK = 'http://127.0.0.1:9';
const T0 = Date.now();
let DB; // the cached bench DB (main)
const log = m => console.error(`[bench ${((Date.now() - T0) / 1000).toFixed(0).padStart(3)} s] ${m}`);

// ---------------------------------------------------------------- thresholds (calibration: docs/MAP.md "bench")
// alarm ⇔ ratio (cand/ref) > 1 + rel AND delta (cand − ref) > abs: small values and small relative moves stay quiet
const RULES = [
  [/^srv\.lat\..*\.p50$/, { rel: 0.15, abs: 2 }],
  [/^srv\.lat\..*\.p90$/, { rel: 0.30, abs: 5 }],
  [/^srv\.cpu\./, { rel: 0.15, abs: 2 }],
  [/^srv\.inst\./, { rel: 0.10, abs: 2 }],
  [/^srv\.first\./, { rel: 0.25, abs: 5 }],
  [/^srv\.sql\./, { rel: 0.10, abs: 2 }],
  [/^srv\.(rss|peak)\./, { rel: 0.10, abs: 2 }],
  [/^idle\.cpu$/, { rel: 0.50, abs: 0.2 }],
  [/^idle\.inst$/, { rel: 0.50, abs: 50 }],
  [/^idle\.wakeups$/, { rel: 0.50, abs: 20 }],
  [/^(wire|size)\./, { rel: 0.01, abs: 16 }],
  [/^dom\./, { rel: 0.02, abs: 20 }],
  [/^ui\..*\.reqs$/, { rel: 0.20, abs: 0.5 }],
  [/^ui\..*\.busy$/, { rel: 0.25, abs: 10 }],
  [/^ui\..*\.cpu4$/, { rel: 0.25, abs: 20 }],
  [/^ui\./, { rel: 0.20, abs: 10 }],
];
// size.binary is judged only between two builds of one non-incremental profile (release, dist): the e2e profile's
// 256 codegen units are partitioned per module, so moving code between modules alone moves its size (w4/rust's split
// of 10 modules into ~37: e2e +2.5 %, release −0.2 %). Otherwise it is information only.
let SIZE_JUDGED = false;
const rule = id => (id === 'size.binary' && !SIZE_JUDGED ? null : (RULES.find(([re]) => re.test(id)) || [null, null])[1]);
const UNITS = [[/^ui\..*\.reqs$/, 'req'], [/^srv\.(lat|first|cpu)\.|^ui\./, 'ms'], [/^srv\.inst\./, 'Minstr'], [/^srv\.sql\./, 'stmts'], [/^srv\.(rss|peak)\./, 'MB'],
  [/^idle\.cpu$/, 'mcores'], [/^idle\.inst$/, 'Minstr/min'], [/^idle\.wakeups$/, '/min'], [/^(wire|size)\./, 'B'], [/^dom\./, 'nodes']];
const unit = id => UNITS.find(([re]) => re.test(id))[1];

// ---------------------------------------------------------------- the workload
const DAY = '2026-10-14';
const LAT = [ // ~10 key endpoints: the pages + what a view switch / the search box / @-mentions fetch
  ['cal-month', `/calendar?view=month&date=${DAY}`],
  ['cal-month-next', `/calendar?view=month&date=${DAY}&month=2026-11&fragment=1`],
  ['cal-week', `/calendar?view=week&date=${DAY}`],
  ['cal-day', `/calendar?view=day&date=${DAY}`],
  ['cal-year', `/calendar?view=planner&date=${DAY}`],
  ['tasks', '/tasks'],
  ['contacts', '/contacts'],
  ['history', '/history'],
  ['settings', '/settings'],
  ['search', '/api/search?q=zahn'],
  ['mentions', '/api/mentions'],
];
const FIRST = [['cal-month', LAT[0][1]], ['search', '/api/search?q=zahn']]; // first hit after an edit
const WIRE_PAGES = [['cal-month', LAT[0][1]], ['cal-week', LAT[2][1]], ['tasks', '/tasks'], ['contacts', '/contacts'], ['history', '/history'], ['settings', '/settings'], ['login', '/login']];
const WIRE_ASSETS = ['app.js', 'style.css', 'i18n-de.js', 'icons.svg', 'theme.js', 'history.js', 'history.css'];
const EDIT = '/api/events/alice/5eed0000-0000-4000-8000-000000000001/review@seed'; // fixture event, edited before a first hit
const LAT_SAMPLES = 4; // per endpoint, side and round (after 2 warm-ups)
const ACCEPT_ENCODING = 'gzip, deflate, br, zstd'; // Chromium's

// ---------------------------------------------------------------- helpers
const median = a => { const s = [...a].sort((x, y) => x - y), n = s.length; return n ? (n % 2 ? s[(n - 1) / 2] : (s[n / 2 - 1] + s[n / 2]) / 2) : NaN; };
const quant = (a, q) => { const s = [...a].sort((x, y) => x - y); if (!s.length) return NaN; const p = (s.length - 1) * q, i = Math.floor(p); return s[i] + (s[Math.min(i + 1, s.length - 1)] - s[i]) * (p - i); };
const r3 = x => Math.round(x * 1000) / 1000;
const sha = (...bufs) => { const h = crypto.createHash('sha256'); for (const b of bufs) h.update(b); return h.digest('hex').slice(0, 12); };
const git = (...a) => execFileSync('git', ['-C', REPO, ...a]).toString().trim();
const sh = (cmd, a) => { try { return execFileSync(cmd, a, { stdio: ['ignore', 'pipe', 'ignore'] }).toString(); } catch { return ''; } };

/** The power state (macOS pmset): source, Low Power/High Power mode, thermal/performance limits. Battery % is
 *  recorded but is no change of state. null elsewhere. */
function power() {
  if (process.platform !== 'darwin') return null;
  const batt = sh('pmset', ['-g', 'batt']), g = sh('pmset', ['-g']), therm = sh('pmset', ['-g', 'therm']);
  const mode = (g.match(/^\s*(?:powermode|lowpowermode)\s+(\d+)/m) || [])[1];
  return {
    source: (batt.match(/drawing from '([^']+)'/) || [])[1] || '?',
    mode: mode === undefined ? '?' : { 0: 'automatic', 1: 'low power', 2: 'high power' }[mode] || mode,
    therm: therm.split('\n').map(l => l.trim()).filter(l => l && !l.startsWith('Note:')).join('; ') || 'no limits recorded',
    battery: (batt.match(/(\d+)%/) || [])[1] || null,
  };
}
const powerKey = p => (p ? `${p.source} | ${p.mode} | ${p.therm}` : 'n/a');

/** The proc.py co-process: [cpu_ns, rss, wakeups, instructions, peak footprint] per PID. */
function procSampler() {
  const p = spawn('python3', [path.join(HERE, 'proc.py')], { stdio: ['pipe', 'pipe', 'inherit'] });
  const rl = readline.createInterface({ input: p.stdout });
  const waiting = [];
  rl.on('line', l => waiting.shift()(JSON.parse(l)));
  return {
    sample: pids => new Promise(res => { waiting.push(res); p.stdin.write(pids.join(' ') + '\n'); }),
    close: () => p.stdin.end(),
  };
}

function request(side, method, p, { body, headers = {}, raw = false } = {}) {
  return new Promise((resolve, reject) => {
    const h = { Cookie: side.cookie || '', ...headers };
    if (!raw) h['Accept-Encoding'] = ACCEPT_ENCODING;
    if (body !== undefined) h['Content-Length'] = Buffer.byteLength(body);
    const t0 = performance.now();
    const req = http.request({ host: '127.0.0.1', port: side.port, method, path: p, headers: h, agent: side.agent }, res => {
      const chunks = [];
      res.on('data', c => chunks.push(c));
      res.on('end', () => { const buf = Buffer.concat(chunks); resolve({ status: res.statusCode, headers: res.headers, bytes: buf.length, buf, ms: performance.now() - t0 }); });
      res.on('error', reject);
    });
    req.on('error', reject);
    if (body !== undefined) req.write(body);
    req.end();
  });
}
const text = r => { const e = r.headers['content-encoding']; return (e === 'gzip' ? zlib.gunzipSync(r.buf) : e === 'br' ? zlib.brotliDecompressSync(r.buf) : r.buf).toString(); };
// every cookie the answer sets (the login form's token is a cookie of its own since w3/http; a Max-Age=0 one is a deletion)
const cookieOf = r => (r.headers['set-cookie'] || []).filter(c => !/;\s*max-age=0/i.test(c)).map(c => c.split(';')[0]).join('; ') || undefined;

async function get(side, p) {
  const r = await request(side, 'GET', p, { headers: { Accept: p.startsWith('/api/') ? '*/*' : 'text/html' } });
  if (r.status !== 200) throw new Error(`${side.name}: GET ${p} → ${r.status}`);
  return r;
}

async function login(side) {
  side.cookie = '';
  const a = await request(side, 'GET', '/login', { raw: true });
  side.cookie = cookieOf(a);
  const csrf = a.buf.toString().match(/name="csrf" value="([^"]+)"/)[1];
  const b = await request(side, 'POST', '/login', { raw: true, body: new URLSearchParams({ username: 'alice', password: 'devpass', csrf }).toString(), headers: { 'Content-Type': 'application/x-www-form-urlencoded', Origin: side.base } });
  side.cookie = cookieOf(b) || side.cookie;
  const s = text(await get(side, '/settings'));
  if (!s.includes('data-user="alice"')) throw new Error(`${side.name}: login as alice/devpass failed`);
  side.csrf = s.match(/<meta name="csrf" content="([^"]+)"/)[1];
}

async function edit(side) {
  const ev = JSON.parse(text(await get(side, EDIT))); // its version is the PUT's precondition
  side.edits = (side.edits || 0) + 1;
  const r = await request(side, 'PUT', EDIT, { body: JSON.stringify({ ...ev, summary: `Projekt-Review ${side.edits}` }), headers: { 'Content-Type': 'application/json', Accept: 'application/json', 'X-CSRF-Token': side.csrf, Origin: side.base } });
  if (r.status !== 200) throw new Error(`${side.name}: PUT ${EDIT} → ${r.status} ${text(r).slice(0, 200)}`);
}

function serverEnv(port, db, extra = {}) {
  return {
    PATH: process.env.PATH, HOME: process.env.HOME, RUST_LOG: 'warn', NO_COLOR: '1',
    PRACTICAL__DB_PATH: db, PRACTICAL__RUSTICAL_URL: '', PRACTICAL__BIND: `127.0.0.1:${port}`, PRACTICAL__PUBLIC_URL: `http://127.0.0.1:${port}`,
    PRACTICAL__ALLOW_PASSWORD_LOGIN: 'true', PRACTICAL__HOLIDAYS_COUNTRY: 'DE', PRACTICAL__HOLIDAYS_SUBDIVISION: 'DE-SN',
    PRACTICAL__PUBLIC_DAV_URL: 'https://dav.example.org', PRACTICAL__DEFAULT_TIMEZONE: 'Europe/Berlin', PRACTICAL__DEFAULT_LANG: 'de',
    // the real clock (development mode would serve other assets and poll them): no purge may change the DB as dates pass
    PRACTICAL__HISTORY_RETENTION_DAYS: '36500', PRACTICAL__TRASH_RETENTION_DAYS: '36500',
    HTTP_PROXY: BLOCK, HTTPS_PROXY: BLOCK, http_proxy: BLOCK, https_proxy: BLOCK, ALL_PROXY: BLOCK, NO_PROXY: '', no_proxy: '', ...extra,
  };
}

/** Starts every side on a fresh copy of the bench DB (prepare(db) may change the copy first; reuse: the copy the
 *  side's last start left); resolves once all answer /healthz and (settle) their CPU time stood still (start-up work done). */
async function startSides(sides, proc, { env = {}, prepare, tag = 'server', reuse = false, settle = true } = {}) {
  for (const s of sides) {
    const dir = path.join(WORK, s.name);
    fs.mkdirSync(dir, { recursive: true });
    const db = path.join(dir, 'db.sqlite3');
    if (!reuse) {
      for (const x of ['', '-wal', '-shm']) fs.rmSync(db + x, { force: true });
      fs.copyFileSync(DB, db, fs.constants.COPYFILE_FICLONE);
      if (prepare) prepare(db);
    }
    s.log = path.join(dir, `${tag}.log`);
    const out = fs.openSync(s.log, 'w');
    s.child = spawn(s.bin, ['serve'], { env: serverEnv(s.port, db, env), stdio: ['ignore', out, out] });
    fs.closeSync(out);
    s.agent = new http.Agent({ keepAlive: true, maxSockets: 4 });
    s.cookie = '';
  }
  for (const s of sides) {
    for (let i = 0; ; i++) {
      if (s.child.exitCode !== null) throw new Error(`${s.name} server exited ${s.child.exitCode}: ${s.log}`);
      if (i > 1500) throw new Error(`${s.name} server not healthy after 30 s`);
      try { if ((await request(s, 'GET', '/healthz', { raw: true })).status === 200) break; } catch {}
      await new Promise(r => setTimeout(r, 20)); // readiness probe
    }
  }
  if (!settle) return;
  // quiescence: < 2 ms CPU per side over the last 500 ms (100-ms probes), at most 20 s
  const hist = [];
  for (let i = 0; i < 200; i++) {
    const smp = await proc.sample(sides.map(s => s.child.pid));
    hist.push(sides.map(s => smp[s.child.pid][0]));
    if (hist.length > 5 && sides.every((_, j) => hist.at(-1)[j] - hist.at(-6)[j] < 2e6)) return;
    await new Promise(r => setTimeout(r, 100));
  }
  log('warning: servers not quiescent after 20 s');
}

async function stopSides(sides) {
  await Promise.all(sides.map(s => new Promise(res => {
    s.agent?.destroy();
    if (s.child.exitCode !== null || s.child.signalCode !== null) return res();
    const k = setTimeout(() => s.child.kill('SIGKILL'), 5000);
    s.child.once('exit', () => { clearTimeout(k); res(); });
    s.child.kill('SIGINT');
  })));
}

/** M[side][metric] = [[values of round 0], [values of round 1], …] */
const put = (m, id, round, v) => { const a = (m[id] ||= []); (a[round] ||= []).push(v); };
const order = (sides, i) => (i % 2 ? [...sides].reverse() : sides);
const pids = sides => sides.map(s => s.child.pid);

// ---------------------------------------------------------------- phases
async function serverPhase(sides, proc, M) {
  for (let round = 0; round < ROUNDS; round++) {
    const sd = order(sides, round);
    await startSides(sd, proc);
    let smp = await proc.sample(pids(sd));
    for (const s of sd) put(M[s.name], 'srv.rss.start', round, smp[s.child.pid][1] / 2 ** 20);
    for (const s of sd) await login(s);
    // first hit after an edit (the warm state first: one request each)
    for (const s of sd) for (const [, p] of FIRST) await get(s, p);
    for (let k = 0; k < 2; k++) for (const [name, p] of FIRST) for (const s of order(sd, k)) { await edit(s); put(M[s.name], `srv.first.${name}`, round, (await get(s, p)).ms); }
    // warm latency, sides alternating per sample; CPU time and instructions of each process over its samples
    for (const [name, p] of LAT) {
      for (const s of sd) for (let w = 0; w < 2; w++) await get(s, p);
      const a = await proc.sample(pids(sd));
      for (let j = 0; j < LAT_SAMPLES; j++) for (const s of order(sd, j)) put(M[s.name], `srv.lat.${name}`, round, (await get(s, p)).ms);
      const b = await proc.sample(pids(sd));
      for (const s of sd) {
        const [c0, , , i0] = a[s.child.pid], [c1, , , i1] = b[s.child.pid];
        put(M[s.name], `srv.cpu.${name}`, round, (c1 - c0) / 1e6 / LAT_SAMPLES);
        if (i0 !== null) put(M[s.name], `srv.inst.${name}`, round, (i1 - i0) / 1e6 / LAT_SAMPLES);
      }
    }
    // the fixed burst: every endpoint 3× concurrently
    for (const s of sd) for (const [, p] of LAT) await Promise.all([0, 1, 2].map(() => get(s, p)));
    smp = await proc.sample(pids(sd));
    for (const s of sd) {
      put(M[s.name], 'srv.rss.burst', round, smp[s.child.pid][1] / 2 ** 20);
      if (smp[s.child.pid][4] !== null) put(M[s.name], 'srv.peak.burst', round, smp[s.child.pid][4] / 2 ** 20);
    }
    await stopSides(sd);
    log(`server round ${round + 1}/${ROUNDS} done`);
  }
}

/** Clock-free counts on one start with sqlx' statement log on: SQL statements per warm request (the fewest of 3: the
 *  2-s change poller's statements can land in a request's window, never fewer), and the bytes on the wire. */
async function countPhase(sides, proc, M) {
  await startSides(sides, proc, { env: { RUST_LOG: 'warn,sqlx::query=debug' }, tag: 'count' });
  try {
    for (const s of sides) {
      await login(s);
      const lines = () => fs.readFileSync(s.log, 'utf8').split('\n').filter(l => l.includes('sqlx::query')).length;
      for (const [name, p] of LAT) {
        for (let w = 0; w < 2; w++) await get(s, p);
        const n = [];
        for (let k = 0; k < 3; k++) { const a = lines(); await get(s, p); n.push(lines() - a); }
        put(M[s.name], `srv.sql.${name}`, 0, Math.min(...n));
      }
      for (const [name, p] of WIRE_PAGES) put(M[s.name], `wire.page.${name}`, 0, (await request(s, 'GET', p, { headers: { Accept: 'text/html', ...(name === 'login' ? { Cookie: '' } : {}) } })).bytes);
      const html = text(await get(s, LAT[0][1]));
      const urls = Object.fromEntries([...html.matchAll(/(?:src|href|content)="(\/assets\/([^"?#]+)[^"]*)"/g)].map(m => [m[2], m[1].replaceAll('&amp;', '&')]));
      for (const a of WIRE_ASSETS) {
        const r = await request(s, 'GET', urls[a] || `/assets/${a}`);
        if (r.status !== 200) throw new Error(`${s.name}: GET ${urls[a]} → ${r.status}`);
        put(M[s.name], `wire.asset.${a}`, 0, r.bytes);
        s.encodings = { ...(s.encodings || {}), [a]: r.headers['content-encoding'] || 'identity' };
      }
    }
  } finally {
    await stopSides(sides);
  }
}

/** Both sides at once (the same CPU state by construction) for IDLE_S, sampled in IDLE_WINDOWS windows: the verdict
 *  is the ratio of the totals (the reminder loop ticks once a minute, in one window only).
 *  The login runs on a start of its own: a login commits to practical_sessions, and in the measured process that
 *  commit cost one change-poller round (next 2-s tick) and one reminder-plan rebuild (its next tick, start + 60 s,
 *  within the window; w4 trace: 3.2 and 7.7 Minstr in 10-s windows that otherwise hold ~1). The measured process reads the stored session and writes
 *  nothing (the SSE request is no page view, sessions::activity). Statement log over the window (2026-10-07):
 *  31 × `PRAGMA data_version` and nothing else per side. */
async function idlePhase(sides, proc, M) {
  const pem = execFileSync(sides.at(-1).bin, ['gen-vapid']).toString().match(/-----BEGIN PRIVATE KEY-----[\s\S]*?-----END PRIVATE KEY-----/)[0];
  // one push subscriber the reminder loop accepts (allowlisted host, valid keys; since w3/push a malformed one is
  // skipped and would leave the loop idle) whose name does not resolve: it does its per-minute work as in production
  const prepare = db => execFileSync('sqlite3', [db, "INSERT INTO practical_push_subscriptions (id, principal, endpoint, p256dh, auth) VALUES ('bench1', 'alice', 'https://bench.invalid.push.apple.com/push', 'BLMbF9ffKBiWQLCKvTHb6LO8Nb6dcUh6TItC455vu2kElga6PQvUmaFyCdykxY2nOSSL3yKgfbmFLRTUaGv4yV8', 'xS03Fi5ErfTNH_l9WHE9Ig')"]);
  const env = { PRACTICAL__VAPID_PRIVATE_KEY_PEM: pem };
  await startSides(sides, proc, { env, prepare, tag: 'idle-login', settle: false });
  try {
    for (const s of sides) await login(s);
  } finally {
    await stopSides(sides);
  }
  const sessions = sides.map(s => s.cookie);
  await startSides(sides, proc, { env, tag: 'idle', reuse: true });
  sides.forEach((s, i) => { s.cookie = sessions[i]; });
  const streams = [];
  try {
    for (const s of sides) {
      await new Promise((res, rej) => {
        const req = http.request({ host: '127.0.0.1', port: s.port, path: '/api/changes', headers: { Cookie: s.cookie, Accept: 'text/event-stream' } }, r => { r.on('data', () => {}); r.statusCode === 200 ? res() : rej(new Error(`${s.name}: SSE ${r.statusCode}`)); });
        req.on('error', e => (streams.closing ? null : rej(e)));
        streams.push(req);
        req.end();
      });
    }
    log(`idle window ${IDLE_S} s (one SSE client per server, VAPID + 1 push subscriber)`);
    let a = await proc.sample(pids(sides));
    for (let w = 0; w < IDLE_WINDOWS; w++) {
      await new Promise(r => setTimeout(r, IDLE_S * 1000 / IDLE_WINDOWS)); // the measurement window itself
      const b = await proc.sample(pids(sides));
      for (const s of sides) {
        const [c0, , w0, i0] = a[s.child.pid], [c1, , w1, i1] = b[s.child.pid];
        put(M[s.name], 'idle.cpu', w, (c1 - c0) / 1e6 / (IDLE_S / IDLE_WINDOWS)); // ms per s = mcores
        if (w0 !== null) put(M[s.name], 'idle.wakeups', w, (w1 - w0) * 60 / (IDLE_S / IDLE_WINDOWS));
        if (i0 !== null) put(M[s.name], 'idle.inst', w, (i1 - i0) / 1e6 * 60 / (IDLE_S / IDLE_WINDOWS));
      }
      a = b;
    }
  } finally {
    streams.closing = true;
    for (const r of streams) r.destroy();
    await stopSides(sides);
  }
}

async function clientPhase(sides, proc, M) {
  await startSides(sides, proc, { tag: 'client' });
  try {
    const res = await runClient(sides.map(s => ({ name: s.name, base: s.base })), UI_SAMPLES, log);
    for (const s of sides) Object.assign(M[s.name], res[s.name]);
  } finally {
    await stopSides(sides);
  }
}

function staticSizes(bin, m) {
  m['size.binary'] = [[fs.statSync(bin).size]];
  let total = 0;
  const dir = path.join(REPO, 'build', 'assets'), src = {};
  for (const f of fs.readdirSync(dir, { recursive: true }).sort()) {
    const st = fs.statSync(path.join(dir, f));
    if (!st.isFile()) continue;
    total += st.size;
    // app.js and style.css are joined from their parts in js/ and css/ (build/src/bundle.rs)
    const name = { js: 'app.js', css: 'style.css' }[f.split(path.sep)[0]] || f;
    if (/^(app\.js|style\.css|history\.js|history\.css|icons\.svg)$/.test(name)) src[name] = (src[name] || 0) + st.size;
  }
  for (const [name, n] of Object.entries(src)) m[`size.src.${name}`] = [[n]];
  m['size.src.total'] = [[total]];
}

// ---------------------------------------------------------------- summary, comparison, output
const TOTAL = /^idle\./; // judged on the ratio of the totals over all windows
const POOLED = /\.p90$/; // judged on the pooled samples (a p90 of 4 samples per round would be their maximum)

/** Per side and metric: the pooled value (median, p90 for latency; idle: the mean rate), sample count, spread (IQR/median). */
function summarize(raw) {
  const s = {};
  for (const [id, rounds] of Object.entries(raw)) {
    const v = rounds.flat();
    const med = median(v), spread = v.length > 2 && med ? r3((quant(v, 0.75) - quant(v, 0.25)) / med) : undefined;
    if (id.startsWith('srv.lat.')) {
      s[id + '.p50'] = { value: r3(med), n: v.length, spread };
      s[id + '.p90'] = { value: r3(quant(v, 0.9)), n: v.length };
    } else s[id] = { value: r3(TOTAL.test(id) ? v.reduce((x, y) => x + y, 0) / v.length : med), n: v.length, spread };
  }
  return s;
}

/** cand vs ref per metric: the median of the per-round ratios and differences (each side's median of that round), so
 *  a change of the CPU's speed between rounds hits both sides of a round alike. noise = IQR of those ratios. */
function compare(rawRef, rawCand, sumRef, sumCand) {
  const rows = [];
  for (const id of Object.keys(sumCand).filter(k => k in sumRef).sort()) {
    const base = id.replace(/\.p(50|90)$/, '');
    const A = rawRef[base], B = rawCand[base];
    let ratio, delta, noise;
    if (POOLED.test(id) || TOTAL.test(id) || !A || !B) {
      ratio = sumRef[id].value ? sumCand[id].value / sumRef[id].value : 1; delta = sumCand[id].value - sumRef[id].value;
      if (TOTAL.test(id) && A && B) { const q = A.map((a, i) => (median(a) ? median(B[i]) / median(a) : 1)); noise = quant(q, 0.75) - quant(q, 0.25); }
    } else {
      const n = Math.min(A.length, B.length), q = [], d = [];
      for (let i = 0; i < n; i++) { const a = median(A[i]), b = median(B[i]); q.push(a ? b / a : 1); d.push(b - a); }
      ratio = median(q); delta = median(d); noise = q.length > 2 ? quant(q, 0.75) - quant(q, 0.25) : undefined;
    }
    const rl = rule(id), alarm = !!rl && ratio > 1 + rl.rel && delta > rl.abs;
    rows.push({ id, unit: unit(id), ref: sumRef[id].value, cand: sumCand[id].value, ratio: r3(ratio), delta: r3(delta), noise: noise === undefined ? undefined : r3(noise), rule: rl ? `×${(1 + rl.rel).toFixed(2)} & +${rl.abs}` : 'report', alarm });
  }
  return rows;
}

function table(rows, single) {
  const fmt = (x, u) => (x === undefined || x === null || Number.isNaN(x) ? '' : ['B', 'nodes', 'stmts'].includes(u) ? String(Math.round(x)) : u === 'mcores' ? x.toFixed(3) : x.toFixed(1));
  const head = single ? ['metric', 'unit', 'value', 'spread'] : ['metric', 'unit', 'ref', 'cand', 'ratio', 'Δ', 'noise', 'rule', ''];
  const body = rows.map(r => single
    ? [r.id, r.unit, fmt(r.cand, r.unit), r.spread ? `${Math.round(r.spread * 100)}%` : '']
    : [r.id, r.unit, fmt(r.ref, r.unit), fmt(r.cand, r.unit), r.ratio.toFixed(3), fmt(r.delta, r.unit), r.noise === undefined ? '' : `±${(r.noise * 50).toFixed(1)}%`, r.rule, r.alarm ? 'REGRESSION' : '']);
  const w = head.map((h, i) => Math.max(h.length, ...body.map(b => b[i].length)));
  return [head, w.map(n => '-'.repeat(n)), ...body].map(r => r.map((c, i) => (i >= 2 && i <= 6 ? c.padStart(w[i]) : c.padEnd(w[i]))).join('  ').trimEnd()).join('\n');
}

// ---------------------------------------------------------------- main
async function main() {
  if (!fs.existsSync(FIXTURE)) throw new Error(`no fixture at ${FIXTURE} — run: mise run e2e-seed (or set E2E_FIXTURE)`);
  const pw = { start: power(), phases: {} };
  if (pw.start) log(`power: ${powerKey(pw.start)}${pw.start.battery ? `, battery ${pw.start.battery}%` : ''}`);
  if (pw.start?.mode === 'low power') log('warning: Low Power Mode — the A/B verdict holds (both sides alike), absolute times are slower and noisier');
  let refMeta = null;
  if (REF) {
    refMeta = JSON.parse(fs.readFileSync(path.join(REF, 'result.json'), 'utf8'));
    if (!fs.existsSync(path.join(REF, 'practical'))) throw new Error(`${REF}/practical: no reference binary`);
    if (refMeta.profile !== PROFILE) log(`warning: reference built with profile ${refMeta.profile}, this run ${PROFILE}`);
    SIZE_JUDGED = PROFILE !== 'e2e' && refMeta.profile === PROFILE;
  }
  const rev = git('rev-parse', '--short', 'HEAD') + (git('status', '--porcelain', '--', 'build') ? '+dirty' : '');
  const OUT = opt('--out') || path.join(CACHE, REF ? `bench-${rev}-vs-${refMeta.rev}` : `bench-${rev}`); // never the reference's dir
  fs.mkdirSync(WORK, { recursive: true });
  const bin = path.join(WORK, 'practical');
  if (!(process.env.BENCH_SKIP_BUILD && fs.existsSync(bin))) {
    log(`building this checkout (${PROFILE} profile)`);
    execFileSync(path.join(E2E, 'build-server.sh'), [bin], { stdio: ['ignore', 'inherit', 'inherit'], env: { ...process.env, E2E_PROFILE: PROFILE } });
  }
  const sides = [...(REF ? [{ name: 'ref', bin: path.join(REF, 'practical'), rev: refMeta.rev }] : []), { name: 'cand', bin, rev }];
  sides.forEach((s, i) => { s.port = PORT + i; s.base = `http://127.0.0.1:${s.port}`; });
  // the bench DB: generated once per (generator, fixture), cached; made by the oldest binary of the run
  DB = path.join(CACHE, `bench-db-${sha(fs.readFileSync(path.join(HERE, 'gen-db.py')), fs.readFileSync(FIXTURE))}.sqlite3`);
  if (!fs.existsSync(DB)) {
    log(`generating the bench DB → ${DB}`);
    fs.mkdirSync(CACHE, { recursive: true });
    execFileSync('python3', [path.join(HERE, 'gen-db.py'), sides[0].bin, FIXTURE, DB, String(PORT + 2)], { stdio: 'inherit' });
  }
  const proc = procSampler();
  const M = Object.fromEntries(sides.map(s => [s.name, {}]));
  const cleanup = () => { for (const s of sides) if (s.child && s.child.exitCode === null) s.child.kill('SIGKILL'); };
  process.on('exit', cleanup);
  for (const sig of ['SIGINT', 'SIGTERM']) process.on(sig, () => { cleanup(); process.exit(130); });
  try {
    for (const [name, fn] of [['server', serverPhase], ['count', countPhase], ['idle', idlePhase], ['client', clientPhase]]) {
      if (!PHASES.includes(name)) continue;
      await fn(sides, proc, M);
      pw.phases[name] = power();
      if (powerKey(pw.phases[name]) !== powerKey(pw.start)) log(`warning: power state changed during the ${name} phase: ${powerKey(pw.start)} → ${powerKey(pw.phases[name])}`);
    }
  } finally {
    proc.close();
  }
  pw.changed = Object.values(pw.phases).some(p => powerKey(p) !== powerKey(pw.start));
  staticSizes(bin, M.cand);
  if (REF) M.ref['size.binary'] = [[fs.statSync(sides[0].bin).size]];
  const S = Object.fromEntries(sides.map(s => [s.name, summarize(M[s.name])]));
  if (REF) for (const [id, v] of Object.entries(refMeta.metrics)) if (/^size\.src\./.test(id)) S.ref[id] = v; // build-free: from the reference's run
  const rows = REF ? compare(M.ref, M.cand, S.ref, S.cand) : Object.keys(S.cand).sort().map(id => ({ id, unit: unit(id), cand: S.cand[id].value, spread: S.cand[id].spread }));
  const alarms = rows.filter(r => r.alarm);
  const result = {
    bench: 2, rev, profile: PROFILE, date: new Date().toISOString(), host: `${os.type()} ${os.arch()} ${os.cpus()[0]?.model || ''} ${os.cpus().length} CPUs`, power: pw,
    unreliable: pw.changed ? 'power state changed during the run' : null, db: path.basename(DB),
    rounds: ROUNDS, uiSamples: UI_SAMPLES, idleSeconds: IDLE_S, phases: PHASES, seconds: Math.round((Date.now() - T0) / 1000),
    encodings: sides.at(-1).encodings || {}, metrics: S.cand, samples: M.cand,
    ...(REF ? { ref: { dir: REF, rev: refMeta.rev, metrics: S.ref, samples: M.ref }, compare: rows, alarms: alarms.map(r => r.id) } : {}),
  };
  fs.mkdirSync(OUT, { recursive: true });
  fs.writeFileSync(path.join(OUT, 'result.json'), JSON.stringify(result, null, 1));
  fs.copyFileSync(bin, path.join(OUT, 'practical'));
  const tbl = table(rows, !REF);
  fs.writeFileSync(path.join(OUT, 'table.txt'), tbl + '\n');
  console.log(tbl);
  console.log(`\n${REF ? `${refMeta.rev} (ref) vs ${rev}` : rev}: ${result.seconds} s, ${ROUNDS} server rounds, ${UI_SAMPLES} UI samples per side, idle ${IDLE_S} s, power ${powerKey(pw.start)} → ${OUT}`);
  if (REF && !SIZE_JUDGED) console.log(`size.binary: information only (profiles ${refMeta.profile} → ${PROFILE}); judged between two release/dist builds: E2E_PROFILE=release against a release-profile reference`);
  for (const s of sides) fs.rmSync(path.join(WORK, s.name), { recursive: true, force: true });
  if (pw.changed) { console.log('UNRELIABLE: the power state changed during the run (see result.json power) — rerun on a steady power state'); if (REF) process.exitCode = 4; return; }
  if (REF) {
    if (alarms.length) { console.log(`REGRESSION in ${alarms.length} metric(s): ${alarms.map(r => `${r.id} ×${r.ratio} (${r.ref} → ${r.cand} ${r.unit})`).join('; ')}`); process.exitCode = 1; }
    else console.log('no regression beyond the thresholds');
  }
}

main().catch(e => { console.error(e); process.exit(3); }); // exit: stops the servers ('exit' handler)
