//! The CalDAV/CardDAV client: discovery, the collections we may offer, and the two reports that
//! move objects. Read-only methods plus the three conditional writes a sync needs — never
//! MKCALENDAR, never a PROPPATCH, so a remote collection's own settings are untouched.
//!
//! Everything here was measured against nextcloud 35.0.1 (`e2e/nextcloud-dev.sh`), the version
//! plan.xarif.de runs; `PLAN.md` records each measurement next to the decision it drove.
//!
//! - [`Dav::discover`] walks `.well-known` → `current-user-principal` → the two home sets. The
//!   `.well-known` hop is a 301 and is the ONLY redirect this client follows (one hop, same origin).
//! - [`Dav::collections`] applies the five-condition filter of [`offerable`]: a shared collection is
//!   refused because nextcloud strips `CLASS:PRIVATE` entirely and guts `CONFIDENTIAL` for a sharee,
//!   so importing one would silently store wrong data.
//! - [`Dav::changes`] is `sync-collection`; a token that comes back LOWER than the one we sent means
//!   the remote counter was reset, which [`Changes::reset`] reports so the caller reconciles fully.
//! - Writes are conditional ([`Dav::put`], [`Dav::delete`]) and never trust a 412 to mean "conflict":
//!   nextcloud answers 412 for an absent href too, so the caller re-reads with [`Dav::head`].

use crate::error::{AppError, AppResult};
use std::fmt;

/// The app password. Its Debug and Display print nothing, so it cannot reach a log or an error
/// message by accident — the same hazard `domain::push` guards for endpoints.
#[derive(Clone)]
pub struct Secret(pub String);

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

/// Which of the two protocols a collection speaks. The reports and data elements differ, the rest
/// of the machinery does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Cal,
    Ab,
}

impl Kind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cal => "cal",
            Self::Ab => "ab",
        }
    }

    pub fn parse(s: &str) -> AppResult<Self> {
        match s {
            "cal" => Ok(Self::Cal),
            "ab" => Ok(Self::Ab),
            _ => Err(AppError::bad("err-remote-kind")),
        }
    }

    /// The collection resourcetype that makes a collection eligible at all (filter condition 1).
    pub(super) const fn resourcetype(self) -> (&'static str, &'static str) {
        match self {
            Self::Cal => (NS_CALDAV, "calendar"),
            Self::Ab => (NS_CARDDAV, "addressbook"),
        }
    }

    pub(super) const fn multiget(self) -> (&'static str, &'static str) {
        match self {
            Self::Cal => (NS_CALDAV, "calendar-multiget"),
            Self::Ab => (NS_CARDDAV, "addressbook-multiget"),
        }
    }

    pub(super) const fn data(self) -> (&'static str, &'static str) {
        match self {
            Self::Cal => (NS_CALDAV, "calendar-data"),
            Self::Ab => (NS_CARDDAV, "address-data"),
        }
    }

    pub const fn ext(self) -> &'static str {
        match self {
            Self::Cal => "ics",
            Self::Ab => "vcf",
        }
    }
}

/// The most one answer may be. A batch of `IMPORT_CHUNK` objects at the per-object cap is far
/// larger than this, which is why `bodies` also halves its batch on a hit: the limit binds the
/// answer, not the request.
pub const MAX_RESPONSE: usize = 16 * 1024 * 1024;

pub const NS_DAV: &str = "DAV:";
pub const NS_CALDAV: &str = "urn:ietf:params:xml:ns:caldav";
pub const NS_CARDDAV: &str = "urn:ietf:params:xml:ns:carddav";
pub const NS_CS: &str = "http://calendarserver.org/ns/";
/// Nextcloud's own properties are in nextcloud.COM, not .org — measured; the wiki lists both and
/// only one is what the server sends for `deleted-at` / `deleted-calendar` / `trash-bin`.
pub const NS_NC: &str = "http://nextcloud.com/ns";
pub const NS_ICAL: &str = "http://apple.com/ns/ical/";

/// One remote collection as the home listing describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Collection {
    pub href: String,
    pub name: String,
    pub color: String,
    pub description: String,
    /// TZID extracted from the collection's VTIMEZONE, empty when absent or unparseable
    pub timezone: String,
    /// "VEVENT,VTODO" as the remote declares it; empty for an address book
    pub components: Vec<String>,
    pub writable: bool,
    pub ctag: String,
}

