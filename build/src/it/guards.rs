//! Source guards: every access to a collection passes `state::Collections` (and so its `ensure`), and nothing
//! reads RustiCal's object or share tables behind it. Both scan `src/**` as text; the names they look for are
//! spelled in pieces here, so this file does not trip them itself.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            sources(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// `(path relative to src/, text)` of every Rust source
fn files() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut paths = vec![];
    sources(&root, &mut paths);
    paths.sort();
    paths.into_iter().map(|p| (p.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/"), std::fs::read_to_string(&p).unwrap())).collect()
}

/// A source split into its code (comments dropped, string literals emptied) and its string literals
fn lex(src: &str) -> (String, Vec<String>) {
    let b = src.as_bytes();
    let (mut code, mut strings, mut i) = (String::new(), vec![], 0);
    let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    while i < b.len() {
        let c = b[i];
        if b[i..].starts_with(b"//") {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
        } else if b[i..].starts_with(b"/*") {
            let end = src[i + 2..].find("*/").map_or(b.len(), |n| i + 2 + n + 2);
            i = end;
        } else if c == b'r' && (i == 0 || !ident(b[i - 1])) && matches!(b.get(i + 1), Some(b'#' | b'"')) {
            let mut j = i + 1;
            while b.get(j) == Some(&b'#') {
                j += 1;
            }
            if b.get(j) != Some(&b'"') {
                code.push('r');
                i += 1;
                continue;
            }
            let close = format!("\"{}", "#".repeat(j - i - 1));
            let end = src[j + 1..].find(&close).map_or(b.len(), |n| j + 1 + n);
            strings.push(src[j + 1..end].to_string());
            code.push_str("\"\"");
            i = end + close.len();
        } else if c == b'"' {
            let mut j = i + 1;
            while j < b.len() && b[j] != b'"' {
                j += if b[j] == b'\\' { 2 } else { 1 };
            }
            strings.push(src[i + 1..j.min(b.len())].to_string());
            code.push_str("\"\"");
            i = j + 1;
        } else if c == b'\'' && b.get(i + 1) == Some(&b'\\') {
            // an escaped char literal ('\'', '\\', '\n', '\u{…}')
            let end = src[i + 2..].find('\'').map_or(b.len(), |n| i + 2 + n + 1);
            code.push_str("' '");
            i = end;
        } else if c == b'\'' && b.get(i + 2) == Some(&b'\'') {
            code.push_str("' '");
            i += 3;
        } else {
            let ch = src[i..].chars().next().unwrap();
            code.push(ch);
            i += ch.len_utf8();
        }
    }
    (code, strings)
}

fn count(hay: &str, needle: &str) -> usize {
    hay.match_indices(needle).count()
}

/// Whether `code` holds `needle` as a name of its own (not a part of a longer identifier after it)
fn has_name(code: &str, needle: &str) -> bool {
    code.match_indices(needle).any(|(i, _)| !code[i + needle.len()..].starts_with(|c: char| c.is_alphanumeric() || c == '_'))
}

#[test]
fn the_lexer_tells_code_comments_and_strings_apart() {
    let (code, strings) = lex("let a = \"x // y\"; // not code \"z\"\nlet b = r#\"q\"#; let c = '\"'; let d = '\\''; fn f<'a>() {}");
    assert_eq!(strings, ["x // y", "q"]);
    assert!(!code.contains("not code") && code.contains("fn f<'a>()"), "{code}");
}

#[test]
fn collections_are_reached_only_through_the_state() {
    let store = |a: &str, b: &str| format!("Sqlite{a}{b}");
    let crate_path = |m: &str| format!("rustical_store::{m}");
    let forbidden = [store("Calendar", "Store"), store("Addressbook", "Store"), store("Share", "Store"), crate_path("access"), crate_path("shared")];
    let field = format!(".{}", "shares");
    let mut found = vec![];
    for (path, src) in files() {
        if matches!(path.as_str(), "state.rs" | "main.rs" | "testkit.rs") {
            continue;
        }
        let (code, _) = lex(&src);
        for f in &forbidden {
            if has_name(&code, f) {
                found.push(format!("{path}: {f}"));
            }
        }
        if has_name(&code, &field) {
            found.push(format!("{path}: {field}"));
        }
        // `use rustical_store::{…, access, …}` (and `shared`), however it is spread over lines
        for (i, _) in code.match_indices(&crate_path("{")) {
            let mut depth = 0;
            let body: String = code[i + crate_path("").len()..]
                .chars()
                .take_while(|c| {
                    match c {
                        '{' => depth += 1,
                        '}' => depth -= 1,
                        _ => {}
                    }
                    depth > 0
                })
                .collect();
            for m in ["access", "shared"] {
                if body.split(|c: char| !(c.is_alphanumeric() || c == '_')).any(|w| w == m) {
                    found.push(format!("{path}: {{… {m} …}}"));
                }
            }
        }
    }
    assert!(found.is_empty(), "outside state.rs, main.rs and testkit.rs: {found:#?}");
}

#[test]
fn no_raw_sql_on_rusticals_object_and_share_tables() {
    let objects = [format!("calendar{}", "objects"), format!("address{}", "objects"), format!("calendarobject{}", "changelog"), format!("addressobject{}", "changelog")];
    let shares = [format!("calendar_{}", "shares"), format!("addressbook_{}", "shares"), format!("principal_{}", "visibility"), format!("principal_{}", "emails")];
    // read-only, each binding the collection's owner and id (`Authorized::owner()/id()`): the search index, the
    // history's ingest, the contacts' card reads
    let allowed: BTreeMap<&str, usize> = [("domain/search/index.rs", 1), ("domain/history/ingest.rs", 6), ("domain/contacts/vcard.rs", 3)].into();
    let (mut got, mut share_hits) = (BTreeMap::new(), vec![]);
    for (path, src) in files() {
        if path.starts_with("it/") || path.ends_with("pinned.rs") || path == "testkit.rs" {
            continue;
        }
        let (_, strings) = lex(&src);
        // `calendarobjects` is no part of `calendarobjectchangelog`: the four names never overlap
        let n: usize = strings.iter().map(|s| objects.iter().map(|t| count(s, t)).sum::<usize>()).sum();
        if n > 0 {
            got.insert(path.clone(), n);
        }
        for s in &strings {
            for t in &shares {
                if s.contains(t.as_str()) {
                    share_hits.push(format!("{path}: {t}"));
                }
            }
        }
    }
    let got: BTreeMap<&str, usize> = got.iter().map(|(k, v)| (k.as_str(), *v)).collect();
    assert_eq!(got, allowed, "SQL on RustiCal's object tables, by file");
    assert!(share_hits.is_empty(), "the share tables are written only through the ShareStore: {share_hits:#?}");
}
