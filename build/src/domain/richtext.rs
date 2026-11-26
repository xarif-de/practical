//! Formatted notes: the HTML copies of an event's or task's DESCRIPTION that other apps keep, read and written
//! so each app shows the note as well as it can (docs/features.md "Formatted notes").
//!
//! iCalendar has no copy every app reads; each keeps its own beside the plain DESCRIPTION (which stays the
//! authoritative text, RFC 5545):
//! - Thunderbird: `DESCRIPTION;ALTREP="data:text/html,<percent-encoded>"` (comm-central CalItemBase.sys.mjs
//!   `descriptionHTML`: exactly that prefix, `encodeURIComponent`; a plain edit there drops it).
//! - Outlook desktop and Evolution: `X-ALT-DESC;FMTTYPE=text/html:<html document>` (MS-OXCICAL 2.1.3.1.1.20.27;
//!   Evolution e-comp-editor-property-parts.c). Thunderbird, Apple, Android keep it untouched on a plain edit.
//! - RFC 9073 §6.5: `STYLED-DESCRIPTION;VALUE=TEXT;FMTTYPE=text/html` — no app known to read it; written for apps
//!   to come. Its "DESCRIPTION should be absent or DERIVED=TRUE" is not followed on purpose: DERIVED forbids
//!   every app to change the note, and the plain-only apps (Apple, Android) are most of them.
//! - Google: the HTML itself in DESCRIPTION (Thunderbird `fixGoogleDescription`, Bugzilla 1792078), recognised by
//!   `X-MOZ-GOOGLE-HTML-DESCRIPTION` or a UID ending in `@google.com`.
//!
//! Reading ([`stored`]) hands out every copy found, sanitised, in that order of precedence; the editor takes the
//! first one whose text still matches DESCRIPTION (another app's plain edit leaves the others stale:
//! js/editor/note.js `ntSet`). Writing ([`lines`]): a note without formatting is plain DESCRIPTION only; with
//! formatting, plain DESCRIPTION + ALTREP + X-ALT-DESC + STYLED-DESCRIPTION, or for Google the HTML in DESCRIPTION.
//! Every HTML that is read or written passes [`clean`]: the one allowlist (`TAGS`), the same as note.js `ntClean`.

use std::collections::{HashMap, HashSet};

use super::{events::prop_name, ical};

/// The properties that hold a formatted copy besides DESCRIPTION; a save that writes the note rewrites them.
pub const COPIES: &[&str] = &["X-ALT-DESC", "STYLED-DESCRIPTION"];

/// The tags a formatted note keeps (js/editor/note.js NT_KEEP + p, span); attributes: `a href`, `span style`
/// (color and background-color only).
const TAGS: &[&str] = &["b", "strong", "i", "em", "u", "s", "strike", "del", "ins", "br", "p", "div", "ul", "ol", "li", "a", "h1", "h2", "h3", "span"];

/// One stored formatted copy: where it was found (`altrep`, `x-alt-desc`, `styled`, `google`) and its HTML.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Copy {
    pub kind: String,
    pub html: String,
}

/// `html` reduced to the allowlist: no script, no style sheet, no image, no event handler, links only to
/// http(s), mailto and tel.
pub fn clean(html: &str) -> String {
    let tags: HashSet<&str> = TAGS.iter().copied().collect();
    let attrs: HashMap<&str, HashSet<&str>> = HashMap::from([("a", HashSet::from(["href"])), ("span", HashSet::from(["style"]))]);
    ammonia::Builder::new()
        .tags(tags)
        .tag_attributes(attrs)
        .generic_attributes(HashSet::new())
        .url_schemes(HashSet::from(["http", "https", "mailto", "tel"]))
        .url_relative(ammonia::UrlRelative::Deny)
        .link_rel(None)
        .filter_style_properties(HashSet::from(["color", "background-color"]))
        .strip_comments(true)
        .clean(html)
        .to_string()
        .trim()
        .to_string()
}

