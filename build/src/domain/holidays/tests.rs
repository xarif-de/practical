use super::*;

fn r(code: &str, iso: &str, name: &str, children: Vec<ApiRegion>) -> ApiRegion {
    ApiRegion { iso_code: iso.into(), code: code.into(), name: vec![ApiName { language: "DE".into(), text: name.into() }], category: vec![ApiName { language: "DE".into(), text: "Kanton".into() }], children }
}

#[test]
fn subdivisions_flatten_parents_first_sorted_without_accents() {
    let mut out = vec![];
    flatten(vec![r("CH-ZH", "CH-ZH", "Zürich", vec![r("CH-ZH-B", "", "Bülach", vec![]), r("CH-ZH-A", "", "Affoltern", vec![])]), r("CH-AG", "CH-AG", "Aargau", vec![])], "de", "CH", 0, &mut out);
    let got: Vec<(&str, &str, u8)> = out.iter().map(|x| (x.code.as_str(), x.parent.as_str(), x.depth)).collect();
    assert_eq!(got, [("CH-AG", "CH", 0), ("CH-ZH", "CH", 0), ("CH-ZH-A", "CH-ZH", 1), ("CH-ZH-B", "CH-ZH", 1)]);
    assert_eq!(out[0].category, "Kanton");
    assert!(fold("Österreich") < fold("Polen"));
}

#[test]
fn iso_code_wins_and_cldr_names_what_the_api_lacks() {
    // OpenHolidays calls Burgenland AT-BL but its ISO code is AT-1; no French name from the API
    let mut out = vec![];
    flatten(vec![r("AT-BL", "AT-1", "Burgenland", vec![])], "fr", "AT", 0, &mut out);
    assert_eq!(out[0].code, "AT-1");
    assert_eq!(out[0].name, "Burgenland");
    assert_eq!(out[0].category, "");
}

#[test]
fn built_in_names_cover_every_language() {
    for l in crate::i18n::LANGS {
        assert!(region_name("DE", l).is_some(), "country name {l}");
        assert!(region_name("DE-SN", l).is_some(), "subdivision name {l}");
    }
    assert_eq!(region_name("US-CA", "de").as_deref(), Some("Kalifornien"));
    assert!(countries("de").len() > 100);
    let mut us = vec![];
    cldr_tree("US", "en", 0, &mut us);
    assert!(us.iter().any(|r| r.code == "US-TX" && r.name == "Texas"));
}

/// The packed tables answer every read exactly as the JSON maps did (`name_in` of before: own
/// language, else English, else any), for every region, UI language and an unknown language;
/// parents and source flags too; and `pack` (run time, a newer CLDR) writes the very bytes build.rs wrote.
#[test]
fn packed_names_read_like_the_json() {
    let d: Data = serde_json::from_str(include_str!("../../data/regions.json")).unwrap();
    let name_in = |n: &HashMap<String, String>, lang: &str| n.get(lang).or_else(|| n.get("en")).or_else(|| n.values().next()).cloned();
    let langs = ["de", "en", "fr", "es", "pt"];
    for (code, c) in &d.countries {
        let r = BUILT_IN_COUNTRIES.find(code).unwrap_or_else(|| panic!("{code}"));
        assert_eq!(r.meta, (c.oh + 2 * c.ng).to_string(), "{code}");
        for l in langs {
            assert_eq!(region_name(code, l), name_in(&c.n, l), "{code} {l}");
        }
    }
    for (code, s) in &d.subs {
        assert_eq!(BUILT_IN_SUBS.find(code).unwrap_or_else(|| panic!("{code}")).parent(), s.p, "{code}");
        for l in langs {
            assert_eq!(region_name(code, l), name_in(&s.n, l), "{code} {l}");
        }
    }
    assert_eq!((BUILT_IN_COUNTRIES.rows().count(), BUILT_IN_SUBS.rows().count()), (d.countries.len(), d.subs.len()));
    assert_eq!(region_name("XX-1", "de"), None);
    assert_eq!(region_name("XX", "de"), None);
    let p = pack(&d);
    assert!(p.countries.rows().zip(BUILT_IN_COUNTRIES.rows()).all(|(a, b)| (a.code, a.meta, a.name("de"), a.name("en"), a.name("fr"), a.name("es")) == (b.code, b.meta, b.name("de"), b.name("en"), b.name("fr"), b.name("es"))));
    assert!(p.subs.rows().zip(BUILT_IN_SUBS.rows()).all(|(a, b)| (a.code, a.parent(), a.name("de"), a.name("en"), a.name("fr"), a.name("es")) == (b.code, b.parent(), b.name("de"), b.name("en"), b.name("fr"), b.name("es"))));
    assert_eq!((p.countries.heap_bytes() > 0, p.subs.rows().count()), (true, d.subs.len()));
    assert_eq!(names::LANGS, crate::i18n::LANGS, "names_columns_are_the_ui_languages");
}

