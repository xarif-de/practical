//! Holidays of a year: the source chain (OpenHolidays → Nager.Date → last cached answer), merging several
//! regions, and the day marks the views draw.

use super::*;

pub(super) fn pick(names: &[ApiName], lang: &str) -> String {
    names.iter().find(|n| n.language.eq_ignore_ascii_case(lang)).or_else(|| names.iter().find(|n| n.language.eq_ignore_ascii_case("en"))).or(names.first()).map(|n| n.text.clone()).unwrap_or_default()
}

async fn from_openholidays(state: &AppState, kind: &str, country: &str, subdivision: &str, year: i32, lang: &str) -> Result<Vec<Holiday>, String> {
    let path = if kind == "school" { "SchoolHolidays" } else { "PublicHolidays" };
    let mut url = format!("{OPENHOLIDAYS}/{path}?countryIsoCode={country}&languageIsoCode={}&validFrom={year}-01-01&validTo={year}-12-31", lang.to_uppercase());
    if !subdivision.is_empty() {
        url.push_str(&format!("&subdivisionCode={subdivision}"));
    }
    let items: Vec<ApiHoliday> = get_json(state, &url).await?;
    Ok(items
        .into_iter()
        .map(|h| Holiday { start: h.start_date, end: h.end_date, name: pick(&h.name, lang), kind: kind.to_string(), nationwide: h.nationwide, source: "openholidays".into(), regions: h.subdivisions.into_iter().map(|s| s.code).filter(|c| !c.is_empty()).collect(), tags: vec![] })
        .collect())
}

/// ISO code Nager.Date knows for a stored region: OpenHolidays' deeper codes (CH-AG-ZZ)
/// fall back to their first level (CH-AG).
pub(super) fn nager_region(subdivision: &str) -> String {
    subdivision.splitn(3, '-').take(2).collect::<Vec<_>>().join("-")
}

/// Nager.Date names a holiday in the country's language and in English; a Latin-script local
/// name is kept (a German reader of Austrian holidays wants "Neujahr"), others read English.
pub(super) fn nager_name(h: &NagerHoliday, lang: &str) -> String {
    if lang == "en" || h.local_name.chars().any(|c| c.is_alphabetic() && (c as u32) > 0x024F) { h.name.clone() } else { h.local_name.clone() }
}

async fn from_nager(state: &AppState, country: &str, subdivision: &str, year: i32, lang: &str) -> Result<Vec<Holiday>, String> {
    let items: Vec<NagerHoliday> = get_json(state, &format!("{NAGER}/PublicHolidays/{year}/{country}")).await?;
    let region = nager_region(subdivision);
    Ok(items
        .into_iter()
        .filter(|h| h.types.iter().any(|t| t == "Public"))
        // the whole country: its regional holidays too (tagged with their regions by for_regions)
        .filter(|h| h.global || region.is_empty() || h.counties.as_ref().is_some_and(|c| c.iter().any(|x| *x == region)))
        .map(|h| Holiday { start: h.date, end: h.date, name: nager_name(&h, lang), kind: "public".into(), nationwide: h.global, source: "nager".into(), regions: if h.global { vec![] } else { h.counties.clone().unwrap_or_default() }, tags: vec![] })
        .collect())
}

/// Cached lookup through the source chain; when every source fails the stale cache (or
/// nothing) is returned — never an error to the view.
pub async fn get(state: &AppState, kind: &str, country: &str, subdivision: &str, year: i32, lang: &str) -> AppResult<Vec<Holiday>> {
    if !state.config.holidays_enabled || country.is_empty() {
        return Ok(vec![]);
    }
    let key = format!("{kind}2-{lang}"); // names are in the page language; 2: entries carry `regions`
    let mk = (country.to_string(), subdivision.to_string(), year, key.clone());
    let mem = state.caches.holidays.0.lock().unwrap().get(&mk).cloned();
    let row = match mem {
        Some(r) => Some(r),
        None => {
            let r = cache_row(state, country, subdivision, year, &key).await?.map(|(j, fetched)| (std::sync::Arc::new(serde_json::from_str::<Vec<Holiday>>(&j).unwrap_or_default()), fetched));
            if let Some(r) = &r {
                let mut m = state.caches.holidays.0.lock().unwrap();
                if m.len() >= MEM_MAX {
                    m.clear();
                }
                m.insert(mk, r.clone());
            }
            r
        }
    };
    let cached: Option<(Vec<Holiday>, bool)> = row.map(|(h, fetched)| ((*h).clone(), age_hours(fetched) < CACHE_DAYS * 24));
    if let Some((h, true)) = &cached {
        return Ok(h.clone());
    }
    let mut errors = vec![];
    let mut answer = None;
    if covered_by_openholidays(country) {
        match from_openholidays(state, kind, country, subdivision, year, lang).await {
            Ok(h) => answer = Some(h),
            Err(e) => errors.push(e),
        }
    }
    if answer.is_none() && kind == "public" && covered_by_nager(country) {
        match from_nager(state, country, subdivision, year, lang).await {
            Ok(h) => answer = Some(h),
            Err(e) => errors.push(e),
        }
    }
    match answer {
        Some(h) => {
            cache_put(state, country, subdivision, year, &key, &serde_json::to_string(&h).unwrap()).await?;
            Ok(h)
        }
        None => {
            if !errors.is_empty() {
                tracing::warn!("holidays {kind} {country}/{subdivision} {year}: every source failed: {}", errors.join("; "));
            }
            Ok(cached.map(|(h, _)| h).unwrap_or_default())
        }
    }
}

/// (country, subdivision) of a stored region code: "DE" → ("DE", ""), "DE-SN" → ("DE", "DE-SN").
pub fn split(code: &str) -> (&str, &str) {
    match code.get(..2) {
        Some(c) if code.len() > 2 => (c, code),
        _ => (code, ""),
    }
}

