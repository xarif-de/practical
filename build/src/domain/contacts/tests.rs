use super::*;

#[test]
fn person_matches_address_book() {
    let c = |id: &str, name: &str, mail: &str| Contact { principal: "p".into(), ab_id: "a".into(), object_id: id.into(), full_name: name.into(), emails: if mail.is_empty() { vec![] } else { vec![Typed { kind: "home".into(), value: mail.into(), pref: false }] }, ..Default::default() };
    let all = vec![c("1", "Erika Musterfrau", "erika@example.org"), c("2", "Max Mustermann", "max.mustermann@firma.de"), c("3", "Maximilian Mustermann", "mm@example.org"), c("4", "Max Müller", ""), c("5", "Thomas Steinbach", "Thomas@Example.org")];
    let keys = |v: Vec<ContactHit>| v.into_iter().map(|h| h.key).collect::<Vec<_>>();
    // e-mail wins, case-insensitively, even when the name differs
    assert_eq!(keys(matches(&all, "T. Steinbach", "thomas@example.org", "")), vec!["p/a/5"]);
    assert_eq!(matches(&all, "Erika Musterfrau", "erika@example.org", "INDIVIDUAL")[0].sub, "erika@example.org");
    // no e-mail hit: similar names, several candidates
    assert_eq!(keys(matches(&all, "Max Mustermann", "max@example.org", "")), vec!["p/a/2", "p/a/3"]);
    assert_eq!(keys(matches(&all, "Erika Musterfrau", "", "")), vec!["p/a/1"]);
    // a lone first name, an unknown person, a room: nothing
    assert!(matches(&all, "Max", "", "").is_empty());
    assert!(matches(&all, "Lena Beispiel", "lena@example.org", "").is_empty());
    assert!(matches(&all, "Max Mustermann", "max.mustermann@firma.de", "ROOM").is_empty());
}

#[test]
fn yearly_dates() {
    use crate::testkit::date;
    assert_eq!((partial_parts("1984-02-29"), partial_parts("--02-29"), partial_parts("irgendwann")), (Some((Some(1984), 2, 29)), Some((None, 2, 29)), None));
    // a day the month lacks is its last day: 29 February in a common year is the 28th
    assert_eq!((yearly_in(2027, 2, 29), yearly_in(2028, 2, 29), yearly_in(2026, 4, 31)), (Some(date("2027-02-28")), Some(date("2028-02-29")), Some(date("2026-04-30"))));
    assert_eq!((yearly_in(2026, 13, 1), yearly_in(2026, 2, 0)), (None, None));
    // today counts; past it, the next year's day (a leap year's own 29th)
    assert_eq!(next_yearly(2, 29, date("2027-02-28")), Some(date("2027-02-28")));
    assert_eq!(next_yearly(2, 29, date("2027-03-01")), Some(date("2028-02-29")));
    assert_eq!(next_yearly(2, 29, date("2026-10-14")), Some(date("2027-02-28")));
    assert_eq!(next_yearly(12, 31, date("2026-12-31")), Some(date("2026-12-31")));
}

#[test]
fn vcard_roundtrip() {
    let i = ContactInput {
        principal: "p".into(),
        ab_id: "a".into(),
        given: "Christin".into(),
        family: "Lang".into(),
        nickname: "Chrissi".into(),
        department: "Vertrieb".into(),
        org: "ACME".into(),
        emails: vec![Typed { kind: "home".into(), value: "christin@example.org".into(), pref: true }],
        phones: vec![Typed { kind: "cell".into(), value: "+49 160 2223344".into(), pref: false }],
        addresses: vec![Address { kind: "home".into(), street: "Parkstraße 12".into(), code: "10115".into(), city: "Berlin".into(), country: "Deutschland".into(), ..Default::default() }],
        urls: vec![Typed { kind: "mastodon".into(), value: "https://example.social/@c".into(), pref: false }],
        impp: vec![Typed { kind: "Signal".into(), value: "+49 160 2223344".into(), pref: false }],
        related: vec![Typed { kind: "spouse".into(), value: "Max Lang".into(), pref: false }],
        dates: vec![Dated { label: "Namenstag".into(), date: "--07-24".into() }],
        birthday: "1984-03-12".into(),
        categories: vec!["Familie".into()],
        note: "Allergie: Nüsse".into(),
        ..Default::default()
    };
    let vcf = build_vcf("u1", &i, &["PHOTO;ENCODING=b;TYPE=JPEG:/9j/4AAQ".to_string(), "X-FOO:bar".to_string()]).unwrap();
    assert!(vcf.contains("X-FOO:bar"), "preserved lines are written back");
    let vc = parse_vcf(&vcf).unwrap();
    let ab = AbInfo { color: String::new(), principal: "p".into(), owner_label: String::new(), is_group: false, id: "a".into(), displayname: "Privat".into(), description: String::new(), deleted_at: None, synctoken: 0, ..AbInfo::bare(crate::state::Authorized::test_own("p", "a")) };
    let c = contact_from(&vc, &raw_lines(&vcf), &ab, "u1", String::new());
    assert_eq!(c.full_name, "Christin Lang");
    assert_eq!(c.family, "Lang");
    assert_eq!(c.primary_email(), "christin@example.org");
    assert_eq!(c.phones[0].kind, "cell");
    assert_eq!(c.primary_address(), "Parkstraße 12, 10115 Berlin, Deutschland");
    assert_eq!(c.addresses[0].code, "10115");
    assert!(c.emails[0].pref && !c.phones[0].pref);
    assert_eq!((c.nickname.as_str(), c.org.as_str(), c.department.as_str()), ("Chrissi", "ACME", "Vertrieb"));
    assert_eq!(c.urls[0].kind, "mastodon");
    assert_eq!(c.impp[0].kind, "Signal");
    assert_eq!((c.related[0].kind.as_str(), c.related[0].value.as_str()), ("spouse", "Max Lang"));
    assert_eq!((c.dates[0].label.as_str(), c.dates[0].date.as_str()), ("Namenstag", "--07-24"));
    assert_eq!(preserved_lines(&vcf), vec!["PHOTO;ENCODING=b;TYPE=JPEG:/9j/4AAQ".to_string(), "X-FOO:bar".to_string()]);
    assert_eq!(c.birthday, "1984-03-12");
    assert_eq!(c.birthday_label(), "12.03.1984");
    assert_eq!(c.categories, vec!["Familie"]);
    assert_eq!((c.given_letter().as_str(), c.given_key().as_str()), ("C", "christin lang"));
    assert_eq!(norm_partial("--0312"), "--03-12");
    assert_eq!(fmt_partial("--03-12"), "12.03.");
}
