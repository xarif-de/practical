//! Reading a WebDAV multistatus. One streaming pass per answer with `quick-xml`, which is already
//! in the lock file through RustiCal.
//!
//! The one thing that must not be hand-rolled here is unescaping: EVERY text node arrives
//! XML-escaped, not just the etags. Measured — a `SUMMARY:A & B <tag> "q"` comes back as
//! `A &amp; B &lt;tag&gt; &quot;q&quot;`, so a reader that only strips quotes from etags would store
//! `&amp;` in the user's events, push that back, and escape it again on every round.
//! `BytesText::xml_content` is what decodes entities and normalises line ends.
//!
//! Namespaces are resolved, not guessed from a prefix: nextcloud binds its own elements to
//! `http://nextcloud.com/ns` under whatever prefix it likes (measured: `x1`, `x2`), and the one
//! element whose namespace actually decides something (`deleted-at`) is one of those.

use super::dav::{Changes, Collection, Kind, NS_CALDAV, NS_CARDDAV, NS_CS, NS_DAV, NS_ICAL, NS_NC};
use quick_xml::events::Event;
use quick_xml::name::ResolveResult;
use quick_xml::{NsReader, Reader, XmlVersion};

/// Escapes a value we put INTO a request body.
pub fn esc(s: &str) -> String {
    quick_xml::escape::escape(s).into_owned()
}

/// A resolved element name as owned strings, so the walker can keep a stack of them.
type Qn = (String, String);

/// What the walker yields. `Text` is already unescaped.
/// One entity as its character. The five XML built-ins plus numeric references, which is all a DAV
/// answer uses — a server-defined entity would need a DTD, which none of them sends.
fn entity(r: &quick_xml::events::BytesRef<'_>) -> Option<char> {
    if let Ok(Some(c)) = r.resolve_char_ref() {
        return Some(c);
    }
    match r.as_ref() {
        "lt" => Some('<'),
        "gt" => Some('>'),
        "amp" => Some('&'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => None,
    }
}

enum Step {
    Open(Qn),
    Empty(Qn),
    Close(Qn),
    Text(String),
}

/// How deeply elements may nest. A multistatus needs six levels
/// (multistatus > response > propstat > prop > resourcetype > element); object bodies arrive as TEXT
/// and add none. Without a limit the path stack grows with the answer: 16 MiB of `<a>` tags
/// (`dav::MAX_RESPONSE`) is ~5.5 million entries, and `inside()` walks the stack per event.
const MAX_DEPTH: usize = 32;

/// One streaming pass with a path stack, which is all the state any of the readers below needs.
struct Walk<'a> {
    r: NsReader<&'a [u8]>,
    buf: Vec<u8>,
    path: Vec<Qn>,
    /// The document ended in the middle of something. Never ignored: a truncated answer read as a
    /// SHORT list is indistinguishable from "the server no longer has these objects", and a full
    /// reconciliation answers that by deleting them locally.
    broken: bool,
}

impl<'a> Walk<'a> {
    /// `trim_text` stays OFF: it would strip the spaces and line ends INSIDE an object body, which
    /// arrives as text here — `SUMMARY:A & B` came out as `A&B`. Whitespace-only text between two
    /// elements matches no reader's branch, so leaving it in costs nothing; the readers trim the
    /// fields where trimming is right (href, etag, token) and keep bodies byte for byte.
    fn new(xml: &'a str) -> Self {
        Self { r: NsReader::from_reader(xml.as_bytes()), buf: vec![], path: vec![], broken: false }
    }

