// js/editor/links.js — part of the editor IIFE (src/bundle.rs; shares its scope): links and attachments
  // ---- links & attachments: one model `LK` for URL/ATTACH/CONFERENCE (`links`) and
  // RELATED-TO (`related`), mirrored into the hidden `links_json` for dirty tracking. Adding: a paste field that
  // detects the kind, a search over events and tasks, a drop zone that embeds files as base64 (2 MiB each).
  // Rows in three groups; a conference gets a join card with provider, meeting id and dial-in numbers. ----
  const LKC = $('#ed-links-card'), LKT = LKC.dataset, lkList = $('#ed-lk-list'), lkDetail = $('#ed-lk-detail'), lkMsg = $('#ed-lk-msg');
  let LK = { links: [], related: [] }, lkSel = null, lkPop = null;
  const CONF_HOSTS = [[/(^|\.)zoom\.us$/, 'Zoom'], [/teams\.(microsoft|live)\.com$/, 'Microsoft Teams'], [/^meet\.google\.com$/, 'Google Meet'], [/jit\.?si/, 'Jitsi'], [/webex\.com$/, 'Webex'], [/bigbluebutton|(^|\.)bbb\./, 'BigBlueButton']];
  const ATTACH_EXT = /\.(pdf|jpe?g|png|gif|webp|heic|docx?|xlsx?|pptx?|odt|ods|zip|ics|txt)(\?.*)?$/i;
  const lkHost = u => { try { return new URL(u).hostname.replace(/^www\./, ''); } catch { return ''; } };
  // a link from the data may be opened unless it runs script (javascript:, vbscript:) or shows foreign markup (data:);
  // the URL parser ignores tab/newline anywhere and controls/spaces around it (render.rs `safe_href`, same rule)
  const lkSafe = u => !/^(javascript|vbscript|data):/i.test(String(u).replace(/[\u0000-\u0020\u007f]/g, ''));
  const lkProvider = u => { const h = lkHost(u); const p = CONF_HOSTS.find(([re]) => re.test(h)); return p ? p[1] : ''; };
  const lkKind = u => /^tel:/i.test(u) ? 'conference' : lkProvider(u) ? 'conference' : ATTACH_EXT.test((() => { try { return new URL(u).pathname; } catch { return u; } })()) ? 'attach' : 'url';
  const lkSize = n => n == null ? '' : n >= 1048576 ? (n / 1048576).toLocaleString(lang, { maximumFractionDigits: 1 }) + ' MB' : n >= 1024 ? Math.round(n / 1024) + ' KB' : n + ' B';
  const lkTitle = l => l.label || l.filename || (l.embedded ? LKT.tAttach + (l.bin_index != null ? ' ' + (l.bin_index + 1) : '') : lkHost(l.uri) || l.uri);
  const lkSub = l => [l.embedded ? LKT.tEmbedded : (l.label || l.filename ? lkHost(l.uri) : l.uri.replace(/^https?:\/\/(www\.)?/i, '').replace(/\/$/, '')), l.fmt && l.embedded ? l.fmt : '', l.size ? lkSize(l.size) : '', l.kind === 'conference' && l.features && l.features.length ? l.features.map(x => x.toLowerCase()).join(', ') : ''].filter(Boolean).join(' · ');
  const lkIcon = l => l.embedded || l.kind === 'attach' ? ((l.fmt || '').startsWith('image/') || /\.(jpe?g|png|gif|webp|heic)$/i.test(l.filename || l.uri) ? 'i-image' : 'i-clip') : l.kind === 'conference' ? 'i-chat' : 'i-link';
  const lkMirror = () => { f.links_json.value = LK.links.length || LK.related.length ? JSON.stringify([LK.links.map(l => [l.kind, l.uri, l.label, l.filename, l.keep, !!l.data]), LK.related.map(r => r.uid)]) : ''; };
  function linksInit(links, related) { LK = { links: (links || []).map(l => ({ ...l, keep: l.embedded ? true : false })), related: (related || []).map(r => ({ ...r })) }; lkSel = null; lkMsg.textContent = ''; lkRender(); }
  const lkSync = () => { lkMirror(); lkRender(); refresh(); };
  const linksOf = () => ({ links: LK.links.map(({ _rule, _mode, ...l }) => ({ ...l, features: l.features || [], extra: l.extra || [] })), related: LK.related.map(({ target, ...r }) => ({ ...r, extra: r.extra || [] })) });
  const lkMeeting = u => { const p = lkProvider(u); let id = '', pwd = false; try { const x = new URL(u); pwd = x.searchParams.has('pwd') || x.searchParams.has('passcode'); const m = x.pathname.match(/\/j\/(\d{9,11})/) || x.pathname.match(/\/([a-z]{3}-[a-z]{4}-[a-z]{3})$/) || x.pathname.match(/\/([A-Za-z0-9._-]{6,})$/); if (m) id = m[1].replace(/^(\d{3})(\d{4})(\d{4})$/, '$1 $2 $3'); } catch {} return { provider: p || LKT.tConf, id, pwd }; };
  const lkActs = (i, kind) => { const l = LK.links[i]; const locked = !!l._locked && !ruleMode; const view = l.embedded ? (f.key.value && l.bin_index != null ? [['download', 'i-download', LKT.tDownload]] : []) : [['open', 'i-ext', LKT.tOpen], ['copy', 'i-copy', LKT.tCopy]]; return `<span class="lk-acts">${view.map(([a, ic, t]) => `<button class="act views" type="button" data-lact="${a}" data-i="${i}" title="${esc(t)}"><svg class="ico"><use href="${practical.sprite}#${ic}"/></svg></button>`).join('')}${locked ? '' : `<button class="act edits${lkSel === i ? ' on' : ''}" type="button" data-lact="edit" data-i="${i}" title="${esc(LKT.tEdit)}"><svg class="ico"><use href="${practical.sprite}#i-dots"/></svg></button><button class="act edits" type="button" data-lact="rm" data-i="${i}" title="${esc(LKT.tRemove)}"><svg class="ico"><use href="${practical.sprite}#i-x"/></svg></button>`}</span>`; };
  const lkRow = i => { const l = LK.links[i]; return `<div class="lk-row${lkSel === i ? ' sel' : ''}${l._locked ? ' locked' : ''}"><svg class="ico"><use href="${practical.sprite}#${lkIcon(l)}"/></svg><span class="lk-t" title="${esc(l.uri || l.filename)}">${esc(lkTitle(l))}</span><span class="lk-s">${esc(lkSub(l))}</span>${lkActs(i)}</div>`; };
  const lkRelRow = i => { const r = LK.related[i], t = r.target; return `<div class="lk-row"><svg class="ico"><use href="${practical.sprite}#${t && t.kind === 'task' ? 'i-tasks' : 'i-calendar'}"/></svg><span class="lk-t">${esc(t ? t.summary || r.uid : r.uid)}</span><span class="lk-s">${esc(t ? [t.kind === 'task' ? LKT.tTask : LKT.tEvent, t.date ? fmtD(t.date) : '', t.done ? LKT.tDone : ''].filter(Boolean).join(' · ') : LKT.tNotfound)}</span><span class="lk-acts">${t ? `<button class="act views" type="button" data-lact="goto" data-i="${i}" title="${esc(LKT.tOpen)}"><svg class="ico"><use href="${practical.sprite}#i-ext"/></svg></button>` : ''}<button class="act edits" type="button" data-lact="rmrel" data-i="${i}" title="${esc(LKT.tRemove)}"><svg class="ico"><use href="${practical.sprite}#i-x"/></svg></button></span></div>`; };
  const lkSec = (icon, title, body) => body ? `<div class="lk-sec"><h4><svg class="ico"><use href="${practical.sprite}#${icon}"/></svg> ${esc(title)}</h4>${body}</div>` : '';
  function lkRender() {
    lkMirror();
    const idx = LK.links.map((l, i) => i);
    const conf = idx.filter(i => LK.links[i].kind === 'conference'), web = conf.filter(i => /^https?:/i.test(LK.links[i].uri)), tel = conf.filter(i => /^tel:/i.test(LK.links[i].uri));
    let c = '';
    if (web.length) { const l = LK.links[web[0]], m = lkMeeting(l.uri); c += `<div class="lk-join"><button class="btn" type="button" data-lact="open" data-i="${web[0]}">${ico('headset')}<span>${esc(LKT.tJoin)}</span></button><b>${esc(l.label && l.label !== m.provider ? l.label : LKT.tMeeting.replace('{provider}', m.provider))}</b><small>${esc([m.id ? LKT.tId + ' ' + m.id : '', m.pwd ? LKT.tPwd : '', (l.features || []).map(x => x.toLowerCase()).join(' + ')].filter(Boolean).join(' · ') || lkHost(l.uri))}</small></div>`; const kv = [[LKT.tLink, l.uri.replace(/^https?:\/\//, '').slice(0, 40) + (l.uri.length > 48 ? '…' : '')], ...tel.map(i => [LKT.tPhone, LK.links[i].uri.replace(/^tel:/i, '') + (LK.links[i].label ? ' · ' + LK.links[i].label : '')])]; c += `<div class="lk-kv">${kv.map(([k, v]) => `<span>${esc(k)}</span><span>${esc(v)}</span>`).join('')}</div>`; }
    c += conf.filter(i => i !== web[0] && !tel.includes(i)).map(lkRow).join('') + (web.length ? lkRow(web[0]) .replace('class="lk-row', 'class="lk-row edits') : '');
    if (!conf.length) c = `<button class="lk-pill edits" type="button" data-lact="jitsi"><svg class="ico"><use href="${practical.sprite}#i-plus"/></svg> ${esc(LKT.tJitsi)}</button>`;
    const internal = LK.related.map((r, i) => lkRelRow(i)).join('');
    const internet = idx.filter(i => LK.links[i].kind !== 'conference').map(lkRow).join('');
    const total = LK.links.filter(l => l.embedded && l.data).reduce((a, l) => a + (l.size || 0), 0);
    lkList.innerHTML = (conf.length || form.classList.contains('mode-edit') ? lkSec('i-chat', LKT.tConf, c) : '') + lkSec('i-calendar', LKT.tInternal, internal) + lkSec('i-globe', LKT.tInternet, internet);
    if (!conf.length && form.classList.contains('mode-edit')) $('.lk-sec', lkList).classList.add('edits'); /* the empty conference section is edit-only; in view mode it is not rendered, so the first section is a real one */
    $('#ed-lk-drop').title = total ? LKT.tTotal.replace('{n}', lkSize(total)) : '';
    lkDetailRender();
  }
  function lkDetailRender() {
    const l = lkSel != null ? LK.links[lkSel] : null;
    lkDetail.hidden = !l;
    if (!l) { lkDetail.innerHTML = ''; return; }
    lkDetail.innerHTML = `<div class="lk-dh"><span>${esc(LKT.tEdit)}: ${esc(lkTitle(l))}</span><button type="button" data-lact="rm" data-i="${lkSel}">${esc(LKT.tRemove)}</button></div>` + (l.embedded ? `<div class="lk-kv" style="margin:0"><span>${esc(LKT.tAttach)}</span><span>${esc(l.filename || '')}</span><span>${esc(l.fmt ? 'Typ' : '')}</span><span>${esc(l.fmt || '')}</span></div>` : `<div class="field"><input type="url" class="aux mirror" data-lk="uri" value="${esc(l.uri)}" aria-label="${esc(LKT.tAddress)}"><span>${esc(LKT.tAddress)}</span></div><div class="field"><input type="text" class="aux mirror" data-lk="label" value="${esc(l.label || '')}" placeholder="${esc(lkHost(l.uri))}" aria-label="${esc(LKT.tLabel)}"><span>${esc(LKT.tLabel)}</span></div><span class="seg">${[['url', 'i-ext', LKT.tUrl], ['attach', 'i-download', LKT.tAttach], ['conference', 'i-chat', LKT.tConfk]].map(([k, ic, t]) => `<button type="button" data-lk-kind="${k}" aria-pressed="${l.kind === k}"><svg class="ico"><use href="${practical.sprite}#${ic}"/></svg> ${esc(t)}</button>`).join('')}</span>`);
  }
  const lkAddUri = (uri, label) => { uri = uri.trim(); if (!uri) return; if (!/^[a-z][a-z0-9+.-]*:/i.test(uri)) uri = 'https://' + uri; const kind = lkKind(uri); if (LK.links.some(l => l.uri === uri)) return; LK.links.push({ kind, uri, label: label || '', embedded: false, filename: '', fmt: '', size: null, features: kind === 'conference' && /^https?:/i.test(uri) ? ['AUDIO', 'VIDEO'] : [], extra: [], keep: false }); lkSel = null; lkSync(); };
  const lkAddFiles = files => { let total = LK.links.filter(l => l.embedded && l.data).reduce((a, l) => a + (l.size || 0), 0); const errs = []; [...files].forEach(file => { if (file.size > 2 * 1024 * 1024 || total + file.size > 6 * 1024 * 1024) { errs.push(LKT.tToobig.replace('{name}', file.name)); return; } total += file.size; const rd = new FileReader(); rd.onload = () => { LK.links.push({ kind: 'attach', uri: '', label: '', embedded: true, filename: file.name, fmt: file.type || 'application/octet-stream', size: file.size, features: [], extra: [], keep: false, data: String(rd.result).replace(/^data:[^,]*,/, '') }); lkSync(); }; rd.readAsDataURL(file); }); lkMsg.className = 'lk-msg' + (errs.length ? ' err' : ''); lkMsg.textContent = errs.join(' · '); };
  const lkClosePop = () => { if (lkPop) { lkPop.remove(); lkPop = null; } };
  let lkSearchT = 0;
  $('#ed-lk-search').addEventListener('input', e => { clearTimeout(lkSearchT); const q = e.target.value.trim(); if (q.length < 2) { lkClosePop(); return; } lkSearchT = setTimeout(async () => { const r = await practical.req('/api/search?q=' + encodeURIComponent(q)); if (!r.ok) return; const d = r.data; const seen = new Set([current && current.uid, ...LK.related.map(x => x.uid)]); const hits = [...d.events.map(x => ({ kind: 'event', key: x.key, uid: x.uid, summary: x.summary, date: x.date, done: false })), ...d.tasks.map(x => ({ kind: 'task', key: x.key, uid: x.uid, summary: x.summary, date: x.due, done: x.completed }))].filter(h => h.uid && !seen.has(h.uid) && seen.add(h.uid)).slice(0, 8); lkClosePop(); lkPop = document.createElement('div'); lkPop.className = 'rrpop'; lkPop.innerHTML = hits.length ? hits.map((h, i) => `<button type="button" class="lk-hit" data-h="${i}"><svg class="ico"><use href="${practical.sprite}#${h.kind === 'task' ? 'i-tasks' : 'i-calendar'}"/></svg><span>${esc(h.summary)}</span><small>${esc([h.kind === 'task' ? LKT.tTask : LKT.tEvent, h.date ? fmtD(h.date) : ''].filter(Boolean).join(' · '))}</small></button>`).join('') : `<div class="lk-none">${esc(LKT.tNothing)}</div>`; document.body.appendChild(lkPop); placeUnder(lkPop, e.target); lkPop.addEventListener('click', ev => { const b = ev.target.closest('.lk-hit'); if (!b) return; const h = hits[+b.dataset.h]; LK.related.push({ uid: h.uid, reltype: '', extra: [], target: h }); e.target.value = ''; lkClosePop(); lkSync(); }); }, 250); });
  $('#ed-lk-search').addEventListener('blur', () => setTimeout(lkClosePop, 150));
  $('#ed-lk-addbtn').onclick = () => { lkAddUri($('#ed-lk-url').value); $('#ed-lk-url').value = ''; };
  $('#ed-lk-url').addEventListener('keydown', e => { if (e.key === 'Enter') { e.preventDefault(); $('#ed-lk-addbtn').click(); } });
  const lkDrop = $('#ed-lk-drop'), lkFile = $('#ed-lk-file');
  lkDrop.addEventListener('dragover', e => { e.preventDefault(); lkDrop.classList.add('over'); });
  lkDrop.addEventListener('dragleave', () => lkDrop.classList.remove('over'));
  lkDrop.addEventListener('drop', e => { e.preventDefault(); lkDrop.classList.remove('over'); if (e.dataTransfer.files.length) lkAddFiles(e.dataTransfer.files); else { const t = e.dataTransfer.getData('text/uri-list') || e.dataTransfer.getData('text/plain'); if (t) lkAddUri(t.split('\n')[0]); } });
  lkDrop.addEventListener('click', e => { if (e.target !== lkFile) lkFile.click(); });
  lkDrop.addEventListener('keydown', e => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); lkFile.click(); } });
  lkFile.addEventListener('change', () => { lkAddFiles(lkFile.files); lkFile.value = ''; });
  lkDetail.addEventListener('input', e => { const l = LK.links[lkSel]; if (!l || !e.target.dataset.lk) return; l[e.target.dataset.lk] = e.target.value.trim(); lkMirror(); $$('.lk-row', lkList)[0] && lkRenderRowsOnly(); refresh(); });
  const lkRenderRowsOnly = () => { const d = lkDetail.innerHTML; lkRender(); lkDetail.innerHTML = d; };
  lkDetail.addEventListener('click', e => { const k = e.target.closest('[data-lk-kind]'); if (k && lkSel != null) { LK.links[lkSel].kind = k.dataset.lkKind; if (k.dataset.lkKind === 'conference' && !(LK.links[lkSel].features || []).length) LK.links[lkSel].features = ['AUDIO', 'VIDEO']; lkSync(); } });
  LKC.addEventListener('click', e => { const b = e.target.closest('[data-lact]'); if (!b) return; const a = b.dataset.lact, i = +b.dataset.i;
    if (a === 'open') { if (lkSafe(LK.links[i].uri)) window.open(LK.links[i].uri, '_blank', 'noopener'); }
    else if (a === 'copy') practical.copy(LK.links[i].uri).then(ok => { if (!ok) return; b.title = D.tCopied; b.style.color = 'var(--accent)'; setTimeout(() => b.style.color = '', 800); });
    else if (a === 'download') window.open(`/api/events/${f.key.value}/attach/${LK.links[i].bin_index}`, '_blank', 'noopener');
    else if (a === 'edit') { lkSel = lkSel === i ? null : i; lkRender(); }
    else if (a === 'rm') { LK.links.splice(i, 1); lkSel = null; lkSync(); }
    else if (a === 'rmrel') { LK.related.splice(i, 1); lkSync(); }
    else if (a === 'goto') { const t = LK.related[i].target; if (t.kind === 'task') location.href = '/tasks#' + t.key; else if (serialize() !== baseline) { $('#ed-error').textContent = LKT.tSaveFirst; } else openEditor(t.key); }
    else if (a === 'jitsi') { const slug = (ruleMode || f.summary.value || 'raum') /* a rule's room is named after the category */.toLowerCase().normalize('NFD').replace(/[̀-ͯ]/g, '').replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '').slice(0, 40) || 'raum'; lkAddUri(`https://meet.jit.si/${slug}-${Math.random().toString(36).slice(2, 8)}`, 'Jitsi'); } });
  // colour: the colour picker; "like the calendar" means no COLOR property (color_cal ticked)
  const toHex = c => { if (/^#[0-9a-f]{6}$/i.test(c)) return c.toLowerCase(); const el = document.createElement('i'); el.style.color = c; document.body.appendChild(el); const rgb = getComputedStyle(el).color.match(/\d+/g); el.remove(); return rgb && rgb.length >= 3 ? '#' + rgb.slice(0, 3).map(n => (+n).toString(16).padStart(2, '0')).join('') : '#3478f6'; };
  const colorBtn = $('#ed-color-btn');
  function paintColor() { const b = $('#ed-color-btn'); b.style.setProperty('--c', f.color.value); b.querySelector('span').textContent = f.color_cal.checked ? b.dataset.tCal : ''; } // hoisted: syncDot calls it
  const setColor = c => { const own = !!c; f.color_cal.checked = !own; f.color.value = toHex(own ? c : (f.calendar.selectedOptions[0] || {}).dataset?.color || '#3478f6'); f.color.disabled = !own; paintColor(); };
  colorBtn.addEventListener('click', () => {
    const o = f.calendar.selectedOptions[0], kind = ruleMode ? (TASK ? 'category-task' : 'category') : TASK ? 'task' : 'event', title = ruleMode || f.summary.value.trim() || (TASK ? T.newTask || '' : T.newEv || '');
    window.colorPicker.open(colorBtn, { title, sub: tr('js-color-k-' + kind), value: colorOf(), inherit: o ? { hex: toHex(o.dataset.color), text: tr('js-color-inherit-cal', { cal: o.textContent }) } : null,
      self: { kind, key: ruleMode || f.key.value }, onPick: v => { setColor(v); syncDot(); refresh(); } });
  });
  const colorOf = () => f.color_cal.checked ? '' : f.color.value;
  const geoOf = () => { const a = f.lat.value.trim(), b = f.lon.value.trim(); return a !== '' && b !== '' && !isNaN(+a) && !isNaN(+b) ? [+a, +b] : null; };
