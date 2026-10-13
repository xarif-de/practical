#!/usr/bin/env python3
"""Stage 2 of the README showcase DB: one plausible, deliberately uncrowded year (2026) of Emma Clark — a few events
and tasks with only some features each, so the shots look calm (the user's wish, 2026-10-14): work, personal, sports
and a shared family calendar, tasks, contacts, category rules, colour clutter for the colour manager, app tokens and a few undone
changes for the change history — written through PractiCal's own HTTP endpoints, like e2e/seed/02-practical.py, whose
helpers (server start/stop with frozen clock and blocked egress, req/ok, ics/vevent/vtodo/vcard, import_new/import_ab)
this script loads via importlib.

Usage: 02-demo.py <practical-binary> <workdir> <out.sqlite3>   (needs <workdir>/db.sqlite3 from 01-rustical.sh)
Synthetic data only: example.org/example.com mails, fake +49 numbers. No request leaves the host.
"""
import datetime as dt
import importlib.util, json, os, random, re, shutil, sqlite3, sys, urllib.parse

HERE = os.path.dirname(os.path.abspath(__file__))
BIN, WORK, OUT = sys.argv[1], sys.argv[2], sys.argv[3]

# the fixture seed reads its arguments and SEED_PORT at import time
spec = importlib.util.spec_from_file_location("fixture_seed", os.path.join(HERE, "..", "seed", "02-practical.py"))
fx = importlib.util.module_from_spec(spec)
sys.argv = [spec.origin, BIN, WORK, OUT]
spec.loader.exec_module(fx)
fx.ENV.update(PRACTICAL__DEV_LOGIN="emma", PRACTICAL__DEFAULT_LANG="en")
req, ok, ics, vevent, vtodo, vcard = fx.req, fx.ok, fx.ics, fx.vevent, fx.vtodo, fx.vcard

TODAY = dt.date(2026, 10, 14)  # = fx.FROZEN (10:00 Europe/Berlin, a Wednesday)
D = dt.date
ALARM = lambda m: f"BEGIN:VALARM\r\nACTION:DISPLAY\r\nTRIGGER:-PT{m}M\r\nDESCRIPTION:Reminder\r\nEND:VALARM\r\n"


def esc(s):
    return s.replace("\\", "\\\\").replace(",", "\\,").replace(";", "\\;").replace("\n", "\\n")


def ymd(d):
    return d.strftime("%Y%m%d")


def days(a, b):
    """a … b inclusive"""
    while a <= b:
        yield a
        a += dt.timedelta(days=1)


# ---------------------------------------------------------------- holidays (cache rows, English names, no egress)
PUBLIC = [("01-01", "New Year's Day"), ("04-03", "Good Friday"), ("04-06", "Easter Monday"), ("05-01", "Labour Day"), ("05-14", "Ascension Day"),
          ("05-25", "Whit Monday"), ("10-03", "Day of German Unity"), ("10-31", "Reformation Day"), ("11-18", "Day of Prayer and Repentance"),
          ("12-25", "Christmas Day"), ("12-26", "Boxing Day")]
NATIONWIDE = {"New Year's Day", "Good Friday", "Easter Monday", "Labour Day", "Ascension Day", "Whit Monday", "Day of German Unity", "Christmas Day", "Boxing Day"}
SCHOOL = [("02-09", "02-21", "Winter holidays"), ("04-03", "04-10", "Easter holidays"), ("07-04", "08-14", "Summer holidays"),
          ("10-12", "10-24", "Autumn holidays"), ("12-23", "01-02", "Christmas holidays")]


