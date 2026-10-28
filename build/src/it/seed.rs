//! The synthetic world most route tests start from (no personal data): alice and bob share the group
//! `group.familie`, carol has nothing. Every date lies in October 2026, so the views asked for
//! `date=2026-10-14` show all of it whatever day the tests run on.

use crate::testkit::{TestApp, vevent, vtodo};

pub struct Seed {
    pub app: TestApp,
    pub arbeit: String,
    pub privat: String,
    pub aufgaben: String,
    pub familie: String,
    pub freunde: String,
}

pub async fn seed() -> Seed {
    let app = TestApp::new().await;
    app.user("alice", "Alice Beispiel").await;
    app.user("bob", "Bob Muster").await;
    app.user("carol", "Carol Test").await;
    app.group("group.familie", "Familie", &["alice", "bob"]).await;
    let arbeit = app.calendar("alice", "Arbeit", "#0a84ff", &["VEVENT", "VTODO"]).await;
    let privat = app.calendar("alice", "Privat", "#30b46c", &["VEVENT"]).await;
    let aufgaben = app.calendar("alice", "Aufgaben", "#ff9f0a", &["VTODO"]).await;
    let familie = app.calendar("group.familie", "Familienkalender", "#af52de", &["VEVENT", "VTODO"]).await;
    let freunde = app.addressbook("alice", "Freunde", "#ff2d55").await;

    // Arbeit: a weekly series with an exception and a reminder, a rich single event, a 3-day fair, a cancelled one
    app.put_ics("alice", &arbeit, "standup", &vevent("standup", "Standup", "DTSTART;TZID=Europe/Berlin:20260902T090000\nDTEND;TZID=Europe/Berlin:20260902T091500\nRRULE:FREQ=WEEKLY;BYDAY=WE\nEXDATE;TZID=Europe/Berlin:20261007T090000\nCATEGORIES:Arbeit\nBEGIN:VALARM\nACTION:DISPLAY\nDESCRIPTION:Standup\nTRIGGER:-PT10M\nEND:VALARM")).await;
    app.put_ics("alice", &arbeit, "review", &vevent("review", "Quartalsreview", "DTSTART;TZID=Europe/Berlin:20261014T140000\nDTEND;TZID=Europe/Berlin:20261014T153000\nLOCATION:Raum 4.12\\, Hauptgebäude\nGEO:51.050407;13.737262\nURL:https://example.org/review\nSTATUS:TENTATIVE\nPRIORITY:1\nCLASS:CONFIDENTIAL\nTRANSP:TRANSPARENT\nDESCRIPTION:Zahlen mitbringen\nORGANIZER;CN=Bob Muster:mailto:bob@example.org\nATTENDEE;CN=Anna Schmidt;PARTSTAT=ACCEPTED:mailto:anna@example.org\nCATEGORIES:Arbeit,Meeting\nRELATED-TO:standup")).await;
    app.put_ics("alice", &arbeit, "messe", &vevent("messe", "Messe", "DTSTART;VALUE=DATE:20261020\nDTEND;VALUE=DATE:20261023")).await;
    app.put_ics("alice", &arbeit, "abgesagt", &vevent("abgesagt", "Abgesagter Termin", "DTSTART;TZID=Europe/Berlin:20261015T100000\nDTEND;TZID=Europe/Berlin:20261015T110000\nSTATUS:CANCELLED")).await;
    // Privat: a vacation (category rule target), a coloured appointment, a UTC event, a yearly birthday
    app.put_ics("alice", &privat, "urlaub", &vevent("urlaub", "Urlaub Ostsee", "DTSTART;VALUE=DATE:20261026\nDTEND;VALUE=DATE:20261031\nCATEGORIES:Urlaub")).await;
    app.put_ics("alice", &privat, "zahnarzt", &vevent("zahnarzt", "Zahnarzt Dr. Weiß", "DTSTART;TZID=Europe/Berlin:20261016T083000\nDTEND;TZID=Europe/Berlin:20261016T091500\nCOLOR:tomato\nCLASS:PRIVATE")).await;
    app.put_ics("alice", &privat, "utc", &vevent("utc", "Abendessen", "DTSTART:20261014T180000Z\nDTEND:20261014T200000Z")).await;
    app.put_ics("alice", &privat, "bday", &vevent("bday", "Geburtstag Oma", "DTSTART;VALUE=DATE:19500312\nDTEND;VALUE=DATE:19500313\nRRULE:FREQ=YEARLY")).await;
    // Aufgaben: open with a due date, done, cancelled, open without any date
    app.put_ics("alice", &aufgaben, "steuer", &vtodo("steuer", "Steuererklärung", "DUE;VALUE=DATE:20261015\nPRIORITY:1\nCATEGORIES:Haushalt")).await;
    app.put_ics("alice", &aufgaben, "fenster", &vtodo("fenster", "Fenster putzen", "DUE;VALUE=DATE:20261001\nSTATUS:COMPLETED\nCOMPLETED:20261001T100000Z\nPERCENT-COMPLETE:100")).await;
    app.put_ics("alice", &aufgaben, "abgesagt", &vtodo("abgesagt", "Garage ausmisten", "DUE;VALUE=DATE:20261002\nSTATUS:CANCELLED")).await;
    app.put_ics("alice", &aufgaben, "irgendwann", &vtodo("irgendwann", "Keller sortieren", "")).await;
    // the group's calendar
    app.put_ics("group.familie", &familie, "treffen", &vevent("treffen", "Familientreffen", "DTSTART;VALUE=DATE:20261018\nDTEND;VALUE=DATE:20261019")).await;
    // Freunde: a full card, an organisation
    app.put_vcf("alice", &freunde, "anna", "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:anna\r\nFN:Anna Schmidt\r\nN:Schmidt;Anna;;;\r\nEMAIL;TYPE=HOME:anna@example.org\r\nTEL;TYPE=CELL:+49 160 1234567\r\nBDAY:1985-04-02\r\nCATEGORIES:Familie\r\nNOTE:Mag Tee\r\nEND:VCARD\r\n").await;
    app.put_vcf("alice", &freunde, "acme", "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:acme\r\nFN:ACME GmbH\r\nN:;;;;\r\nORG:ACME GmbH;Einkauf\r\nEND:VCARD\r\n").await;
    // alice names her holiday regions. Holidays have no default (prefs::Prefs::regions), so without
    // this every test that expects a holiday or a school-holiday band in a view would see none.
    let mut p = crate::domain::prefs::Prefs::defaults();
    p.public_regions = Some("DE-SN".into());
    p.school_regions = Some("DE-SN".into());
    crate::domain::prefs::set(&app.state.db, "alice", &p).await.unwrap();
    Seed { app, arbeit, privat, aufgaben, familie, freunde }
}