    fn next(&mut self) -> Option<Step> {
        loop {
            self.buf.clear();
            // `broken`, not a silent stop: see the field's own comment
            if self.path.len() > MAX_DEPTH {
                self.broken = true;
                return None;
            }
            let (ns, ev) = match self.r.read_resolved_event_into(&mut self.buf) {
                Ok(v) => v,
                Err(_) => {
                    self.broken = true;
                    return None;
                }
            };
            let ns = match ns {
                ResolveResult::Bound(n) => n.as_ref().to_string(),
                _ => String::new(),
            };
            match ev {
                Event::Start(e) => {
                    let q = (ns, e.local_name().as_ref().to_string());
                    self.path.push(q.clone());
                    return Some(Step::Open(q));
                }
                Event::Empty(e) => {
                    return Some(Step::Empty((ns, e.local_name().as_ref().to_string())));
                }
                Event::End(_) => {
                    return match self.path.pop() {
                        Some(q) => Some(Step::Close(q)),
                        // a closing tag with nothing open: the text is not a document
                        None => {
                            self.broken = true;
                            None
                        }
                    };
                }
                Event::Text(t) => {
                    // the only correct place to unescape, and it covers bodies as well as etags
                    let s = t.xml_content(XmlVersion::Implicit1_0).into_owned();
                    if !s.is_empty() {
                        return Some(Step::Text(s));
                    }
                }
                Event::CData(c) => {
                    let s = c.xml_content(XmlVersion::Implicit1_0).into_owned();
                    if !s.is_empty() {
                        return Some(Step::Text(s));
                    }
                }
                // quick-xml 0.42 reports every entity as its OWN event, so the text around one
                // arrives in pieces: `&quot;e1&quot;` is Ref, Text("e1"), Ref. Dropping these
                // would silently lose every quote, `&` and `<` the server sent — the one failure
                // this reader exists to prevent, because it would also be pushed back and
                // re-escaped on the next round.
                Event::GeneralRef(r) => {
                    if let Some(c) = entity(&r) {
                        return Some(Step::Text(c.to_string()));
                    }
                }
                Event::Eof => {
                    // an element still open at the end means the body was cut
                    self.broken = !self.path.is_empty();
                    return None;
                }
                _ => {}
            }
        }
    }

    fn inside(&self, ns: &str, name: &str) -> bool {
        self.path.iter().any(|(n, l)| n == ns && l == name)
    }

    fn parent_is(&self, ns: &str, name: &str) -> bool {
        self.path.last().is_some_and(|(n, l)| n == ns && l == name)
    }
}

/// The first `<d:href>` inside the named property — what discovery needs three times.
pub fn first_href(xml: &str, ns: &str, prop: &str) -> Option<String> {
    let mut w = Walk::new(xml);
    let mut want = false;
    while let Some(s) = w.next() {
        match s {
            Step::Open(_) => want = w.inside(ns, prop) && w.inside(NS_DAV, "href"),
            Step::Text(t) if want => return Some(t.trim().to_string()),
            _ => {}
        }
    }
    None
}

/// One `<d:response>` of a collection listing.
#[derive(Default)]
struct Resp {
    href: String,
    rt: Vec<Qn>,
    name: String,
    owner: Option<String>,
    privs: Vec<String>,
    ctag: String,
    source: String,
    deleted: bool,
    color: String,
    description: String,
    timezone: String,
}

/// One `<d:propstat>` while it is being read.
///
/// Buffered, because the `<d:status>` that says whether these properties EXIST comes AFTER them.
/// A 404 propstat lists the properties the server does not have, as EMPTY elements — so taking
/// them as they arrive reads `<nc:deleted-at/>` in a 404 block as "this collection is deleted" and
/// refuses every collection on the server. Measured against nextcloud 35.0.1, which answers
/// exactly that way for a live calendar.
#[derive(Default)]
struct Stat {
    rt: Vec<Qn>,
    name: String,
    owner: Option<String>,
    privs: Vec<String>,
    ctag: String,
    source: String,
    deleted: bool,
    color: String,
    description: String,
    timezone: String,
}

impl Stat {
    /// Folds a 200 propstat into the response. A 404 one is dropped unread.
    fn apply(self, r: &mut Resp) {
        r.rt.extend(self.rt);
        r.privs.extend(self.privs);
        r.deleted |= self.deleted;
        for (dst, src) in [
            (&mut r.name, self.name),
            (&mut r.ctag, self.ctag),
            (&mut r.source, self.source),
            (&mut r.color, self.color),
            (&mut r.description, self.description),
            (&mut r.timezone, self.timezone),
        ] {
            if !src.is_empty() {
                *dst = src;
            }
        }
        if self.owner.is_some() {
            r.owner = self.owner;
        }
    }
}

