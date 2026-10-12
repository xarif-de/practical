//! The filter, the XML reader and the fingerprint rule, against answers measured from
//! nextcloud 35.0.1 (`e2e/nextcloud-dev.sh`). Every fixture below is a real answer, trimmed —
//! writing them by hand would only pin what we believe the server sends.

use super::dav::{Kind, offerable};
use super::xml;

const HOME: &str = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:s="http://sabredav.org/ns" xmlns:cal="urn:ietf:params:xml:ns:caldav" xmlns:cs="http://calendarserver.org/ns/" xmlns:card="urn:ietf:params:xml:ns:carddav" xmlns:oc="http://owncloud.org/ns" xmlns:nc="http://nextcloud.org/ns" xmlns:ocs="http://open-collaboration-services.org/ns" xmlns:ocm="http://open-cloud-mesh.org/ns">
 <d:response><d:href>/remote.php/dav/calendars/alice/work/</d:href><d:propstat><d:prop>
   <d:resourcetype><d:collection/><cal:calendar/></d:resourcetype>
   <d:displayname>work</d:displayname>
   <d:owner><d:href>/remote.php/dav/principals/users/alice/</d:href></d:owner>
   <d:current-user-privilege-set><d:privilege><d:write/></d:privilege><d:privilege><d:read/></d:privilege><d:privilege><d:write-content/></d:privilege></d:current-user-privilege-set>
   <cs:getctag>http://sabre.io/ns/sync/3</cs:getctag>
   <x1:calendar-color xmlns:x1="http://apple.com/ns/ical/">#0082c9</x1:calendar-color>
  </d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
 <d:response><d:href>/remote.php/dav/calendars/alice/bobs-plan_shared_by_bob/</d:href><d:propstat><d:prop>
   <d:resourcetype><d:collection/><cal:calendar/></d:resourcetype>
   <d:displayname>bobs-plan (Bob)</d:displayname>
   <d:owner><d:href>/remote.php/dav/principals/users/bob/</d:href></d:owner>
  </d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
 <d:response><d:href>/remote.php/dav/calendars/alice/bobs-readonly_shared_by_bob/</d:href><d:propstat><d:prop>
   <d:resourcetype><d:collection/><cal:calendar/></d:resourcetype>
   <d:displayname>bobs-readonly (Bob)</d:displayname>
   <d:owner><d:href>/remote.php/dav/principals/users/bob/</d:href></d:owner>
   <d:current-user-privilege-set><d:privilege><d:read/></d:privilege></d:current-user-privilege-set>
  </d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
 <d:response><d:href>/remote.php/dav/calendars/alice/contact_birthdays/</d:href><d:propstat><d:prop>
   <d:resourcetype><d:collection/><cal:calendar/></d:resourcetype>
   <d:displayname>Contact birthdays</d:displayname>
   <d:owner><d:href>/remote.php/dav/principals/users/alice/</d:href></d:owner>
  </d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
 <d:response><d:href>/remote.php/dav/calendars/alice/trashed/</d:href><d:propstat><d:prop>
   <d:resourcetype><d:collection/><x2:deleted-calendar xmlns:x2="http://nextcloud.com/ns"/></d:resourcetype>
   <d:displayname>trashed</d:displayname>
   <d:owner><d:href>/remote.php/dav/principals/users/alice/</d:href></d:owner>
   <x2:deleted-at xmlns:x2="http://nextcloud.com/ns">2026-10-10T10:15:40+00:00</x2:deleted-at>
  </d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
 <d:response><d:href>/remote.php/dav/calendars/alice/holiday-feed/</d:href><d:propstat><d:prop>
   <d:resourcetype><d:collection/><cs:subscribed/></d:resourcetype>
   <d:displayname>holiday-feed</d:displayname>
   <d:owner><d:href>/remote.php/dav/principals/users/alice/</d:href></d:owner>
   <cs:source><d:href>https://www.officeholidays.com/ics/germany/saxony</d:href></cs:source>
  </d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
 <d:response><d:href>/remote.php/dav/calendars/alice/inbox/</d:href><d:propstat><d:prop>
   <d:resourcetype><d:collection/><cal:schedule-inbox/></d:resourcetype>
   <d:owner><d:href>/remote.php/dav/principals/users/alice/</d:href></d:owner>
  </d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
 <d:response><d:href>/remote.php/dav/calendars/alice/trashbin/</d:href><d:propstat><d:prop>
   <d:resourcetype><d:collection/><x2:trash-bin xmlns:x2="http://nextcloud.com/ns"/></d:resourcetype>
   <d:owner><d:href>/remote.php/dav/principals/users/alice/</d:href></d:owner>
  </d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
