// js/remote.js — importing from a remote server once, and setting up a lasting sync.
//
// The sign-in happens in a new browser window at the remote server itself, so this file never sees
// a password: it starts the flow, opens the window, and asks the server every two seconds whether
// the user confirmed. Nothing keeps running when the page is left — the asking stops with it.
//
// Several collections can be taken in one go. Each one is its own request, so the line under the
// list can say which one is running: an import of a few thousand events takes seconds per
// collection, and a page that only sits there looks broken.
(function () {
  const settings = document.querySelector('.settings');
  if (!settings) return;
  const { $, $$, escQ, req, send, tell } = practical;
  const T = trs('js-rm-', ['waiting', 'nothing', 'readonly-src', 'both-hint', 'pull-hint', 'need-server', 'need-pick']);

  settings.querySelectorAll('.rm-form').forEach(form => {
    const kind = form.dataset.kind, sync = form.dataset.sync === 'true';
    const q = s => form.querySelector(s);
    const origin = q('.rm-origin'), go = q('.rm-go'), wait = q('.rm-wait'), busy = q('.rm-busy');
    const picked = q('.rm-picked'), list = q('.rm-list'), take = q('.rm-take');
    let handle = null, polling = null, chosen = null;

    /* The address is required and nothing starts without it. Checked here so the user is told at
       the field rather than by a failed request; the server checks it again (`origin_of`) and the
       resolver checks what it resolves to, because a browser check is a courtesy, not a guard. */
    const needServer = () => {
      if (origin.value.trim()) return false;
      origin.setCustomValidity(T['need-server']);
      origin.reportValidity();
      origin.focus();
      return true;
    };
    origin.addEventListener('input', () => origin.setCustomValidity(''));

    // the direction radios decide what the hint under them says, and a collection the remote will
    // not let us write can only ever be pulled
    const dirHint = q('.rm-dir-hint');
    if (sync) {
      form.querySelectorAll('input[name=rmdir]').forEach(r => r.addEventListener('change', () => {
        if (dirHint) dirHint.textContent = r.value === 'pull' ? T['pull-hint'] : T['both-hint'];
      }));
    }

    const stop = () => { if (polling) { clearInterval(polling); polling = null; } wait.hidden = true; };
    // a page left mid-flow must not keep asking
    addEventListener('pagehide', stop);

    /** The line under the list: what is running, or nothing. */
    const say = text => { busy.hidden = !text; busy.textContent = text || ''; };

    go.addEventListener('click', async () => {
      if (needServer()) return;
      go.disabled = true;
      const r = await req('/settings/remote/login', { method: 'POST', json: { origin: origin.value } });
      go.disabled = false;
      if (!tell(r).ok) return;
      handle = r.data.handle;
      // a user gesture opened this, so the popup blocker lets it through
      window.open(r.data.url, '_blank', 'noopener');
      wait.hidden = false;
      polling = setInterval(async () => {
        const p = await req('/settings/remote/poll', { method: 'POST', json: { handle } });
        if (!p.ok) { stop(); tell(p); return; }
        if (!p.data.granted) return;
        stop();
        await offer();
      }, 2000);
    });

    async function offer() {
      say(T.waiting);
      const r = await req('/settings/remote/collections', { method: 'POST', json: { handle } });
      say('');
      if (!tell(r).ok) return;
      const items = kind === 'cal' ? r.data.calendars : r.data.addressbooks;
      picked.hidden = false;
      if (!items.length) { list.innerHTML = `<div class="cell noic"><span>${escQ(T.nothing)}</span></div>`; take.disabled = true; return; }
      /* A toggle per row, which is how the settings ask for several things at once (the component
         set of a new calendar). Everything is on to begin with: a user who signed in to take their
         calendars over means all of them more often than one. */
      /* Three columns like every other row that carries a colour: swatch, name, value. Putting the
         swatch INSIDE the name cell made the row wrap to 62 px instead of the usual 44. */
      list.innerHTML = items
        .map(
          (c, i) =>
            `<label class="cell"><span class="swatch" style="background:${escQ(c.color || '#0a84ff')}">${practical.ico(kind === 'cal' ? 'calendar' : 'contacts')}</span>`
            + `<span>${escQ(c.name)}${c.writable ? '' : `<span class="sub">${escQ(T['readonly-src'])}</span>`}</span>`
            + `<span class="val"><span class="toggle"><input type="checkbox" class="rm-pick" value="${i}" checked><i></i></span></span></label>`,
        )
        .join('');
      chosen = items;
      take.disabled = false;
      list.querySelectorAll('.rm-pick').forEach(c => c.addEventListener('change', pickChanged));
      pickChanged();
    }

    /** Which rows are ticked. */
    const ticked = () => [...form.querySelectorAll('.rm-pick:checked')].map(c => chosen[+c.value]).filter(Boolean);

    /* "Both ways" needs every chosen collection to be writable: the direction is one setting for
       the whole batch, so one read-only collection decides it for all of them. */
    function pickChanged() {
      const picks = ticked();
      take.disabled = !picks.length;
      if (!sync) return;
      const both = form.querySelector('input[name=rmdir][value=both]');
      if (!both) return;
      const writable = picks.length > 0 && picks.every(c => c.writable);
      both.disabled = !writable;
      if (!writable) {
        form.querySelector('input[name=rmdir][value=pull]').checked = true;
        if (dirHint) dirHint.textContent = T['pull-hint'];
      }
    }

    form.addEventListener('submit', async e => {
      e.preventDefault();
      if (needServer()) return;
      const picks = ticked();
      if (!handle || !picks.length) { say(T['need-pick']); return; }
      const direction = sync ? (form.querySelector('input[name=rmdir]:checked') || {}).value || 'both' : null;
      const every = sync ? +(q('.rm-every') || {}).value || 5 : null;

      /* One request per collection, so the line can name the one that is running. The writer gives
         the page its busy cursor and the top bar's progress line, and disables the buttons for as
         long as the batch runs — without that, a second click would start the whole batch again. */
      const w = practical.writer(form, { buttons: () => [take, go] });
      let done = 0, failed = 0;
      await w.run(async () => {
        let last = { ok: true };
        for (const [i, c] of picks.entries()) {
          say(tr('js-rm-doing', { n: i + 1, of: picks.length, name: c.name }));
          const body = { handle, kind, href: c.href, name: c.name, color: c.color || '' };
          if (sync) {
            body.direction = direction;
            body.every_min = every;
          }
          last = await req(sync ? '/settings/remote/link' : '/settings/remote/import', { method: 'POST', json: body });
          if (last.ok) done += 1;
          else {
            // one collection the server will not take must not lose the others
            failed += 1;
            tell(last);
          }
        }
        // the sign-in is over once the batch is: one request, so a password is not left behind
        if (!sync) await req('/settings/remote/done', { method: 'POST', json: { handle } });
        if (done) {
          sessionStorage.toast = tr('js-rm-took', { n: done });
          practical.leaving = true;
          // back to the overview the user started from: that is where the new collections are
          location.hash = '#collections';
          location.reload();
        }
        say(failed ? tr('js-rm-failed', { n: failed }) : '');
        // the writer wants an answer to tell about; a partial batch has already told each failure
        return { ok: true, status: 200, data: null };
      });
    });
  });

  // the link rows: sync now, or stop syncing
  $$('.rm-row').forEach(row => {
    const id = row.dataset.rmId;
    const post = async body => tell(await send(`/settings/remote/link/${encodeURIComponent(id)}`, 'POST', body));
    const now = row.querySelector('.rm-now'), drop = row.querySelector('.rm-drop');
    if (now) now.addEventListener('click', async () => { now.disabled = true; const r = await post({ action: 'now' }); now.disabled = false; if (r.ok) { practical.leaving = true; location.reload(); } });
    if (drop) drop.addEventListener('click', async () => { if (!confirm(tr('js-rm-remove-hint'))) return; const r = await post({ action: 'remove' }); if (r.ok) { practical.leaving = true; location.hash = '#collections'; location.reload(); } });
  });
})();