/// The offerable collections of one home listing, already filtered. `None` = the answer was cut,
/// which must not be read as "there are none".
pub fn collections(xml: &str, kind: Kind, principal: &str) -> Option<Vec<Collection>> {
    let mut out: Vec<Collection> = vec![];
    let mut w = Walk::new(xml);
    let (mut r, mut st, mut ok) = (Resp::default(), Stat::default(), true);
    while let Some(s) = w.next() {
        match &s {
            Step::Open(q) if q == &(NS_DAV.into(), "response".into()) => r = Resp::default(),
            Step::Open(q) if q == &(NS_DAV.into(), "propstat".into()) => (st, ok) = (Stat::default(), true),
            Step::Close(q) if q == &(NS_DAV.into(), "propstat".into()) => {
                if ok {
                    std::mem::take(&mut st).apply(&mut r);
                }
            }
            Step::Close(q) if q == &(NS_DAV.into(), "response".into()) => {
                let name = r.href.trim_end_matches('/').rsplit('/').next().unwrap_or_default().to_string();
                // a webcal subscription has a source and no calendar resourcetype; the source is
                // read anyway because it is the honest reason a row is missing
                if r.source.is_empty() && super::dav::offerable(&r.rt, r.deleted, r.owner.as_deref(), principal, &name, kind) {
                    out.push(Collection {
                        href: r.href.clone(),
                        name: if r.name.is_empty() { name } else { r.name.clone() },
                        /* A colour is free text from a foreign server and ends up in a `style`
                           attribute of the pick list and in the local collection. Quoting the
                           attribute is not enough: `red;position:fixed;inset:0` carries no quote
                           and would cover the page. Checked by the predicate the local collections
                           use, and dropped rather than refusing the row — a collection is still
                           offerable without a colour. */
                        color: crate::domain::collections::valid_color(&r.color).then(|| r.color.clone()).unwrap_or_default(),
                        description: r.description.clone(),
                        timezone: tzid(&r.timezone),
                        components: vec![],
                        // write-content is the privilege a read-only sharee loses
                        writable: r.privs.iter().any(|p| p == "write" || p == "write-content" || p == "all"),
                        ctag: r.ctag.clone(),
                    });
                }
            }
            // resourcetype children and privilege names are empty elements
            Step::Empty(q) if w.parent_is(NS_DAV, "resourcetype") => st.rt.push(q.clone()),
            Step::Open(q) if w.inside(NS_DAV, "resourcetype") && q != &(NS_DAV.to_string(), "resourcetype".to_string()) => st.rt.push(q.clone()),
            Step::Empty(q) if w.parent_is(NS_DAV, "privilege") => st.privs.push(q.1.clone()),
            // an empty deleted-at still says "deleted" — but only inside a 200 propstat, which is
            // what the buffering above decides
            Step::Empty(q) if q == &(NS_NC.into(), "deleted-at".into()) => st.deleted = true,
            Step::Text(t) if !t.trim().is_empty() => {
                let t = t.trim().to_string();
                if w.inside(NS_DAV, "status") {
                    // "HTTP/1.1 200 OK" vs "HTTP/1.1 404 Not Found": only a 200 block holds values
                    ok = t.contains(" 200");
                } else if w.inside(NS_DAV, "owner") && w.inside(NS_DAV, "href") {
                    st.owner = Some(t);
                } else if w.parent_is(NS_DAV, "href") && r.href.is_empty() {
                    // the response's own href sits outside every propstat
                    r.href = t;
                } else if w.parent_is(NS_DAV, "displayname") {
                    st.name.push_str(&t);
                } else if w.parent_is(NS_CS, "getctag") {
                    st.ctag.push_str(&t);
                } else if w.parent_is(NS_CS, "source") || (w.inside(NS_CS, "source") && w.inside(NS_DAV, "href")) {
                    st.source.push_str(&t);
                } else if w.parent_is(NS_NC, "deleted-at") {
                    st.deleted = true;
                } else if w.parent_is(NS_ICAL, "calendar-color") {
                    st.color.push_str(&t);
                } else if w.parent_is(NS_CALDAV, "calendar-description") || w.parent_is(NS_CARDDAV, "addressbook-description") {
                    st.description.push_str(&t);
                } else if w.parent_is(NS_CALDAV, "calendar-timezone") {
                    st.timezone.push_str(&t);
                }
            }
            _ => {}
        }
    }
    if w.broken {
        return None;
    }
    // the component set lives in an attribute, which the walk above does not carry
    for c in &mut out {
        c.components = comps(xml, &c.href);
    }
    Some(out)
}

