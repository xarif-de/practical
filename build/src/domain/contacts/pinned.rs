//! Characterisation of the vCard model: what is read from the dialects in the wild (Apple item
//! groups, vCard 4 PREF, 2.1 bare types), the lines a save writes, the sort keys and dates.

use super::*;

fn ab() -> AbInfo {
    AbInfo { color: "#ff2d55".into(), principal: "p".into(), owner_label: String::new(), is_group: true, id: "a".into(), displayname: "Freunde".into(), description: String::new(), deleted_at: None, synctoken: 0, ..AbInfo::bare(crate::state::Authorized::test_own("p", "a")) }
}

fn read(vcf: &str) -> Contact {
    contact_from(&parse_vcf(vcf).unwrap(), &raw_lines(vcf), &ab(), "obj", "etag".into())
}

const APPLE: &str = "BEGIN:VCARD\r\nVERSION:3.0\r\nPRODID:-//Apple Inc.//iOS 18.0//EN\r\nN:Muster;Erika;Maria;Dr.;jun.\r\nFN:Dr. Erika Muster\r\nORG:ACME\\, Inc.;Forschung\r\nTITLE:Leiterin\r\nitem1.EMAIL;type=INTERNET;type=HOME;type=pref:erika@example.org\r\nitem1.X-ABLabel:_$!<Other>!$_\r\nEMAIL;TYPE=WORK:e.muster@acme.example\r\nTEL;type=CELL;type=VOICE;type=pref:+49 151 1\r\nTEL;TYPE=\"home,voice\":+49 351 2\r\nitem2.ADR;type=HOME;type=pref:Postfach 1;Hinterhaus;Hauptstr. 1;Dresden;Sachsen;01067;Deutschland\r\nitem3.URL;type=pref:https://example.org\r\nIMPP;X-SERVICE-TYPE=Signal;type=pref:+49 151 1\r\nX-ABRELATEDNAMES;type=pref:Max\r\nX-ABDATE;X-ABLABEL=Hochzeit:2010-06-12\r\nBDAY;VALUE=date:1984-03-12\r\nANNIVERSARY:--0612\r\nNICKNAME:Eri\r\nCATEGORIES:Familie,Arbeit\r\nNOTE:Zeile\\nzwei\r\nPHOTO;ENCODING=b;TYPE=JPEG:/9j/4AAQ\r\n SkZJRg==\r\nX-CUSTOM:bleibt\r\nUID:erika\r\nEND:VCARD\r\n";

#[test]
fn reading_an_apple_card() {
    let c = read(APPLE);
    let v = serde_json::to_value(&c).unwrap();
    crate::testkit::assert_snapshot("contacts/apple_card.json", &serde_json::to_string_pretty(&v).unwrap());
    assert_eq!((c.full_name.as_str(), c.given.as_str(), c.family.as_str(), c.additional.as_str(), c.prefix.as_str(), c.suffix.as_str()), ("Dr. Erika Muster", "Erika", "Muster", "Maria", "Dr.", "jun."));
    assert_eq!((c.org.as_str(), c.department.as_str()), ("ACME, Inc.", "Forschung"));
    assert_eq!(c.emails.iter().map(|e| (e.kind.as_str(), e.pref)).collect::<Vec<_>>(), [("home", true), ("work", false)]);
    assert_eq!(c.phones.iter().map(|e| (e.kind.as_str(), e.pref)).collect::<Vec<_>>(), [("cell", true), ("home", false)]);
    assert_eq!(c.addresses[0].street, "Postfach 1, Hinterhaus, Hauptstr. 1");
    assert_eq!(c.primary_address(), "Postfach 1, Hinterhaus, Hauptstr. 1, 01067 Dresden, Sachsen, Deutschland");
    assert_eq!(c.impp[0].kind, "Signal");
    assert!(c.related.is_empty(), "X-ABRELATEDNAMES is not RELATED");
    assert_eq!((c.dates[0].label.as_str(), c.dates[0].date.as_str()), ("Hochzeit", "2010-06-12"));
    assert_eq!((c.birthday.as_str(), c.anniversary.as_str()), ("1984-03-12", "--06-12"));
    assert_eq!((c.ab_name.as_str(), c.ab_color.as_str(), c.is_group, c.uid.as_str(), c.etag.as_str()), ("Freunde", "#ff2d55", true, "erika", "etag"));
    assert_eq!((c.initials(), c.given_letter(), c.given_key(), c.sort_key()), ("DE".to_string(), "E".to_string(), "erika muster".to_string(), "muster erika".to_string()));
    assert_eq!(c.category(), "Familie");
    // PRODID is written anew (managed); Apple's label of item1 stays although its EMAIL is rewritten
    assert_eq!(preserved_lines(APPLE), ["item1.X-ABLabel:_$!<Other>!$_", "X-ABRELATEDNAMES;type=pref:Max", "PHOTO;ENCODING=b;TYPE=JPEG:/9j/4AAQSkZJRg==", "X-CUSTOM:bleibt"]);
}

