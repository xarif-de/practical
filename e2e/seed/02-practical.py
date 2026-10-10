#!/usr/bin/env python3
"""Stage 2 of the e2e fixture: fill the stage-1 store through PractiCal's own HTTP endpoints, then make
every rendered value reproducible: collection and token ids get fixed UUIDs, clock-derived columns get
fixed instants. Two seeds on different days render the same pages (e2e/visual proves it).

Usage: 02-practical.py <practical-binary> <workdir> <out.sqlite3>
Needs <workdir>/db.sqlite3 from 01-rustical.sh. Synthetic data only. The PractiCal process runs with
PRACTICAL__DEV_NOW (the server's clock frozen at FROZEN) and its egress blocked through
HTTP(S)_PROXY=127.0.0.1:9 (holiday/geo/CLDR lookups fail at once; nothing leaves the host).
"""
import datetime as dt
import http.cookiejar, json, os, re, shutil, signal, sqlite3, subprocess, sys, time, urllib.parse, urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
BIN, WORK, OUT = sys.argv[1], sys.argv[2], sys.argv[3]
DB = os.path.join(WORK, "db.sqlite3")
PORT = int(os.environ.get("SEED_PORT", "4111"))
BASE = f"http://127.0.0.1:{PORT}"
FROZEN = dt.datetime(2026, 10, 14, 8, 0, 0)  # UTC = 10:00 Europe/Berlin (CEST), a Wednesday; e2e/lib/env.mjs NOW
SQL_TS = "%Y-%m-%d %H:%M:%S"
BLOCK = "http://127.0.0.1:9"

ENV = dict(os.environ,
    PRACTICAL__DB_PATH=DB, PRACTICAL__RUSTICAL_URL="", PRACTICAL__BIND=f"127.0.0.1:{PORT}",
    PRACTICAL__PUBLIC_URL=BASE, PRACTICAL__ALLOW_PASSWORD_LOGIN="true",
    PRACTICAL__DEV_ASSETS_DIR=os.path.join(REPO, "build", "assets"), PRACTICAL__DEV_LOGIN="alice",
    PRACTICAL__DEV_NOW=FROZEN.strftime("%Y-%m-%dT%H:%M:%SZ"),
    PRACTICAL__HOLIDAYS_COUNTRY="DE",
    PRACTICAL__PUBLIC_DAV_URL="https://dav.example.org", PRACTICAL__DEFAULT_TIMEZONE="Europe/Berlin",
    PRACTICAL__DEFAULT_LANG="de",
    HTTP_PROXY=BLOCK, HTTPS_PROXY=BLOCK, http_proxy=BLOCK, https_proxy=BLOCK, ALL_PROXY=BLOCK, NO_PROXY="", no_proxy="",
    RUST_LOG="info,tower_http=warn")
# urllib in THIS script talks to 127.0.0.1 only; it must not use a proxy from the caller's shell
NOPROXY = urllib.request.ProxyHandler({})


def start(tag):
    log = open(os.path.join(WORK, f"practical-{tag}.log"), "w")
    p = subprocess.Popen([BIN, "serve"], env=ENV, stdout=log, stderr=subprocess.STDOUT)
    op = urllib.request.build_opener(NOPROXY)
    for _ in range(150):
        if p.poll() is not None:
            sys.exit(f"practical exited ({tag}), see {log.name}")
        try:
            if op.open(BASE + "/healthz", timeout=1).read() == b"ok":
                log.flush()
                if "development clock" not in open(log.name).read():  # this checkout's binary honours DEV_NOW
                    p.kill(); sys.exit(f"{BIN} ignores PRACTICAL__DEV_NOW (wrong build?), see {log.name}")
                return p
        except Exception:
            time.sleep(0.2)
    p.kill(); sys.exit(f"practical did not come up ({tag})")


def stop(p):
    p.send_signal(signal.SIGINT)
    try:
        p.wait(15)
    except subprocess.TimeoutExpired:
        p.kill(); p.wait()