def holiday_rows():
    rows = []
    for y in (2025, 2026, 2027):  # same dates every year: only 2026 is shown
        pub = [{"start": f"{y}-{md}", "end": f"{y}-{md}", "name": n, "kind": "public", "nationwide": n in NATIONWIDE, "source": "openholidays", "regions": []} for md, n in PUBLIC]
        sch = [{"start": f"{y}-{a}", "end": f"{y + (b < a)}-{b}", "name": n, "kind": "school", "nationwide": False, "source": "openholidays", "regions": []} for a, b, n in SCHOOL]
        rows += [("DE", "DE-SN", y, "public2-en", json.dumps(pub)), ("DE", "DE-SN", y, "school2-en", json.dumps(sch))]
    return rows


PUBLIC_DAYS = {D.fromisoformat(f"2026-{md}") for md, _ in PUBLIC}

# ---------------------------------------------------------------- the year
# vacations (category Vacation = the vacation rule): first day, last day (inclusive), title, calendar
VACATIONS = [(D(2026, 2, 14), D(2026, 2, 22), "Skiing in Zell am See", "fam"), (D(2026, 4, 4), D(2026, 4, 9), "Easter on the Baltic coast", "fam"),
             (D(2026, 5, 15), D(2026, 5, 15), "Bridge day", "personal"), (D(2026, 7, 25), D(2026, 8, 9), "Summer in Cornwall", "fam"),
             (D(2026, 10, 24), D(2026, 11, 1), "Autumn break at Lake Garda", "fam"), (D(2026, 12, 24), D(2027, 1, 1), "Christmas with the grandparents", "fam")]
OFF = {d for a, b, *_ in VACATIONS for d in days(a, b)}


def weekly(cal, uid, summary, first, last, wd, t0, t1, extra="", skip=lambda d: False):
    """A weekly series on weekday `wd` (0 = Monday) with EXDATEs on vacation/holiday days (and `skip`)."""
    d = first + dt.timedelta(days=(wd - first.weekday()) % 7)
    ex = []
    while d <= last:
        if d in OFF or d in PUBLIC_DAYS or skip(d):
            ex.append(f"{ymd(d)}T{t0}00")
        d += dt.timedelta(days=7)
    byday = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"][wd]
    exd = "".join(f"EXDATE;TZID=Europe/Berlin:{','.join(ex[i:i + 20])}\r\n" for i in range(0, len(ex), 20))
    start = first + dt.timedelta(days=(wd - first.weekday()) % 7)
    cal.append(vevent(uid, summary, f"{ymd(start)}T{t0}00", f"{ymd(start)}T{t1}00", f"RRULE:FREQ=WEEKLY;BYDAY={byday};UNTIL={ymd(last)}T225959Z\r\n{exd}{extra}"))


def ev(cal, uid, summary, day, t0, t1, extra=""):
    cal.append(vevent(uid, esc(summary), f"{ymd(day)}T{t0}00", f"{ymd(day)}T{t1}00", extra))


def allday(cal, uid, summary, first, last, extra=""):
    cal.append(vevent(uid, esc(summary), ymd(first), ymd(last + dt.timedelta(days=1)), extra, allday=True))


