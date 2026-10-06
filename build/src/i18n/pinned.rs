//! Every message the code and the pages ask for exists, and every language fills the same
//! placeholders. Keys are collected from the sources as literals passed to a translation call;
//! keys built at run time (`format!("mon-{m}")`) are listed by hand below.

use super::*;
use std::collections::{BTreeMap, BTreeSet};

/// message id → its text (continuation lines joined), per .ftl source
fn messages(ftl: &str) -> BTreeMap<String, String> {
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    let mut cur: Option<String> = None;
    for l in ftl.lines() {
        if l.starts_with('#') || l.trim().is_empty() {
            continue;
        }
        if l.starts_with(' ') {
            if let Some(k) = &cur {
                let v = out.get_mut(k).unwrap();
                v.push('\n');
                v.push_str(l.trim());
            }
            continue;
        }
        if let Some((k, v)) = l.split_once(" =").filter(|(k, _)| !k.contains(' ')) {
            out.insert(k.to_string(), v.trim().to_string());
            cur = Some(k.to_string());
        }
    }
    out
}

fn placeholders(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (i, _) in text.match_indices('$') {
        let name: String = text[i + 1..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').collect();
        if !name.is_empty() {
            out.insert(format!("${name}"));
        }
    }
    // app.js / history.js placeholders
    for (i, _) in text.match_indices('%') {
        let name: String = text[i + 1..].chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
        if !name.is_empty() {
            out.insert(format!("%{name}"));
        }
    }
    out
}

fn files(dir: &str, ext: &str) -> Vec<(String, String)> {
    let mut out = vec![];
    let mut stack = vec![std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(dir)];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == ext) && !p.components().any(|c| c.as_os_str() == "pinned.rs") {
                out.push((p.display().to_string(), std::fs::read_to_string(&p).unwrap()));
            }
        }
    }
    out
}

