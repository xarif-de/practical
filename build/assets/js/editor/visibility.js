// js/editor/visibility.js — part of the editor IIFE (src/bundle.rs; shares its scope): the visibility slider
  /* ---- visibility: the hidden CLASS select stays the model (dirty tracking, reset, submit and rules work as
     before); the slider is its face. Three steps public → private → confidential; "" (none set) shows as public and
     writes nothing until the slider moves — then PUBLIC too. A value another client wrote (X-SECRET …) stays the
     model until a step is picked: an extra option of the select (clsSet), shown as itself on the private step, which
     is how RustiCal treats it for sharees (rustical_ical visibility.rs `class_level`). In rule mode one more step on
     the left: "beliebig" (= "", the rule leaves it). A rule lock (RLOCK) disables it, and so does view mode (mode()
     renders it again). Below it what sharees see, for a calendar whose `#ed-cal` option carries the calendar's levels
     (data-vis="private,confidential": own with shares, or a share), of the strictest CLASS of the entry and its
     overrides (`class_overrides`), as RustiCal decides. ---- */
  const VIS = $x('#ed-vis'), visRange = $x('#ed-vis-range', VIS), VIS_CLASSES = ['PUBLIC', 'PRIVATE', 'CONFIDENTIAL'];
  // 0 public or none, 1 private or unknown, 2 confidential; the level sharees get is [everything, private, confidential] of the calendar's levels
  const visRank = c => !c || c === 'PUBLIC' ? 0 : c === 'CONFIDENTIAL' ? 2 : 1;
  const visLevel = (cls, pol) => [4, pol[0], pol[1]][Math.max(visRank(cls), visRank(current && current.class_overrides))];
  // the stored CLASS into the select; one the select does not offer becomes its extra option (the last fill's goes)
  function clsSet(v) {
    if (!f.class) return;
    $$('option[data-x]', f.class).forEach(o => o.remove());
    v = v || '';
    if (v && ![...f.class.options].some(o => o.value === v)) { const o = document.createElement('option'); o.value = v; o.textContent = v; o.dataset.x = '1'; f.class.appendChild(o); }
    f.class.value = v;
  }
  function visRender() {
    if (!f.class) return;
    const any = !!ruleMode, view = form.classList.contains('mode-view'), cls = f.class.value, k = VIS_CLASSES.indexOf(cls), i = k >= 0 ? k : cls ? 1 : -1;
    visRange.min = any ? '-1' : '0'; visRange.value = String(i < 0 ? (any ? -1 : 0) : i);
    visRange.disabled = view || RLOCK.has('class');
    const opt = [...f.class.options].find(o => o.value === (cls ? cls : 'PUBLIC'));
    $x('#ed-vis-out', VIS).textContent = any && !cls ? VIS.dataset.tAny : opt ? opt.textContent : cls;
    const o = f.calendar.selectedOptions[0], pol = o && o.dataset.vis ? o.dataset.vis.split(',').map(Number) : null, hint = $x('#ed-vis-hint', VIS);
    hint.hidden = !pol || (any && !cls);
    if (pol) hint.textContent = VIS.dataset.tHint.replace('{what}', VIS.dataset.tLevels.split('|')[visLevel(cls, pol)] || '');
    // the steps over the slider: "beliebig" only in rule mode; each sits over its stop (--i of --n, css card-form.css)
    const shown = $$('.vis-steps button', VIS).filter(b => { b.hidden = !any && b.dataset.i === '-1'; return !b.hidden; });
    shown.forEach((b, k) => { b.style.setProperty('--i', String(k)); b.style.setProperty('--n', String(shown.length)); b.classList.toggle('first', k === 0); b.classList.toggle('last', k === shown.length - 1); b.classList.toggle('on', b.dataset.i === visRange.value); b.disabled = visRange.disabled; });
    VIS.hidden = view && !cls; // nothing to tell in view mode without a CLASS
  }
  // its input and change events reach the form's own handlers (wiring.js: dirty state, rule footers); the calendar
  // select re-renders it there (another calendar's levels), stRender after a fill or a rule, mode() on lock/unlock
  // a step's symbol moves the slider there, as a drag would; on the step it sits on already only when that is another
  // client's value (it shows on "private"): the click picks PRIVATE itself
  $x('.vis-steps', VIS).addEventListener('click', e => { const b = e.target.closest('button[data-i]'); if (!b || b.disabled || (b.dataset.i === visRange.value && (!f.class.value || VIS_CLASSES.includes(f.class.value)))) return; visRange.value = b.dataset.i; ['input', 'change'].forEach(t => visRange.dispatchEvent(new Event(t, { bubbles: true }))); });
  visRange.addEventListener('input', () => { const v = +visRange.value; f.class.value = v < 0 ? '' : VIS_CLASSES[v]; visRender(); });
