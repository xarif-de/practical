//! Characterisation of import and export on the real store.

use super::*;
use rustical_store::CalendarReadStore;
use crate::testkit::{TestApp, ics, vevent, vtodo};

#[tokio::test]
async fn new_calendar_from_a_file() {
    let app = TestApp::new().await;
    let alice = app.user("alice", "Alice").await;
    let file = format!("{}{}", ics("X-WR-CALNAME:Datei\nX-WR-TIMEZONE:Europe/Berlin\nBEGIN:VEVENT\nUID:a\nDTSTAMP:20260101T000000Z\nSUMMARY:A\nDTSTART:20261001T100000Z\nEND:VEVENT\nBEGIN:VTODO\nUID:t\nDTSTAMP:20260101T000000Z\nSUMMARY:T\nEND:VTODO\nBEGIN:VEVENT\nUID:with/slash\nDTSTAMP:20260101T000000Z\nSUMMARY:S\nDTSTART:20261001T100000Z\nEND:VEVENT"), vevent("b", "B", "DTSTART:20261002T100000Z"));
    let n = collections::NewCalendar { owner: "alice".into(), name: " Import ".into(), color: "#010203".into(), description: String::new(), components: vec![CalendarObjectType::Journal], timezone: "Nowhere/Land".into() };
    let (id, count) = import_new_calendar(&app.state, &alice, n, file.as_bytes()).await.unwrap();
    assert_eq!(count, 3, "the UID with a slash is skipped");
    let cal = app.state.raw().calendars.get_calendar("alice", &id, false).await.unwrap();
    assert_eq!((cal.meta.displayname.as_deref(), cal.timezone_id.as_deref(), cal.components.iter().map(|c| c.as_str()).collect::<Vec<_>>()), (Some("Import"), None, vec!["VEVENT", "VTODO", "VJOURNAL"]), "the store keeps its own order");
    let a = app.state.raw().calendars.get_object("alice", &id, "a", false).await.unwrap().get_ics().to_string();
    assert!(!a.contains("X-WR-CALNAME"), "calendar-level lines stay out of the objects: {a}");
    let (name, out) = export_calendar(&app.state, &alice, "alice", &id).await.unwrap();
    assert_eq!(name, "Import");
    assert_eq!((out.matches("BEGIN:VEVENT").count(), out.matches("BEGIN:VTODO").count()), (2, 1));
    let empty = collections::NewCalendar { owner: "alice".into(), name: "x".into(), color: String::new(), description: String::new(), components: vec![], timezone: String::new() };
    assert!(matches!(import_new_calendar(&app.state, &alice, empty, ics("").as_bytes()).await, Err(AppError::BadRequest(m)) if m == "no calendar objects found"));
    // merge into an existing calendar: same UID replaces
    let merged = import_calendar(&app.state, &alice, "alice", &id, vtodo("t", "T neu", "").as_bytes()).await.unwrap();
    assert_eq!(merged, (1, 0));
    assert!(app.state.raw().calendars.get_object("alice", &id, "t", false).await.unwrap().get_ics().contains("SUMMARY:T neu"));
    assert!(matches!(import_calendar(&app.state, &alice, "alice", "missing", vtodo("t", "T", "").as_bytes()).await, Err(AppError::Store(_))));
    let bob = app.user("bob", "Bob").await;
    assert!(matches!(import_calendar(&app.state, &bob, "alice", &id, vtodo("t", "T", "").as_bytes()).await, Err(AppError::Forbidden)));
}

#[tokio::test]
async fn address_books_from_files() {
    let app = TestApp::new().await;
    let alice = app.user("alice", "Alice").await;
    let vcf = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Ohne UID\r\nEND:VCARD\r\nBEGIN:VCARD\r\nVERSION:3.0\r\nUID:mit\r\nFN:Mit UID\r\nEND:VCARD\r\n";
    assert!(matches!(import_new_addressbook(&app.state, &alice, "alice", "Neu", "blue", "", vcf.as_bytes()).await, Err(AppError::BadRequest(m)) if m == "color must be #rrggbb"));
    assert!(matches!(import_new_addressbook(&app.state, &alice, "alice", " ", "#000000", "", vcf.as_bytes()).await, Err(AppError::BadRequest(m)) if m == "name required"));
    let (id, n) = import_new_addressbook(&app.state, &alice, "alice", "Neu", "#000000", "D", vcf.as_bytes()).await.unwrap();
    assert_eq!(n, 2);
    let (name, out) = export_addressbook(&app.state, &alice, "alice", &id).await.unwrap();
    assert_eq!(name, "Neu");
    assert_eq!(out.matches("BEGIN:VCARD").count(), 2);
    assert_eq!(out.matches("UID:").count(), 2, "the card without UID got one");
    assert!(out.ends_with("END:VCARD\r\n"));
    assert_eq!(import_addressbook(&app.state, &alice, "alice", &id, "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:mit\r\nFN:Neu\r\nEND:VCARD\r\n".as_bytes()).await.unwrap(), 1);
    assert!(matches!(import_addressbook(&app.state, &alice, "alice", &id, b"").await, Err(AppError::BadRequest(m)) if m == "no vCards found"));
    assert!(matches!(import_new_addressbook(&app.state, &alice, "alice", "X", "#000000", "", b"garbage").await, Err(AppError::BadRequest(_))));
}