/// What one `sync-collection` round learned.
#[derive(Debug, Default)]
pub struct Changes {
    /// (href, etag) of objects to fetch
    pub changed: Vec<(String, String)>,
    pub gone: Vec<String>,
    pub token: String,
    /// The remote counter went backwards (restore from backup, collection recreated): the caller
    /// must reconcile fully instead of trusting this round.
    pub reset: bool,
}

/// The outcome of a conditional write. `etag` is None when the server rewrote the body and answered
/// without one (measured: nextcloud repairs a missing PRODID and sends `X-Sabre-Ew-Gross` instead),
/// so the caller must read the stored object back.
#[derive(Debug)]
pub enum Wrote {
    Ok { etag: Option<String> },
    /// The precondition failed. NOT necessarily a conflict: nextcloud answers this for an absent
    /// href too, so the caller re-reads before deciding.
    Precondition,
    /// The UID already exists under another href. Nextcloud names that href in the
    /// `no-uid-conflict` body (measured), which is the only usable value: falling back to the href
    /// we just tried cannot work, because the server refused it precisely because the object lives
    /// somewhere else. `None` = the body named none.
    UidTaken(Option<String>),
    /// The server refused the body itself. Counted and skipped; never retried in a loop.
    Refused(u16),
}

pub struct Dav {
    http: reqwest::Client,
    base: url::Url,
    login: String,
    secret: Secret,
}

impl Dav {
    /// `base` is the origin [`super::origin_of`] accepted; nothing here re-opens that decision, and
    /// every server-supplied href is checked against it before it is dialled. Whether the origin
    /// may be reached at all is the resolver's call ([`super::client`]).
    pub fn new(http: reqwest::Client, base: url::Url, login: String, secret: Secret) -> Self {
        Self { http, base, login, secret }
    }

    /// The absolute URL of a server-supplied path, refused when it leaves the allowed origin — a
    /// server must not be able to walk us elsewhere with a crafted href.
    fn at(&self, href: &str) -> AppResult<url::Url> {
        let u = self.base.join(href).map_err(|_| AppError::bad("err-remote-href"))?;
        if (u.scheme(), u.host_str(), u.port_or_known_default()) != (self.base.scheme(), self.base.host_str(), self.base.port_or_known_default()) {
            return Err(AppError::bad("err-remote-href"));
        }
        Ok(u)
    }

    fn req(&self, method: &str, u: url::Url) -> AppResult<reqwest::RequestBuilder> {
        let m = reqwest::Method::from_bytes(method.as_bytes()).map_err(|e| AppError::Internal(e.to_string()))?;
        // the credential goes in a header, never in the URL: a reqwest error carries the URL and
        // would then print it
        Ok(self.http.request(m, u).basic_auth(&self.login, Some(&self.secret.0)))
    }

