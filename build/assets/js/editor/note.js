// js/editor/note.js — part of the editor IIFE (src/bundle.rs; shares its scope): the title + note field
  /* ---- one field (templates/editor.html .tn): its first line is the title (#edh, SUMMARY), below it the note as rich
     text (#ed-note, contenteditable) with a format bar (#ed-tnbar). The save reads two hidden fields that ntSync
     keeps: `description` (plain: the DESCRIPTION every app shows) and `description_html` (the formatted note, '' when
     it holds no formatting; the server writes each app's copy of it, domain/richtext.rs). Whatever comes in —
     typing, paste, a note from Outlook/Thunderbird/Google — passes ntClean, an allowlist that rebuilds the markup
     (the same as richtext::clean), so nothing else reaches the page. A note left untouched goes back as it came
     (ntBody: no html sent, the server keeps the stored copies byte for byte).
     Suggestions while typing come from a lazily loaded index (contacts, categories, events, tasks) even without a
     prefix; picking one inserts @Name / #Category / !Title. Detected persons, links and categories are offered as
     chips that create the entry in the matching card. The view mode makes links, mentions, phone numbers and e-mail
     addresses clickable. ---- */
  const NC = $('#ed-tn'), NT = NC.dataset, NE = $('#ed-note'), NBAR = $('#ed-tnbar'), ntFound = $('#ed-note-found');
  /* the note as filled or edited (ntSrc), kept while the view shows it decorated: edit mode and the save start from it,
     never from the view's links and mentions (they would turn a plain note into a formatted one) */
  let ntSrc = '', ntShown = false;
  let NTX = null, ntLoading = null, ntPop = null, ntTimer = 0, ntTouched = false, ntStale = false;
  const ntIndex = () => NTX ? Promise.resolve(NTX) : (ntLoading ||= fetch('/api/mentions').then(r => r.ok ? r.json() : { contacts: [], categories: [], events: [], tasks: [] }).then(d => (NTX = d)).catch(() => { ntLoading = null; return { contacts: [], categories: [], events: [], tasks: [] }; }));
  const ntInvalidate = () => { NTX = null; ntLoading = null; };
  const { RE_URL, RE_MAIL, RE_TEL, RE_MENTION } = practical.noteRe;

  // ---- the allowlist: tags kept (others give their text), line tags (a paragraph each), dropped with their content
  const NT_KEEP = { B: 'b', STRONG: 'b', I: 'i', EM: 'i', U: 'u', INS: 'u', S: 's', STRIKE: 's', DEL: 's', UL: 'ul', OL: 'ol', LI: 'li', A: 'a', H1: 'h1', H2: 'h2', H3: 'h3', H4: 'h3', H5: 'h3', H6: 'h3' };
  const NT_LINE = /^(P|DIV|BLOCKQUOTE|PRE|SECTION|ARTICLE|HEADER|FOOTER|MAIN|ADDRESS|CENTER|TR|DL|DT|DD|FIGURE|FIGCAPTION|TABLE|TBODY|THEAD|TFOOT|CAPTION)$/;
  const NT_DROP = /^(SCRIPT|STYLE|HEAD|TITLE|META|LINK|BASE|TEMPLATE|IFRAME|FRAME|FRAMESET|OBJECT|EMBED|SVG|MATH|NOSCRIPT|IMG|PICTURE|VIDEO|AUDIO|SOURCE|TRACK|CANVAS|MAP|AREA|INPUT|BUTTON|SELECT|OPTION|TEXTAREA|FORM|DIALOG|XML)$|:/; // ":" = Office's o:p and friends
  const NT_HREF = /^(https?:|mailto:|tel:)/i;
  // a colour that stays (normalised by the browser); black text and a white background are the defaults of the
  // app it came from, not a choice — kept, they would vanish in dark mode
  const ntColor = (c, bg) => { c = String(c || '').trim().toLowerCase(); if (!c || c.length > 40 || /url|var|expression|[^#a-z0-9(),.%\s-]/.test(c) || !CSS.supports('color', c)) return '';
    if (/^(transparent|inherit|initial|unset|currentcolor|windowtext|window|auto)$/.test(c)) return '';
    const probe = document.createElement('i'); probe.style.color = c; const n = probe.style.color.replace(/\s/g, '');
    if (!bg && /^(black|#000|#000000|rgb\(0,0,0\))$/.test(n)) return ''; if (bg && /^(white|#fff|#ffffff|rgb\(255,255,255\))$/.test(n)) return '';
    return probe.style.color; };
  /* rebuilds `html` from the allowlist into a new <div> (DOMParser's document runs and loads nothing). Bold, italic,
     underline, strike from inline styles become tags (Outlook writes spans), colours a <span style> with color /
     background-color only. A line tag inside a line becomes a line break */
  function ntClean(html) {
    // (handler attributes go first: they would not run — CSP, and the walk keeps no attribute — but Chrome reports each one as a CSP violation)
    const doc = new DOMParser().parseFromString(String(html || '').replace(/\son[a-z]+\s*=\s*("[^"]*"|'[^']*'|[^\s>]+)/gi, ''), 'text/html'), out = document.createElement('div');
    const inLine = to => to !== out && !!to.closest('p, li, h1, h2, h3');
    const walk = (from, to) => {
      for (const n of [...from.childNodes]) {
        if (n.nodeType === 3) { const t = n.data.replace(/[\t\n\r ]+/g, ' '); if (t) to.append(t); continue; }
        if (n.nodeType !== 1) continue;
        const tag = n.tagName.toUpperCase(); if (NT_DROP.test(tag)) continue;
        if (tag === 'BR') { to.append(document.createElement('br')); continue; }
        if (tag === 'HR') continue;
        if (tag === 'TD' || tag === 'TH') { walk(n, to); to.append(' '); continue; } // a table's cells: side by side in their row's line
        let host = to; const wrap = t => { const e = document.createElement(t); host.append(e); host = e; return e; };
        if (NT_LINE.test(tag) || tag === 'P') {
          if (inLine(to)) { if (to.lastChild && to.lastChild.nodeName !== 'BR') to.append(document.createElement('br')); walk(n, to); continue; }
          if (to === out || /^(UL|OL)$/.test(to.tagName)) { host = document.createElement('p'); to.append(host); }
        } else if (NT_KEEP[tag]) {
          const t = NT_KEEP[tag];
          if (t === 'a') { const h = (n.getAttribute('href') || '').trim(); if (NT_HREF.test(h)) wrap('a').setAttribute('href', h); }
          else if (/^h\d$/.test(t)) { if (!inLine(to)) wrap(t); }
          else if (t === 'li') { if (!/^(UL|OL)$/.test(to.tagName)) wrap('ul'); wrap('li'); }
          else wrap(t);
        } else if (tag === 'FONT') { const c = ntColor(n.getAttribute('color')); if (c) wrap('span').style.color = c; }
        const st = n.style || {}, fw = st.fontWeight || '', td = st.textDecorationLine || st.textDecoration || '';
        if ((fw === 'bold' || +fw >= 600) && !/^(B|STRONG|H\d)$/.test(tag)) wrap('b');
        if (st.fontStyle === 'italic' && !/^(I|EM)$/.test(tag)) wrap('i');
        if (/underline/.test(td) && !/^(U|INS|A)$/.test(tag)) wrap('u');
        if (/line-through/.test(td) && !/^(S|STRIKE|DEL)$/.test(tag)) wrap('s');
        const c = ntColor(st.color), bg = ntColor(st.backgroundColor, true);
        if (c || bg) { const s = wrap('span'); if (c) s.style.color = c; if (bg) s.style.backgroundColor = bg; }
        walk(n, host);
      }
    };
    walk(doc.body, out);
    // tidy: empty inline wrappers and empty lists go, a line loses a closing <br>
    for (let again = true; again;) { again = false; out.querySelectorAll('b, i, u, s, span, a, ul, ol').forEach(e => { if (!e.textContent.trim() && !e.querySelector('br, li')) { e.replaceWith(...e.childNodes); again = true; } }); }
    out.querySelectorAll('p, h1, h2, h3').forEach(e => { if (!e.textContent.trim() && !e.querySelector('br')) e.remove(); }); // (a list typed inside a line leaves its line empty)
    out.querySelectorAll('p, li, h1, h2, h3').forEach(e => { const l = e.lastChild; if (l && l.nodeName === 'BR' && e.childNodes.length > 1) l.remove(); });
    return out;
  }
  /* the plain text of a cleaned note (DESCRIPTION, for the apps without formatting): a line per block, list items
     as "- " / "1. " (two spaces deeper per level), a link as "text <url>" unless its text is the address (the way
     Outlook writes it). ntWords (the stale check) drops all of that again */
  function ntPlain(root) {
    let out = ''; const nl = () => { if (out && !out.endsWith('\n')) out += '\n'; };
    const go = (n, depth) => { for (const c of n.childNodes) {
      if (c.nodeType === 3) { out += c.data.replace(/ /g, ' '); continue; }
      if (c.nodeType !== 1) continue; const t = c.tagName;
      if (t === 'BR') { out += '\n'; continue; }
      if (t === 'UL' || t === 'OL') { nl(); let i = 0; for (const x of c.children) { nl(); out += '  '.repeat(depth) + (t === 'OL' ? `${++i}. ` : '- '); go(x, depth + 1); } nl(); continue; }
      if (/^(P|H1|H2|H3|LI|DIV)$/.test(t)) { nl(); go(c, depth); nl(); continue; }
      if (t === 'A') { const h = c.getAttribute('href') || '', bare = h.replace(/^(mailto:|tel:)/i, ''), x = c.textContent.trim(); go(c, depth); if (h && x !== h && x !== bare && x.replace(/\/$/, '') !== h.replace(/\/$/, '')) out += ` <${bare}>`; continue; }
      go(c, depth); } };
    go(root, 0); return out.replace(/[ \t]+\n/g, '\n').replace(/\n{3,}/g, '\n\n').trim();
  }
  const ntHasFmt = root => !!root.querySelector('b, i, u, s, ul, ol, a, h1, h2, h3, span');
  // the words of a note: what an app's plain text and our HTML's text share (no link addresses, no list marks,
  // no spacing — each app writes those its own way)
  const ntWords = s => String(s || '').normalize('NFC').replace(/\s*<(https?|mailto|tel):[^>]*>|\s*\((https?|mailto):[^)]*\)|\bhttps?:\/\/\S+/gi, ' ').replace(/^[ \t]*([-*•]|\d+[.)])[ \t]+/gm, '').toLowerCase().replace(/[^\p{L}\p{N}]+/gu, '');
  const ntPlainHtml = t => esc(t).replace(/\n/g, '<br>');

  // the note as it is now into the hidden fields (the save, the dirty check and the detection read those)
  function ntSync() {
    const c = ntClean(ntShown ? ntSrc : NE.innerHTML); f.description.value = ntPlain(c); f.description_html.value = ntHasFmt(c) ? c.innerHTML : '';
    NE.classList.toggle('empty', !NE.textContent.trim() && !NE.querySelector('li'));
  }
  /* fill: the first stored formatted copy (`rich`, richtext::stored) whose words are still the note's (another app's
     plain edit leaves the others behind; Google keeps its HTML in DESCRIPTION itself), else the plain text */
  function ntSet(plain, rich, touched) {
    ntTouched = !!touched; ntStale = false; // touched: a snapshot of unsaved work (snapDetail) comes back
    $$('[aria-pressed]', NBAR).forEach(b => b.setAttribute('aria-pressed', 'false')); // a new note starts unformatted at the caret (as a fresh page does)
    const ok = (rich || []).find(r => r.kind === 'google' || ntWords(ntPlain(ntClean(r.html))) === ntWords(plain));
    ntStale = !!(rich && rich.length && !ok); // the copies are stale: the next save of the note drops them
    NE.innerHTML = ok ? ntClean(ok.html).innerHTML : ntPlainHtml(plain || ''); ntShown = false;
    ntSync(); ntFound.innerHTML = '';
  }
  // the note in a snapshot (rule-editor.js snapDetail): its text, formatting and whether it was changed
  const ntSnap = () => { ntSync(); return { description: f.description.value, description_rich: f.description_html.value ? [{ kind: 'ours', html: f.description_html.value }] : [], description_touched: ntTouched || ntStale }; };
  // the note in the save: untouched → the stored text and no html (the server keeps the stored lines); else both
  const ntBody = () => { ntSync(); const keep = !ntTouched && !ntStale && current && !form.classList.contains('new');
    return keep ? { description: current.description || '', description_html: null } : { description: f.description.value, description_html: f.description_html.value }; };

  // ---- typing: paste cleaned, Enter/↓ in the title goes down into the note, Backspace/↑ at the note's start up
  try { document.execCommand('defaultParagraphSeparator', false, 'p'); } catch (_) {}
  const ntInput = () => { ntTouched = true; ntSync(); };
  NE.addEventListener('input', () => { ntInput(); ntSuggest(); clearTimeout(ntTimer); ntTimer = setTimeout(ntDetect, 300); });
  NE.addEventListener('paste', e => { const d = e.clipboardData; if (!d || NE.contentEditable !== 'true') return; e.preventDefault(); const h = d.getData('text/html');
    if (h) document.execCommand('insertHTML', false, ntClean(h).innerHTML); else document.execCommand('insertText', false, d.getData('text/plain')); });
  NE.addEventListener('drop', e => { if (e.dataTransfer && e.dataTransfer.types.includes('text/html')) e.preventDefault(); }); // dropped markup would pass by ntClean
  const ntAtStart = () => { const s = getSelection(); if (!s.rangeCount || !s.isCollapsed || !NE.contains(s.anchorNode)) return false; const r = document.createRange(); r.selectNodeContents(NE); r.setEnd(s.anchorNode, s.anchorOffset); return !r.toString().length && !r.cloneContents().querySelector('br, li'); };
  const ntCaretTo = start => { NE.focus(); const r = document.createRange(); r.selectNodeContents(NE); r.collapse(start); const s = getSelection(); s.removeAllRanges(); s.addRange(r); };
  f.summary.addEventListener('keydown', e => {
    if (e.defaultPrevented || e.isComposing || NE.contentEditable !== 'true') return;
    if ((e.key === 'Enter' && !e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey) || (e.key === 'ArrowDown' && !e.shiftKey && !e.altKey)) { e.preventDefault(); ntCaretTo(true); }
  });
  NE.addEventListener('keydown', e => {
    if (ntPop && (e.key === 'Tab' || e.key === 'Enter') && !e.metaKey && !e.ctrlKey) { e.preventDefault(); ntPop._pick(); return; }
    if (e.key === 'Escape' && ntPop) { e.stopPropagation(); ntClosePop(); return; }
    if ((e.key === 'Backspace' || e.key === 'ArrowUp') && !e.shiftKey && !e.metaKey && !e.ctrlKey && !e.altKey && ntAtStart() && !f.summary.readOnly) { e.preventDefault(); f.summary.focus(); const n = f.summary.value.length; f.summary.setSelectionRange(n, n); }
  });
  NE.addEventListener('blur', () => setTimeout(ntClosePop, 150));

  // ---- the format bar: its buttons keep the selection (mousedown), their state follows the caret
  const NT_CMDS = { bold: 'bold', italic: 'italic', underline: 'underline', strike: 'strikeThrough', ul: 'insertUnorderedList', ol: 'insertOrderedList' };
  const NT_COLORS = ['#d0302f', '#e8710a', '#b78a00', '#1e8e3e', '#1a73e8', '#7e3ff2', '#6e6e73'], NT_MARKS = ['#fff2a8', '#ffd8a8', '#c8f2c2', '#c9e3ff', '#f2d0ff'];
  let ntBarPop = null, ntRange = null; // the open popover; the selection it acts on (its own input takes the focus)
  const ntBarClose = () => { if (ntBarPop) { ntBarPop.remove(); ntBarPop = null; } };
  const ntKeep = () => { const s = getSelection(); ntRange = s.rangeCount && NE.contains(s.anchorNode) ? s.getRangeAt(0).cloneRange() : null; };
  const ntBack = () => { NE.focus(); if (ntRange) { const s = getSelection(); s.removeAllRanges(); s.addRange(ntRange); } };
  const ntExec = (c, v) => { if (NE.contentEditable !== 'true') return; if (!NE.contains(document.activeElement) && document.activeElement !== NE) ntBack(); document.execCommand('styleWithCSS', false, c === 'foreColor' || c === 'hiliteColor'); document.execCommand(c, false, v); document.execCommand('styleWithCSS', false, false); ntInput(); ntBarState(); refresh(); updateButtons(); };
  function ntBarState() { if (NE.contentEditable !== 'true') return; for (const [k, c] of Object.entries(NT_CMDS)) { const b = $(`[data-fmt=${k}]`, NBAR); if (b) { let on = false; try { on = document.queryCommandState(c); } catch (_) {} b.setAttribute('aria-pressed', on ? 'true' : 'false'); } } }
  document.addEventListener('selectionchange', () => { if (document.activeElement === NE) { ntBarState(); ntKeep(); } });
  function ntBarOpen(b, html, on) { ntBarClose(); ntBarPop = document.createElement('div'); ntBarPop.className = 'rrpop tn-pop'; ntBarPop.innerHTML = html; document.body.appendChild(ntBarPop); placeUnder(ntBarPop, b); ntBarPop.addEventListener('mousedown', e => { if (!e.target.closest('input')) e.preventDefault(); }); on(ntBarPop); }
  NBAR.addEventListener('mousedown', e => { if (e.target.closest('button')) e.preventDefault(); });
  NBAR.addEventListener('click', e => {
    const b = e.target.closest('button[data-fmt]'); if (!b || NE.contentEditable !== 'true') return; const k = b.dataset.fmt;
    if (NT_CMDS[k]) return ntExec(NT_CMDS[k]);
    if (k === 'clear') { ntExec('removeFormat'); ntExec('unlink'); ntExec('formatBlock', 'p'); return; }
    if (k === 'heading') return ntBarOpen(b, ['p', 'h1', 'h2', 'h3'].map(t => `<button type="button" class="lk-hit tn-h tn-h-${t}" data-v="${t}"><span>${esc(t === 'p' ? NT.tNormal : NT['tH' + t[1]])}</span></button>`).join(''), p => p.addEventListener('click', ev => { const x = ev.target.closest('[data-v]'); if (!x) return; ntBarClose(); ntBack(); ntExec('formatBlock', x.dataset.v); }));
    if (k === 'color' || k === 'mark') { const list = k === 'color' ? NT_COLORS : NT_MARKS, cmd = k === 'color' ? 'foreColor' : 'hiliteColor';
      return ntBarOpen(b, `<div class="tn-sw">${list.map(c => `<button type="button" data-c="${c}" style="--c:${c}" aria-label="${c}"></button>`).join('')}<button type="button" data-c="" class="none" title="${esc(NT.tNone)}" aria-label="${esc(NT.tNone)}">∅</button></div>`, p => p.addEventListener('click', ev => { const x = ev.target.closest('[data-c]'); if (!x) return; ntBarClose(); ntBack();
        ntExec(cmd, x.dataset.c || (k === 'color' ? 'inherit' : 'transparent')); })); }
    if (k === 'link') { const a = ntRange && ntRange.startContainer, el = a && (a.nodeType === 1 ? a : a.parentElement), cur = el && el.closest('a');
      return ntBarOpen(b, `<div class="tn-link"><input type="url" placeholder="${esc(NT.tLinkPh)}" aria-label="${esc(NT.tLink)}" value="${esc(cur ? cur.getAttribute('href') : '')}">${practical.dbtn('btn-primary', 'check', NT.tOk, 'data-ok')}</div>`, p => { const i = $('input', p); i.focus(); i.select();
        const ok = () => { let u = i.value.trim(); if (u && !NT_HREF.test(u)) u = (/^[^\s@/]+@[^\s@]+$/.test(u) ? 'mailto:' : 'https://') + u; ntBarClose(); ntBack();
          if (!u) ntExec('unlink'); else if (ntRange && ntRange.collapsed && !cur) ntExec('insertHTML', `<a href="${esc(u)}">${esc(u.replace(/^mailto:/, ''))}</a>&nbsp;`); else ntExec('createLink', u); };
        i.addEventListener('keydown', ev => { if (ev.key === 'Enter') { ev.preventDefault(); ok(); } else if (ev.key === 'Escape') { ev.preventDefault(); ev.stopPropagation(); ntBarClose(); ntBack(); } }); $('[data-ok]', p).addEventListener('click', ok); }); }
  });
  document.addEventListener('mousedown', e => { if (ntBarPop && !ntBarPop.contains(e.target) && !NBAR.contains(e.target)) ntBarClose(); });
  addEventListener('keydown', e => { if (e.key === 'Escape' && ntBarPop) { e.preventDefault(); e.stopImmediatePropagation(); ntBarClose(); ntBack(); } }, true);
  // the modes: the note is editable in edit mode only (a rule has none); the view mode renders it with its links
  function ntMode(m) { const on = m === 'edit' && !ruleMode && !form.classList.contains('ro'); ntBarClose(); ntClosePop(); NE.contentEditable = on ? 'true' : 'false'; NE.setAttribute('aria-readonly', on ? 'false' : 'true'); if (on) { NE.innerHTML = ntClean(ntShown ? ntSrc : NE.innerHTML).innerHTML; ntShown = false; ntBarState(); ntDetect(); } else ntRender(); }

  // ---- suggestions at the caret (@ # !)
  const ntWordAtCaret = () => { const s = getSelection(); if (!s.rangeCount || !s.isCollapsed || !NE.contains(s.anchorNode) || s.anchorNode.nodeType !== 3) return null;
    const v = s.anchorNode.data.slice(0, s.anchorOffset).replace(/ /g, ' '), m = /(^|\s)([@#!]?)([^\s@#!]{1,60})$/.exec(v); return m ? { node: s.anchorNode, prefix: m[2], word: m[3], start: v.length - m[2].length - m[3].length, end: s.anchorOffset } : null; };
  const ntClosePop = () => { if (ntPop) { ntPop.remove(); ntPop = null; } };
  async function ntSuggest() {
    const w = ntWordAtCaret();
    if (!w || (w.prefix ? w.word.length < 1 : w.word.length < 3)) { ntClosePop(); return; }
    const ix = await ntIndex(), q = w.word.toLowerCase(), has = s => s.toLowerCase().includes(q);
    const hits = [];
    if (!w.prefix || w.prefix === '@') ix.contacts.filter(c => has(c.name)).slice(0, 4).forEach(c => hits.push({ p: '@', v: c.name, sub: NT.tContact + (c.email ? ' · ' + c.email : '') }));
    if (!w.prefix || w.prefix === '#') ix.categories.filter(c => has(c.name)).slice(0, 3).forEach(c => hits.push({ p: '#', v: c.name, sub: NT.tCat + ' · ' + c.count }));
    if (!w.prefix || w.prefix === '!') { ix.events.filter(e => has(e.summary)).slice(0, 3).forEach(e => hits.push({ p: '!', v: e.summary, sub: NT.tEvent + (e.date ? ' · ' + fmtD(e.date) : '') })); ix.tasks.filter(e => has(e.summary)).slice(0, 2).forEach(e => hits.push({ p: '!', v: e.summary, sub: NT.tTask })); }
    ntClosePop();
    if (!hits.length || ntWordAtCaret()?.word !== w.word) return;
    ntPop = document.createElement('div'); ntPop.className = 'rrpop';
    ntPop.innerHTML = hits.slice(0, 7).map((h, i) => `<button type="button" class="lk-hit" data-h="${i}"><span class="nt-pre">${h.p}</span><span>${esc(h.v)}</span><small>${esc(h.sub)}</small></button>`).join('');
    document.body.appendChild(ntPop);
    const s = getSelection(), rc = s.rangeCount ? s.getRangeAt(0).getBoundingClientRect() : null;
    if (rc && rc.height) placeUnder(ntPop, NE, rc); else placeUnder(ntPop, NE); // under the caret
    ntPop.addEventListener('mousedown', e => e.preventDefault()); // keep the note focused
    ntPop.addEventListener('click', e => { const b = e.target.closest('.lk-hit'); if (b) ntPick(hits[+b.dataset.h]); });
    ntPop._pick = () => ntPick(hits[0]);
  }
  function ntPick(h) { const w = ntWordAtCaret(); if (!w) return; const r = document.createRange(); r.setStart(w.node, w.start); r.setEnd(w.node, w.end); const s = getSelection(); s.removeAllRanges(); s.addRange(r);
    document.execCommand('insertText', false, h.p + h.v + ' '); ntClosePop(); ntInput(); refresh(); ntDetect(); }

  // ---- detection: what the note mentions that the other cards do not have yet
  const chips2 = () => chips(); // the event's categories: ntDetect has a local `chips` of its own
  async function ntDetect() {
    const text = f.description.value; ntFound.innerHTML = '';
    if (!text.trim() || !form.classList.contains('mode-edit') || ruleMode) return;
    const ix = await ntIndex(); if (f.description.value !== text) return;
    const chips = [];
    const present = new Set(peAll().map(p => p.email.toLowerCase()));
    const mentioned = new Set([...text.matchAll(RE_MENTION)].filter(m => m[1] === '@').map(m => m[2].replace(/[.,;:!?]+$/, '').toLowerCase()));
    ix.contacts.filter(c => c.email && !present.has(c.email.toLowerCase())).filter(c => mentioned.has(c.name.toLowerCase()) || (c.name.length >= 5 && new RegExp('(?<![\\p{L}])' + c.name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&') + '(?![\\p{L}])', 'iu').test(text))).slice(0, 4).forEach(c => chips.push({ k: 'person', icon: 'i-users', text: c.name, sub: NT.tPeople, data: c }));
    [...new Set((text.match(RE_URL) || []).map(u => u.replace(/[.,;:!?>]+$/, '')))].filter(u => !LK.links.some(l => l.uri === u)).slice(0, 4).forEach(u => chips.push({ k: 'link', icon: 'i-link', text: u.replace(/^https?:\/\/(www\.)?/i, ''), sub: NT.tLinks, data: u }));
    const have = new Set(chips2().map(c => c.toLowerCase()));
    const tags = new Set([...text.matchAll(RE_MENTION)].filter(m => m[1] === '#').map(m => m[2].replace(/[.,;:!?]+$/, '')));
    ix.categories.filter(c => c.name.length >= 4 && new RegExp('(?<![\\p{L}#])' + c.name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&') + '(?![\\p{L}])', 'iu').test(text)).forEach(c => tags.add(c.name));
    [...tags].filter(t => !have.has(t.toLowerCase())).slice(0, 4).forEach(t => chips.push({ k: 'cat', icon: 'i-tag', text: t, sub: NT.tCats, data: t }));
    [...new Set(text.match(RE_TEL) || [])].slice(0, 2).forEach(t => chips.push({ k: 'tel', icon: 'i-phone', text: t.trim(), sub: NT.tCall, data: 'tel:' + t.replace(/[^\d+]/g, ''), ghost: true }));
    [...new Set(text.match(RE_MAIL) || [])].filter(m => !present.has(m.toLowerCase())).slice(0, 2).forEach(m => chips.push({ k: 'mail', icon: 'i-mail', text: m, sub: NT.tMail, data: 'mailto:' + m, ghost: true }));
    ntFound.innerHTML = chips.map((c, i) => `<button class="pillbtn${c.ghost ? ' ghost' : ''}" type="button" data-nt="${i}"><svg class="ico"><use href="${practical.sprite}#${c.icon}"/></svg><span>${esc(c.text)}</span><small>${esc(c.sub)}</small></button>`).join('');
    ntFound._chips = chips;
  }
  ntFound.addEventListener('click', e => { const b = e.target.closest('[data-nt]'); if (!b) return; const c = ntFound._chips[+b.dataset.nt];
    if (c.k === 'person') { addCard('people'); PEOPLE.attendees.push({ name: c.data.name, email: c.data.email, role: 'REQ-PARTICIPANT', status: '', cutype: '', extra: [], contacts: [{ key: c.data.key, name: c.data.name, sub: c.data.email, initials: c.data.name.split(/\s+/).map(w => w[0]).slice(0, 2).join('').toUpperCase(), color_index: 1 }] }); peopleRender(); }
    else if (c.k === 'link') { addCard('links'); lkAddUri(c.data); }
    else if (c.k === 'cat') { catAdd(c.data); }
    else { location.href = c.data; return; }
    refresh(); ntDetect(); });

  // ---- view mode: the text outside links gets links, phone numbers, e-mail addresses and @ # ! mentions
  //      (practical.noteHtml per text node); web links open in a new tab
  function ntRender() {
    if (!ntShown) ntSrc = NE.innerHTML;
    ntShown = true;
    const c = ntClean(ntSrc), w = document.createTreeWalker(c, NodeFilter.SHOW_TEXT), ts = [];
    for (let n = w.nextNode(); n; n = w.nextNode()) if (!n.parentElement.closest('a')) ts.push(n);
    for (const n of ts) { const h = practical.noteHtml(n.data); if (h !== esc(n.data)) { const t = document.createElement('template'); t.innerHTML = h; n.replaceWith(t.content); } }
    c.querySelectorAll('a[href^="http"]').forEach(a => { a.target = '_blank'; a.rel = 'noopener'; });
    NE.innerHTML = c.innerHTML; NE.classList.toggle('empty', !NE.textContent.trim());
  }
  const ntSearch = v => { const q = $('#q'); if (q) { q.value = v; q.focus(); q.dispatchEvent(new Event('input', { bubbles: true })); } };
  NE.addEventListener('click', async e => { if (NE.contentEditable === 'true') return; const m = e.target.closest('.nt-m'); if (!m) return; const k = m.dataset.k, v = m.dataset.v.toLowerCase();
    if (k === '#') { ntSearch(m.dataset.v); return; }
    const ix = await ntIndex();
    if (k === '@') { const c = ix.contacts.find(x => x.name.toLowerCase() === v); location.href = c ? '/contacts#' + c.key : '/contacts'; }
    else { const ev = ix.events.find(x => x.summary.toLowerCase() === v) || ix.events.find(x => x.summary.toLowerCase().startsWith(v)); const tk = !ev && (ix.tasks.find(x => x.summary.toLowerCase() === v) || ix.tasks.find(x => x.summary.toLowerCase().startsWith(v))); if (ev) openEditor(ev.key); else if (tk) location.href = '/tasks#' + tk.key; else ntSearch(m.dataset.v); } });