def build_events():
    work, personal, sports, fam = [], [], [], []
    cals = {"work": work, "personal": personal, "sports": sports, "fam": fam}
    W = "CATEGORIES:Work\r\n"

    # ---- recurring
    weekly(work, "standup-mon@demo", "Team stand-up", D(2026, 8, 31), D(2026, 12, 18), 0, "0930", "0945", W)
    weekly(sports, "yoga@demo", "Yoga", D(2026, 1, 8), D(2026, 12, 17), 3, "1900", "2000", "LOCATION:Studio Lotus\\, Alaunstraße 21\r\nCATEGORIES:Sports\r\n")

    # ---- vacations
    for k, (a, b, t, c) in enumerate(VACATIONS):
        allday(cals[c], f"vacation{k}@demo", t, a, b, "CATEGORIES:Vacation\r\nTRANSP:TRANSPARENT\r\n")

    # ---- the showcase week (12–18 October) and the rest of October, by hand
    roadmap = ("LOCATION:Meeting room Elbe\\, 3rd floor\r\nCATEGORIES:Work\r\n"
               "ORGANIZER;CN=Emma Clark:mailto:emma.clark@example.com\r\n"
               "ATTENDEE;CN=Sarah Thompson;ROLE=REQ-PARTICIPANT;PARTSTAT=ACCEPTED:mailto:sarah.thompson@example.com\r\n"
               "ATTENDEE;CN=David Kim;ROLE=REQ-PARTICIPANT;PARTSTAT=ACCEPTED:mailto:david.kim@example.com\r\n"
               "DESCRIPTION:Agree on the three big bets for 2027.\r\n" + ALARM(15))
    ev(work, "roadmap@demo", "Q4 roadmap workshop", D(2026, 10, 14), "1000", "1200", roadmap)
    ev(personal, "lunch-priya@demo", "Lunch with Priya", D(2026, 10, 14), "1230", "1330", "LOCATION:Little India\\, Louisenstraße 42\r\n")
    ev(personal, "dentist@demo", "Dentist", D(2026, 10, 14), "1600", "1645", "COLOR:tomato\r\nLOCATION:Dr. Weber dental practice\\, Bautzner Str. 7\r\n")
    ev(work, "sprint@demo", "Sprint planning", D(2026, 10, 12), "1000", "1130", W)
    ev(work, "qbr@demo", "Quarterly business review", D(2026, 10, 15), "1300", "1500", W)
    ev(personal, "drinks@demo", "Friday drinks", D(2026, 10, 16), "1800", "2000", "")
    ev(fam, "lily-party@demo", "Lily's birthday party", D(2026, 10, 18), "1500", "1800", "COLOR:#ff2d55\r\n")
    ev(work, "interview@demo", "Interview: senior designer", D(2026, 10, 21), "1400", "1500", W)
    allday(sports, "hike-oct@demo", "Hiking in Saxon Switzerland", D(2026, 10, 3), D(2026, 10, 3), "CATEGORIES:Sports\r\n")
    allday(work, "hackathon@demo", "Hackathon", D(2026, 10, 8), D(2026, 10, 9), W)
    ev(fam, "pumpkin@demo", "Pumpkin carving", D(2026, 10, 11), "1500", "1700", "")
    # own colours that look alike (the colour optimiser's material), outside the October shots
    ev(personal, "car-service@demo", "Car service", D(2026, 9, 15), "0800", "0830", "COLOR:#ff9f0a\r\n")
    ev(personal, "physio@demo", "Physiotherapy", D(2026, 11, 3), "0800", "0845", "COLOR:#f44336\r\n")
    ev(personal, "doctor@demo", "Doctor's appointment", D(2026, 11, 19), "0815", "0900", "COLOR:#ff3b30\r\n")
    ev(work, "board@demo", "Board meeting", D(2026, 11, 24), "1500", "1630", W + "COLOR:#0a84ff\r\n")
    # November / December: the next things on the horizon
    allday(work, "websummit@demo", "Web Summit Lisbon", D(2026, 11, 10), D(2026, 11, 12), W + "LOCATION:Lisbon\\, Portugal\r\n")
    allday(fam, "lantern@demo", "St Martin's lantern walk", D(2026, 11, 11), D(2026, 11, 11), "")
    ev(personal, "concert@demo", "Concert: Dresden Philharmonic", D(2026, 11, 21), "1930", "2200", "COLOR:#5856d6\r\nLOCATION:Kulturpalast\r\n")
    allday(fam, "xmas-market@demo", "Striezelmarkt with the kids", D(2026, 12, 5), D(2026, 12, 5), "")
    ev(work, "xmas-party@demo", "Office Christmas party", D(2026, 12, 11), "1800", "2300", W)
    ev(fam, "nativity@demo", "Lily's nativity play", D(2026, 12, 17), "1600", "1700", "")

    # ---- the rest of the year: a few events per week from deterministic pools (no showcase month: October)
    rnd = random.Random(2026)
    work_pool = ["Client workshop – Lumen Analytics", "Design review", "Budget review", "Hiring panel", "Vendor call – Northwind", "Architecture review",
                 "Customer demo", "Sprint retrospective", "Roadmap sync", "Security training", "Product demo", "Quarterly planning", "Partner meeting"]
    personal_pool = [("Haircut", "1700", "1745"), ("Dinner with Tom & Mia", "1930", "2200"), ("Cinema night", "2000", "2230"), ("Theatre: Faust", "1930", "2200"),
                     ("Coffee with Hannah", "1600", "1700"), ("Pottery class", "1830", "2030")]
    weekend_pool = [("fam", "Visit Grandma & Grandpa"), ("sports", "Bike tour along the Elbe"), ("fam", "Zoo with Lily"), ("personal", "Weekend in Prague"),
                    ("sports", "Bouldering with Lukas"), ("fam", "Family BBQ"), ("personal", "Flea market"), ("fam", "Swimming at the lake"), ("personal", "Wedding of Tom & Mia")]
    d, k = D(2026, 1, 5), 0
    while d <= D(2026, 12, 20):
        if d.month != 10 and d not in OFF:
            k += 1
            if d.weekday() < 5 and d not in PUBLIC_DAYS:
                if rnd.random() < 0.2:
                    h = rnd.choice(["1000", "1100", "1300", "1400", "1500"])
                    ev(work, f"w{k}@demo", rnd.choice(work_pool), d, h, f"{int(h[:2]) + 1:02d}{h[2:]}", W)
                if d.weekday() in (2, 4) and rnd.random() < 0.1:
                    t, a, b = rnd.choice(personal_pool)
                    ev(personal, f"p{k}@demo", t, d, a, b)
            elif d.weekday() == 5 and rnd.random() < 0.3:
                c, t = rnd.choice(weekend_pool)
                if t == "Weekend in Prague":
                    allday(cals[c], f"we{k}@demo", t, d, d + dt.timedelta(days=1))
                else:
                    day = d + dt.timedelta(days=rnd.random() < 0.4)
                    ev(cals[c], f"we{k}@demo", t, day, "1100", "1600")
                    if rnd.random() < 0.1:  # a second plan the same day: a busy weekend day
                        t2, c2, a, b = rnd.choice([("Farmers' market", "fam", "0900", "1030"), ("Dinner with Tom & Mia", "personal", "1930", "2200"), ("Morning run", "sports", "0800", "0900")])
                        ev(cals[c2], f"we{k}b@demo", t2, day, a, b)
        d += dt.timedelta(days=1)
    ev(sports, "halfmarathon@demo", "Dresden half marathon", D(2026, 4, 19), "0900", "1130", "CATEGORIES:Sports\r\n")
    ev(sports, "triathlon@demo", "Moritzburg sprint triathlon", D(2026, 6, 14), "0800", "1200", "CATEGORIES:Sports\r\n")
    return cals