    /// Sends a request and maps the status to our errors. The body is returned as text for the XML
    /// reader; a failure names only the status, never a header or body dump.
    ///
    /// The body is read with a ceiling ([`MAX_RESPONSE`]): a DAV answer has no declared length, so
    /// without one a server (or a wrong href) could hand us more than the pod's 192 MiB. Exceeding
    /// it is an error, never a short read — a truncated multistatus looks exactly like a shorter
    /// list, and a full reconciliation answers a shorter list by deleting locally.
    async fn send(&self, rb: reqwest::RequestBuilder) -> AppResult<(reqwest::StatusCode, reqwest::header::HeaderMap, String)> {
        use futures_util::StreamExt;
        let r = rb.send().await.map_err(net_err)?;
        let (status, headers) = (r.status(), r.headers().clone());
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(AppError::Rejected(axum::http::StatusCode::UNAUTHORIZED, crate::error::Msg { key: "err-remote-auth", args: vec![] }));
        }
        if let Some(wait) = retry_after(&headers).filter(|_| status.as_u16() == 429 || status.as_u16() == 503) {
            return Err(AppError::BadRequest(crate::error::Msg { key: "err-remote-busy", args: vec![("seconds", wait.to_string())] }));
        }
        let mut buf: Vec<u8> = Vec::new();
        let mut body = r.bytes_stream();
        while let Some(chunk) = body.next().await {
            let chunk = chunk.map_err(net_err)?;
            if buf.len() + chunk.len() > MAX_RESPONSE {
                return Err(AppError::BadRequest(crate::error::Msg { key: "err-remote-big", args: vec![("limit", MAX_RESPONSE.to_string())] }));
            }
            buf.extend_from_slice(&chunk);
        }
        // from_utf8 takes the buffer over; lossy would COPY it (Cow::Borrowed → owned), holding
        // both at up to MAX_RESPONSE each at the peak of every request
        let text = String::from_utf8(buf).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned());
        Ok((status, headers, text))
    }

    /// RFC 6764 discovery. Returns (calendar home, address book home).
    ///
    /// The `.well-known` hop is a 301 (measured; `PROPFIND /` answers 405, so it is not optional)
    /// and is read by hand because the client keeps `redirect::Policy::none()`: exactly one hop,
    /// only from this path, only to the same origin.
    pub async fn discover(&self) -> AppResult<(String, String)> {
        let mut root = "/.well-known/caldav".to_string();
        for _ in 0..1 {
            let r = self.req("GET", self.at(&root)?)?.send().await.map_err(net_err)?;
            if !r.status().is_redirection() {
                break;
            }
            let loc = r.headers().get(reqwest::header::LOCATION).and_then(|v| v.to_str().ok()).ok_or_else(|| AppError::bad("err-remote-discover"))?;
            // `at` refuses a cross-origin target, which is the whole point of following by hand
            root = self.at(loc)?.path().to_string();
        }
        let (_, _, body) = self
            .send(self.req("PROPFIND", self.at(&root)?)?.header("Depth", "0").header(reqwest::header::CONTENT_TYPE, "application/xml").body(propfind(&[(NS_DAV, "current-user-principal")])))
            .await?;
        let principal = super::xml::first_href(&body, NS_DAV, "current-user-principal").ok_or_else(|| AppError::bad("err-remote-discover"))?;
        let (_, _, body) = self
            .send(self.req("PROPFIND", self.at(&principal)?)?.header("Depth", "0").header(reqwest::header::CONTENT_TYPE, "application/xml").body(propfind(&[
                (NS_CALDAV, "calendar-home-set"),
                (NS_CARDDAV, "addressbook-home-set"),
            ])))
            .await?;
        let cal = super::xml::first_href(&body, NS_CALDAV, "calendar-home-set").ok_or_else(|| AppError::bad("err-remote-discover"))?;
        let ab = super::xml::first_href(&body, NS_CARDDAV, "addressbook-home-set").unwrap_or_default();
        Ok((cal, ab))
    }

    /// The `current-user-principal` of this login: the ONLY value the owner filter may compare
    /// against, and in exactly the string form `{DAV:}owner` arrives in (measured: both are
    /// `/remote.php/dav/principals/users/<u>/`, trailing slash included).
    pub async fn principal(&self) -> AppResult<String> {
        let (_, _, body) = self
            .send(self.req("PROPFIND", self.at("/remote.php/dav/")?)?.header("Depth", "0").header(reqwest::header::CONTENT_TYPE, "application/xml").body(propfind(&[(NS_DAV, "current-user-principal")])))
            .await?;
        super::xml::first_href(&body, NS_DAV, "current-user-principal").ok_or_else(|| AppError::bad("err-remote-discover"))
    }

    /// The collections of one home we may offer, and the principal they must belong to.
    pub async fn collections(&self, home: &str, kind: Kind, principal: &str) -> AppResult<Vec<Collection>> {
        let (_, _, body) = self
            .send(self.req("PROPFIND", self.at(home)?)?.header("Depth", "1").header(reqwest::header::CONTENT_TYPE, "application/xml").body(props_for(kind)))
            .await?;
        super::xml::collections(&body, kind, principal).ok_or_else(cut)
    }

    /// `sync-collection` with the stored token; an empty token asks for everything.
    pub async fn changes(&self, href: &str, token: &str) -> AppResult<Changes> {
        let body = format!(
            r#"<?xml version="1.0" encoding="utf-8"?><d:sync-collection xmlns:d="DAV:"><d:sync-token>{}</d:sync-token><d:sync-level>1</d:sync-level><d:prop><d:getetag/></d:prop></d:sync-collection>"#,
            super::xml::esc(token)
        );
        let (status, _, text) = self.send(self.req("REPORT", self.at(href)?)?.header("Depth", "0").header(reqwest::header::CONTENT_TYPE, "application/xml").body(body)).await?;
        if !status.is_success() {
            // an unusable token answers DAV:valid-sync-token; that is a reset, not a failure
            if text.contains("valid-sync-token") {
                return Ok(Changes { reset: true, ..Default::default() });
            }
            return Err(dav_err(status));
        }
        let mut c = super::xml::changes(&text).ok_or_else(cut)?;
        // A token coming back LOWER than the one we sent means the remote counter was reset
        // (measured: sending .../9999 against a collection at .../17 answers 207, zero responses
        // and .../17 — no error at all). Without this the objects that changed before our stale
        // token would never be learned by any later round.
        c.reset = c.reset || counter(&c.token).zip(counter(token)).is_some_and(|(back, sent)| back < sent);
        Ok(c)
    }

    /// Every href with its etag, bodies left out: the listing the periodic full reconciliation
    /// compares against, and the only way to notice what a lying token hid.
    /// The collection's current sync-token, for a full pass to start the next incremental round
    /// from. Measured against nextcloud 35.0.1: this answers 466 bytes, while asking
    /// `sync-collection` with an empty token — which is what a full pass used to do for the token
    /// alone — is an INITIAL sync and answers the complete listing a second time (1 919 bytes for
    /// five objects, and it grows with the collection).
    pub async fn sync_token(&self, href: &str) -> AppResult<String> {
        let (status, _, text) = self
            .send(
                self.req("PROPFIND", self.at(href)?)?
                    .header("Depth", "0")
                    .header(reqwest::header::CONTENT_TYPE, "application/xml")
                    .body(propfind(&[(NS_DAV, "sync-token")])),
            )
            .await?;
        if !status.is_success() {
            return Err(dav_err(status));
        }
        super::xml::prop_text(&text, NS_DAV, "sync-token").ok_or_else(cut)
    }

    pub async fn list(&self, href: &str, kind: Kind) -> AppResult<Vec<(String, String)>> {
        let (method, body) = match kind {
            // a calendar-query with a bare VCALENDAR filter is the cheapest complete listing
            Kind::Cal => (
                "REPORT",
                format!(
                    r#"<?xml version="1.0" encoding="utf-8"?><c:calendar-query xmlns:d="DAV:" xmlns:c="{NS_CALDAV}"><d:prop><d:getetag/></d:prop><c:filter><c:comp-filter name="VCALENDAR"/></c:filter></c:calendar-query>"#
                ),
            ),
            // CardDAV has no such report; a plain PROPFIND is the equivalent
            Kind::Ab => ("PROPFIND", propfind(&[(NS_DAV, "getetag")])),
        };
        let (status, _, text) = self.send(self.req(method, self.at(href)?)?.header("Depth", "1").header(reqwest::header::CONTENT_TYPE, "application/xml").body(body)).await?;
        if !status.is_success() {
            return Err(dav_err(status));
        }
        // the collection itself answers too and carries no etag: `etags` leaves those out
        super::xml::etags(&text).ok_or_else(cut)
    }

    /// The bodies of up to a batch of hrefs. A href the server no longer has is simply absent from
    /// the answer (measured) — the same signal as a 404 row in a sync report.
    ///
    /// An answer over [`MAX_RESPONSE`] splits the batch and retries each half, down to one href: a
    /// single huge object is then the only thing that can fail, and it fails alone instead of
    /// taking its batch with it.
    pub async fn bodies(&self, href: &str, kind: Kind, hrefs: &[String]) -> AppResult<Vec<(String, String, String)>> {
        if hrefs.is_empty() {
            return Ok(vec![]);
        }
        match self.bodies_once(href, kind, hrefs).await {
            Err(AppError::BadRequest(m)) if m.key == "err-remote-big" && hrefs.len() > 1 => {
                let (a, b) = hrefs.split_at(hrefs.len() / 2);
                let mut out = Box::pin(self.bodies(href, kind, a)).await?;
                out.extend(Box::pin(self.bodies(href, kind, b)).await?);
                Ok(out)
            }
            other => other,
        }
    }

    async fn bodies_once(&self, href: &str, kind: Kind, hrefs: &[String]) -> AppResult<Vec<(String, String, String)>> {
        let (ns, report) = kind.multiget();
        let mut body = format!(r#"<?xml version="1.0" encoding="utf-8"?><r:{report} xmlns:d="DAV:" xmlns:r="{ns}"><d:prop><d:getetag/><r:{}/></d:prop>"#, kind.data().1);
        for h in hrefs {
            body.push_str(&format!("<d:href>{}</d:href>", super::xml::esc(h)));
        }
        body.push_str(&format!("</r:{report}>"));
        let (status, _, text) = self.send(self.req("REPORT", self.at(href)?)?.header("Depth", "1").header(reqwest::header::CONTENT_TYPE, "application/xml").body(body)).await?;
        if !status.is_success() {
            return Err(dav_err(status));
        }
        super::xml::bodies(&text, kind).ok_or_else(cut)
    }

    /// (etag, body) of one object, or None when it is gone. Used to resolve a 412 and to read back
    /// an object the server rewrote.
    pub async fn get(&self, href: &str) -> AppResult<Option<(String, String)>> {
        let (status, headers, text) = self.send(self.req("GET", self.at(href)?)?).await?;
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !status.is_success() {
            return Err(dav_err(status));
        }
        Ok(Some((header(&headers, reqwest::header::ETAG), text)))
    }

    /// Writes one object. `etag` None = create (`If-None-Match: *`), Some = update (`If-Match`).
    pub async fn put(&self, href: &str, kind: Kind, etag: Option<&str>, body: String) -> AppResult<Wrote> {
        let mime = match kind {
            Kind::Cal => "text/calendar; charset=utf-8",
            Kind::Ab => "text/vcard; charset=utf-8",
        };
        let rb = self.req("PUT", self.at(href)?)?.header(reqwest::header::CONTENT_TYPE, mime);
        let rb = match etag {
            Some(e) => rb.header(reqwest::header::IF_MATCH, e),
            None => rb.header(reqwest::header::IF_NONE_MATCH, "*"),
        };
        let (status, headers, text) = self.send(rb.body(body)).await?;
        Ok(match status.as_u16() {
            // measured: a repaired body answers without an ETag (X-Sabre-Ew-Gross), so None here
            // means "read the stored object back" rather than "no change"
            200 | 201 | 204 => Wrote::Ok { etag: Some(header(&headers, reqwest::header::ETAG)).filter(|e| !e.is_empty()) },
            412 => Wrote::Precondition,
            // only a UID clash may be resolved by adopting the other href; any other 409 is a
            // refusal of this body
            409 if text.contains("no-uid-conflict") => Wrote::UidTaken(super::xml::first_href(&text, NS_CALDAV, "no-uid-conflict").or_else(|| super::xml::first_href(&text, NS_CARDDAV, "no-uid-conflict"))),
            s => Wrote::Refused(s),
        })
    }

    pub async fn delete(&self, href: &str, etag: &str) -> AppResult<Wrote> {
        let (status, _, _) = self.send(self.req("DELETE", self.at(href)?)?.header(reqwest::header::IF_MATCH, etag)).await?;
        Ok(match status.as_u16() {
            200 | 204 | 404 => Wrote::Ok { etag: None },
            412 => Wrote::Precondition,
            s => Wrote::Refused(s),
        })
    }

    /// The etag of one href, or None when it is gone: what resolves a 412 into "conflict" or
    /// "already in that state".
    pub async fn head(&self, href: &str) -> AppResult<Option<String>> {
        let (status, headers, _) = self.send(self.req("HEAD", self.at(href)?)?).await?;
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !status.is_success() {
            return Err(dav_err(status));
        }
        Ok(Some(header(&headers, reqwest::header::ETAG)))
    }
}