/// The string literal right after each occurrence of `call` (`call` ends with the opening quote),
/// when it is the whole argument (`tr('js-x' + n)` builds its key: skipped).
fn literals_after(text: &str, call: &str) -> Vec<String> {
    text.match_indices(call).filter_map(|(i, _)| {
        let rest = &text[i + call.len()..];
        let end = rest.find(['"', '\'', '`'])?;
        rest[end + 1..].trim_start().starts_with([')', ',']).then(|| rest[..end].to_string())
    }).filter(|k| !k.is_empty() && k.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') && !k.ends_with('-')).collect()
}

fn used_keys() -> BTreeMap<String, String> {
    let mut used = BTreeMap::new();
    let mut add = |k: String, at: &str| {
        used.entry(k).or_insert_with(|| at.to_string());
    };
    for (path, text) in files("templates", "html").into_iter().chain(files("src", "rs")) {
        for call in ["ctx.t(\"", "ctx.t1(\"", "ctx.tp(\"", "ctx.tn(\"", "l.t(\"", "l.ta(\"", "l.tn(\"", "::bad(\"", "::bad1(\"", "::conflict(\"", "Msg { key: \"", ".t(&ctx.lang, \"", ".t(lang, \"", "i18n.t(&ctx.lang, \"", "i18n.t_args(&self.ctx.lang, \"", "(self.t)(\"", "self.ctx.t(\"", "self.ctx.tn(\"", "self.ctx.t1(\""] {
            for k in literals_after(&text, call) {
                add(k, &path);
            }
        }
        // flash keys handed to `finish` as Some("…")
        for k in literals_after(&text, "Some(\"flash-") {
            add(format!("flash-{k}"), &path);
        }
    }
    for (path, text) in files("assets", "js") {
        if path.ends_with("app.js") {
            for call in ["tr('", "tr(\"", "tr(`"] {
                for k in literals_after(&text, call) {
                    add(k, &path);
                }
            }
        }
        if path.ends_with("history.js") {
            for call in ["S('", "S(\""] {
                for k in literals_after(&text, call) {
                    add(format!("hxj-{k}"), &path);
                }
            }
        }
    }
    // keys made at run time from a fixed set
    for m in 1..=12 {
        add(format!("mon-{m}"), "render.rs L::mon");
    }
    for d in 1..=7 {
        add(format!("dow-{d}"), "render.rs L::dow");
        add(format!("dowl-{d}"), "render.rs L::dowl");
    }
    for k in ["date-long", "date-dm", "date-fmt", "date-week", "date-week-short", "date-day-title", "set-comp-event", "set-comp-todo", "set-comp-journal", "hx-unit-w", "hx-unit-d", "hx-unit-h", "hx-unit-min", "hx-al-at-start", "hx-al-before", "hx-al-after", "hx-rr-until", "hx-a-coll", "hx-a-override"] {
        add(k.to_string(), "built at run time");
    }
    used
}

#[test]
fn every_used_key_exists_in_every_language() {
    let used = used_keys();
    assert!(used.len() > 300, "the scan found {} keys only", used.len());
    for k in ["nav-calendar", "js-cfo-apply", "hxj-today", "flash-saved", "v-more", "set-sources", "login-title"] {
        assert!(used.contains_key(k), "the scan misses {k}");
    }
    let mut missing = vec![];
    for (lang, src) in SOURCES {
        let have = messages(src);
        for (k, at) in &used {
            if !have.contains_key(k) {
                missing.push(format!("{lang}:{k} ({at})"));
            }
        }
    }
    assert_eq!(missing, Vec::<String>::new());
}

#[test]
fn placeholders_agree_across_languages() {
    let de = messages(SOURCES[0].1);
    let mut wrong = vec![];
    for (lang, src) in &SOURCES[1..] {
        for (k, text) in messages(src) {
            let (want, got) = (placeholders(&de[&k]), placeholders(&text));
            if want != got {
                wrong.push(format!("{lang}:{k} has {got:?}, de has {want:?}"));
            }
        }
    }
    assert_eq!(wrong, Vec::<String>::new());
}

#[test]
fn messages_format_without_errors() {
    // every message formats in every language with its placeholders given (a Fluent error would
    // otherwise only show up as a warning in the log)
    let i = I18n::load();
    for (lang, src) in SOURCES {
        for (k, text) in messages(src) {
            let mut args = FluentArgs::new();
            for p in placeholders(&text).into_iter().filter(|p| p.starts_with('$')) {
                args.set(p[1..].to_string(), FluentValue::from(2));
            }
            let b = i.get(lang).unwrap();
            let Some(pattern) = b.get_message(&k).and_then(|m| m.value()) else { continue };
            let mut errors = vec![];
            let _ = b.format_pattern(pattern, Some(&args), &mut errors);
            assert!(errors.is_empty(), "{lang}/{k}: {errors:?}");
        }
    }
}

#[test]
fn lookups_and_fallbacks() {
    let i = I18n::load();
    assert_eq!(i.t("xx", "nav-calendar"), "Calendar", "an unknown language reads English");
    assert_eq!(i.t("de", "no-such-key"), "⟦no-such-key⟧");
    // `lookup`: the same fallback chain, None (no placeholder) where no language has the key
    assert_eq!((i.lookup("de", "no-such-key"), i.lookup("xx", "nav-calendar").as_deref()), (None, Some("Calendar")));
    assert_eq!(i.t1("de", "v-rr-every-days", "n", 3), "Alle 3 Tage");
    let js = i.with_prefix("en", "js-");
    assert!(js.len() > 100 && js.iter().all(|(k, _)| k.starts_with("js-")));
    assert_eq!(js.len(), i.with_prefix("de", "js-").len());
    assert_eq!((locale("en"), locale("xx")), ("en-GB", "de-DE"));
    assert!(supported("fr") && !supported("it"));
    assert_eq!(negotiate(Some("ES;q=0.5, *"), "en"), "es");
    assert_eq!(negotiate(Some("*"), "en"), "en");
    assert_eq!(negotiate(Some(""), "de"), "de");
    assert_eq!(keys_of("a = 1\n  cont\n# c\nb =\n    .attr = x\nnot a key = y\n"), ["a", "b"]);
}

#[test]
fn plural_messages_reach_the_browser_as_forms() {
    let i = I18n::load();
    // a js-* plural: its forms and the name of its number; app.js picks one with Intl.PluralRules
    let js: serde_json::Value = serde_json::from_str(&i.js_json("de")).unwrap();
    assert_eq!(js["js-set-imported"], serde_json::json!({ "$": "n", "one": "%n Objekt importiert.", "other": "%n Objekte importiert." }));
    assert_eq!(js["js-imp-done"]["$"], "count");
    assert_eq!(i.with_prefix_json("fr", "hxj-").into_iter().find(|(k, _)| k == "hxj-more").unwrap().1, serde_json::json!({ "$": "n", "one": "et %n autre", "other": "et %n autres" }));
    assert!(js["js-set-err"].is_string(), "any other message stays a string");
    // a data attribute of a template: the forms with {n}
    assert_eq!(i.plural_json("de", "ct-days-in"), r#"{"one":"in {n} Tag","other":"in {n} Tagen"}"#);
    assert_eq!(i.plural_json("en", "ed-h-count"), r#"{"one":"once","other":"{n} times"}"#);
    assert_eq!(i.plural_json("de", "ed-h-count"), "{n}-mal", "a language without plural forms: the plain text");
    // every plural form the browser picks from is named by a CLDR category (what Intl.PluralRules answers),
    // "other" always there (Rust-only messages may also select on a number literal: v-rr-ordinal [1])
    let tp: Vec<String> = files("templates", "html").iter().flat_map(|(_, t)| literals_after(t, "ctx.tp(\"")).collect();
    assert!(tp.len() >= 4, "{tp:?}");
    for (lang, src) in SOURCES {
        for k in messages(src).into_keys().filter(|k| k.starts_with("js-") || k.starts_with("hxj-") || tp.contains(k)) {
            if let Some((_, forms)) = i.plural_forms(lang, &k, |v| v.to_string()) {
                assert!(forms.iter().all(|(c, _)| ["zero", "one", "two", "few", "many", "other"].contains(&c.as_str())) && forms.iter().any(|(c, _)| c == "other"), "{lang}/{k}: {forms:?}");
            }
        }
    }
}
