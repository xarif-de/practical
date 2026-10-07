#!/usr/bin/env python3
"""The benchmark DB: the e2e fixture plus a large synthetic data set for alice, made through PractiCal's own
HTTP endpoints (import + JSON API, so it does not depend on RustiCal's table layout). Fixed input (seed, UIDs,
dates around 2026-10-14) and a fixed server clock; ids the server assigns (new collections, POSTed objects) differ
between two generations, which is why bench.mjs caches ONE generated file and gives every server a copy of it.

Usage: gen-db.py <practical-binary> <fixture.sqlite3> <out.sqlite3> [port]
Content (alice): calendar "Bench Arbeit" (1,500 VEVENT, 60 of them recurring, a third with VALARM), calendar
"Bench Gemischt" (2,000 VEVENT + 150 VTODO), address book "Bench Kontakte" (300 vCards), then a change history of
API writes (150 event creates, 150 updates, 40 deletes, 20 task creates, 30 contact creates, 50 updates), the
imports' own history entries purged (main()). The server: password login alice/devpass of the fixture, development
mode only for a fixed clock, egress blocked.
"""
import datetime as dt
import http.cookiejar, json, os, random, re, shutil, signal, sqlite3, subprocess, sys, time, urllib.parse, urllib.request, uuid

HERE = os.path.dirname(os.path.abspath(__file__))
BIN, FIXTURE, OUT = sys.argv[1], sys.argv[2], sys.argv[3]
PORT = int(sys.argv[4]) if len(sys.argv) > 4 else 4602
BASE = f"http://127.0.0.1:{PORT}"
BLOCK = "http://127.0.0.1:9"
WORK = OUT + ".work"
DB = os.path.join(WORK, "db.sqlite3")
R = random.Random(20261014)
NOPROXY = urllib.request.ProxyHandler({})
jar = http.cookiejar.CookieJar()
op = urllib.request.build_opener(NOPROXY, urllib.request.HTTPCookieProcessor(jar))
CSRF = ""


def env(now, retention):
    # development mode for its fixed clock only (DEV_NOW needs DEV_ASSETS_DIR; no DEV_LOGIN: the password login as below)
    return dict(PATH=os.environ.get("PATH", ""), HOME=os.environ.get("HOME", ""), RUST_LOG="warn",
        PRACTICAL__DEV_ASSETS_DIR=os.path.join(os.path.dirname(os.path.dirname(HERE)), "build", "assets"), PRACTICAL__DEV_NOW=now,
        PRACTICAL__HISTORY_RETENTION_DAYS=str(retention),
        PRACTICAL__DB_PATH=DB, PRACTICAL__RUSTICAL_URL="", PRACTICAL__BIND=f"127.0.0.1:{PORT}", PRACTICAL__PUBLIC_URL=BASE,
        PRACTICAL__ALLOW_PASSWORD_LOGIN="true", PRACTICAL__HOLIDAYS_COUNTRY="DE", PRACTICAL__HOLIDAYS_SUBDIVISION="DE-SN",
        PRACTICAL__PUBLIC_DAV_URL="https://dav.example.org", PRACTICAL__DEFAULT_TIMEZONE="Europe/Berlin", PRACTICAL__DEFAULT_LANG="de",
        HTTP_PROXY=BLOCK, HTTPS_PROXY=BLOCK, http_proxy=BLOCK, https_proxy=BLOCK, ALL_PROXY=BLOCK, NO_PROXY="", no_proxy="")


def req(method, path, body=None, ctype=None, form=None):
    data, headers = None, {"Origin": BASE, "X-CSRF-Token": CSRF, "Accept": "application/json"}
    if form is not None:
        data = urllib.parse.urlencode(form).encode(); headers["Content-Type"] = "application/x-www-form-urlencoded"
    elif body is not None:
        data = body if isinstance(body, bytes) else json.dumps(body).encode(); headers["Content-Type"] = ctype or "application/json"
    r = urllib.request.Request(BASE + path, data=data, method=method, headers=headers)
    try:
        with op.open(r) as resp:
            t = resp.read().decode()
    except urllib.error.HTTPError as e:
        sys.exit(f"FAIL {method} {path}: {e.code} {e.read().decode()[:300]}")
    try:
        return json.loads(t)
    except ValueError:
        return t


