//! The settings page's derived texts and its zone list.

use super::*;

fn page(lang: &str, public: &str, school: &str, names: &[(&str, &str)], tz: Option<&str>) -> SettingsPage {
    let ctx = PageCtx { lang: lang.into(), user: None, csrf: String::new(), path: "/settings".into(), flash: None, i18n: crate::i18n::I18n::load(), public_dav_url: String::new(), push_enabled: false, asset_v: "", dev: false, wants_json: false, scheme: "auto".into() };
    SettingsPage {
        ctx, groups: vec![], individuals: vec![], trashed: vec![], calendars: vec![], addressbooks: vec![], deleted_calendars: vec![], deleted_addressbooks: vec![], tokens: vec![], new_token: None,
        prefs: Prefs { timezone: tz.map(str::to_string), scheme: Some("dark".into()), ..Prefs::defaults() }, owners: vec![], palette: vec![], retention_days: 90, default_timezone: "Europe/Berlin".into(),
        public_regions: public.into(), school_regions: school.into(), region_names: names.iter().map(|(c, n)| (c.to_string(), (n.to_string(), String::new()))).collect(),
        picker_json: String::new(), holiday_css: String::new(), vacation_kind: String::new(), vacation_category: String::new(), vacation_calendar: String::new(), vacation_title: String::new(),
        vacation_calendars: vec![], vacation_calendar_unknown: false, vacation_summary: String::new(), alarm_snaps_start: String::new(), alarm_snaps_end: String::new(), alarm_snaps_default_start: String::new(), alarm_snaps_default_end: String::new(),
    }
}

#[test]
fn derived_texts() {
    let p = page("de", "DE-SN,DE-BY", "", &[("DE-SN", "Sachsen")], None);
    assert_eq!(p.regions_text(false), "Sachsen, DE-BY", "an unknown name shows the code");
    assert_eq!(p.regions_text(true), "Keine");
    assert_eq!(page("de", "DE-SN,DE-BY,AT", "", &[], None).regions_text(false), "3 Regionen");
    // every chosen country's sources, for both kinds: no school region chosen, still a school source
    assert_eq!(p.sources_text(), "Feiertage: OpenHolidays → Nager.Date · Schulferien: OpenHolidays");
    assert_eq!(page("de", "US", "", &[], None).sources_text(), "Feiertage: Nager.Date · Schulferien: keine Quelle");
    assert_eq!(page("en", "US", "DE-SN", &[], None).sources_text(), "Holidays: OpenHolidays → Nager.Date · School holidays: OpenHolidays");
    assert_eq!(p.tz_name(), "Standard (Europe/Berlin)");
    assert_eq!(page("de", "", "", &[], Some("America/New_York")).tz_name(), "America/New York");
    assert!(p.scheme_is("dark") && !p.scheme_is("auto"));
    assert!(p.lang_is("de") && !p.lang_is("en"));
    assert_eq!(p.comps(&["VEVENT".into(), "VTODO".into(), "VJOURNAL".into(), "X".into()]), "Termine, Aufgaben, Journal, X");
}

#[test]
fn zone_list() {
    let z = timezones();
    assert_eq!(z.iter().map(|(a, _)| a.as_str()).collect::<Vec<_>>(), ["Africa", "America", "Antarctica", "Arctic", "Asia", "Atlantic", "Australia", "Europe", "Indian", "Pacific"]);
    let eu = &z.iter().find(|(a, _)| a == "Europe").unwrap().1;
    assert!(eu.contains(&("Europe/Berlin".to_string(), "Berlin".to_string())));
    let am = &z.iter().find(|(a, _)| a == "America").unwrap().1;
    assert!(am.contains(&("America/Argentina/Buenos_Aires".to_string(), "Argentina / Buenos Aires".to_string())));
    assert!(!z.iter().flat_map(|(_, v)| v).any(|(n, _)| n.starts_with("US/") || n.starts_with("Etc/")));
}