# ---------------------------------------------------------------- tasks: (calendar, uid, summary, extra)
def task(due=None, start=None, prio=None, pct=None, status=None, cats=None, done=None, more=""):
    s = ""
    if start:
        s += f"DTSTART;VALUE=DATE:{start}\r\n"
    if due:
        s += (f"DUE;TZID=Europe/Berlin:{due}\r\n" if "T" in due else f"DUE;VALUE=DATE:{due}\r\n")
    if prio:
        s += f"PRIORITY:{prio}\r\n"
    if done:
        s += f"STATUS:COMPLETED\r\nPERCENT-COMPLETE:100\r\nCOMPLETED:{done}\r\n"
    else:
        if status:
            s += f"STATUS:{status}\r\n"
        if pct is not None:
            s += f"PERCENT-COMPLETE:{pct}\r\n"
    if cats:
        s += f"CATEGORIES:{cats}\r\n"
    return s + more


TASKS = [
    ("work", "t-slides@demo", "Prepare Q4 roadmap slides", task("20261014T170000", "20261009", None, 60, "IN-PROCESS", "Work",
     more="DESCRIPTION:Where we are\\, what we learned\\, three bets for 2027.\r\n")),
    ("work", "t-proposal@demo", "Send proposal to Lumen Analytics", task("20261016", None, 1, None, None, "Work")),
    ("work", "t-hotel@demo", "Book hotel for Web Summit", task("20261020", None, None, None, None, "Travel")),
    ("work", "t-q3@demo", "Finish Q3 report", task("20261007", "20260928", None, cats="Work", done="20261007T143000Z")),
    ("todo", "t-plumber@demo", "Call the plumber", task("20261015", None, None, None, None, "Home")),
    ("todo", "t-passport@demo", "Renew passport", task("20261021")),
    ("todo", "t-tax@demo", "Tax return 2025", task("20261031", "20261001", None, 45, "IN-PROCESS", "Finance")),
    ("todo", "t-garage@demo", "Clean out the garage", task()),
    ("todo", "t-electricity@demo", "Pay electricity bill", task("20261005", cats="Home", done="20261005T180000Z")),
    ("fam", "t-party@demo", "Organise Lily's birthday party", task("20261018", "20261005", None, 70, "IN-PROCESS", "Family", more="DESCRIPTION:Invitations sent. Still to do: the cake.\r\n")),
    ("fam", "t-present@demo", "Buy birthday present for Lily", task("20261017", None, None, None, None, "Family")),
    *[({"Work": "work", "Family": "fam"}.get(c, "todo"), f"t-done{k}@demo", t, task(f"2026{md}", cats=c, done=f"2026{md}T{h}0000Z")) for k, (t, md, c, h) in enumerate([
        ("Book ski hotel", "0112", "Travel", "19"), ("Plan Easter trip", "0310", "Travel", "20"), ("Ship release v2.2", "0611", "Work", "14"),
        ("Book Cornwall cottage", "0629", "Travel", "19"), ("Back-to-school shopping", "0905", "Family", "10"), ("Q3 OKR check-in", "0917", "Work", "09")])],
]


