// js/remote.js — importing from a remote server once, and setting up a lasting sync.
//
// The sign-in happens in a new browser window at the remote server itself, so this file never sees
// a password: it starts the flow, opens the window, and asks the server every two seconds whether
// the user confirmed. Nothing keeps running when the page is left — the asking stops with it.
(function () {
  const settings = document.querySelector('.settings');
  if (!settings) return;
  const { $, $$, escQ, req, send, tell } = practical;
  const T = trs('js-rm-', ['waiting', 'nothing', 'readonly-src', 'both-hint', 'pull-hint', 'need-server']);

  settings.querySelectorAll('.rm-form').forEach(form => {
    const kind = form.dataset.kind, sync = form.dataset.sync === 'true';
    const q = s => form.querySelector(s);
    const origin = q('.rm-origin'), go = q('.rm-go'), wait = q('.rm-wait'), picked = q('.rm-picked'), list = q('.rm-list'), take = q('.rm-take');
    let handle = null, polling = null, chosen = null;

    // the direction radios decide what the hint under them says, and a pull-only collection
    // cannot offer "both ways" at all
    const dirHint = q('.rm-dir-hint');
    if (sync) {
      form.querySelectorAll('input[name=rmdir]').forEach(r => r.addEventListener('change', () => {
        if (dirHint) dirHint.textContent = r.value === 'pull' ? T['pull-hint'] : T['both-hint'];
      }));
    }

    const stop = () => { if (polling) { clearInterval(polling); polling = null; } wait.hidden = true; };
    // a page left mid-flow must not keep asking
    addEventListener('pagehide', stop);

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

    go.addEventListener('click', async () => {
      if (needServer()) return;
      const r = await req('/settings/remote/login', { method: 'POST', json: { origin: origin.value } });
      if (!tell(r).ok) return;
      handle = r.data.handle;
      // a user gesture opened this, so the popup blocker lets it through
      window.open(r.data.url, '_blank', 'noopener');
      wait.hidden = false;
      stop.done = false;
      polling = setInterval(async () => {
        const p = await req('/settings/remote/poll', { method: 'POST', json: { handle } });
        if (!p.ok) { stop(); tell(p); return; }
        if (!p.data.granted) return;
        stop();
        await offer();
      }, 2000);
    });

    async function offer() {
      const r = await req('/settings/remote/collections', { method: 'POST', json: { handle } });
      if (!tell(r).ok) return;
      const items = kind === 'cal' ? r.data.calendars : r.data.addressbooks;
      picked.hidden = false;
      if (!items.length) { list.innerHTML = `<div class="cell noic"><span>${escQ(T.nothing)}</span></div>`; take.disabled = true; return; }
      list.innerHTML = items
        .map((c, i) => `<label class="cell noic"><span><span class="swatch" style="background:${escQ(c.color || '#0a84ff')}"></span> ${escQ(c.name)}${c.writable ? '' : `<span class="sub">${escQ(T['readonly-src'])}</span>`}</span><span class="val"><input type="radio" name="rmpick" value="${i}"${i ? '' : ' checked'}></span></label>`)
        .join('');
      chosen = items;
      take.disabled = false;
      pickChanged();
      list.querySelectorAll('input[name=rmpick]').forEach(r => r.addEventListener('change', pickChanged));
    }

    // a collection the remote will not let us write can only be pulled: the radio says so instead
    // of letting the user ask for something that cannot happen
    function pickChanged() {
      if (!sync || !chosen) return;
      const c = chosen[+(form.querySelector('input[name=rmpick]:checked') || {}).value || 0];
      const both = form.querySelector('input[name=rmdir][value=both]');
      if (!both) return;
      both.disabled = !c.writable;
      if (!c.writable) {
        form.querySelector('input[name=rmdir][value=pull]').checked = true;
        if (dirHint) dirHint.textContent = T['pull-hint'];
      }
    }

    form.addEventListener('submit', async e => {
      e.preventDefault();
      if (needServer()) return;
      if (!chosen || !handle) return;
      const c = chosen[+(form.querySelector('input[name=rmpick]:checked') || {}).value || 0];
      const body = { handle, kind, href: c.href, name: c.name, color: c.color || '' };
      if (sync) {
        body.direction = (form.querySelector('input[name=rmdir]:checked') || {}).value || 'both';
        body.every_min = +(q('.rm-every') || {}).value || 5;
      }
      const w = practical.writer(form);
      await w.run(async () => {
        const r = await req(sync ? '/settings/remote/link' : '/settings/remote/import', { method: 'POST', json: body });
        if (r.ok) { sessionStorage.toast = tr('js-set-imported', { n: r.data.count }); practical.leaving = true; location.reload(); }
        return r;
      });
    });
  });

  // the link rows: sync now, or stop syncing
  $$('.rm-row').forEach(row => {
    const id = row.dataset.rmId;
    const post = async body => tell(await send(`/settings/remote/link/${encodeURIComponent(id)}`, 'POST', body));
    const now = row.querySelector('.rm-now'), drop = row.querySelector('.rm-drop');
    if (now) now.addEventListener('click', async () => { now.disabled = true; const r = await post({ action: 'now' }); now.disabled = false; if (r.ok) { practical.leaving = true; location.reload(); } });
    if (drop) drop.addEventListener('click', async () => { if (!confirm(tr('js-rm-remove-hint'))) return; const r = await post({ action: 'remove' }); if (r.ok) { practical.leaving = true; location.reload(); } });
  });
})();