</d:multistatus>"#;

const ME: &str = "/remote.php/dav/principals/users/alice/";

#[test]
fn only_the_users_own_real_collections_are_offered() {
    let got: Vec<(String, bool)> = xml::collections(HOME, Kind::Cal, ME).unwrap().into_iter().map(|c| (c.name, c.writable)).collect();
    // work only: every other row of a real home listing is refused, each for its own reason
    assert_eq!(got, [("work".to_string(), true)]);
}

#[test]
fn each_refusal_has_its_own_reason() {
    // the five conditions, one case each, so a later change cannot quietly drop one of them
    let own = Some(ME);
    let cal = vec![(super::dav::NS_CALDAV.to_string(), "calendar".to_string())];
    let sub = vec![(super::dav::NS_CS.to_string(), "subscribed".to_string())];
    assert!(offerable(&cal, false, own, ME, "work", Kind::Cal), "an own calendar");
    assert!(!offerable(&sub, false, own, ME, "feed", Kind::Cal), "a webcal subscription has no calendar type");
    assert!(!offerable(&cal, true, own, ME, "trashed", Kind::Cal), "deleted-at refuses it whatever the resourcetype says");
    assert!(!offerable(&cal, false, Some("/remote.php/dav/principals/users/bob/"), ME, "shared", Kind::Cal), "a share: nextcloud withholds PRIVATE from a sharee");
    assert!(!offerable(&cal, false, own, ME, "contact_birthdays", Kind::Cal), "generated from the contacts");
    assert!(!offerable(&cal, false, own, ME, "z-app-generated--contactsinteraction--recent", Kind::Ab), "generated by an app");
    assert!(!offerable(&cal, false, None, ME, "work", Kind::Cal), "an unreadable owner is refused, never guessed");
    // the address book half uses its own resourcetype, so a calendar must not pass as one
    assert!(!offerable(&cal, false, own, ME, "contacts", Kind::Ab));
}

/// Measured: nextcloud answers a NARROW resourcetype request for a trashed calendar as a plain
/// calendar. `deleted-at` is the condition that holds regardless, which is why it exists.
#[test]
fn a_trashed_calendar_is_refused_even_when_its_resourcetype_lies() {
    let narrow = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:cal="urn:ietf:params:xml:ns:caldav" xmlns:nc="http://nextcloud.com/ns">
 <d:response><d:href>/remote.php/dav/calendars/alice/trashed/</d:href><d:propstat><d:prop>
   <d:resourcetype><d:collection/><cal:calendar/></d:resourcetype>
   <d:displayname>trashed</d:displayname>
   <d:owner><d:href>/remote.php/dav/principals/users/alice/</d:href></d:owner>
   <nc:deleted-at>2026-10-10T10:15:40+00:00</nc:deleted-at>
  </d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
</d:multistatus>"#;
    assert!(xml::collections(narrow, Kind::Cal, ME).unwrap().is_empty());
}

#[test]
fn a_missing_property_is_not_an_empty_one() {
    // a 404 propstat lists what the server does NOT have; its values must not be kept
    let x = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:cal="urn:ietf:params:xml:ns:caldav">
 <d:response><d:href>/remote.php/dav/calendars/alice/work/</d:href>
  <d:propstat><d:prop>
    <d:resourcetype><d:collection/><cal:calendar/></d:resourcetype>
    <d:owner><d:href>/remote.php/dav/principals/users/alice/</d:href></d:owner>
   </d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat>
  <d:propstat><d:prop><d:displayname/><cal:calendar-timezone/></d:prop><d:status>HTTP/1.1 404 Not Found</d:status></d:propstat>
 </d:response>
</d:multistatus>"#;
    let c = &xml::collections(x, Kind::Cal, ME).unwrap()[0];
    // no displayname: the href's last segment stands in, and no zone is claimed
    assert_eq!((c.name.as_str(), c.timezone.as_str()), ("work", ""));
}