# ---------------------------------------------------------------- contacts
def card(uid, fn, n, **kw):
    s = ""
    for key, prop in (("nick", "NICKNAME"), ("org", "ORG"), ("title", "TITLE"), ("bday", "BDAY"), ("anniv", "ANNIVERSARY"), ("cats", "CATEGORIES"), ("note", "NOTE"), ("url", "URL;TYPE=website")):
        if kw.get(key):
            s += f"{prop}:{esc(kw[key]) if key in ('note', 'title') else kw[key]}\r\n"
    for t, v in kw.get("tel", []):
        s += f"TEL;TYPE={t}:{v}\r\n"
    for t, v in kw.get("mail", []):
        s += f"EMAIL;TYPE={t}:{v}\r\n"
    if kw.get("adr"):
        t, street, city, zip_, country = kw["adr"]
        s += f"ADR;TYPE={t}:;;{street};{city};;{zip_};{country}\r\n"
    return vcard(uid, fn, n, s + kw.get("more", ""))


CONTACTS = [
    card("c-james@demo", "James Clark", "Clark;James;;;", bday="19850302", anniv="20120616", cats="Family", tel=[("cell", "+49 151 2345 6701")], mail=[("home", "james.clark@example.org")],
         adr=("home", "Hechtstraße 18", "Dresden", "01097", "Germany"), more="RELATED;TYPE=spouse;VALUE=text:Emma Clark\r\n"),
    card("c-lily@demo", "Lily Clark", "Clark;Lily;;;", bday="20171019", cats="Family", note="Allergic to peanuts."),
    card("c-margaret@demo", "Margaret Clark", "Clark;Margaret;;;", nick="Mum", bday="19581109", cats="Family", tel=[("home", "+49 351 4567 8901")], mail=[("home", "margaret.clark@example.org")],
         adr=("home", "Am Weinberg 4", "Radebeul", "01445", "Germany")),
    card("c-priya@demo", "Priya Sharma", "Sharma;Priya;;;", org="Lumen Analytics GmbH", title="Head of Data Science", bday="19901028", cats="Friends,Work",
         tel=[("cell", "+49 160 9876 5432")], mail=[("work", "priya.sharma@example.com")], adr=("home", "Louisenstraße 42", "Dresden", "01099", "Germany"),
         note="Loves bouldering."),
    card("c-tom@demo", "Tom Becker", "Becker;Tom;;;", bday="19870403", cats="Friends", tel=[("cell", "+49 152 1111 2233")], mail=[("home", "tom.becker@example.org")]),
    card("c-sarah@demo", "Sarah Thompson", "Thompson;Sarah;;;", org="Brightwave Software", title="VP Product", cats="Work", tel=[("work", "+49 351 2000 100")], mail=[("work", "sarah.thompson@example.com")]),
    card("c-david@demo", "David Kim", "Kim;David;;;", org="Brightwave Software", title="Engineering Manager", bday="19860930", cats="Work", tel=[("work", "+49 351 2000 142")], mail=[("work", "david.kim@example.com")]),
    card("c-dentist@demo", "Dr. Anna Weber", "Weber;Anna;;Dr.;", org="Dental practice Dr. Weber", cats="Services,Health", tel=[("work", "+49 351 8012 345")], adr=("work", "Bautzner Str. 7", "Dresden", "01099", "Germany")),
]


