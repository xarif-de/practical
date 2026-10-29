// js/editor/fill.js — part of the editor IIFE (src/bundle.rs; shares its scope): filling the form, openEditor
  // `asNew`: `d` is a template (`tplUse`) — its attributes fill a new event
  function fill(d, date, asNew) {
    const old = d && !asNew; current = old ? d : null; openDate = date; touched = false; form.reset(); clsSet(''); $('#ed-error').textContent = '';
    $$('.card[data-added]', form).forEach(x => x.removeAttribute('data-added')); catsInit([]); $$('.rows .row', form).forEach(x => x.remove());
    // the read-only calendars as the view shown now knows them (its toolbar's data-ro, routes::calendar read_only_cals; a
    // share's access change reloads it): no page reload needed. The server checks every write anyway.
    const roList = $('.toolbar[data-ro]'); if (roList) { const ro = new Set(roList.dataset.ro.split(',')); [...f.calendar.options].forEach(o => { o.disabled = ro.has(o.value); }); }
    f.key.value = old ? `${d.principal}/${d.cal_id}/${d.object_id}` : ''; f.etag.value = old ? d.etag : '';
    form.classList.toggle('new', !old); form.classList.toggle('ro', !!(old && d.read_only)); f.summary.placeholder = old ? D.tTitle : D.tNewtitle;
    const b = $('#ed-save'); b.title = old ? D.tSave : D.tCreate; b.setAttribute('aria-label', b.title);
    if (d && TASK) {
      f.summary.value = d.summary; f.location.value = d.location; f.description.value = d.description; catsInit(d.categories);
      f.calendar.value = `${d.principal}/${d.cal_id}`;
      setStamp('start', d.start, d.start_all_day); setStamp('due', d.due, d.due_all_day); setStamp('completed', d.completed_at, false);
      f.status.value = d.status; f.percent.value = d.percent == null ? 0 : d.percent; pctOut();
      if (d.priority != null) setSel(f.priority, d.priority); clsSet(d.class);
      alarmsInit(d.alarms); linksInit(d.links, d.related); setColor(d.color); stRender();
    } else if (d) {
      f.summary.value = d.summary; f.location.value = d.location; f.description.value = d.description; catsInit(d.categories);
      f.calendar.value = `${d.principal}/${d.cal_id}`; f.date.value = d.start.slice(0, 10); f.end_date.value = d.all_day ? lastDay(d) : d.end.slice(0, 10);
      f.from.value = d.start.slice(11, 16); f.to.value = d.end.slice(11, 16); setAllDay(d.all_day);
      alarmsInit(d.alarms); peopleInit(d.organizer, d.attendees);
      rrInit(d.rrule, d.rrule_until, d.exdates, d.rdates);
      f.status.value = d.status; f.transp.value = d.transp; clsSet(d.class); if (d.priority != null) setSel(f.priority, d.priority); stRender(); linksInit(d.links, d.related); setColor(d.color); if (d.geo) { f.lat.value = d.geo[0]; f.lon.value = d.geo[1]; }
      $('#ed-hint').textContent = d.rrule ? D.tSeries : '';
    } else if (TASK) {
      f.status.value = 'NEEDS-ACTION'; f.percent.value = 0; pctOut(); setColor('');
      if (state.cals) { const first = state.cals.split(',')[0]; if ([...f.calendar.options].some(o => o.value === first)) f.calendar.value = first; }
      stRender();
    } else {
      f.date.value = date || state.date; f.end_date.value = f.date.value; f.from.value = '09:00'; f.to.value = '10:00'; setAllDay(false); rrInit(null, null, [], []); setColor(''); alarmsInit([]); peopleInit(null, []); stRender(); linksInit([], []); $('#ed-hint').textContent = '';
      if (state.cals) { const first = state.cals.split(',')[0]; if ([...f.calendar.options].some(o => o.value === first)) f.calendar.value = first; }
    }
    if (!old && f.calendar.selectedOptions[0] && f.calendar.selectedOptions[0].disabled) { const o = [...f.calendar.options].find(x => !x.disabled); if (o) f.calendar.value = o.value; } // a new entry never lands in a read-only calendar
    locInit(); syncDot(); mode(old ? 'view' : 'edit'); const pre = serialize(); const adjusted = rulesApply(old ? 'open' : 'new'); baseline = adjusted && !(old && d.read_only) ? pre : serialize(); updateButtons(); // a read-only entry cannot be saved: what a lock rule adjusted shows, but is no change that Close then asks to discard
  }
  // `quiet`: a new form that is always on screen (the tasks page) must not take the focus when it appears
  // `only`: open a single card of an existing entry (a table cell that cannot be edited in place)
  async function openEditor(key, date, quiet, only) {
    // a new event needs a calendar it may write into: with none (all read-only, or none at all) every create path
    // (the + button, a day's +, …) ends in this dialog instead of a form that cannot be saved (events::create_event)
    if (!key && !TASK && ![...f.calendar.options].some(o => !o.disabled)) {
      if (await askChoice(tr(f.calendar.options.length ? 'js-nocal-ro' : 'js-nocal-none'), [['go', tr('js-nocal-go')]], tr('js-close')) === 'go') location.href = '/settings#collections'; // the user picks there: create, import or sync a calendar
      return;
    }
    soloEnd();
    if (key) {
      const pre = await practical.prefetched(API + '/' + key); let d = null; try { d = pre && JSON.parse(pre); } catch (_) {} // the calendar's intent (calendar.js)
      if (!d) { const r = await practical.req(API + '/' + key); if (!r.ok) { practical.tell(r); return; } d = r.data; }
      fill(d);
    } else fill(null, date);
    if (key && only) soloStart(only);
    const was = ed.hidden; if (was) opener = document.activeElement; ed.hidden = false;
    // the focus moves into the dialog: a new entry's title, an existing one's unlock button (Enter edits); a read-only
    // entry has none (card-form.css `.cardform.ro #ed-lock`): its close button
    if (!key && !quiet) f.summary.focus(); else if (key && was && !ed.contains(document.activeElement)) $(form.classList.contains('ro') ? '#ed-close' : '#ed-lock').focus();
    dayline();
  }