def login():
    global CSRF
    page = op.open(BASE + "/login").read().decode()
    CSRF = re.search(r'name="csrf" value="([^"]+)"', page).group(1)
    op.open(urllib.request.Request(BASE + "/login", data=urllib.parse.urlencode({"username": "alice", "password": "devpass", "csrf": CSRF}).encode(), method="POST", headers={"Origin": BASE}))
    page = op.open(BASE + "/settings").read().decode()
    if 'data-user="alice"' not in page:
        sys.exit("login as alice/devpass failed (fixture without that password?)")
    CSRF = re.search(r'<meta name="csrf" content="([^"]+)"', page).group(1)


WORDS = "Meeting Zahnarzt Sport Projekt Review Einkauf Training Kino Besuch Planung Termin Werkstatt Lauf Konzert Essen Abstimmung Workshop Arzt Elternabend Reise".split()
BASE_DAY = dt.datetime(2026, 10, 14)
ALARM = "BEGIN:VALARM\r\nACTION:DISPLAY\r\nDESCRIPTION:x\r\nTRIGGER:-PT15M\r\nEND:VALARM\r\n"


def uid():
    return str(uuid.UUID(int=R.getrandbits(128)))


VTZ = open(os.path.join(os.path.dirname(HERE), "seed", "vtimezone.ics")).read()


def ics(blocks):
    return ("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//xarif.de//PractiCal bench//DE\r\n" + VTZ + "".join(blocks) + "END:VCALENDAR\r\n").encode()


def vevent(i, tag):
    s = BASE_DAY + dt.timedelta(days=R.randint(-200, 200), hours=R.randint(7, 20), minutes=R.choice([0, 15, 30, 45]))
    e = s + dt.timedelta(minutes=R.choice([30, 60, 90, 120]))
    when = (f"DTSTART;VALUE=DATE:{s:%Y%m%d}\r\nDTEND;VALUE=DATE:{s + dt.timedelta(days=1):%Y%m%d}\r\n" if i % 20 == 0
            else f"DTSTART;TZID=Europe/Berlin:{s:%Y%m%dT%H%M%S}\r\nDTEND;TZID=Europe/Berlin:{e:%Y%m%dT%H%M%S}\r\n")
    return (f"BEGIN:VEVENT\r\nUID:{uid()}\r\nDTSTAMP:20260101T000000Z\r\n{when}SUMMARY:{R.choice(WORDS)} {R.choice(WORDS)} {tag}{i}\r\n"
            f"LOCATION:Raum {i % 17}\r\nDESCRIPTION:Synthetic event {i}\r\nCATEGORIES:{R.choice(WORDS)}\r\n{ALARM if i % 3 == 0 else ''}END:VEVENT\r\n")


def series(i):
    s = BASE_DAY + dt.timedelta(days=R.randint(-300, 0), hours=R.randint(7, 20))
    rr = R.choice(["FREQ=WEEKLY", "FREQ=DAILY;INTERVAL=2", "FREQ=MONTHLY", "FREQ=WEEKLY;BYDAY=MO,WE,FR", "FREQ=YEARLY"])
    return (f"BEGIN:VEVENT\r\nUID:{uid()}\r\nDTSTAMP:20260101T000000Z\r\nDTSTART;TZID=Europe/Berlin:{s:%Y%m%dT%H%M%S}\r\n"
            f"DTEND;TZID=Europe/Berlin:{s + dt.timedelta(hours=1):%Y%m%dT%H%M%S}\r\nRRULE:{rr}\r\nSUMMARY:Serie {R.choice(WORDS)} {i}\r\n{ALARM}END:VEVENT\r\n")