/// Is `code` a region code as stored (DE, DE-SN, CH-AG-ZZ, FR-69M)?
pub fn valid_code(code: &str) -> bool {
    let mut parts = code.split('-');
    parts.next().is_some_and(|c| c.len() == 2 && c.chars().all(|x| x.is_ascii_uppercase())) && parts.all(|p| !p.is_empty() && p.len() <= 6 && p.chars().all(|x| x.is_ascii_uppercase() || x.is_ascii_digit()))
}

/// Short codes for tags: without the country when every chosen region lies in one ("SN").
pub(super) fn shorten(codes: &[String], regions: &[String]) -> Vec<String> {
    let one = regions.first().map(|r| split(r).0).filter(|c| regions.iter().all(|r| split(r).0 == *c));
    codes.iter().map(|c| one.and_then(|o| c.strip_prefix(o).and_then(|x| x.strip_prefix('-'))).unwrap_or(c).to_string()).collect()
}

/// Several chosen regions of one country that all have a holiday: the country stands for them
/// ("DE" instead of "DE-BY, DE-SN").
fn collapse(mut codes: Vec<String>, covers: &std::collections::BTreeSet<usize>, regions: &[String]) -> Vec<String> {
    let mut countries: Vec<&str> = regions.iter().map(|r| split(r).0).collect();
    countries.dedup();
    for c in countries {
        let of_c: Vec<usize> = (0..regions.len()).filter(|i| split(&regions[*i]).0 == c).collect();
        if of_c.len() > 1 && of_c.iter().all(|i| covers.contains(i)) {
            let at = codes.iter().position(|x| split(x).0 == c).unwrap_or(codes.len());
            codes.retain(|x| split(x).0 != c);
            codes.insert(at.min(codes.len()), c.to_string());
        }
    }
    codes
}

/// The holidays of several regions in one list: a holiday (same name and dates) several regions
/// share appears once; `tags` names the regions it applies to unless it applies to all of them.
/// A whole country counts as covered by a nationwide holiday only; a regional one names its
/// subdivisions.
pub async fn for_regions(state: &AppState, kind: &str, regions: &[String], year: i32, lang: &str) -> AppResult<Vec<Holiday>> {
    let mut per = vec![];
    for r in regions {
        let (country, sub) = split(r);
        per.push(get(state, kind, country, sub, year, lang).await?);
    }
    Ok(merge(regions, per))
}

pub(super) fn merge(regions: &[String], per: Vec<Vec<Holiday>>) -> Vec<Holiday> {
    // (holiday, codes it applies to, indexes of the chosen regions it fully covers)
    let mut acc: Vec<(Holiday, Vec<String>, std::collections::BTreeSet<usize>)> = vec![];
    for (i, hs) in per.into_iter().enumerate() {
        let (country, sub) = split(&regions[i]);
        for h in hs {
            let full = !sub.is_empty() || h.nationwide || h.regions.is_empty();
            let codes: Vec<String> = if !sub.is_empty() { vec![sub.to_string()] } else if full { vec![country.to_string()] } else { h.regions.clone() };
            let at = acc.iter().position(|(x, _, _)| x.start == h.start && x.end == h.end && x.name == h.name);
            let (_, have, covers) = match at {
                Some(k) => &mut acc[k],
                None => {
                    acc.push((h, vec![], Default::default()));
                    acc.last_mut().unwrap()
                }
            };
            for c in codes {
                if !have.contains(&c) {
                    have.push(c);
                }
            }
            if full {
                covers.insert(i);
            }
        }
    }
    let mut out: Vec<Holiday> = acc
        .into_iter()
        .map(|(mut h, codes, covers)| {
            h.tags = if covers.len() == regions.len() { vec![] } else { shorten(&collapse(codes, &covers, regions), regions) };
            h
        })
        .collect();
    out.sort_by(|a, b| (a.start, &a.name).cmp(&(b.start, &b.name)));
    out
}

/// A public holiday or school holiday on one day, as the day/week/month views show it.
#[derive(Debug, Clone)]
pub struct Mark {
    pub name: String,
    /// the chosen regions it applies to when not all of them (`Holiday::tags`)
    pub tags: Vec<String>,
    pub school: bool,
    /// the first day of the holiday (block) this day belongs to
    pub start: NaiveDate,
}

/// The holidays the user asked for (settings), by day, for `from..to` — named, in `lang`.
/// Public ones come first on a day; the years in the window are fetched (cached) once each.
pub async fn marks(state: &AppState, p: &crate::domain::prefs::Prefs, from: NaiveDate, to: NaiveDate, lang: &str) -> AppResult<HashMap<NaiveDate, Vec<Mark>>> {
    use chrono::Datelike;
    let mut out: HashMap<NaiveDate, Vec<Mark>> = HashMap::new();
    // a block across New Year (Weihnachtsferien) comes back for both years: count it once
    let mut seen = std::collections::HashSet::new();
    for (kind, on) in [("public", p.holidays_enabled), ("school", p.school_holidays_enabled)] {
        if !on {
            continue;
        }
        let regions = p.regions(kind == "school");
        for year in from.year()..=to.year() {
            for h in for_regions(state, kind, &regions, year, lang).await? {
                if !seen.insert((kind, h.start, h.end, h.name.clone(), h.tags.clone())) {
                    continue;
                }
                let mut d = h.start.max(from);
                while d <= h.end && d < to {
                    out.entry(d).or_default().push(Mark { name: h.name.clone(), tags: h.tags.clone(), school: kind == "school", start: h.start });
                    let Some(next) = d.succ_opt() else { break };
                    d = next;
                }
            }
        }
    }
    Ok(out)
}
