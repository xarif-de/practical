//! Fluent translations embedded at compile time. One bundle per language in LANGS,
//! shared read-only across requests (`concurrent::FluentBundle`). A key a language lacks
//! falls back to English, then German, so a new language can grow file by file; the
//! test below keeps every shipped file complete.

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource, FluentValue};
use std::sync::Arc;
use unic_langid::LanguageIdentifier;

/// Languages of the UI, in the order of the language choice. Adding one: a `locales/<lang>.ftl`
/// here and in SOURCES, NATIVE, LOCALE, and the region names (`tools/gen_regions.py` LANGS).
pub const LANGS: &[&str] = &["de", "en", "fr", "es"];

/// Native names for the language choice (never translated: a reader must find their own).
pub const NATIVE: &[(&str, &str)] = &[("de", "Deutsch"), ("en", "English"), ("fr", "Français"), ("es", "Español")];

/// BCP 47 locale per language for date formatting in the browser (`Intl`).
pub const LOCALE: &[(&str, &str)] = &[("de", "de-DE"), ("en", "en-GB"), ("fr", "fr-FR"), ("es", "es-ES")];

const SOURCES: &[(&str, &str)] = &[
    ("de", include_str!("../locales/de.ftl")),
    ("en", include_str!("../locales/en.ftl")),
    ("fr", include_str!("../locales/fr.ftl")),
    ("es", include_str!("../locales/es.ftl")),
];

pub struct I18n {
    bundles: Vec<(&'static str, FluentBundle<FluentResource>)>,
}

fn bundle(lang: &str, ftl: &str) -> FluentBundle<FluentResource> {
    let id: LanguageIdentifier = lang.parse().expect("language id");
    let res = FluentResource::try_new(ftl.to_string()).unwrap_or_else(|(_, errs)| {
        panic!("invalid .ftl for {lang}: {errs:?}");
    });
    let mut b = FluentBundle::new_concurrent(vec![id]);
    // Isolation marks (U+2068/2069) around placeables are for bidi text; our languages
    // are LTR and the marks show up as junk in <title>/attributes.
    b.set_use_isolating(false);
    b.add_resource(res).expect("duplicate message ids in .ftl");
    b
}

/// Message ids of one .ftl source (continuation lines and comments skipped).
fn keys_of(ftl: &str) -> Vec<String> {
    ftl.lines()
        .filter(|l| !l.starts_with(' ') && !l.starts_with('#'))
        .filter_map(|l| l.split_once(" = ").or_else(|| l.strip_suffix(" =").map(|k| (k, ""))).map(|(k, _)| k.trim().to_string()))
        .filter(|k| !k.is_empty() && !k.contains(' '))
        .collect()
}

pub fn supported(lang: &str) -> bool {
    LANGS.contains(&lang)
}

pub fn locale(lang: &str) -> &'static str {
    LOCALE.iter().find(|(l, _)| *l == lang).map(|(_, x)| *x).unwrap_or("de-DE")
}

