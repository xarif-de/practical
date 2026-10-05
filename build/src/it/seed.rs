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
    Seed { app, arbeit, privat, aufgaben, familie, freunde }
}