# ---------------------------------------------------------------- holidays (cache rows, no egress)
def holiday_rows():
    pub_sn = [("2026-01-01", "Neujahr"), ("2026-04-03", "Karfreitag"), ("2026-04-06", "Ostermontag"), ("2026-05-01", "Tag der Arbeit"),
              ("2026-05-14", "Christi Himmelfahrt"), ("2026-05-25", "Pfingstmontag"), ("2026-10-03", "Tag der Deutschen Einheit"),
              ("2026-10-31", "Reformationstag"), ("2026-11-18", "Buß- und Bettag"), ("2026-12-25", "1. Weihnachtsfeiertag"), ("2026-12-26", "2. Weihnachtsfeiertag")]
    pub_by = [d for d in pub_sn if d[1] not in ("Reformationstag", "Buß- und Bettag")] + [("2026-01-06", "Heilige Drei Könige"), ("2026-06-04", "Fronleichnam"), ("2026-08-15", "Mariä Himmelfahrt"), ("2026-11-01", "Allerheiligen")]
    school_sn = [("2026-02-09", "2026-02-21", "Winterferien"), ("2026-04-03", "2026-04-10", "Osterferien"), ("2026-07-04", "2026-08-14", "Sommerferien"),
                 ("2026-10-12", "2026-10-24", "Herbstferien"), ("2026-12-23", "2027-01-02", "Weihnachtsferien")]

    def pub(lst, y):
        return [{"start": d.replace("2026", str(y), 1), "end": d.replace("2026", str(y), 1), "name": n, "kind": "public",
                 "nationwide": n in ("Neujahr", "Tag der Deutschen Einheit", "1. Weihnachtsfeiertag"), "source": "openholidays", "regions": []} for d, n in lst]

    def school(y):
        return [{"start": a.replace("2026", str(y), 1), "end": b.replace("2026", str(y), 1).replace("2027", str(y + 1), 1), "name": n, "kind": "school",
                 "nationwide": False, "source": "openholidays", "regions": []} for a, b, n in school_sn]
    rows = []
    for lang in ("de", "en", "fr", "es"):  # the cache key carries the page language
        for y in (2025, 2026, 2027):
            rows += [("DE", "DE-SN", y, f"public2-{lang}", json.dumps(pub(pub_sn, y))),
                     ("DE", "DE-BY", y, f"public2-{lang}", json.dumps(pub(pub_by, y))),
                     ("DE", "DE-SN", y, f"school2-{lang}", json.dumps(school(y)))]
    return rows


# ---------------------------------------------------------------- HTTP as alice (DEV_LOGIN session)
jar = http.cookiejar.CookieJar()
op = urllib.request.build_opener(NOPROXY, urllib.request.HTTPCookieProcessor(jar))
CSRF = ""


def req(method, path, body=None, ctype=None, accept=None, form=None):
    data, headers = None, {"Origin": BASE, "X-CSRF-Token": CSRF}
    if form is not None:
        data = urllib.parse.urlencode({**form, "csrf": CSRF}, doseq=True).encode(); headers["Content-Type"] = "application/x-www-form-urlencoded"
    elif body is not None:
        data = body if isinstance(body, bytes) else json.dumps(body).encode(); headers["Content-Type"] = ctype or "application/json"
    if accept:
        headers["Accept"] = accept
    r = urllib.request.Request(BASE + path, data=data, method=method, headers=headers)
    try:
        with op.open(r) as resp:
            return resp.status, resp.read().decode()
    except urllib.error.HTTPError as e:
        return e.code, e.read().decode()


def ok(res, what):
    st, txt = res
    if st >= 400:
        sys.exit(f"FAIL {what}: {st} {txt[:300]}")
    if txt.startswith('{"ok":') or txt.startswith('{"msg":'):
        j = json.loads(txt)
        if j.get("ok") is False:
            sys.exit(f"FAIL {what}: {j}")
    return txt


VTZ = open(os.path.join(HERE, "vtimezone.ics")).read()


def ics(*blocks, name=None):
    head = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//xarif.de//PractiCal seed//DE\r\n" + (f"X-WR-CALNAME:{name}\r\n" if name else "")
    return (head + VTZ + "".join(blocks) + "END:VCALENDAR\r\n").encode()


def vevent(uid, summary, start, end, extra="", allday=False):
    s = f"DTSTART;VALUE=DATE:{start}\r\nDTEND;VALUE=DATE:{end}\r\n" if allday else f"DTSTART;TZID=Europe/Berlin:{start}\r\nDTEND;TZID=Europe/Berlin:{end}\r\n"
    return f"BEGIN:VEVENT\r\nUID:{uid}\r\nDTSTAMP:20261001T080000Z\r\nSUMMARY:{summary}\r\n{s}{extra}END:VEVENT\r\n"