#[test]
fn discovery_reads_the_principal_and_both_homes() {
    let x = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:cal="urn:ietf:params:xml:ns:caldav" xmlns:card="urn:ietf:params:xml:ns:carddav">
 <d:response><d:href>/remote.php/dav/principals/users/alice/</d:href><d:propstat><d:prop>
  <cal:calendar-home-set><d:href>/remote.php/dav/calendars/alice/</d:href></cal:calendar-home-set>
  <card:addressbook-home-set><d:href>/remote.php/dav/addressbooks/users/alice/</d:href></card:addressbook-home-set>
 </d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
</d:multistatus>"#;
    assert_eq!(xml::first_href(x, super::dav::NS_CALDAV, "calendar-home-set").unwrap(), "/remote.php/dav/calendars/alice/");
    assert_eq!(xml::first_href(x, super::dav::NS_CARDDAV, "addressbook-home-set").unwrap(), "/remote.php/dav/addressbooks/users/alice/");
    assert_eq!(xml::first_href(x, super::dav::NS_CALDAV, "nope"), None);
}

#[test]
fn a_sync_report_separates_changes_from_deletions() {
    let x = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:">
 <d:response><d:href>/remote.php/dav/calendars/alice/work/w1.ics</d:href><d:propstat><d:prop><d:getetag>&quot;a1aabd12&quot;</d:getetag></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
 <d:response><d:href>/remote.php/dav/calendars/alice/work/w3.ics</d:href><d:propstat><d:prop><d:getetag>&quot;33539634&quot;</d:getetag></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
 <d:response><d:href>/remote.php/dav/calendars/alice/work/w2.ics</d:href><d:status>HTTP/1.1 404 Not Found</d:status></d:response>
 <d:sync-token>http://sabre.io/ns/sync/6</d:sync-token>
</d:multistatus>"#;
    let c = xml::changes(x).unwrap();
    // the etags arrive HTML-escaped and must come out as the server's own value
    assert_eq!(c.changed, [("/remote.php/dav/calendars/alice/work/w1.ics".into(), "\"a1aabd12\"".into()), ("/remote.php/dav/calendars/alice/work/w3.ics".into(), "\"33539634\"".into())]);
    assert_eq!(c.gone, ["/remote.php/dav/calendars/alice/work/w2.ics"]);
    assert_eq!(c.token, "http://sabre.io/ns/sync/6");
    assert!(!c.reset);
}