#[test]
fn other_dialects() {
    // vCard 4: PREF=1, VALUE=text RELATED, a card without N and FN from the given name
    let v4 = "BEGIN:VCARD\r\nVERSION:4.0\r\nFN:Kim\r\nEMAIL;PREF=1:kim@example.org\r\nEMAIL;PREF=2:k@example.org\r\nRELATED;TYPE=friend;VALUE=text:Lee\r\nBDAY:--1224\r\nUID:kim\r\nEND:VCARD\r\n";
    let c = read(v4);
    assert_eq!((c.full_name.as_str(), c.given.as_str(), c.emails[0].pref, c.emails[1].pref), ("Kim", "", true, false));
    assert_eq!((c.related[0].kind.as_str(), c.related[0].value.as_str()), ("friend", "Lee"));
    assert_eq!((c.birthday.as_str(), c.birthday_label().as_str()), ("--12-24", "24.12."));
    assert_eq!((c.given_letter(), c.initials()), ("K".to_string(), "K".to_string()));
    // vCard 2.1-style bare parameters (TEL;CELL, TEL;HOME;VOICE, EMAIL;INTERNET) read as TYPE — caldata alone
    // rejects such a card — also in a folded line, next to a quoted value with ";" and a value with "="
    let v21 = "BEGIN:VCARD\r\nVERSION:2.1\r\nN:Muster;Olga;;;\r\nFN:Olga Muster\r\nTEL;CELL:0151\r\nTEL;HOME;VOICE:030 1\r\nEMAIL;INTERNET;PREF:olga@example.org\r\nURL;WORK:https://example.org/?a=b\r\nADR;X-A=\"x;y\";HO\r\n ME:;;Weg 1;Ort;;;\r\nBDAY:1990-02-28\r\nEND:VCARD\r\n";
    assert!(caldata::VcardParser::from_slice(v21.as_bytes()).expect_one().is_err(), "caldata itself still rejects it");
    let c = read(v21);
    assert_eq!(c.phones.iter().map(|t| (t.kind.as_str(), t.value.as_str())).collect::<Vec<_>>(), [("cell", "0151"), ("home", "030 1")]);
    assert_eq!((c.emails[0].value.as_str(), c.emails[0].pref, c.urls[0].kind.as_str(), c.urls[0].value.as_str()), ("olga@example.org", true, "work", "https://example.org/?a=b"));
    assert_eq!((c.addresses[0].kind.as_str(), c.addresses[0].street.as_str(), c.birthday.as_str()), ("home", "Weg 1", "1990-02-28"));
    assert!(bare_params_as_type(v21).contains("\r\nTEL;TYPE=HOME;TYPE=VOICE:030 1\r\n"));
    // a card without one is handed on as it is
    assert!(matches!(bare_params_as_type(v4), std::borrow::Cow::Borrowed(_)));
    // an empty FN: the name comes from N
    let c = read("BEGIN:VCARD\r\nVERSION:3.0\r\nN:Öztürk;Ayşe;;;\r\nFN:\r\nTEL;TYPE=cell:0151\r\nEND:VCARD\r\n");
    assert_eq!((c.full_name.as_str(), c.phones[0].kind.as_str(), c.uid.as_str(), c.initials().as_str()), ("Ayşe Öztürk", "cell", "obj", "AÖ"));
    let nobody = read("BEGIN:VCARD\r\nVERSION:3.0\r\nFN:\r\nEND:VCARD\r\n");
    assert_eq!((nobody.initials(), nobody.given_letter()), ("?".to_string(), "#".to_string()));
    assert!(parse_vcf("not a card").is_err());
    // the raw line reader: groups, quoted parameters, repeated types
    let r = raw_lines("item9.TEL;TYPE=\"cell,pref\";X-A=\"a:b\";CELL:+1:2\r\n");
    assert_eq!((r[0].name.as_str(), r[0].value.as_str()), ("TEL", "+1:2"));
    assert_eq!(r[0].params, [("TYPE".to_string(), "cell".to_string()), ("TYPE".into(), "pref".into()), ("X-A".into(), "a:b".into()), ("TYPE".into(), "CELL".into())]);
    assert_eq!((r[0].kind(), r[0].pref()), ("cell".to_string(), true));
}

