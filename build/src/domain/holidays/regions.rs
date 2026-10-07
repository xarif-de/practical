//! Region choice: the country list and the flattened subdivision tree of a country.

use super::*;

/// A country or subdivision for the region choice. Subdivisions come flattened in display
/// order (parents before their children); `parent` is the country for the top level.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Region {
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub parent: String,
    #[serde(default)]
    pub depth: u8,
    /// what the level is called in `lang` ("Bundesland", "Kanton", "Provinz" …), where known
    #[serde(default)]
    pub category: String,
    /// countries: a source for school holidays exists
    #[serde(default)]
    pub school: bool,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ApiRegion {
    #[serde(default)]
    pub(super) iso_code: String,
    #[serde(default)]
    pub(super) code: String,
    #[serde(default)]
    pub(super) name: Vec<ApiName>,
    #[serde(default)]
    pub(super) category: Vec<ApiName>,
    #[serde(default)]
    pub(super) children: Vec<ApiRegion>,
}

/// sort key: letters without their accents, so "Österreich" sorts with the O's
pub(super) fn fold(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'ä' | 'à' | 'á' | 'â' | 'ã' | 'å' => 'a',
            'ö' | 'ò' | 'ó' | 'ô' | 'õ' => 'o',
            'ü' | 'ù' | 'ú' | 'û' => 'u',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ç' | 'č' | 'ć' => 'c',
            'ñ' => 'n',
            'ß' | 'š' => 's',
            'ž' => 'z',
            o => o,
        })
        .collect()
}

fn sort(v: &mut [Region]) {
    v.sort_by_key(|r| fold(&r.name));
}

/// The countries a holiday source covers, named in `lang` (built in, no request).
pub fn countries(lang: &str) -> Vec<Region> {
    let cov = COVERAGE.read().unwrap().clone();
    let mut v: Vec<Region> = cov
        .oh
        .union(&cov.ng)
        .map(|code| Region { code: code.clone(), name: region_name(code, lang).unwrap_or_else(|| code.clone()), parent: String::new(), depth: 0, category: String::new(), school: cov.oh.contains(code) })
        .collect();
    sort(&mut v);
    v
}

/// OpenHolidays' tree, flattened; a region is named from the API in `lang`, else from CLDR
/// (its ISO code), else from the API in any language.
pub(super) fn flatten(items: Vec<ApiRegion>, lang: &str, parent: &str, depth: u8, out: &mut Vec<Region>) {
    let mut level: Vec<(Region, Vec<ApiRegion>)> = items
        .into_iter()
        .map(|r| {
            let code = if r.iso_code.is_empty() { r.code.clone() } else { r.iso_code.clone() };
            let exact = r.name.iter().find(|n| n.language.eq_ignore_ascii_case(lang)).map(|n| n.text.clone());
            let name = exact.or_else(|| region_name(&code, lang)).unwrap_or_else(|| pick(&r.name, lang));
            let cat = r.category.iter().find(|n| n.language.eq_ignore_ascii_case(lang)).map(|n| n.text.clone()).unwrap_or_default();
            (Region { code, name, parent: parent.to_string(), depth, category: cat, school: false }, r.children)
        })
        .collect();
    level.sort_by_key(|(r, _)| fold(&r.name));
    for (r, children) in level {
        let code = r.code.clone();
        out.push(r);
        flatten(children, lang, &code, depth + 1, out);
    }
}

/// CLDR's hierarchy below `parent`, flattened the same way (countries OpenHolidays lacks).
pub(super) fn cldr_tree(parent: &str, lang: &str, depth: u8, out: &mut Vec<Region>) {
    // an ISO 3166-2 code starts with its country's: only that country's run of the sorted table
    let mut level: Vec<Region> = with_table(true, |t| {
        t.with_prefix(&format!("{}-", &parent[..2.min(parent.len())]))
            .filter(|r| r.parent() == parent)
            .map(|r| Region { code: r.code.to_string(), name: r.name(lang).unwrap_or(r.code).to_string(), parent: parent.to_string(), depth, category: String::new(), school: false })
            .collect()
    });
    sort(&mut level);
    for r in level {
        let code = r.code.clone();
        out.push(r);
        cldr_tree(&code, lang, depth + 1, out);
    }
}

/// The subdivisions of `country`, all levels, parents first.
pub async fn subdivisions(state: &AppState, country: &str, lang: &str) -> AppResult<Vec<Region>> {
    if !covered_by_openholidays(country) && !covered_by_nager(country) {
        return Ok(vec![]);
    }
    if covered_by_openholidays(country) {
        let key = format!("subdivisions2-{lang}"); // 2: entries carry `parent` (the first format did not)
        let cached: Option<(Vec<Region>, bool)> = cache_get(state, country, "", 0, &key).await?.map(|(j, fresh)| (serde_json::from_str(&j).unwrap_or_default(), fresh));
        if let Some((r, true)) = &cached {
            return Ok(r.clone());
        }
        let url = format!("{OPENHOLIDAYS}/Subdivisions?countryIsoCode={country}&languageIsoCode={}", lang.to_uppercase());
        match get_json::<Vec<ApiRegion>>(state, &url).await {
            Ok(items) => {
                let mut out = vec![];
                flatten(items, lang, country, 0, &mut out);
                cache_put(state, country, "", 0, &key, &serde_json::to_string(&out).unwrap()).await?;
                return Ok(out);
            }
            Err(e) => {
                tracing::warn!("openholidays subdivisions {country}: {e}");
                if let Some((r, _)) = cached {
                    return Ok(r);
                }
            }
        }
    }
    let mut out = vec![];
    cldr_tree(country, lang, 0, &mut out);
    Ok(out)
}
