#!/usr/bin/env python3
"""Generate src/data/regions.json: country and subdivision names (Unicode CLDR) for every
language of the app — all ISO countries, not only the covered ones, so a country a holiday
source adds later has its names already — the subdivision hierarchy (CLDR supplemental), and
which holiday source covers which country (OpenHolidays, Nager.Date). That coverage is only the
starting point: the app refreshes it itself (domain/holidays.rs `refresh_coverage`).

Run from build/: `python3 tools/gen_regions.py`. Re-run after a CLDR release or when a
language is added to LANGS. Data: Unicode CLDR, Unicode License v3 (attribution kept in the file).
"""
import json, re, sys, urllib.request, datetime, xml.etree.ElementTree as ET

CLDR = "release-48-2"  # pinned: a moving branch would change names under the app's feet
LANGS = ["de", "en", "fr", "es"]
RAW = f"https://raw.githubusercontent.com/unicode-org/cldr/{CLDR}/common"

def get(url, js=False):
    req = urllib.request.Request(url, headers={"Accept": "application/json" if js else "*/*", "User-Agent": "rustical-ui gen_regions"})
    with urllib.request.urlopen(req, timeout=60) as r:
        data = r.read()
    return json.loads(data) if js else data

def iso(cldr_code):  # "desn" → "DE-SN", "fr69m" → "FR-69M"
    return cldr_code[:2].upper() + "-" + cldr_code[2:].upper()

oh = {c["isoCode"] for c in get("https://openholidaysapi.org/Countries", True)}
ng = {c["countryCode"] for c in get("https://date.nager.at/api/v3/AvailableCountries", True)}
print(f"OpenHolidays {len(oh)}, Nager.Date {len(ng)}, together {len(oh | ng)}", file=sys.stderr)

# every ISO 3166-1 country CLDR names (two letters; ZZ, EU, UN … are groupings or placeholders)
NOT_COUNTRIES = {"ZZ", "EU", "EZ", "UN", "QO", "XA", "XB"}
en = ET.fromstring(get(f"{RAW}/main/en.xml"))
codes = {t.get("type") for t in en.iter("territory") if re.fullmatch(r"[A-Z]{2}", t.get("type") or "") and t.get("type") not in NOT_COUNTRIES}
missing = (oh | ng) - codes
if missing:
    sys.exit(f"covered but unknown to CLDR: {sorted(missing)}")
countries = {c: {"oh": int(c in oh), "ng": int(c in ng), "n": {}} for c in sorted(codes)}
subs = {}
root = ET.fromstring(get(f"{RAW}/supplemental/subdivisions.xml"))
for g in root.iter("subgroup"):
    parent = g.get("type")
    parent = parent if len(parent) == 2 else iso(parent)
    if parent[:2] not in countries:
        continue
    for c in g.get("contains").split():
        subs[iso(c)] = {"p": parent, "n": {}}

for lang in LANGS:
    m = ET.fromstring(get(f"{RAW}/main/{lang}.xml"))
    for t in m.iter("territory"):
        if t.get("alt") is None and t.get("type") in countries:
            countries[t.get("type")]["n"][lang] = t.text
    s = ET.fromstring(get(f"{RAW}/subdivisions/{lang}.xml"))
    for d in s.iter("subdivision"):
        code = iso(d.get("type"))
        if code in subs and d.get("alt") is None:
            subs[code]["n"][lang] = d.text

# names every language lacks are dropped from the name map; the app falls back to en, then the code
out = {
    "source": f"Unicode CLDR {CLDR} (Unicode License v3, https://www.unicode.org/license.txt); coverage: openholidaysapi.org, date.nager.at",
    "cldr": CLDR,  # the app compares this with CLDR's latest release (holidays.rs `check_cldr`)
    "generated": datetime.date.today().isoformat(),
    "langs": LANGS,
    "countries": countries,
    "subs": subs,
}
path = "src/data/regions.json"
with open(path, "w") as f:
    json.dump(out, f, ensure_ascii=False, separators=(",", ":"), sort_keys=True)
missing = [c for c, v in countries.items() if "en" not in v["n"]]
print(f"{len(countries)} countries ({len(oh | ng)} covered), {len(subs)} subdivisions → {path}; no English name: {missing}", file=sys.stderr)