#[test]
fn cldr_versions_and_xml() {
    assert!(newer("release-49", "release-48-2"));
    assert!(newer("release-48-2", "release-48"));
    assert!(!newer("release-48", "release-48-2"));
    assert!(!newer("", "release-48"));
    assert_eq!(BUILT_IN_CLDR, "release-48-2");
    let xml = r#"<territories><territory type="DE">Deutschland</territory><territory type="US" alt="short">USA</territory><territory type="US">Vereinigte Staaten</territory><territory type="BA">Bosnien &amp; Herzegowina</territory></territories>
<subdivision type="desn" draft="contributed">Sachsen</subdivision>
<subgroup type="DE" contains="debb desn"/><subgroup type="frara" contains="fr01 fr69m"/>"#;
    assert_eq!(xml_items(xml, "territory"), [("DE".into(), "Deutschland".into()), ("US".into(), "Vereinigte Staaten".into()), ("BA".into(), "Bosnien & Herzegowina".into())]);
    assert_eq!(xml_items(xml, "subdivision"), [("desn".into(), "Sachsen".into())]);
    assert_eq!(xml_subgroups(xml), [("DE".into(), "debb desn".into()), ("frara".into(), "fr01 fr69m".into())]);
    assert_eq!(iso("fr69m"), "FR-69M");
}

#[test]
fn a_broken_coverage_list_keeps_the_known_one() {
    let known: std::collections::BTreeSet<String> = ["AT", "CH", "DE", "FR"].map(String::from).into();
    assert_eq!(accept(&known, Ok(vec![]), "t"), known);
    assert_eq!(accept(&known, Ok(vec!["DE".into()]), "t"), known);
    assert_eq!(accept(&known, Err("down".into()), "t"), known);
    let grown = accept(&known, Ok(["AT", "CH", "DE", "FR", "IT"].map(String::from).to_vec()), "t");
    assert!(grown.contains("IT") && grown.len() == 5);
    assert!(region_name("KR", "de").is_some(), "names for countries no source covers yet are built in");
}

fn hol(name: &str, day: u32, nationwide: bool, regions: &[&str]) -> Holiday {
    let d = NaiveDate::from_ymd_opt(2026, 11, day).unwrap();
    Holiday { start: d, end: d, name: name.into(), kind: "public".into(), nationwide, source: String::new(), regions: regions.iter().map(|r| r.to_string()).collect(), tags: vec![] }
}

#[test]
fn several_regions_merge_and_tag_what_not_all_share() {
    let regions: Vec<String> = ["DE-SN", "DE-BY"].map(String::from).into();
    let sn = vec![hol("Buß- und Bettag", 18, false, &["DE-SN"]), hol("Neujahr", 2, true, &[])];
    let by = vec![hol("Allerheiligen", 1, false, &["DE-BY"]), hol("Neujahr", 2, true, &[])];
    let got: Vec<String> = merge(&regions, vec![sn, by]).iter().map(Holiday::label).collect();
    assert_eq!(got, ["Allerheiligen (BY)", "Neujahr", "Buß- und Bettag (SN)"]);
    // a whole country: covered by nationwide ones only; regional ones name their subdivisions
    let de: Vec<String> = vec!["DE".into()];
    let got: Vec<String> = merge(&de, vec![vec![hol("Neujahr", 2, true, &[]), hol("Allerheiligen", 1, false, &["DE-BW", "DE-BY"])]]).iter().map(Holiday::label).collect();
    assert_eq!(got, ["Allerheiligen (BW, BY)", "Neujahr"]);
    // two countries keep the country in the tag
    let two: Vec<String> = ["DE-SN", "AT"].map(String::from).into();
    let got: Vec<String> = merge(&two, vec![vec![hol("Neujahr", 2, true, &[])], vec![hol("Neujahr", 2, true, &[]), hol("Leopold", 15, false, &["AT-3"])]]).iter().map(Holiday::label).collect();
    assert_eq!(got, ["Neujahr", "Leopold (AT-3)"]);
    // every chosen region of one country has it: the country stands for them
    let three: Vec<String> = ["DE-BY", "DE-SN", "AT"].map(String::from).into();
    let xmas = || vec![hol("1. Weihnachtsfeiertag", 25, true, &[])];
    let got: Vec<String> = merge(&three, vec![xmas(), xmas(), vec![hol("Christtag", 25, true, &[])]]).iter().map(Holiday::label).collect();
    assert_eq!(got, ["1. Weihnachtsfeiertag (DE)", "Christtag (AT)"]);
    assert_eq!(split("CH-AG-ZZ"), ("CH", "CH-AG-ZZ"));
    assert!(valid_code("DE") && valid_code("FR-69M") && valid_code("CH-AG-ZZ"));
    assert!(!valid_code("de") && !valid_code("D") && !valid_code("DE-") && !valid_code("DE,AT"));
}

#[test]
fn nager_regions_and_names() {
    assert_eq!(nager_region("CH-AG-ZZ"), "CH-AG");
    assert_eq!(nager_region("FR-69M"), "FR-69M");
    let h = |local: &str| NagerHoliday { date: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(), local_name: local.into(), name: "New Year's Day".into(), global: true, counties: None, types: vec!["Public".into()] };
    assert_eq!(nager_name(&h("Neujahr"), "de"), "Neujahr");
    assert_eq!(nager_name(&h("元日"), "de"), "New Year's Day");
    assert_eq!(nager_name(&h("Neujahr"), "en"), "New Year's Day");
}