def vtodo(i):
    due = BASE_DAY + dt.timedelta(days=R.randint(-30, 60), hours=12)
    st = "COMPLETED" if i % 4 == 0 else "NEEDS-ACTION"
    return (f"BEGIN:VTODO\r\nUID:{uid()}\r\nDTSTAMP:20260101T000000Z\r\nDUE;TZID=Europe/Berlin:{due:%Y%m%dT%H%M%S}\r\n"
            f"SUMMARY:Aufgabe {R.choice(WORDS)} {i}\r\nSTATUS:{st}\r\nPRIORITY:{R.choice([0, 1, 5, 9])}\r\n{ALARM if i % 5 == 0 else ''}END:VTODO\r\n")


def vcard(i):
    f, l = R.choice("Anna Ben Clara David Eva Felix Greta Hans Ida Jonas Klara Lukas Mia Noah Olga Paul".split()), R.choice("Muster Beispiel Probe Test Synth Demo".split())
    return (f"BEGIN:VCARD\r\nVERSION:3.0\r\nUID:{uid()}\r\nFN:{f} {l} {i}\r\nN:{l};{f};;;\r\nEMAIL:{f.lower()}.{i}@example.org\r\n"
            f"TEL:+49 30 {1000000 + i}\r\nBDAY:19{R.randint(50, 99)}-{R.randint(1, 12):02d}-{R.randint(1, 28):02d}\r\nEND:VCARD\r\n")


def import_cal(name, comps, body):
    q = urllib.parse.urlencode({"owner": "alice", "name": name, "color": "#5856d6", "description": "", "timezone": "Europe/Berlin", "components": comps})
    return req("POST", f"/settings/import/calendars?{q}", body, "text/calendar")["id"]


def imports():
    work = import_cal("Bench Arbeit", "VEVENT", ics([vevent(i, "A") for i in range(1440)] + [series(i) for i in range(60)]))
    mixed = import_cal("Bench Gemischt", "VEVENT,VTODO", ics([vevent(i, "M") for i in range(2000)] + [vtodo(i) for i in range(150)]))
    q = urllib.parse.urlencode({"owner": "alice", "name": "Bench Kontakte", "color": "#ff2d55", "description": ""})
    ab = req("POST", f"/settings/import/addressbooks?{q}", "".join(vcard(i) for i in range(300)).encode(), "text/vcard")["id"]
    return work, mixed, ab


def edits(work, mixed, ab):
    evs = []
    for i in range(150):
        d = BASE_DAY + dt.timedelta(days=R.randint(-80, 120)); h = R.randint(7, 19); cal = R.choice([work, mixed])
        body = {"principal": "alice", "cal_id": cal, "summary": f"Hist {R.choice(WORDS)} {i}", "location": f"Ort {i % 9}", "description": f"note {i}",
                "start": f"{d:%Y-%m-%d}T{h:02d}:00:00", "end": f"{d:%Y-%m-%d}T{h + 1:02d}:00:00", "all_day": False, "categories": [R.choice(WORDS)]}
        evs.append([cal, req("POST", "/api/events", body)["id"], body])
    for n in range(150):
        e = R.choice(evs)
        if e[1] is None:
            continue
        body = dict(e[2], summary=e[2]["summary"] + "+", location=f"Ort {n % 13}")
        if n % 3 == 0:
            body["categories"] = [R.choice(WORDS), R.choice(WORDS)]
        req("PUT", f"/api/events/alice/{e[0]}/{e[1]}", body); e[2] = body
    for _ in range(40):
        e = R.choice(evs)
        if e[1] is not None:
            req("DELETE", f"/api/events/alice/{e[0]}/{e[1]}"); e[1] = None
    for i in range(20):
        req("POST", "/api/tasks", {"principal": "alice", "cal_id": mixed, "summary": f"Hist Aufgabe {i}", "due": f"{BASE_DAY + dt.timedelta(days=R.randint(-10, 40)):%Y-%m-%d}"})
    cts = []
    for i in range(30):
        body = {"principal": "alice", "ab_id": ab, "given": R.choice(["Ada", "Bo", "Cy", "Di"]), "family": f"Hist{i}", "emails": [{"kind": "home", "value": f"h{i}@example.org"}]}
        cts.append(req("POST", "/api/contacts", body)["id"])
    for n in range(50):
        i = R.randrange(len(cts))
        req("PUT", f"/api/contacts/alice/{ab}/{cts[i]}", {"principal": "alice", "ab_id": ab, "given": "Ed", "family": f"Hist{i}x{n}", "emails": [{"kind": "work", "value": f"w{n}@example.org"}]})