#[test]
fn partial_dates() {
    for (input, norm) in [("19840312", "1984-03-12"), ("1984-03-12", "1984-03-12"), ("1984-03-12T00:00:00Z", "1984-03-12"), ("--0312", "--03-12"), ("--03-12", "--03-12"), ("--3-12", ""), ("1984", ""), ("garbage", ""), ("", "")] {
        assert_eq!(norm_partial(input), norm, "{input}");
    }
    for (iso, label) in [("1984-03-12", "12.03.1984"), ("--03-12", "12.03."), ("", ""), ("x", "x")] {
        assert_eq!(fmt_partial(iso), label);
    }
    assert_eq!(partial_to_vcard("1984-03-12"), Some("19840312".into()));
    assert_eq!(partial_to_vcard("--03-12"), Some("--0312".into()));
    assert_eq!(partial_to_vcard("März"), None);
}

#[test]
fn writing() {
    let i: ContactInput = serde_json::from_value(serde_json::json!({
        "principal": "p", "ab_id": "a", "given": " Anna ", "family": "Schmidt", "prefix": "Dr.", "org": "", "department": "Einkauf",
        "emails": [{ "kind": "home", "value": "a@example.org", "pref": true }, { "kind": "", "value": "b@example.org", "pref": true }, { "kind": "weird kind!", "value": "c@example.org" }, { "kind": "work", "value": " " }],
        "phones": [{ "kind": "cell", "value": "+49; 1" }], "addresses": [{ "kind": "home", "city": "Dresden" }, { "kind": "work" }],
        "urls": [{ "kind": "blog", "value": "https://x.example/a,b" }], "impp": [{ "kind": "Matrix chat", "value": "@a:x" }, { "kind": "Signal", "value": "+49" }],
        "related": [{ "kind": "spouse", "value": "Bo" }], "dates": [{ "label": "Namens Tag", "date": "--07-24" }, { "label": "x", "date": "bad" }],
        "birthday": "1985-04-02", "anniversary": "nope", "categories": ["A,B", ""], "note": "n;1"
    }))
    .unwrap();
    let vcf = build_vcf("uid-9", &i, &["X-KEEP:1".to_string()]).unwrap();
    let lines: Vec<&str> = vcf.split("\r\n").filter(|l| !l.starts_with("REV:")).collect();
    assert_eq!(lines, [
        "BEGIN:VCARD", "VERSION:3.0", "PRODID:-//xarif.de//PractiCal//DE", "UID:uid-9", "FN:Anna Schmidt", "N:Schmidt;Anna;;Dr.;",
        "ORG:;Einkauf", "EMAIL;TYPE=HOME,PREF:a@example.org", "EMAIL;TYPE=PREF:b@example.org", "EMAIL:c@example.org", r"TEL;TYPE=CELL:+49\; 1",
        "ADR;TYPE=HOME:;;;Dresden;;;", "URL;TYPE=BLOG:https://x.example/a,b", "IMPP:@a:x", "IMPP;X-SERVICE-TYPE=Signal:+49", "RELATED;TYPE=SPOUSE;VALUE=text:Bo",
        "X-ABDATE:--0724", "BDAY:19850402", "CATEGORIES:A\\,B", r"NOTE:n\;1", "X-KEEP:1", "END:VCARD", "",
    ]);
    assert!(matches!(build_vcf("u", &ContactInput::default(), &[]), Err(AppError::BadRequest(m)) if m == "name required"));
    let only_full = ContactInput { full_name: "ACME".into(), ..Default::default() };
    assert!(build_vcf("u", &only_full, &[]).unwrap().contains("\r\nFN:ACME\r\nN:;;;;\r\n"));
    assert_eq!(type_pref("", false), "");
    assert_eq!(type_param("x-custom"), ";TYPE=X-CUSTOM");
    assert_eq!(word(" a_b-1 "), Some("a_b-1"));
    assert_eq!(word("a b"), None);
    assert_eq!(Address { street: "S".into(), code: "1".into(), ..Default::default() }.one_line(), "S, 1");
    // the written card reads back to the same input fields
    let c = read(&vcf);
    assert_eq!((c.full_name.as_str(), c.emails.len(), c.impp[0].kind.as_str(), c.dates.len(), c.birthday.as_str()), ("Anna Schmidt", 3, "", 1, "1985-04-02"));
    assert_eq!(c.color_index(), Contact { uid: "uid-9".into(), ..Default::default() }.color_index());
    assert!((1..=6).contains(&c.color_index()));
}
