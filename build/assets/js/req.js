/* requests and writes: every request a user waits for goes through practical.req, every write of an editor or a
   form through practical.writer. A failure is told (toast, the editor's error line, the offline notice), never
   thrown, and never drawn as a page inside the page (audit3-robustness R5, R6, R10, R13). */
(function () {
  const P = window.practical, nap = ms => new Promise(r => setTimeout(r, ms));
  const KIND = s => s === 401 ? 'auth' : s === 403 ? 'forbidden' : s === 404 ? 'notfound' : s === 409 ? 'conflict' : s === 429 || s === 503 ? 'busy' : s >= 500 ? 'server' : 'invalid';
  const OWN = new Set(['forbidden', 'notfound', 'conflict', 'invalid']); // kinds whose server text (localized for a signed-in user: ctx::localize_errors) says more than the app's
  /* practical.req(url, o) → { ok: true, status, data } | { ok: false, kind, status, msg, body }; never throws.
     o: method, json (sent as JSON) or body, headers, as ('json' | 'text'; default: by the answer's Content-Type), signal,
     keepalive, wait (ms after which the page shows its progress line: navigations).
     kind: 'offline' (no answer) · 'auth' (401, or sent on to /login: the session ended — the page leaves for the
     login, authLost below) · 'forbidden' 403 · 'notfound' 404 · 'conflict' 409 · 'invalid' any other 4xx ·
     'busy' 429/503 (asked again twice first, after Retry-After or 0.5 / 1.5 s) · 'server' 5xx or an unreadable
     answer · 'aborted' (o.signal). msg: the server's own text where it has one (text/plain, ≤ 300 characters) for
     the kinds in OWN, else the app's message for the kind; body: the answer's text (a 409 of /history carries JSON). */
  P.req = async (url, o = {}) => {
    const method = (o.method || 'GET').toUpperCase(), headers = { ...o.headers }; let body = o.body;
    if (o.json !== undefined) { body = JSON.stringify(o.json); headers['Content-Type'] = 'application/json'; }
    if (method !== 'GET') headers['X-CSRF-Token'] = P.csrf();
    const fail = (kind, status, text = '', own = '') => ({ ok: false, kind, status, body: text, msg: (OWN.has(kind) && own) || tr('js-req-' + kind) });
    const done = o.wait != null ? P.progress(o.wait) : null;
    try {
      for (let i = 0; ; i++) {
        let r;
        try { r = await fetch(url, { method, headers, body, signal: o.signal, keepalive: o.keepalive }); }
        catch (e) { if (e && e.name === 'AbortError') return { ok: false, kind: 'aborted', status: 0, msg: '', body: '' }; P.online(false); return fail('offline', 0); }
        P.online(true);
        if (r.status === 401 || (r.redirected && new URL(r.url).pathname === '/login')) { P.authLost(); return fail('auth', 401); }
        const kind = r.ok ? '' : KIND(r.status), type = r.headers.get('Content-Type') || '';
        if (kind === 'busy' && i < 2) { const ra = +r.headers.get('Retry-After'); await nap(ra > 0 ? Math.min(ra, 5) * 1000 : 500 * 3 ** i); continue; }
        if (r.ok) { try { return { ok: true, status: r.status, data: (o.as ? o.as === 'json' : type.includes('json')) ? await r.json() : await r.text() }; } catch (_) { return fail('server', r.status); } } // a cut or foreign body
        let text = ''; try { text = await r.text(); } catch (_) {}
        return fail(kind, r.status, text, type.startsWith('text/plain') && text.length <= 300 ? text.trim() : '');
      }
    } finally { if (done) done(); }
  };
  // a failed answer as a toast ('auth': the page is leaving; 'aborted': the caller's own doing); returns the answer
  P.tell = r => { if (r && !r.ok && r.kind !== 'auth' && r.kind !== 'aborted') toast(r.msg, true); return r; };

  // the progress line (the top bar's bottom edge in the accent colour) for a navigation slower than `ms` (perf3 #10)
  let busyN = 0;
  P.progress = ms => { let on = false, off = false; const t = setTimeout(() => { on = true; if (!busyN++) document.documentElement.setAttribute('data-busy', ''); }, ms);
    return () => { if (off) return; off = true; clearTimeout(t); if (on && !--busyN) document.documentElement.removeAttribute('data-busy'); }; }; // stop(): once, however often called

  // the offline notice: shown while the browser says it is offline or a request got no answer, gone with the next answer
  let netEl = null;
  P.online = on => {
    if (on === !netEl) return;
    if (on) { netEl.remove(); netEl = null; return; }
    netEl = document.createElement('div'); netEl.className = 'toast error net'; netEl.setAttribute('role', 'status'); netEl.textContent = tr('js-req-offline-bar');
    document.body.appendChild(netEl);
  };
  addEventListener('offline', () => P.online(false)); addEventListener('online', () => P.online(true));

  /* the session ended (every server restart ends all sessions, R6): what an editor holds unsaved is kept
     (practical.keep → practical.stash, sessionStorage), then the page goes to the login, which brings the user back to
     this address; the editor takes its work back from there (practical.unstash) */
  const keepers = []; let gone = false;
  P.keep = fn => keepers.push(fn);
  P.authLost = () => {
    if (gone) return; gone = true; P.leaving = true;
    for (const fn of keepers) try { fn(); } catch (_) {}
    location.href = '/login?redirect_uri=' + encodeURIComponent(location.pathname + location.search + location.hash);
  };
  P.stash = (name, v) => { try { sessionStorage['practical.stash.' + name] = JSON.stringify({ path: location.pathname, t: Date.now(), v }); } catch (_) {} }; // too big for the storage: lost, as before
  P.unstash = name => { const k = 'practical.stash.' + name; let s = null; try { s = JSON.parse(sessionStorage[k] || 'null'); delete sessionStorage[k]; } catch (_) {} return s && s.path === location.pathname && Date.now() - s.t < 864e5 ? s.v : null; };

  // leaving the page with unsaved work asks first (the browser's own question); practical.leaving: a navigation the app starts after a save
  const dirt = [];
  P.guard = fn => dirt.push(fn);
  addEventListener('beforeunload', e => { if (!P.leaving && dirt.some(fn => fn())) { e.preventDefault(); e.returnValue = ''; } });

  /* the write cycle of an editor or a form: w = practical.writer(box, { buttons, error, conflict, idle }).
     w.run(fn) — fn() → a req() answer — runs one write at a time: called again while one is pending (a double click,
     Enter and a click) it returns the pending one; run(fn, true) instead runs fn once more after it (an autosave: the
     newest state wins). Meanwhile box[data-state=saving][aria-busy=true] and buttons() are disabled; afterwards idle()
     (else each button gets its own state back). A failed answer: 'auth' → nothing (the page leaves for the login);
     'conflict' with conflict = { overwrite, reload } → the question "keep mine / load theirs" (Escape: back to the
     editing, the message stays); else its message into `error` (an element) or a toast. Slower than 300 ms: the
     progress line too. box[data-state]: saving | conflict | error, none once done — the editor's own states (closed,
     view, edit, dirty) live in its classes. */
  P.writer = (box, o = {}) => {
    let pending = null, queued = null;
    const set = s => { if (s) box.dataset.state = s; else delete box.dataset.state; if (s === 'saving') box.setAttribute('aria-busy', 'true'); else box.removeAttribute('aria-busy'); };
    const w = {
      get busy() { return !!pending; },
      run(fn, again) {
        if (pending) { if (!again) return pending; queued = fn; return pending.then(() => { const q = queued; queued = null; return q ? w.run(q) : null; }); }
        const btns = (o.buttons ? o.buttons() : []).filter(Boolean), was = btns.map(b => b.disabled);
        if (o.error) o.error.textContent = '';
        set('saving'); btns.forEach(b => { b.disabled = true; }); let stop = P.progress(300);
        return (pending = (async () => {
          let r, tell = true;
          try {
            r = await fn();
            while (r && !r.ok && r.kind === 'conflict' && o.conflict) {
              set('conflict'); stop(); // the question waits for the user, not for the server
              const a = await askYesNo(tr('js-req-conflict-ask'), tr('js-req-mine'), tr('js-req-theirs'), true);
              if (a === null) break;
              set('saving'); stop = P.progress(300);
              if (!a) { await o.conflict.reload(); tell = false; break; }
              r = await o.conflict.overwrite();
            }
          } catch (e) { console.error(e); r = { ok: false, kind: 'server', status: 0, msg: tr('js-req-server'), body: '' }; }
          pending = null; stop();
          if (o.idle) o.idle(); else btns.forEach((b, i) => { b.disabled = was[i]; });
          if (r && !r.ok && tell && r.kind !== 'auth' && r.kind !== 'aborted') { set('error'); if (o.error) o.error.textContent = r.msg; else toast(r.msg, true); } else set('');
          return r;
        })());
      },
    };
    return w;
  };

  // to the clipboard; a refused write (no focus, no permission, no clipboard) is told instead of failing unseen
  P.copy = text => Promise.resolve().then(() => navigator.clipboard.writeText(text)).then(() => true, () => { toast(tr('js-req-copy'), true); return false; });
})();