/// Google's subset (b, i, u, lists, links, line breaks): a block ends in `<br>`, a heading turns bold, the rest
/// of the formatting goes (Google drops it anyway). One line: Google turns a raw line break into one more.
fn google_html(html: &str) -> String {
    let mut s = clean(html);
    for (a, b) in [("<h1>", "<b>"), ("<h2>", "<b>"), ("<h3>", "<b>"), ("</h1>", "</b><br>"), ("</h2>", "</b><br>"), ("</h3>", "</b><br>"), ("</p>", "<br>"), ("</div>", "<br>")] {
        s = s.replace(a, b);
    }
    let tags: HashSet<&str> = HashSet::from(["b", "i", "u", "br", "ul", "ol", "li", "a"]);
    let s = ammonia::Builder::new().tags(tags).tag_attributes(HashMap::from([("a", HashSet::from(["href"]))])).generic_attributes(HashSet::new()).link_rel(None).url_schemes(HashSet::from(["http", "https", "mailto", "tel"])).url_relative(ammonia::UrlRelative::Deny).clean(&s).to_string();
    let mut s = s.replace(['\r', '\n'], " ");
    while s.ends_with("<br>") {
        s.truncate(s.len() - 4);
    }
    s
}

/// RFC 3986 percent-encoding as JavaScript's `encodeURIComponent` (what Thunderbird writes and decodes): every
/// byte but `A-Za-z0-9-_.!~*'()` — so no DQUOTE, `;`, `,` or `:` is left to break the quoted parameter.
fn encode_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3 / 2);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn decode_percent(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let h = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(h, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// The HTML of a `data:` URI with a text/html media type (`data:text/html,…`, also `;charset=utf-8` and
/// `;base64`, which Thunderbird itself does not read), else None.
fn data_html(uri: &str) -> Option<String> {
    let rest = uri.trim().strip_prefix("data:").or_else(|| uri.trim().strip_prefix("DATA:"))?;
    let (meta, body) = rest.split_once(',')?;
    let mut parts = meta.split(';').map(|p| p.trim().to_ascii_lowercase());
    if parts.next()? != "text/html" {
        return None;
    }
    if meta.to_ascii_lowercase().split(';').any(|p| p.trim() == "base64") {
        use base64ct::{Base64, Encoding};
        let raw = Base64::decode_vec(&decode_percent(body)?).ok()?;
        return String::from_utf8(raw).ok();
    }
    decode_percent(body)
}

/// The inside of `<body>` of a whole HTML document (Outlook writes one), else the text itself.
fn body_of(html: &str) -> &str {
    let low = html.to_ascii_lowercase();
    let Some(b) = low.find("<body") else { return html };
    let Some(gt) = low[b..].find('>') else { return html };
    let start = b + gt + 1;
    let end = low[start..].find("</body").map(|e| start + e).unwrap_or(html.len());
    &html[start..end]
}

/// Google keeps its HTML in DESCRIPTION: the flag Thunderbird adds, or a Google UID.
pub fn is_google(props: &[String], uid: &str) -> bool {
    uid.trim().to_ascii_lowercase().ends_with("@google.com") || props.iter().any(|x| prop_name(x) == "X-MOZ-GOOGLE-HTML-DESCRIPTION")
}

/// Markup in a DESCRIPTION, as Evolution tells it (`ecepp_description_contains_html`).
fn looks_html(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    ["<html", "<body", "<br", "<span", "<b>", "<i>", "<u>", "&nbsp;", "<ul", "<li", "</a>", "<p>", "<div"].iter().any(|t| l.contains(t))
}

/// Every formatted copy of the note in the master's (unfolded) lines, sanitised, in precedence order.
pub fn stored(props: &[String], uid: &str) -> Vec<Copy> {
    let mut out = vec![];
    let mut push = |kind: &str, html: String| {
        let h = clean(&html);
        if !h.is_empty() {
            out.push(Copy { kind: kind.into(), html: h });
        }
    };
    let desc = props.iter().find(|x| prop_name(x) == "DESCRIPTION").map(|x| ical::split_line(x));
    if let Some((_, params, _)) = &desc
        && let Some((_, v)) = params.iter().find(|(k, _)| k == "ALTREP")
        && let Some(h) = data_html(v)
    {
        push("altrep", h);
    }
    for x in props {
        let (name, params, value) = ical::split_line(x);
        let fmt = params.iter().find(|(k, _)| k == "FMTTYPE").map(|(_, v)| v.to_ascii_lowercase());
        if name == "X-ALT-DESC" && fmt.as_deref() == Some("text/html") {
            push("x-alt-desc", body_of(&ical::unescape(&value)).to_string());
        } else if name == "STYLED-DESCRIPTION" && fmt.as_deref().is_none_or(|f| f == "text/html") && params.iter().all(|(k, v)| k != "VALUE" || v == "TEXT") {
            push("styled", ical::unescape(&value));
        }
    }
    if let Some((_, _, v)) = desc
        && is_google(props, uid)
    {
        let t = ical::unescape(&v);
        if looks_html(&t) {
            push("google", t.replace("\r\n", "\n").replace('\n', "<br>"));
        }
    }
    out
}

/// The note's lines for a written master: `plain` (DESCRIPTION, what every app shows) and `html` (the formatted
/// note, '' without formatting); `google`: Google's own way.
pub fn lines(plain: &str, html: &str, google: bool) -> Vec<String> {
    let plain = plain.trim();
    let html = clean(html);
    if html.is_empty() {
        return if plain.is_empty() { vec![] } else { vec![format!("DESCRIPTION:{}", ical::escape(plain))] };
    }
    if google {
        return vec![format!("DESCRIPTION:{}", ical::escape(&google_html(&html)))];
    }
    let one = html.replace(['\r', '\n'], " ");
    vec![
        format!("DESCRIPTION;ALTREP=\"data:text/html,{}\":{}", encode_component(&one), ical::escape(plain)),
        format!("X-ALT-DESC;FMTTYPE=text/html:{}", ical::escape(&format!("<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 3.2//EN\"><HTML><BODY>{one}</BODY></HTML>"))),
        format!("STYLED-DESCRIPTION;VALUE=TEXT;FMTTYPE=text/html:{}", ical::escape(&one)),
    ]
}

/// The stored DESCRIPTION's text (without its parameters).
pub fn stored_plain(props: &[String]) -> String {
    props.iter().find(|x| prop_name(x) == "DESCRIPTION").map(|x| ical::unescape(ical::split_line(x).2.trim())).unwrap_or_default()
}

/// The note's part of a save, shared by events and tasks: the lines to write and whether the stored copies
/// (COPIES, ALTREP) stay. `html` None = the note was not touched: if its text is the stored one, the stored
/// DESCRIPTION line goes back byte for byte and every copy stays; else (an older client, an API caller) the
/// text is written plain and the copies go, being stale now.
pub fn save(props: Option<&[String]>, uid: &str, plain: &str, html: Option<&str>) -> (Vec<String>, bool) {
    let props = props.unwrap_or(&[]);
    if html.is_none() && stored_plain(props).trim() == plain.trim() {
        let line = props.iter().find(|x| prop_name(x) == "DESCRIPTION").cloned();
        return (line.into_iter().collect(), true);
    }
    (lines(plain, html.unwrap_or(""), is_google(props, uid)), false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_keeps_the_allowlist_only() {
        let h = clean(r#"<p onclick="x()">a <b>b</b> <script>bad()</script><img src="http://x/y.png"><a href="javascript:alert(1)">j</a> <a href="https://x.de" target="_blank">l</a> <span style="color: red; position: fixed">r</span></p>"#);
        assert_eq!(h, r#"<p>a <b>b</b> <a>j</a> <a href="https://x.de">l</a> <span style="color:red">r</span></p>"#);
    }

    #[test]
    fn our_write_is_read_back_by_each_app_way() {
        let l = lines("Hallo\n- eins", "<p>Hallo</p><ul><li><b>eins</b>, \"zwei\"; drei</li></ul>", false);
        assert_eq!(l.len(), 3);
        assert!(l[0].starts_with("DESCRIPTION;ALTREP=\"data:text/html,%3Cp%3EHallo"), "{}", l[0]);
        let enc = &l[0]["DESCRIPTION;ALTREP=\"data:text/html,".len()..l[0].find("\":").unwrap()];
        assert!(!enc.contains(['"', ';', ',', ':']), "the encoded HTML holds no DQUOTE/;/,/: — {enc}");
        assert!(l[0].ends_with(":Hallo\\n- eins"));
        assert!(l[1].starts_with("X-ALT-DESC;FMTTYPE=text/html:<!DOCTYPE HTML"));
        let c = stored(&l, "u1");
        let kinds: Vec<&str> = c.iter().map(|x| x.kind.as_str()).collect();
        assert_eq!(kinds, ["altrep", "x-alt-desc", "styled"]);
        assert!(c.iter().all(|x| x.html == "<p>Hallo</p><ul><li><b>eins</b>, \"zwei\"; drei</li></ul>"), "{c:?}");
    }

    #[test]
    fn no_formatting_writes_plain_only() {
        assert_eq!(lines("x", "", false), ["DESCRIPTION:x"]);
        assert_eq!(lines("", "", false), Vec::<String>::new());
    }

    #[test]
    fn google_gets_its_html_in_description() {
        let l = lines("T", "<h2>T</h2><p>a <span style=\"color:red\">b</span></p>", true);
        assert_eq!(l, ["DESCRIPTION:<b>T</b><br>a b"]);
        assert!(is_google(&[], "abc@google.com"));
        let c = stored(&["DESCRIPTION:<b>x</b>\\ny".into()], "abc@google.com");
        assert_eq!(c, [Copy { kind: "google".into(), html: "<b>x</b><br>y".into() }]);
        assert!(stored(&["DESCRIPTION:<b>x</b>".into()], "abc@example.org").is_empty(), "not Google: plain text that holds a tag");
    }

    #[test]
    fn data_uris_thunderbird_and_others() {
        assert_eq!(data_html("data:text/html,%3Cb%3Ex%3C%2Fb%3E").as_deref(), Some("<b>x</b>"));
        assert_eq!(data_html("data:text/html;charset=utf-8;base64,PGI+eDwvYj4=").as_deref(), Some("<b>x</b>"));
        assert_eq!(data_html("data:text/plain,x"), None);
        assert_eq!(data_html("cid:abc"), None);
    }

    #[test]
    fn outlook_document_gives_its_body() {
        let x = "X-ALT-DESC;FMTTYPE=text/html:<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 3.2//EN\">\\n<HTML><HEAD><TITLE></TITLE></HEAD><BODY><P><B>Agenda</B></P></BODY></HTML>";
        assert_eq!(stored(&[x.into()], "u"), [Copy { kind: "x-alt-desc".into(), html: "<p><b>Agenda</b></p>".into() }]);
    }

    #[test]
    fn untouched_note_keeps_its_lines_a_changed_one_drops_the_copies() {
        let p: Vec<String> = vec!["DESCRIPTION;ALTREP=\"data:text/html,%3Cb%3Ex%3C%2Fb%3E\":x".into(), "X-ALT-DESC;FMTTYPE=text/html:<b>x</b>".into()];
        assert_eq!(save(Some(&p), "u", "x", None), (vec![p[0].clone()], true));
        assert_eq!(save(Some(&p), "u", "y", None), (vec!["DESCRIPTION:y".to_string()], false));
        assert_eq!(save(Some(&p), "u", "x", Some("")), (vec!["DESCRIPTION:x".to_string()], false));
        assert_eq!(save(None, "u", "", None), (vec![], true));
    }
}