impl I18n {
    /// One process-wide instance (the bundles never change): for texts rendered without a request, the
    /// English Display of an error message (`error::Msg`).
    pub fn shared() -> &'static Self {
        static I: std::sync::OnceLock<Arc<I18n>> = std::sync::OnceLock::new();
        I.get_or_init(Self::load)
    }

    pub fn load() -> Arc<Self> {
        Arc::new(Self { bundles: SOURCES.iter().map(|(l, src)| (*l, bundle(l, src))).collect() })
    }

    fn get(&self, lang: &str) -> Option<&FluentBundle<FluentResource>> {
        self.bundles.iter().find(|(l, _)| *l == lang).map(|(_, b)| b)
    }

    pub fn t(&self, lang: &str, key: &str) -> String {
        self.t_args(lang, key, None)
    }

    pub fn t_args(&self, lang: &str, key: &str, args: Option<&FluentArgs>) -> String {
        self.lookup_args(lang, key, args).unwrap_or_else(|| {
            tracing::warn!("missing translation {lang}/{key}");
            format!("⟦{key}⟧")
        })
    }

    /// The message in the asked language, then English, then German; None (and no warning) where no
    /// language has it — for keys built from data (`hx-a-<property>`), whose caller has a fallback.
    pub fn lookup(&self, lang: &str, key: &str) -> Option<String> {
        self.lookup_args(lang, key, None)
    }

    fn lookup_args(&self, lang: &str, key: &str, args: Option<&FluentArgs>) -> Option<String> {
        for l in [lang, "en", "de"] {
            let Some(b) = self.get(l) else { continue };
            let Some(pattern) = b.get_message(key).and_then(|m| m.value()) else { continue };
            if l != lang {
                tracing::debug!("translation {lang}/{key} missing, using {l}");
            }
            let mut errors = vec![];
            let s = b.format_pattern(pattern, args, &mut errors).into_owned();
            if !errors.is_empty() {
                tracing::warn!("translation {l}/{key}: {errors:?}");
            }
            return Some(s);
        }
        None
    }

    /// Message with one named argument.
    pub fn t1(&self, lang: &str, key: &str, name: &str, value: impl Into<FluentValue<'static>>) -> String {
        let mut args = FluentArgs::new();
        args.set(name.to_string(), value.into());
        self.t_args(lang, key, Some(&args))
    }

    /// Every message whose id starts with `prefix`, formatted in `lang` — the strings app.js
    /// needs (`js-*`; build.rs `i18n_js` ships them as /assets/i18n-<lang>.js). Placeholders there are `%name`.
    pub fn with_prefix(&self, lang: &str, prefix: &str) -> Vec<(String, String)> {
        keys_of(SOURCES[0].1)
            .into_iter()
            .filter(|k| k.starts_with(prefix))
            .map(|k| {
                let v = self.t(lang, &k);
                (k, v)
            })
            .collect()
    }

    /// The same for the browser: a plural message (`plural_forms`) as an object of its forms plus `"$"`, the
    /// name of its number (app.js `tr` picks the form with Intl.PluralRules); every other one as a string.
    pub fn with_prefix_json(&self, lang: &str, prefix: &str) -> Vec<(String, serde_json::Value)> {
        self.with_prefix(lang, prefix).into_iter().map(|(k, v)| {
            let v = match self.plural_forms(lang, &k, |var| format!("%{var}")) {
                Some((var, forms)) => {
                    let mut o: serde_json::Map<String, serde_json::Value> = forms.into_iter().map(|(c, s)| (c, serde_json::Value::String(s))).collect();
                    o.insert("$".into(), var.into());
                    serde_json::Value::Object(o)
                }
                None => serde_json::Value::String(v),
            };
            (k, v)
        }).collect()
    }

    /// The forms of a plural message — one whose whole pattern selects on a number (`{ $n -> [one] … *[other] … }`):
    /// (the number's name, [(CLDR category or number, the form with `placeholder(name)` for the number)]). None for
    /// any other message. The browser picks the form (Intl.PluralRules of the page locale, the same CLDR rules).
    pub fn plural_forms(&self, lang: &str, key: &str, placeholder: impl Fn(&str) -> String) -> Option<(String, Vec<(String, String)>)> {
        use fluent_syntax::ast::{Expression, InlineExpression, PatternElement, VariantKey};
        let (b, pattern) = [lang, "en", "de"].into_iter().find_map(|l| Some((self.get(l)?, self.get(l)?.get_message(key)?.value()?)))?;
        let [PatternElement::Placeable { expression: Expression::Select { selector: InlineExpression::VariableReference { id }, variants } }] = pattern.elements.as_slice() else { return None };
        let mut args = FluentArgs::new();
        args.set(id.name.to_string(), FluentValue::from(placeholder(id.name)));
        let forms = variants.iter().map(|v| {
            let name = match &v.key { VariantKey::Identifier { name } | VariantKey::NumberLiteral { value: name } => name.to_string() };
            let mut errors = vec![];
            (name, b.format_pattern(&v.value, Some(&args), &mut errors).into_owned())
        }).collect();
        Some((id.name.to_string(), forms))
    }

    /// A plural message for a data attribute of a template: its forms as a JSON object, the number as `{n}`
    /// (app.js `plural`); any other message as it reads with `{n}`.
    pub fn plural_json(&self, lang: &str, key: &str) -> String {
        match self.plural_forms(lang, key, |var| format!("{{{var}}}")) {
            Some((_, forms)) => serde_json::Value::Object(forms.into_iter().map(|(c, s)| (c, serde_json::Value::String(s))).collect()).to_string(),
            None => self.t1(lang, key, "n", "{n}".to_string()),
        }
    }

    /// The js-* strings of `lang` as one JSON object: what build.rs (`#[path]`, this very code) writes
    /// into /assets/i18n-<lang>.js (test `i18n_assets_are_the_runtime_strings`).
    #[allow(dead_code)] // called by build.rs and the tests, not by the binary
    pub fn js_json(&self, lang: &str) -> serde_json::Value {
        serde_json::Value::Object(self.with_prefix_json(lang, "js-").into_iter().collect())
    }

    /// Message ids missing in a language (or extra there), compared with German.
    #[cfg(test)]
    pub fn missing_keys(&self) -> Vec<String> {
        let de: std::collections::BTreeSet<String> = keys_of(SOURCES[0].1).into_iter().collect();
        let mut out = vec![];
        for (l, src) in &SOURCES[1..] {
            let other: std::collections::BTreeSet<String> = keys_of(src).into_iter().collect();
            out.extend(de.symmetric_difference(&other).map(|k| format!("{l}:{k}")));
        }
        out
    }
}

/// Pick a supported language from an Accept-Language header value.
pub fn negotiate(accept_language: Option<&str>, default: &str) -> String {
    if let Some(al) = accept_language {
        for part in al.split(',') {
            let tag = part.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
            let primary = tag.split('-').next().unwrap_or("");
            if supported(primary) {
                return primary.to_string();
            }
        }
    }
    if supported(default) { default.to_string() } else { "de".to_string() }
}

#[cfg(test)]
mod pinned;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundles_load_and_are_symmetric() {
        let i = I18n::load();
        assert_eq!(i.missing_keys(), Vec::<String>::new());
        assert_eq!(i.t("de", "nav-calendar"), "Kalender");
        assert_eq!(i.t("en", "nav-calendar"), "Calendar");
    }

    #[test]
    fn every_language_is_wired() {
        for l in LANGS {
            assert!(SOURCES.iter().any(|(s, _)| s == l), "{l} has no .ftl");
            assert!(NATIVE.iter().any(|(s, _)| s == l), "{l} has no native name");
            assert!(LOCALE.iter().any(|(s, _)| s == l), "{l} has no locale");
        }
    }

    #[test]
    fn negotiation() {
        assert_eq!(negotiate(Some("en-US,en;q=0.9,de;q=0.8"), "de"), "en");
        assert_eq!(negotiate(Some("fr-FR,fr;q=0.9"), "de"), "fr");
        assert_eq!(negotiate(Some("pt-BR,pt;q=0.9"), "de"), "de");
        assert_eq!(negotiate(None, "en"), "en");
        assert_eq!(negotiate(None, "xx"), "de");
    }
}