def vtodo(uid, summary, extra=""):
    return f"BEGIN:VTODO\r\nUID:{uid}\r\nDTSTAMP:20261001T080000Z\r\nCREATED:20261001T080000Z\r\nSUMMARY:{summary}\r\n{extra}END:VTODO\r\n"


def vcard(uid, fn, n, extra=""):
    return f"BEGIN:VCARD\r\nVERSION:4.0\r\nUID:{uid}\r\nFN:{fn}\r\nN:{n}\r\n{extra}END:VCARD\r\n"


ALARM15 = "BEGIN:VALARM\r\nACTION:DISPLAY\r\nTRIGGER:-PT15M\r\nDESCRIPTION:Erinnerung\r\nEND:VALARM\r\n"


def import_new(owner, name, color, comps, body, desc=""):
    q = urllib.parse.urlencode({"owner": owner, "name": name, "color": color, "description": desc, "timezone": "Europe/Berlin", "components": comps})
    return json.loads(ok(req("POST", f"/settings/import/calendars?{q}", body, "text/calendar"), f"import {name}"))["id"]


def import_ab(owner, name, color, body, desc=""):
    q = urllib.parse.urlencode({"owner": owner, "name": name, "color": color, "description": desc})
    return json.loads(ok(req("POST", f"/settings/import/addressbooks?{q}", body, "text/vcard"), f"import {name}"))["id"]