/// [`seed`] plus sharing: alice shares her Privat with dave (read); bob shares his calendar Team and his address
/// book Teamkontakte with `group.team` = {erin} (write), and Teamkontakte with dave (read), through the settings routes. Team holds a public event
/// with bob's reminder, a private one with an attached file (sharees see only its time: Team's levels are set to 1/0)
/// and a confidential one (hidden from them), a public task and a private one, each hidden part carrying the word
/// "Marker7" (the attachment's text too).
pub struct SharedSeed {
    pub s: Seed,
    pub team: String,
    pub team_ab: String,
    /// the names of the copies in their sharees' homes
    pub privat_copy: String,
    pub team_copy: String,
    pub team_ab_copy: String,
    /// dave's read copy of Teamkontakte
    pub team_ab_dave_copy: String,
}

pub async fn seed_shared() -> SharedSeed {
    let s = seed().await;
    let app = &s.app;
    app.user("dave", "Dave Leser").await;
    app.user("erin", "Erin Schreiber").await;
    app.group("group.team", "Team", &["erin"]).await;
    let team = app.calendar("bob", "Team", "#5856d6", &["VEVENT", "VTODO"]).await;
    let team_ab = app.addressbook("bob", "Teamkontakte", "#34c759").await;
    app.put_ics("bob", &team, "plan", &vevent("plan", "Teamplan", "DTSTART;TZID=Europe/Berlin:20261014T100000\nDTEND;TZID=Europe/Berlin:20261014T110000\nBEGIN:VALARM\nACTION:DISPLAY\nDESCRIPTION:Bobalarm\nTRIGGER:-PT15M\nEND:VALARM")).await;
    app.put_ics("bob", &team, "busy", &vevent("busy", "Geheimprojekt Marker7", "DTSTART;TZID=Europe/Berlin:20261015T100000\nDTEND;TZID=Europe/Berlin:20261015T110000\nLOCATION:Marker7-Keller\nCLASS:PRIVATE\nATTACH;FMTTYPE=text/plain;FILENAME=Marker7.txt;ENCODING=BASE64;VALUE=BINARY:TWFya2VyNw==")).await;
    app.put_ics("bob", &team, "gone", &vevent("gone", "Verborgen Marker7", "DTSTART;TZID=Europe/Berlin:20261016T100000\nDTEND;TZID=Europe/Berlin:20261016T110000\nCLASS:CONFIDENTIAL")).await;
    app.put_ics("bob", &team, "todo", &vtodo("todo", "Teamaufgabe", "DUE;VALUE=DATE:20261020")).await;
    app.put_ics("bob", &team, "geheim", &vtodo("geheim", "Geheimaufgabe Marker7", "DUE;VALUE=DATE:20261017\nDESCRIPTION:Notiz Marker7\nCLASS:PRIVATE")).await;
    app.put_vcf("bob", &team_ab, "tina", "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:tina\r\nFN:Tina Team\r\nN:Team;Tina;;;\r\nEMAIL:tina@example.org\r\nEND:VCARD\r\n").await;
    let mut bob = app.login("bob").await;
    let r = bob.post_form(&format!("/settings/calendars/bob/{team}/share"), &[("sharee", "group.team"), ("access", "write")]).await;
    assert_eq!(r.status, axum::http::StatusCode::SEE_OTHER, "{}", r.body);
    bob.post_form(&format!("/settings/addressbooks/bob/{team_ab}/share"), &[("sharee", "group.team"), ("access", "write")]).await;
    bob.post_form(&format!("/settings/addressbooks/bob/{team_ab}/share"), &[("sharee", "dave"), ("access", "read")]).await;
    let mut alice = app.login("alice").await;
    alice.post_form(&format!("/settings/calendars/alice/{}/share", s.privat), &[("sharee", "dave"), ("access", "read")]).await;
    // the names of the copies as their owners' share lists give them
    let names = |owner: &'static str, id: String, ab: bool, sharee: &'static str| async move {
        let o = app.principal(owner).await;
        let c = app.state.collections();
        let v: Vec<String> = if ab {
            let a = crate::domain::collections::ensure_addressbook(&app.state, &o, owner, &id, crate::state::Need::Own).await.unwrap();
            c.addressbook_shares(&a).await.unwrap().into_iter().filter(|x| x.sharee == sharee).map(|x| x.name).collect()
        } else {
            let a = crate::domain::collections::ensure_calendar(&app.state, &o, owner, &id, crate::state::Need::Own).await.unwrap();
            c.calendar_shares(&a).await.unwrap().into_iter().filter(|x| x.sharee == sharee).map(|x| x.name).collect()
        };
        assert_eq!(v.len(), 1, "{owner}/{id} → {sharee}: {v:?}");
        v.into_iter().next().unwrap()
    };
    let team_copy = names("bob", team.clone(), false, "group.team").await;
    let team_ab_copy = names("bob", team_ab.clone(), true, "group.team").await;
    let team_ab_dave_copy = names("bob", team_ab.clone(), true, "dave").await;
    let privat_copy = names("alice", s.privat.clone(), false, "dave").await;
    // Team's levels: private events show their time (every calendar hides both unless set)
    let bob = app.principal("bob").await;
    let a = crate::domain::collections::ensure_calendar(&app.state, &bob, "bob", &team, crate::state::Need::Own).await.unwrap();
    app.state.collections().set_visibility(&a, crate::state::Policy::new(1, 0).unwrap()).await.unwrap();
    SharedSeed { s, team, team_ab, privat_copy, team_copy, team_ab_copy, team_ab_dave_copy }
}