def seed():
    ok(req("GET", "/login"), "dev login")  # DEV_LOGIN: the session is emma's
    page = ok(req("GET", "/settings", accept="text/html"), "settings")
    fx.CSRF = re.search(r'<meta name="csrf" content="([^"]+)"', page).group(1)
    ok(req("POST", "/settings/prefs", accept="application/json", form={"lang": "en", "scheme": "auto", "timezone": "Europe/Berlin", "holiday_color": "", "school_color": "",
        "public_regions": "DE-SN", "school_regions": "DE-SN", "vacation_days": "30", "vacation_kind": "category", "vacation_category": "Vacation",
        "vacation_calendar": "", "vacation_title": "", "alarm_snaps_start": "", "alarm_snaps_end": "", "search_history_days": "90"}), "prefs")

    ev = build_events()
    ids = {}
    ids["work"] = fx.import_new("emma", "Work", "#007aff", "VEVENT,VTODO", ics(*ev["work"], *[vtodo(u, esc(s), x) for c, u, s, x in TASKS if c == "work"], name="Work"), "Brightwave Software")
    ids["personal"] = fx.import_new("emma", "Personal", "#34c759", "VEVENT", ics(*ev["personal"], name="Personal"))
    ids["sports"] = fx.import_new("emma", "Sports", "#ff9500", "VEVENT", ics(*ev["sports"], name="Sports"))
    ids["todo"] = fx.import_new("emma", "To-do", "#5856d6", "VTODO", ics(*[vtodo(u, esc(s), x) for c, u, s, x in TASKS if c == "todo"], name="To-do"))
    ids["fam"] = fx.import_new("group.family", "Family", "#af52de", "VEVENT,VTODO", ics(*ev["fam"], *[vtodo(u, esc(s), x) for c, u, s, x in TASKS if c == "fam"], name="Family"))

    ab = fx.import_ab("emma", "Contacts", "#32ade6", "".join(CONTACTS).encode())
    ids["contacts"] = ab

    # category rules: Vacation (all-day, own colour, free), Work (reminder + office), Sports (colour)
    ok(req("PUT", "/api/category-rules/Vacation?scope=event", {"when": {"all_day": {"mode": "lock", "value": True}, "color": {"mode": "default", "value": "#30b46c"}}, "status": {"transp": {"mode": "default", "value": "TRANSPARENT"}}}), "rule Vacation")
    ok(req("PUT", "/api/category-rules/Work?scope=event", {"alarm": {"alarms": [{"mode": "default", "minutes": 15, "action": "DISPLAY"}]}, "where": {"mode": "default", "location": "Brightwave Software, Königsbrücker Str. 12, Dresden"}}), "rule Work")
    ok(req("PUT", "/api/category-rules/Sports?scope=event", {"when": {"color": {"mode": "default", "value": "#ff9f0a"}}}), "rule Sports")
    ok(req("PUT", "/api/alarm-presets", [{"action": "DISPLAY", "minutes": 0}, {"action": "DISPLAY", "minutes": 15}, {"action": "DISPLAY", "minutes": 60}, {"action": "DISPLAY", "minutes": 1440}]), "presets")
    for q in ("roadmap", "priya", "tax"):
        ok(req("POST", "/api/search/history", {"q": q}), f"history {q}")
    # app tokens for the account page (docs picture): ids and dates pinned by fx.pin
    for name in ("Pixel 9 (DAVx⁵)", "MacBook (Thunderbird)", "iPhone (Calendar)"):
        ok(req("POST", "/settings/tokens", accept="application/json", form={"name": name}), f"token {name}")

    # UI changes for the change history (docs picture), none of them visible in another shot: an edit undone, an
    # event created and deleted; the undo is the newest change (the one the history list opens with)
    cal = f"/api/events/emma/{ids['personal']}"
    concert = json.loads(ok(req("GET", f"{cal}/concert@demo"), "get concert"))
    edit = {**concert, "location": "Kulturpalast, Schloßstraße 2", "alarms": [{"action": "DISPLAY", "minutes": 120}]}
    ok(req("PUT", f"{cal}/concert@demo", edit), "edit concert")
    edited = max(c["id"] for c in json.loads(ok(req("GET", "/api/history", accept="application/json"), "history"))["changes"])
    made = json.loads(ok(req("POST", "/api/events", {"principal": "emma", "cal_id": ids["personal"], "summary": "Haircut", "location": "Salon Schnittpunkt",
        "description": "", "start": "2026-10-27T17:00:00", "end": "2026-10-27T17:45:00", "all_day": False, "timezone": "Europe/Berlin"}), "create haircut"))
    ok(req("DELETE", f"{cal}/{made['id']}"), "delete haircut")
    ok(req("POST", f"/api/history/{edited}/undo", {"confirm": True}), "undo concert edit")
    return ids