def seed_through_http():
    """Everything a user would do in the UI, as alice. Returns {label: collection id}."""
    global CSRF
    ok(req("GET", "/login"), "dev login")  # DEV_LOGIN: the session is alice's
    page = ok(req("GET", "/settings", accept="text/html"), "settings")
    CSRF = re.search(r'<meta name="csrf" content="([^"]+)"', page).group(1)

    # ---- preferences (autosave form; JSON answer). lang "" = none: the browser's Accept-Language picks
    # the page language, so the suite renders de and en without touching the DB.
    ok(req("POST", "/settings/prefs", accept="application/json", form={"lang": "", "scheme": "auto", "timezone": "Europe/Berlin", "holiday_color": "", "school_color": "",
        "public_regions": "DE-SN,DE-BY", "school_regions": "DE-SN", "vacation_days": "30", "vacation_kind": "category", "vacation_category": "Urlaub",
        "vacation_calendar": "", "vacation_title": "", "alarm_snaps_start": "2 min, 5 min, 15 min, 30 min, 1 h, 3 h, 8 h, 1 d, 2 d, 3 d, 1 w, 2 w", "alarm_snaps_end": "5 min, 10 min, 15 min, 30 min, 1 h",
        "search_history_days": "90"}), "prefs")

    # ---- first start: these are long-standing users with data, so the setup is through
    # (domain::onboarding `completed`, which also keeps the language unset — "" above means the
    # suite renders de and en from Accept-Language). Without this the language gate covers every
    # page and nothing is reachable: on 2026-10-09 every a11y and e2e state failed at once.
    ok(req("POST", "/settings/onboarding/complete"), "onboarding complete")

    # ---- calendars (import: object id = UID)
    demo = open(os.path.join(REPO, "build", "tests", "fixtures", "demo-event-full.ics"), "rb").read()
    work = import_new("alice", "Arbeit", "#0a84ff", "VEVENT,VTODO", ics(
        vevent("standup@seed", "Team-Standup", "20260907T090000", "20260907T091500", "RRULE:FREQ=WEEKLY;BYDAY=MO,WE,FR;UNTIL=20261218T225959Z\r\nEXDATE;TZID=Europe/Berlin:20261016T090000\r\nCATEGORIES:Arbeit\r\n" + ALARM15),
        vevent("review@seed", "Projekt-Review", "20261014T100000", "20261014T113000",
               "LOCATION:Rathaus\\, Dr.-Külz-Ring 19\\, 01067 Dresden\r\nGEO:51.049259;13.738144\r\nSTATUS:TENTATIVE\r\nPRIORITY:1\r\nCATEGORIES:Arbeit\r\n"
               "ORGANIZER;CN=Alice Beispiel:mailto:alice@example.org\r\nATTENDEE;CN=Bob Muster;ROLE=REQ-PARTICIPANT;PARTSTAT=ACCEPTED:mailto:bob@example.org\r\n"
               "ATTENDEE;CN=Dora Dahl;ROLE=OPT-PARTICIPANT;PARTSTAT=TENTATIVE:mailto:dora@example.org\r\nATTENDEE;CN=Raum 3;CUTYPE=ROOM;PARTSTAT=ACCEPTED:mailto:raum3@example.org\r\n"
               "CONFERENCE;VALUE=URI;FEATURE=AUDIO,VIDEO;LABEL=Zoom:https://example.zoom.us/j/12345678901\r\nURL:https://example.org/projekt\r\nDESCRIPTION:Agenda:\\n- Status\\n- Risiken @Bob Muster #Arbeit\r\n" + ALARM15),
        vevent("kunde@seed", "Kundentermin", "20261014T103000", "20261014T121500", "CATEGORIES:Kunde\r\n"),
        vevent("lunch@seed", "Mittagessen mit Bob", "20261014T123000", "20261014T133000", "TRANSP:TRANSPARENT\r\n"),
        vevent("messe@seed", "Messe Leipzig", "20261020T080000", "20261022T180000", "LOCATION:Leipziger Messe\r\nCATEGORIES:Arbeit\r\n"),
        vevent("abgesagt@seed", "Abgesagtes Meeting", "20261015T140000", "20261015T150000", "STATUS:CANCELLED\r\n"),
        vevent("spaet@seed", "Spätschicht", "20261015T190000", "20261015T230000", "CLASS:CONFIDENTIAL\r\n"),
        *[vevent(f"busy{i}@seed", f"Termin {i}", f"20261019T1{i}0000", f"20261019T1{i}3000") for i in range(1, 6)],
        vtodo("task-overdue@seed", "Steuererklärung abgeben", "DUE;VALUE=DATE:20261010\r\nPRIORITY:1\r\nCATEGORIES:Haushalt\r\nSTATUS:NEEDS-ACTION\r\n"),
        vtodo("task-today@seed", "Angebot schreiben", "DTSTART;TZID=Europe/Berlin:20261012T090000\r\nDUE;TZID=Europe/Berlin:20261014T170000\r\nSTATUS:IN-PROCESS\r\nPERCENT-COMPLETE:40\r\nPRIORITY:5\r\nRELATED-TO:review@seed\r\n" + ALARM15),
        name="Arbeit"))
    privat = import_new("alice", "Privat", "#30b46c", "VEVENT", ics(
        vevent("urlaub-ostsee@seed", "Urlaub Ostsee", "20261026", "20261031", "CATEGORIES:Urlaub\r\nTRANSP:TRANSPARENT\r\n", allday=True),
        vevent("urlaub-alpen@seed", "Urlaub Alpen", "20260713", "20260725", "CATEGORIES:Urlaub\r\n", allday=True),
        vevent("zahnarzt@seed", "Zahnarzt", "20261014T160000", "20261014T164500", "COLOR:tomato\r\nCLASS:PRIVATE\r\nLOCATION:Praxis Dr. Weiß\\, Hauptstraße 1\\, Dresden\r\n" + ALARM15),
        vevent("oma@seed", "Geburtstag Oma", "20261017", "20261018", "RRULE:FREQ=YEARLY\r\n", allday=True),
        vevent("radtour@seed", "Radtour", "20261024T100000", "20261024T160000", "CATEGORIES:Sport\r\n"),
        name="Privat"))
    ok(req("POST", f"/settings/import/calendars/alice/{privat}", demo, "text/calendar"), "demo event into Privat")
    tasks = import_new("alice", "Aufgaben", "#ff9500", "VTODO", ics(
        vtodo("t-week@seed", "Reifen wechseln", "DUE;VALUE=DATE:20261018\r\nCATEGORIES:Auto\r\nLOCATION:Werkstatt Meier\r\n"),
        vtodo("t-later@seed", "Weihnachtsgeschenke", "DUE;VALUE=DATE:20261130\r\nPRIORITY:9\r\n"),
        vtodo("t-nodate@seed", "Keller aufräumen", "CATEGORIES:Haushalt\r\nDESCRIPTION:Regal\\, Kisten\\, Sperrmüll\r\n"),
        vtodo("t-done@seed", "Rechnung bezahlt", "DUE;VALUE=DATE:20261012\r\nSTATUS:COMPLETED\r\nPERCENT-COMPLETE:100\r\nCOMPLETED:20261012T133000Z\r\n"),
        vtodo("t-done-old@seed", "Fahrrad repariert", "STATUS:COMPLETED\r\nCOMPLETED:20260901T090000Z\r\n"),
        vtodo("t-cancel@seed", "Kurs anmelden", "STATUS:CANCELLED\r\nDUE;VALUE=DATE:20261013\r\n"),
        vtodo("t-range@seed", "Bericht Q4", "DTSTART;VALUE=DATE:20261005\r\nDUE;VALUE=DATE:20261030\r\nSTATUS:IN-PROCESS\r\nPERCENT-COMPLETE:60\r\nURL:https://example.org/bericht\r\n"),
        name="Aufgaben"))
    fam = import_new("group.familie", "Familienkalender", "#af52de", "VEVENT,VTODO", ics(
        vevent("familienfest@seed", "Familienfest", "20261025", "20261026", "", allday=True),
        vevent("elternabend@seed", "Elternabend", "20261021T193000", "20261021T210000", "LOCATION:Grundschule\r\n"),
        vtodo("t-fam@seed", "Kuchen backen", "DUE;VALUE=DATE:20261024\r\n"), name="Familienkalender"))
    old = import_new("alice", "Altes Projekt", "#8e8e93", "VEVENT", ics(vevent("alt@seed", "Altlast", "20260301T100000", "20260301T110000")))
    ok(req("POST", f"/settings/calendars/alice/{old}/delete", accept="application/json", form={}), "trash calendar")

    # ---- address books + contact colour
    cards = "".join([
        vcard("c-anna@seed", "Anna Albrecht", "Albrecht;Anna;;;", "NICKNAME:Anni\r\nTEL;TYPE=cell;PREF=1:+49 151 1234567\r\nTEL;TYPE=work:+49 351 765432\r\nEMAIL;TYPE=home:anna@example.org\r\n"
              "ADR;TYPE=home:;;Lindenweg 5;Dresden;;01099;Deutschland\r\nBDAY:19850617\r\nANNIVERSARY:20100521\r\nORG:Stadtwerke\r\nTITLE:Ingenieurin\r\nCATEGORIES:Freunde,Sport\r\n"
              "URL;TYPE=website:https://anna.example.org\r\nIMPP;X-SERVICE-TYPE=Signal:+491511234567\r\nRELATED;TYPE=spouse;VALUE=text:Ben Albrecht\r\nNOTE:Kennt sich mit Fahrrädern aus.\r\n"),
        vcard("c-ben@seed", "Ben Albrecht", "Albrecht;Ben;;;", "EMAIL;TYPE=work:ben@example.org\r\nBDAY:--10-20\r\nCATEGORIES:Freunde\r\n"),
        vcard("c-dora@seed", "Dora Dahl", "Dahl;Dora;;;", "EMAIL:dora@example.org\r\nTEL:+49 30 111222\r\n"),
        vcard("c-oezil@seed", "Özlem Öztürk", "Öztürk;Özlem;;;", "BDAY:19921015\r\nCATEGORIES:Arbeit\r\n"),
        vcard("c-zora@seed", "Zora Zimmermann", "Zimmermann;Zora;;Dr.;", "ORG:Klinik Nord\r\nEMAIL:zora@example.org\r\n"),
        vcard("c-firma@seed", "Hausverwaltung GmbH", ";;;;", "ORG:Hausverwaltung GmbH\r\nTEL;TYPE=work:+49 351 999\r\n"),
    ])
    ab1 = import_ab("alice", "Freunde", "#30b46c", cards.encode())
    ab2 = import_ab("group.familie", "Familie Adressen", "#af52de", vcard("c-opa@seed", "Opa Otto", "Beispiel;Otto;;;", "BDAY:19400322\r\nTEL:+49 351 4711\r\n").encode())
    ok(req("PUT", f"/api/colors/addressbook/alice/{ab1}", {"color": "#32ade6"}), "addressbook colour")

    # ---- a group created and trashed through the UI, category rules, presets, search history
    ok(req("POST", "/settings/groups", accept="application/json", form={"name": "Verein"}), "group create")
    ok(req("POST", "/settings/groups/group.verein/delete", accept="application/json", form={"confirm": "Verein"}), "group trash")
    ok(req("PUT", "/api/category-rules/Urlaub?scope=event", {"when": {"all_day": {"mode": "lock", "value": True}, "color": {"mode": "default", "value": "#30b46c"}}, "status": {"transp": {"mode": "default", "value": "TRANSPARENT"}}}), "rule Urlaub")
    ok(req("PUT", "/api/category-rules/Arbeit?scope=event", {"alarm": {"alarms": [{"mode": "default", "minutes": 15, "action": "DISPLAY"}]}, "where": {"mode": "default", "location": "Büro, Hauptstraße 10, Dresden"}}), "rule Arbeit")
    ok(req("PUT", "/api/category-rules/Haushalt?scope=task", {"status": {"priority": {"mode": "default", "value": 5}}}), "rule Haushalt")
    ok(req("PUT", "/api/alarm-presets", [{"action": "DISPLAY", "minutes": 0}, {"action": "DISPLAY", "minutes": 15}, {"action": "DISPLAY", "minutes": 1440}]), "presets")
    ok(req("POST", "/api/search/history", {"q": "zahnarzt", "target": {"kind": "event", "key": f"alice/{privat}/zahnarzt@seed", "date": "2026-10-14", "label": "Zahnarzt"}}), "history 1")
    ok(req("POST", "/api/search/history", {"q": "anna"}), "history 2")

    # ---- UI changes for the change history: an edit, a delete, an undo
    ev = json.loads(ok(req("GET", f"/api/events/alice/{privat}/zahnarzt@seed"), "get event"))
    ev["summary"] = "Zahnarzt Dr. Weiß"
    ok(req("PUT", f"/api/events/alice/{privat}/zahnarzt@seed", ev), "edit event")
    ok(req("DELETE", f"/api/events/alice/{work}/abgesagt@seed"), "delete event")
    h = json.loads(ok(req("GET", "/api/history", accept="application/json"), "history"))
    last = max(c["id"] for c in h["changes"])
    ok(req("POST", f"/api/history/{last}/undo", {"confirm": True}), "undo")
    ok(req("POST", "/settings/tokens", accept="application/json", form={"name": "Kalender-App"}), "token via UI")
    return {"Arbeit": work, "Privat": privat, "Aufgaben": tasks, "Familienkalender": fam, "Altes Projekt": old, "Freunde": ab1, "Familie Adressen": ab2}