/// `<cal:comp name="VEVENT"/>` inside the response of one href.
fn comps(xml: &str, href: &str) -> Vec<String> {
    let mut r = Reader::from_reader(xml.as_bytes());
    r.config_mut().trim_text(true);
    let (mut buf, mut here, mut cur, mut out, mut in_href) = (vec![], false, String::new(), vec![], false);
    loop {
        buf.clear();
        match r.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let l = e.local_name().as_ref().to_string();
                if l == "response" {
                    (here, cur) = (false, String::new());
                }
                in_href = l == "href";
            }
            Ok(Event::Text(t)) if in_href && cur.is_empty() => {
                cur = t.xml_content(XmlVersion::Implicit1_0).trim().to_string();
                here = cur == href;
            }
            Ok(Event::Empty(e)) if here && e.local_name().as_ref() == "comp" => {
                if let Some(a) = e.attributes().flatten().find(|a| a.key.as_ref() == "name") {
                    out.push(a.value.into_owned());
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

/// The TZID out of a collection's VTIMEZONE. Nextcloud sends a whole component, not a bare id, and
/// `import_new_calendar` drops a zone it cannot parse — so we hand it an id or nothing.
fn tzid(vtimezone: &str) -> String {
    vtimezone
        .lines()
        .find_map(|l| l.trim().strip_prefix("TZID:").map(str::trim))
        .filter(|t| t.parse::<chrono_tz::Tz>().is_ok())
        .unwrap_or_default()
        .to_string()
}

/// A `sync-collection` answer: changed hrefs with etags, gone hrefs, and the new token.
/// A response whose status says 404 is a deletion; the rest carry an etag.
pub fn changes(xml: &str) -> Option<Changes> {
    let mut c = Changes::default();
    let mut w = Walk::new(xml);
    let (mut href, mut etag, mut gone) = (String::new(), String::new(), false);
    while let Some(s) = w.next() {
        match &s {
            Step::Open(q) if q == &(NS_DAV.into(), "response".into()) => (href, etag, gone) = (String::new(), String::new(), false),
            Step::Close(q) if q == &(NS_DAV.into(), "response".into()) => {
                if !href.is_empty() {
                    if gone {
                        c.gone.push(std::mem::take(&mut href));
                    } else {
                        c.changed.push((std::mem::take(&mut href), std::mem::take(&mut etag)));
                    }
                }
            }
            Step::Text(t) if !t.trim().is_empty() => {
                let t = t.trim().to_string();
                // the multistatus-level token, which is not inside a response
                if w.parent_is(NS_DAV, "sync-token") && !w.inside(NS_DAV, "response") {
                    c.token.push_str(&t);
                } else if w.parent_is(NS_DAV, "href") && href.is_empty() {
                    href = t.clone();
                } else if w.parent_is(NS_DAV, "getetag") {
                    etag.push_str(&t);
                } else if w.parent_is(NS_DAV, "status") && t.contains(" 404") {
                    gone = true;
                }
            }
            _ => {}
        }
    }
    (!w.broken).then_some(c)
}

/// (href, etag) of every response that has an etag — the collection itself has none, so it drops
/// out without a special case.
pub fn etags(xml: &str) -> Option<Vec<(String, String)>> {
    let mut out = vec![];
    let mut w = Walk::new(xml);
    let (mut href, mut etag) = (String::new(), String::new());
    while let Some(s) = w.next() {
        match &s {
            Step::Open(q) if q == &(NS_DAV.into(), "response".into()) => (href, etag) = (String::new(), String::new()),
            Step::Close(q) if q == &(NS_DAV.into(), "response".into()) => {
                if !href.is_empty() && !etag.is_empty() {
                    out.push((std::mem::take(&mut href), std::mem::take(&mut etag)));
                }
            }
            Step::Text(t) if !t.trim().is_empty() => {
                let t = t.trim().to_string();
                if w.parent_is(NS_DAV, "href") && href.is_empty() {
                    href = t;
                } else if w.parent_is(NS_DAV, "getetag") {
                    etag.push_str(&t);
                }
            }
            _ => {}
        }
    }
    (!w.broken).then_some(out)
}

/// (href, etag, body) of a multiget answer.
pub fn bodies(xml: &str, kind: Kind) -> Option<Vec<(String, String, String)>> {
    let (dns, dlocal) = kind.data();
    let mut out = vec![];
    let mut w = Walk::new(xml);
    let (mut href, mut etag, mut body) = (String::new(), String::new(), String::new());
    while let Some(s) = w.next() {
        match &s {
            Step::Open(q) if q == &(NS_DAV.into(), "response".into()) => (href, etag, body) = (String::new(), String::new(), String::new()),
            Step::Close(q) if q == &(NS_DAV.into(), "response".into()) => {
                if !href.is_empty() && !body.is_empty() {
                    out.push((std::mem::take(&mut href), std::mem::take(&mut etag), std::mem::take(&mut body)));
                }
            }
            Step::Text(t) => {
                if w.parent_is(dns, dlocal) {
                    // a body arrives in pieces (an entity splits it), and every piece counts
                    body.push_str(t);
                } else if !t.trim().is_empty() {
                    let t = t.trim();
                    if w.parent_is(NS_DAV, "href") && href.is_empty() {
                        href = t.to_string();
                    } else if w.parent_is(NS_DAV, "getetag") {
                        etag.push_str(t);
                    }
                }
            }
            _ => {}
        }
    }
    (!w.broken).then_some(out)
}
