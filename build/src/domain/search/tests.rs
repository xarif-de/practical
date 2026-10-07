use super::*;

#[test]
fn date_queries() {
    let today = NaiveDate::from_ymd_opt(2026, 9, 15).unwrap();
    assert_eq!(parse_date_query("2027", today).unwrap().date, NaiveDate::from_ymd_opt(2027, 1, 1).unwrap());
    assert_eq!(parse_date_query("März", today).unwrap().date, NaiveDate::from_ymd_opt(2026, 3, 1).unwrap());
    assert_eq!(parse_date_query("3. März 2027", today).unwrap().date, NaiveDate::from_ymd_opt(2027, 3, 3).unwrap());
    assert_eq!(parse_date_query("15.09.", today).unwrap().date, NaiveDate::from_ymd_opt(2026, 9, 15).unwrap());
    assert_eq!(parse_date_query("1.1.27", today).unwrap().date, NaiveDate::from_ymd_opt(2027, 1, 1).unwrap());
    assert!(parse_date_query("Zahnarzt", today).is_none());
    assert_eq!(parse_date_query("sep 2027", today).unwrap().view, "month");
}

#[test]
fn folding_and_scopes() {
    assert_eq!(fold("Müller Straße ÉTÉ"), "muller strasse ete");
    assert_eq!(split_scope("termin: Zahnarzt"), (Some("event"), " Zahnarzt"));
    assert_eq!(split_scope("Kontakt:anna"), (Some("contact"), "anna"));
    assert_eq!(split_scope("todo: x").0, Some("task"));
    assert_eq!(split_scope("12:30 Treffen"), (None, "12:30 Treffen"));
}

fn doc(text: &str) -> Doc {
    Doc { fields: vec![Field::new("t", text, 1.0)], ..Default::default() }
}
fn score(q: &str, text: &str) -> f32 {
    score_doc(&Query::new(q), &doc(text)).score
}
/// the exact pass only (typos are the fallback)
fn strict(q: &str, text: &str) -> f32 {
    score_doc(&Query { tier: 0, ..Query::new(q) }, &doc(text)).score
}

#[test]
fn ranking_by_match_kind() {
    // one query against different fields: the whole field, its start, a word, a word's start, inside a word, a typo
    let order = [score("zahnarzt", "Zahnarzt"), score("zahnarzt", "Zahnarzt Weber"), score("zahnarzt", "Termin Zahnarzt"), score("zahnarzt", "Termin Zahnarztpraxis"), score("zahnarzt", "Kinderzahnarzt"), score("zahnartz", "Termin Zahnarzt")];
    assert!(order.windows(2).all(|w| w[0] > w[1]) && order[5] > 0.0, "{order:?}");
    assert_eq!(score("zahnarzt weber", "Zahnarzt"), 0.0, "every word must match");
    assert!(score("muller", "Müller") > 0.9, "diacritics fold");
    assert_eq!(score("xy", "Zahnarzt"), 0.0);
    assert!(score("dietmarstein", "Dietmar Steinbach") > score("steinba", "Dietmar Steinbach") * 0.8, "words run together");
    assert_eq!(score("marstein", "Dietmar Steinbach"), 0.0, "run together only from a word's start");
    assert_eq!(score("dietm", "Dietmar Steinbach") > 0.0, true);
    let chain = score("dietstein", "Dietmar Steinbach");
    assert!(chain > 0.0 && chain < score("dietmarstein", "Dietmar Steinbach"), "starts of consecutive words: {chain}");
    assert!(score("chrstein", "Christin Steinbach") > 0.0);
    assert_eq!(strict("dstein", "Dietmar Steinbach"), 0.0, "a part needs 2 letters");
    assert_eq!(strict("urlab", "Urlaubs-Abrechnung"), 0.0, "the last part needs 3 letters");
    let loose = |q: &str, d: &Doc| score_doc(&Query { tier: 2, ..Query::new(q) }, d).score;
    assert!(loose("zahnarzt weber", &doc("Zahnarzt")) > 0.0 && score("zahnarzt weber", "Zahnarzt") == 0.0, "the loosest tier lets half the words go missing");
    assert_eq!(loose("zahnarzt weber meier", &doc("Zahnarzt")), 0.0, "but not more than half");
    let note = Doc { fields: vec![Field::new("t", "x", 1.0), Field::new("note", "abgesagt", 0.4)], ..Default::default() };
    assert!(loose("abgesagx", &note) > 0.0, "the loosest tier allows typos everywhere");
    assert_eq!(loose("zugesagt", &note), 0.0, "a typo keeps the first letter");
    assert_eq!(strict("dietbach", "Dietmar Steinbach"), 0.0, "no skipped word, starts only");
    let d = Doc { fields: vec![Field::new("t", "Zahnarzt", 1.0), Field::collection("Familie")], ..Default::default() };
    assert!(score_doc(&Query::new("familie zahn"), &d).score > 0.0 && score_doc(&Query::new("familie"), &d).score == 0.0, "a collection name only narrows");
    let bday = DateField::vcard("bday", "1987-11-17", 0.95).unwrap();
    assert!(bday.hits((Some(1987), 11, 17)) && bday.hits((None, 11, 17)) && !bday.hits((Some(1988), 11, 17)));
    assert!(DateField::vcard("bday", "--11-17", 1.0).unwrap().hits((Some(2030), 11, 17)), "a date without a year");
    assert_eq!(date_parts("17.11.1987"), Some((Some(1987), Some(11), Some(17))));
    let long = format!("{} Zahnarzt abgesagt {}", "x".repeat(300), "y".repeat(300));
    let c = crop(&long, &["abgesagt"]);
    assert!(c.contains("abgesagt") && c.starts_with('…') && c.ends_with('…') && c.chars().count() <= 122, "{c}");
    assert!(score("chrstinb", "Christin Steinbach") > score("chrstinb", "Christine Ackermann"), "letters in order break the tie");
    assert!(score("chrstinbach", "Christin Steinbach") > 0.0, "letters left out");
    assert!(score("chrstinb", "Christin Steinbach") > score("chrstinb", "Christian Berger"), "a typo of a whole word beats letters left out");
    assert_eq!(strict("christina", "Christin Altersvorsorge"), 0.0, "one letter of the next word is no joined match");
    assert_eq!(score("chrstinbach", "Christine Ackermann"), 0.0);
    let note = Doc { fields: vec![Field::new("t", "Einkauf", 1.0), Field::new("note", "Christin Steinbach kommt", 0.4)], ..Default::default() };
    assert_eq!(score_doc(&Query::new("chrstinbach"), &note).score, 0.0, "no fuzzy kinds in a note");
    assert_eq!(score_doc(&Query::new("zugesagt"), &Doc { fields: vec![Field::new("t", "x", 1.0), Field::new("note", "abgesagt", 0.4)], ..Default::default() }).score, 0.0, "no typos in a note");
    let m = score_doc(&Query::new("chrstinb"), &doc("Christin Steinbach"));
    assert_eq!((m.hits[0].kind, m.hits[0].typo.as_deref()), (Kind::Typo, Some("christin")));
    assert!(score("kalnder", "Kalender") > 0.0, "one edit below 8 letters");
    let m = score_doc(&Query::new("kalner"), &doc("Kalender"));
    assert!(m.score > 0.0 && m.hits[0].kind == Kind::Gaps, "two edits on 6 letters are no typo, but letters left out");
    assert_eq!(score("demo", "devops"), 0.0, "no typos below 5 letters");
    assert_eq!(score("kalner", "Termin Zahnarzt"), 0.0);
    assert!(score("0171 234", "x") == 0.0 && score_doc(&Query::new("234"), &Doc { digits: vec!["01712345".into()], ..doc("Anna") }).score > 0.0, "digits match phone numbers");
}

