// js/editor/visibility.js — part of the editor IIFE (src/bundle.rs; shares its scope): the visibility slider
  /* ---- visibility (R11): the hidden CLASS select stays the model (dirty tracking, reset, submit and rules work as
     before); the slider is its face. Three steps public → private → confidential; "" (none set) shows as public and
     writes nothing until the slider moves — then PUBLIC too. In rule mode one more step on the left: "beliebig" (= "",
     the rule leaves it). A rule lock (RLOCK) disables it. Below it what sharees see, for a calendar whose `#ed-cal`
     option carries the owner's levels (data-vis="private,confidential": own with shares, or a share). ---- */
  const VIS = $x('#ed-vis'), visRange = $x('#ed-vis-range', VIS), VIS_CLASSES = ['PUBLIC', 'PRIVATE', 'CONFIDENTIAL'];
  // the level sharees get for the CLASS chosen: [private, confidential] of the owner's policy; public is always 4 (everything)
  const visLevel = (cls, pol) => cls === 'PRIVATE' ? pol[0] : cls === 'CONFIDENTIAL' ? pol[1] : 4;
  function visRender() {
    if (!f.class) return;
    const any = !!ruleMode, view = form.classList.contains('mode-view'), cls = f.class.value, i = VIS_CLASSES.indexOf(cls);
    visRange.min = any ? '-1' : '0'; visRange.value = String(i < 0 ? (any ? -1 : 0) : i);
    visRange.disabled = view || RLOCK.has('class');
    const opt = [...f.class.options].find(o => o.value === (i < 0 ? 'PUBLIC' : cls));
    $x('#ed-vis-out', VIS).textContent = any && i < 0 ? VIS.dataset.tAny : opt ? opt.textContent : cls;
    const o = f.calendar.selectedOptions[0], pol = o && o.dataset.vis ? o.dataset.vis.split(',').map(Number) : null, hint = $x('#ed-vis-hint', VIS);
    hint.hidden = !pol || (any && i < 0);
    if (pol) hint.textContent = VIS.dataset.tHint.replace('{what}', VIS.dataset.tLevels.split('|')[visLevel(cls, pol)] || '');
    VIS.hidden = view && !cls; // nothing to tell in view mode without a CLASS
  }
  // its input and change events reach the form's own handlers (wiring.js: dirty state, rule footers); the calendar
  // select re-renders it there (another owner's policy), stRender after a fill or a rule
  visRange.addEventListener('input', () => { const v = +visRange.value; f.class.value = v < 0 ? '' : VIS_CLASSES[v]; visRender(); });