#[tokio::test]
async fn a_sloppy_ics_gets_its_required_properties() {
    let app = TestApp::new().await;
    let alice = app.user("alice", "Alice").await;
    let cal = app.calendar("alice", "Privat", "#0a84ff", &["VEVENT", "VTODO"]).await;
    // no PRODID: caldata parses it, but RustiCal could not read the stored objects back — they were hidden
    let no_prodid = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:np\r\nDTSTAMP:20261001T080000Z\r\nDTSTART:20261102T080000Z\r\nSUMMARY:Ohne PRODID\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    assert!(rustical_ical::CalendarObject::from_ics(CalendarObject::from(IcalParser::from_slice(no_prodid.as_bytes()).next().unwrap().unwrap().into_objects().unwrap().remove(0)).get_ics().to_string()).is_err(), "what the old import stored");
    assert_eq!(import_calendar(&app.state, &alice, "alice", &cal, no_prodid.as_bytes()).await.unwrap(), (1, 0));
    let stored = app.state.raw().calendars.get_objects("alice", &cal).await.unwrap();
    assert_eq!(stored.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(), ["np"], "stored and readable");
    assert!(stored[0].1.get_ics().contains(&format!("PRODID:{PRODID}")));
    // no VERSION, no UID, no DTSTAMP, lower-case names: supplied; the VALARM gets none of them
    let sloppy = "begin:vcalendar\nbegin:vevent\ndtstart:20261103T080000Z\nsummary:Ohne alles\nBEGIN:VALARM\nACTION:DISPLAY\nTRIGGER:-PT5M\nEND:VALARM\nend:vevent\nBEGIN:VTODO\nUID:td\nSUMMARY:Aufgabe\nEND:VTODO\nend:vcalendar\n";
    let done = complete_ics(sloppy);
    assert!(done.starts_with(&format!("BEGIN:VCALENDAR\r\nPRODID:{PRODID}\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:")) && done.matches("DTSTAMP:").count() == 2 && done.matches("UID:").count() == 2, "{done}");
    let (id, n) = import_new_calendar(&app.state, &alice, collections::NewCalendar { owner: "alice".into(), name: "Lose".into(), color: String::new(), description: String::new(), components: vec![], timezone: String::new() }, sloppy.as_bytes()).await.unwrap();
    assert_eq!(n, 2);
    assert_eq!(app.state.raw().calendars.get_objects("alice", &id).await.unwrap().len(), 2, "the count is what got stored");
    // a complete file is handed on untouched
    let full = ics(&vevent("x", "X", "DTSTART:20261001T100000Z"));
    assert!(matches!(complete_ics(&full), std::borrow::Cow::Borrowed(_)));
}

#[tokio::test]
async fn a_vcard_2_1_file_imports() {
    let app = TestApp::new().await;
    let alice = app.user("alice", "Alice").await;
    let ab = app.addressbook("alice", "Alt", "#000000").await;
    let vcf = "BEGIN:VCARD\r\nVERSION:2.1\r\nUID:v21\r\nN:Alt;Otto;;;\r\nFN:Otto Alt\r\nTEL;CELL;VOICE:0151 1\r\nEMAIL;INTERNET:otto@example.org\r\nEND:VCARD\r\n";
    assert_eq!(import_addressbook(&app.state, &alice, "alice", &ab, vcf.as_bytes()).await.unwrap(), 1);
    let (_, out) = export_addressbook(&app.state, &alice, "alice", &ab).await.unwrap();
    assert!(out.contains("TEL;TYPE=CELL,VOICE:0151 1") || out.contains("TEL;TYPE=CELL;TYPE=VOICE:0151 1"), "{out}");
}