fn header(h: &reqwest::header::HeaderMap, name: reqwest::header::HeaderName) -> String {
    h.get(name).and_then(|v| v.to_str().ok()).unwrap_or_default().to_string()
}

fn retry_after(h: &reqwest::header::HeaderMap) -> Option<u64> {
    h.get(reqwest::header::RETRY_AFTER)?.to_str().ok()?.trim().parse().ok()
}

/// A transport failure, named by its kind only: a reqwest error's Display carries the URL, and a URL
/// is the one place a credential could still appear.
///
/// The resolver's refusal gets its OWN message. It is the most likely failure of a typed-in address
/// and the only one a user cannot guess from "not reachable": the name resolves fine, it just does
/// not resolve to somewhere this server is allowed to go.
pub fn net_err(e: reqwest::Error) -> AppError {
    if refused_by_resolver(&e) {
        return AppError::bad("err-remote-private");
    }
    let what = if e.is_timeout() {
        "timeout"
    } else if e.is_connect() {
        "connect"
    } else if e.is_decode() {
        "decode"
    } else {
        "request"
    };
    AppError::BadRequest(crate::error::Msg { key: "err-remote-net", args: vec![("what", what.to_string())] })
}

/// Did `PublicOrAllowed` refuse the name? Its error text is the only marker reqwest carries it in,
/// so the string is matched against the one this crate produces (`super::PublicOrAllowed`).
fn refused_by_resolver(e: &reqwest::Error) -> bool {
    let mut src: Option<&(dyn std::error::Error + 'static)> = Some(e);
    while let Some(x) = src {
        if x.to_string().contains("no public address") {
            return true;
        }
        src = x.source();
    }
    false
}

/// The answer was not a whole document. Its own error, because reading a cut multistatus as a
/// SHORT list is the one failure that can delete a user's objects: a full reconciliation treats
/// "not in the listing" as "the remote no longer has it".
fn cut() -> AppError {
    AppError::BadRequest(crate::error::Msg { key: "err-remote-cut", args: vec![] })
}

fn dav_err(status: reqwest::StatusCode) -> AppError {
    AppError::BadRequest(crate::error::Msg { key: "err-remote-status", args: vec![("status", status.as_u16().to_string())] })
}

/// The trailing number of a sabre sync token (`http://sabre.io/ns/sync/7`). None for any other
/// shape, which the caller treats as "cannot compare".
pub(super) fn counter(token: &str) -> Option<u64> {
    token.rsplit('/').next()?.parse().ok()
}

fn propfind(props: &[(&str, &str)]) -> String {
    let mut s = String::from(r#"<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:"><d:prop>"#);
    for (i, (ns, name)) in props.iter().enumerate() {
        if *ns == NS_DAV {
            s.push_str(&format!("<d:{name}/>"));
        } else {
            s.push_str(&format!(r#"<x{i}:{name} xmlns:x{i}="{ns}"/>"#));
        }
    }
    s.push_str("</d:prop></d:propfind>");
    s
}

/// The property set the filter needs. It is deliberately wide: measured, nextcloud reports a
/// trashed calendar's resourcetype as a plain `cal:calendar` when the request asks for the
/// resourcetype ALONE, and only names `deleted-calendar` for a wider set. `deleted-at` is asked for
/// as an independent second condition so the filter does not rest on that quirk.
fn props_for(kind: Kind) -> String {
    let mut p = vec![
        (NS_DAV, "resourcetype"),
        (NS_DAV, "displayname"),
        (NS_DAV, "owner"),
        (NS_DAV, "current-user-privilege-set"),
        (NS_DAV, "sync-token"),
        (NS_CS, "getctag"),
        (NS_CS, "source"),
        (NS_NC, "deleted-at"),
    ];
    if kind == Kind::Cal {
        p.extend([
            (NS_CALDAV, "supported-calendar-component-set"),
            (NS_CALDAV, "calendar-timezone"),
            (NS_CALDAV, "calendar-description"),
            (NS_ICAL, "calendar-color"),
        ]);
    } else {
        p.push((NS_CARDDAV, "addressbook-description"));
    }
    propfind(&p)
}

/// Five conditions, every one of which must hold. Measured against nextcloud 35.0.1; `PLAN.md` §4
/// carries the table this came from.
///
/// 1. the collection's own resourcetype — drops a webcal subscription (`cs:subscribed` WITHOUT
///    `cal:calendar`), the scheduling inbox/outbox and the trash bin;
/// 2. no `deleted-at` — a trashed calendar, whose resourcetype alone cannot be relied on;
/// 3. the owner is this very principal — drops every share, and with it the data loss a sharee
///    cannot see (PRIVATE withheld, CONFIDENTIAL reduced to a busy slot), plus nextcloud 35's
///    federated calendars and the system address book;
/// 4. not a collection nextcloud generates from other data (the birthday calendar, the
///    `z-*-generated--*` address books): importing one copies a snapshot that never updates;
/// 5. the owner was readable at all — an unreadable owner is refused, never guessed.
pub fn offerable(rt: &[(String, String)], deleted: bool, owner: Option<&str>, principal: &str, name: &str, kind: Kind) -> bool {
    let (ns, local) = kind.resourcetype();
    rt.iter().any(|(n, l)| n == ns && l == local)
        && !deleted
        && owner.is_some_and(|o| o == principal)
        && !generated(name)
}

/// Collections nextcloud derives from something else. The birthday calendar is also not writable
/// (measured: PUT answers 403), but the name check is what keeps it out of a pull-only link too.
fn generated(name: &str) -> bool {
    name == "contact_birthdays" || name.starts_with("z-app-generated--") || name.starts_with("z-server-generated--")
}