def main():
    if not os.path.exists(fx.DB):
        sys.exit("run 01-rustical.sh first")
    fx.stop(fx.start("1"))  # PractiCal's migrations create practical_*; the holiday coverage refresh fails (blocked)
    con = sqlite3.connect(fx.DB)
    con.executemany("INSERT OR REPLACE INTO practical_holidays (country, subdivision, year, kind, json, fetched_at) VALUES (?,?,?,?,?,'2999-01-01 00:00:00')", holiday_rows())
    con.execute("UPDATE practical_holidays SET fetched_at='2999-01-01 00:00:00' WHERE year=0")
    con.commit(); con.close()
    p = fx.start("2")
    try:
        ids = seed()
    finally:
        fx.stop(p)
    # the fixture's pinning: fixed collection ids (uuid order decides some orders on the pages: the day view's calendar
    # list, the colour optimiser's groups — seen differing between two seeds) and fixed clock-derived values
    fx.COLL_IDS = {"work": 1, "personal": 2, "sports": 3, "todo": 4, "fam": 5, "contacts": 11}
    con = sqlite3.connect(fx.DB)
    fx.pin(con, ids)
    con.execute("PRAGMA wal_checkpoint(TRUNCATE)")
    con.execute("PRAGMA journal_mode = DELETE")
    con.execute("VACUUM")
    con.close()
    shutil.copyfile(fx.DB, OUT)
    print(f"demo DB: {OUT}")


if __name__ == "__main__":
    main()