fn ev(title: &str, days_ago: &[i64], cancelled: bool) -> EvDoc {
    let now = NaiveDate::from_ymd_opt(2026, 9, 15).unwrap().and_hms_opt(12, 0, 0).unwrap();
    let mut occ: Vec<(NaiveDateTime, String)> = days_ago.iter().map(|d| (now - Duration::days(*d), format!("t{d}"))).collect();
    occ.sort();
    EvDoc { key: format!("k/{title}/{}", days_ago.first().unwrap_or(&0)), uid: String::new(), summary: title.into(), color: String::new(), cal_name: String::new(), cancelled, occ, doc: Doc { fields: vec![Field::new("title", title, 1.0)], ..Default::default() } }
}

fn suggest_titles(evs: &[EvDoc], q: &str) -> Vec<(String, usize)> {
    let now = NaiveDate::from_ymd_opt(2026, 9, 15).unwrap().and_hms_opt(12, 0, 0).unwrap();
    let mut query = Query::new(q);
    query.tier = 1;
    suggestions(evs, &query, now, 3).into_iter().map(|s| (s.summary, s.count)).collect()
}

#[test]
fn suggestions_rank_by_frequency_before_the_exact_title() {
    let evs = vec![
        ev("Zahnarzt", &[40], false),
        ev("Zahnarzt Kontrolle", &[10, 100, 200], false),
        ev("zahnarzt kontrolle", &[300], false), // same title, another event: counted together
        ev("Zahnarztpraxis", &[5, 6], false),
        ev("Zahnreinigung", &[1, 2, 3, 4, 5], false), // no match for "zahnarzt"
        ev("Zahnarzt Notdienst", &[-3], false),     // only in the future: no template
        ev("Zahnarzt abgesagt", &[1, 2, 3, 4, 5, 6], true), // cancelled
    ];
    assert_eq!(suggest_titles(&evs, "zahnarzt"), vec![("Zahnarzt Kontrolle".into(), 4), ("Zahnarztpraxis".into(), 2), ("Zahnarzt".into(), 1)]);
}

#[test]
fn suggestions_take_the_latest_occurrence_and_drop_typos_beside_clear_matches() {
    let evs = vec![ev("Yoga", &[30], false), ev("Yoga", &[2, 9], false), ev("Yogo Kurs", &[1, 2, 3, 4], false), ev("Kalender pflegen", &[7], false)];
    let now = NaiveDate::from_ymd_opt(2026, 9, 15).unwrap().and_hms_opt(12, 0, 0).unwrap();
    let mut q = Query::new("yoga");
    q.tier = 1;
    let s = suggestions(&evs, &q, now, 3);
    assert_eq!(s.len(), 1, "a typo (.5) falls below a whole word's floor");
    assert_eq!((s[0].count, s[0].key.as_str(), s[0].time.as_str()), (3, "k/Yoga/2", "t2"));
    assert_eq!(suggest_titles(&evs, "kalnder"), vec![("Kalender pflegen".into(), 1)], "a typo alone is still offered");
}