# fixed ids, in the shape of the uuid4 the app makes (same length on every page)
COLL_IDS = {"Arbeit": 1, "Privat": 2, "Aufgaben": 3, "Familienkalender": 4, "Altes Projekt": 5, "Freunde": 11, "Familie Adressen": 12}


def fixed_uuid(group, n):
    return f"5eed0000-0000-4000-8{group:03d}-{n:012d}"


def replace_everywhere(con, old, new):
    """Every TEXT value in every table: the ids are uuids, unique strings, so a plain replace is exact."""
    for (t,) in con.execute("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'").fetchall():
        for col in con.execute(f'PRAGMA table_info("{t}")').fetchall():
            c = col[1]
            con.execute(f'UPDATE "{t}" SET "{c}" = replace("{c}", ?, ?) WHERE typeof("{c}") = \'text\' AND instr("{c}", ?) > 0', (old, new, old))


def pin(con, ids):
    con.execute("PRAGMA foreign_keys = OFF")
    # every person is a long-standing user here, not a first-time one: the setup is through for all of
    # them (domain::onboarding). Only alice went through it over HTTP above; the others get the record
    # written directly, or the language gate would block every test that logs in as them.
    done = '{"v":1,"confirmed":[],"completed":true}'
    for (pid,) in con.execute("SELECT id FROM principals WHERE principal_type = 'INDIVIDUAL'").fetchall():
        con.execute("INSERT INTO practical_user_settings (principal, onboarding_json) VALUES (?, ?)"
                    " ON CONFLICT(principal) DO UPDATE SET onboarding_json = excluded.onboarding_json", (pid, done))
    remap = {}
    for label, cid in ids.items():
        remap[cid] = fixed_uuid(0, COLL_IDS[label])
    for table, n0 in (("calendars", 100), ("addressbooks", 200)):
        for k, (topic,) in enumerate(con.execute(f"SELECT push_topic FROM {table} ORDER BY displayname").fetchall()):
            remap[topic] = fixed_uuid(1, n0 + k)
    for k, (tid,) in enumerate(con.execute("SELECT id FROM app_tokens ORDER BY rowid").fetchall()):
        remap[tid] = fixed_uuid(2, k + 1)
    for old, new in remap.items():
        replace_everywhere(con, old, new)
    left = [i for i in remap if con.execute("SELECT 1 FROM calendars WHERE id = ? OR push_topic = ? UNION SELECT 1 FROM addressbooks WHERE id = ? OR push_topic = ? UNION SELECT 1 FROM app_tokens WHERE id = ?", (i,) * 5).fetchone()]
    if left:
        sys.exit(f"ids not remapped: {left}")

    # clock-derived values: history oldest 5 h apart, newest 1 h before FROZEN; the rest at fixed instants
    fmt = lambda t: t.strftime(SQL_TS)
    hist = [r[0] for r in con.execute("SELECT id FROM practical_history_changes ORDER BY id")]
    for k, i in enumerate(hist):
        con.execute("UPDATE practical_history_changes SET at = ? WHERE id = ?", (fmt(FROZEN - dt.timedelta(hours=1 + 5 * (len(hist) - 1 - k))), i))
    for k, (rid,) in enumerate(con.execute("SELECT rowid FROM app_tokens ORDER BY rowid").fetchall()):
        con.execute("UPDATE app_tokens SET created_at = ? WHERE rowid = ?", (fmt(FROZEN - dt.timedelta(days=30 - 3 * k)), rid))
    for k, (rid,) in enumerate(con.execute("SELECT id FROM practical_search_history ORDER BY id").fetchall()):
        con.execute("UPDATE practical_search_history SET used_at = ? WHERE id = ?", (fmt(FROZEN - dt.timedelta(hours=2 + k)), rid))
    con.execute("UPDATE practical_trashed_groups SET deleted_at = '2026-10-10T09:00:00+00:00'")
    con.execute("UPDATE calendars SET deleted_at = '2026-10-11 09:00:00' WHERE deleted_at IS NOT NULL")
    stamp = fmt(FROZEN - dt.timedelta(days=1))
    for t, c in (("calendarobjects", "updated_at"), ("addressobjects", "updated_at"), ("calendarobjectchangelog", "created_at"), ("addressobjectchangelog", "created_at"),
                 ("practical_user_settings", "updated_at"), ("practical_category_rules", "updated_at"), ("practical_migrations", "applied_at"), ("_sqlx_migrations", "installed_on")):
        con.execute(f'UPDATE "{t}" SET "{c}" = ?', (stamp,))
    con.commit()
    bad = con.execute("PRAGMA foreign_key_check").fetchall()
    if bad:
        sys.exit(f"foreign key check: {bad[:5]}")
    print(f"history changes: {len(hist)}; ids pinned: {len(remap)}")


def main():
    if not os.path.exists(DB):
        sys.exit("run 01-rustical.sh first")
    # 1st start: PractiCal's own migrations create practical_*; the coverage refresh fails (blocked) and saves the built-in list
    stop(start("1"))
    con = sqlite3.connect(DB)
    con.executemany("INSERT OR REPLACE INTO practical_holidays (country, subdivision, year, kind, json, fetched_at) VALUES (?,?,?,?,?,'2999-01-01 00:00:00')", holiday_rows())
    con.execute("UPDATE practical_holidays SET fetched_at='2999-01-01 00:00:00' WHERE year=0")
    con.commit(); con.close()

    p = start("2")
    try:
        ids = seed_through_http()
        time.sleep(2.5)  # one round of the change poller (tasks.rs, 2 s): its cursors catch up before the stop
    finally:
        stop(p)

    con = sqlite3.connect(DB)
    pin(con, ids)
    con.execute("PRAGMA wal_checkpoint(TRUNCATE)")
    con.execute("PRAGMA journal_mode = DELETE")
    con.execute("VACUUM")
    con.close()
    shutil.copyfile(DB, OUT)
    print(f"fixture: {OUT}")


if __name__ == "__main__":
    main()
