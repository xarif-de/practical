/* ===== settings: import files, browser push ===== */
(function () {
  const settings = document.querySelector('.settings');
  if (!settings) return;
  const { esc } = practical, csrf = practical.csrf();
  const T = { ...trs('js-set-', ['err', 'active', 'denied', 'unsupported']), done: n => tr('js-set-imported', { n }) };
  // import: read the chosen file, POST its bytes (no multipart)
  settings.querySelectorAll('input[type=file][data-import]').forEach(inp => { const w = practical.writer(inp.closest('form') || inp); inp.addEventListener('change', async () => {
    const file = inp.files[0]; if (!file) return;
    const body = await file.arrayBuffer();
    await w.run(async () => { const r = await practical.req(inp.dataset.import, { method: 'POST', headers: { 'Content-Type': 'application/octet-stream' }, body }); if (r.ok) { sessionStorage.toast = T.done(r.data.imported); practical.leaving = true; location.reload(); } return r; });
    inp.value = '';
  }); });
  // import into a NEW calendar or address book (.imp-form, data-kind = the route's collection kind): the page
  // reads the file first — for a calendar its calendar-level X-WR-* lines (the ones RustiCal's own IMPORT
  // reads, plus Apple's colour) prefill the form and the types it holds are switched on and locked; a .vcf
  // carries no book metadata, so its name comes from the file name. The server creates the collection
  // and its objects in one transaction.
  const unfold = text => text.replace(/\r?\n[ \t]/g, '').split(/\r?\n/);
  const READ = {
    calendars: text => {
      const head = [], uids = { VEVENT: new Set(), VTODO: new Set(), VJOURNAL: new Set() };
      let cur = null, body = false;
      for (const l of unfold(text)) {
        const u = l.toUpperCase(), b = u.match(/^BEGIN:(VEVENT|VTODO|VJOURNAL|VTIMEZONE)$/);
        if (b) { body = true; if (b[1] !== 'VTIMEZONE') cur = b[1]; continue; }
        if (/^END:(VEVENT|VTODO|VJOURNAL)$/.test(u)) { cur = null; continue; }
        if (!body) head.push(l); else if (cur && /^UID[:;]/.test(u)) uids[cur].add(l.slice(l.indexOf(':') + 1).trim()); // overrides share their series' UID: one object
      }
      const prop = n => { const l = head.find(x => x.toUpperCase().startsWith(n + ':') || x.toUpperCase().startsWith(n + ';')); return l ? l.slice(l.indexOf(':') + 1).replace(/\\[nN]/g, '\n').replace(/\\([,;\\])/g, '$1').trim() : ''; };
      return { name: prop('X-WR-CALNAME'), desc: prop('X-WR-CALDESC'), color: prop('X-WR-CALCOLOR') || prop('X-APPLE-CALENDAR-COLOR'), tz: prop('X-WR-TIMEZONE'), n: { ev: uids.VEVENT.size, td: uids.VTODO.size, jr: uids.VJOURNAL.size }, types: { ev: 'VEVENT', td: 'VTODO', jr: 'VJOURNAL' } };
    },
    addressbooks: text => {
      let cards = 0, groups = 0, card = false, grp = false;
      for (const l of unfold(text)) {
        const u = l.toUpperCase();
        if (u === 'BEGIN:VCARD') { card = true; grp = false; } else if (u === 'END:VCARD' && card) { card = false; if (grp) groups++; else cards++; }
        else if (card && /^(KIND|X-ADDRESSBOOKSERVER-KIND):GROUP$/.test(u)) grp = true;
      }
      return { n: { ct: cards, gr: groups } };
    },
  };
  const hexOf = v => { const m = v.match(/^#?([0-9a-f]{6})([0-9a-f]{2})?$/i); return m ? '#' + m[1].toLowerCase() : (window.colorPicker && window.colorPicker.toHex(v)) || ''; }; // #rrggbbaa (Apple) loses its alpha
  const count = (k, n) => tr(`js-imp-${k}`, { n });
  settings.querySelectorAll('.imp-form').forEach(form => {
    const kind = form.dataset.kind, q = s => form.querySelector(s), el = form.elements, fields = q('.imp-fields'), info = q('.imp-info'), go = q('.imp-go'), goText = go.textContent;
    let buf = null;
    q('.imp-file').addEventListener('change', async e => {
      const f = e.target.files[0]; if (!f) return;
      buf = await f.arrayBuffer(); const d = READ[kind](new TextDecoder().decode(buf)), taken = [];
      const found = Object.keys(d.n).filter(k => d.n[k]);
      q('.imp-pick span').textContent = tr('js-imp-other');
      if (!found.length) { buf = null; fields.hidden = true; info.textContent = ''; info.append(tr('js-imp-file', { file: f.name }) + ' · ', Object.assign(document.createElement('span'), { className: 'danger', textContent: tr(kind === 'calendars' ? 'js-imp-empty' : 'js-imp-ab-empty') })); return; }
      el.name.value = d.name || f.name.replace(/\.(ics|ical|ifb|icalendar|vcf|vcard)$/i, ''); if (d.name) taken.push(tr('js-imp-f-name'));
      el.description.value = d.desc || ''; if (d.desc) taken.push(tr('js-imp-f-desc'));
      const hex = d.color && hexOf(d.color);
      el.color.value = hex || form.dataset.defaultColor; q('.cpk-btn').style.setProperty('--c', el.color.value); if (hex) taken.push(tr('js-imp-f-color'));
      if (el.timezone) { el.timezone.value = d.tz || form.dataset.defaultTz; if (d.tz) taken.push(tr('js-imp-f-tz')); }
      form.querySelectorAll('input[name=comp]').forEach(c => { const has = found.some(k => d.types[k] === c.value); c.checked = has; c.disabled = has; c.closest('label').title = has ? tr('js-imp-in-file') : ''; });
      info.textContent = [tr('js-imp-file', { file: f.name }), ...found.map(k => count(k, d.n[k]))].join(' · ') + (taken.length ? ' · ' + tr('js-imp-taken', { what: taken.join(', ') }) : '');
      fields.hidden = false; el.name.focus(); el.name.select();
    });
    const w = practical.writer(form, { buttons: () => [go] });
    form.addEventListener('submit', e => {
      e.preventDefault(); if (!buf || !form.reportValidity()) return;
      go.textContent = tr('js-imp-busy');
      const p = new URLSearchParams({ owner: el.owner.value, name: el.name.value.trim(), color: el.color.value, description: el.description.value.trim() });
      if (kind === 'calendars') { p.set('timezone', el.timezone.value.trim()); p.set('components', [...form.querySelectorAll('input[name=comp]')].filter(c => c.checked).map(c => c.value).join(',')); }
      w.run(async () => {
        const r = await practical.req(`/settings/import/${kind}?${p}`, { method: 'POST', headers: { 'Content-Type': kind === 'calendars' ? 'text/calendar' : 'text/vcard' }, body: buf });
        if (r.ok) { const d = r.data; sessionStorage.toast = tr('js-imp-done', { title: el.name.value.trim(), count: d.imported }); location.hash = `#${kind === 'calendars' ? 'cal' : 'ab'}-${d.owner}-${d.id}`; practical.leaving = true; location.reload(); return r; }
        go.textContent = goText; return r;
      });
    });
  });
  // push
  const en = document.getElementById('push-enable'), dis = document.getElementById('push-disable'), st = document.getElementById('push-status'), list = document.getElementById('push-devices');
  if (!en) return;
  const b64 = s => { const p = '='.repeat((4 - s.length % 4) % 4); const raw = atob((s + p).replace(/-/g, '+').replace(/_/g, '/')); return Uint8Array.from(raw, c => c.charCodeAt(0)); };
  async function refresh() {
    try { const subs = await (await fetch('/api/push/subscriptions')).json(); list.innerHTML = subs.map(s => `<li><small class="muted">${esc((s.user_agent || '').slice(0, 60))} · ${s.created_at}</small></li>`).join('') || ''; } catch (_) {}
    if (!('serviceWorker' in navigator) || !('PushManager' in window)) { st.textContent = T.unsupported; en.disabled = true; return; }
    const reg = await navigator.serviceWorker.getRegistration('/'); const sub = reg && await reg.pushManager.getSubscription();
    en.hidden = !!sub; dis.hidden = !sub; st.textContent = sub ? T.active : '';
  }
  en.onclick = async () => {
    try {
      const perm = await Notification.requestPermission(); if (perm !== 'granted') { st.textContent = T.denied; return; }
      const reg = await navigator.serviceWorker.register('/assets/sw.js', { scope: '/' });
      const key = (await (await fetch('/api/push/key')).json()).key;
      const sub = await reg.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: b64(key) });
      const j = sub.toJSON();
      const r = await practical.send('/api/push/subscriptions', 'POST', { endpoint: j.endpoint, keys: j.keys });
      if (!r.ok) throw new Error(r.status);
      refresh();
    } catch (e) { st.textContent = T.err + ': ' + e.message; }
  };
  dis.onclick = async () => {
    const reg = await navigator.serviceWorker.getRegistration('/'); const sub = reg && await reg.pushManager.getSubscription();
    if (sub) { practical.tell(await practical.send('/api/push/subscriptions', 'DELETE', { endpoint: sub.endpoint })); await sub.unsubscribe(); }
    refresh();
  };
  refresh();
})();