/// Measured: EVERY text node is escaped, not just the etags. A reader that only unescapes etags
/// stores `&amp;` in the user's events, pushes that back, and escapes it again every round.
#[test]
fn a_body_comes_out_exactly_as_the_server_holds_it() {
    let x = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:cal="urn:ietf:params:xml:ns:caldav">
 <d:response><d:href>/c/x.ics</d:href><d:propstat><d:prop>
  <d:getetag>&quot;e1&quot;</d:getetag>
  <cal:calendar-data>BEGIN:VCALENDAR
SUMMARY:A &amp; B &lt;tag&gt; &quot;q&quot; &apos;a&apos;
END:VCALENDAR
</cal:calendar-data>
 </d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
</d:multistatus>"#;
    let b = xml::bodies(x, Kind::Cal).unwrap();
    assert_eq!(b.len(), 1);
    assert_eq!(b[0].1, "\"e1\"");
    assert!(b[0].2.contains(r#"SUMMARY:A & B <tag> "q" 'a'"#), "got {:?}", b[0].2);
}

#[test]
fn a_listing_leaves_the_collection_itself_out() {
    let x = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:">
 <d:response><d:href>/c/</d:href><d:propstat><d:prop/><d:status>HTTP/1.1 404 Not Found</d:status></d:propstat></d:response>
 <d:response><d:href>/c/a.ics</d:href><d:propstat><d:prop><d:getetag>&quot;e1&quot;</d:getetag></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
</d:multistatus>"#;
    assert_eq!(xml::etags(x).unwrap(), [("/c/a.ics".to_string(), "\"e1\"".to_string())]);
}

/// The whole point of `mine`: RustiCal does not store the text it is given, so a fingerprint taken
/// from our own bytes would never match and every object would ping-pong forever.
#[test]
fn the_fingerprint_is_of_the_text_rustical_stores_not_of_ours() {
    // lower-case names, a different fold, a redundant parameter: all things `generate()` rewrites
    let sloppy = "BEGIN:VCALENDAR\r\nversion:2.0\r\nprodid:-//x//y//EN\r\nBEGIN:VEVENT\r\nuid:e1\r\ndtstamp:20261001T120000Z\r\ndtstart;VALUE=DATE-TIME:20261012T090000Z\r\ndtend:20261012T100000Z\r\nsummary:Hello\r\n  world\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let fp = super::fingerprint(Kind::Cal, sloppy).expect("it parses");
    // what the store will hold for that text
    let stored = rustical_ical::CalendarObject::import(&crate::domain::transfer::complete_ics(sloppy), None).unwrap();
    assert_eq!(fp, stored.get_etag());
    // and it really is a different byte string than what we handed in, or this test proves nothing
    assert_ne!(stored.get_ics(), sloppy, "the normalisation must be visible, else the echo test is vacuous");
    // reading the stored text back gives the same fingerprint: that is what makes the comparison work
    assert_eq!(super::fingerprint(Kind::Cal, stored.get_ics()).unwrap(), fp);
}

#[test]
fn an_address_book_fingerprint_follows_the_other_rule() {
    // AddressObject has no `import`: `from_vcf` caches its input, so the rule is not the same one
    let vcf = "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:c1\r\nFN:Carla\r\nEND:VCARD\r\n";
    let fp = super::fingerprint(Kind::Ab, vcf).expect("it parses");
    let stored = rustical_ical::AddressObject::from_vcf(crate::domain::contacts::bare_params_as_type(vcf).into_owned()).unwrap();
    assert_eq!(fp, stored.get_etag());
    assert_eq!(super::fingerprint(Kind::Ab, stored.get_vcf()).unwrap(), fp);
}

#[test]
fn a_broken_body_is_an_error_not_a_panic() {
    assert!(super::fingerprint(Kind::Cal, "not a calendar at all").is_err());
    assert!(super::fingerprint(Kind::Ab, "not a card at all").is_err());
}

/// The answer nextcloud 35.0.1 really sends for the fixture's calendar home, byte for byte
/// (`e2e/nextcloud-dev.sh`, captured 2026-10-10). A hand-written fixture cannot catch what this
/// one does: the live answer puts the properties a collection does NOT have into a SECOND
/// propstat, as EMPTY elements, with its `404` status AFTER them — so a reader that takes values
/// as they arrive sees `<nc:deleted-at/>` on every live calendar and refuses the whole server.
#[test]
fn the_real_home_listing_offers_exactly_the_three_own_calendars() {
    let real = include_str!("fixtures/home-cal.xml");
    let mut got: Vec<String> = xml::collections(real, Kind::Cal, ME).unwrap().into_iter().map(|c| c.name).collect();
    got.sort();
    assert_eq!(got, ["mixed", "repeating", "work"]);
}

/// Same answer, same reader: the metadata a new local collection is prefilled from.
#[test]
fn the_real_listing_carries_the_metadata_the_import_needs() {
    let real = include_str!("fixtures/home-cal.xml");
    let work = xml::collections(real, Kind::Cal, ME).unwrap().into_iter().find(|c| c.name == "work").expect("work is offered");
    assert!(work.href.ends_with("/calendars/alice/work/"), "{}", work.href);
    assert!(work.writable, "an own calendar is writable");
    assert!(!work.ctag.is_empty(), "the ctag is what a cheap change check reads");
    // the fixture creates it through occ, which sets no colour: an empty one must not be invented
    // measured: nextcloud declares all three for an occ-made calendar, so the local copy accepts all three
    assert_eq!(work.components, ["VEVENT", "VTODO", "VJOURNAL"]);
}

/// Measured: a `multiget` hands out an event carrying `DTSTART;TZID=Europe/Berlin` with NO
/// `VTIMEZONE` beside it, which the strict parser refuses. RFC 7809 allows the omission for IANA
/// zones, and RustiCal's own CalDAV endpoint accepts it, so the sync must too — otherwise every
/// event a user created in a named zone is silently skipped.
#[test]
fn an_event_whose_timezone_block_is_missing_still_parses() {
    let no_vtimezone = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//x//y//EN\r\nBEGIN:VEVENT\r\nUID:tz1\r\nDTSTAMP:20261001T120000Z\r\nDTSTART;TZID=Europe/Berlin:20261012T090000\r\nDTEND;TZID=Europe/Berlin:20261012T100000\r\nSUMMARY:Standup\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    assert!(rustical_ical::CalendarObject::import(no_vtimezone, None).is_err(), "the strict parser refuses it, which is what bit us");
    let fp = super::fingerprint(Kind::Cal, no_vtimezone).expect("with rfc7809 it parses");
    assert!(!fp.is_empty());
    // and the fingerprint of the stored text matches, so echo suppression still holds for it
    let stored = rustical_ical::CalendarObject::import(&crate::domain::transfer::complete_ics(no_vtimezone), Some(super::parser_options())).unwrap();
    assert_eq!(super::fingerprint(Kind::Cal, stored.get_ics()).unwrap(), fp);
}

/// The conflict rule is "Nextcloud wins", which in the round means: an object in BOTH change lists
/// is dropped from the local push set, so the remote version is the one applied. Exercises the
/// production predicate (`round::pushable`), which is the part a live race cannot test reliably.
#[test]
fn an_object_changed_on_both_sides_is_not_pushed() {
    use super::round::pushable;
    let (changed_remote, gone_remote) = (["b"].map(String::from), ["c"].map(String::from));
    let remote_touched = |id: &str| changed_remote.iter().any(|x| x == id) || gone_remote.iter().any(|x| x == id);
    assert!(pushable("a", &remote_touched), "only this side changed it");
    assert!(!pushable("b", &remote_touched), "changed remotely too: remote wins");
    assert!(!pushable("c", &remote_touched), "deleted remotely: remote wins");
    assert!(pushable("d", &remote_touched), "the remote knows nothing about it");
}

/// A deeply nested answer must end the pass as BROKEN, not grow the path stack with it. Reported
/// as a cut answer, which is the one safe reading: a short list would be acted on by deleting
/// locally. Without the limit this allocates until the pod is killed.
#[test]
fn a_deeply_nested_answer_is_cut_not_followed() {
    let deep = format!(
        "<?xml version=\"1.0\"?><d:multistatus xmlns:d=\"DAV:\">{}{}</d:multistatus>",
        "<d:a>".repeat(500),
        "</d:a>".repeat(500)
    );
    assert!(xml::collections(&deep, Kind::Cal, "/p/alice/").is_none(), "a stack deeper than the limit is a broken answer");
    assert!(xml::etags(&deep).is_none(), "every reader stops, not just one");
    assert!(xml::changes(&deep).is_none());

    // the real answers are nowhere near the limit, so nothing legitimate is lost
    assert!(xml::collections(HOME, Kind::Cal, "/remote.php/dav/principals/users/alice/").is_some());
}

/// A colour is free text from a foreign server and ends up in a `style` attribute. Checked where it
/// enters, by the predicate the local collections use, so no consumer has to repeat it. The real
/// answer with its colour swapped, so the row stays exactly as nextcloud sends it.
#[test]
fn a_colour_the_remote_invented_is_dropped() {
    let own = "/remote.php/dav/principals/users/alice/";
    let color_of = |c: &str| {
        let answer = HOME.replace("#0082c9", c);
        xml::collections(&answer, Kind::Cal, own).unwrap().into_iter().find(|x| x.name == "work").map(|x| x.color)
    };
    assert_eq!(color_of("#0082c9").as_deref(), Some("#0082c9"), "a real colour is kept");
    assert_eq!(color_of("red;position:fixed;inset:0;z-index:99").as_deref(), Some(""), "CSS of its own is dropped");
    assert_eq!(color_of("rebeccapurple").as_deref(), Some(""), "so is a name the local collections would not take either");
    // the row itself survives: a collection without a colour is still offerable
    assert!(color_of("nonsense").is_some(), "the colour is dropped, not the collection");
}

/// A full pass needs the collection's token and nothing else. Measured against nextcloud 35.0.1:
/// this is the `Depth: 0` answer, 466 bytes, where asking `sync-collection` with an empty token
/// repeats the whole listing.
#[test]
fn the_token_of_a_collection_is_read_from_a_small_propfind() {
    let answer = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:s="http://sabredav.org/ns" xmlns:cal="urn:ietf:params:xml:ns:caldav" xmlns:cs="http://calendarserver.org/ns/" xmlns:oc="http://owncloud.org/ns" xmlns:nc="http://nextcloud.org/ns"><d:response><d:href>/remote.php/dav/calendars/alice/work/</d:href><d:propstat><d:prop><d:sync-token>http://sabre.io/ns/sync/22</d:sync-token></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response></d:multistatus>"#;
    assert_eq!(xml::prop_text(answer, "DAV:", "sync-token").as_deref(), Some("http://sabre.io/ns/sync/22"));
    // a server that does not answer the property leaves an empty token, which makes the next round
    // a full one again rather than trusting a token nobody gave us
    let without = answer.replace("<d:sync-token>http://sabre.io/ns/sync/22</d:sync-token>", "");
    assert_eq!(xml::prop_text(&without, "DAV:", "sync-token").as_deref(), Some(""));
    // and a cut answer stays an error, like every other reader
    assert!(xml::prop_text(&answer[..answer.len() - 40], "DAV:", "sync-token").is_none());
}

/// Our own push comes back in the next change list with the etag the server gave us. It must NOT
/// count as a remote change: `pushable` would then refuse the next local edit of that object, and
/// since a refused push is not a REFUSAL, the local token would advance past that edit and no later
/// round would report it again. Exercises the production function, so building the set without the
/// echo filter fails here.
#[test]
fn a_push_echo_is_not_a_remote_change() {
    use super::Mapped;
    use super::round::touched_remotely;
    let m = |id: &str, href: &str, etag: &str| Mapped {
        object_id: id.to_string(),
        uid: id.to_string(),
        href: href.to_string(),
        etag: etag.to_string(),
        mine: "fp".to_string(),
    };
    let map = [m("ours", "/c/ours.ics", "\"e1\""), m("theirs", "/c/theirs.ics", "\"e7\""), m("dropped", "/c/dropped.ics", "\"e9\"")];
    let by_href: std::collections::HashMap<&str, &Mapped> = map.iter().map(|x| (x.href.as_str(), x)).collect();
    let known = |h: &str| by_href.get(h).copied();

    let changed = [
        // what we wrote last round: same etag, so the remote did not touch it
        ("/c/ours.ics".to_string(), "\"e1\"".to_string()),
        // someone else wrote this one: a new etag
        ("/c/theirs.ics".to_string(), "\"e8\"".to_string()),
        // an href we have no record of, and one without an etag: neither is comparable
        ("/c/new.ics".to_string(), "\"e2\"".to_string()),
    ];
    let gone = ["/c/dropped.ics".to_string()];
    let touched = touched_remotely(&changed, &gone, &known);

    assert!(!touched.contains("ours"), "our own echo must leave the object pushable");
    assert!(touched.contains("theirs"), "a different etag is a real remote change");
    assert!(touched.contains("dropped"), "a remote deletion blocks a local push");
    assert_eq!(touched.len(), 2, "nothing else is touched: {touched:?}");

    // the predicate itself, at its edges
    use super::round::remote_echo;
    assert!(remote_echo("\"e1\"", known("/c/ours.ics")), "the etag we recorded");
    assert!(!remote_echo("", known("/c/ours.ics")), "no etag is never an echo");
    assert!(!remote_echo("\"e1\"", None), "nothing recorded for this href");
}

/// A sabre token whose counter went BACKWARDS means the remote was restored or recreated. Detected
/// in the same answer, with no extra request — measured: sending `…/sync/9999` to a collection at
/// `…/sync/17` answers 207, zero responses, and the token `…/sync/17`, with no error at all.
///
/// Exercises the production comparison (`dav::counter`), not a copy of it.
#[test]
fn a_token_going_backwards_is_read_as_a_reset() {
    use super::dav::counter;
    let back = |sent: &str, got: &str| counter(got).zip(counter(sent)).is_some_and(|(b, s)| b < s);
    assert!(back("http://sabre.io/ns/sync/9999", "http://sabre.io/ns/sync/17"), "the measured case");
    assert!(!back("http://sabre.io/ns/sync/17", "http://sabre.io/ns/sync/18"), "normal progress");
    assert!(!back("http://sabre.io/ns/sync/17", "http://sabre.io/ns/sync/17"), "nothing changed");
    assert!(!back("", "http://sabre.io/ns/sync/3"), "a first round cannot go backwards");
    // an unparseable shape cannot be compared, so it must not be mistaken for a reset here: the
    // DAV:valid-sync-token error is what covers that case
    assert!(!back("opaque-token", "another-opaque-token"));
    assert_eq!(counter("http://sabre.io/ns/sync/17"), Some(17));
    assert_eq!(counter("opaque"), None);
}

/// A cut answer must NEVER read as a shorter list. The full reconciliation treats "not in the
/// listing" as "the remote deleted it" and deletes locally, so a truncated body would delete the
/// user's objects. Every reader therefore answers None on a broken document.
#[test]
fn a_truncated_answer_is_an_error_not_a_short_list() {
    let whole = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:">
 <d:response><d:href>/c/a.ics</d:href><d:propstat><d:prop><d:getetag>&quot;e1&quot;</d:getetag></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
 <d:response><d:href>/c/b.ics</d:href><d:propstat><d:prop><d:getetag>&quot;e2&quot;</d:getetag></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
 <d:sync-token>http://sabre.io/ns/sync/9</d:sync-token>
</d:multistatus>"#;
    assert_eq!(xml::etags(whole).unwrap().len(), 2, "the whole document reads both");
    // cut after the first response: the second object and the token are missing
    let cut = &whole[..whole.find("/c/b.ics").unwrap()];
    assert!(xml::etags(cut).is_none(), "a cut listing must not look like one object");
    assert!(xml::changes(cut).is_none());
    assert!(xml::bodies(cut, Kind::Cal).is_none());
    assert!(xml::collections(cut, Kind::Cal, ME).is_none());
    // and something that is not XML at all is refused too
    assert!(xml::etags("<<<not xml").is_none());
}

/// With the address typed in, the resolver is the ONLY thing between the field and the pod's
/// neighbours: a name that resolves to a private address must not be dialled, however it was
/// spelled. `localhost` is the cheapest name that always resolves to loopback.
#[tokio::test]
async fn a_name_that_resolves_privately_is_not_dialled() {
    use reqwest::dns::Resolve;
    let bare = super::PublicOrAllowed { allow: std::sync::Arc::new(vec![]) };
    let e = bare.resolve("localhost".parse().unwrap()).await;
    assert!(e.is_err_and(|e| e.to_string().contains("no public address")), "loopback must be refused");

    // the operator's exemption, and ONLY for the host it names
    let allowed = super::PublicOrAllowed { allow: std::sync::Arc::new(vec!["localhost".to_string()]) };
    assert!(allowed.resolve("localhost".parse().unwrap()).await.is_ok(), "an exempt host resolves");
    assert!(
        super::PublicOrAllowed { allow: std::sync::Arc::new(vec!["other.invalid".to_string()]) }
            .resolve("localhost".parse().unwrap())
            .await
            .is_err(),
        "the exemption must not cover a different host"
    );
}

/// The exemption list is reduced to host names, because that is what a resolved name is. A scheme
/// or a port in the configured origin must not make the host unrecognisable, and an unusable entry
/// must be dropped rather than matched by accident.
#[test]
fn the_exemption_list_is_read_as_host_names() {
    let hosts = super::exempt_hosts(&["https://my.xarif.de".into(), "http://localhost:8099".into(), "not a url".into(), String::new()]);
    assert_eq!(hosts, ["my.xarif.de", "localhost"], "scheme and port dropped, nonsense dropped");
}

/// The one-off import and the sync must read a body by the SAME rule, or a calendar can be synced
/// but not imported. Measured during development: the import refused every event in a named zone
/// with "TZID Europe/Berlin does not refer to a valid timezone" while the sync took it, because
/// only the sync had been given the RFC 7809 option.
#[test]
fn the_import_and_the_sync_parse_a_body_by_one_rule() {
    assert_eq!(super::parser_options().rfc7809, crate::domain::transfer::options().rfc7809);
    assert!(crate::domain::transfer::options().rfc7809, "a TZID without its VTIMEZONE must be accepted");
    // and it really is the case the strict parser refuses, or this pins nothing
    let no_vtimezone = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//x//y//EN\r\nBEGIN:VEVENT\r\nUID:tz2\r\nDTSTAMP:20261001T120000Z\r\nDTSTART;TZID=Europe/Berlin:20261012T090000\r\nDTEND;TZID=Europe/Berlin:20261012T100000\r\nSUMMARY:Standup\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    assert!(rustical_ical::CalendarObject::import(no_vtimezone, None).is_err());
    assert!(rustical_ical::CalendarObject::import(no_vtimezone, Some(super::parser_options())).is_ok());
}
