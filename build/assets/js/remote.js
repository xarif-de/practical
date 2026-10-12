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
  /* The addresses the user signed in at before (`#rm-servers`, settings.html). Read once: the same
     list serves all four pages, because it is the same question each time. */
  let SERVERS = [];
  try { SERVERS = JSON.parse($('#rm-servers')?.textContent || '[]'); } catch (_) { SERVERS = []; }
  /* Closers of the suggestion boxes, one per page that has a field. Scroll fires per wheel tick, so
     the four pages share one listener pair instead of binding their own. */
  const CLOSERS = [];
  const closeAllSug = () => CLOSERS.forEach(f => f());
  addEventListener('scroll', closeAllSug, { passive: true, capture: true });
  addEventListener('resize', closeAllSug);

  const T = trs('js-rm-', ['waiting', 'nothing', 'readonly-src', 'both-hint', 'pull-hint', 'need-server', 'need-pick', 'doing-title']);
  T['doing'] = T['doing-title'];

  /* The progress dialog, shared by all four remote pages (one batch runs at a time). It is the
     whole progress report: a bar divided by the NUMBER OF COLLECTIONS, which this file already
     knows because each one is its own request, so the server needs no say in it. One collection
     leaves nothing to divide — the bar then carries no value, which is what makes it sweep.

     Escape and the backdrop are taken away while the batch runs: a request the server is already
     serving cannot be called back, and a dialog that closes on a collection still being imported
     would say the opposite. */
  const dlgEl = $('.rm-dlg');
  const dlg = {
    head: dlgEl && $('#rm-dlg-h', dlgEl),
    bar: dlgEl && $('.rm-dlg-bar', dlgEl),
    now: dlgEl && $('.rm-dlg-now', dlgEl),
    list: dlgEl && $('.rm-dlg-list', dlgEl),
    close: dlgEl && $('.rm-dlg-close', dlgEl),
    busy: false,
    start(n) {
      if (!dlgEl) return;
      this.busy = true;
      this.list.innerHTML = '';
      this.now.textContent = '';
      this.head.textContent = T['doing'];
      this.close.hidden = true;
      if (n > 1) { this.bar.max = n; this.bar.value = 0; } else this.bar.removeAttribute('value');
      if (!dlgEl.open) dlgEl.showModal();
    },
    at(done, n, name) {
      if (!dlgEl) return;
      if (n > 1) this.bar.value = done;
      if (name) this.now.textContent = tr('js-rm-doing', { n: done + 1, of: n, name });
    },
    /* `why` is the server's own message for a refused collection. It goes in the ROW, not only in
       the toast `tell` raises: the toast times out while the dialog stays, and "work" with a red
       cross and no reason is the one thing the user cannot act on. */
    add(name, count, why) {
      if (!dlgEl) return;
      const li = document.createElement('li');
      if (count == null) li.className = 'bad';
      li.innerHTML = practical.ico(count == null ? 'x' : 'check');
      li.append(` ${name}`, count == null ? (why ? ` · ${why}` : '') : ` · ${tr('js-rm-entry-count', { n: count })}`);
      this.list.appendChild(li);
    },
    /* A clean batch is followed by a reload of the collections list, which takes the dialog with
       it; leaving it open until then keeps the page from flashing empty. A batch with a failure
       keeps the page, so the dialog stays with its lines readable and its button is the way on —
       which still reloads when something WAS taken, or the list would not show it. */
    end(failed, done) {
      if (!dlgEl) return;
      this.busy = false;
      this.now.textContent = '';
      this.reloadOnClose = failed > 0 && done > 0;
      if (failed) {
        // a heading still reading "being taken over" over a finished list said the wrong thing
        this.head.textContent = tr('js-rm-failed', { n: failed });
        this.close.hidden = false;
        this.close.focus();
      } else dlgEl.close();
    },
  };
  if (dlgEl) {
    dlgEl.addEventListener('cancel', e => { if (dlg.busy) e.preventDefault(); });
    dlg.close.addEventListener('click', () => dlgEl.close());
    /* On `close`, not on the button: Escape closes the dialog too, and it used to leave the page
       behind with a collection list that no longer matched what was taken. */
    dlgEl.addEventListener('close', () => {
      if (!dlg.reloadOnClose) return;
      dlg.reloadOnClose = false;
      practical.leaving = true;
      location.hash = '#collections';
      location.reload();
    });
  }

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

    /* ---- the addresses used before ------------------------------------------------------------
       Built like the search suggestions (search.js): a listbox of our own, not a <datalist>, whose
       dropdown triangle no other field here has and which came back on hover even when hidden.
       Opens on focus, narrows as letters are typed, ↑/↓ walks it, Enter takes the marked one. */
    const sug = q('.rm-sug');
    let marked = -1, shown = [];

    const closeSug = () => {
      sug.hidden = true;
      marked = -1;
      origin.setAttribute('aria-expanded', 'false');
      origin.removeAttribute('aria-activedescendant');
    };

    const mark = i => {
      marked = i;
      [...sug.children].forEach((el, n) => el.setAttribute('aria-selected', String(n === i)));
      if (i >= 0) origin.setAttribute('aria-activedescendant', sug.children[i].id);
      else origin.removeAttribute('aria-activedescendant');
    };

    /* The list is `fixed` (settings.css), so its place is set here. It is put under the FIELD, not
       the row, so it lines up with what is being typed, and kept inside the window. */
    const place = () => {
      const f = origin.getBoundingClientRect();
      sug.style.width = `${Math.max(f.width, 190)}px`;
      sug.style.left = `${Math.min(f.left, innerWidth - sug.offsetWidth - 8)}px`;
      const below = innerHeight - f.bottom;
      // above the field when there is no room below it
      if (below < sug.offsetHeight + 12 && f.top > sug.offsetHeight + 12) {
        sug.style.top = `${f.top - sug.offsetHeight - 4}px`;
      } else {
        sug.style.top = `${f.bottom + 4}px`;
      }
    };

    function drawSug() {
      const typed = origin.value.trim().toLowerCase();
      // what is already typed in full is no suggestion: offering it back says nothing
      shown = SERVERS.filter(v => v.toLowerCase().includes(typed) && v.toLowerCase() !== typed);
      sug.innerHTML = shown
        .map((v, i) => `<div class="r" role="option" id="${sug.id}-o${i}" aria-selected="false" data-v="${escQ(v)}">${escQ(v)}</div>`)
        .join('');
      sug.hidden = !shown.length;
      origin.setAttribute('aria-expanded', String(!!shown.length));
      if (shown.length) place();
      mark(-1);
    }

    const takeSug = v => {
      origin.value = v;
      origin.setCustomValidity('');
      closeSug();
      origin.focus();
    };

    if (sug && SERVERS.length) {
      sug.id = `rm-sug-${form.closest('.page').id}`;
      origin.setAttribute('aria-controls', sug.id);
      origin.addEventListener('focus', drawSug);
      origin.addEventListener('input', drawSug);
      // a click must not pull the focus out of the field first
      sug.addEventListener('mousedown', e => e.preventDefault());
      sug.addEventListener('click', e => {
        const r = e.target.closest('.r');
        if (r) takeSug(r.dataset.v);
      });
      sug.addEventListener('mousemove', e => {
        const r = e.target.closest('.r');
        if (r) mark([...sug.children].indexOf(r));
      });
      origin.addEventListener('blur', () => setTimeout(closeSug, 120));
      // a fixed box does not travel with the page: scrolling or resizing closes it rather than
      // leaving it beside the field it belongs to. One shared pair of listeners: four pages carry
      // one of these fields each, and only one of them can be open.
      CLOSERS.push(closeSug);
    }

    origin.addEventListener('keydown', e => {
      const open = !sug.hidden && shown.length;
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        if (!open) { drawSug(); return; }
        e.preventDefault();
        const n = shown.length;
        mark(e.key === 'ArrowDown' ? (marked + 1) % n : (marked <= 0 ? n : marked) - 1);
        return;
      }
      if (e.key === 'Escape' && open) { e.stopPropagation(); closeSug(); return; }
      /* Enter: take the marked suggestion, else connect. It must never reach the form, whose
         submit is the import BATCH — an address plus Enter would have imported nothing. */
      if (e.key === 'Enter') {
        e.preventDefault();
        if (marked >= 0) { takeSug(shown[marked]); return; }
        closeSug();
        if (!go.disabled) go.click();
      }
    });

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
      // a second click while the first is still polling would leave the first interval running for
      // ever on a handle nobody reads, and overwrite the handle the user is granting
      stop();
      go.disabled = true;
      /* A credential this user already granted for the same address is used again, so a second
         import or a second link asks for no sign-in at all. 404 means there is nothing to reuse —
         the normal case after a one-off import, whose password is revoked at the remote on
         purpose — and then the usual flow runs. */
      const reuse = await req('/settings/remote/reuse', { method: 'POST', json: { origin: origin.value } });
      if (reuse.ok) {
        handle = reuse.data.handle;
        go.disabled = false;
        await offer();
        return;
      }
      if (!reuse.ok && reuse.status !== 404) { go.disabled = false; tell(reuse); return; }
      const r = await req('/settings/remote/login', { method: 'POST', json: { origin: origin.value } });
      go.disabled = false;
      if (!tell(r).ok) return;
      stop();
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

      /* One request per collection, so the dialog can name the one that is running AND divide the
         bar by their number — no progress has to be asked of the server for that. With a single
         collection there is nothing to divide, and the bar says "running" instead (see dlg.start).
         The writer still runs around the batch: it disables the buttons, without which a second
         click would start the whole batch again. */
      const w = practical.writer(form, { buttons: () => [take, go] });
      let done = 0, failed = 0;
      dlg.start(picks.length);
      await w.run(async () => {
        let last = { ok: true };
        for (const [i, c] of picks.entries()) {
          dlg.at(i, picks.length, c.name);
          const body = { handle, kind, href: c.href, name: c.name, color: c.color || '' };
          if (sync) {
            body.direction = direction;
            body.every_min = every;
          }
          last = await req(sync ? '/settings/remote/link' : '/settings/remote/import', { method: 'POST', json: body });
          if (last.ok) {
            done += 1;
            dlg.add(c.name, (last.data || {}).count);
          } else {
            // one collection the server will not take must not lose the others
            failed += 1;
            dlg.add(c.name, null, last.msg);
            tell(last);
          }
          dlg.at(i + 1, picks.length, null);
        }
        // the sign-in is over once the batch is: one request, so a password is not left behind
        if (!sync) await req('/settings/remote/done', { method: 'POST', json: { handle } });
        /* A clean batch leaves at once: back to the overview the user started from, which is where
           the new collections are. A batch with a failure must NOT — measured: with one collection
           refused and one taken, the reload tore the dialog away before its red line could be
           read, so the only thing left was a toast that did not say which one. The dialog stays,
           and leaving is what its button does (`dlg.end`). */
        if (done && !failed) {
          sessionStorage.toast = tr('js-rm-took', { n: done });
          practical.leaving = true;
          location.hash = '#collections';
          location.reload();
        }
        dlg.end(failed, done);
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
    // the shell's own confirmation: `confirm` has no focus return, no Escape handling and no style
    if (drop) drop.addEventListener('click', async () => { if (!(await practical.ask(tr('js-rm-remove-hint')))) return; const r = await post({ action: 'remove' }); if (r.ok) { practical.leaving = true; location.hash = '#collections'; location.reload(); } });
  });
})();