def history_caught_up():
    """True once the history's cursor of every collection has reached the collection's synctoken (ingest done)."""
    c = sqlite3.connect(f"file:{DB}?mode=ro", uri=True, timeout=10)
    try:
        behind = c.execute("""SELECT count(*) FROM (SELECT 'cal' AS s, principal, id, synctoken FROM calendars WHERE deleted_at IS NULL UNION ALL SELECT 'ab', principal, id, synctoken FROM addressbooks WHERE deleted_at IS NULL) x
            LEFT JOIN practical_history_cursors h ON h.principal = x.principal AND h.coll = x.id AND h.store = x.s WHERE h.synctoken IS NULL OR h.synctoken < x.synctoken""").fetchone()[0]
        return behind == 0
    finally:
        c.close()


def old_changes_left():
    c = sqlite3.connect(f"file:{DB}?mode=ro", uri=True, timeout=10)
    try:
        return c.execute("SELECT count(*) FROM practical_history_changes WHERE at < '2026-07-01'").fetchone()[0]
    finally:
        c.close()


def stage(now, retention, work):
    """One server run at the fixed clock `now`; work() as alice; then until the history ingest caught up."""
    global jar, op
    jar = http.cookiejar.CookieJar(); op = urllib.request.build_opener(NOPROXY, urllib.request.HTTPCookieProcessor(jar))
    log = open(os.path.join(WORK, "server.log"), "a")
    p = subprocess.Popen([BIN, "serve"], env=env(now, retention), stdout=log, stderr=subprocess.STDOUT)
    try:
        for _ in range(500):
            if p.poll() is not None:
                sys.exit(f"practical exited, see {log.name}")
            try:
                if urllib.request.build_opener(NOPROXY).open(BASE + "/healthz", timeout=1).read() == b"ok":
                    break
            except Exception:
                time.sleep(0.02)  # readiness probe of a starting process
        login()
        res = work()
        for _ in range(3000):  # the history ingest of the last writes (2-s poller); a probe on the DB, ≤ 30 s
            if history_caught_up():
                break
            time.sleep(0.01)
        return res
    finally:
        p.send_signal(signal.SIGINT)
        try:
            p.wait(15)
        except subprocess.TimeoutExpired:
            p.kill(); p.wait()


def main():
    shutil.rmtree(WORK, ignore_errors=True); os.makedirs(WORK)
    shutil.copyfile(FIXTURE, DB)
    t0 = time.time()
    # The imports at 2026-01-01: the next stage's purger (its first round runs at start, retention 90 days) drops
    # their history entries (4,000 items: the /history page alone held ~350 MB RSS after the bench's burst and hid
    # every other memory change). The edits at 2026-10-01 stay; bench.mjs serves with a retention no date reaches.
    ids = stage("2026-01-01T08:00:00Z", 36500, imports)

    def later():
        for _ in range(3000):  # the start-up purge, a probe on the DB, ≤ 30 s
            if old_changes_left() == 0:
                break
            time.sleep(0.01)
        if old_changes_left():
            sys.exit("the import's history was not purged")
        edits(*ids)
    stage("2026-10-01T08:00:00Z", 90, later)
    c = sqlite3.connect(DB)
    c.execute("PRAGMA wal_checkpoint(TRUNCATE)"); c.close()
    shutil.move(DB, OUT + ".tmp"); os.replace(OUT + ".tmp", OUT)
    shutil.rmtree(WORK, ignore_errors=True)
    print(f"gen-db: {OUT} ({os.path.getsize(OUT) >> 20} MiB) in {time.time() - t0:.0f} s", file=sys.stderr)


main()
